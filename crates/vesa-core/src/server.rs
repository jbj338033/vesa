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

/// Release hotkey: Escape
/// macOS keycode for Escape = 53
/// Linux evdev KEY_ESC = 1
/// Windows vk_to_evdev maps VK_ESCAPE(0x1B) → 1
const RELEASE_KEY_MACOS: u32 = 53;
const RELEASE_KEY_EVDEV: u32 = 1;

pub struct Server {
    config: ServerConfig,
    state: ServerState,
    cert_dir: PathBuf,
    position_rx: watch::Receiver<Position>,
    client_connected_tx: watch::Sender<bool>,
}

impl Server {
    pub fn new(
        config: ServerConfig,
        cert_dir: PathBuf,
        position_rx: watch::Receiver<Position>,
        client_connected_tx: watch::Sender<bool>,
    ) -> Self {
        Self {
            config,
            state: ServerState::Idle,
            cert_dir,
            position_rx,
            client_connected_tx,
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

        // Read the client's handshake ping and reply with pong
        match stream.recv().await {
            Ok(Message::Ping) => {
                debug!("[server::handle] received handshake ping, sending pong");
                if let Err(e) = stream.send(&Message::Pong).await {
                    warn!("[server::handle] failed to send pong: {}", e);
                }
            }
            Ok(other) => {
                debug!("[server::handle] expected Ping, got {:?}", other);
            }
            Err(e) => {
                error!("[server::handle] failed to read handshake: {}", e);
                return;
            }
        }

        // Send AssignPosition after handshake
        let client_position = *self.position_rx.borrow();
        info!("[server::handle] assigning position {:?} to client", client_position);
        if let Err(e) = stream.send(&Message::AssignPosition(client_position)).await {
            warn!("[server::handle] failed to send AssignPosition: {}", e);
            return;
        }

        // Notify UI that a client is connected
        let _ = self.client_connected_tx.send(true);

        info!("[server::handle] stream established with {} — entering event loop", addr);
        info!("[server::handle] client_position={:?}, edge_threshold={}", client_position, EDGE_PUSH_THRESHOLD);

        let mut client_position = client_position;
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
                _ = self.position_rx.changed() => {
                    let new_pos = *self.position_rx.borrow();
                    if new_pos != client_position {
                        info!("[server::handle] position changed {:?} → {:?}", client_position, new_pos);
                        client_position = new_pos;
                        edge_push_count = 0;
                        if let Err(e) = stream.send(&Message::AssignPosition(new_pos)).await {
                            warn!("[server::handle] failed to send AssignPosition: {}", e);
                        }
                    }
                }
                Some(event) = event_rx.recv() => {
                    total_events += 1;
                    if total_events % 200 == 1 {
                        debug!("[server::handle] event #{}, state={:?}", total_events, self.state);
                    }

                    match self.state {
                        ServerState::Idle => {
                            let (cx, cy) = capture.cursor_position();
                            let (sx, sy, sw, sh) = capture.screen_bounds();

                            if let Some(push_dir) = detect_edge_push(&event, cx, cy, sx, sy, sw, sh) {
                                if push_dir == client_position {
                                    edge_push_count += 1;
                                    debug!(
                                        "[server::edge] push {:?} count={}/{} cursor=({:.0},{:.0}) screen=({:.0},{:.0},{:.0},{:.0})",
                                        push_dir, edge_push_count, EDGE_PUSH_THRESHOLD, cx, cy, sx, sy, sw, sh
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
                                self.leave_capture(capture, &mut stream, 0.5).await;
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
                result = stream.recv() => {
                    match result {
                        Ok(Message::Leave(y_ratio)) => {
                            info!("[server::handle] client requested return (edge push), y_ratio={:.3}", y_ratio);
                            if self.state != ServerState::Idle {
                                self.leave_capture(capture, &mut stream, y_ratio).await;
                            }
                        }
                        Ok(msg) => {
                            debug!("[server::handle] received stream message from client: {:?}", msg);
                        }
                        Err(e) => {
                            warn!("[server::handle] stream read error: {}", e);
                            if self.state != ServerState::Idle {
                                self.leave_capture(capture, &mut stream, 0.5).await;
                            }
                            break;
                        }
                    }
                }
                result = conn.read_datagram() => {
                    match result {
                        Ok(msg) => debug!("[server::handle] received datagram from client: {:?}", msg),
                        Err(e) => {
                            warn!("[server::handle] client {} disconnected: {}", addr, e);
                            if self.state != ServerState::Idle {
                                self.leave_capture(capture, &mut stream, 0.5).await;
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Notify UI that client disconnected
        let _ = self.client_connected_tx.send(false);

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
        y_ratio: f64,
    ) {
        info!("[server::capture] <<< LEAVING capture mode, y_ratio={:.3}", y_ratio);

        // Compute the cursor warp position on the return edge BEFORE releasing capture
        let target = match &self.state {
            ServerState::Capturing { target } => *target,
            _ => Position::Right,
        };
        let (sx, sy, sw, sh) = capture.screen_bounds();
        let warp_y = sy + sh * y_ratio.clamp(0.0, 1.0);
        let warp_x = match target {
            Position::Right => sx + sw - 1.0,
            Position::Left => sx + 1.0,
            Position::Bottom => sx + sw * 0.5,
            Position::Top => sx + sw * 0.5,
        };
        // For top/bottom, also apply y from the edge
        let warp_y = match target {
            Position::Top => sy + 1.0,
            Position::Bottom => sy + sh - 1.0,
            _ => warp_y,
        };

        self.state = ServerState::Idle;
        capture.set_capturing(false);
        capture.warp_cursor(warp_x, warp_y);
        debug!(
            "[server::capture] cursor warped to ({:.0},{:.0}), sending Leave message...",
            warp_x, warp_y
        );

        if let Err(e) = stream.send(&Message::Leave(y_ratio)).await {
            warn!("[server::capture] failed to send Leave message: {}", e);
        } else {
            debug!("[server::capture] Leave message sent successfully");
        }

        info!("[server::capture] input capture released");
    }
}

/// Pixels from screen edge within which we consider the cursor "at the edge".
const EDGE_MARGIN: f64 = 2.0;

/// Detect if a mouse motion event is pushing against a screen edge.
/// Requires: (1) cursor is within EDGE_MARGIN of the screen boundary AND
/// (2) the delta continues pushing toward that boundary.
fn detect_edge_push(
    event: &InputEvent,
    cursor_x: f64,
    cursor_y: f64,
    screen_x: f64,
    screen_y: f64,
    screen_w: f64,
    screen_h: f64,
) -> Option<Position> {
    if let InputEvent::PointerMotion { dx, dy, .. } = event {
        let adx = dx.abs();
        let ady = dy.abs();

        if adx < 0.5 && ady < 0.5 {
            return None;
        }

        // Check if cursor is at each edge and delta pushes toward it
        let at_right = cursor_x >= screen_x + screen_w - EDGE_MARGIN;
        let at_left = cursor_x <= screen_x + EDGE_MARGIN;
        let at_bottom = cursor_y >= screen_y + screen_h - EDGE_MARGIN;
        let at_top = cursor_y <= screen_y + EDGE_MARGIN;

        if at_right && *dx > 0.0 && adx > ady {
            return Some(Position::Right);
        }
        if at_left && *dx < 0.0 && adx > ady {
            return Some(Position::Left);
        }
        if at_bottom && *dy > 0.0 && ady > adx {
            return Some(Position::Bottom);
        }
        if at_top && *dy < 0.0 && ady > adx {
            return Some(Position::Top);
        }
    }
    None
}

/// Check if an input event is the release hotkey (Escape).
fn is_release_hotkey(event: &InputEvent) -> bool {
    if let InputEvent::KeyboardKey { key, state, .. } = event {
        if matches!(state, vesa_event::KeyState::Press) {
            // Accept both macOS keycode and evdev code for ScrollLock
            return *key == RELEASE_KEY_MACOS || *key == RELEASE_KEY_EVDEV;
        }
    }
    false
}
