use crate::config_handler::BackupConfig;
use crate::recheck_directory::FileHash;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

fn copy_with_limit(src: &str, dst: &str, max_bytes_per_second: u64) -> std::io::Result<()> {
    let mut input = File::open(src)?;
    let mut output = File::create(dst)?;

    let mut buffer = [0u8; 64 * 1024];
    let start = Instant::now();
    let mut bytes_written = 0u64;

    loop {
        let n = input.read(&mut buffer)?;

        if n == 0 {
            break;
        }

        output.write_all(&buffer[..n])?;

        bytes_written += n as u64;

        let expected = Duration::from_secs_f64(bytes_written as f64 / max_bytes_per_second as f64);

        let elapsed = start.elapsed();

        if expected > elapsed {
            thread::sleep(expected - elapsed);
        }
    }

    Ok(())
}

fn copy_file(current_backup: &BackupConfig, relative_path: &str) -> std::io::Result<()> {
    let source_path = PathBuf::from(&current_backup.source_directory).join(relative_path);

    let destination_path = PathBuf::from(&current_backup.destination_directory).join(relative_path);

    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    println!(
        "Copying {} -> {}",
        source_path.display(),
        destination_path.display()
    );

    copy_with_limit(
        source_path.to_str().unwrap(),
        destination_path.to_str().unwrap(),
        current_backup.source_bandwidth_cap_in_bytes,
    )
}

fn remove_empty_directories(root: &PathBuf) {
    for entry in WalkDir::new(root)
        .contents_first(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();

        if path.is_dir() && path != root {
            match fs::remove_dir(path) {
                Ok(_) => println!("Removed empty directory: {}", path.display()),
                Err(_) => {}
            }
        }
    }
}

pub fn local_backup(
    current_backup: &BackupConfig,
    source_file_hash: Vec<FileHash>,
    destination_file_hash: Vec<FileHash>,
) {
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
            Some(destination_hash) => {
                if *destination_hash != source_file.hash {
                    println!("Changed: {}", source_file.path);
                    true
                } else {
                    false
                }
            }

            None => {
                println!("New file: {}", source_file.path);
                true
            }
        };

        if should_copy {
            if let Err(e) = copy_file(&current_backup, &source_file.path) {
                eprintln!("Failed copying {}: {}", source_file.path, e);
            }
        }
    }

    for destination_file in &destination_file_hash {
        if !source_map.contains_key(destination_file.path.as_str()) {
            let stale_file =
                PathBuf::from(&current_backup.destination_directory).join(&destination_file.path);

            println!("Deleting stale file: {}", stale_file.display());

            if let Err(e) = fs::remove_file(&stale_file) {
                eprintln!("Failed deleting {}: {}", stale_file.display(), e);
            }
        }
    }
    let destination_root = PathBuf::from(&current_backup.destination_directory);

    remove_empty_directories(&destination_root);
}
