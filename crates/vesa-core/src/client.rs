use tokio::sync::watch;
use tracing::{debug, error, info, warn};
use vesa_net::VesaClient as NetClient;
use vesa_proto::Message;

use crate::config::ClientConfig;

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
        info!("[client] stream opened successfully, entering event loop");

        let mut datagram_count: u64 = 0;
        let mut event_count: u64 = 0;

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
                        }
                        Ok(Message::Leave) => {
                            info!("[client] <<< LEAVE — deactivating input emulation (emitted {} events)", event_count);
                            self.state = ClientState::Connected;
                            event_count = 0;
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
