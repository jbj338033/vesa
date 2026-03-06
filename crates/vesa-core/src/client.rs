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
        let bind_addr = "0.0.0.0:0".parse().unwrap();
        let conn = NetClient::connect(self.config.server_addr, bind_addr).await?;
        self.state = ClientState::Connected;
        info!("connected to server at {}", self.config.server_addr);

        let mut emulate = vesa_emulate::create_emulate()?;
        let mut stream = conn.open_stream().await?;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("client shutting down");
                        break;
                    }
                }
                result = conn.read_datagram() => {
                    match result {
                        Ok(msg) => {
                            if let Some(event) = msg.to_input_event()
                                && self.state == ClientState::Active
                                && let Err(e) = emulate.emit(event)
                            {
                                warn!("failed to emit event: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("datagram read error: {}", e);
                            break;
                        }
                    }
                }
                result = stream.recv() => {
                    match result {
                        Ok(Message::Enter(pos)) => {
                            info!("server entered from {:?}", pos);
                            self.state = ClientState::Active;
                        }
                        Ok(Message::Leave) => {
                            info!("server left");
                            self.state = ClientState::Connected;
                        }
                        Ok(msg) => {
                            debug!("received stream message: {:?}", msg);
                        }
                        Err(e) => {
                            error!("stream read error: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        emulate.destroy();
        conn.close();
        self.state = ClientState::Disconnected;
        Ok(())
    }
}
