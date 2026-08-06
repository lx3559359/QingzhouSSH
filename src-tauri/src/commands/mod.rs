pub mod bootstrap;
pub mod executions;
pub mod logs;
pub mod operations;
pub mod remediation;
pub mod scripts;
pub mod servers;
pub mod transfers;
pub mod updates;
pub mod workflows;

use tauri::{ipc::Channel, State};

use crate::{
    core::ssh::executor::EventSink,
    domain::events::ExecutionEvent,
    domain::workflow_events::{WorkflowEvent, WorkflowEventSink},
    error::{AppError, AppResult},
    services::app_services::AppServices,
    services::update_service::UpdateManager,
    state::AppState,
};

pub(crate) async fn services(state: &State<'_, AppState>) -> AppResult<AppServices> {
    state
        .services
        .read()
        .await
        .clone()
        .ok_or(AppError::NotReady)
}

pub(crate) async fn updater(state: &State<'_, AppState>) -> AppResult<UpdateManager> {
    state.updater.read().await.clone().ok_or(AppError::NotReady)
}

pub(crate) struct ChannelEventSink(pub Channel<ExecutionEvent>);

impl EventSink for ChannelEventSink {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        self.0
            .send(event)
            .map_err(|error| AppError::Ipc(error.to_string()))
    }
}

pub(crate) struct WorkflowChannelEventSink(pub Channel<WorkflowEvent>);

impl WorkflowEventSink for WorkflowChannelEventSink {
    fn send(&mut self, event: WorkflowEvent) -> AppResult<()> {
        self.0
            .send(event)
            .map_err(|error| AppError::Ipc(error.to_string()))
    }
}
