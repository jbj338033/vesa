pub mod cert;

use cert::{CertError, Identity};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};
use vesa_proto::{DecodeError, Message};

#[derive(Debug, Error)]
pub enum NetError {
    #[error("certificate error: {0}")]
    Cert(#[from] CertError),
    #[error("quinn connection error: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("quinn connect error: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("quinn write error: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("quinn read error: {0}")]
    Read(#[from] quinn::ReadExactError),
    #[error("quinn send datagram error: {0}")]
    SendDatagram(#[from] quinn::SendDatagramError),
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),
}

pub struct VesaServer {
    endpoint: quinn::Endpoint,
}

impl VesaServer {
    pub fn bind(addr: SocketAddr, identity: &Identity) -> Result<Self, NetError> {
        let server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![identity.rustls_cert()],
                identity.rustls_key().map_err(NetError::Cert)?,
            )?;

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .expect("valid server config"),
        ));

        let transport = Arc::new(Self::transport_config());
        server_config.transport_config(transport);

        let endpoint = quinn::Endpoint::server(server_config, addr)?;
        info!("server bound to {}", addr);

        Ok(Self { endpoint })
    }

    pub async fn accept(&self) -> Option<VesaConnection> {
        let incoming = self.endpoint.accept().await?;
        match incoming.await {
            Ok(conn) => {
                info!("accepted connection from {}", conn.remote_address());
                Some(VesaConnection { conn })
            }
            Err(e) => {
                error!("failed to accept connection: {}", e);
                None
            }
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr, NetError> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"shutdown");
    }

    fn transport_config() -> quinn::TransportConfig {
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(
            quinn::IdleTimeout::try_from(std::time::Duration::from_secs(30)).unwrap(),
        ));
        transport.keep_alive_interval(Some(std::time::Duration::from_secs(5)));
        transport.datagram_receive_buffer_size(Some(65536));
        transport.datagram_send_buffer_size(65536);
        transport
    }
}

pub struct VesaClient;

impl VesaClient {
    pub async fn connect(
        server_addr: SocketAddr,
        bind_addr: SocketAddr,
    ) -> Result<VesaConnection, NetError> {
        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let mut endpoint = quinn::Endpoint::client(bind_addr)?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .expect("valid client config"),
        ));
        client_config.transport_config(Arc::new(VesaServer::transport_config()));
        endpoint.set_default_client_config(client_config);

        info!("connecting to {}", server_addr);
        let conn = endpoint.connect(server_addr, "vesa")?.await?;
        info!("connected to {}", server_addr);

        Ok(VesaConnection { conn })
    }
}

pub struct VesaConnection {
    conn: quinn::Connection,
}

impl VesaConnection {
    pub fn send_datagram(&self, msg: &Message) -> Result<(), NetError> {
        let data = vesa_proto::encode(msg);
        self.conn.send_datagram(data.into())?;
        Ok(())
    }

    pub async fn read_datagram(&self) -> Result<Message, NetError> {
        let data = self.conn.read_datagram().await?;
        let msg = vesa_proto::decode(&data)?;
        Ok(msg)
    }

    pub async fn open_stream(&self) -> Result<VesaStream, NetError> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok(VesaStream { send, recv })
    }

    pub async fn accept_stream(&self) -> Result<VesaStream, NetError> {
        let (send, recv) = self.conn.accept_bi().await?;
        Ok(VesaStream { send, recv })
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    pub fn close(&self) {
        self.conn.close(0u32.into(), b"done");
    }
}

pub struct VesaStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl VesaStream {
    pub async fn send(&mut self, msg: &Message) -> Result<(), NetError> {
        let data = vesa_proto::encode(msg);
        let len = u8::try_from(data.len()).expect("message fits in u8");
        self.send.write_all(&[len]).await?;
        self.send.write_all(&data).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Message, NetError> {
        let mut len_buf = [0u8; 1];
        self.recv.read_exact(&mut len_buf).await?;
        let len = len_buf[0] as usize;
        let mut buf = vec![0u8; len];
        self.recv.read_exact(&mut buf).await?;
        let msg = vesa_proto::decode(&buf)?;
        Ok(msg)
    }
}

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}
