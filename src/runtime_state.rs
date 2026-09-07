use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::SystemTime,
};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum BackupStatus {
    Waiting,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupRuntimeInfo {
    pub unique_id: String,
    pub name: String,
    pub is_running: bool,
    pub status: BackupStatus,
    pub current_operation: Option<String>,
    pub last_error: Option<String>,
    pub last_backup: Option<SystemTime>,
    pub next_backup: Option<SystemTime>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RuntimeState {
    pub backups: HashMap<String, BackupRuntimeInfo>,
    pub web_server_active: bool,
    pub daemon_active: bool,
    pub tcp_server_active: bool,
}

pub type SharedRuntimeState = Arc<RwLock<RuntimeState>>;
