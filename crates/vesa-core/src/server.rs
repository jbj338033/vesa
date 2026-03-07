use std::path::PathBuf;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, trace, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::cert::Identity;
use vesa_net::{VesaConnection, VesaServer as NetServer, VesaStream};
use vesa_proto::Message;

use crate::config::ServerConfig;
use crate::edge::{EDGE_PUSH_THRESHOLD, detect_edge_push};

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

    pub async fn run(&mut self, mut shutdown_rx: watch::Receiver<bool>) -> Result<(), ServerError> {
        info!("[server] starting, bind_addr={}", self.config.bind_addr);

        let identity = Identity::load_or_generate(&self.cert_dir)?;
        info!(
            "[server] certificate fingerprint: {:02x?}",
            &identity.fingerprint()[..8]
        );

        let server = NetServer::bind(self.config.bind_addr, &identity)?;
        info!("[server] listening on {}", self.config.bind_addr);

        let mut capture = vesa_capture::create_capture()?;
        let mut event_rx = capture.start()?;
        debug!("[server] input capture started");

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
                        trace!("[server] idle event #{} (no client connected, discarding)", idle_event_count);
                    }
                }
            }
        }

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

        let mut stream = match conn.accept_stream().await {
            Ok(s) => {
                debug!("[server::handle] stream accepted from {}", addr);
                s
            }
            Err(e) => {
                error!(
                    "[server::handle] FAILED to accept stream from {}: {}",
                    addr, e
                );
                return;
            }
        };

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

        let client_position = *self.position_rx.borrow();
        info!(
            "[server::handle] assigning position {:?} to client",
            client_position
        );
        if let Err(e) = stream.send(&Message::AssignPosition(client_position)).await {
            warn!("[server::handle] failed to send AssignPosition: {}", e);
            return;
        }

        let _ = self.client_connected_tx.send(true);

        info!(
            "[server::handle] session started with {}, position={:?}",
            addr, client_position
        );

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
                    if total_events % 500 == 1 {
                        trace!("[server::handle] event #{}, state={:?}", total_events, self.state);
                    }

                    match self.state {
                        ServerState::Idle => {
                            let (cx, cy) = capture.cursor_position();
                            let (sx, sy, sw, sh) = capture.screen_bounds();

                            if let Some(push_dir) = detect_edge_push(&event, cx, cy, sx, sy, sw, sh) {
                                if push_dir == client_position {
                                    edge_push_count += 1;
                                    trace!(
                                        "[server::edge] push {:?} count={}/{} cursor=({:.0},{:.0})",
                                        push_dir, edge_push_count, EDGE_PUSH_THRESHOLD, cx, cy
                                    );
                                    if edge_push_count >= EDGE_PUSH_THRESHOLD {
                                        info!("[server::edge] THRESHOLD REACHED — entering capture mode");
                                        self.enter_capture(client_position, capture, &mut stream).await;
                                        edge_push_count = 0;
                                    }
                                } else {
                                    edge_push_count = 0;
                                }
                            } else if matches!(event, InputEvent::PointerMotion { .. }) {
                                edge_push_count = 0;
                            }
                        }
                        ServerState::Capturing { .. } => {
                            if is_release_hotkey(&event) {
                                info!("[server::capture] release hotkey pressed, leaving capture");
                                self.leave_capture(capture, &mut stream, 0.5).await;
                                continue;
                            }

                            forwarded_events += 1;
                            let msg = Message::from_input_event(&event);
                            if let Err(e) = conn.send_datagram(&msg) {
                                warn!("[server::capture] failed to send datagram #{}: {}", forwarded_events, e);
                            } else if forwarded_events % 500 == 1 {
                                trace!("[server::capture] forwarded event #{}: {:?}", forwarded_events, msg);
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
        info!(
            "[server::capture] entering capture mode, target={:?}",
            target
        );
        self.state = ServerState::Capturing { target };
        capture.set_capturing(true);

        if let Err(e) = stream.send(&Message::Enter(target)).await {
            warn!("[server::capture] failed to send Enter message: {}", e);
        }
    }

    async fn leave_capture(
        &mut self,
        capture: &mut Box<dyn vesa_capture::InputCapture>,
        stream: &mut VesaStream,
        y_ratio: f64,
    ) {
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
        let warp_y = match target {
            Position::Top => sy + 1.0,
            Position::Bottom => sy + sh - 1.0,
            _ => warp_y,
        };

        self.state = ServerState::Idle;
        capture.set_capturing(false);
        capture.warp_cursor(warp_x, warp_y);

        if let Err(e) = stream.send(&Message::Leave(y_ratio)).await {
            warn!("[server::capture] failed to send Leave message: {}", e);
        }

        info!(
            "[server::capture] leaving capture mode, y_ratio={:.3}",
            y_ratio
        );
    }
}

fn is_release_hotkey(event: &InputEvent) -> bool {
    if let InputEvent::KeyboardKey { key, state, .. } = event {
        if matches!(state, vesa_event::KeyState::Press) {
            return *key == RELEASE_KEY_MACOS || *key == RELEASE_KEY_EVDEV;
        }
    }
    false
}
