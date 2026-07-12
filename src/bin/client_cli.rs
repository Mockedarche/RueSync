use std::{
    char::MAX,
    env,
    io::{self, Read, Write},
    net::TcpStream,
};
use uuid::Uuid;

use RueSync::{
    config_handler::{BackupConfig, BackupLocation, NetworkConfig, NetworkConfigBackup},
    requests::Request,
};
use serde_json;

fn prompt(message: &str) -> String {
    let mut input = String::new();

    println!("{}", message);

    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");

    input.trim().to_string()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage:");
        println!("  client <ip:port> ping");
        println!("  client <ip:port> new_backup <args>");
        return;
    }

    let addr = &args[1];
    let command = &args[2];

    let request = match command.as_str() {
        "-ping" => Request::Basic {
            command: "ping".to_string(),
        },

        "-check" => {
            if args.len() < 4 {
                println!("Usage: client <ip:port> check <directory>");
                return;
            }

            Request::Basic {
                command: format!("check {}", args[3]),
            }
        }

        "-init" => {
            if args.len() < 4 {
                println!("Usage: client <ip:port> init <directory>");
                return;
            }

            Request::Basic {
                command: format!("init {}", args[3]),
            }
        }

        "-new_backup" => {
            let mut backups_name: String;
            let mut backup_enabled: bool;
            let mut source_directory_backup: String;
            let mut destination_directory_backup: String;
            let mut location_type: BackupLocation;
            let mut network_info: NetworkConfigBackup;
            let mut source_bandwith_cap_in_bytes_backup: u64 = 0;
            let mut destination_bandwith_cap_in_bytes_backup: u64 = 0;
            let mut backup_interval_in_seconds_backup: u64 = 0;
            let mut data_validated = false;
            /*
            backups_name = prompt("Please enter the backups name: ");
            let temp = prompt("(T/F) enable this backup: ");
            if temp.to_lowercase() == "t" {
                backup_enabled = true;
            } else {
                backup_enabled = false;
            }
            source_directory_backup = prompt("Please enter the source directory: ");
            destination_directory_backup = prompt("Please enter the destination directory: ");

            let mut input = prompt("Please enter the upload cap in bytes: ");

            let source_bandwidth_cap_in_bytes = match input.trim().parse::<u64>() {
                Ok(number) => number,
                Err(_) => {
                    println!("Invalid number");
                    return;
                }
            };

            //println!("Upload cap: {}", source_bandwidth_cap_in_bytes);
            let mut input = prompt("Please enter the download cap in bytes: ");

            let destination_bandwidth_cap_in_bytes = match input.trim().parse::<u64>() {
                Ok(number) => number,
                Err(_) => {
                    println!("Invalid number");
                    return;
                }
            };

            let mut input = prompt("Please enter the backup interval in seconds: ");

            let backup_interval_in_seconds_backup = match input.trim().parse::<u64>() {
                Ok(number) => number,
                Err(_) => {
                    println!("Invalid number");
                    return;
                }
            };

            let mut backup_config = BackupConfig {
                name: backups_name,
                enabled: backup_enabled,
                source_directory: source_directory_backup,
                destination_directory: destination_directory_backup,
                local_lan_wan: BackupLocation::Lan,
                network_information: NetworkConfigBackup {
                    address: "1222".to_string(),
                    port: 1222,
                },
                source_bandwidth_cap_in_bytes: source_bandwith_cap_in_bytes_backup,
                destination_bandwidth_cap_in_bytes: destination_bandwith_cap_in_bytes_backup,
                backup_interval_in_seconds: backup_interval_in_seconds_backup,
            };
            */

            let id = Uuid::new_v4().to_string();

            let backup_config = BackupConfig {
                name: "test".to_string(),
                unique_id: id,
                source_of_backup: true,
                enabled: true,
                source_directory: "TEMP".to_string(),
                destination_directory: "TEMP".to_string(),
                local_lan_wan: BackupLocation::Lan,
                network_information: NetworkConfigBackup {
                    address: "TEMP".to_string(),
                    port: 55001,
                },
                source_bandwidth_cap_in_bytes: u64::MAX,
                destination_bandwidth_cap_in_bytes: u64::MAX,
                backup_interval_in_seconds: 2,
                task_active: false,
            };

            Request::Backup {
                command: "new_backup".to_string(),
                backup: backup_config,
            }
        }
        "-debug" => {
            let debug_command = args[3].as_str();
            match debug_command {
                "init" => Request::Debug {
                    command: "init".to_string(),
                    argument: args[4].clone(),
                },
                _ => {
                    println!("Invalid debug command/structure");
                    return;
                }
            }
        }
        _ => {
            println!("Unknown command: {}", command);
            return;
        }
    };

    let json = match serde_json::to_string(&request) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to serialize request: {}", e);
            return;
        }
    };

    match TcpStream::connect(addr) {
        Ok(mut stream) => {
            if let Err(e) = stream.write_all(json.as_bytes()) {
                eprintln!("Write error: {}", e);
                return;
            }

            let mut buffer = [0u8; 1024];

            match stream.read(&mut buffer) {
                Ok(size) => {
                    println!("{}", String::from_utf8_lossy(&buffer[..size]));
                }

                Err(e) => eprintln!("Read error: {}", e),
            }
        }

        Err(e) => eprintln!("Connection failed: {}", e),
    }
}
