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
use std::sync::mpsc::{Receiver, Sender};

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
    tx: Sender<LoadOutcome>,
    rx: Receiver<LoadOutcome>,
    in_flight: Option<PathBuf>,
}

impl VolumeLoader {
    pub(crate) fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tx,
            rx,
            in_flight: None,
        }
    }

    /// Spawn a worker to load `path`. Ignored if a load is already in flight.
    ///
    /// The [`JoinHandle`](std::thread::JoinHandle) is intentionally dropped
    /// (detached): the worker owns everything it needs and communicates only
    /// through the channel. If the app exits and drops the [`Receiver`], the
    /// worker's `send` simply returns `Err` and the thread exits — nothing is
    /// leaked and no panic occurs.
    pub(crate) fn request(&mut self, path: PathBuf) {
        if self.in_flight.is_some() {
            return;
        }
        self.in_flight = Some(path.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let outcome = load_outcome(path);
            // A dropped receiver (app shutting down) yields Err; ignore it.
            let _ = tx.send(outcome);
        });
    }

    /// Return a completed load result if one is ready, clearing the in-flight
    /// marker. Non-blocking.
    pub(crate) fn poll(&mut self) -> Option<LoadOutcome> {
        match self.rx.try_recv() {
            Ok(outcome) => {
                self.in_flight = None;
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// The path currently being loaded, if any.
    pub(crate) fn loading(&self) -> Option<&Path> {
        self.in_flight.as_deref()
    }
}

/// Run the full CPU pipeline for `path` and wrap the result in a [`LoadOutcome`].
fn load_outcome(path: PathBuf) -> LoadOutcome {
    let result = if path.is_dir() {
        load_directory(&path)
    } else {
        load_single_file(&path)
    };
    match result {
        Ok(layers) => LoadOutcome::Loaded { path, layers },
        Err(error) => LoadOutcome::Failed { path, error },
    }
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
        loader.request(path.clone());

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
        loader.request(path.clone());
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

        loader.request(first.clone());
        // Do NOT poll here: in_flight stays set until a poll drains it, so the
        // second request is deterministically ignored regardless of worker
        // timing.
        loader.request(second);
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
}
