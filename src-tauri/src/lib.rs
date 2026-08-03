#![allow(linker_messages)]

pub mod core;
pub mod error;
pub mod window;

use core::data_root::{initialize_data_root, resolve_runtime_data_root};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let resolution = resolve_runtime_data_root()?;
            if let Some(root) = resolution.path.as_deref() {
                initialize_data_root(root)?;
            }
            window::build_main_window(app.handle(), resolution.path.as_deref())?;
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
