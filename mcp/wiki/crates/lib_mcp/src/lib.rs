//! MCP server implementation for Wikipedia knowledge retrieval

use lib_common::{Article, Chunk, SearchResult, SearchParams};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Wikipedia MCP Server
pub struct WikiServer {
    articles: Arc<RwLock<Vec<Article>>>,
}

impl WikiServer {
    pub fn new() -> Self {
        Self {
            articles: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn add_articles(&self, articles: Vec<Article>) {
        let mut store = self.articles.write().await;
        for article in articles {
            store.push(article);
        }
        info!("Added {} articles to store", store.len());
    }

    pub async fn wiki_search(&self, params: SearchParams) -> Vec<SearchResult> {
        info!("Search query: {} with limit {}", params.query, params.limit);
        
        let store = self.articles.read().await;
        let query_lower = params.query.to_lowercase();
        
        let results: Vec<SearchResult> = store
            .iter()
            .filter(|a| {
                a.title.to_lowercase().contains(&query_lower) 
                || a.get_text().to_lowercase().contains(&query_lower)
            })
            .map(|article| {
                SearchResult::new(
                    Chunk::new(
                        format!("{}-chunk-0", article.id),
                        article.id.clone(),
                        article.title.clone(),
                        0,
                        article.get_text().chars().take(500).collect(),
                        500,
                    ),
                    1.0,
                )
            })
            .take(params.limit)
            .collect();
        
        info!("Found {} results", results.len());
        results
    }

    pub async fn wiki_semantic_search(&self, params: SearchParams) -> Vec<SearchResult> {
        info!("Semantic search query: {} with limit {}", params.query, params.limit);
        self.wiki_search(params).await
    }

    pub async fn wiki_read(&self, title: &str) -> Option<Article> {
        info!("Read article: {}", title);
        
        let store = self.articles.read().await;
        store.iter()
            .find(|a| a.title == title || a.title.replace('_', " ") == title)
            .cloned()
    }

    pub async fn wiki_related(&self, title: &str, limit: usize) -> Vec<String> {
        info!("Find related to: {} with limit {}", title, limit);
        Vec::new()
    }
}

impl Default for WikiServer {
    fn default() -> Self {
        Self::new()
    }
}
