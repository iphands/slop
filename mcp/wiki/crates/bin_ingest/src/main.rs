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
use lib_common::WikiConfig;
use lib_embeddings::{EmbeddingConfig, EmbeddingPipeline};
use lib_vector::{VectorStore, VectorStoreConfig};
use lib_wiki_parse::{cleaner, parse_dump};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, Level};

#[derive(Parser, Debug)]
#[command(name = "ingest")]
#[command(about = "Ingest Wikipedia dump into Qdrant vector store")]
struct Args {
    /// Directory containing Wikipedia dump files
    #[arg(short, long)]
    dump_dir: PathBuf,

    /// Qdrant host
    #[arg(short, long, default_value = "localhost")]
    qdrant_host: String,

    /// Qdrant port
    #[arg(short, long, default_value = "6333")]
    qdrant_port: u16,

    /// Collection name
    #[arg(short, long, default_value = "wikipedia_chunks")]
    collection: String,

    /// Chunk size (characters)
    #[arg(long, default_value = "1000")]
    chunk_size: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    let args = Args::parse();
    info!("Starting Wikipedia ingestion pipeline");

    // Initialize components
    let vector_config = VectorStoreConfig::default()
        .with_host(&args.qdrant_host)
        .with_port(args.qdrant_port)
        .with_collection(&args.collection);
    
    let vector_store = VectorStore::new(vector_config).await?;
    
    let embedding_config = EmbeddingConfig::default();
    let embedding_pipeline = EmbeddingPipeline::new(embedding_config)?;
    
    let chunk_config = ChunkConfig {
        max_size: args.chunk_size,
        ..Default::default()
    };

    // Parse dump files
    let start = Instant::now();
    info!("Parsing Wikipedia dump from: {:?}", args.dump_dir);
    
    let mut total_articles = 0;
    let mut total_chunks = 0;
    let mut total_vectors = 0;

    // Process each XML file in the dump directory
    for entry in std::fs::read_dir(&args.dump_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("xml") {
            info!("Processing: {:?}", path);
            
            let articles = parse_dump(&path)?;
            total_articles += articles.len();
            info!("  Parsed {} articles", articles.len());

            // Process each article
            for article in articles {
                // Clean markup
                let cleaned_text = cleaner::clean_markup(article.get_text());
                
                // Chunk the article
                let mut article = article.clone();
                article.cleaned_text = Some(cleaned_text.clone());
                
                let chunks = chunk_article(&article, &chunk_config);
                total_chunks += chunks.len();
                
                // Generate embeddings for each chunk
                let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
                let embeddings = embedding_pipeline.generate_embeddings(&texts).await?;
                total_vectors += embeddings.len();
                
                // Upsert to Qdrant
                vector_store.upsert_chunks(&chunks, &embeddings).await?;
            }
        }
    }

    let duration = start.elapsed();
    info!("Ingestion complete!");
    info!("  Articles: {}", total_articles);
    info!("  Chunks: {}", total_chunks);
    info!("  Vectors: {}", total_vectors);
    info!("  Duration: {:?}", duration);

    Ok(())
}
