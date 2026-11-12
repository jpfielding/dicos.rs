//! Background volume loading.
//!
//! Loading a DICOS file or directory is CPU-heavy: parse + codec decode, threat
//! sidecar merging, GPU packing ([`Volume::pack_for_gpu`](crate::volume::Volume::pack_for_gpu)),
//! and center-of-mass computation. Running that on the render thread freezes the
//! window. [`VolumeLoader`] moves the *entire* CPU pipeline onto a worker thread
//! so the render thread only performs wgpu resource operations on the result.
//!
//! Each [`LoadedLayer`] arrives GPU-ready: it already carries the packed
//! `RGBA16Unorm` buffer and the density-weighted center of mass, so installing
//! it costs nothing beyond `queue.write_texture`.

use crate::app::merge_unique_threats;
use crate::volume::{self, Volume};

use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender};

/// Where a load request gets its data from.
pub(crate) enum LoadSource {
    /// A file or directory on disk (native only).
    #[cfg(not(target_arch = "wasm32"))]
    Path(PathBuf),
    /// In-memory `(file name, bytes)` pairs, e.g. from a browser file picker
    /// or `fetch`. The largest file is the CT volume; the rest are offered as
    /// threat sidecars. `name` is used for display only.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Bytes {
        name: String,
        files: Vec<(String, Vec<u8>)>,
    },
}

impl LoadSource {
    /// The path used for display and for the [`LoadOutcome`].
    fn display_path(&self) -> PathBuf {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Path(path) => path.clone(),
            Self::Bytes { name, .. } => PathBuf::from(name),
        }
    }
}

/// Index of the largest file by byte length: the CT volume among a multi-file
/// selection. `None` when `files` is empty.
pub(crate) fn largest_file(files: &[(String, Vec<u8>)]) -> Option<usize> {
    files
        .iter()
        .enumerate()
        .max_by_key(|(_, (_, bytes))| bytes.len())
        .map(|(i, _)| i)
}

/// A single named volume layer, loaded and packed off the render thread.
pub(crate) struct LoadedLayer {
    /// Display name (filename stem).
    pub(crate) name: String,
    /// The decoded volume data.
    pub(crate) volume: Volume,
    /// GPU-ready packed buffer (`[density, grad_x, grad_y, grad_z]` per voxel).
    pub(crate) packed: Vec<u16>,
    /// Density-weighted center of mass in normalized `[0,1]` coordinates.
    pub(crate) center_of_mass: [f32; 3],
}

/// The result of a background load request.
pub(crate) enum LoadOutcome {
    /// One or more layers loaded successfully.
    Loaded {
        path: PathBuf,
        layers: Vec<LoadedLayer>,
    },
    /// Loading failed; `error` is a user-facing message.
    Failed { path: PathBuf, error: String },
}

/// Owns a background worker channel for asynchronous volume loading.
///
/// Only one load runs at a time. [`request`](Self::request) is a no-op while a
/// load is in flight (the UI disables the Open buttons during loading anyway),
/// and [`poll`](Self::poll) drains completed results without blocking.
pub(crate) struct VolumeLoader {
    #[cfg(not(target_arch = "wasm32"))]
    tx: Sender<LoadOutcome>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: Receiver<LoadOutcome>,
    /// wasm: the deferred request and whether a frame has been drawn since it
    /// was queued (so the "Loading..." state renders before parsing blocks).
    #[cfg(target_arch = "wasm32")]
    pending: Option<(LoadSource, bool)>,
    in_flight: Option<PathBuf>,
}

impl VolumeLoader {
    pub(crate) fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            tx,
            #[cfg(not(target_arch = "wasm32"))]
            rx,
            #[cfg(target_arch = "wasm32")]
            pending: None,
            in_flight: None,
        }
    }

    /// Spawn a worker to load `source`. Ignored if a load is already in flight.
    ///
    /// The [`JoinHandle`](std::thread::JoinHandle) is intentionally dropped
    /// (detached): the worker owns everything it needs and communicates only
    /// through the channel. If the app exits and drops the [`Receiver`], the
    /// worker's `send` simply returns `Err` and the thread exits — nothing is
    /// leaked and no panic occurs.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request(&mut self, source: LoadSource) {
        if self.in_flight.is_some() {
            return;
        }
        self.in_flight = Some(source.display_path());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let outcome = load_outcome(source);
            // A dropped receiver (app shutting down) yields Err; ignore it.
            let _ = tx.send(outcome);
        });
    }

    /// Queue `source` to load on the main thread. The work runs from
    /// [`poll`](Self::poll) one frame later so the loading state is drawn
    /// first. Ignored if a load is already in flight.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn request(&mut self, source: LoadSource) {
        if self.in_flight.is_some() {
            return;
        }
        self.in_flight = Some(source.display_path());
        self.pending = Some((source, false));
    }

    /// Return a completed load result if one is ready, clearing the in-flight
    /// marker. Non-blocking on native.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn poll(&mut self) -> Option<LoadOutcome> {
        match self.rx.try_recv() {
            Ok(outcome) => {
                self.in_flight = None;
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// Run the queued request, but only on the frame after it was queued.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn poll(&mut self) -> Option<LoadOutcome> {
        let (source, frame_drawn) = self.pending.take()?;
        if !frame_drawn {
            self.pending = Some((source, true));
            return None;
        }
        self.in_flight = None;
        Some(load_outcome(source))
    }

    /// The path currently being loaded, if any.
    pub(crate) fn loading(&self) -> Option<&Path> {
        self.in_flight.as_deref()
    }
}

/// Run the full CPU pipeline for `source` and wrap the result in a [`LoadOutcome`].
fn load_outcome(source: LoadSource) -> LoadOutcome {
    let path = source.display_path();
    let result = match source {
        #[cfg(not(target_arch = "wasm32"))]
        LoadSource::Path(path) => {
            if path.is_dir() {
                load_directory(&path)
            } else {
                load_single_file(&path)
            }
        }
        LoadSource::Bytes { files, .. } => load_bytes(&files),
    };
    match result {
        Ok(layers) => LoadOutcome::Loaded { path, layers },
        Err(error) => LoadOutcome::Failed { path, error },
    }
}

/// Load one volume layer from in-memory files: the largest is the CT, the rest
/// are searched for threat sidecars.
fn load_bytes(files: &[(String, Vec<u8>)]) -> Result<Vec<LoadedLayer>, String> {
    let ct_index = largest_file(files).ok_or_else(|| "No files selected".to_string())?;
    let (ct_name, ct_bytes) = &files[ct_index];
    let mut vol = volume::load_dicos_bytes(ct_bytes).map_err(|e| e.to_string())?;

    let others: Vec<(String, Vec<u8>)> = files
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != ct_index)
        .map(|(_, f)| f.clone())
        .collect();
    let sidecars =
        volume::load_threat_sidecars_from_bytes(&others, [vol.dim_x, vol.dim_y, vol.dim_z]);
    let added = merge_unique_threats(&mut vol.threats, sidecars);
    if added > 0 {
        log::info!("Loaded {added} threat box(es) from sidecar files");
    }

    let name = Path::new(ct_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    log::info!("Loaded 1 volume from {ct_name}");
    Ok(vec![finish_layer(name, vol)])
}

/// Pack and compute the center of mass for a volume, producing a GPU-ready layer.
fn finish_layer(name: String, volume: Volume) -> LoadedLayer {
    let packed = volume.pack_for_gpu();
    let center_of_mass = volume.center_of_mass();
    LoadedLayer {
        name,
        volume,
        packed,
        center_of_mass,
    }
}

/// Load a single DICOS file as one volume layer, merging threat sidecars from
/// the file's directory.
#[cfg(not(target_arch = "wasm32"))]
fn load_single_file(path: &Path) -> Result<Vec<LoadedLayer>, String> {
    let mut vol = volume::load_dicos_path(path).map_err(|e| e.to_string())?;

    if let Some(dir) = path.parent() {
        let sidecars =
            volume::load_threat_sidecars_from_dir(dir, [vol.dim_x, vol.dim_y, vol.dim_z]);
        let added = merge_unique_threats(&mut vol.threats, sidecars);
        if added > 0 {
            log::info!("Loaded {added} threat box(es) from sidecar files");
        }
    }

    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    log::info!("Loaded 1 volume from {}", path.display());
    Ok(vec![finish_layer(name, vol)])
}

/// Load a directory of DICOS files as separate volume layers.
///
/// Each `.dcs`/`.dcm` file becomes its own named layer (matching the Go viewer,
/// which keeps volumes separate rather than stacking). Threat sidecars are
/// merged into the layers whose dimensions match the largest volume.
#[cfg(not(target_arch = "wasm32"))]
fn load_directory(dir: &Path) -> Result<Vec<LoadedLayer>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if ext == "dcs" || ext == "dcm" {
                    return Some(path);
                }
            }
            None
        })
        .collect();

    files.sort();

    if files.is_empty() {
        return Err("No .dcs or .dcm files found".to_string());
    }

    log::info!("Loading {} DICOS files from {}", files.len(), dir.display());

    let mut volumes: Vec<(String, Volume)> = Vec::new();
    for file in &files {
        match volume::load_dicos_path(file) {
            Ok(vol) => {
                let name = file
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                log::info!(
                    "  {} -> {}x{}x{} ({})",
                    name,
                    vol.dim_x,
                    vol.dim_y,
                    vol.dim_z,
                    vol.modality
                );
                volumes.push((name, vol));
            }
            Err(e) => {
                log::warn!("Skipping {}: {e}", file.display());
            }
        }
    }

    if volumes.is_empty() {
        return Err("No readable DICOS volumes found".to_string());
    }

    if let Some((dim_x, dim_y, dim_z)) = volumes
        .iter()
        .map(|(_, vol)| (vol.dim_x, vol.dim_y, vol.dim_z))
        .max_by_key(|(x, y, z)| x.saturating_mul(*y).saturating_mul(*z))
    {
        let sidecars = volume::load_threat_sidecars_from_dir(dir, [dim_x, dim_y, dim_z]);
        for (_, vol) in volumes.iter_mut() {
            if (vol.dim_x, vol.dim_y, vol.dim_z) == (dim_x, dim_y, dim_z) {
                merge_unique_threats(&mut vol.threats, sidecars.clone());
            }
        }
    }

    log::info!(
        "Loaded {} volume layers from {}",
        volumes.len(),
        dir.display()
    );

    Ok(volumes
        .into_iter()
        .map(|(name, vol)| finish_layer(name, vol))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Poll until an outcome arrives or a generous timeout elapses.
    fn poll_blocking(loader: &mut VolumeLoader) -> LoadOutcome {
        for _ in 0..2000 {
            if let Some(outcome) = loader.poll() {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("timed out waiting for load outcome");
    }

    #[test]
    fn nonexistent_file_yields_failed_outcome() {
        let mut loader = VolumeLoader::new();
        let path = PathBuf::from("/definitely/does/not/exist.dcs");
        loader.request(LoadSource::Path(path.clone()));

        match poll_blocking(&mut loader) {
            LoadOutcome::Failed {
                path: failed_path,
                error,
            } => {
                assert_eq!(failed_path, path);
                assert!(!error.is_empty(), "error message should be populated");
            }
            LoadOutcome::Loaded { .. } => panic!("expected Failed outcome"),
        }
    }

    #[test]
    fn loading_lifecycle_clears_on_receipt() {
        let mut loader = VolumeLoader::new();
        assert!(loader.loading().is_none());

        let path = PathBuf::from("/definitely/does/not/exist.dcs");
        loader.request(LoadSource::Path(path.clone()));
        assert_eq!(loader.loading(), Some(path.as_path()));

        let _ = poll_blocking(&mut loader);
        assert!(
            loader.loading().is_none(),
            "loading() must clear once the outcome is received"
        );
    }

    #[test]
    fn second_request_while_in_flight_is_ignored() {
        let mut loader = VolumeLoader::new();
        let first = PathBuf::from("/definitely/does/not/exist_a.dcs");
        let second = PathBuf::from("/definitely/does/not/exist_b.dcs");

        loader.request(LoadSource::Path(first.clone()));
        // Do NOT poll here: in_flight stays set until a poll drains it, so the
        // second request is deterministically ignored regardless of worker
        // timing.
        loader.request(LoadSource::Path(second));
        assert_eq!(
            loader.loading(),
            Some(first.as_path()),
            "second request must not replace the in-flight one"
        );

        // Only the first request produced work; draining yields its path.
        match poll_blocking(&mut loader) {
            LoadOutcome::Failed { path, .. } => assert_eq!(path, first),
            LoadOutcome::Loaded { .. } => panic!("expected Failed outcome"),
        }
        // And there is no second queued message.
        assert!(loader.poll().is_none());
    }

    fn minimal_dicos_bytes(rows: u16, cols: u16) -> Vec<u8> {
        use dicos::types::{Dataset, Element, Value};
        use dicos::vr::Vr;
        let mut ds = Dataset::new();
        ds.put_u16(dicos::tag::ROWS, Vr::US, rows);
        ds.put_u16(dicos::tag::COLUMNS, Vr::US, cols);
        ds.put_string(dicos::tag::MODALITY, Vr::CS, "CT");
        let pixels: Vec<u8> = (0..rows as usize * cols as usize)
            .flat_map(|i| (i as u16 + 1).to_le_bytes())
            .collect();
        ds.insert(Element::new(
            dicos::tag::PIXEL_DATA,
            Vr::OW,
            Value::Bytes(pixels),
        ));
        let mut out = Vec::new();
        dicos::writer::write(&mut out, &ds).expect("write dicos bytes");
        out
    }

    #[test]
    fn bytes_source_loads_largest_file_as_ct() {
        let mut loader = VolumeLoader::new();
        let files = vec![
            ("tdr.dcs".to_string(), minimal_dicos_bytes(2, 2)),
            ("ct.dcs".to_string(), minimal_dicos_bytes(8, 8)),
        ];
        loader.request(LoadSource::Bytes {
            name: "ct.dcs".to_string(),
            files,
        });
        match poll_blocking(&mut loader) {
            LoadOutcome::Loaded { path, layers } => {
                assert_eq!(path, PathBuf::from("ct.dcs"));
                assert_eq!(layers.len(), 1);
                assert_eq!(layers[0].name, "ct");
                assert_eq!(layers[0].volume.dim_x, 8);
            }
            LoadOutcome::Failed { error, .. } => panic!("expected Loaded, got: {error}"),
        }
    }

    #[test]
    fn bytes_source_with_no_files_fails() {
        let mut loader = VolumeLoader::new();
        loader.request(LoadSource::Bytes {
            name: "none".to_string(),
            files: Vec::new(),
        });
        assert!(matches!(
            poll_blocking(&mut loader),
            LoadOutcome::Failed { .. }
        ));
    }
}
