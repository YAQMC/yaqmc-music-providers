#![no_std]

extern crate alloc;

use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec::Vec,
};
use serde_json::{json, Map, Value};

pub fn dispatch<F>(
    provider_id: &str,
    platform: &str,
    capability: &str,
    operation: &str,
    payload_json: &str,
    request: F,
) -> Result<String, String>
where
    F: FnOnce(String) -> Result<String, String>,
{
    dispatch_with_mode(
        provider_id,
        platform,
        capability,
        operation,
        payload_json,
        "isolated",
        request,
    )
}

pub fn dispatch_with_mode<F>(
    provider_id: &str,
    platform: &str,
    capability: &str,
    operation: &str,
    payload_json: &str,
    mode: &str,
    request: F,
) -> Result<String, String>
where
    F: FnOnce(String) -> Result<String, String>,
{
    if mode != "isolated" && mode != "aggregate" {
        return Err(error("invalid-request", "provider mode is invalid"));
    }
    let mut payload: Value = serde_json::from_str(payload_json)
        .map_err(|_| error("invalid-request", "payload is not valid JSON"))?;
    if !payload.is_object() {
        payload = Value::Object(Map::new());
    }
    let aggregate = mode == "aggregate";
    let mut backend_mode = mode.to_owned();
    let mut backend_platform = (!aggregate).then(|| platform.to_owned());
    prepare_backend_payload(
        platform,
        operation,
        &mut payload,
        aggregate,
        &mut backend_mode,
        &mut backend_platform,
    );
    let profile_id = payload
        .get("profileId")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_owned();
    if payload.get("profileId").is_none() {
        if let Some(object) = payload.as_object_mut() {
            object.insert("profileId".to_owned(), json!(profile_id));
        }
    }
    let backend_operation = backend_operation(capability, operation)?;
    let mut backend_request = json!({
        "id": format!("{provider_id}-{operation}"),
        "version": 1,
        "mode": backend_mode,
        "profileId": profile_id,
        "operation": backend_operation,
        "payload": payload,
    });
    if let Some(platform) = backend_platform {
        backend_request["platform"] = json!(platform);
    }
    let raw = request(backend_request.to_string())?;
    let envelope: Value = serde_json::from_str(&raw)
        .map_err(|_| error("invalid-provider-response", "backend response is not JSON"))?;
    if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(map_backend_error(&envelope));
    }
    let result = envelope.get("result").cloned().unwrap_or(Value::Null);
    map_result(
        provider_id,
        platform,
        capability,
        operation,
        &result,
        aggregate,
    )
}

fn prepare_backend_payload(
    platform: &str,
    operation: &str,
    payload: &mut Value,
    aggregate: bool,
    backend_mode: &mut String,
    backend_platform: &mut Option<String>,
) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    match operation {
        "catalog.song" => {
            if let Some(id) = object.get("id").cloned() {
                if aggregate {
                    let (route_platform, route_track_id) = track_reference(&id, object, platform);
                    if route_platform != platform
                        || id.as_str().is_some_and(|value| value.contains("::"))
                    {
                        object.insert("query".to_owned(), json!(route_track_id));
                        *backend_mode = "isolated".to_owned();
                        *backend_platform = Some(route_platform);
                    } else {
                        object.insert("query".to_owned(), id);
                    }
                } else {
                    object.insert("query".to_owned(), id);
                }
            }
            object.insert("limit".to_owned(), json!(1));
        }
        "lyrics.get" | "playback.resolve" | "playback.resolve-client-fallback" => {
            let track_id = object
                .get("trackId")
                .cloned()
                .or_else(|| object.get("songId").cloned())
                .or_else(|| {
                    object
                        .get("song")
                        .and_then(|song| song.get("provider"))
                        .and_then(|provider| provider.get("trackId"))
                        .cloned()
                })
                .or_else(|| object.get("song").and_then(|song| song.get("id")).cloned());
            if let Some(track_id) = track_id {
                let (route_platform, route_track_id) = if aggregate {
                    track_reference(&track_id, object, platform)
                } else {
                    (
                        platform.to_owned(),
                        track_id.as_str().unwrap_or_default().to_owned(),
                    )
                };
                if !route_track_id.is_empty() {
                    object.insert("trackId".to_owned(), json!(route_track_id));
                }
                if aggregate {
                    *backend_mode = "isolated".to_owned();
                    *backend_platform = Some(route_platform);
                }
            }
        }
        "account.auth.prepare-oauth" => {
            object.insert("method".to_owned(), json!("browser-oauth"));
            object.insert("flow".to_owned(), json!("oauth"));
        }
        "account.auth.start-qr" => {
            object.insert("method".to_owned(), json!("qr"));
            object.insert("flow".to_owned(), json!("qr"));
        }
        "account.auth.complete-oauth" | "account.auth.cancel-oauth" => {
            object.insert("flow".to_owned(), json!("oauth"));
        }
        "account.auth.heartbeat-qr" | "account.auth.cancel-qr" | "account.auth.refresh-qr" => {
            object.insert("flow".to_owned(), json!("qr"));
        }
        _ => {}
    }
}

fn track_reference(
    track_id: &Value,
    object: &Map<String, Value>,
    default_platform: &str,
) -> (String, String) {
    let explicit_platform = object.get("platform").and_then(Value::as_str);
    let raw = track_id.as_str().unwrap_or_default();
    if let Some((route_platform, route_track_id)) = raw.split_once("::") {
        if is_platform(route_platform) && !route_track_id.is_empty() {
            return (route_platform.to_owned(), route_track_id.to_owned());
        }
    }
    (
        explicit_platform
            .filter(|value| is_platform(value))
            .unwrap_or(default_platform)
            .to_owned(),
        raw.to_owned(),
    )
}

fn is_platform(value: &str) -> bool {
    matches!(value, "spotify" | "netease" | "kugou" | "qqmusic")
}

fn backend_operation(capability: &str, operation: &str) -> Result<&'static str, String> {
    match (capability, operation) {
        ("provider.catalog", "catalog.search") => Ok("catalog.search"),
        ("provider.catalog", "catalog.song") => Ok("catalog.search"),
        ("provider.lyrics", "lyrics.get") => Ok("lyrics.get"),
        ("provider.playback", "playback.resolve")
        | ("provider.playback", "playback.resolve-client-fallback") => Ok("playback.resolve"),
        ("provider.playback", "playback.set-preferred-quality")
        | ("provider.playback", "playback.set-current-quality") => Ok("health"),
        ("provider.account", "account.auth.login-methods") => Ok("auth.login-methods"),
        ("provider.account", "account.auth.prepare-oauth")
        | ("provider.account", "account.auth.start-qr") => Ok("auth.prepare"),
        ("provider.account", "account.auth.complete-oauth")
        | ("provider.account", "account.auth.heartbeat-qr") => Ok("auth.complete"),
        ("provider.account", "account.auth.cancel-oauth")
        | ("provider.account", "account.auth.cancel-qr") => Ok("auth.cancel"),
        ("provider.account", "account.auth.refresh-qr") => Ok("auth.refresh"),
        ("provider.account", "account.sign-out") => Ok("auth.logout"),
        ("provider.account", "account.snapshot") => Ok("account.snapshot"),
        ("provider.account", "account.favorite-songs") => Ok("library.snapshot"),
        ("provider.account", "account.playlists") => Ok("library.snapshot"),
        ("provider.account", "account.restore-session") => Ok("health"),
        _ => Err(error(
            "unsupported-operation",
            "the provider operation is not implemented",
        )),
    }
}

fn map_result(
    provider_id: &str,
    platform: &str,
    capability: &str,
    operation: &str,
    value: &Value,
    aggregate: bool,
) -> Result<String, String> {
    match (capability, operation) {
        ("provider.catalog", "catalog.search") => {
            Ok(search_response(provider_id, platform, value, aggregate))
        }
        ("provider.catalog", "catalog.song") => {
            let item = value
                .get("items")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .ok_or_else(|| error("not-found", "song was not found"))?;
            Ok(song(provider_id, platform, item, aggregate).to_string())
        }
        ("provider.lyrics", "lyrics.get") => Ok(lyrics(value, aggregate).to_string()),
        ("provider.playback", "playback.resolve")
        | ("provider.playback", "playback.resolve-client-fallback") => playback(value),
        ("provider.playback", "playback.set-preferred-quality") => {
            Ok(status(provider_id, platform).to_string())
        }
        ("provider.playback", "playback.set-current-quality")
        | ("provider.account", "account.restore-session") => Ok("null".to_owned()),
        ("provider.account", "account.auth.login-methods") => Ok(login_methods(value, platform)),
        ("provider.account", "account.auth.prepare-oauth") => Ok(oauth_prepare(value)),
        ("provider.account", "account.auth.start-qr")
        | ("provider.account", "account.auth.heartbeat-qr")
        | ("provider.account", "account.auth.refresh-qr")
        | ("provider.account", "account.auth.cancel-qr") => {
            Ok(qr_snapshot(provider_id, platform, value))
        }
        ("provider.account", "account.auth.complete-oauth")
        | ("provider.account", "account.sign-out")
        | ("provider.account", "account.snapshot") => {
            Ok(account_snapshot(provider_id, platform, value))
        }
        ("provider.account", "account.auth.cancel-oauth") => {
            Ok(account_snapshot(provider_id, platform, value))
        }
        ("provider.account", "account.favorite-songs") => {
            Ok(page(value, provider_id, platform, aggregate))
        }
        ("provider.account", "account.playlists") => {
            Ok(playlist_page(value, provider_id, platform))
        }
        _ => Err(error(
            "unsupported-operation",
            "the provider operation is not implemented",
        )),
    }
}

fn search_response(provider_id: &str, platform: &str, value: &Value, aggregate: bool) -> String {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| song(provider_id, platform, item, aggregate))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "kind": "song",
        "query": "",
        "page": 1,
        "hasMore": value.get("nextCursor").is_some_and(|cursor| !cursor.is_null()),
        "items": items
    })
    .to_string()
}

fn song(provider_id: &str, platform: &str, value: &Value, aggregate: bool) -> Value {
    let source = value.get("source").cloned().unwrap_or(Value::Null);
    let source_platform = source
        .get("platform")
        .and_then(Value::as_str)
        .unwrap_or(platform);
    let track_id = source
        .get("trackId")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let exposed_track_id = if aggregate {
        format!("{source_platform}::{track_id}")
    } else {
        track_id.to_owned()
    };
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
    let artists = value
        .get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    let name = artist.as_str().unwrap_or("Unknown");
                    json!({"id": format!("{source_platform}-artist-{index}"), "name": name})
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let album_title = value
        .get("album")
        .and_then(Value::as_str)
        .unwrap_or("Unknown album");
    let artwork = value
        .get("artworkUrl")
        .and_then(Value::as_str)
        .unwrap_or("");
    let preview = value
        .get("previewUrl")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty());
    let playable = value
        .get("playable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let availability = if playable {
        json!({"status": "available"})
    } else {
        json!({"status": "unavailable", "reason": "source-unavailable"})
    };
    let playback_capability = if preview.is_some() {
        json!({"status": "preview", "startMs": 0, "endMs": 30_000})
    } else if playable {
        json!({"status": "full"})
    } else {
        json!({"status": "unavailable", "reason": "source-unavailable"})
    };
    json!({
        "id": exposed_track_id,
        "title": title,
        "artists": artists,
        "album": {"id": format!("{source_platform}-album"), "title": album_title},
        "artwork": {
            "src": artwork,
            "alt": title,
            "dominantColor": "#334155",
            "variants": []
        },
        "durationMs": value.get("durationMs").and_then(Value::as_u64).unwrap_or(0),
        "trackNumber": 0,
        "isFavorite": false,
        "quality": "standard",
        "availability": availability,
        "playbackCapability": playback_capability,
        "provider": {
            "providerId": provider_id,
            "profileId": source.get("profileId").and_then(Value::as_str).unwrap_or("default"),
            "trackId": if aggregate { format!("{source_platform}::{track_id}") } else { track_id.to_owned() }
        }
    })
}

fn lyrics(value: &Value, aggregate: bool) -> Value {
    let source = value.get("source").cloned().unwrap_or(Value::Null);
    let track_id = source
        .get("trackId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_platform = source
        .get("platform")
        .and_then(Value::as_str)
        .unwrap_or("provider");
    let song_id = if aggregate {
        format!("{source_platform}::{track_id}")
    } else {
        track_id.to_owned()
    };
    let lines = value
        .get("synced")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    json!({
                        "id": format!("line-{index}"),
                        "startMs": line.get("startMs").and_then(Value::as_u64),
                        "endMs": Value::Null,
                        "text": line.get("text").and_then(Value::as_str).unwrap_or(""),
                        "words": []
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "songId": song_id,
        "syncMode": if lines.is_empty() { "unsynchronized" } else { "line" },
        "metadata": {"sourceLabel": "YAQMC Music Providers", "offsetMs": 0},
        "vocalists": [],
        "lines": lines
    })
}

fn playback(value: &Value) -> Result<String, String> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://127.0.0.1:"))
        .ok_or_else(|| {
            error(
                "full-playback-unavailable",
                "backend returned no safe media URL",
            )
        })?;
    let source = value.get("source").cloned().unwrap_or(Value::Null);
    let preview = value
        .get("isPreview")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let length = value
        .get("contentLength")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("full-playback-unavailable", "media length is unknown"))?;
    Ok(json!({
        "source": {"kind": "https", "request": {"method": "GET", "url": url}},
        "cacheKey": format!(
            "{}:{}",
            source.get("platform").and_then(Value::as_str).unwrap_or("provider"),
            source.get("trackId").and_then(Value::as_str).unwrap_or("track")
        ),
        "format": "mp3",
        "mimeType": value.get("mimeType").cloned().unwrap_or(json!("audio/mpeg")),
        "qualityLabel": value.get("quality").cloned().unwrap_or(json!("standard")),
        "contentLength": length,
        "isPreview": preview,
        "selection": {
            "requestedQuality": "automatic",
            "resolvedQuality": value.get("quality").cloned().unwrap_or(json!("standard")),
            "preview": preview,
            "qualityCapabilities": []
        }
    })
    .to_string())
}

fn account_snapshot(provider_id: &str, platform: &str, value: &Value) -> String {
    let authenticated = value
        .get("authenticated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let profile_id = value
        .get("profileId")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let state = if authenticated {
        json!({
            "state": "authenticated",
            "profile": {
                "avatarUrl": null,
                "nickname": value.get("displayName").cloned().unwrap_or(json!(platform)),
                "maskedIdentity": value.get("maskedIdentity").cloned().unwrap_or(json!("masked"))
            },
            "entitlement": {
                "tier": "free",
                "membership": "active",
                "expiresAtMs": null,
                "permittedQualities": ["standard"],
                "observedMaximumQuality": "standard",
                "restrictions": []
            }
        })
    } else {
        json!({"state": "guest", "profile": null, "entitlement": null})
    };
    let mut object = state.as_object().cloned().unwrap_or_default();
    object.insert("providerId".to_owned(), json!(provider_id));
    object.insert("profileId".to_owned(), json!(profile_id));
    object.insert(
        "revision".to_owned(),
        json!(if authenticated { 1 } else { 0 }),
    );
    object.insert(
        "capabilities".to_owned(),
        json!({
            "qrLogin": platform != "spotify",
            "favoriteRead": authenticated,
            "favoriteWrite": false,
            "playlistRead": authenticated,
            "playlistWrite": false,
            "recentHistoryRead": false
        }),
    );
    Value::Object(object).to_string()
}

fn login_methods(value: &Value, platform: &str) -> String {
    if platform != "spotify" {
        return "[]".to_owned();
    }
    let methods = value
        .get("methods")
        .and_then(Value::as_array)
        .map(|methods| {
            methods
                .iter()
                .filter(|method| {
                    method
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id == "browser-oauth")
                })
                .map(|method| {
                    json!({
                        "id": method.get("id").cloned().unwrap_or(json!("browser-oauth")),
                        "label": method.get("label").cloned().unwrap_or(json!("Browser login")),
                        "flow": "oauth"
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Value::Array(methods).to_string()
}

fn oauth_prepare(value: &Value) -> String {
    let url = value
        .get("authorizationUrl")
        .cloned()
        .unwrap_or(json!("http://127.0.0.1:43821"));
    json!({
        "url": url,
        "navigationAllowlist": [url],
        "callbackMatcher": {"urlPrefix": "http://127.0.0.1:43821/oauth/spotify/callback"}
    })
    .to_string()
}

fn qr_snapshot(provider_id: &str, platform: &str, value: &Value) -> String {
    if value
        .get("authenticated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return account_snapshot(provider_id, platform, value);
    }
    let profile_id = value
        .get("profileId")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let state = match status {
        "expired" => "expired",
        "cancelled" => "cancelled",
        "rejected" => "rejected",
        "network-error" => "network-error",
        _ => {
            if value.get("phase").and_then(Value::as_str) == Some("waiting-for-confirmation") {
                "waiting-for-confirmation"
            } else {
                "waiting-for-scan"
            }
        }
    };
    let snapshot = account_snapshot(
        provider_id,
        platform,
        &json!({"authenticated": false, "profileId": profile_id}),
    );
    let mut object: Value = serde_json::from_str(&snapshot).unwrap_or(Value::Null);
    if let Some(map) = object.as_object_mut() {
        map.insert("state".to_owned(), json!(state));
        map.insert(
            "attemptId".to_owned(),
            value.get("attemptId").cloned().unwrap_or(Value::Null),
        );
        map.insert("ownerLeaseId".to_owned(), json!("backend"));
        map.insert(
            "qrImageDataUri".to_owned(),
            if matches!(state, "waiting-for-scan" | "waiting-for-confirmation") {
                json!(value.get("qrPayload").and_then(Value::as_str).unwrap_or(""))
            } else {
                json!("")
            },
        );
        map.insert(
            "expiresAtMs".to_owned(),
            value.get("expiresAtMs").cloned().unwrap_or(json!(0)),
        );
        map.insert("pollAfterMs".to_owned(), json!(1500));
    }
    object.to_string()
}

fn page(value: &Value, provider_id: &str, platform: &str, aggregate: bool) -> String {
    let items = value
        .get("tracks")
        .and_then(Value::as_array)
        .map(|tracks| {
            tracks
                .iter()
                .map(|track| song(provider_id, platform, track, aggregate))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let total = items.len();
    json!({
        "items": items,
        "nextCursor": null,
        "total": total,
        "fetchedAtMs": 0,
        "stale": false,
        "authRevision": 0
    })
    .to_string()
}

fn playlist_page(value: &Value, provider_id: &str, platform: &str) -> String {
    let playlists = value
        .get("playlists")
        .and_then(Value::as_array)
        .map(|playlists| {
            playlists
                .iter()
                .map(|playlist| {
                    json!({
                        "providerId": provider_id,
                        "profileId": playlist
                            .get("source")
                            .and_then(|source| source.get("profileId"))
                            .and_then(Value::as_str)
                            .unwrap_or("default"),
                        "id": playlist.get("id").cloned().unwrap_or(json!("")),
                        "reference": {"kind": "owned", "tid": playlist.get("id").cloned().unwrap_or(json!(""))},
                        "title": playlist.get("title").cloned().unwrap_or(json!("Playlist")),
                        "description": "",
                        "owner": {"id": platform, "displayName": platform},
                        "artwork": {"src": playlist.get("artworkUrl").cloned().unwrap_or(json!("")), "alt": "Playlist", "dominantColor": "#334155", "variants": []},
                        "ownership": "owned",
                        "capabilities": {"canAddTracks": false, "canRemoveTracks": false, "canRename": false, "canDelete": false, "canReorder": false},
                        "trackCount": playlist.get("trackCount").cloned().unwrap_or(json!(0)),
                        "updatedAtMs": null
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let total = playlists.len();
    json!({
        "items": playlists,
        "nextCursor": null,
        "total": total,
        "fetchedAtMs": 0,
        "stale": false,
        "authRevision": 0
    })
    .to_string()
}

fn status(provider_id: &str, platform: &str) -> Value {
    json!({
        "providerId": provider_id,
        "profileId": "default",
        "displayName": platform,
        "connection": "ready",
        "message": "",
        "preferredQuality": "automatic",
        "capabilities": {
            "search": true,
            "album": true,
            "artist": true,
            "playlist": true,
            "lyrics": true,
            "wordTimedLyrics": false,
            "streaming": true,
            "qualitySelection": true
        }
    })
}

fn map_backend_error(envelope: &Value) -> String {
    let error = envelope.get("error").cloned().unwrap_or(Value::Null);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("provider-failure");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("backend request failed");
    let retryable = error
        .get("retryable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    json!({"code": code, "message": message, "retryable": retryable}).to_string()
}

fn error(code: &str, message: &str) -> String {
    json!({"code": code, "message": message, "retryable": false}).to_string()
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn account_operations_use_separate_backend_lifecycle_routes() {
        assert_eq!(
            backend_operation("provider.account", "account.auth.cancel-qr").unwrap(),
            "auth.cancel"
        );
        assert_eq!(
            backend_operation("provider.account", "account.auth.refresh-qr").unwrap(),
            "auth.refresh"
        );
        assert_eq!(
            backend_operation("provider.account", "account.sign-out").unwrap(),
            "auth.logout"
        );
    }

    #[test]
    fn only_spotify_exposes_oauth_login_methods() {
        let value = json!({
            "methods": [
                {"id": "browser-oauth", "label": "Spotify", "requiresLogin": false},
                {"id": "qr", "label": "QR", "requiresLogin": false}
            ]
        });
        let spotify: Value = serde_json::from_str(&login_methods(&value, "spotify")).unwrap();
        assert_eq!(spotify.as_array().unwrap().len(), 1);
        assert_eq!(spotify[0]["flow"], "oauth");
        assert_eq!(login_methods(&value, "netease"), "[]");
    }

    #[test]
    fn qr_terminal_states_are_exposed_without_logging_out() {
        let pending: Value = serde_json::from_str(&qr_snapshot(
            "org.example.provider",
            "netease",
            &json!({
                "status": "pending",
                "attemptId": "attempt-1",
                "profileId": "work",
                "qrPayload": "data:image/png;base64,AA==",
                "expiresAtMs": 123
            }),
        ))
        .unwrap();
        assert_eq!(pending["state"], "waiting-for-scan");
        assert_eq!(pending["qrImageDataUri"], "data:image/png;base64,AA==");

        let cancelled: Value = serde_json::from_str(&qr_snapshot(
            "org.example.provider",
            "netease",
            &json!({"status": "cancelled", "attemptId": "attempt-1", "profileId": "work"}),
        ))
        .unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        assert_eq!(cancelled["profileId"], "work");
    }

    #[test]
    fn dispatch_marks_qr_cancellation_as_qr_flow() {
        let response = dispatch_with_mode(
            "org.example.provider",
            "kugou",
            "provider.account",
            "account.auth.cancel-qr",
            r#"{"attemptId":"attempt-1"}"#,
            "isolated",
            |body| {
                let request: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(request["operation"], "auth.cancel");
                assert_eq!(request["payload"]["flow"], "qr");
                Ok(json!({
                    "ok": true,
                    "result": {
                        "status": "cancelled",
                        "attemptId": "attempt-1",
                        "profileId": "default"
                    }
                })
                .to_string())
            },
        )
        .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["state"], "cancelled");
    }
}
