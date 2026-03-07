use tokio::sync::watch;
use tracing::{debug, error, info, trace, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::VesaClient as NetClient;
use vesa_proto::Message;

use crate::config::ClientConfig;
use crate::edge::{EDGE_PUSH_THRESHOLD, detect_edge_push};

const DEFAULT_POSITION: Position = Position::Right;

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

    pub async fn run(&mut self, mut shutdown_rx: watch::Receiver<bool>) -> Result<(), ClientError> {
        info!("[client] starting, server_addr={}", self.config.server_addr);

        let bind_addr = "0.0.0.0:0".parse().unwrap();
        debug!("[client] connecting to server...");
        let conn = NetClient::connect(self.config.server_addr, bind_addr).await?;
        self.state = ClientState::Connected;
        info!(
            "[client] connected to server at {}",
            self.config.server_addr
        );

        let mut emulate = vesa_emulate::create_emulate()?;
        debug!("[client] emulate backend created");

        let mut stream = conn.open_stream().await?;
        debug!("[client] stream opened, sending handshake...");

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

        let mut position = match stream.recv().await {
            Ok(Message::AssignPosition(pos)) => {
                info!("[client] server assigned position: {:?}", pos);
                pos
            }
            Ok(other) => {
                warn!(
                    "[client] expected AssignPosition, got {:?} — using default",
                    other
                );
                DEFAULT_POSITION
            }
            Err(e) => {
                error!("[client] failed to receive position assignment: {}", e);
                return Err(ClientError::Net(e));
            }
        };

        let mut datagram_count: u64 = 0;
        let mut event_count: u64 = 0;

        let mut return_direction = position.opposite();
        let mut edge_push_count: u32 = 0;

        info!(
            "[client] position={:?}, return_direction={:?}",
            position, return_direction
        );

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
                            if datagram_count % 500 == 1 {
                                trace!("[client] datagram #{}: {:?}, state={:?}", datagram_count, msg, self.state);
                            }
                            if let Some(event) = msg.to_input_event() {
                                if self.state == ClientState::Active {
                                    event_count += 1;

                                    if let Err(e) = emulate.emit(event) {
                                        warn!("[client] failed to emit event #{}: {}", event_count, e);
                                    }

                                    let (cx, cy) = emulate.cursor_position();
                                    let (sx, sy, sw, sh) = emulate.screen_bounds();
                                    if let Some(dir) = detect_edge_push(&event, cx, cy, sx, sy, sw, sh) {
                                        if dir == return_direction {
                                            edge_push_count += 1;
                                            trace!(
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

        info!(
            "[client] shutting down, total datagrams received: {}",
            datagram_count
        );
        emulate.destroy();
        conn.close();
        self.state = ClientState::Disconnected;
        Ok(())
    }
}
