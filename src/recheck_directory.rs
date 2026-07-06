use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/*
 * FileInfo - Struct
 * Expects: Valid file metadata collected from scan_directory
 * Does: Stores file path, size, modification time, and blake3 hash
 * Returns: N/A (data container)
 */
#[derive(Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    pub hash: String,
}

/*
 * ChangeSet - Struct
 * Expects: Result of comparing directory state snapshots
 * Does: Stores added, removed, and modified file paths
 * Returns: N/A (data container)
 */
#[derive(Debug)]
pub struct ChangeSet {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

/*
 * ScanError - Enum
 * Expects: Failures during snapshot loading or scanning
 * Does: Represents IO or parsing errors
 * Returns: Error information to caller
 */
#[derive(Debug)]
pub enum ScanError {
    SnapshotMissing(String),
    IoError(String),
    DeserializeError(String),
}

/*
 * system_time_to_u64 - Function
 * Expects: Valid SystemTime value
 * Does: Converts SystemTime into UNIX timestamp seconds
 * Returns: u64 timestamp
 */
fn system_time_to_u64(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/*
 * hash_file - Function
 * Expects: Valid file path
 * Does: Streams file and computes blake3 hash
 * Returns: Resulting hash or IO error
 */
fn hash_file(path: &Path) -> io::Result<blake3::Hash> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize())
}

/*
 * snapshot_path_for - Function
 * Expects: Valid directory path
 * Does: Creates stable snapshot filename from hashed absolute path
 * Returns: Path to state file inside ./state/
 */
fn snapshot_path_for(root: &Path) -> PathBuf {
    let abs = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let hash = blake3::hash(abs.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();

    let _ = fs::create_dir_all("state");

    PathBuf::from("state").join(format!("{hash}.json"))
}

/*
 * load_snapshot - Function
 * Expects: Valid snapshot file path
 * Does: Loads previous scan state from disk
 * Returns: HashMap or error if missing/invalid
 */
fn load_snapshot(path: &Path) -> Result<HashMap<String, FileInfo>, ScanError> {
    let file = File::open(path).map_err(|e| ScanError::SnapshotMissing(e.to_string()))?;

    serde_json::from_reader(file).map_err(|e| ScanError::DeserializeError(e.to_string()))
}

/*
 * save_snapshot - Function
 * Expects: Valid snapshot path and data
 * Does: Writes current state to disk as JSON
 * Returns: N/A (silent failure)
 */
fn save_snapshot(path: &Path, data: &HashMap<String, FileInfo>) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = fs::write(path, json);
    }
}

/*
 * scan_directory - Function
 * Expects: Valid directory and previous snapshot
 * Does: Walks directory, reuses hash if size + mtime match, otherwise rehashes
 * Returns: New directory state
 */
fn scan_directory(root: &Path, previous: &HashMap<String, FileInfo>) -> HashMap<String, FileInfo> {
    let mut out = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();

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

        if let Some(old) = previous.get(&path_str) {
            if old.size == size && old.modified == modified {
                out.insert(path_str.clone(), old.clone());
                continue;
            }
        }

        let hash = match hash_file(path) {
            Ok(h) => h.to_hex().to_string(),
            Err(_) => continue,
        };

        out.insert(
            path_str.clone(),
            FileInfo {
                path: path_str,
                size,
                modified,
                hash,
            },
        );
    }

    out
}

/*
 * check_directory - Function
 * Expects: Valid directory path
 * Does: Loads snapshot, scans directory, compares states, returns differences
 * Returns: ChangeSet or error if snapshot missing
 */
pub fn check_directory(root: &Path) -> Result<ChangeSet, ScanError> {
    let snapshot_path = snapshot_path_for(root);

    let old = load_snapshot(&snapshot_path)?;

    let start = Instant::now();
    let new = scan_directory(root, &old);

    let old_keys: HashSet<_> = old.keys().collect();
    let new_keys: HashSet<_> = new.keys().collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for k in new_keys.difference(&old_keys) {
        added.push((*k).to_string());
    }

    for k in old_keys.difference(&new_keys) {
        removed.push((*k).to_string());
    }

    for k in new_keys.intersection(&old_keys) {
        if let (Some(o), Some(n)) = (old.get(*k), new.get(*k)) {
            if o.hash != n.hash {
                modified.push((*k).to_string());
            }
        }
    }

    println!("Files scanned: {}", new.len());
    println!("Scan time: {:?}", start.elapsed());

    let changed = !added.is_empty() || !removed.is_empty() || !modified.is_empty();

    if changed {
        println!("Changes detected");
    } else {
        println!("No changes detected");
    }

    save_snapshot(&snapshot_path, &new);

    Ok(ChangeSet {
        added,
        removed,
        modified,
    })
}
