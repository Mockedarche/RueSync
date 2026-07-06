use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::Serialize;
use walkdir::WalkDir;

/*
 * FileInfo - Struct
 * Expects: Valid file metadata from scan_directory
 * Does: Stores file path, size, modified time, and blake3 hash
 * Returns: N/A (data container)
 */
#[derive(Serialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    pub hash: String,
}

/*
 * system_time_to_u64 - Function
 * Expects: Valid SystemTime input
 * Does: Converts SystemTime into UNIX timestamp (seconds)
 * Returns: u64 timestamp
 */
fn system_time_to_u64(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/*
 * hash_file - Function
 * Expects: Valid readable file path
 * Does: Streams file and computes blake3 hash
 * Returns: Result containing hash or IO error
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
 * scan_directory - Function
 * Expects: Valid directory path
 * Does: Walks directory tree, hashes each file, collects metadata
 * Returns: HashMap mapping file path → FileInfo
 */
fn scan_directory(root: &Path) -> HashMap<String, FileInfo> {
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
 * scan_to_json - Function (MAIN ENTRY POINT)
 * Expects: Valid directory path
 * Does: Scans directory, hashes directory path into filename,
 *       writes snapshot JSON into ./state/ folder outside src
 * Returns: N/A (writes file and prints stats)
 */
pub fn scan_to_json(root: &Path) {
    let start = std::time::Instant::now();

    let data = scan_directory(root);

    println!("Files: {}", data.len());
    println!("Scan time: {:?}", start.elapsed());

    // --- create state folder outside src ---
    let state_dir = PathBuf::from("state");
    let _ = fs::create_dir_all(&state_dir);

    // --- hash absolute path for filename ---
    let abs = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let hash = blake3::hash(abs.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();

    let json_path = state_dir.join(format!("{}.json", hash));

    // --- write snapshot ---
    let json = serde_json::to_string_pretty(&data).unwrap();
    let _ = fs::write(json_path, json);
}
