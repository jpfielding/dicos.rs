//! Browser glue: file picking and sample fetching (wasm32 only).
//!
//! Async tasks deliver their results to a thread-local inbox that
//! [`App::render`](crate::app::App::render) drains each frame.

use crate::loader::{largest_file, LoadSource};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// Relative URLs (resolved against the page, so the GitHub Pages base path works).
const SAMPLE_CT: &str = "samples/fishtank-knife/ct.dcs";
const SAMPLE_TDR: &str = "samples/fishtank-knife/tdr.dcs";

thread_local! {
    static INBOX: RefCell<Vec<Result<LoadSource, String>>> = const { RefCell::new(Vec::new()) };
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

/// Remember the egui context so async completions can request a repaint.
pub(crate) fn set_context(ctx: egui::Context) {
    CONTEXT.with(|c| *c.borrow_mut() = Some(ctx));
}

/// Drain everything delivered since the last frame.
pub(crate) fn take() -> Vec<Result<LoadSource, String>> {
    INBOX.with(|inbox| std::mem::take(&mut *inbox.borrow_mut()))
}

fn deliver(item: Result<LoadSource, String>) {
    INBOX.with(|inbox| inbox.borrow_mut().push(item));
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}

fn source_from_files(files: Vec<(String, Vec<u8>)>) -> Result<LoadSource, String> {
    let index = largest_file(&files).ok_or_else(|| "No files selected".to_string())?;
    let name = files[index].0.clone();
    Ok(LoadSource::Bytes { name, files })
}

/// Open the browser file picker (multi-select) and deliver the chosen files.
///
/// Must be called from a user gesture handler (a click) or the browser will
/// refuse to open the dialog.
pub fn pick_files() {
    spawn_local(async {
        let Some(handles) = rfd::AsyncFileDialog::new()
            .add_filter("DICOS", &["dcs", "dcm"])
            .pick_files()
            .await
        else {
            return;
        };
        let mut files = Vec::with_capacity(handles.len());
        for handle in handles {
            files.push((handle.file_name(), handle.read().await));
        }
        deliver(source_from_files(files));
    });
}

/// Fetch the bundled sample scan (CT plus its threat report) and deliver it.
pub fn load_sample() {
    spawn_local(async {
        let ct = match fetch_bytes(SAMPLE_CT).await {
            Ok(bytes) => bytes,
            Err(e) => return deliver(Err(format!("Failed to load sample: {e}"))),
        };
        let mut files = vec![("ct.dcs".to_string(), ct)];
        match fetch_bytes(SAMPLE_TDR).await {
            Ok(bytes) => files.push(("tdr.dcs".to_string(), bytes)),
            Err(e) => log::warn!("Sample threat report unavailable: {e}"),
        }
        deliver(source_from_files(files));
    });
}

fn js_err(e: wasm_bindgen::JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(js_err)?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| "fetch did not return a Response".to_string())?;
    if !response.ok() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(js_err)?)
        .await
        .map_err(js_err)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// The page's `<canvas id="roxel">`, if present.
pub fn find_canvas() -> Option<web_sys::HtmlCanvasElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("roxel")?
        .dyn_into()
        .ok()
}

/// Show `message` in the page's `<div id="roxel-error">` (and the console).
pub fn show_error(message: &str) {
    log::error!("{message}");
    let element = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("roxel-error"));
    if let Some(element) = element {
        element.set_text_content(Some(message));
        let _ = element.set_attribute("style", "display:block");
    }
}
