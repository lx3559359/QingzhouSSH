use std::path::Path;

use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::error::AppResult;

pub fn build_main_window(app: &AppHandle, data_root: Option<&Path>) -> AppResult<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("轻舟 SSH")
        .inner_size(1180.0, 760.0)
        .min_inner_size(960.0, 640.0);
    let builder = match data_root {
        Some(root) => builder.data_directory(root.join("cache").join("webview2")),
        None => builder.incognito(true),
    };
    Ok(builder.build()?)
}
