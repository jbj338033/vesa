use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::watch;
use tracing::info;
use vesa_core::config::{ClientConfig, ServerConfig};
use vesa_core::{Client, Server};

#[derive(Parser)]
#[command(name = "vesa", version, about = "Software KVM - share keyboard and mouse across machines")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "~/.config/vesa")]
    config_dir: String,
}

#[derive(Subcommand)]
enum Commands {
    Server {
        #[arg(short, long, default_value = "0.0.0.0:4920")]
        bind: SocketAddr,
    },
    Client {
        #[arg(short, long)]
        server: SocketAddr,

        #[arg(short, long, default_value = "Right")]
        position: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let config_dir = shellexpand(&cli.config_dir);
    let config_dir = PathBuf::from(config_dir);
    std::fs::create_dir_all(&config_dir)?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received Ctrl+C, shutting down");
        let _ = shutdown_tx.send(true);
    });

    match cli.command {
        Commands::Server { bind } => {
            let config = ServerConfig {
                bind_addr: bind,
                clients: vec![],
                release_hotkey: "ScrollLock".to_string(),
            };
            let cert_dir = config_dir.join("certs");
            let mut server = Server::new(config, cert_dir);
            server.run(shutdown_rx).await?;
        }
        Commands::Client { server, position } => {
            let pos = match position.as_str() {
                "Left" | "left" => vesa_event::Position::Left,
                "Right" | "right" => vesa_event::Position::Right,
                "Top" | "top" => vesa_event::Position::Top,
                "Bottom" | "bottom" => vesa_event::Position::Bottom,
                _ => {
                    eprintln!("invalid position: {position} (use Left/Right/Top/Bottom)");
                    std::process::exit(1);
                }
            };
            let config = ClientConfig {
                server_addr: server,
                position: pos,
            };
            let mut client = Client::new(config);
            client.run(shutdown_rx).await?;
        }
    }

    Ok(())
}

fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs_home()
    {
        return format!("{home}/{rest}");
    }
    path.to_string()
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
