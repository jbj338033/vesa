use std::path::PathBuf;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};
use vesa_event::{InputEvent, Position};
use vesa_net::cert::Identity;
use vesa_net::{VesaConnection, VesaServer as NetServer};
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
                    self.handle_client(conn, &mut event_rx, &mut shutdown_rx).await;
                }
                Some(event) = event_rx.recv() => {
                    debug!("event while idle: {:?}", event);
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
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        let addr = conn.remote_address();

        let _stream = match conn.accept_stream().await {
            Ok(s) => s,
            Err(e) => {
                error!("failed to accept stream from {}: {}", addr, e);
                return;
            }
        };

        info!("stream established with {}", addr);

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
                            debug!("event while idle (client connected): {:?}", event);
                        }
                        ServerState::Capturing { .. } => {
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
                            break;
                        }
                    }
                }
            }
        }

        conn.close();
    }

    pub fn enter_capture(&mut self, target: Position) {
        self.state = ServerState::Capturing { target };
        info!("capturing input for {:?}", target);
    }

    pub fn leave_capture(&mut self) {
        self.state = ServerState::Idle;
        info!("released input capture");
    }
}
