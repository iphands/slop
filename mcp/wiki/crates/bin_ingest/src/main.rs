//! Wikipedia ingestion CLI
//!
//! TODO: Complete pipeline orchestration

use clap::Parser;
use std::path::PathBuf;
use tracing::{info, Level};

#[derive(Parser, Debug)]
#[command(name = "ingest")]
#[command(about = "Ingest Wikipedia dump into vector store")]
struct Args {
    /// Directory containing Wikipedia dump files
    #[arg(short, long)]
    dump_dir: PathBuf,

    /// Qdrant host
    #[arg(long, default_value = "localhost")]
    qdrant_host: String,

    /// Qdrant port  
    #[arg(long, default_value = "6333")]
    qdrant_port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    let _args = Args::parse();
    info!("Wikipedia ingestion pipeline - WIP");
    info!("Dump directory: {:?}", _args.dump_dir);
    info!("Qdrant: {}:{},", _args.qdrant_host, _args.qdrant_port);

    // TODO: Implement full pipeline:
    // 1. Parse XML dump with lib_wiki_parse
    // 2. Clean markup with cleaner
    // 3. Chunk with lib_chunking
    // 4. Generate embeddings with lib_embeddings
    // 5. Upsert to Qdrant with lib_vector

    Ok(())
}
