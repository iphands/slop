//! Integration tests for lib_common

use lib_common::WikiConfig;
use std::path::PathBuf;

#[test]
fn test_fixture_file_exists() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    assert!(fixture_path.exists(), "Fixture file should exist");
}

#[test]
fn test_fixture_is_valid_xml() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    let content = std::fs::read_to_string(&fixture_path)
        .expect("Should read fixture file");
    
    // Basic XML validation - should start with XML declaration
    assert!(content.starts_with("<?xml"), "Should be valid XML");
    assert!(content.contains("<mediawiki"), "Should contain mediawiki root");
    assert!(content.contains("<page>"), "Should contain page elements");
}

#[test]
fn test_fixture_has_expected_articles() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    let content = std::fs::read_to_string(&fixture_path)
        .expect("Should read fixture file");
    
    // Check for expected article titles
    assert!(content.contains("<title>Test_Article</title>"));
    assert!(content.contains("<title>History_of_Testing</title>"));
    assert!(content.contains("<title>Test_Redirect</title>"));
    assert!(content.contains("<title>User:Test_User</title>"));
}

#[test]
fn test_fixture_has_redirect() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    let content = std::fs::read_to_string(&fixture_path)
        .expect("Should read fixture file");
    
    // Check for redirect structure
    assert!(content.contains("<redirect title=\"Test_Article\"/>"));
}

#[test]
fn test_fixture_has_nested_sections() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    let content = std::fs::read_to_string(&fixture_path)
        .expect("Should read fixture file");
    
    // Check for nested sections (=== level 3 headings)
    assert!(content.contains("=== Manual Testing ==="));
    assert!(content.contains("=== First Tools ==="));
    assert!(content.contains("==== Unit Testing ===="));
    assert!(content.contains("==== Integration Testing ===="));
}

#[test]
fn test_fixture_has_various_markup() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_dump.xml");
    
    let content = std::fs::read_to_string(&fixture_path)
        .expect("Should read fixture file");
    
    // Check for various MediaWiki markup patterns
    assert!(content.contains("'''Bold text'''"));  // Bold
    assert!(content.contains("''Italic text''"));   // Italic
    assert!(content.contains("[["));                // Internal links
    assert!(content.contains("[[File:"));           // File references
    assert!(content.contains("[[Category:"));       // Categories
}

#[test]
fn test_config_can_load_from_fixture_dir() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures");
    
    let config = WikiConfig::default()
        .with_dump_dir(fixture_path);
    
    assert!(config.dump_dir.exists());
    assert_eq!(config.chunk_size_min, 300);
    assert_eq!(config.chunk_size_max, 800);
}
