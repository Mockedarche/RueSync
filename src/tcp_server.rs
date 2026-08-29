use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use crate::{
    backup::{local, remote, remote::RemoteBackupRequest},
    config_handler::{self, BackupConfig, BackupLocation, Config},
    init_directory_state,
    recheck_directory::{self, FileHash},
    requests::Request,
};

use serde::{Deserialize, Serialize};

pub type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Success { message: String },
    Error { message: String },
    FileState { files: Vec<FileHash> },
}

pub fn start(config: SharedConfig) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        start_tcp_server(config);
    })
}

fn start_tcp_server(config: SharedConfig) {
    start_backup_listener(Arc::clone(&config));

    let listener = TcpListener::bind("0.0.0.0:55000").expect("Failed to bind to 0.0.0.0:55000");

    println!("RueSync TCP server running on 0.0.0.0:55000");

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
}

fn start_backup_listener(config: SharedConfig) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind("0.0.0.0:55001").expect("Failed binding backup port");

        println!("RueSync backup listener running on 0.0.0.0:55001");

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let config = Arc::clone(&config);

                    thread::spawn(move || {
                        let request = match remote::receive_request(&mut stream) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("Failed receiving backup request: {}", e);
                                return;
                            }
                        };

                        let backup_config = match request {
                            RemoteBackupRequest::BeginBackup { backup_id } => {
                                let cfg = config.read().unwrap();

                                match cfg.backups.iter().find(|b| b.unique_id == backup_id) {
                                    Some(b) => b.clone(),

                                    None => {
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
                                let _ = remote::send_request(
                                    &mut stream,
                                    &RemoteBackupRequest::Error {
                                        message: "Expected BeginBackup".to_string(),
                                    },
                                );

                                return;
                            }
                        };

                        if let Err(e) =
                            remote::send_request(&mut stream, &RemoteBackupRequest::Ready)
                        {
                            eprintln!("Failed sending ready: {}", e);
                            return;
                        }

                        if let Err(e) = remote::receive_backup(&mut stream, &backup_config) {
                            eprintln!("Backup failed: {}", e);
                        }
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
            eprintln!("Read error: {e}");
            return;
        }
    };

    let msg = String::from_utf8_lossy(&buffer[..size]);

    let request: Request = match serde_json::from_str(&msg) {
        Ok(req) => req,

        Err(e) => {
            send_response(
                &mut stream,
                Response::Error {
                    message: format!("JSON error: {}", e),
                },
            );

            return;
        }
    };

    let response = match request {
        Request::Basic { command } => match command.as_str() {
            "ping" => Response::Success {
                message: "pong".to_string(),
            },

            _ => Response::Error {
                message: "unknown basic command".to_string(),
            },
        },

        Request::Backup { command, backup } => handle_backup_command(command, backup, config),

        Request::Debug { command, argument } => match command.as_str() {
            "temp" => Response::Success {
                message: "temp".to_string(),
            },

            "init" => {
                let path = Path::new(&argument);

                if !path.exists() {
                    Response::Error {
                        message: "Given path doesn't exist".to_string(),
                    }
                } else if !path.is_dir() {
                    Response::Error {
                        message: "Given path isn't a directory".to_string(),
                    }
                } else {
                    init_directory_state::scan_to_json(path);

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

    send_response(&mut stream, response);
}

fn handle_backup_command(command: String, backup: BackupConfig, config: SharedConfig) -> Response {
    match command.as_str() {
        "new_backup" => {
            let source_path = Path::new(&backup.source_directory);

            if backup.source_of_backup && (!source_path.exists() || !source_path.is_dir()) {
                return Response::Error {
                    message: "Source directory does not exist".to_string(),
                };
            }

            let mut cfg = config.write().unwrap();

            cfg.backups.push(backup);

            drop(cfg);

            config_handler::save_modified_config(&config);

            Response::Success {
                message: "backup added".to_string(),
            }
        }

        "receive_backup" => {
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
    }
}

fn send_response(stream: &mut TcpStream, response: Response) {
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
