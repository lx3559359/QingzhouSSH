use tokio::sync::RwLock;

use crate::services::app_services::AppServices;

pub struct AppState {
    pub services: RwLock<Option<AppServices>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            services: RwLock::new(None),
        }
    }
}
