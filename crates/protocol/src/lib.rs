//! Versioned wire types shared by the local backend and YAQMC adapters.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_BACKEND_PORT: u16 = 43821;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Spotify,
    Netease,
    Kugou,
    Qqmusic,
}

impl Platform {
    pub const ALL_EXTERNAL: [Self; 3] = [Self::Spotify, Self::Netease, Self::Kugou];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Netease => "netease",
            Self::Kugou => "kugou",
            Self::Qqmusic => "qqmusic",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UseMode {
    #[default]
    Isolated,
    Aggregate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    #[serde(default = "protocol_version")]
    pub version: u32,
    #[serde(default)]
    pub mode: UseMode,
    #[serde(default)]
    pub platform: Option<Platform>,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub operation: String,
    #[serde(default)]
    pub payload: Value,
}

fn protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub id: String,
    pub version: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl ProtocolError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: "invalid-request".to_owned(),
            message: message.into(),
            retryable: false,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "unsupported-operation".to_owned(),
            message: message.into(),
            retryable: false,
        }
    }

    pub fn internal() -> Self {
        Self {
            code: "internal-error".to_owned(),
            message: "the backend could not complete the request".to_owned(),
            retryable: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSource {
    pub platform: Platform,
    pub profile_id: String,
    pub track_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub source: TrackSource,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub preview_url: Option<String>,
    #[serde(default)]
    pub playable: bool,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub items: Vec<Track>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    pub platform: Option<Platform>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub platform: Platform,
    pub profile_id: String,
    pub authenticated: bool,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub playlists: Vec<PlaylistSummary>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub track_count: Option<u64>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    pub source: TrackSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub source: TrackSource,
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub synced: Vec<LyricLine>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSource {
    pub source: TrackSource,
    pub url: String,
    pub mime_type: Option<String>,
    #[serde(default)]
    pub content_length: Option<u64>,
    pub expires_at_ms: Option<u64>,
    pub is_preview: bool,
    pub quality: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    pub id: String,
    pub label: String,
    pub requires_login: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthAttempt {
    pub attempt_id: String,
    pub platform: Platform,
    pub profile_id: String,
    pub authorization_url: Option<String>,
    pub qr_payload: Option<String>,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub platform: Platform,
    pub profile_id: String,
    pub authenticated: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub masked_identity: Option<String>,
    #[serde(default)]
    pub entitlement: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_to_v1_isolated_mode() {
        let request: Request =
            serde_json::from_str(r#"{"id":"1","operation":"health","payload":{}}"#)
                .expect("request");
        assert_eq!(request.version, PROTOCOL_VERSION);
        assert_eq!(request.mode, UseMode::Isolated);
    }

    #[test]
    fn source_keeps_platform_and_profile_identity() {
        let source = TrackSource {
            platform: Platform::Spotify,
            profile_id: "personal".to_owned(),
            track_id: "track-1".to_owned(),
        };
        let encoded = serde_json::to_string(&source).expect("encode");
        assert!(encoded.contains("profileId"));
        assert!(encoded.contains("spotify"));
    }
}
