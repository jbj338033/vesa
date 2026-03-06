use tokio::sync::watch;
use tracing::{debug, error, info, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::VesaClient as NetClient;
use vesa_proto::Message;

use crate::config::ClientConfig;

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
        info!("[client] starting, server_addr={}, position={:?}", self.config.server_addr, self.config.position);

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
                info!("[client] handshake complete, entering event loop");
            }
            Ok(other) => {
                warn!("[client] expected Pong, got {:?}", other);
            }
            Err(e) => {
                error!("[client] handshake failed: {}", e);
                return Err(ClientError::Net(e));
            }
        }

        let mut datagram_count: u64 = 0;
        let mut event_count: u64 = 0;

        // The return edge is opposite to our position relative to the server.
        // e.g. if we are to the Right of the server, pushing Left returns control.
        let return_direction = self.config.position.opposite();
        let mut edge_push_count: u32 = 0;

        info!("[client] position={:?}, return_direction={:?}", self.config.position, return_direction);

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

                                    // Detect return edge push before emulating
                                    if let Some(dir) = detect_edge_push(&event) {
                                        if dir == return_direction {
                                            edge_push_count += 1;
                                            debug!(
                                                "[client::edge] return push {:?} count={}/{}",
                                                dir, edge_push_count, EDGE_PUSH_THRESHOLD
                                            );
                                            if edge_push_count >= EDGE_PUSH_THRESHOLD {
                                                info!("[client::edge] THRESHOLD — requesting return to server");
                                                if let Err(e) = stream.send(&Message::Leave).await {
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

                                    if let Err(e) = emulate.emit(event) {
                                        warn!("[client] failed to emit event #{}: {}", event_count, e);
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
                        Ok(Message::Leave) => {
                            info!("[client] <<< LEAVE — deactivating input emulation (emitted {} events)", event_count);
                            self.state = ClientState::Connected;
                            event_count = 0;
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

/// Detect directional edge push from a pointer motion event.
fn detect_edge_push(event: &InputEvent) -> Option<Position> {
    if let InputEvent::PointerMotion { dx, dy, .. } = event {
        let adx = dx.abs();
        let ady = dy.abs();

        if adx < 0.5 && ady < 0.5 {
            return None;
        }

        if adx > ady * 1.5 {
            return if *dx > 0.0 {
                Some(Position::Right)
            } else {
                Some(Position::Left)
            };
        }

        if ady > adx * 1.5 {
            return if *dy > 0.0 {
                Some(Position::Bottom)
            } else {
                Some(Position::Top)
            };
        }

        if adx >= ady && *dx > 0.0 {
            Some(Position::Right)
        } else if adx >= ady {
            Some(Position::Left)
        } else if *dy > 0.0 {
            Some(Position::Bottom)
        } else {
            Some(Position::Top)
        }
    } else {
        None
    }
}
