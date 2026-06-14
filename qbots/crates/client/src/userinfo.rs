//! `userinfo` builder — the InfoString carried in the `connect` OOB command.
//!
//! The server gets the player's userinfo from that `connect` argument (argv[4]); there is
//! no separate `clc_userinfo` during the handshake. See `cl_network.c:136`.

use q2proto::InfoString;

/// Sensible defaults for an external bot client.
const DEFAULT_SKIN: &str = "male/grunt";
const DEFAULT_RATE: &str = "25000";

/// A bot's userinfo, built over [`q2proto::InfoString`].
#[derive(Debug, Clone, Default)]
pub struct Userinfo {
    info: InfoString,
}

impl Userinfo {
    /// A new userinfo with the given display `name` and standard bot defaults.
    pub fn new(name: &str) -> Self {
        let mut info = InfoString::new();
        info.set("name", name);
        info.set("skin", DEFAULT_SKIN);
        info.set("rate", DEFAULT_RATE);
        info.set("msg", "0"); // message filter: 0 = all
        info.set("hand", "0"); // 0 = right, 1 = left, 2 = center
        info.set("fov", "90");
        Self { info }
    }

    /// Set/replace a key (validated by `InfoString`).
    pub fn set(&mut self, key: &str, value: &str) -> &mut Self {
        self.info.set(key, value);
        self
    }

    /// Look up a key.
    pub fn get(&self, key: &str) -> Option<String> {
        self.info.get(key)
    }

    /// The raw `\\key\\value` serialization (NUL-terminated by the writer when sent).
    pub fn as_str(&self) -> &str {
        self.info.as_str()
    }

    /// Consume into the raw string.
    pub fn into_raw(self) -> String {
        self.info.into_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_present() {
        let u = Userinfo::new("qbots");
        assert_eq!(u.get("name").as_deref(), Some("qbots"));
        assert_eq!(u.get("rate").as_deref(), Some("25000"));
        assert_eq!(u.get("skin").as_deref(), Some("male/grunt"));
        // every value parses back through InfoString
        assert!(u.as_str().starts_with("\\name\\qbots"));
    }

    #[test]
    fn overrides_apply() {
        let mut u = Userinfo::new("a");
        u.set("rate", "8000");
        assert_eq!(u.get("rate").as_deref(), Some("8000"));
        assert_eq!(u.get("name").as_deref(), Some("a"));
    }
}
