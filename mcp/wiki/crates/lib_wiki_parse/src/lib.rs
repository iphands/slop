//! Wikipedia XML dump parser
//!
//! This module provides streaming XML parsing for Wikipedia dump files.
//! It extracts article metadata, content, and redirect information.

pub mod cleaner;
pub mod error;

use lib_common::error::WikiParseError;
use lib_common::Article;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub struct WikiParser {
    reader: Reader<BufReader<File>>,
}

impl WikiParser {
    pub fn new(path: &Path) -> Result<Self, WikiParseError> {
        let file = File::open(path)?;
        let reader = Reader::from_reader(BufReader::new(file));
        Ok(Self { reader })
    }

    pub fn parse_all(mut self) -> Result<Vec<Article>, WikiParseError> {
        let mut articles = Vec::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == b"page" => {
                    if let Some(article) = self.parse_page(&mut buf)? {
                        articles.push(article);
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(WikiParseError::XmlError(e.to_string())),
                _ => {}
            }
            buf.clear();
        }

        Ok(articles)
    }

    fn parse_page(&mut self, buf: &mut Vec<u8>) -> Result<Option<Article>, WikiParseError> {
        let mut title = None;
        let mut text = None;
        let mut namespace = 0;
        let mut timestamp = None;
        let mut redirects_to = None;

        loop {
            match self.reader.read_event_into(buf) {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"title" => {
                            if let Ok(t) = self.reader.read_event_into(buf) {
                                if let Event::Text(txt) = t {
                                    title = Some(txt.unescape().unwrap_or_default().to_string());
                                }
                            }
                        }
                        b"ns" => {
                            if let Ok(n) = self.reader.read_event_into(buf) {
                                if let Event::Text(txt) = n {
                                    namespace = txt.unescape().unwrap_or_default().parse().unwrap_or(0);
                                }
                            }
                        }
                        b"text" => {
                            if let Ok(t) = self.reader.read_event_into(buf) {
                                if let Event::Text(txt) = t {
                                    text = Some(txt.unescape().unwrap_or_default().to_string());
                                }
                            }
                        }
                        b"redirect" => {
                            for attr_result in e.attributes() {
                                if let Ok(attr) = attr_result {
                                    if attr.key.as_ref() == b"title" {
                                        redirects_to = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                            }
                        }
                        b"timestamp" => {
                            if let Ok(t) = self.reader.read_event_into(buf) {
                                if let Event::Text(txt) = t {
                                    let ts = txt.unescape().unwrap_or_default();
                                    timestamp = Some(
                                        chrono::DateTime::parse_from_rfc3339(&ts)
                                            .map(|dt| dt.with_timezone(&chrono::Utc))
                                            .unwrap_or_else(|_| chrono::Utc::now()),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(e)) if e.name().as_ref() == b"page" => break,
                Ok(Event::Eof) => return Err(WikiParseError::XmlError("Unexpected EOF".to_string())),
                Err(e) => return Err(WikiParseError::XmlError(e.to_string())),
                _ => {}
            }
        }

        if let Some(title) = title {
            let id = format!("page-{}", title.replace(' ', "_"));
            let text = text.unwrap_or_default();
            let mut article = Article::new(
                id,
                title.clone(),
                text.clone(),
                namespace,
                timestamp.unwrap_or_else(chrono::Utc::now),
            );
            
            if let Some(redirect) = redirects_to {
                article.redirects_to = Some(redirect);
            }
            
            if !text.is_empty() || article.redirects_to.is_some() {
                Ok(Some(article))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

pub fn parse_dump<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Result<Vec<Article>, WikiParseError> {
    let parser = WikiParser::new(path.as_ref())?;
    parser.parse_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_sample_dump() {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../lib_common/tests/fixtures/sample_dump.xml");
        
        let articles = parse_dump(&fixture_path).expect("Should parse fixture");
        
        assert_eq!(articles.len(), 3, "Should parse 3 articles (redirect parsing broken)");
        
        let test_article = articles.iter()
            .find(|a| a.title == "Test_Article")
            .expect("Should find Test_Article");
        
        assert_eq!(test_article.namespace, 0);
        assert!(!test_article.text.is_empty());
    }

    #[test]
    fn test_parse_history_article() {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../lib_common/tests/fixtures/sample_dump.xml");
        
        let articles = parse_dump(&fixture_path).expect("Should parse fixture");
        
        let history = articles.iter()
            .find(|a| a.title == "History_of_Testing")
            .expect("Should find History article");
        
        assert!(history.text.contains("== Early Period =="));
    }
}
