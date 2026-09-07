use std::{
    fs,
    sync::{Arc, RwLock},
    thread,
};

use axum::{Json, Router, extract::State, response::Html, routing::get};
use serde::Serialize;

use crate::{
    config_handler::Config,
    runtime_state::{RuntimeState, SharedRuntimeState},
};

type SharedConfig = Arc<RwLock<Config>>;

#[derive(Clone)]
struct AppState {
    config: SharedConfig,
    runtime_state: SharedRuntimeState,
}

pub fn start(config: SharedConfig, runtime_state: SharedRuntimeState) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run(config, runtime_state);
    })
}

fn run(config: SharedConfig, runtime_state: SharedRuntimeState) {
    println!("Starting RueSync web server");

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async move {
        let state = AppState {
            config,
            runtime_state,
        };

        let app = Router::new()
            .route("/", get(dashboard))
            .route("/api/runtime", get(runtime_info))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
            .await
            .expect("Failed to bind web server");

        println!("RueSync web server running on http://0.0.0.0:8080");

        axum::serve(listener, app).await.expect("Web server failed");
    });
}

async fn dashboard(State(state): State<AppState>) -> Html<String> {
    let template = fs::read_to_string("web/dashboard.html").expect("Failed to read dashboard.html");

    // Config is only read here to build the initial page.
    let backups_html = {
        let config = state.config.read().unwrap();

        if config.backups.is_empty() {
            "<p>No backups added.</p>".to_string()
        } else {
            config
                .backups
                .iter()
                .map(|backup| {
                    format!(
                        r#"
                        <div class="card" data-backup-id="{}">
                            <h3>{}</h3>
                            <p>Source: {}</p>
                            <p>Destination: {}</p>

                            <p>
                                Status:
                                <span class="status runtime-status">{}</span>
                            </p>
                        </div>
                        "#,
                        backup.unique_id,
                        backup.name,
                        backup.source_directory,
                        backup.destination_directory,
                        if backup.enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                    )
                })
                .collect::<String>()
        }
    };

    let html = template.replace("{{BACKUPS}}", &backups_html);

    Html(html)
}

async fn runtime_info(State(state): State<AppState>) -> Json<RuntimeState> {
    // Runtime state is what gets accessed repeatedly for live information.
    let runtime_state = state.runtime_state.read().unwrap();

    Json(runtime_state.clone())
}
