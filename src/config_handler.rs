use std::{
    fs,
    path::Path,
    sync::{Arc, RwLock},
    u64,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub ruesync: RueSyncConfig,
    pub backups: Vec<BackupConfig>,
    pub network: NetworkConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RueSyncConfig {
    pub state_folder: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum BackupLocation {
    Local,
    Lan,
    Wan,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupConfig {
    pub name: String,
    pub enabled: bool,
    pub source_directory: String,
    pub destination_directory: String,
    pub local_lan_wan: BackupLocation,
    pub source_bandwidth_cap_in_bytes: u64,
    pub destination_bandwidth_cap_in_bytes: u64,
    pub backup_interval_in_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub port: u16,
    pub upload_bandwidth_cap_in_bytes: u64,
    pub download_bandwidth_cap_in_bytes: u64,
    pub upload_data_cap_in_bytes: u64,
    pub download_data_cap_in_bytes: u64,
}

pub fn create_config() {
    let config = Config {
        ruesync: RueSyncConfig {
            state_folder: "state".to_string(),
        },
        backups: Vec::new(),
        network: NetworkConfig {
            enabled: false,
            port: 55000,
            upload_bandwidth_cap_in_bytes: u64::MAX,
            download_bandwidth_cap_in_bytes: u64::MAX,
            upload_data_cap_in_bytes: u64::MAX,
            download_data_cap_in_bytes: u64::MAX,
        },
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    if !Path::new("config.json").exists() {
        fs::write("config.json", json).unwrap();
        println!("Created config.json");
    } else {
        println!("A Config already exists WHEN we tried to create a new one");
        println!("Didn't overwrite instead exiting");
    }
}

fn load_config() -> Config {
    let json = fs::read_to_string("config.json").unwrap();
    let config: Config = serde_json::from_str(&json).unwrap();

    println!("Loaded config:");
    println!("{:#?}", config);

    config
}

pub fn save_modified_config(config: Arc<RwLock<Config>>) {
    let json = {
        let cfg = config.read().unwrap();
        serde_json::to_string_pretty(&*cfg).unwrap()
    };

    fs::write("config.json", json).unwrap();
}

pub fn init() -> Config {
    if Path::new("config.json").exists() {
        load_config()
    } else {
        create_config();
        load_config()
    }
}
