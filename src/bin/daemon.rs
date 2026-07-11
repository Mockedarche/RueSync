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

use RueSync::backup::local;
use RueSync::config_handler::{self, BackupConfig, BackupLocation, Config};
use RueSync::init_directory_state;
use RueSync::recheck_directory;
use RueSync::requests::Request;

use serde::{Deserialize, Serialize};

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Debug, Eq, PartialEq)]
pub struct Backup_Task {
    pub run_at: Instant,
    pub backup_index: usize,
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
            eprintln!("JSON error: {}", e);
            return;
        }
    };

    println!("Received request: {:#?}", request);

    let response = match request {
        Request::Basic { command } => match command.as_str() {
            "ping" => {
                println!("Received a ping");
                "pong".to_string()
            }

            _ => "unknown basic command".to_string(),
        },

        Request::Backup { command, backup } => match command.as_str() {
            "new_backup" => {
                println!("Received new backup: {:?}", backup.name);
                println!("{:?}", backup);
                {
                    let mut cfg = config.write().unwrap();

                    cfg.backups.push(backup);
                }

                config_handler::save_modified_config(&config);

                "backup added".to_string()
            }

            _ => "unknown backup command".to_string(),
        },

        Request::Debug { command, argument } => match command.as_str() {
            "temp" => "temp".to_string(),
            "init" => {
                let mut return_string = "init_complete";
                let given_path = Path::new(&argument);
                if given_path.exists() {
                    if given_path.is_dir() {
                        init_directory_state::scan_to_json(given_path);
                    } else {
                        return_string = "Given path isn't a directory";
                    }
                } else {
                    return_string = "Given path doesn't exist";
                }
                return_string.to_string()
            }
            _ => "unknown debug command".to_string(),
        },
    };

    let _ = stream.write_all(response.as_bytes());
}

fn start_daemon(config: SharedConfig) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener =
            TcpListener::bind("127.0.0.1:55000").expect("Failed to bind to 127.0.0.1:55000");

        println!("RueSync daemon running on 127.0.0.1:55000");

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
            if !cfg.backups[index].task_active {
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

                    let destination_file_hash = match recheck_directory::get_directory_state(
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
                } else {
                    println!(
                        "Something went wrong in recheck directroy for {}",
                        current_backup_config.name
                    );
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
            let cfg = config.read().unwrap();
            if tasks.len() < cfg.backups.len() {
                let mut index = 0;
                for backup in &cfg.backups {
                    if !backup.task_active {
                        tasks.push(Backup_Task {
                            run_at: Instant::now(),
                            backup_index: index,
                        });
                    }
                    index += 1;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}
