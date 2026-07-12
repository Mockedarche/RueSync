use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::config_handler::BackupLocation;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHash {
    pub path: String,
    pub hash: String,
}

#[derive(Debug)]
pub enum ScanError {
    SnapshotMissing(String),
    IoError(String),
    DeserializeError(String),
}

fn system_time_to_u64(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hash_file(path: &Path) -> io::Result<blake3::Hash> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();

    let mut buffer = [0u8; 64 * 1024];

    loop {
        let size = file.read(&mut buffer)?;

        if size == 0 {
            break;
        }

        hasher.update(&buffer[..size]);
    }

    Ok(hasher.finalize())
}

fn snapshot_path_for(root: &Path, state_path: &Path) -> PathBuf {
    let absolute = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let hash = blake3::hash(absolute.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();

    fs::create_dir_all(state_path).unwrap();

    PathBuf::from(state_path).join(format!("{}.json", hash))
}

fn load_snapshot(path: &Path) -> Result<HashMap<String, FileInfo>, ScanError> {
    let file = File::open(path).map_err(|e| ScanError::SnapshotMissing(e.to_string()))?;

    serde_json::from_reader(file).map_err(|e| ScanError::DeserializeError(e.to_string()))
}

fn save_snapshot(path: &Path, data: &HashMap<String, FileInfo>) {
    let json = serde_json::to_string_pretty(data).unwrap();

    fs::write(path, json).unwrap();
}

fn scan_directory(root: &Path, previous: &HashMap<String, FileInfo>) -> HashMap<String, FileInfo> {
    let mut current = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();

        let relative_path = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let size = metadata.len();

        let modified = metadata
            .modified()
            .ok()
            .map(system_time_to_u64)
            .unwrap_or(0);

        if let Some(old) = previous.get(&relative_path) {
            if old.size == size && old.modified == modified {
                current.insert(relative_path.clone(), old.clone());

                continue;
            }
        }

        let hash = match hash_file(path) {
            Ok(h) => h.to_hex().to_string(),
            Err(_) => continue,
        };

        current.insert(
            relative_path.clone(),
            FileInfo {
                path: relative_path,
                size,
                modified,
                hash,
            },
        );
    }

    current
}

pub fn get_directory_state(root: &Path, state_path: &Path) -> Result<Vec<FileHash>, ScanError> {
    let snapshot = snapshot_path_for(root, state_path);

    let old = if snapshot.exists() {
        load_snapshot(&snapshot)?
    } else {
        HashMap::new()
    };

    let new = scan_directory(root, &old);

    save_snapshot(&snapshot, &new);

    let result = new
        .into_values()
        .map(|file| FileHash {
            path: file.path,
            hash: file.hash,
        })
        .collect();

    Ok(result)
}
