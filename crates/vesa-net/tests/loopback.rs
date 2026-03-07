use std::net::SocketAddr;
use vesa_event::Position;
use vesa_net::cert::Identity;
use vesa_net::{VesaClient, VesaServer};
use vesa_proto::Message;

fn localhost() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[tokio::test]
async fn datagram_ping_pong() {
    let identity = Identity::generate().unwrap();
    let server = VesaServer::bind(localhost(), &identity).unwrap();
    let server_addr = server.local_addr().unwrap();

    let client_handle = tokio::spawn(async move {
        let conn = VesaClient::connect(server_addr, localhost()).await.unwrap();

        conn.send_datagram(&Message::Ping).unwrap();

        let pong = conn.read_datagram().await.unwrap();
        assert_eq!(pong, Message::Pong);

        conn.close();
    });

    let server_conn = server.accept().await.unwrap();

    let msg = server_conn.read_datagram().await.unwrap();
    assert_eq!(msg, Message::Ping);

    server_conn.send_datagram(&Message::Pong).unwrap();

    client_handle.await.unwrap();
    server.close();
}

#[tokio::test]
async fn datagram_input_events() {
    let identity = Identity::generate().unwrap();
    let server = VesaServer::bind(localhost(), &identity).unwrap();
    let server_addr = server.local_addr().unwrap();

    let client_handle = tokio::spawn(async move {
        let conn = VesaClient::connect(server_addr, localhost()).await.unwrap();

        let msgs = [
            Message::PointerMotion {
                time: 42,
                dx: 1.5,
                dy: -2.5,
            },
            Message::KeyboardKey {
                time: 100,
                key: 30,
                state: 1,
            },
            Message::PointerButton {
                time: 200,
                button: 272,
                state: 1,
            },
        ];

        for msg in &msgs {
            conn.send_datagram(msg).unwrap();
        }

        // Small delay to ensure server receives before close
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.close();
    });

    let server_conn = server.accept().await.unwrap();

    let msg1 = server_conn.read_datagram().await.unwrap();
    assert!(matches!(msg1, Message::PointerMotion { time: 42, .. }));

    let msg2 = server_conn.read_datagram().await.unwrap();
    assert!(matches!(
        msg2,
        Message::KeyboardKey {
            time: 100,
            key: 30,
            state: 1
        }
    ));

    let msg3 = server_conn.read_datagram().await.unwrap();
    assert!(matches!(
        msg3,
        Message::PointerButton {
            time: 200,
            button: 272,
            state: 1
        }
    ));

    client_handle.await.unwrap();
    server.close();
}

#[tokio::test]
async fn stream_enter_ack_leave() {
    let identity = Identity::generate().unwrap();
    let server = VesaServer::bind(localhost(), &identity).unwrap();
    let server_addr = server.local_addr().unwrap();

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let client_handle = tokio::spawn(async move {
        let conn = VesaClient::connect(server_addr, localhost()).await.unwrap();
        let mut stream = conn.open_stream().await.unwrap();

        stream.send(&Message::Enter(Position::Right)).await.unwrap();

        let ack = stream.recv().await.unwrap();
        assert_eq!(ack, Message::Ack(1));

        stream.send(&Message::Leave(0.5)).await.unwrap();

        // Wait for server to finish reading before closing
        done_rx.await.ok();
        conn.close();
    });

    let server_conn = server.accept().await.unwrap();
    let mut stream = server_conn.accept_stream().await.unwrap();

    let enter = stream.recv().await.unwrap();
    assert_eq!(enter, Message::Enter(Position::Right));

    stream.send(&Message::Ack(1)).await.unwrap();

    let leave = stream.recv().await.unwrap();
    assert_eq!(leave, Message::Leave(0.5));

    // Signal client it's safe to close
    let _ = done_tx.send(());

    client_handle.await.unwrap();
    server.close();
}

#[tokio::test]
async fn stream_multiple_messages() {
    let identity = Identity::generate().unwrap();
    let server = VesaServer::bind(localhost(), &identity).unwrap();
    let server_addr = server.local_addr().unwrap();

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let client_handle = tokio::spawn(async move {
        let conn = VesaClient::connect(server_addr, localhost()).await.unwrap();
        let mut stream = conn.open_stream().await.unwrap();

        for i in 0..10u32 {
            stream.send(&Message::Ack(i)).await.unwrap();
        }

        done_rx.await.ok();
        conn.close();
    });

    let server_conn = server.accept().await.unwrap();
    let mut stream = server_conn.accept_stream().await.unwrap();

    for i in 0..10u32 {
        let msg = stream.recv().await.unwrap();
        assert_eq!(msg, Message::Ack(i));
    }

    let _ = done_tx.send(());
    client_handle.await.unwrap();
    server.close();
}

#[tokio::test]
async fn cert_generate_unique_fingerprints() {
    let id1 = Identity::generate().unwrap();
    let id2 = Identity::generate().unwrap();
    assert_ne!(id1.fingerprint(), id2.fingerprint());
    assert_eq!(id1.fingerprint(), id1.fingerprint());
    assert!(!id1.cert_der.is_empty());
    assert!(!id1.key_der.is_empty());
}

#[tokio::test]
async fn cert_save_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let identity = Identity::generate().unwrap();
    identity.save(dir.path()).unwrap();

    let loaded = Identity::load(dir.path()).unwrap();
    assert_eq!(identity.cert_der, loaded.cert_der);
    assert_eq!(identity.key_der, loaded.key_der);
    assert_eq!(identity.fingerprint(), loaded.fingerprint());
}

#[tokio::test]
async fn cert_load_or_generate_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let id = Identity::load_or_generate(dir.path()).unwrap();

    assert!(dir.path().join("cert.der").exists());
    assert!(dir.path().join("key.der").exists());

    let id2 = Identity::load_or_generate(dir.path()).unwrap();
    assert_eq!(id.cert_der, id2.cert_der);
}

#[tokio::test]
async fn multiple_connections() {
    let identity = Identity::generate().unwrap();
    let server = VesaServer::bind(localhost(), &identity).unwrap();
    let server_addr = server.local_addr().unwrap();

    // Connect two clients
    let c1 = tokio::spawn({
        let addr = server_addr;
        async move {
            let conn = VesaClient::connect(addr, localhost()).await.unwrap();
            conn.send_datagram(&Message::Ping).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            conn.close();
        }
    });

    let c2 = tokio::spawn({
        let addr = server_addr;
        async move {
            let conn = VesaClient::connect(addr, localhost()).await.unwrap();
            conn.send_datagram(&Message::Pong).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            conn.close();
        }
    });

    let conn1 = server.accept().await.unwrap();
    let conn2 = server.accept().await.unwrap();

    let m1 = conn1.read_datagram().await.unwrap();
    let m2 = conn2.read_datagram().await.unwrap();

    // One should be Ping, the other Pong (order may vary)
    assert!(
        (m1 == Message::Ping && m2 == Message::Pong)
            || (m1 == Message::Pong && m2 == Message::Ping)
    );

    c1.await.unwrap();
    c2.await.unwrap();
    server.close();
}
