#![allow(linker_messages)]

mod commands;
pub mod core;
pub mod domain;
pub mod error;
pub mod repositories;
pub mod services;
mod state;
pub mod window;

pub use core::data_migration::run_process_mode;
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
            commands::data_migration::preflight_data_root_migration,
            commands::data_migration::preflight_retry_data_root_migration,
            commands::data_migration::preflight_portable_default_data_root_migration,
            commands::data_migration::start_data_root_migration,
            commands::data_migration::get_data_root_migration_status,
            commands::data_migration::acknowledge_data_root_migration,
            commands::data_migration::open_data_root_folder,
            commands::servers::list_servers,
            commands::servers::create_server,
            commands::servers::inspect_server_host_key,
            commands::servers::trust_server_host_key,
            commands::servers::test_server_connection,
            commands::executions::list_task_definitions,
            commands::executions::get_task_library_snapshot,
            commands::executions::start_task_execution,
            commands::executions::start_custom_execution,
            commands::executions::cancel_execution,
            commands::remediation::preview_task_remediation,
            commands::remediation::confirm_task_remediation,
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
            commands::operations::preview_operation,
            commands::operations::start_operation,
            commands::operations::confirm_operation,
            commands::operations::list_operation_restore_points,
            commands::operations::rollback_operation,
            commands::operations::inspect_uncertain_operation,
            commands::operations::cleanup_operation_restore_assets,
            commands::operations::cancel_operation,
            commands::operations::get_operation,
            commands::operations::list_operations,
            commands::operations::start_operation_batch,
            commands::operations::cancel_operation_batch,
            commands::operations::get_operation_batch,
            commands::operations::export_operation_report,
            commands::operations::export_operation_batch_report,
            commands::scripts::list_personal_scripts,
            commands::scripts::get_personal_script_for_editor,
            commands::scripts::list_personal_script_versions,
            commands::scripts::create_personal_script,
            commands::scripts::save_personal_script_version,
            commands::scripts::update_personal_script_metadata,
            commands::scripts::copy_personal_script,
            commands::scripts::delete_personal_script,
            commands::scripts::set_personal_script_favorite,
            commands::scripts::set_personal_script_enabled,
            commands::scripts::import_personal_script,
            commands::scripts::export_personal_script,
            commands::scripts::preview_personal_script_run,
            commands::scripts::confirm_personal_script_run,
            commands::scripts::cancel_personal_script_run,
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
                let services = tauri::async_runtime::block_on(AppServices::open(root))?
                    .with_data_root_resolution(&resolution);
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
