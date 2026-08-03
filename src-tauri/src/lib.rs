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
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            commands::transfers::upload_file,
            commands::transfers::download_file,
            commands::executions::list_executions,
            commands::executions::get_execution,
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
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    *state.services.write().await = Some(services);
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
    fn package_identity_is_stable() {
        assert_eq!(env!("CARGO_PKG_NAME"), "qingzhou-ssh");
    }
}
