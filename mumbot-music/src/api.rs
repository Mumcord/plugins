//! The mumbot music API (mumbot 0.3 `docs/music.md`): request builders, answer parsing and the
//! plain-text messages for every failure.

use mumcord_plugin_sdk::{
    events::HttpResult,
    http::{Body, Header, HeaderValue, HttpError, HttpRequest, Method},
};
use serde::Deserialize;

use crate::text::{t, t_with};

/// Longest track title shown; bot answers are untrusted and the UI allows 200 characters.
const MAX_TITLE: usize = 80;

/// One call to the bot.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `GET /queue`: the connection test and the refresh.
    Queue,
    Skip,
    Stop,
    Volume(u8),
    /// `POST /play` with a file from the media folder or a URL.
    Play {
        source: String,
        interrupt: bool,
    },
    /// `POST /play` with an MP3 the user picked.
    Upload {
        file: u64,
        interrupt: bool,
    },
}

/// Builds the request; the host inserts the API key from the named secret.
pub fn request(base: &str, secret: &str, call: &Call) -> HttpRequest {
    let mut headers = vec![Header {
        name: "X-API-Key".into(),
        value: HeaderValue::Secret(secret.to_owned()),
    }];
    let json = |value: serde_json::Value| Body::Bytes(value.to_string().into_bytes());
    let (method, path, body, content_type) = match call {
        Call::Queue => (Method::Get, "/queue".to_owned(), Body::Empty, None),
        Call::Skip => (Method::Post, "/skip".to_owned(), Body::Empty, None),
        Call::Stop => (Method::Post, "/stop".to_owned(), Body::Empty, None),
        Call::Volume(v) => (
            Method::Post,
            "/volume".to_owned(),
            json(serde_json::json!({ "volume": v })),
            Some("application/json"),
        ),
        Call::Play { source, interrupt } => {
            let field = if is_url(source) { "url" } else { "file" };
            (
                Method::Post,
                "/play".to_owned(),
                json(serde_json::json!({ field: source, "interrupt": interrupt })),
                Some("application/json"),
            )
        }
        Call::Upload { file, interrupt } => (
            Method::Post,
            format!("/play?interrupt={interrupt}"),
            Body::File(*file),
            Some("audio/mpeg"),
        ),
    };
    if let Some(content_type) = content_type {
        headers.push(Header {
            name: "Content-Type".into(),
            value: HeaderValue::Text(content_type.into()),
        });
    }
    HttpRequest {
        method,
        url: format!("{base}{path}"),
        headers,
        body,
    }
}

pub fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// A track as the bot reports it; titles are already cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub duration_secs: Option<u64>,
}

/// The answer of `GET /queue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub now_playing: Option<Track>,
    pub queue: Vec<Track>,
    pub volume: u8,
}

#[derive(Deserialize)]
struct RawTrack {
    #[serde(default)]
    title: String,
    #[serde(default)]
    duration_secs: Option<u64>,
}

#[derive(Deserialize)]
struct RawQueue {
    now_playing: Option<RawTrack>,
    #[serde(default)]
    queue: Vec<RawTrack>,
    #[serde(default)]
    volume: u8,
}

impl From<RawTrack> for Track {
    fn from(raw: RawTrack) -> Self {
        Self {
            title: clean(&raw.title, MAX_TITLE),
            duration_secs: raw.duration_secs,
        }
    }
}

pub fn parse_queue(body: &[u8]) -> Option<Queue> {
    let raw: RawQueue = serde_json::from_slice(body).ok()?;
    Some(Queue {
        now_playing: raw.now_playing.map(Track::from),
        queue: raw.queue.into_iter().map(Track::from).collect(),
        volume: raw.volume.min(100),
    })
}

/// The new volume from a `POST /volume` answer.
pub fn parse_volume(body: &[u8]) -> Option<u8> {
    #[derive(Deserialize)]
    struct Answer {
        volume: u8,
    }
    serde_json::from_slice::<Answer>(body)
        .ok()
        .map(|a| a.volume.min(100))
}

/// Untrusted text for display: no control characters, one line, at most `max` characters.
pub fn clean(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        return flat.to_owned();
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// How a request ended, as far as the UI cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A 2xx answer with its body.
    Ok(Vec<u8>),
    /// The bot cannot be reached or is not on the server: the player shows it offline.
    Offline(String),
    /// 429: back off.
    RateLimited,
    /// Any other failure, with its message.
    Failed(String),
}

pub fn rate_limited_text() -> String {
    t("rate_limited")
}

pub fn plain_http_text() -> String {
    t("plain_http")
}

pub fn key_rejected_text() -> String {
    t("key_rejected")
}

pub fn bad_certificate_text() -> String {
    t("bad_certificate")
}

pub fn bot_disconnected_text() -> String {
    t("bot_disconnected")
}

pub fn outcome(result: HttpResult, address: &str) -> Outcome {
    let unreachable =
        || Outcome::Offline(t_with("unreachable", &[("address", &clean(address, 64))]));
    let response = match result.outcome {
        Ok(response) => response,
        Err(HttpError::Connect(_) | HttpError::Timeout) => return unreachable(),
        Err(HttpError::HostNotAllowed) => return Outcome::Failed(plain_http_text()),
        Err(HttpError::Tls(_)) => return Outcome::Failed(bad_certificate_text()),
        Err(HttpError::Redirect) => {
            return Outcome::Failed(t("redirect"));
        }
        Err(HttpError::InvalidRequest) => {
            return Outcome::Failed(t("enter_both"));
        }
        Err(HttpError::ResponseTooLarge) => {
            return Outcome::Failed(t("answer_too_large"));
        }
    };
    let text = match response.status {
        200..=299 => return Outcome::Ok(response.body),
        429 => return Outcome::RateLimited,
        503 => return Outcome::Offline(bot_disconnected_text()),
        400 => t("http_400"),
        401 => key_rejected_text(),
        403 => t("http_403"),
        404 => t("http_404"),
        408 => t("http_408"),
        409 => t("http_409"),
        413 => t("http_413"),
        422 => t("http_422"),
        502 => t("http_502"),
        status => {
            return Outcome::Failed(t_with("status_error", &[("status", &status.to_string())]));
        }
    };
    Outcome::Failed(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugin_sdk::http::HttpResponse;

    fn status(status: u16) -> HttpResult {
        HttpResult {
            id: 1,
            outcome: Ok(HttpResponse {
                status,
                headers: Vec::new(),
                body: b"{}".to_vec(),
            }),
        }
    }

    fn failed(error: HttpError) -> HttpResult {
        HttpResult {
            id: 1,
            outcome: Err(error),
        }
    }

    #[test]
    fn requests_follow_the_mumbot_api() {
        let r = request("http://h:1", "bot/s/p/a", &Call::Queue);
        assert_eq!(
            (r.method, r.url.as_str()),
            (Method::Get, "http://h:1/queue")
        );
        assert_eq!(r.headers[0].name, "X-API-Key");
        assert!(matches!(&r.headers[0].value, HeaderValue::Secret(s) if s == "bot/s/p/a"));

        let r = request(
            "http://h:1",
            "s",
            &Call::Play {
                source: "a.mp3".into(),
                interrupt: true,
            },
        );
        assert_eq!(r.url, "http://h:1/play");
        let Body::Bytes(body) = r.body else {
            panic!("json body")
        };
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"file": "a.mp3", "interrupt": true})
        );

        let r = request(
            "http://h:1",
            "s",
            &Call::Play {
                source: "https://x/y.mp3".into(),
                interrupt: false,
            },
        );
        let Body::Bytes(body) = r.body else {
            panic!("json body")
        };
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"url": "https://x/y.mp3", "interrupt": false})
        );

        let r = request(
            "http://h:1",
            "s",
            &Call::Upload {
                file: 7,
                interrupt: false,
            },
        );
        assert_eq!(r.url, "http://h:1/play?interrupt=false");
        assert!(matches!(r.body, Body::File(7)));
        assert!(matches!(&r.headers[1].value, HeaderValue::Text(t) if t == "audio/mpeg"));

        let r = request("http://h:1", "s", &Call::Volume(40));
        assert!(matches!(&r.body, Body::Bytes(b) if b == br#"{"volume":40}"#));
        assert_eq!(request("x", "s", &Call::Skip).url, "x/skip");
        assert_eq!(request("x", "s", &Call::Stop).method, Method::Post);
    }

    #[test]
    fn queue_answers_are_parsed_leniently() {
        let body = br#"{"now_playing":{"id":1,"title":"Song","source":"file","channel":1,"duration_secs":184},
            "queue":[{"id":2,"title":"Next"}],"volume":60}"#;
        let q = parse_queue(body).unwrap();
        assert_eq!(q.now_playing.unwrap().duration_secs, Some(184));
        assert_eq!(q.queue[0].title, "Next");
        assert_eq!(q.volume, 60);
        assert_eq!(
            parse_queue(br#"{"now_playing":null,"queue":[],"volume":20}"#)
                .unwrap()
                .now_playing,
            None
        );
        assert_eq!(parse_queue(b"<html>"), None);
        assert_eq!(parse_volume(br#"{"volume":40}"#), Some(40));
    }

    #[test]
    fn hostile_titles_become_short_plain_text() {
        let title = format!("<b>x</b>\n\u{7}\u{202e}{}", "y".repeat(500));
        let shown = clean(&title, MAX_TITLE);
        assert!(shown.starts_with("<b>x</b> y"));
        assert_eq!(shown.chars().count(), MAX_TITLE);
        assert!(shown.ends_with('…'));
        assert!(!shown.chars().any(|c| c.is_control() || c == '\u{202e}'));
    }

    #[test]
    fn failures_map_to_plain_text() {
        assert_eq!(
            outcome(status(401), "a"),
            Outcome::Failed(key_rejected_text())
        );
        assert_eq!(
            outcome(status(503), "a"),
            Outcome::Offline(bot_disconnected_text())
        );
        assert_eq!(outcome(status(429), "a"), Outcome::RateLimited);
        assert_eq!(outcome(status(204), "a"), Outcome::Ok(b"{}".to_vec()));
        assert_eq!(
            outcome(status(409), "a"),
            Outcome::Failed("The queue is full".into())
        );
        assert_eq!(
            outcome(status(418), "a"),
            Outcome::Failed("The bot answered with an error (status 418)".into())
        );
        assert_eq!(
            outcome(
                failed(HttpError::Connect("refused".into())),
                "10.0.0.2:8081"
            ),
            Outcome::Offline("Can't reach the bot at 10.0.0.2:8081".into())
        );
        assert_eq!(
            outcome(failed(HttpError::HostNotAllowed), "a"),
            Outcome::Failed(plain_http_text())
        );
        assert_eq!(
            outcome(failed(HttpError::Tls("expired".into())), "a"),
            Outcome::Failed(bad_certificate_text())
        );
    }
}
