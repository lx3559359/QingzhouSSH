use std::sync::atomic::AtomicBool;

use tokio::sync::RwLock;

use crate::services::{app_services::AppServices, update_service::UpdateManager};

pub struct AppState {
    pub services: RwLock<Option<AppServices>>,
    pub updater: RwLock<Option<UpdateManager>>,
    pub migration_starting: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            services: RwLock::new(None),
            updater: RwLock::new(None),
            migration_starting: AtomicBool::new(false),
        }
    }
}
