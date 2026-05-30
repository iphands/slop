//! Wikipedia ingestion CLI
//!
//! Orchestrates the full ingestion pipeline:
//! 1. Parse Wikipedia XML dump
//! 2. Clean MediaWiki markup
//! 3. Chunk articles
//! 4. Generate embeddings
//! 5. Upsert to Qdrant

use clap::Parser;
use lib_chunking::{chunk_article, ChunkConfig};
use lib_wiki_parse::cleaner;
use lib_wiki_parse::parse_dump;
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

    let args = Args::parse();
    info!("Starting Wikipedia ingestion pipeline");
    info!("Dump directory: {:?}", args.dump_dir);
    info!("Qdrant: {}:{},", args.qdrant_host, args.qdrant_port);

    // TODO: Implement full pipeline:
    // 1. Parse XML dump with lib_wiki_parse
    // 2. Clean markup with cleaner
    // 3. Chunk with lib_chunking
    // 4. Generate embeddings with lib_embeddings
    // 5. Upsert to Qdrant with lib_vector

    // For now, just parse and display what we find
    for entry in std::fs::read_dir(&args.dump_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("xml") {
            info!("Processing: {:?}", path);
            
            let articles = parse_dump(&path)?;
            info!("  Parsed {} articles", articles.len());
            
            for article in &articles {
                let _cleaned = cleaner::clean_markup(article.get_text());
                let chunks = chunk_article(article, &ChunkConfig::default());
                info!("  Article '{}' -> {} chunks", article.title, chunks.len());
            }
        }
    }

    info!("Ingestion pipeline complete (WIP - no vector storage yet)");

    Ok(())
}
