use crate::{
    config_handler::BackupConfig, config_handler::BackupLocation, recheck_directory,
    recheck_directory::FileHash, requests::Request,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Write},
    net::TcpStream,
    path::Path,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Success { message: String },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteBackupRequest {
    BeginBackup {
        backup_id: String,
    },

    BeginFile {
        relative_path: String,
        file_size: u64,
    },

    FileChunk {
        data: Vec<u8>,
    },

    EndFile,

    DeleteFile {
        relative_path: String,
    },

    CleanupDirectories,

    EndBackup,

    // Responses from receiver
    Ready,

    GotFile {
        relative_path: String,
    },

    Error {
        message: String,
    },
}

pub fn send_request<T: Serialize>(stream: &mut TcpStream, request: &T) -> io::Result<()> {
    let json = serde_json::to_vec(request)?;

    let size = json.len() as u32;

    stream.write_all(&size.to_be_bytes())?;
    stream.write_all(&json)?;

    Ok(())
}

pub fn receive_request<T: DeserializeOwned>(stream: &mut TcpStream) -> io::Result<T> {
    let mut size_buffer = [0u8; 4];

    stream.read_exact(&mut size_buffer)?;

    let size = u32::from_be_bytes(size_buffer) as usize;

    let mut buffer = vec![0u8; size];

    stream.read_exact(&mut buffer)?;

    Ok(serde_json::from_slice(&buffer)?)
}

fn send_file(
    stream: &mut TcpStream,
    current_backup: &BackupConfig,
    relative_path: &str,
) -> io::Result<()> {
    let source_path = PathBuf::from(&current_backup.source_directory).join(relative_path);

    let mut file = File::open(&source_path)?;

    let file_size = file.metadata()?.len();

    send_request(
        stream,
        &RemoteBackupRequest::BeginFile {
            relative_path: relative_path.to_string(),
            file_size,
        },
    )?;

    let bandwidth_limit = current_backup
        .source_bandwidth_cap_in_bytes
        .min(current_backup.destination_bandwidth_cap_in_bytes);

    let mut buffer = [0u8; 64 * 1024];

    let start = Instant::now();
    let mut bytes_sent = 0u64;

    loop {
        let size = file.read(&mut buffer)?;

        if size == 0 {
            break;
        }

        send_request(
            stream,
            &RemoteBackupRequest::FileChunk {
                data: buffer[..size].to_vec(),
            },
        )?;

        bytes_sent += size as u64;

        if bandwidth_limit > 0 {
            let expected = Duration::from_secs_f64(bytes_sent as f64 / bandwidth_limit as f64);

            if expected > start.elapsed() {
                thread::sleep(expected - start.elapsed());
            }
        }
    }

    send_request(stream, &RemoteBackupRequest::EndFile)?;

    match receive_request::<RemoteBackupRequest>(stream)? {
        RemoteBackupRequest::GotFile {
            relative_path: path,
        } => {
            println!("Destination received {}", path);
            Ok(())
        }

        RemoteBackupRequest::Error { message } => {
            Err(io::Error::new(io::ErrorKind::Other, message))
        }

        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected GotFile",
        )),
    }
}

pub fn remote_backup(
    current_backup: &BackupConfig,
    source_file_hash: Vec<FileHash>,
    destination_file_hash: Vec<FileHash>,
) -> io::Result<()> {
    // Ask destination daemon on port 55000 to prepare receiving
    let control_address = format!("{}:{}", current_backup.network_information.address, 55000);

    println!("Connecting to control port {}", control_address);

    let mut control_stream = TcpStream::connect(control_address)?;

    let request = Request::Backup {
        command: "receive_backup".to_string(),
        backup: current_backup.clone(),
    };

    let json = serde_json::to_vec(&request)?;

    control_stream.write_all(&json)?;

    let mut response_buffer = [0u8; 4096];

    let size = control_stream.read(&mut response_buffer)?;

    if size == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Destination closed control connection",
        ));
    }

    let response: Response = serde_json::from_slice(&response_buffer[..size])?;

    match response {
        Response::Success { message } => {
            println!("Destination response: {}", message);

            if message != "backup receiver ready" {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Destination refused backup",
                ));
            }
        }

        Response::Error { message } => {
            return Err(io::Error::new(io::ErrorKind::Other, message));
        }
    }

    // Connect to dedicated backup port
    let backup_address = format!("{}:{}", current_backup.network_information.address, 55001);

    println!("Connecting to backup port {}", backup_address);

    let mut stream = TcpStream::connect(backup_address)?;

    println!("Connected to backup port");

    send_request(
        &mut stream,
        &RemoteBackupRequest::BeginBackup {
            backup_id: current_backup.unique_id.clone(),
        },
    )?;

    println!("Sent BeginBackup");

    match receive_request::<RemoteBackupRequest>(&mut stream)? {
        RemoteBackupRequest::Ready => {
            println!("Destination ready");
        }

        RemoteBackupRequest::Error { message } => {
            return Err(io::Error::new(io::ErrorKind::Other, message));
        }

        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Expected Ready, got {:?}", other),
            ));
        }
    }

    let destination_map: HashMap<&str, &str> = destination_file_hash
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect();

    let source_map: HashMap<&str, &str> = source_file_hash
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect();

    for source_file in &source_file_hash {
        let should_copy = match destination_map.get(source_file.path.as_str()) {
            Some(destination_hash) => *destination_hash != source_file.hash,
            None => true,
        };

        if should_copy {
            println!("Sending {}", source_file.path);

            send_file(&mut stream, current_backup, &source_file.path)?;
        }
    }

    for destination_file in &destination_file_hash {
        if !source_map.contains_key(destination_file.path.as_str()) {
            println!("Deleting {}", destination_file.path);

            send_request(
                &mut stream,
                &RemoteBackupRequest::DeleteFile {
                    relative_path: destination_file.path.clone(),
                },
            )?;
        }
    }

    send_request(&mut stream, &RemoteBackupRequest::CleanupDirectories)?;

    send_request(&mut stream, &RemoteBackupRequest::EndBackup)?;

    println!("Remote backup complete");

    Ok(())
}

fn create_file_path(current_backup: &BackupConfig, relative_path: &str) -> io::Result<File> {
    let destination_path = PathBuf::from(&current_backup.destination_directory).join(relative_path);

    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    println!("Creating file {}", destination_path.display());

    File::create(destination_path)
}

fn remove_empty_directories(root: &PathBuf) {
    for entry in WalkDir::new(root)
        .contents_first(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();

        if path.is_dir() && path != root {
            if fs::remove_dir(path).is_ok() {
                println!("Removed directory {}", path.display());
            }
        }
    }
}

pub fn receive_backup(stream: &mut TcpStream, current_backup: &BackupConfig) -> io::Result<()> {
    let mut current_file: Option<File> = None;
    let mut current_file_path: Option<String> = None;

    println!("receive_backup started");

    loop {
        let request: RemoteBackupRequest = receive_request(stream)?;

        //println!("{:?}", request);
        match &request {
            RemoteBackupRequest::BeginFile {
                relative_path,
                file_size,
            } => {
                println!("Receiving {} ({} bytes)", relative_path, file_size);
            }

            RemoteBackupRequest::EndFile => {
                println!("File complete");
            }

            RemoteBackupRequest::DeleteFile { relative_path } => {
                println!("Deleting {}", relative_path);
            }

            RemoteBackupRequest::CleanupDirectories => {
                println!("Cleaning directories");
            }

            RemoteBackupRequest::EndBackup => {
                println!("Backup finished");
            }

            _ => {}
        }

        match request {
            RemoteBackupRequest::BeginFile {
                relative_path,
                file_size,
            } => {
                println!("Receiving {} ({} bytes)", relative_path, file_size);

                current_file_path = Some(relative_path.clone());

                current_file = Some(create_file_path(current_backup, &relative_path)?);
            }

            RemoteBackupRequest::FileChunk { data } => match current_file.as_mut() {
                Some(file) => {
                    file.write_all(&data)?;
                }

                None => {
                    send_request(
                        stream,
                        &RemoteBackupRequest::Error {
                            message: "Received chunk without active file".into(),
                        },
                    )?;

                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Chunk without active file",
                    ));
                }
            },

            RemoteBackupRequest::EndFile => {
                if let Some(mut file) = current_file.take() {
                    file.flush()?;
                }

                let path = current_file_path
                    .take()
                    .unwrap_or_else(|| "unknown".to_string());

                println!("Finished receiving {}", path);

                send_request(
                    stream,
                    &RemoteBackupRequest::GotFile {
                        relative_path: path,
                    },
                )?;
            }

            RemoteBackupRequest::DeleteFile { relative_path } => {
                let path =
                    PathBuf::from(&current_backup.destination_directory).join(&relative_path);

                println!("Deleting {}", path.display());

                if let Err(e) = fs::remove_file(&path) {
                    eprintln!("Failed deleting {}: {}", path.display(), e);
                }
            }

            RemoteBackupRequest::CleanupDirectories => {
                println!("Cleaning empty directories");

                remove_empty_directories(&PathBuf::from(&current_backup.destination_directory));
            }

            RemoteBackupRequest::EndBackup => {
                println!("Backup finished");

                break;
            }

            RemoteBackupRequest::BeginBackup { .. } => {
                send_request(
                    stream,
                    &RemoteBackupRequest::Error {
                        message: "BeginBackup already handled".into(),
                    },
                )?;
            }

            RemoteBackupRequest::Ready
            | RemoteBackupRequest::GotFile { .. }
            | RemoteBackupRequest::Error { .. } => {}
        }
    }

    Ok(())
}
