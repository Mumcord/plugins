//! Saved bots: which bot this is, where it lives and which keychain slot holds its API key.
//!
//! A bot is identified by the opaque server key from `self-state` plus the bot's peer key (its
//! certificate), never by its session. Both keys are cut to 32 hex characters so that the
//! secret name `bot/<server>/<peer>/<slot>` stays well under the 128-byte limit of `secrets`;
//! 128 bits are still unique enough to tell bots apart.

use serde::{Deserialize, Serialize};

/// Length both keys are cut to in storage keys and secret names.
const KEY_CHARS: usize = 32;

/// Storage prefix of every saved bot (identity scope).
pub const PREFIX: &str = "bots/";

/// One bot user on one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotId {
    server: String,
    peer: String,
}

impl BotId {
    pub fn new(server: &str, peer: &str) -> Self {
        Self {
            server: short(server),
            peer: short(peer),
        }
    }

    /// Whether `peer_key` (a full peer key from the host) is this bot.
    /// Whether this bot belongs to the server the user is on.
    pub fn on_server(&self, server: Option<&str>) -> bool {
        server.is_some_and(|s| short(s) == self.server)
    }

    pub fn matches(&self, server: Option<&str>, peer_key: Option<&str>) -> bool {
        server.is_some_and(|s| short(s) == self.server)
            && peer_key.is_some_and(|k| short(k) == self.peer)
    }

    pub fn storage_key(&self) -> String {
        format!("{PREFIX}{}/{}", self.server, self.peer)
    }

    pub fn from_storage_key(key: &str) -> Option<Self> {
        let (server, peer) = key.strip_prefix(PREFIX)?.split_once('/')?;
        (!server.is_empty() && !peer.is_empty() && !peer.contains('/')).then(|| Self {
            server: server.to_owned(),
            peer: peer.to_owned(),
        })
    }

    pub fn secret(&self, slot: KeySlot) -> String {
        format!("bot/{}/{}/{}", self.server, self.peer, slot.name())
    }
}

fn short(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .take(KEY_CHARS)
        .collect()
}

/// Two keychain slots per bot, so that editing the API key never loses the working one: the
/// edit writes the inactive slot and Save only switches the pointer once the test passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySlot {
    A,
    B,
}

impl KeySlot {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }
}

/// What is stored per bot. The API key itself lives only in the keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotConfig {
    /// The bot's user name when it was configured, for the settings list.
    pub name: String,
    /// The address as the user typed it.
    pub address: String,
    /// The keychain slot that holds the working API key.
    pub key: KeySlot,
}

/// The base URL for an address the user typed: `host:port` without a scheme means plain http,
/// which is what mumbot speaks; the host refuses plain http to public addresses anyway.
pub fn base_url(address: &str) -> Option<String> {
    let address = address.trim();
    if address.is_empty() || address.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let url = if address.starts_with("http://") || address.starts_with("https://") {
        address.to_owned()
    } else if address.contains("://") {
        return None;
    } else {
        format!("http://{address}")
    };
    let url = url.trim_end_matches('/');
    let host = url.split_once("://").map_or("", |(_, rest)| rest);
    (!host.is_empty()).then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_cut_and_secret_names_stay_short() {
        let id = BotId::new(&"a".repeat(64), &"b".repeat(64));
        assert_eq!(
            id.storage_key(),
            format!("bots/{}/{}", "a".repeat(32), "b".repeat(32))
        );
        let secret = id.secret(KeySlot::B);
        assert_eq!(
            secret,
            format!("bot/{}/{}/b", "a".repeat(32), "b".repeat(32))
        );
        assert!(secret.len() <= 128);
        assert_eq!(BotId::from_storage_key(&id.storage_key()), Some(id.clone()));
        assert!(id.matches(Some(&"a".repeat(64)), Some(&"b".repeat(64))));
        assert!(!id.matches(Some(&"a".repeat(64)), Some(&"c".repeat(64))));
        assert!(!id.matches(None, Some(&"b".repeat(64))));
    }

    #[test]
    fn foreign_storage_keys_are_ignored() {
        assert_eq!(BotId::from_storage_key("other/x/y"), None);
        assert_eq!(BotId::from_storage_key("bots/x"), None);
        assert_eq!(BotId::from_storage_key("bots/x/y/z"), None);
    }

    #[test]
    fn addresses_become_base_urls() {
        assert_eq!(
            base_url("127.0.0.1:8081").as_deref(),
            Some("http://127.0.0.1:8081")
        );
        assert_eq!(
            base_url(" https://bot.example.org/ ").as_deref(),
            Some("https://bot.example.org")
        );
        assert_eq!(base_url(""), None);
        assert_eq!(base_url("http://"), None);
        assert_eq!(base_url("ftp://x"), None);
        assert_eq!(base_url("a b"), None);
    }
}
