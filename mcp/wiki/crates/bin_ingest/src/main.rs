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
use lib_embeddings::{EmbeddingConfig, EmbeddingGenerator, FastEmbedGenerator};
use lib_vector::{ChunkMetadata, QdrantConfig, VectorStore};
use lib_wiki_parse::cleaner;
use lib_wiki_parse::parse_dump;
use qdrant_client::qdrant::PointStruct;
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

    /// Collection name
    #[arg(long, default_value = "wikipedia")]
    collection_name: String,

    /// Skip Qdrant operations (for testing)
    #[arg(long, default_value = "false")]
    skip_qdrant: bool,
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
    info!("Collection: {}", args.collection_name);

    // Setup Qdrant connection
    let qdrant_config = QdrantConfig {
        host: args.qdrant_host,
        port: args.qdrant_port,
        tls: false,
        api_key: None,
    };

    let qdrant_client = if !args.skip_qdrant {
        Some(qdrant_config.create_client()?)
    } else {
        None
    };

    // Setup embedding generator
    let embedding_config = EmbeddingConfig::default();
    let embedding_generator = FastEmbedGenerator::with_config(embedding_config)?;
    let vector_size = embedding_generator.dimension();
    info!("Using embedding model: {} ({} dimensions)", embedding_generator.model_name(), vector_size);

    // Setup vector store if Qdrant is enabled
    let vector_store = if let Some(ref client) = qdrant_client {
        Some(VectorStore::new(client.clone(), args.collection_name.clone(), vector_size))
    } else {
        None
    };

    // Create collection if Qdrant is enabled
    if let Some(ref store) = vector_store {
        if !args.skip_qdrant {
            info!("Creating Qdrant collection...");
            store.create_collection().await?;
            info!("Collection created successfully");
        }
    }

    // Process each XML file
    let mut total_chunks = 0;
    let mut total_articles = 0;

    for entry in std::fs::read_dir(&args.dump_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("xml") {
            info!("Processing: {:?}", path);
            
            let articles = parse_dump(&path)?;
            info!("  Parsed {} articles", articles.len());
            total_articles += articles.len();
            
            let mut batch_points = Vec::new();
            
            for article in &articles {
                // Clean markup
                let _cleaned = cleaner::clean_markup(article.get_text());
                
                // Chunk article
                let chunks = chunk_article(article, &ChunkConfig::default());
                info!("  Article '{}' -> {} chunks", article.title, chunks.len());
                
                // Process each chunk
                for (idx, chunk) in chunks.iter().enumerate() {
                    // Generate embedding
                    let embedding = embedding_generator.embed(&chunk.text).await?;
                    
                    // Create chunk metadata
                    let chunk_metadata = ChunkMetadata {
                        title: article.title.clone(),
                        section_path: chunk.section_path.clone(),
                        text: chunk.text.clone(),
                        offset: chunk.offset,
                        namespace: article.namespace,
                    };
                    
                    // Create Qdrant point if not skipping
                    if !args.skip_qdrant {
                        let point_id = format!("{}-chunk-{}", article.id, idx);
                        let payload = chunk_metadata.to_payload();
                        
                        let point = PointStruct::new(
                            point_id,
                            embedding.vector.clone(),
                            payload,
                        );
                        
                        batch_points.push(point);
                    }
                }
                
                // Upsert batch if we have points
                if !args.skip_qdrant && !batch_points.is_empty() {
                    if let Some(ref store) = vector_store {
                        info!("  Upserting {} points...", batch_points.len());
                        store.upsert(batch_points.clone()).await?;
                        batch_points.clear();
                    }
                }
                
                total_chunks += chunks.len();
            }
            
            // Final upsert if there are remaining points
            if !args.skip_qdrant && !batch_points.is_empty() {
                if let Some(ref store) = vector_store {
                    info!("  Upserting {} final points...", batch_points.len());
                    store.upsert(batch_points).await?;
                }
            }
        }
    }

    info!("Ingestion pipeline complete!");
    info!("  Total articles processed: {}", total_articles);
    info!("  Total chunks created: {}", total_chunks);
    
    if args.skip_qdrant {
        info!("  (Skipped Qdrant operations - no data stored)");
    } else {
        info!("  Data upserted to Qdrant collection: {}", args.collection_name);
    }

    Ok(())
}
