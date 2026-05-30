//! MCP Server for Wikipedia knowledge retrieval
//!
//! Implements a stdio-based MCP server with Qdrant integration

use lib_embeddings::{EmbeddingConfig, EmbeddingGenerator, FastEmbedGenerator};
use lib_vector::{QdrantConfig, VectorStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// MCP Request
#[derive(Debug, Deserialize)]
struct Request {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// MCP Response
#[derive(Debug, Serialize)]
struct Response {
    id: Option<Value>,
    result: Option<Value>,
    error: Option<Value>,
}

/// MCP Service with Qdrant integration
struct WikiService {
    qdrant_host: String,
    qdrant_port: u16,
    collection_name: String,
    embedding_generator: Arc<FastEmbedGenerator>,
    vector_size: usize,
}

impl WikiService {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let embedding_config = EmbeddingConfig::default();
        let embedding_generator = FastEmbedGenerator::with_config(embedding_config)?;
        let vector_size = embedding_generator.dimension();
        
        Ok(Self {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6333,
            collection_name: "wikipedia".to_string(),
            embedding_generator: Arc::new(embedding_generator),
            vector_size,
        })
    }

    async fn get_vector_store(&self) -> Result<VectorStore, Box<dyn std::error::Error>> {
        let qdrant_config = QdrantConfig {
            host: self.qdrant_host.clone(),
            port: self.qdrant_port,
            tls: false,
            api_key: None,
        };
        let client = qdrant_config.create_client()?;
        Ok(VectorStore::new(client, self.collection_name.clone(), self.vector_size))
    }

    async fn handle_request(&self, request: Request) -> Response {
        match request.method.as_str() {
            "initialize" => Response {
                id: request.id,
                result: Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "wikipedia-mcp", "version": "0.1.0"}
                })),
                error: None,
            },
            "tools/list" => Response {
                id: request.id,
                result: Some(json!({
                    "tools": [
                        {"name": "wiki_search", "description": "Search Wikipedia articles by keyword or semantic similarity", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer"}}, "required": ["query"]}},
                        {"name": "wiki_read", "description": "Read a full Wikipedia article by title", "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}}, "required": ["title"]}}
                    ]
                })),
                error: None,
            },
            "tools/call" => self.handle_tool_call(request.id, request.params).await,
            _ => Response {
                id: request.id,
                result: None,
                error: Some(json!({"code": -32601, "message": format!("Method not found: {}", request.method)})),
            }
        }
    }

    async fn handle_tool_call(&self, id: Option<Value>, params: Option<Value>) -> Response {
        let params = match params {
            Some(p) => p,
            None => return Response { id, result: None, error: Some(json!({"code": -32602, "message": "Missing parameters"})) },
        };

        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned();

        match tool_name {
            "wiki_search" => {
                let args = match arguments {
                    Some(a) => a,
                    None => return Response { id, result: None, error: Some(json!({"code": -32602, "message": "Missing arguments"})) },
                };

                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10) as usize;

                if query.is_empty() {
                    return Response { id, result: Some(json!({"content": [{"type": "text", "text": json!({"error": "Query cannot be empty"}).to_string()}]})), error: None };
                }

                // Generate embedding for semantic search
                match self.embedding_generator.embed(&query).await {
                    Ok(embedding) => match self.get_vector_store().await {
                        Ok(store) => match store.search_dense(embedding.vector.clone(), limit as u64, None).await {
                            Ok(results) => {
                                let mut results_json = Vec::new();
                                for r in &results {
                                    let title: String = r.payload.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| "Unknown".to_string());
                                    let text: String = r.payload.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| "".to_string());
                                    let section = r.payload.get("section_path").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    
                                    results_json.push(json!({
                                        "title": title,
                                        "text": text.chars().take(200).collect::<String>(),
                                        "score": r.score,
                                        "section": section
                                    }));
                                }

                                Response {
                                    id,
                                    result: Some(json!({"content": [{"type": "text", "text": json!({"query": query, "results": results_json, "count": results.len()}).to_string()}]})),
                                    error: None,
                                }
                            },
                            Err(e) => Response { id, result: None, error: Some(json!({"code": -32000, "message": format!("Search error: {}", e)})) }
                        },
                        Err(e) => Response { id, result: None, error: Some(json!({"code": -32000, "message": format!("Vector store error: {}", e)})) }
                    },
                    Err(e) => Response { id, result: None, error: Some(json!({"code": -32000, "message": format!("Embedding error: {}", e)})) }
                }
            },
            "wiki_read" => {
                let args = match arguments {
                    Some(a) => a,
                    None => return Response { id, result: None, error: Some(json!({"code": -32602, "message": "Missing arguments"})) },
                };

                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");

                if title.is_empty() {
                    return Response { id, result: Some(json!({"content": [{"type": "text", "text": json!({"error": "Title cannot be empty"}).to_string()}]})), error: None };
                }

                // Search for the article by title using filter
                match self.get_vector_store().await {
                    Ok(store) => {
                        let filter = VectorStore::build_filter(Some(title), None);
                        let dummy_vector = vec![0.0; self.vector_size];
                        match store.search_dense(dummy_vector, 1, filter).await {
                            Ok(results) => {
                                if let Some(result) = results.first() {
                                    let title: String = result.payload.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| "".to_string());
                                    let content: String = result.payload.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| "".to_string());
                                    
                                    Response {
                                        id,
                                        result: Some(json!({"content": [{"type": "text", "text": json!({
                                            "title": title,
                                            "content": content,
                                            "found": true
                                        }).to_string()}]})),
                                        error: None,
                                    }
                                } else {
                                    Response {
                                        id,
                                        result: Some(json!({"content": [{"type": "text", "text": json!({"title": title, "content": "", "found": false}).to_string()}]})),
                                        error: None,
                                    }
                                }
                            },
                            Err(e) => Response { id, result: None, error: Some(json!({"code": -32000, "message": format!("Search error: {}", e)})) }
                        }
                    },
                    Err(e) => Response { id, result: None, error: Some(json!({"code": -32000, "message": format!("Vector store error: {}", e)})) }
                }
            },
            _ => Response { id, result: None, error: Some(json!({"code": -32602, "message": format!("Unknown tool: {}", tool_name)})) }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Wikipedia MCP Server");

    let service = WikiService::new()?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if let Ok(request) = serde_json::from_str::<Request>(&line) {
            info!("Received request: {}", request.method);
            let response = service.handle_request(request).await;
            let response_json = serde_json::to_string(&response)?;
            writeln!(stdout, "{}", response_json)?;
            stdout.flush()?;
        } else {
            info!("Failed to parse request");
        }
    }

    Ok(())
}
