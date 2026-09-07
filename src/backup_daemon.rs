use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::{
    backup::{local, remote},
    config_handler::{BackupConfig, BackupLocation, Config},
    recheck_directory::{self, FileHash},
    requests::Request,
    runtime_state::{BackupRuntimeInfo, BackupStatus, SharedRuntimeState},
};

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Eq, PartialEq)]
pub struct BackupTask {
    pub run_at: Instant,
    pub backup_id: String,
}

impl Ord for BackupTask {
    fn cmp(&self, other: &Self) -> Ordering {
        other.run_at.cmp(&self.run_at)
    }
}

impl PartialOrd for BackupTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn start(config: SharedConfig, runtime_state: SharedRuntimeState) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run(config, runtime_state);
    })
}

fn run(config: SharedConfig, runtime_state: SharedRuntimeState) {
    println!("RueSync backup daemon started");

    let mut tasks: BinaryHeap<BackupTask> = BinaryHeap::new();

    // Add existing backups to the task queue.
    {
        let mut cfg = config.write().unwrap();

        for backup in cfg.backups.iter_mut() {
            if !backup.task_active && backup.enabled && backup.source_of_backup {
                tasks.push(BackupTask {
                    run_at: Instant::now(),
                    backup_id: backup.unique_id.clone(),
                });

                backup.task_active = true;

                println!("Added backup {} to tasks", backup.name);
            }
        }
    }

    loop {
        if let Some(task) = tasks.peek() {
            if task.run_at <= Instant::now() {
                let backup_id = task.backup_id.clone();

                // Look up the backup by unique ID rather than vector index.
                let current_backup_config = {
                    let cfg = config.read().unwrap();

                    cfg.backups
                        .iter()
                        .find(|backup| backup.unique_id == backup_id)
                        .cloned()
                };

                // Remove the current task regardless.
                tasks.pop();

                match current_backup_config {
                    Some(backup) => {
                        run_backup(&backup, &runtime_state);

                        // Only reschedule if it still exists and is enabled.
                        if backup.enabled && backup.source_of_backup {
                            tasks.push(BackupTask {
                                run_at: Instant::now()
                                    + Duration::from_secs(backup.backup_interval_in_seconds),
                                backup_id: backup.unique_id.clone(),
                            });
                        }
                    }

                    None => {
                        println!("Backup {} no longer exists, removing task", backup_id);
                    }
                }
            }
        }

        // Check for newly added backups.
        {
            let mut cfg = config.write().unwrap();

            for backup in cfg.backups.iter_mut() {
                if !backup.task_active && backup.enabled && backup.source_of_backup {
                    tasks.push(BackupTask {
                        run_at: Instant::now(),
                        backup_id: backup.unique_id.clone(),
                    });

                    backup.task_active = true;

                    println!("Scheduled new backup {}", backup.name);
                }
            }
        }

        thread::sleep(Duration::from_secs(1));
    }
}

fn run_backup(backup: &BackupConfig, runtime_state: &SharedRuntimeState) {
    if !backup.enabled {
        return;
    }

    // Mark backup as currently running.
    {
        let mut state = runtime_state.write().unwrap();

        state.backups.insert(
            backup.unique_id.clone(),
            BackupRuntimeInfo {
                unique_id: backup.unique_id.clone(),
                name: backup.name.clone(),
                is_running: true,
                status: BackupStatus::Running,
                current_operation: Some("Starting backup".to_string()),
                last_error: None,
                last_backup: None,
                next_backup: None,
            },
        );
    }

    let backup_path = Path::new(&backup.source_directory);

    if !backup_path.exists() || !backup_path.is_dir() {
        let error = format!(
            "Backup source directory does not exist: {}",
            backup.source_directory
        );

        eprintln!("{}", error);

        let mut state = runtime_state.write().unwrap();

        if let Some(info) = state.backups.get_mut(&backup.unique_id) {
            info.status = BackupStatus::Error;
            info.is_running = false;
            info.current_operation = None;
            info.last_error = Some(error);
        }

        return;
    }

    // Update current operation.
    {
        let mut state = runtime_state.write().unwrap();

        if let Some(info) = state.backups.get_mut(&backup.unique_id) {
            info.current_operation = Some("Checking source files".to_string());
        }
    }

    let backup_state = match recheck_directory::get_directory_state(backup_path, Path::new("state"))
    {
        Ok(state) => state,

        Err(e) => {
            let error = format!("Failed to get backup state for {}: {:?}", backup.name, e);

            eprintln!("{}", error);

            let mut state = runtime_state.write().unwrap();

            if let Some(info) = state.backups.get_mut(&backup.unique_id) {
                info.status = BackupStatus::Error;
                info.is_running = false;
                info.current_operation = None;
                info.last_error = Some(error);
            }

            return;
        }
    };

    // Update current operation.
    {
        let mut state = runtime_state.write().unwrap();

        if let Some(info) = state.backups.get_mut(&backup.unique_id) {
            info.current_operation = Some("Performing backup".to_string());
        }
    }

    match backup.local_lan_wan {
        BackupLocation::Local => {
            run_local_backup(backup, backup_state);
        }

        BackupLocation::Lan | BackupLocation::Wan => {
            run_remote_backup(backup, backup_state);
        }
    }

    // Update final runtime status.
    let mut state = runtime_state.write().unwrap();

    if let Some(info) = state.backups.get_mut(&backup.unique_id) {
        info.status = BackupStatus::Success;
        info.is_running = false;
        info.current_operation = None;
        info.last_backup = Some(SystemTime::now());
        info.last_error = None;
    }
}

fn run_local_backup(backup: &BackupConfig, backup_state: Vec<FileHash>) {
    let destination_file_hash = match recheck_directory::get_directory_state(
        Path::new(&backup.destination_directory),
        Path::new("local_state"),
    ) {
        Ok(state) => state,

        Err(e) => {
            eprintln!("Failed to get destination state: {:?}", e);
            return;
        }
    };

    local::local_backup(backup, backup_state, destination_file_hash);
}

fn run_remote_backup(backup: &BackupConfig, backup_state: Vec<FileHash>) {
    let destination_file_hash = match get_remote_state(backup) {
        Ok(state) => state,

        Err(e) => {
            eprintln!("Failed to get destination state: {:?}", e);
            return;
        }
    };

    match remote::remote_backup(backup, backup_state, destination_file_hash) {
        Ok(()) => {
            println!("Remote backup {} completed successfully", backup.name);
        }

        Err(e) => {
            eprintln!("Remote backup {} failed: {}", backup.name, e);
        }
    }
}

fn get_remote_state(backup: &BackupConfig) -> std::io::Result<Vec<FileHash>> {
    let addr = format!("{}:55000", backup.network_information.address);

    let socket_addr = addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let mut stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(1))?;

    let request = Request::Backup {
        command: "get_state".to_string(),
        backup: backup.clone(),
    };

    let json = serde_json::to_string(&request)?;

    stream.write_all(json.as_bytes())?;

    let mut buffer = [0u8; 4096];

    let size = stream.read(&mut buffer)?;

    let response: crate::tcp_server::Response = serde_json::from_slice(&buffer[..size])?;

    match response {
        crate::tcp_server::Response::FileState { files } => Ok(files),

        crate::tcp_server::Response::Error { message } => {
            Err(std::io::Error::new(std::io::ErrorKind::Other, message))
        }

        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Unexpected response",
        )),
    }
}
