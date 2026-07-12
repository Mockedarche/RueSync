use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    io::{Read, Write},
    mem::take,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};

use RueSync::backup::{local, remote, remote::RemoteBackupRequest};
use RueSync::config_handler::{self, BackupConfig, BackupLocation, Config};
use RueSync::init_directory_state;
use RueSync::recheck_directory::{self, FileHash};
use RueSync::requests::Request;

use serde::{Deserialize, Serialize};

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Eq, PartialEq)]
pub struct Backup_Task {
    pub run_at: Instant,
    pub backup_index: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Success { message: String },
    Error { message: String },
    FileState { files: Vec<FileHash> },
}

fn start_backup_listener(config: SharedConfig) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind("0.0.0.0:55001").expect("Failed binding backup port");

        println!("RueSync backup daemon running on 0.0.0.0:55001");

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    println!("Backup connection received");

                    let config = Arc::clone(&config);

                    thread::spawn(move || {
                        let request: RemoteBackupRequest =
                            match remote::receive_request(&mut stream) {
                                Ok(r) => r,
                                Err(e) => {
                                    eprintln!("Failed receiving backup request: {}", e);
                                    return;
                                }
                            };

                        println!("Backup listener received: {:?}", request);

                        let backup_config = match request {
                            RemoteBackupRequest::BeginBackup { backup_id } => {
                                println!("BeginBackup received: {}", backup_id);

                                let cfg = config.read().unwrap();

                                match cfg.backups.iter().find(|b| b.unique_id == backup_id) {
                                    Some(b) => {
                                        println!("Found backup: {}", b.name);
                                        b.clone()
                                    }

                                    None => {
                                        println!("Backup not found: {}", backup_id);

                                        let _ = remote::send_request(
                                            &mut stream,
                                            &RemoteBackupRequest::Error {
                                                message: "Backup not found".to_string(),
                                            },
                                        );

                                        return;
                                    }
                                }
                            }

                            _ => {
                                println!("Invalid first backup request");

                                let _ = remote::send_request(
                                    &mut stream,
                                    &RemoteBackupRequest::Error {
                                        message: "Expected BeginBackup".to_string(),
                                    },
                                );

                                return;
                            }
                        };

                        println!("Sending Ready");

                        if let Err(e) =
                            remote::send_request(&mut stream, &RemoteBackupRequest::Ready)
                        {
                            eprintln!("Failed sending ready: {}", e);
                            return;
                        }

                        println!("Starting receive_backup");

                        if let Err(e) = remote::receive_backup(&mut stream, &backup_config) {
                            eprintln!("Backup failed: {}", e);
                        }

                        println!("Backup connection finished");
                    });
                }

                Err(e) => {
                    eprintln!("Backup connection failed: {}", e);
                }
            }
        }
    })
}

fn handle_client(mut stream: TcpStream, config: SharedConfig) {
    let mut buffer = [0u8; 4096];

    let size = match stream.read(&mut buffer) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read error: {e}");
            return;
        }
    };

    let msg = String::from_utf8_lossy(&buffer[..size]);

    println!("Raw message: {}", msg);

    let request: Request = match serde_json::from_str(&msg) {
        Ok(req) => req,
        Err(e) => {
            let response = Response::Error {
                message: format!("JSON error: {}", e),
            };

            let json = serde_json::to_string(&response).unwrap();
            let _ = stream.write_all(json.as_bytes());
            return;
        }
    };

    println!("Received request: {:#?}", request);

    let response = match request {
        Request::Basic { command } => match command.as_str() {
            "ping" => {
                println!("Received a ping");

                Response::Success {
                    message: "pong".to_string(),
                }
            }

            _ => Response::Error {
                message: "unknown basic command".to_string(),
            },
        },

        Request::Backup { command, backup } => match command.as_str() {
            "new_backup" => {
                println!("Received new backup: {}", backup.name);

                if backup.source_of_backup {
                    let source_path = Path::new(&backup.source_directory);

                    if !source_path.exists() || !source_path.is_dir() {
                        Response::Error {
                            message: "Source directory does not exist".to_string(),
                        }
                    } else {
                        match backup.local_lan_wan {
                            BackupLocation::Lan | BackupLocation::Wan => {
                                let addr =
                                    format!("{}:{}", backup.network_information.address, "55000",);

                                let request = Request::Backup {
                                    command: "new_backup".to_string(),
                                    backup: BackupConfig {
                                        source_of_backup: false,
                                        ..backup.clone()
                                    },
                                };

                                match addr.parse() {
                                    Ok(socket_addr) => {
                                        match TcpStream::connect_timeout(
                                            &socket_addr,
                                            Duration::from_secs(1),
                                        ) {
                                            Ok(mut remote_stream) => {
                                                let json = match serde_json::to_string(&request) {
                                                    Ok(j) => j,
                                                    Err(e) => {
                                                        return;
                                                    }
                                                };

                                                if let Err(e) =
                                                    remote_stream.write_all(json.as_bytes())
                                                {
                                                    Response::Error {
                                                        message: format!(
                                                            "Failed sending destination request: {}",
                                                            e
                                                        ),
                                                    }
                                                } else {
                                                    let mut response_buffer = [0u8; 4096];

                                                    match remote_stream.read(&mut response_buffer) {
                                                        Ok(size) => {
                                                            println!(
                                                                "Destination response: {}",
                                                                String::from_utf8_lossy(
                                                                    &response_buffer[..size]
                                                                )
                                                            );

                                                            let mut cfg = config.write().unwrap();

                                                            cfg.backups.push(backup);

                                                            drop(cfg);

                                                            config_handler::save_modified_config(
                                                                &config,
                                                            );

                                                            Response::Success {
                                                                message: "backup added".to_string(),
                                                            }
                                                        }

                                                        Err(e) => Response::Error {
                                                            message: format!(
                                                                "Destination timeout/error: {}",
                                                                e
                                                            ),
                                                        },
                                                    }
                                                }
                                            }

                                            Err(e) => Response::Error {
                                                message: format!(
                                                    "Destination daemon unavailable: {}",
                                                    e
                                                ),
                                            },
                                        }
                                    }

                                    Err(e) => Response::Error {
                                        message: format!("Invalid destination address: {}", e),
                                    },
                                }
                            }

                            BackupLocation::Local => {
                                let mut cfg = config.write().unwrap();

                                cfg.backups.push(backup);

                                drop(cfg);

                                config_handler::save_modified_config(&config);

                                Response::Success {
                                    message: "backup added".to_string(),
                                }
                            }
                        }
                    }
                } else {
                    let mut cfg = config.write().unwrap();

                    cfg.backups.push(backup);

                    drop(cfg);

                    config_handler::save_modified_config(&config);

                    Response::Success {
                        message: "backup added".to_string(),
                    }
                }
            }
            "receive_backup" => {
                println!("Preparing backup receiver for {}", backup.name);

                let exists = {
                    let cfg = config.read().unwrap();
                    cfg.backups.iter().any(|b| b.unique_id == backup.unique_id)
                };

                if exists {
                    Response::Success {
                        message: "backup receiver ready".to_string(),
                    }
                } else {
                    Response::Error {
                        message: format!("Backup {} not found", backup.unique_id),
                    }
                }
            }

            "get_state" => {
                println!(
                    "Received get_state for {} ({})",
                    backup.name, backup.unique_id
                );

                let state_path = match backup.local_lan_wan {
                    BackupLocation::Local => Path::new("local_state"),
                    BackupLocation::Lan => Path::new("lan_state"),
                    BackupLocation::Wan => Path::new("wan_state"),
                };

                match recheck_directory::get_directory_state(
                    Path::new(&backup.destination_directory),
                    state_path,
                ) {
                    Ok(files) => Response::FileState { files },

                    Err(e) => Response::Error {
                        message: format!("Failed getting state: {:?}", e),
                    },
                }
            }

            _ => Response::Error {
                message: "unknown backup command".to_string(),
            },
        },

        Request::Debug { command, argument } => match command.as_str() {
            "temp" => Response::Success {
                message: "temp".to_string(),
            },

            "init" => {
                let given_path = Path::new(&argument);

                if !given_path.exists() {
                    Response::Error {
                        message: "Given path doesn't exist".to_string(),
                    }
                } else if !given_path.is_dir() {
                    Response::Error {
                        message: "Given path isn't a directory".to_string(),
                    }
                } else {
                    init_directory_state::scan_to_json(given_path);

                    Response::Success {
                        message: "init_complete".to_string(),
                    }
                }
            }

            _ => Response::Error {
                message: "unknown debug command".to_string(),
            },
        },
    };

    let json = match serde_json::to_string(&response) {
        Ok(data) => data,

        Err(e) => {
            eprintln!("Response serialization failed: {}", e);
            return;
        }
    };

    if let Err(e) = stream.write_all(json.as_bytes()) {
        eprintln!("Response write failed: {}", e);
    }
}

fn start_daemon(config: SharedConfig) -> thread::JoinHandle<()> {
    let backup_config = Arc::clone(&config);

    start_backup_listener(backup_config);

    thread::spawn(move || {
        let listener = TcpListener::bind("0.0.0.0:55000").expect("Failed to bind to 0.0.0.0:55000");

        println!("RueSync daemon running on 0.0.0.0:55000");

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let config = Arc::clone(&config);

                    thread::spawn(move || {
                        handle_client(stream, config);
                    });
                }

                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }
    })
}

fn get_remote_state(backup: &BackupConfig) -> std::io::Result<Vec<FileHash>> {
    let addr = format!(
        "{}:{}",
        backup.network_information.address,
        55000 // normal communication port
    );

    let mut stream = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(1))?;

    let request = Request::Backup {
        command: "get_state".to_string(),
        backup: backup.clone(),
    };

    let json = serde_json::to_string(&request)?;

    stream.write_all(json.as_bytes())?;

    let mut buffer = [0u8; 4096];

    let size = stream.read(&mut buffer)?;

    let response: Response = serde_json::from_slice(&buffer[..size])?;

    match response {
        Response::FileState { files } => Ok(files),

        Response::Error { message } => Err(std::io::Error::new(std::io::ErrorKind::Other, message)),

        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Unexpected response",
        )),
    }
}

/* START OF HELPERS FOR THE TASKS */
impl Ord for Backup_Task {
    fn cmp(&self, other: &Self) -> Ordering {
        other.run_at.cmp(&self.run_at)
    }
}

impl PartialOrd for Backup_Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/* END OF HELPERS FOR THE TASKS */

fn main() {
    println!("Starting RueSync daemon");

    let config: SharedConfig = Arc::new(RwLock::new(config_handler::init()));
    println!("Config loaded");

    let _daemon_thread = start_daemon(Arc::clone(&config));

    let mut tasks: BinaryHeap<Backup_Task> = BinaryHeap::new();

    {
        let mut cfg = config.write().unwrap();

        for index in 0..cfg.backups.len() {
            if !cfg.backups[index].task_active
                && cfg.backups[index].enabled
                && cfg.backups[index].source_of_backup
            {
                tasks.push(Backup_Task {
                    run_at: Instant::now(),
                    backup_index: index,
                });

                cfg.backups[index].task_active = true;

                println!("Added backup {} to tasks", cfg.backups[index].name);
            }
        }
    }

    // Main daemon loop.
    loop {
        if let Some(task) = tasks.peek() {
            if task.run_at <= Instant::now() {
                let current_backup_config = {
                    let cfg = config.read().unwrap();

                    cfg.backups[task.backup_index].clone()
                };
                if current_backup_config.enabled {
                    let backup_path = Path::new(&current_backup_config.source_directory);
                    if backup_path.exists() && backup_path.is_dir() {
                        let backup_state = match recheck_directory::get_directory_state(
                            backup_path,
                            Path::new("state"),
                        ) {
                            Ok(state) => state,
                            Err(e) => {
                                eprintln!("Failed to get backup state: {:?}", e);
                                return;
                            }
                        };
                        match current_backup_config.local_lan_wan {
                            BackupLocation::Local => {
                                let destination_file_hash =
                                    match recheck_directory::get_directory_state(
                                        Path::new(&current_backup_config.destination_directory),
                                        Path::new("local_state"),
                                    ) {
                                        Ok(state) => state,
                                        Err(e) => {
                                            eprintln!("Failed to get destination state: {:?}", e);
                                            return;
                                        }
                                    };

                                local::local_backup(
                                    &current_backup_config,
                                    backup_state,
                                    destination_file_hash,
                                );
                            }
                            BackupLocation::Lan | BackupLocation::Wan => {
                                // get the sources state file aka vec<FileHash>
                                let destination_file_hash =
                                    match get_remote_state(&current_backup_config) {
                                        Ok(state) => state,
                                        Err(e) => {
                                            eprintln!("Failed to get destination state: {:?}", e);
                                            return;
                                        }
                                    };

                                match remote::remote_backup(
                                    &current_backup_config,
                                    backup_state,
                                    destination_file_hash,
                                ) {
                                    Ok(()) => {
                                        println!("Remote backup completed successfully");
                                    }

                                    Err(e) => {
                                        eprintln!("Remote backup failed: {}", e);
                                    }
                                }
                            }
                        }
                    } else {
                        println!(
                            "Something went wrong in recheck directroy for {}",
                            current_backup_config.name
                        );
                    }
                }
                let current_backup_index = task.backup_index;
                tasks.pop();
                tasks.push(Backup_Task {
                    run_at: Instant::now()
                        + Duration::from_secs(current_backup_config.backup_interval_in_seconds),
                    backup_index: current_backup_index,
                });
            }
        }
        {
            let mut cfg = config.write().unwrap();
            let mut index = 0;
            for backup in &mut cfg.backups {
                if !backup.task_active && backup.enabled && backup.source_of_backup {
                    tasks.push(Backup_Task {
                        run_at: Instant::now(),
                        backup_index: index,
                    });

                    backup.task_active = true;

                    println!("Scheduled new backup {}", backup.name);
                }
                index += 1;
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}
