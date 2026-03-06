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
        info!("[server] starting, bind_addr={}", self.config.bind_addr);
        debug!("[server] config: {:?}", self.config);

        let identity = Identity::load_or_generate(&self.cert_dir)?;
        info!(
            "[server] certificate fingerprint: {:02x?}",
            &identity.fingerprint()[..8]
        );

        let server = NetServer::bind(self.config.bind_addr, &identity)?;
        info!("[server] listening on {}", self.config.bind_addr);

        debug!("[server] creating input capture backend...");
        let mut capture = vesa_capture::create_capture()?;
        info!("[server] input capture backend created");

        debug!("[server] starting input capture...");
        let mut event_rx = capture.start()?;
        info!("[server] input capture started, waiting for connections...");

        let mut idle_event_count: u64 = 0;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("[server] shutdown signal received");
                        break;
                    }
                }
                Some(conn) = server.accept() => {
                    info!("[server] === CLIENT CONNECTED from {} ===", conn.remote_address());
                    debug!("[server] entering handle_client, discarded {} idle events so far", idle_event_count);
                    self.handle_client(conn, &mut event_rx, &mut capture, &mut shutdown_rx).await;
                    info!("[server] === CLIENT DISCONNECTED, back to waiting ===");
                }
                Some(_event) = event_rx.recv() => {
                    idle_event_count += 1;
                    if idle_event_count % 500 == 1 {
                        debug!("[server] idle event #{} (no client connected, discarding)", idle_event_count);
                    }
                }
            }
        }

        info!("[server] stopping capture...");
        capture.stop()?;
        server.close();
        info!("[server] stopped");
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
        info!("[server::handle] waiting for bi-directional stream from {}...", addr);

        let mut stream = match conn.accept_stream().await {
            Ok(s) => {
                info!("[server::handle] stream accepted from {}", addr);
                s
            }
            Err(e) => {
                error!("[server::handle] FAILED to accept stream from {}: {}", addr, e);
                return;
            }
        };

        info!("[server::handle] stream established with {} — entering event loop", addr);

        // Determine which screen edge triggers switch to this client.
        let client_position = self
            .config
            .clients
            .first()
            .map(|c| c.position)
            .unwrap_or(vesa_event::Position::Right);

        info!("[server::handle] client_position={:?}, edge_threshold={}", client_position, EDGE_PUSH_THRESHOLD);

        let mut edge_push_count: u32 = 0;
        let mut total_events: u64 = 0;
        let mut forwarded_events: u64 = 0;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("[server::handle] shutdown during client session");
                        break;
                    }
                }
                Some(event) = event_rx.recv() => {
                    total_events += 1;
                    if total_events % 200 == 1 {
                        debug!("[server::handle] event #{}, state={:?}", total_events, self.state);
                    }

                    match self.state {
                        ServerState::Idle => {
                            // Check for edge push to enter capture mode
                            if let Some(push_dir) = detect_edge_push(&event) {
                                if push_dir == client_position {
                                    edge_push_count += 1;
                                    debug!(
                                        "[server::edge] push {:?} count={}/{} (event: dx/dy from {:?})",
                                        push_dir, edge_push_count, EDGE_PUSH_THRESHOLD, event
                                    );
                                    if edge_push_count >= EDGE_PUSH_THRESHOLD {
                                        info!("[server::edge] THRESHOLD REACHED — entering capture mode");
                                        self.enter_capture(client_position, capture, &mut stream).await;
                                        edge_push_count = 0;
                                    }
                                } else {
                                    if edge_push_count > 0 {
                                        debug!(
                                            "[server::edge] reset: got {:?}, wanted {:?} (count was {})",
                                            push_dir, client_position, edge_push_count
                                        );
                                    }
                                    edge_push_count = 0;
                                }
                            } else if matches!(event, InputEvent::PointerMotion { .. }) {
                                if edge_push_count > 0 {
                                    debug!("[server::edge] reset: non-directional motion (count was {})", edge_push_count);
                                }
                                edge_push_count = 0;
                            }
                        }
                        ServerState::Capturing { .. } => {
                            // Check release hotkey
                            if is_release_hotkey(&event) {
                                info!("[server::capture] release hotkey pressed, leaving capture");
                                self.leave_capture(capture, &mut stream).await;
                                continue;
                            }

                            // Forward event to client via datagram
                            forwarded_events += 1;
                            let msg = Message::from_input_event(&event);
                            if let Err(e) = conn.send_datagram(&msg) {
                                warn!("[server::capture] failed to send datagram #{}: {}", forwarded_events, e);
                            } else if forwarded_events % 100 == 1 {
                                debug!("[server::capture] forwarded event #{}: {:?}", forwarded_events, msg);
                            }
                        }
                    }
                }
                result = conn.read_datagram() => {
                    match result {
                        Ok(msg) => debug!("[server::handle] received datagram from client: {:?}", msg),
                        Err(e) => {
                            warn!("[server::handle] client {} disconnected: {}", addr, e);
                            if self.state != ServerState::Idle {
                                self.leave_capture(capture, &mut stream).await;
                            }
                            break;
                        }
                    }
                }
            }
        }

        info!(
            "[server::handle] session ended: total_events={}, forwarded={}",
            total_events, forwarded_events
        );
        conn.close();
    }

    async fn enter_capture(
        &mut self,
        target: Position,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        stream: &mut VesaStream,
    ) {
        info!("[server::capture] >>> ENTERING capture mode, target={:?}", target);
        self.state = ServerState::Capturing { target };
        capture.set_capturing(true);
        debug!("[server::capture] set_capturing(true), sending Enter message...");

        if let Err(e) = stream.send(&Message::Enter(target)).await {
            warn!("[server::capture] failed to send Enter message: {}", e);
        } else {
            debug!("[server::capture] Enter message sent successfully");
        }

        info!("[server::capture] now capturing input → {:?}", target);
    }

    async fn leave_capture(
        &mut self,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        stream: &mut VesaStream,
    ) {
        info!("[server::capture] <<< LEAVING capture mode");
        self.state = ServerState::Idle;
        capture.set_capturing(false);
        debug!("[server::capture] set_capturing(false), sending Leave message...");

        if let Err(e) = stream.send(&Message::Leave).await {
            warn!("[server::capture] failed to send Leave message: {}", e);
        } else {
            debug!("[server::capture] Leave message sent successfully");
        }

        info!("[server::capture] input capture released");
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

        // Ignore tiny movements
        if adx < 0.5 && ady < 0.5 {
            return None;
        }

        // Determine dominant direction — relaxed to 1.5x ratio
        // Horizontal push
        if adx > ady * 1.5 {
            if *dx > 0.0 {
                return Some(Position::Right);
            } else {
                return Some(Position::Left);
            }
        }

        // Vertical push
        if ady > adx * 1.5 {
            if *dy > 0.0 {
                return Some(Position::Bottom);
            } else {
                return Some(Position::Top);
            }
        }

        // Diagonal — pick dominant axis even without strong ratio
        if adx >= ady && *dx > 0.0 {
            return Some(Position::Right);
        } else if adx >= ady {
            return Some(Position::Left);
        } else if *dy > 0.0 {
            return Some(Position::Bottom);
        } else {
            return Some(Position::Top);
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
