#![allow(linker_messages)]

pub mod error;

use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("轻舟 SSH")
                .inner_size(1180.0, 760.0)
                .min_inner_size(960.0, 640.0)
                .incognito(true)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run QingzhouSSH");
}

#[cfg(test)]
mod tests {
    #[test]
    fn package_identity_is_stable() {
        assert_eq!(env!("CARGO_PKG_NAME"), "qingzhou-ssh");
    }
}
