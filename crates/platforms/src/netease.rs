use crate::{
    bounded_limit,
    http::{cookie, ApiHttp},
    now_ms, require_non_empty, AuthCompletion, AuthPreparation, PlatformAdapter, PlatformError,
    ProfileSession,
};
use async_trait::async_trait;
use reqwest::Method;
use serde_json::Value;
use uuid::Uuid;
use yaqmc_music_providers_protocol::{
    AccountSnapshot, AuthMethod, LibrarySnapshot, LyricLine, Lyrics, Platform, PlaybackSource,
    PlaylistSummary, SearchResult, Track, TrackSource,
};

#[derive(Clone, Debug)]
pub struct NeteaseConfig {
    pub base_url: String,
}

#[derive(Clone)]
pub struct NeteaseClient {
    api: ApiHttp,
}

impl NeteaseClient {
    pub fn new(config: NeteaseConfig) -> Result<Self, PlatformError> {
        Ok(Self {
            api: ApiHttp::new(
                &config.base_url,
                config.base_url.starts_with("http://127.0.0.1"),
            )?,
        })
    }

    async fn value(
        &self,
        session: &ProfileSession,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, PlatformError> {
        let headers = session
            .secret
            .as_deref()
            .map(cookie)
            .into_iter()
            .collect::<Vec<_>>();
        let value: Value = self
            .api
            .json(Method::GET, path, query, &headers, None)
            .await?;
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(200);
        if code != 200 {
            return Err(PlatformError::Upstream(format!(
                "NetEase API returned code {code}"
            )));
        }
        Ok(value)
    }

    fn map_song(value: &Value, profile_id: &str) -> Option<Track> {
        let id = value
            .get("id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .or_else(|| {
                value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })?;
        let title = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .to_owned();
        let artists = value
            .get("ar")
            .or_else(|| value.get("artists"))
            .and_then(Value::as_array)
            .map(|artists| {
                artists
                    .iter()
                    .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let album = value
            .get("al")
            .or_else(|| value.get("album"))
            .and_then(|album| album.get("name"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let artwork_url = value
            .get("al")
            .or_else(|| value.get("album"))
            .and_then(|album| album.get("picUrl"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        Some(Track {
            source: TrackSource {
                platform: Platform::Netease,
                profile_id: profile_id.to_owned(),
                track_id: id,
            },
            title,
            artists,
            album,
            duration_ms: value
                .get("dt")
                .and_then(Value::as_u64)
                .or_else(|| value.get("duration").and_then(Value::as_u64)),
            artwork_url,
            preview_url: None,
            playable: true,
            metadata: value.clone(),
        })
    }

    fn extract_cookie(value: &Value) -> Option<String> {
        value
            .get("cookie")
            .and_then(Value::as_str)
            .filter(|cookie| !cookie.is_empty())
            .map(ToOwned::to_owned)
    }
}

#[async_trait]
impl PlatformAdapter for NeteaseClient {
    fn platform(&self) -> Platform {
        Platform::Netease
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![
            AuthMethod {
                id: "qr".to_owned(),
                label: "NetEase QR login".to_owned(),
                requires_login: false,
            },
            AuthMethod {
                id: "phone".to_owned(),
                label: "NetEase phone login".to_owned(),
                requires_login: false,
            },
        ]
    }

    async fn prepare_auth(
        &self,
        _session: &ProfileSession,
        method: &str,
    ) -> Result<AuthPreparation, PlatformError> {
        match method {
            "qr" => {
                let key_response: Value = self
                    .api
                    .json(
                        Method::GET,
                        "/login/qr/key",
                        &[("timestamp", now_ms().to_string())],
                        &[],
                        None,
                    )
                    .await?;
                let key = key_response
                    .pointer("/data/unikey")
                    .and_then(Value::as_str)
                    .ok_or(PlatformError::InvalidResponse)?;
                let qr: Value = self
                    .api
                    .json(
                        Method::GET,
                        "/login/qr/create",
                        &[
                            ("key", key.to_owned()),
                            ("qrimg", "true".to_owned()),
                            ("timestamp", now_ms().to_string()),
                        ],
                        &[],
                        None,
                    )
                    .await?;
                Ok(AuthPreparation {
                    attempt_id: Uuid::new_v4().to_string(),
                    authorization_url: qr
                        .get("data")
                        .and_then(|data| data.get("qrurl"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    qr_payload: qr
                        .get("data")
                        .and_then(|data| data.get("qrimg"))
                        .and_then(Value::as_str)
                        .or(Some(key))
                        .map(ToOwned::to_owned),
                    expires_at_ms: now_ms().saturating_add(5 * 60 * 1000),
                    verifier: Some(key.to_owned()),
                    state: None,
                })
            }
            "phone" => Ok(AuthPreparation {
                attempt_id: Uuid::new_v4().to_string(),
                expires_at_ms: now_ms().saturating_add(5 * 60 * 1000),
                ..AuthPreparation::default()
            }),
            _ => Err(PlatformError::Unsupported),
        }
    }

    async fn complete_auth(
        &self,
        _session: &ProfileSession,
        preparation: &AuthPreparation,
        payload: &Value,
    ) -> Result<AuthCompletion, PlatformError> {
        if payload.get("phone").is_some() {
            let phone = payload
                .get("phone")
                .and_then(Value::as_str)
                .ok_or_else(|| PlatformError::Configuration("phone is invalid".to_owned()))?;
            let password = payload
                .get("password")
                .and_then(Value::as_str)
                .ok_or_else(|| PlatformError::Configuration("password is required".to_owned()))?;
            require_non_empty(phone, "phone")?;
            require_non_empty(password, "password")?;
            let response: Value = self
                .api
                .json(
                    Method::GET,
                    "/login/cellphone",
                    &[
                        ("phone", phone.to_owned()),
                        ("password", password.to_owned()),
                        ("timestamp", now_ms().to_string()),
                    ],
                    &[],
                    None,
                )
                .await?;
            let cookie = Self::extract_cookie(&response).ok_or(PlatformError::InvalidResponse)?;
            return Ok(AuthCompletion {
                secret: Some(cookie),
                display_name: response
                    .pointer("/profile/nickname")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                ..AuthCompletion::default()
            });
        }
        let key = preparation
            .verifier
            .as_deref()
            .ok_or(PlatformError::InvalidResponse)?;
        let response: Value = self
            .api
            .json(
                Method::GET,
                "/login/qr/check",
                &[("key", key.to_owned()), ("timestamp", now_ms().to_string())],
                &[],
                None,
            )
            .await?;
        let code = response.get("code").and_then(Value::as_i64).unwrap_or(0);
        match code {
            803 => Ok(AuthCompletion {
                secret: Self::extract_cookie(&response),
                ..AuthCompletion::default()
            }),
            800 => Err(PlatformError::Upstream("QR login expired".to_owned())),
            _ => Err(PlatformError::AuthenticationPending),
        }
    }

    async fn account_snapshot(
        &self,
        session: &ProfileSession,
    ) -> Result<AccountSnapshot, PlatformError> {
        let Some(cookie) = session.secret.as_deref() else {
            return Ok(AccountSnapshot {
                platform: Platform::Netease,
                profile_id: session.profile_id.clone(),
                authenticated: false,
                display_name: None,
                masked_identity: None,
                entitlement: None,
            });
        };
        let response = self
            .value(
                session,
                "/login/status",
                &[("timestamp", now_ms().to_string())],
            )
            .await?;
        let nickname = response
            .pointer("/data/profile/nickname")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        Ok(AccountSnapshot {
            platform: Platform::Netease,
            profile_id: session.profile_id.clone(),
            authenticated: !cookie.is_empty(),
            display_name: nickname,
            masked_identity: response
                .pointer("/data/profile/userId")
                .and_then(Value::as_i64)
                .map(|id| format!("user-{id}")),
            entitlement: None,
        })
    }

    async fn search(
        &self,
        session: &ProfileSession,
        query: &str,
        page: u32,
        limit: u32,
    ) -> Result<SearchResult, PlatformError> {
        let limit = bounded_limit(limit);
        let response = self
            .value(
                session,
                "/cloudsearch",
                &[
                    ("keywords", query.to_owned()),
                    ("type", "1".to_owned()),
                    ("limit", limit.to_string()),
                    ("offset", page.saturating_mul(limit).to_string()),
                ],
            )
            .await?;
        let items = response
            .pointer("/result/songs")
            .and_then(Value::as_array)
            .map(|songs| {
                songs
                    .iter()
                    .filter_map(|song| Self::map_song(song, &session.profile_id))
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchResult {
            items,
            next_cursor: Some((page + 1).to_string()),
            platform: Some(Platform::Netease),
            warnings: Vec::new(),
        })
    }

    async fn library(
        &self,
        session: &ProfileSession,
        limit: u32,
    ) -> Result<LibrarySnapshot, PlatformError> {
        let account = self.value(session, "/user/account", &[]).await?;
        let uid = account
            .pointer("/account/id")
            .and_then(Value::as_i64)
            .ok_or(PlatformError::InvalidResponse)?;
        let playlists = self
            .value(
                session,
                "/user/playlist",
                &[
                    ("uid", uid.to_string()),
                    ("limit", bounded_limit(limit).to_string()),
                ],
            )
            .await?;
        let summaries = playlists
            .pointer("/playlist")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let id = item.get("id").and_then(Value::as_i64)?.to_string();
                        Some(PlaylistSummary {
                            id: id.clone(),
                            title: item.get("name").and_then(Value::as_str)?.to_owned(),
                            track_count: item.get("trackCount").and_then(Value::as_u64),
                            artwork_url: item
                                .get("coverImgUrl")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            source: TrackSource {
                                platform: Platform::Netease,
                                profile_id: session.profile_id.clone(),
                                track_id: id,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(LibrarySnapshot {
            platform: Platform::Netease,
            profile_id: session.profile_id.clone(),
            authenticated: true,
            tracks: Vec::new(),
            playlists: summaries,
            warnings: Vec::new(),
        })
    }

    async fn lyrics(
        &self,
        session: &ProfileSession,
        track_id: &str,
    ) -> Result<Lyrics, PlatformError> {
        let response = self
            .value(session, "/lyric", &[("id", track_id.to_owned())])
            .await?;
        let plain = response
            .pointer("/lrc/lyric")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        Ok(Lyrics {
            source: TrackSource {
                platform: Platform::Netease,
                profile_id: session.profile_id.clone(),
                track_id: track_id.to_owned(),
            },
            synced: plain.as_deref().map(parse_lrc).unwrap_or_default(),
            plain,
        })
    }

    async fn playback(
        &self,
        session: &ProfileSession,
        track_id: &str,
        quality: Option<&str>,
    ) -> Result<PlaybackSource, PlatformError> {
        let response = self
            .value(
                session,
                "/song/url/v1",
                &[
                    ("id", track_id.to_owned()),
                    ("level", quality.unwrap_or("exhigh").to_owned()),
                ],
            )
            .await?;
        let item = response
            .pointer("/data/0")
            .ok_or(PlatformError::InvalidResponse)?;
        let url = item
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://"))
            .ok_or(PlatformError::FullPlaybackUnavailable)?;
        Ok(PlaybackSource {
            source: TrackSource {
                platform: Platform::Netease,
                profile_id: session.profile_id.clone(),
                track_id: track_id.to_owned(),
            },
            url: url.to_owned(),
            mime_type: Some("audio/mpeg".to_owned()),
            content_length: None,
            expires_at_ms: None,
            is_preview: false,
            quality: quality.map(ToOwned::to_owned),
        })
    }
}

fn parse_lrc(value: &str) -> Vec<LyricLine> {
    value
        .lines()
        .filter_map(|line| {
            let close = line.find(']')?;
            let timestamp = line.get(1..close)?;
            let mut parts = timestamp.split(':');
            let minutes: u64 = parts.next()?.parse().ok()?;
            let seconds: f64 = parts.next()?.parse().ok()?;
            Some(LyricLine {
                start_ms: minutes
                    .saturating_mul(60_000)
                    .saturating_add((seconds * 1000.0) as u64),
                text: line.get(close + 1..)?.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lrc_parser_keeps_timing_and_text() {
        let lines = parse_lrc("[00:01.25]hello\n[01:02.50]world");
        assert_eq!(lines[0].start_ms, 1_250);
        assert_eq!(lines[1].start_ms, 62_500);
        assert_eq!(lines[1].text, "world");
    }

    #[test]
    fn song_mapping_preserves_source() {
        let song = NeteaseClient::map_song(
            &json!({
                "id": 42,
                "name": "Track",
                "ar": [{"name": "Artist"}],
                "al": {"name": "Album", "picUrl": "https://img.example/a.jpg"},
                "dt": 1200
            }),
            "work",
        )
        .expect("song");
        assert_eq!(song.source.platform, Platform::Netease);
        assert_eq!(song.source.profile_id, "work");
        assert_eq!(song.source.track_id, "42");
    }
}
