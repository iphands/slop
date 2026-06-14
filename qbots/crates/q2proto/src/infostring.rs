//! `InfoString` — the backslash-delimited `\\key\\value` format used for `userinfo`.
//!
//! Ports `Info_ValueForKey` / `Info_SetValueForKey` / `Info_RemoveKey` / `Info_Validate`
//! from yquake2 `src/common/shared/shared.c`, and the size limits from
//! `common/header/shared.h:411-414` (`MAX_INFO_*`).

/// Max bytes in a serialized info string, *including* the terminating NUL on the wire
/// (`shared.h:414`). Content is therefore capped to `MAX_INFO_STRING - 1`.
pub const MAX_INFO_STRING: usize = 512;
/// Max bytes in a key (`shared.h:411`); usable length is `MAX_INFO_KEY - 1`.
pub const MAX_INFO_KEY: usize = 64;
/// Max bytes in a value (`shared.h:412`); usable length is `MAX_INFO_VALUE - 1`.
pub const MAX_INFO_VALUE: usize = 64;

/// A mutable `\\key\\value\\key\\value` info string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoString(String);

impl InfoString {
    /// Empty info string.
    pub fn new() -> Self {
        Self(String::new())
    }

    /// Wrap an already-serialized raw info string (e.g. parsed off the wire).
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The raw serialized form (no NUL; the caller NUL-terminates when writing).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `Info_ValueForKey` — look up a value by key.
    pub fn get(&self, key: &str) -> Option<String> {
        let body = self.0.strip_prefix('\\').unwrap_or(&self.0);
        let mut parts = body.split('\\');
        loop {
            let k = parts.next()?;
            let v = parts.next()?;
            if k == key {
                return Some(v.to_string());
            }
        }
    }

    /// `Info_RemoveKey` — drop the `\\key\\value` pair if present. A key containing a
    /// backslash is ignored (matches the C guard).
    pub fn remove(&mut self, key: &str) {
        if key.is_empty() || key.contains('\\') {
            return;
        }
        let body = self.0.strip_prefix('\\').unwrap_or(&self.0);
        let mut rebuilt = String::with_capacity(self.0.len());
        let mut parts = body.split('\\');
        while let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k != key {
                rebuilt.push('\\');
                rebuilt.push_str(k);
                rebuilt.push('\\');
                rebuilt.push_str(v);
            }
        }
        self.0 = rebuilt;
    }

    /// `Info_SetValueForKey` — validate, remove any existing value for `key`, then
    /// append `\\key\\value`. An empty value just removes the key. Over-length results
    /// are rejected (the key stays removed), matching the C overflow guard.
    pub fn set(&mut self, key: &str, value: &str) {
        if !validate_key_value(key, value) {
            return;
        }
        self.remove(key);
        if value.is_empty() {
            return;
        }
        // `\key\value` (2 separators + key + value)
        let needed = 2 + key.len() + value.len();
        if self.0.len() + needed >= MAX_INFO_STRING {
            return; // would overflow the MAX_INFO_STRING buffer
        }
        self.0.push('\\');
        self.0.push_str(key);
        self.0.push('\\');
        self.0.push_str(value);
    }

    /// `Info_Validate` — true unless the string contains `"` or `;`.
    pub fn is_valid(&self) -> bool {
        !self.0.contains('"') && !self.0.contains(';')
    }

    /// Consume into the raw string.
    pub fn into_raw(self) -> String {
        self.0
    }
}

/// `Info_ValidateKeyValue` — keys may not contain `"`/`\`/`;`; values may not contain
/// `"`/`\`; both must fit their `MAX_INFO_*` limit.
fn validate_key_value(key: &str, value: &str) -> bool {
    if key.contains('"') || key.contains('\\') || key.contains(';') {
        return false;
    }
    if value.contains('"') || value.contains('\\') {
        return false;
    }
    if key.len() >= MAX_INFO_KEY || value.len() >= MAX_INFO_VALUE {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get() {
        let mut info = InfoString::new();
        info.set("name", "qbots");
        info.set("rate", "25000");
        assert_eq!(info.get("name").as_deref(), Some("qbots"));
        assert_eq!(info.get("rate").as_deref(), Some("25000"));
        assert_eq!(info.get("missing"), None);
        assert_eq!(info.as_str(), "\\name\\qbots\\rate\\25000");
    }

    #[test]
    fn set_replaces_existing_no_dup() {
        let mut info = InfoString::new();
        info.set("name", "a");
        info.set("name", "b");
        assert_eq!(info.get("name").as_deref(), Some("b"));
        assert_eq!(info.as_str().matches("name").count(), 1);
    }

    #[test]
    fn empty_value_removes_key() {
        let mut info = InfoString::new();
        info.set("name", "x");
        info.set("name", "");
        assert_eq!(info.get("name"), None);
        assert_eq!(info.as_str(), "");
    }

    #[test]
    fn remove_drops_pair() {
        let mut info = InfoString::new();
        info.set("name", "x");
        info.set("rate", "25000");
        info.remove("name");
        assert_eq!(info.get("name"), None);
        assert_eq!(info.get("rate").as_deref(), Some("25000"));
    }

    #[test]
    fn rejects_invalid_keys_and_values() {
        let mut info = InfoString::new();
        info.set("bad\"key", "v");
        info.set("ok", "bad\\val");
        info.set("semi;", "v");
        assert!(info.as_str().is_empty());
    }

    #[test]
    fn rejects_over_length() {
        let mut info = InfoString::new();
        let long = "x".repeat(MAX_INFO_VALUE);
        info.set("k", &long);
        assert_eq!(info.get("k"), None);
    }

    #[test]
    fn validate_flags_quotes_and_semicolons() {
        let bad = InfoString::from_raw("a\"b;c");
        assert!(!bad.is_valid());
        assert!(InfoString::from_raw("\\name\\ok").is_valid());
    }

    #[test]
    fn parse_off_wire() {
        let info = InfoString::from_raw("\\name\\qbots\\skin\\male/grunt");
        assert_eq!(info.get("name").as_deref(), Some("qbots"));
        assert_eq!(info.get("skin").as_deref(), Some("male/grunt"));
    }
}
