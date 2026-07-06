/*
 * Made by Austin Lunbeck
 * Early development stages for a easy to use data backup software
 * Currently all prototypes for working through the basic pieces of software AKA
 * RIGHT NOW THIS ISN'T FUNCTIONAL and is purely educational
 *
 */

use std::{
    env, fs,
    path::{Path, PathBuf},
};

mod init_directory_state;
mod recheck_directory;

fn clear_state_folder() {
    let state_dir = Path::new("state");

    if !state_dir.exists() {
        println!("state folder does not exist");
        return;
    }

    match fs::read_dir(state_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_file() {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to read state folder: {}", e);
        }
    }
}

fn print_changes(changes: recheck_directory::ChangeSet) {
    if !changes.added.is_empty() {
        println!("Added: {:?}", changes.added);
    } else {
        println!("No files added");
    }

    if !changes.removed.is_empty() {
        println!("Removed: {:?}", changes.removed);
    } else {
        println!("No files removed");
    }

    if !changes.modified.is_empty() {
        println!("Modified: {:?}", changes.modified);
    } else {
        println!("No files modified");
    }
}

fn debug_mode(args: &[String]) {
    // example: --debug delete-config <path>
    match args.get(2).map(|s| s.as_str()) {
        Some("delete-config") => {
            if let Some(path) = args.get(3) {
                let p = PathBuf::from(path);

                if p.exists() {
                    let _ = fs::remove_file(&p);
                    println!("Deleted config: {}", p.display());
                } else {
                    println!("Config not found: {}", p.display());
                }
            } else {
                println!("Missing path for delete-config");
            }
        }

        Some("reset") => {
            println!("Debug reset mode");
            clear_state_folder();
        }

        _ => {
            println!("Unknown debug command");
            println!("Available: delete-config <path>, reset");
        }
    }
}

fn create_config() {}

fn load_config() {}

fn main() {
    let config_path = Path::new("config.json");

    if config_path.exists() {
        load_config();
    } else {
        create_config();
    }

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage:");
        println!("  init <dir> <json>");
        println!("  check <dir> <json>");
        println!("  --debug <command> ...");
        return;
    }

    match args[1].as_str() {
        "init" => {
            let root = PathBuf::from(&args[2]);

            println!("Initializing state...");
            init_directory_state::scan_to_json(&root);
        }

        "check" => {
            let root = PathBuf::from(&args[2]);

            println!("Running directory check...");

            match recheck_directory::check_directory(&root) {
                Ok(changes) => {
                    print_changes(changes);
                }
                Err(e) => {
                    eprintln!("Check failed: {:?}", e);
                }
            }
        }

        "--debug" => {
            debug_mode(&args);
        }

        _ => {
            println!("Unknown command: {}", args[1]);
            println!("Use: init | check | --debug");
        }
    }
}
