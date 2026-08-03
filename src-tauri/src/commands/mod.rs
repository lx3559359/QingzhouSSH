pub mod bootstrap;
pub mod executions;
pub mod logs;
pub mod servers;
pub mod transfers;

use tauri::{ipc::Channel, State};

use crate::{
    core::ssh::executor::EventSink,
    domain::events::ExecutionEvent,
    error::{AppError, AppResult},
    services::app_services::AppServices,
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

pub(crate) struct ChannelEventSink(pub Channel<ExecutionEvent>);

impl EventSink for ChannelEventSink {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        self.0
            .send(event)
            .map_err(|error| AppError::Ipc(error.to_string()))
    }
}
