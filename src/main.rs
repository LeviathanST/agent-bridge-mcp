mod bridge;
mod db;
mod hub;
mod models;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rmcp::transport::io::stdio;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::local::LocalSessionManager,
};
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use bridge::AgentBridge;
use db::Db;
use hub::Hub;
use ws::{WsState, ws_handler};

#[derive(Parser)]
#[command(name = "agent-bridge", about = "Multi-agent MCP communication bridge")]
struct Cli {
    /// Run in stdio mode (for Claude Code)
    #[arg(long)]
    stdio: bool,

    /// HTTP server port (streamable HTTP transport)
    #[arg(long)]
    sse_port: Option<u16>,

    /// WebSocket server port for real-time agent communication
    #[arg(long)]
    ws_port: Option<u16>,

    /// Path to SQLite database
    #[arg(long, default_value = "~/.agent-bridge/bridge.db")]
    db_path: String,
}

fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var("HOME").ok().map(PathBuf::from)
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if !cli.stdio && cli.sse_port.is_none() && cli.ws_port.is_none() {
        eprintln!("Error: specify --stdio, --sse-port <PORT>, and/or --ws-port <PORT>");
        std::process::exit(1);
    }

    let db_path = expand_path(&cli.db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Arc::new(Db::open(&db_path)?);
    let hub = Hub::new(256);
    tracing::info!("Database opened at {}", db_path.display());

    let mut handles = Vec::new();

    if let Some(port) = cli.sse_port {
        let db = db.clone();
        let hub = hub.clone();
        let handle = tokio::spawn(async move {
            let bind_addr = format!("0.0.0.0:{port}");
            tracing::info!("Starting HTTP server on {bind_addr}");

            let ct = tokio_util::sync::CancellationToken::new();

            let service: StreamableHttpService<AgentBridge, LocalSessionManager> =
                StreamableHttpService::new(
                    move || Ok(AgentBridge::new(db.clone(), hub.clone())),
                    Default::default(),
                    StreamableHttpServerConfig {
                        stateful_mode: true,
                        sse_keep_alive: None,
                        cancellation_token: ct.child_token(),
                        ..Default::default()
                    },
                );

            let router = axum::Router::new().nest_service("/mcp", service);
            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .expect("Failed to bind HTTP listener");

            tracing::info!("MCP HTTP endpoint: http://{bind_addr}/mcp");

            axum::serve(listener, router)
                .await
                .expect("HTTP server failed");
        });
        handles.push(handle);
    }

    if let Some(port) = cli.ws_port {
        let db = db.clone();
        let hub = hub.clone();
        let handle = tokio::spawn(async move {
            let bind_addr = format!("0.0.0.0:{port}");
            tracing::info!("Starting WebSocket server on {bind_addr}");

            let ws_state = WsState { db, hub };

            let router = axum::Router::new()
                .route("/ws", axum::routing::get(ws_handler))
                .with_state(ws_state);

            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .expect("Failed to bind WebSocket listener");

            tracing::info!("WebSocket endpoint: ws://{bind_addr}/ws");

            axum::serve(listener, router)
                .await
                .expect("WebSocket server failed");
        });
        handles.push(handle);
    }

    if cli.stdio {
        let bridge = AgentBridge::new(db.clone(), hub.clone());
        let handle = tokio::spawn(async move {
            tracing::info!("Starting stdio transport");
            let transport = stdio();
            let server = bridge.serve(transport).await.expect("stdio serve failed");
            let _ = server.waiting().await;
        });
        handles.push(handle);
    }

    futures::future::join_all(handles).await;
    Ok(())
}
