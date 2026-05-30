//! Article chunking module

use lib_common::{Article, Chunk};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub overlap: f64,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            min_size: 200,
            max_size: 1000,
            overlap: 0.1,
        }
    }
}

pub fn chunk_article(article: &Article, config: &ChunkConfig) -> Vec<Chunk> {
    let text = article.get_text();
    let sections = split_by_sections(text);
    
    if sections.is_empty() {
        return chunk_by_size(text, &article.title, &article.id, config);
    }
    
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut chunk_index = 0;
    
    for (heading, content) in sections {
        let section_text = if let Some(h) = heading {
            format!("{}\n{}", h, content)
        } else {
            content
        };
        
        if current_chunk.len() + section_text.len() > config.max_size && !current_chunk.is_empty() {
            chunks.push(create_chunk(article, chunk_index, &current_chunk));
            chunk_index += 1;
            current_chunk = section_text;
        } else {
            if !current_chunk.is_empty() {
                current_chunk.push_str("\n\n");
            }
            current_chunk.push_str(&section_text);
        }
    }
    
    if !current_chunk.is_empty() {
        chunks.push(create_chunk(article, chunk_index, &current_chunk));
    }
    
    chunks
}

fn split_by_sections(text: &str) -> Vec<(Option<String>, String)> {
    let re = Regex::new(r"(?m)^={2,6}\s*(.+?)\s*={2,6}\s*\n").unwrap();
    
    let mut sections = Vec::new();
    
    for mat in re.find_iter(text) {
        let heading = text[mat.start()..mat.end()]
            .split('=')
            .skip(1)
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        
        let section_start = mat.end();
        
        if let Some(next_mat) = re.find_at(text, section_start) {
            let section_content = &text[section_start..next_mat.start()];
            sections.push((Some(heading), section_content.to_string()));
        } else {
            let section_content = &text[section_start..];
            sections.push((Some(heading), section_content.to_string()));
        }
    }
    
    if let Some(first_mat) = re.find(text) {
        let intro = &text[..first_mat.start()];
        if !intro.trim().is_empty() {
            sections.insert(0, (None, intro.to_string()));
        }
    }
    
    sections
}

fn create_chunk(article: &Article, index: usize, text: &str) -> Chunk {
    Chunk::new(
        format!("{}-chunk-{}", article.id, index),
        article.id.clone(),
        article.title.clone(),
        index,
        text.to_string(),
        text.len(),
    )
}

fn chunk_by_size(text: &str, title: &str, article_id: &str, config: &ChunkConfig) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut index = 0;
    let mut offset = 0;
    
    while offset < text.len() {
        let end = (offset + config.max_size).min(text.len());
        let chunk_text = &text[offset..end];
        
        chunks.push(Chunk::new(
            format!("{}-chunk-{}", article_id, index),
            article_id.to_string(),
            title.to_string(),
            index,
            chunk_text.to_string(),
            chunk_text.len(),
        ));
        
        offset = if end >= text.len() {
            end
        } else {
            (offset + config.max_size - (config.max_size as f64 * config.overlap) as usize).min(text.len())
        };
        
        index += 1;
    }
    
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_article() -> Article {
        Article::new(
            "test-1".to_string(),
            "Test_Article".to_string(),
            "Intro\n\n== Section 1 ==\nContent 1\n\n== Section 2 ==\nContent 2".to_string(),
            0,
            Utc::now(),
        )
    }

    #[test]
    fn test_chunk_article() {
        let article = test_article();
        let config = ChunkConfig::default();
        let chunks = chunk_article(&article, &config);
        
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].article_id, article.id);
    }
}
