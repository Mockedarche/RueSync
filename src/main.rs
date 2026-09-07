use std::sync::{Arc, RwLock};

use RueSync::{
    backup_daemon, config_handler,
    runtime_state::{RuntimeState, SharedRuntimeState},
    tcp_server, web_server,
};

type SharedConfig = Arc<RwLock<config_handler::Config>>;

fn main() {
    println!("Starting RueSync");

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Handle reset immediately and exit.
    if args.contains(&"--reset".to_string()) {
        if args.len() != 1 {
            eprintln!("Error: --reset cannot be combined with other arguments.");
            return;
        }

        reset_ruesync();
        return;
    }

    let run_tcp = args.iter().any(|arg| arg == "--tcp");
    let run_web = args.iter().any(|arg| arg == "--web");
    let run_daemon = args.iter().any(|arg| arg == "--daemon");

    if !run_tcp && !run_web && !run_daemon {
        println!("Usage:");
        println!("  ruesync --tcp");
        println!("  ruesync --web");
        println!("  ruesync --daemon");
        println!("  ruesync --tcp --web --daemon");
        println!("  ruesync --reset");
        return;
    }

    let config: SharedConfig = Arc::new(RwLock::new(config_handler::init()));

    let runtime_state: SharedRuntimeState = Arc::new(RwLock::new(RuntimeState::default()));

    if run_tcp {
        {
            let mut state = runtime_state.write().unwrap();
            state.tcp_server_active = true;
        }
        tcp_server::start(Arc::clone(&config));
    }

    if run_web {
        {
            let mut state = runtime_state.write().unwrap();
            state.web_server_active = true;
        }
        web_server::start(Arc::clone(&config), Arc::clone(&runtime_state));
    }

    if run_daemon {
        {
            let mut state = runtime_state.write().unwrap();
            state.daemon_active = true;
        }
        backup_daemon::start(Arc::clone(&config), Arc::clone(&runtime_state));
    }

    loop {
        std::thread::park();
    }
}

fn reset_ruesync() {
    println!("Resetting RueSync...");

    for directory in ["local_state", "lan_state", "wan_state", "state"] {
        match std::fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();

                            let result = if path.is_dir() {
                                std::fs::remove_dir_all(&path)
                            } else {
                                std::fs::remove_file(&path)
                            };

                            if let Err(e) = result {
                                eprintln!("Failed to delete {:?}: {}", path, e);
                            } else {
                                println!("Deleted {:?}", path);
                            }
                        }

                        Err(e) => {
                            eprintln!("Failed reading {}: {}", directory, e);
                        }
                    }
                }
            }

            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("{} does not exist", directory);
            }

            Err(e) => {
                eprintln!("Failed to read {}: {}", directory, e);
            }
        }
    }

    match std::fs::remove_file("config.json") {
        Ok(()) => println!("Deleted config.json"),

        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("config.json does not exist");
        }

        Err(e) => {
            eprintln!("Failed to delete config.json: {}", e);
        }
    }

    println!("RueSync reset complete.");
}
