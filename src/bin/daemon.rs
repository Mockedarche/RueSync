use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use RueSync::config_handler::{self, BackupConfig, Config};
use RueSync::init_directory_state;
use RueSync::recheck_directory;
use RueSync::requests::Request;

use serde::{Deserialize, Serialize};

type SharedConfig = Arc<RwLock<Config>>;

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

    let request: Request = match serde_json::from_str(&msg) {
        Ok(req) => req,
        Err(e) => {
            let _ = stream.write_all(format!("invalid request: {e}").as_bytes());
            return;
        }
    };

    let response = match request {
        Request::Basic { command } => match command.as_str() {
            "ping" => "pong".to_string(),

            _ => "unknown basic command".to_string(),
        },

        Request::Backup { command, backup } => match command.as_str() {
            "new_backup" => {
                println!("Received new backup: {:?}", backup.name);

                let mut cfg = config.write().unwrap();
                cfg.backups.push(backup);

                "backup added".to_string()
            }

            _ => "unknown backup command".to_string(),
        },

        Request::Debug { command, argument } => match command.as_str() {
            "temp" => "temp".to_string(),
            _ => "unknown debug command".to_string(),
        },
    };

    let _ = stream.write_all(response.as_bytes());
}

fn start_daemon(config: Config) -> std::io::Result<()> {
    let config: SharedConfig = Arc::new(RwLock::new(config));

    let listener = TcpListener::bind("127.0.0.1:55000")?;
    println!("RueSync daemon running on 127.0.0.1:55000");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let config = Arc::clone(&config);

                thread::spawn(move || {
                    handle_client(stream, config);
                });
            }
            Err(e) => eprintln!("connection failed: {e}"),
        }
    }

    Ok(())
}

fn main() {
    println!("Starting RueSync daemon");

    let config: Config = config_handler::init();
    println!("Config loaded");

    thread::sleep(Duration::from_millis(200));

    if let Err(e) = start_daemon(config) {
        eprintln!("daemon crashed: {e:?}");
    }
}
