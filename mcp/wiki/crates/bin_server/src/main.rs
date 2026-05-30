//! MCP Server binary for Wikipedia knowledge retrieval

use lib_mcp::WikiServer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Wikipedia MCP Server");

    // Create server
    let _server = WikiServer::new();
    
    info!("Server initialized (WIP)");
    info!("Tools available: wiki_search, wiki_semantic_search, wiki_read, wiki_related");
    
    // Keep running until Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    
    Ok(())
}
