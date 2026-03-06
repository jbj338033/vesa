use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use vesa_core::config::{ClientConfig, ServerConfig};
use vesa_core::{Client, Server};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    Idle,
    Server,
    Client,
}

pub struct AppState {
    pub shutdown_tx: Option<watch::Sender<bool>>,
    pub mode: RunMode,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            shutdown_tx: None,
            mode: RunMode::Idle,
        }
    }
}

#[tauri::command]
pub async fn start_server(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    bind_addr: String,
) -> Result<(), String> {
    let addr = bind_addr.parse().map_err(|e| format!("invalid address: {e}"))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    {
        let mut s = state.lock().await;
        s.shutdown_tx = Some(shutdown_tx);
        s.mode = RunMode::Server;
    }

    let config = ServerConfig {
        bind_addr: addr,
        clients: vec![],
        release_hotkey: "ScrollLock".to_string(),
    };

    let cert_dir = config_dir().join("certs");
    let mut server = Server::new(config, cert_dir);

    tokio::spawn(async move {
        if let Err(e) = server.run(shutdown_rx).await {
            tracing::error!("server error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_server(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    if let Some(tx) = s.shutdown_tx.take() {
        let _ = tx.send(true);
    }
    s.mode = RunMode::Idle;
    Ok(())
}

#[tauri::command]
pub async fn start_client(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    server_addr: String,
    position: String,
) -> Result<(), String> {
    let addr = server_addr.parse().map_err(|e| format!("invalid address: {e}"))?;
    let pos = match position.as_str() {
        "Left" => vesa_event::Position::Left,
        "Right" => vesa_event::Position::Right,
        "Top" => vesa_event::Position::Top,
        "Bottom" => vesa_event::Position::Bottom,
        _ => return Err("invalid position".to_string()),
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    {
        let mut s = state.lock().await;
        s.shutdown_tx = Some(shutdown_tx);
        s.mode = RunMode::Client;
    }

    let config = ClientConfig {
        server_addr: addr,
        position: pos,
    };

    let mut client = Client::new(config);

    tokio::spawn(async move {
        if let Err(e) = client.run(shutdown_rx).await {
            tracing::error!("client error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_client(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    if let Some(tx) = s.shutdown_tx.take() {
        let _ = tx.send(true);
    }
    s.mode = RunMode::Idle;
    Ok(())
}

#[tauri::command]
pub async fn get_status(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let s = state.lock().await;
    let status = match s.mode {
        RunMode::Idle => "idle",
        RunMode::Server => "server",
        RunMode::Client => "client",
    };
    Ok(status.to_string())
}

fn config_dir() -> PathBuf {
    dirs_home()
        .map(|h| PathBuf::from(h).join(".config").join("vesa"))
        .unwrap_or_else(|| PathBuf::from(".vesa"))
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
