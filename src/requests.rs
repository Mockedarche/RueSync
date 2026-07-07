use serde::{Deserialize, Serialize};

use crate::config_handler::BackupConfig;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    Basic {
        command: String,
    },

    Backup {
        command: String,
        backup: BackupConfig,
    },

    Debug {
        command: String,
        argument: String,
    },
}
