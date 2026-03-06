use tokio::sync::watch;
use tracing::{debug, error, info, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::VesaClient as NetClient;
use vesa_proto::Message;

use crate::config::ClientConfig;

/// Default position used before server assigns one.
const DEFAULT_POSITION: Position = Position::Right;

/// Same threshold as server for consistency.
const EDGE_PUSH_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    Disconnected,
    Connected,
    Active,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("emulate error: {0}")]
    Emulate(#[from] vesa_emulate::EmulateError),
    #[error("net error: {0}")]
    Net(#[from] vesa_net::NetError),
}

pub struct Client {
    config: ClientConfig,
    state: ClientState,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            state: ClientState::Disconnected,
        }
    }

    pub fn state(&self) -> &ClientState {
        &self.state
    }

    pub async fn run(
        &mut self,
        mut shutdown_rx: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        info!("[client] starting, server_addr={}", self.config.server_addr);

        let bind_addr = "0.0.0.0:0".parse().unwrap();
        debug!("[client] connecting to server...");
        let conn = NetClient::connect(self.config.server_addr, bind_addr).await?;
        self.state = ClientState::Connected;
        info!("[client] connected to server at {}", self.config.server_addr);

        debug!("[client] creating emulate backend...");
        let mut emulate = vesa_emulate::create_emulate()?;
        info!("[client] emulate backend created successfully");

        debug!("[client] opening bi-directional stream to server...");
        let mut stream = conn.open_stream().await?;
        info!("[client] stream opened, sending initial handshake...");

        // Send a Ping to materialize the QUIC stream — open_bi() is local-only
        // and the server's accept_bi() won't fire until data is actually sent.
        stream.send(&Message::Ping).await.map_err(|e| {
            error!("[client] failed to send handshake ping: {}", e);
            ClientError::Net(e)
        })?;
        debug!("[client] handshake ping sent, waiting for pong...");

        match stream.recv().await {
            Ok(Message::Pong) => {
                debug!("[client] pong received, waiting for position assignment...");
            }
            Ok(other) => {
                warn!("[client] expected Pong, got {:?}", other);
            }
            Err(e) => {
                error!("[client] handshake failed: {}", e);
                return Err(ClientError::Net(e));
            }
        }

        // Wait for server to assign our position
        let mut position = match stream.recv().await {
            Ok(Message::AssignPosition(pos)) => {
                info!("[client] server assigned position: {:?}", pos);
                pos
            }
            Ok(other) => {
                warn!("[client] expected AssignPosition, got {:?} — using default", other);
                DEFAULT_POSITION
            }
            Err(e) => {
                error!("[client] failed to receive position assignment: {}", e);
                return Err(ClientError::Net(e));
            }
        };

        let mut datagram_count: u64 = 0;
        let mut event_count: u64 = 0;

        // The return edge is opposite to our position relative to the server.
        // e.g. if we are to the Right of the server, pushing Left returns control.
        let mut return_direction = position.opposite();
        let mut edge_push_count: u32 = 0;

        info!("[client] position={:?}, return_direction={:?}", position, return_direction);

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("[client] shutdown signal received");
                        break;
                    }
                }
                result = conn.read_datagram() => {
                    match result {
                        Ok(msg) => {
                            datagram_count += 1;
                            if datagram_count % 100 == 1 {
                                debug!("[client] datagram #{}: {:?}, state={:?}", datagram_count, msg, self.state);
                            }
                            if let Some(event) = msg.to_input_event() {
                                if self.state == ClientState::Active {
                                    event_count += 1;

                                    // Check edge push AFTER emulating so cursor position reflects this event
                                    if let Err(e) = emulate.emit(event) {
                                        warn!("[client] failed to emit event #{}: {}", event_count, e);
                                    }

                                    let (cx, cy) = emulate.cursor_position();
                                    let (sx, sy, sw, sh) = emulate.screen_bounds();
                                    if let Some(dir) = detect_edge_push(&event, cx, cy, sx, sy, sw, sh) {
                                        if dir == return_direction {
                                            edge_push_count += 1;
                                            debug!(
                                                "[client::edge] return push {:?} count={}/{} cursor=({:.0},{:.0})",
                                                dir, edge_push_count, EDGE_PUSH_THRESHOLD, cx, cy
                                            );
                                            if edge_push_count >= EDGE_PUSH_THRESHOLD {
                                                info!("[client::edge] THRESHOLD — requesting return to server");
                                                let y_ratio = if sh > 0.0 { (cy - sy) / sh } else { 0.5 };
                                                let y_ratio = y_ratio.clamp(0.0, 1.0);
                                                if let Err(e) = stream.send(&Message::Leave(y_ratio)).await {
                                                    warn!("[client] failed to send Leave: {}", e);
                                                }
                                                self.state = ClientState::Connected;
                                                edge_push_count = 0;
                                                event_count = 0;
                                                continue;
                                            }
                                        } else {
                                            edge_push_count = 0;
                                        }
                                    } else if matches!(event, InputEvent::PointerMotion { .. }) {
                                        edge_push_count = 0;
                                    }
                                } else {
                                    debug!("[client] ignoring event, state={:?}", self.state);
                                }
                            }
                        }
                        Err(e) => {
                            error!("[client] datagram read error: {}", e);
                            break;
                        }
                    }
                }
                result = stream.recv() => {
                    match result {
                        Ok(Message::Enter(pos)) => {
                            info!("[client] >>> ENTER from {:?} — activating input emulation", pos);
                            self.state = ClientState::Active;
                            edge_push_count = 0;
                        }
                        Ok(Message::Leave(_)) => {
                            info!("[client] <<< LEAVE — deactivating input emulation (emitted {} events)", event_count);
                            self.state = ClientState::Connected;
                            event_count = 0;
                            edge_push_count = 0;
                        }
                        Ok(Message::AssignPosition(pos)) => {
                            info!("[client] position reassigned: {:?} → {:?}", position, pos);
                            position = pos;
                            return_direction = pos.opposite();
                            edge_push_count = 0;
                        }
                        Ok(msg) => {
                            debug!("[client] received stream message: {:?}", msg);
                        }
                        Err(e) => {
                            error!("[client] stream read error: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        info!("[client] shutting down, total datagrams received: {}", datagram_count);
        emulate.destroy();
        conn.close();
        self.state = ClientState::Disconnected;
        Ok(())
    }
}

const EDGE_MARGIN: f64 = 2.0;

/// Detect if cursor is at a screen edge and being pushed toward it.
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
