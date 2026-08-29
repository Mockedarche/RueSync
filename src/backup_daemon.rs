use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};

use crate::{
    backup::{local, remote},
    config_handler::{BackupConfig, BackupLocation, Config},
    recheck_directory::{self, FileHash},
    requests::Request,
};

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Eq, PartialEq)]
pub struct BackupTask {
    pub run_at: Instant,
    pub backup_index: usize,
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

pub fn start(config: SharedConfig) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run(config);
    })
}

fn run(config: SharedConfig) {
    println!("RueSync backup daemon started");

    let mut tasks: BinaryHeap<BackupTask> = BinaryHeap::new();

    {
        let mut cfg = config.write().unwrap();

        for index in 0..cfg.backups.len() {
            if !cfg.backups[index].task_active
                && cfg.backups[index].enabled
                && cfg.backups[index].source_of_backup
            {
                tasks.push(BackupTask {
                    run_at: Instant::now(),
                    backup_index: index,
                });

                cfg.backups[index].task_active = true;

                println!("Added backup {} to tasks", cfg.backups[index].name);
            }
        }
    }

    loop {
        if let Some(task) = tasks.peek() {
            if task.run_at <= Instant::now() {
                let current_backup_config = {
                    let cfg = config.read().unwrap();

                    if task.backup_index >= cfg.backups.len() {
                        tasks.pop();
                        continue;
                    }

                    cfg.backups[task.backup_index].clone()
                };

                run_backup(&current_backup_config);

                let current_backup_index = task.backup_index;

                tasks.pop();

                tasks.push(BackupTask {
                    run_at: Instant::now()
                        + Duration::from_secs(current_backup_config.backup_interval_in_seconds),
                    backup_index: current_backup_index,
                });
            }
        }

        {
            let mut cfg = config.write().unwrap();

            for (index, backup) in cfg.backups.iter_mut().enumerate() {
                if !backup.task_active && backup.enabled && backup.source_of_backup {
                    tasks.push(BackupTask {
                        run_at: Instant::now(),
                        backup_index: index,
                    });

                    backup.task_active = true;

                    println!("Scheduled new backup {}", backup.name);
                }
            }
        }

        thread::sleep(Duration::from_secs(1));
    }
}

fn run_backup(backup: &BackupConfig) {
    if !backup.enabled {
        return;
    }

    let backup_path = Path::new(&backup.source_directory);

    if !backup_path.exists() || !backup_path.is_dir() {
        eprintln!("Backup source directory does not exist for {}", backup.name);
        return;
    }

    let backup_state = match recheck_directory::get_directory_state(backup_path, Path::new("state"))
    {
        Ok(state) => state,

        Err(e) => {
            eprintln!("Failed to get backup state for {}: {:?}", backup.name, e);
            return;
        }
    };

    match backup.local_lan_wan {
        BackupLocation::Local => {
            run_local_backup(backup, backup_state);
        }

        BackupLocation::Lan | BackupLocation::Wan => {
            run_remote_backup(backup, backup_state);
        }
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
