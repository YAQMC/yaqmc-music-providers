use crate::{
    bounded_limit,
    http::{cookie, ApiHttp},
    now_ms, AuthCompletion, AuthPreparation, PlatformAdapter, PlatformError, ProfileSession,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::Method;
use serde_json::Value;
use uuid::Uuid;
use yaqmc_music_providers_protocol::{
    AccountSnapshot, AuthMethod, LibrarySnapshot, LyricLine, Lyrics, Platform, PlaybackSource,
    PlaylistSummary, SearchResult, Track, TrackSource,
};

#[derive(Clone, Debug)]
pub struct KugouConfig {
    pub base_url: String,
}

#[derive(Clone)]
pub struct KugouClient {
    api: ApiHttp,
}

impl KugouClient {
    pub fn new(config: KugouConfig) -> Result<Self, PlatformError> {
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
        let code = value
            .get("code")
            .and_then(Value::as_i64)
            .or_else(|| value.pointer("/data/code").and_then(Value::as_i64))
            .unwrap_or(0);
        if code != 0 && code != 200 {
            return Err(PlatformError::Upstream(format!(
                "KuGou API returned code {code}"
            )));
        }
        Ok(value)
    }

    fn map_song(value: &Value, profile_id: &str) -> Option<Track> {
        let id = value
            .get("hash")
            .or_else(|| value.get("Hash"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())?
            .to_owned();
        let title = value
            .get("songname")
            .or_else(|| value.get("SongName"))
            .or_else(|| value.get("filename"))
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .to_owned();
        let artist = value
            .get("singername")
            .or_else(|| value.get("SingerName"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .into_iter()
            .collect();
        Some(Track {
            source: TrackSource {
                platform: Platform::Kugou,
                profile_id: profile_id.to_owned(),
                track_id: id,
            },
            title,
            artists: artist,
            album: value
                .get("album_name")
                .or_else(|| value.get("AlbumName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            duration_ms: value
                .get("duration")
                .and_then(Value::as_u64)
                .map(|seconds| seconds.saturating_mul(1000))
                .or_else(|| value.get("Duration").and_then(Value::as_u64)),
            artwork_url: value
                .get("trans_param")
                .and_then(|params| params.get("union_cover"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            preview_url: None,
            playable: true,
            metadata: value.clone(),
        })
    }

    fn data_items(value: &Value) -> impl Iterator<Item = &Value> {
        value
            .pointer("/data/info")
            .or_else(|| value.pointer("/data/lists"))
            .or_else(|| value.pointer("/data/song_list"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }
}

#[async_trait]
impl PlatformAdapter for KugouClient {
    fn platform(&self) -> Platform {
        Platform::Kugou
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod {
            id: "qr".to_owned(),
            label: "KuGou QR login".to_owned(),
            requires_login: false,
        }]
    }

    async fn prepare_auth(
        &self,
        _session: &ProfileSession,
        method: &str,
    ) -> Result<AuthPreparation, PlatformError> {
        if method != "qr" {
            return Err(PlatformError::Unsupported);
        }
        let response = self
            .api
            .json::<Value>(
                Method::GET,
                "/login/qr/create",
                &[("timestamp", now_ms().to_string())],
                &[],
                None,
            )
            .await?;
        let payload = response
            .pointer("/data/qr")
            .or_else(|| response.pointer("/data/qrimg"))
            .or_else(|| response.pointer("/data/url"))
            .and_then(Value::as_str)
            .ok_or(PlatformError::InvalidResponse)?;
        Ok(AuthPreparation {
            attempt_id: Uuid::new_v4().to_string(),
            authorization_url: response
                .pointer("/data/url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            qr_payload: Some(payload.to_owned()),
            expires_at_ms: now_ms().saturating_add(5 * 60 * 1000),
            verifier: response
                .pointer("/data/token")
                .or_else(|| response.pointer("/data/key"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            state: None,
        })
    }

    async fn complete_auth(
        &self,
        _session: &ProfileSession,
        preparation: &AuthPreparation,
        _payload: &Value,
    ) -> Result<AuthCompletion, PlatformError> {
        let token = preparation
            .verifier
            .as_deref()
            .ok_or(PlatformError::InvalidResponse)?;
        let response = self
            .api
            .json::<Value>(
                Method::GET,
                "/login/qr/check",
                &[
                    ("token", token.to_owned()),
                    ("timestamp", now_ms().to_string()),
                ],
                &[],
                None,
            )
            .await?;
        let status = response
            .pointer("/data/status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        match status {
            "success" | "logged_in" | "ok" => {
                let secret = response
                    .pointer("/data/cookie")
                    .or_else(|| response.pointer("/data/token"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                Ok(AuthCompletion {
                    secret,
                    display_name: response
                        .pointer("/data/nickname")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    ..AuthCompletion::default()
                })
            }
            "expired" => Err(PlatformError::Upstream("QR login expired".to_owned())),
            _ => Err(PlatformError::AuthenticationPending),
        }
    }

    async fn account_snapshot(
        &self,
        session: &ProfileSession,
    ) -> Result<AccountSnapshot, PlatformError> {
        let Some(secret) = session.secret.as_deref() else {
            return Ok(AccountSnapshot {
                platform: Platform::Kugou,
                profile_id: session.profile_id.clone(),
                authenticated: false,
                display_name: None,
                masked_identity: None,
                entitlement: None,
            });
        };
        let response = self.value(session, "/user/info", &[]).await?;
        Ok(AccountSnapshot {
            platform: Platform::Kugou,
            profile_id: session.profile_id.clone(),
            authenticated: !secret.is_empty(),
            display_name: response
                .pointer("/data/nickname")
                .or_else(|| response.pointer("/data/user_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            masked_identity: response
                .pointer("/data/user_id")
                .and_then(Value::as_str)
                .map(mask_identity),
            entitlement: response
                .pointer("/data/vip_type")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }

    async fn search(
        &self,
        session: &ProfileSession,
        query: &str,
        page: u32,
        limit: u32,
    ) -> Result<SearchResult, PlatformError> {
        let response = self
            .value(
                session,
                "/search",
                &[
                    ("keywords", query.to_owned()),
                    ("page", (page + 1).to_string()),
                    ("pagesize", bounded_limit(limit).to_string()),
                ],
            )
            .await?;
        Ok(SearchResult {
            items: Self::data_items(&response)
                .filter_map(|item| Self::map_song(item, &session.profile_id))
                .collect(),
            next_cursor: Some((page + 1).to_string()),
            platform: Some(Platform::Kugou),
            warnings: Vec::new(),
        })
    }

    async fn library(
        &self,
        session: &ProfileSession,
        limit: u32,
    ) -> Result<LibrarySnapshot, PlatformError> {
        let response = self
            .value(
                session,
                "/user/playlist",
                &[("pagesize", bounded_limit(limit).to_string())],
            )
            .await?;
        let playlists = response
            .pointer("/data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let id = item
                            .get("global_collection_id")
                            .or_else(|| item.get("specialid"))
                            .and_then(Value::as_str)?
                            .to_owned();
                        Some(PlaylistSummary {
                            id: id.clone(),
                            title: item
                                .get("name")
                                .or_else(|| item.get("specialname"))
                                .and_then(Value::as_str)?
                                .to_owned(),
                            track_count: item
                                .get("song_count")
                                .and_then(Value::as_u64)
                                .or_else(|| item.get("songcount").and_then(Value::as_u64)),
                            artwork_url: item
                                .get("img")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            source: TrackSource {
                                platform: Platform::Kugou,
                                profile_id: session.profile_id.clone(),
                                track_id: id,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(LibrarySnapshot {
            platform: Platform::Kugou,
            profile_id: session.profile_id.clone(),
            authenticated: true,
            tracks: Vec::new(),
            playlists,
            warnings: Vec::new(),
        })
    }

    async fn lyrics(
        &self,
        session: &ProfileSession,
        track_id: &str,
    ) -> Result<Lyrics, PlatformError> {
        let response = self
            .value(session, "/lyric", &[("hash", track_id.to_owned())])
            .await?;
        let encoded = response
            .pointer("/data/lyrics")
            .or_else(|| response.pointer("/data/lyric"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let plain = STANDARD
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .or_else(|| (!encoded.is_empty()).then(|| encoded.to_owned()));
        Ok(Lyrics {
            source: TrackSource {
                platform: Platform::Kugou,
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
                "/song/url",
                &[
                    ("hash", track_id.to_owned()),
                    ("quality", quality.unwrap_or("high").to_owned()),
                ],
            )
            .await?;
        let url = response
            .pointer("/data/url")
            .or_else(|| response.pointer("/data/play_url"))
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://"))
            .ok_or(PlatformError::FullPlaybackUnavailable)?;
        Ok(PlaybackSource {
            source: TrackSource {
                platform: Platform::Kugou,
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

fn mask_identity(value: &str) -> String {
    let first = value.chars().next().unwrap_or('*');
    let last = value.chars().last().unwrap_or('*');
    format!("{first}***{last}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_common_kugou_search_shape() {
        let result = KugouClient::map_song(
            &json!({
                "hash": "abc",
                "songname": "Track",
                "singername": "Artist",
                "album_name": "Album",
                "duration": 90
            }),
            "default",
        )
        .expect("track");
        assert_eq!(result.source.platform, Platform::Kugou);
        assert_eq!(result.source.track_id, "abc");
        assert_eq!(result.duration_ms, Some(90_000));
    }

    #[test]
    fn decodes_base64_lyrics() {
        let lyric = STANDARD.encode("[00:02.00]hello");
        let decoded = STANDARD.decode(lyric).expect("decode");
        assert_eq!(String::from_utf8(decoded).expect("utf8"), "[00:02.00]hello");
    }
}
