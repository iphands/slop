//! MediaWiki markup cleaner
//!
//! This module provides functions to clean MediaWiki markup from article text,
//! converting it to plain text suitable for embedding and search.

use regex::Regex;

/// Clean MediaWiki markup from article text
pub fn clean_markup(text: &str) -> String {
    let mut result = text.to_string();
    
    result = RE_FILE.replace_all(&result, "").to_string();
    result = RE_CATEGORY.replace_all(&result, "").to_string();
    result = RE_TEMPLATE.replace_all(&result, "").to_string();
    result = RE_HTML_TAG.replace_all(&result, "").to_string();
    result = RE_INTERNAL_LINK_WITH_TEXT.replace_all(&result, "$2").to_string();
    result = RE_INTERNAL_LINK.replace_all(&result, "$1").to_string();
    result = RE_EXTERNAL_LINK.replace_all(&result, "$2").to_string();
    result = RE_BOLD.replace_all(&result, "$1").to_string();
    result = RE_ITALIC.replace_all(&result, "$1").to_string();
    result = RE_HEADING.replace_all(&result, "\n$1\n").to_string();
    result = RE_MULTIPLE_NEWLINES.replace_all(&result, "\n\n").to_string();
    result = RE_MULTIPLE_SPACES.replace_all(&result, " ").to_string();
    
    result.trim().to_string()
}

lazy_static::lazy_static! {
    static ref RE_FILE: Regex = Regex::new(r"\[\[File:[^\]]+\]\]").unwrap();
    static ref RE_CATEGORY: Regex = Regex::new(r"\[\[Category:[^\]]+\]\]").unwrap();
    static ref RE_TEMPLATE: Regex = Regex::new(r"\{\{[^}]+\}\}").unwrap();
    static ref RE_HTML_TAG: Regex = Regex::new(r"<\s*(ref|gallery|table|tr|td|th|div|span|p|br|hr|ul|ol|li|b|i|u|s|sup|sub|code|pre|blockquote|img|a|abbr|acronym|cite|dfn|em|kbd|samp|var|strong|small|center|big|font|strike|tt|hr)[^>]*/?\s*>|<\s*(ref|br|hr|img|input|meta|link|base)[^>]*\/?\s*>").unwrap();
    static ref RE_INTERNAL_LINK_WITH_TEXT: Regex = Regex::new(r"\[\[([^\|]+)\|([^\]]+)\]\]").unwrap();
    static ref RE_INTERNAL_LINK: Regex = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    static ref RE_EXTERNAL_LINK: Regex = Regex::new(r"\[(https?://[^\s\]]+)\s+([^\]]+)\]").unwrap();
    static ref RE_BOLD: Regex = Regex::new(r"'''(.+?)'''").unwrap();
    static ref RE_ITALIC: Regex = Regex::new(r"''(.+?)''").unwrap();
    static ref RE_HEADING: Regex = Regex::new(r"(?m)^={1,6}\s*(.+?)\s*={1,6}$").unwrap();
    static ref RE_MULTIPLE_NEWLINES: Regex = Regex::new(r"\n{3,}").unwrap();
    static ref RE_MULTIPLE_SPACES: Regex = Regex::new(r" +").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_internal_links() {
        assert_eq!(clean_markup("[[Test_Article]]"), "Test_Article");
        assert_eq!(clean_markup("[[Test_Article|Test Article]]"), "Test Article");
    }

    #[test]
    fn test_clean_external_links() {
        assert_eq!(clean_markup("[https://example.com Example]"), "Example");
    }

    #[test]
    fn test_clean_bold_italic() {
        assert_eq!(clean_markup("'''bold text'''"), "bold text");
        assert_eq!(clean_markup("''italic text''"), "italic text");
    }

    #[test]
    fn test_clean_files_categories() {
        assert_eq!(clean_markup("[[File:example.jpg|thumbnail]]"), "");
        assert_eq!(clean_markup("[[Category:Test]]"), "");
    }

    #[test]
    fn test_clean_templates() {
        assert_eq!(clean_markup("{{Template|param=value}}"), "");
    }

    #[test]
    fn test_clean_headings() {
        assert_eq!(clean_markup("== Section ==\n\nContent"), "Section\n\nContent");
    }

    #[test]
    fn test_complex_cleanup() {
        let input = r#"
== Introduction ==
This is a '''bold''' and ''italic'' text with [[Internal_Link]] and [[External_Link|display text]].
[[File:example.jpg|thumb|Example]] and [[Category:Examples]].
Visit [https://example.com our website] for more.
{{Note|This is a template}}
"#;
        let output = clean_markup(input);
        assert!(output.contains("Introduction"));
        assert!(output.contains("bold"));
        assert!(output.contains("italic"));
        assert!(output.contains("display"));
        assert!(!output.contains("File:"));
        assert!(!output.contains("Category:"));
        assert!(!output.contains("{{"));
        assert!(!output.contains("[["));
    }
}
