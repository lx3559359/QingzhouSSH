#![allow(linker_messages)]

mod commands;
pub mod core;
pub mod domain;
pub mod error;
pub mod repositories;
pub mod services;
mod state;
pub mod window;

use core::data_root::{initialize_data_root, resolve_runtime_data_root};
use services::app_services::AppServices;
use services::update_service::UpdateManager;
use state::AppState;
use tauri::Manager;

fn install_tls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_tls_crypto_provider();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::bootstrap_status,
            commands::bootstrap::initialize_data_root,
            commands::servers::list_servers,
            commands::servers::create_server,
            commands::servers::inspect_server_host_key,
            commands::servers::trust_server_host_key,
            commands::servers::test_server_connection,
            commands::executions::list_task_definitions,
            commands::executions::start_task_execution,
            commands::executions::start_custom_execution,
            commands::executions::cancel_execution,
            commands::logs::search_logs,
            commands::logs::read_log_result_page,
            commands::logs::download_log_result,
            commands::transfers::list_local_directory,
            commands::transfers::list_remote_directory,
            commands::transfers::upload_file,
            commands::transfers::download_file,
            commands::updates::get_update_status,
            commands::updates::set_auto_update_check,
            commands::updates::check_for_update,
            commands::updates::download_update,
            commands::updates::install_update,
            commands::updates::clear_downloaded_update,
            commands::executions::list_executions,
            commands::executions::get_execution,
            commands::operations::list_operations_tasks,
            commands::operations::preflight_operation,
            commands::operations::start_operation,
            commands::operations::cancel_operation,
            commands::operations::get_operation,
            commands::operations::list_operations,
            commands::operations::start_operation_batch,
            commands::operations::cancel_operation_batch,
            commands::operations::get_operation_batch,
            commands::workflows::list_workflows,
            commands::workflows::get_workflow,
            commands::workflows::save_workflow,
            commands::workflows::delete_workflow,
            commands::workflows::validate_workflow,
            commands::workflows::start_workflow_run,
            commands::workflows::cancel_workflow_run,
            commands::workflows::retry_workflow_node,
            commands::workflows::list_workflow_runs,
            commands::workflows::get_workflow_run,
            commands::workflows::rollback_workflow_run,
            commands::workflows::cleanup_workflow_restore_points,
            commands::workflows::export_workflow_diagnostics,
        ])
        .setup(|app| {
            let resolution = resolve_runtime_data_root()?;
            if let Some(root) = resolution.path.as_deref() {
                initialize_data_root(root)?;
                let services = tauri::async_runtime::block_on(AppServices::open(root))?;
                let updater = UpdateManager::new(
                    app.package_info().version.to_string(),
                    root,
                    app.handle().clone(),
                )?;
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    *state.services.write().await = Some(services);
                    *state.updater.write().await = Some(updater);
                });
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
    fn installs_ring_provider_before_reqwest_client_creation() {
        super::install_tls_crypto_provider();

        let client = std::panic::catch_unwind(|| reqwest::Client::builder().build());
        assert!(client.is_ok(), "reqwest client construction panicked");
        assert!(
            client.unwrap().is_ok(),
            "reqwest client construction failed"
        );
    }

    #[test]
    fn package_identity_is_stable() {
        assert_eq!(env!("CARGO_PKG_NAME"), "qingzhou-ssh");
    }
}
