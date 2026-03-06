use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use vesa_core::config::{ClientConfig, ServerConfig};
use vesa_core::{Client, Server};
use vesa_event::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    Idle,
    Server,
    Client,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
    pub position: String,
}

pub struct AppState {
    pub shutdown_tx: Option<watch::Sender<bool>>,
    pub mode: RunMode,
    pub position_tx: Option<watch::Sender<Position>>,
    pub client_connected_rx: Option<watch::Receiver<bool>>,
    pub client_position: Position,
    pub client_connected: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            shutdown_tx: None,
            mode: RunMode::Idle,
            position_tx: None,
            client_connected_rx: None,
            client_position: Position::Right,
            client_connected: false,
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
    let (position_tx, position_rx) = watch::channel(Position::Right);
    let (client_connected_tx, client_connected_rx) = watch::channel(false);

    {
        let mut s = state.lock().await;
        s.shutdown_tx = Some(shutdown_tx);
        s.mode = RunMode::Server;
        s.position_tx = Some(position_tx);
        s.client_connected_rx = Some(client_connected_rx);
        s.client_position = Position::Right;
        s.client_connected = false;
    }

    let config = ServerConfig {
        bind_addr: addr,
        clients: vec![],
        release_hotkey: "ScrollLock".to_string(),
    };

    let cert_dir = config_dir().join("certs");
    let mut server = Server::new(config, cert_dir, position_rx, client_connected_tx);

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
    s.position_tx = None;
    s.client_connected_rx = None;
    s.client_connected = false;
    Ok(())
}

#[tauri::command]
pub async fn start_client(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    server_addr: String,
) -> Result<(), String> {
    let addr = server_addr.parse().map_err(|e| format!("invalid address: {e}"))?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    {
        let mut s = state.lock().await;
        s.shutdown_tx = Some(shutdown_tx);
        s.mode = RunMode::Client;
    }

    let config = ClientConfig {
        server_addr: addr,
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

#[tauri::command]
pub async fn get_clients(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<ClientInfo>, String> {
    let mut s = state.lock().await;

    // Check if client connection state has changed
    if let Some(ref mut rx) = s.client_connected_rx {
        // Non-blocking check for updates
        if rx.has_changed().unwrap_or(false) {
            s.client_connected = *rx.borrow_and_update();
        }
    }

    if s.client_connected {
        let pos = s.client_position;
        Ok(vec![ClientInfo {
            id: "client-1".to_string(),
            name: "Client".to_string(),
            position: format!("{pos:?}"),
        }])
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
pub async fn set_client_position(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    position: String,
) -> Result<(), String> {
    let pos = match position.as_str() {
        "Left" => Position::Left,
        "Right" => Position::Right,
        "Top" => Position::Top,
        "Bottom" => Position::Bottom,
        _ => return Err(format!("invalid position: {position}")),
    };

    let mut s = state.lock().await;
    s.client_position = pos;
    if let Some(ref tx) = s.position_tx {
        let _ = tx.send(pos);
    }
    Ok(())
}

fn config_dir() -> PathBuf {
    dirs_home()
        .map(|h| PathBuf::from(h).join(".config").join("vesa"))
        .unwrap_or_else(|| PathBuf::from(".vesa"))
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
