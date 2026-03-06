use std::path::PathBuf;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::cert::Identity;
use vesa_net::{VesaConnection, VesaServer as NetServer, VesaStream};
use vesa_proto::Message;

use crate::config::ServerConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    Idle,
    Capturing { target: Position },
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("capture error: {0}")]
    Capture(#[from] vesa_capture::CaptureError),
    #[error("net error: {0}")]
    Net(#[from] vesa_net::NetError),
    #[error("cert error: {0}")]
    Cert(#[from] vesa_net::cert::CertError),
    #[error("no clients configured")]
    NoClients,
}

/// Threshold of consecutive edge-pushing motion events before switching.
const EDGE_PUSH_THRESHOLD: u32 = 3;

/// macOS keycode for ScrollLock (there isn't a standard one; use F15 = 0x71 = 113)
/// Linux evdev KEY_SCROLLLOCK = 70
const RELEASE_KEY_MACOS: u32 = 113;
const RELEASE_KEY_EVDEV: u32 = 70;

pub struct Server {
    config: ServerConfig,
    state: ServerState,
    cert_dir: PathBuf,
}

impl Server {
    pub fn new(config: ServerConfig, cert_dir: PathBuf) -> Self {
        Self {
            config,
            state: ServerState::Idle,
            cert_dir,
        }
    }

    pub fn state(&self) -> &ServerState {
        &self.state
    }

    pub async fn run(
        &mut self,
        mut shutdown_rx: watch::Receiver<bool>,
    ) -> Result<(), ServerError> {
        let identity = Identity::load_or_generate(&self.cert_dir)?;
        info!(
            "certificate fingerprint: {:02x?}",
            &identity.fingerprint()[..8]
        );

        let server = NetServer::bind(self.config.bind_addr, &identity)?;
        info!("server listening on {}", self.config.bind_addr);

        let mut capture = vesa_capture::create_capture()?;
        let mut event_rx = capture.start()?;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("server shutting down");
                        break;
                    }
                }
                Some(conn) = server.accept() => {
                    info!("client connected from {}", conn.remote_address());
                    self.handle_client(conn, &mut event_rx, &mut capture, &mut shutdown_rx).await;
                }
                Some(_event) = event_rx.recv() => {
                    // Events while no client connected — discard
                }
            }
        }

        capture.stop()?;
        server.close();
        Ok(())
    }

    async fn handle_client(
        &mut self,
        conn: VesaConnection,
        event_rx: &mut mpsc::Receiver<InputEvent>,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        let addr = conn.remote_address();

        let mut stream = match conn.accept_stream().await {
            Ok(s) => s,
            Err(e) => {
                error!("failed to accept stream from {}: {}", addr, e);
                return;
            }
        };

        info!("stream established with {}", addr);

        // Determine which screen edge triggers switch to this client.
        // Use config if available, otherwise default to Right.
        let client_position = self
            .config
            .clients
            .first()
            .map(|c| c.position)
            .unwrap_or(vesa_event::Position::Right);

        let mut edge_push_count: u32 = 0;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                Some(event) = event_rx.recv() => {
                    match self.state {
                        ServerState::Idle => {
                            // Check for edge push to enter capture mode
                            if let Some(push_dir) = detect_edge_push(&event) {
                                if push_dir == client_position {
                                    edge_push_count += 1;
                                    if edge_push_count >= EDGE_PUSH_THRESHOLD {
                                        self.enter_capture(client_position, capture, &mut stream).await;
                                        edge_push_count = 0;
                                    }
                                } else {
                                    edge_push_count = 0;
                                }
                            } else {
                                edge_push_count = 0;
                            }

                            // Check for release hotkey
                            // (no-op in Idle, but reset edge count on any key press)
                        }
                        ServerState::Capturing { .. } => {
                            // Check release hotkey
                            if is_release_hotkey(&event) {
                                self.leave_capture(capture, &mut stream).await;
                                continue;
                            }

                            // Forward event to client via datagram
                            let msg = Message::from_input_event(&event);
                            if let Err(e) = conn.send_datagram(&msg) {
                                warn!("failed to send datagram: {}", e);
                            }
                        }
                    }
                }
                result = conn.read_datagram() => {
                    match result {
                        Ok(msg) => debug!("received datagram from client: {:?}", msg),
                        Err(e) => {
                            warn!("client {} disconnected: {}", addr, e);
                            if self.state != ServerState::Idle {
                                self.leave_capture(capture, &mut stream).await;
                            }
                            break;
                        }
                    }
                }
            }
        }

        conn.close();
    }

    async fn enter_capture(
        &mut self,
        target: Position,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        stream: &mut VesaStream,
    ) {
        self.state = ServerState::Capturing { target };
        capture.set_capturing(true);

        // Send Enter message to client via reliable stream
        if let Err(e) = stream.send(&Message::Enter(target)).await {
            warn!("failed to send Enter message: {}", e);
        }

        info!("capturing input → {:?}", target);
    }

    async fn leave_capture(
        &mut self,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        stream: &mut VesaStream,
    ) {
        self.state = ServerState::Idle;
        capture.set_capturing(false);

        // Send Leave message to client via reliable stream
        if let Err(e) = stream.send(&Message::Leave).await {
            warn!("failed to send Leave message: {}", e);
        }

        info!("released input capture");
    }
}

/// Detect if a mouse motion event is pushing against a screen edge.
/// Returns the direction being pushed (Left/Right/Top/Bottom) if the cursor
/// is at the edge and the delta continues in that direction.
///
/// For low-level capture hooks, the cursor is warped to center when capturing,
/// but when Idle the cursor moves freely. We detect edge pushing by checking
/// if the delta is strongly directional (the cursor is stuck at the edge,
/// so the OS reports no actual movement, but the raw delta from the device
/// still shows the intended direction).
fn detect_edge_push(event: &InputEvent) -> Option<Position> {
    if let InputEvent::PointerMotion { dx, dy, .. } = event {
        let adx = dx.abs();
        let ady = dy.abs();

        // Only consider clearly directional movements
        if adx < 1.0 && ady < 1.0 {
            return None;
        }

        // Horizontal push
        if adx > ady * 2.0 {
            if *dx > 0.0 {
                return Some(Position::Right);
            } else {
                return Some(Position::Left);
            }
        }

        // Vertical push
        if ady > adx * 2.0 {
            if *dy > 0.0 {
                return Some(Position::Bottom);
            } else {
                return Some(Position::Top);
            }
        }
    }
    None
}

/// Check if an input event is the release hotkey (ScrollLock).
fn is_release_hotkey(event: &InputEvent) -> bool {
    if let InputEvent::KeyboardKey { key, state, .. } = event {
        if matches!(state, vesa_event::KeyState::Press) {
            // Accept both macOS keycode and evdev code for ScrollLock
            return *key == RELEASE_KEY_MACOS || *key == RELEASE_KEY_EVDEV;
        }
    }
    false
}
