use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec::Vec,
};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha1::Sha1;

use super::{
    clear_credential, credential_handle, credential_key, error, json_u64, mark_favorite, now_ms,
    playlist_summary, random_bytes, replace_credential,
    request_with_meta_headers_and_credentials_as, request_with_meta_headers_as, song_value,
    storage_get, storage_set,
};

const OPEN_SPOTIFY: &str = "https://open.spotify.com";
const SPOTIFY_API: &str = "https://api.spotify.com";
const API_PARTNER: &str = "https://api-partner.spotify.com";
const CLIENT_TOKEN: &str = "https://clienttoken.spotify.com";
const SPCLIENT: &str = "https://spclient.wg.spotify.com";
const WEBGATE_SPCLIENT: &str = "https://guc3-spclient.spotify.com";
const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const CLIENT_VERSION: &str = "1.3.8.509.g26b6eac7300d";
const WEB_TOKEN_REQUEST_VERSION: &str = "web-player-v1";
const WEB_PLAYER_SDK_VERSION: &str = "4.82.0";
const BROWSER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
const SEARCH_TRACKS_HASH: &str = "b02683192a98dde7966b5e6655a79eeb62713eab703eda9902c932818dd52751";
const GET_TRACK_HASH: &str = "fe16b8083eec2305854db612ca75589bcb71e009dc7a4d014f29bd534a4c01f5";
const SEARCH_PLAYLISTS_HASH: &str =
    "d520014e748f9ea44f7707d8df1819867ac1205e8b7f3e28f22fe5fc858921b1";
const FETCH_PLAYLIST_HASH: &str =
    "21ea11a6ac75a5f3e01fd365d29c18e0a4a8b7dc09f89bbb5d73910190c6acaf";
const PROFILE_ATTRIBUTES_HASH: &str =
    "08ffb4730af3746e04a8301396f20875dbbce10c75243803091a9274eacc8ac0";
const LIBRARY_V3_HASH: &str = "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455";
const FETCH_LIBRARY_TRACKS_HASH: &str =
    "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240";
const HOME_HASH: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";
const BROWSE_PAGE_HASH: &str = "f5c4e6d668f5716464a231c1cc8b22c1cbf6ad68b09929fd7de813a30581298b";
const GET_ALBUM_HASH: &str = "0b6ce3511e1b02e4a40fa95440f992acbe85080eb308af9194322d708fc1914f";

const TOTP_SECRETS: [(&str, u32); 3] = [
    (r#",7/*F("rLJ2oxaKL^f+E1xvP@N"#, 61),
    (r#"OmE{ZA.J^":0FG\Uz?[@WW"#, 60),
    (r#"{iOFn;4}<1PFYKPV?5{%u14]M>/V0hDH"#, 59),
];

pub(super) const RESPONSE_FIELDS: &[&str] = &[
    "errors",
    "message",
    "data",
    "searchV2",
    "tracksV2",
    "playlists",
    "items",
    "item",
    "__typename",
    "uri",
    "id",
    "name",
    "artists",
    "profile",
    "albumOfTrack",
    "coverArt",
    "sources",
    "url",
    "width",
    "height",
    "duration",
    "trackDuration",
    "totalMilliseconds",
    "playability",
    "playable",
    "trackUnion",
    "firstArtist",
    "otherArtists",
    "description",
    "ownerV2",
    "username",
    "content",
    "totalCount",
    "images",
    "playlistV2",
    "itemV2",
    "me",
    "library",
    "libraryV3",
    "tracks",
    "track",
    "_uri",
    "avatar",
    "avatarBackgroundColor",
    "isAnonymous",
    "accessToken",
    "accessTokenExpirationTimestampMs",
    "clientId",
    "home",
    "sectionContainer",
    "sections",
    "sectionItems",
    "title",
    "transformedLabel",
    "browse",
    "albumUnion",
    "date",
    "year",
    "isoString",
    "revisionId",
    "count",
    "previews",
    "audioPreviews",
    "topTracks",
    "discography",
];

pub(super) struct AccountProfile {
    pub(super) username: String,
    pub(super) nickname: String,
    pub(super) avatar_url: Option<String>,
}

pub(super) struct AuthenticatedWebSession {
    pub(super) access_handle: String,
    pub(super) preview_access_handle: String,
    pub(super) client_token_handle: String,
    pub(super) media_access_handle: String,
    pub(super) media_client_token_handle: String,
    pub(super) license_access_handle: String,
    pub(super) license_client_token_handle: String,
    pub(super) expires_ms: u64,
    pub(super) client_token_expires_ms: u64,
    pub(super) profile: AccountProfile,
}

struct WebAccess {
    access_handle: String,
    client_token_handle: String,
    media_access_handle: String,
    media_client_token_handle: String,
    license_access_handle: String,
    license_client_token_handle: String,
    expires_ms: u64,
}

struct WebToken {
    access_token: String,
    client_id: String,
    expires_ms: u64,
}

struct ClientToken {
    token: String,
    expires_ms: u64,
}

pub(super) fn home(profile: &str) -> Result<String, String> {
    let response = pathfinder_query(
        profile,
        "home",
        HOME_HASH,
        json!({
            "timeZone":"Asia/Shanghai", "sp_t":"", "facet":"music-chip", "sectionItemsLimit":3,
            "homeEndUserIntegration":"INTEGRATION_WEB_PLAYER"
        }),
    )?;
    let sections = response
        .pointer("/data/home/sectionContainer/sections/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify home sections are missing",
                false,
            )
        })?;
    let mut playlists = Vec::new();
    let mut albums = Vec::new();
    let mut recent = Vec::new();
    for section in sections {
        for item in section
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(entity) = item.pointer("/content/data") else {
                continue;
            };
            let value = match entity.get("__typename").and_then(Value::as_str) {
                Some("Playlist") => playlist_card(entity).map(|card| {
                    if !playlists.iter().any(|old: &Value| old["id"] == card["id"]) {
                        playlists.push(card.clone());
                    }
                    json!({"type":"playlist","item":card})
                }),
                Some("Album") => album_card(entity).map(|card| {
                    if !albums.iter().any(|old: &Value| old["id"] == card["id"]) {
                        albums.push(card.clone());
                    }
                    json!({"type":"album","item":card})
                }),
                _ => None,
            };
            if section.pointer("/data/__typename").and_then(Value::as_str)
                == Some("HomeRecentlyPlayedSectionData")
            {
                if let Some(value) = value {
                    recent.push(value);
                }
            }
        }
    }
    let featured = if let Some(album) = albums.first() {
        album.clone()
    } else {
        let first = playlists
            .first()
            .and_then(|playlist| playlist["id"].as_str())
            .ok_or_else(|| {
                error(
                    "unavailable",
                    "Spotify returned no music recommendations",
                    false,
                )
            })?;
        let detail = pathfinder_query(
            profile,
            "fetchPlaylist",
            FETCH_PLAYLIST_HASH,
            json!({"uri":format!("spotify:playlist:{first}"),"offset":0,"limit":1,"enableWatchFeedEntrypoint":false}),
        )?;
        detail
            .pointer("/data/playlistV2/content/items/0/itemV2/data/albumOfTrack")
            .and_then(album_card)
            .ok_or_else(|| {
                error(
                    "malformed-response",
                    "Spotify featured album is missing",
                    false,
                )
            })?
    };
    Ok(
        json!({"featured":{"eyebrow":"Spotify","album":featured},"recentlyPlayed":recent,
        "madeForYou":[],"newReleases":albums,"recommendedSonglists":playlists})
        .to_string(),
    )
}

pub(super) fn discover(profile: &str) -> Result<String, String> {
    let response = pathfinder_query(
        profile,
        "browsePage",
        BROWSE_PAGE_HASH,
        json!({
            "uri":"spotify:page:0JQ5DAudkNjCgYMM0TZXDw", "pagePagination":{"offset":0,"limit":10},
            "sectionPagination":{"offset":0,"limit":20},"browseEndUserIntegration":"INTEGRATION_WEB_PLAYER"
        }),
    )?;
    let sections = response
        .pointer("/data/browse/sections/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify chart sections are missing",
                false,
            )
        })?;
    let mut charts = Vec::new();
    for section in sections {
        for item in section
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(card) = item.pointer("/content/data").and_then(playlist_card) {
                if !charts.iter().any(|old: &Value| old["id"] == card["id"]) {
                    charts.push(card);
                }
            }
        }
    }
    Ok(json!({"charts":charts}).to_string())
}

fn playlist_card(item: &Value) -> Option<Value> {
    let preview = playlist_preview(item)?;
    Some(json!({"id":preview["id"], "title":preview["title"],
        "description":item.get("description").and_then(Value::as_str).unwrap_or_default(),
        "owner":{"id":item.pointer("/ownerV2/data/username").and_then(Value::as_str).unwrap_or("spotify"),"displayName":preview["creator"]},
        "artwork":preview["artwork"],"updatedLabel":"","tracks":[]}))
}

fn album_card(item: &Value) -> Option<Value> {
    let id = item
        .get("uri")
        .and_then(Value::as_str)?
        .strip_prefix("spotify:album:")?;
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Unknown album");
    let artist = item.pointer("/artists/items/0");
    let artist_id = artist
        .and_then(|artist| artist.get("uri"))
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("spotify:artist:"))
        .unwrap_or("spotify");
    let year = item
        .pointer("/date/year")
        .and_then(Value::as_u64)
        .or_else(|| {
            item.pointer("/date/isoString")
                .and_then(Value::as_str)?
                .get(..4)?
                .parse()
                .ok()
        })
        .unwrap_or(0);
    Some(json!({"id":id,"title":title,"artist":{"id":artist_id,
        "name":artist.and_then(|artist| artist.pointer("/profile/name")).and_then(Value::as_str).unwrap_or("Spotify")},
        "artwork":artwork_from_value(item).unwrap_or_else(|| empty_artwork(title)),"releaseYear":year,
        "genre":"","description":"","tracks":[]}))
}

pub(super) fn album(profile: &str, payload: &Value) -> Result<String, String> {
    let id = super::track_id(payload);
    if id.is_empty() {
        return Err(error("invalid-request", "album ID is empty", false));
    }
    let mut offset = 0;
    let mut result = None;
    let signed_in = web_account_signed_in(profile)?;
    loop {
        let response = pathfinder_query(
            profile,
            "getAlbum",
            GET_ALBUM_HASH,
            json!({"uri":format!("spotify:album:{id}"),"locale":"","offset":offset,"limit":50}),
        )?;
        let item = response
            .pointer("/data/albumUnion")
            .ok_or_else(|| error("malformed-response", "Spotify album is missing", false))?;
        let tracks = item
            .pointer("/tracksV2/items")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                error(
                    "malformed-response",
                    "Spotify album tracks are missing",
                    false,
                )
            })?;
        let mut album = result
            .take()
            .or_else(|| album_card(item))
            .ok_or_else(|| error("not-found", "Spotify album was not found", false))?;
        for track in tracks.iter().filter_map(|item| item.get("track")) {
            if let Some(mut song) = map_track(track, profile, None, signed_in) {
                song["album"] = json!({"id":id,"title":album["title"]});
                song["artwork"] = album["artwork"].clone();
                album["tracks"].as_array_mut().unwrap().push(song);
            }
        }
        offset += tracks.len() as u64;
        let total = item
            .pointer("/tracksV2/totalCount")
            .and_then(Value::as_u64)
            .unwrap_or(offset);
        if offset >= total || tracks.is_empty() {
            return Ok(album.to_string());
        }
        if offset >= 10_000 {
            return Err(error(
                "unavailable",
                "Spotify album exceeds the supported size",
                false,
            ));
        }
        result = Some(album);
    }
}

pub(super) fn search(
    profile: &str,
    query: &str,
    page: u64,
    limit: u64,
) -> Result<(Vec<Value>, bool), String> {
    let variables = json!({
        "searchTerm": query,
        "offset": page.saturating_sub(1).saturating_mul(limit),
        "limit": limit,
        "numberOfTopResults": 20,
        "includeAudiobooks": false,
        "includePreReleases": false,
        "includeAuthors": false
    });
    let response = pathfinder_query(profile, "searchTracks", SEARCH_TRACKS_HASH, variables)?;
    let raw_items = response
        .pointer("/data/searchV2/tracksV2/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify Web Player returned no track search page",
                true,
            )
        })?;
    let signed_in = web_account_signed_in(profile)?;
    let items = raw_items
        .iter()
        .filter_map(|item| item.pointer("/item/data"))
        .filter_map(|track| map_track(track, profile, None, signed_in))
        .collect::<Vec<_>>();
    Ok((items, raw_items.len() as u64 >= limit))
}

pub(super) fn search_playlists(
    profile: &str,
    query: &str,
    page: u64,
    limit: u64,
) -> Result<(Vec<Value>, bool), String> {
    let variables = json!({
        "searchTerm": query,
        "offset": page.saturating_sub(1).saturating_mul(limit),
        "limit": limit,
        "numberOfTopResults": 5,
        "includeAudiobooks": false,
        "includePreReleases": false,
        "includeAuthors": false
    });
    let response = pathfinder_query(profile, "searchPlaylists", SEARCH_PLAYLISTS_HASH, variables)?;
    let raw_items = response
        .pointer("/data/searchV2/playlists/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify Web Player returned no playlist search page",
                true,
            )
        })?;
    let items = raw_items
        .iter()
        .filter_map(|item| item.get("data"))
        .filter_map(playlist_preview)
        .collect::<Vec<_>>();
    Ok((items, raw_items.len() as u64 >= limit))
}

pub(super) fn playlist(profile: &str, id: &str, payload: &Value) -> Result<String, String> {
    if id.is_empty() {
        return Err(error("invalid-request", "playlist ID is empty", false));
    }
    let cursor = match payload.get("cursor").and_then(Value::as_str) {
        None => 0,
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|offset| *offset <= 10_000_000)
            .ok_or_else(|| error("invalid-request", "playlist cursor is invalid", false))?,
    };
    const PAGE: u64 = 20;
    let response = pathfinder_query(
        profile,
        "fetchPlaylist",
        FETCH_PLAYLIST_HASH,
        json!({
            "uri": format!("spotify:playlist:{id}"),
            "offset": cursor,
            "limit": PAGE,
            "enableWatchFeedEntrypoint": false
        }),
    )?;
    let playlist = response
        .pointer("/data/playlistV2")
        .filter(|value| value.get("uri").and_then(Value::as_str).is_some())
        .ok_or_else(|| error("not-found", "Spotify playlist was not found", false))?;
    let raw_items = playlist
        .pointer("/content/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify playlist tracks are missing",
                true,
            )
        })?;
    let signed_in = web_account_signed_in(profile)?;
    let tracks = raw_items
        .iter()
        .filter_map(|item| item.pointer("/itemV2/data"))
        .filter_map(|track| map_track(track, profile, None, signed_in))
        .collect::<Vec<_>>();
    let total = playlist_track_count(playlist).unwrap_or(raw_items.len() as u64);
    let consumed = raw_items.len() as u64;
    let next = (consumed > 0 && cursor.saturating_add(consumed) < total)
        .then(|| cursor.saturating_add(consumed).to_string());
    let title = playlist
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled playlist");
    let playlist_id = playlist
        .get("uri")
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("spotify:playlist:"))
        .unwrap_or(id);
    let owner = playlist.get("ownerV2").and_then(|owner| owner.get("data"));
    let mut value = json!({
        "id": playlist_id,
        "title": title,
        "description": playlist.get("description").and_then(Value::as_str).unwrap_or_default(),
        "owner": {
            "id": owner.and_then(|value| value.get("username")).and_then(Value::as_str).unwrap_or_default(),
            "displayName": owner.and_then(|value| value.get("name")).and_then(Value::as_str).unwrap_or("Spotify")
        },
        "artwork": artwork_from_value(playlist).unwrap_or_else(|| empty_artwork(title)),
        "updatedLabel": "",
        "tracks": tracks,
        "nextCursor": next
    });
    value["trackCount"] = json!(total);
    Ok(value.to_string())
}

fn playlist_preview(playlist: &Value) -> Option<Value> {
    let id = playlist
        .get("uri")
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("spotify:playlist:"))?;
    let title = playlist
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled playlist");
    Some(json!({
        "id": id,
        "title": title,
        "creator": playlist
            .pointer("/ownerV2/data/name")
            .and_then(Value::as_str)
            .unwrap_or("Spotify"),
        "artwork": artwork_from_value(playlist).unwrap_or_else(|| empty_artwork(title)),
        "trackCount": playlist_track_count(playlist).unwrap_or(0)
    }))
}

fn artwork_from_value(value: &Value) -> Option<Value> {
    let sources = value
        .pointer("/coverArt/sources")
        .or_else(|| value.pointer("/images/items/0/sources"))
        .or_else(|| value.pointer("/images/sources"))
        .and_then(Value::as_array)?;
    let source = sources.iter().max_by_key(|source| {
        source
            .get("width")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_mul(source.get("height").and_then(Value::as_u64).unwrap_or(0))
    })?;
    let src = source
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let src = if let Some(ids) = src.strip_prefix("spotify:mosaic:") {
        format!("https://mosaic.scdn.co/640/{}", ids.replace(':', ""))
    } else {
        src.to_owned()
    };
    Some(json!({
        "src": src,
        "alt": value.get("name").and_then(Value::as_str).unwrap_or_default(),
        "dominantColor": "#334155",
        "variants": []
    }))
}

fn empty_artwork(title: &str) -> Value {
    json!({
        "src": "",
        "alt": title,
        "dominantColor": "#334155",
        "variants": []
    })
}

pub(super) fn song(profile: &str, id: &str) -> Result<Value, String> {
    let response = pathfinder_query(
        profile,
        "getTrack",
        GET_TRACK_HASH,
        json!({"uri": format!("spotify:track:{id}")}),
    )?;
    let track = response
        .pointer("/data/trackUnion")
        .ok_or_else(|| error("not-found", "Spotify track was not found", false))?;
    map_track(track, profile, Some(id), web_account_signed_in(profile)?).ok_or_else(|| {
        error(
            "malformed-response",
            "Spotify Web Player returned an incomplete track",
            true,
        )
    })
}

fn request_web_media(access: &WebAccess, uri: &str) -> Result<Value, String> {
    let media_url = format!(
        "{SPCLIENT}/track-playback/v1/media/{uri}?manifestFileFormat=file_ids_mp4&manifestFileFormat=file_ids_mp4_dual"
    );
    request_with_meta_headers_and_credentials_as::<Value>(
        "GET",
        &media_url,
        None,
        None,
        None,
        None,
        &[
            ("accept", "application/json".to_owned()),
            ("app-platform", "WebPlayer".to_owned()),
            ("referer", "https://open.spotify.com/".to_owned()),
            ("user-agent", BROWSER_AGENT.to_owned()),
        ],
        &[
            (&access.media_access_handle, "authorization", "Bearer "),
            (&access.media_client_token_handle, "client-token", ""),
        ],
    )
    .map(|response| response.body)
}

pub(super) fn playback(profile: &str, id: &str, quality: &str) -> Result<String, String> {
    let uri = format!("spotify:track:{id}");
    let mut access = ensure_web_access(profile)?;
    let mut media_response = request_web_media(&access, &uri);
    if media_response
        .as_ref()
        .is_err_and(|reason| error_code(reason).as_deref() == Some("authentication-expired"))
    {
        clear_web_access(profile);
        access = ensure_web_access(profile)?;
        media_response = request_web_media(&access, &uri);
    }
    let media = media_response?;
    let entries = media
        .get("media")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            error(
                "full-playback-unavailable",
                "Spotify returned no media manifest",
                false,
            )
        })?;
    let entry = entries
        .get(&uri)
        .or_else(|| {
            entries.values().find(|entry| {
                entry.pointer("/item/track_type").and_then(Value::as_str) != Some("VIDEO")
            })
        })
        .ok_or_else(|| {
            error(
                "full-playback-unavailable",
                "Spotify returned no audio entry for this track",
                false,
            )
        })?;
    let manifest = entry
        .pointer("/item/manifest")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            error(
                "full-playback-unavailable",
                "Spotify returned no audio renditions",
                false,
            )
        })?;
    let mut files = Vec::new();
    for format in ["file_ids_mp4", "file_ids_mp4_dual"] {
        for file in manifest
            .get(format)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let file_id = file
                .get("file_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let track_type = file.get("track_type").and_then(Value::as_str);
            if file_id.len() == 40
                && file_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && matches!(track_type, None | Some("") | Some("AUDIO"))
            {
                files.push((
                    json_u64(file, &["bitrate"]).unwrap_or(0),
                    file_id.to_owned(),
                ));
            }
        }
        if !files.is_empty() {
            break;
        }
    }
    files.sort_by_key(|(bitrate, _)| *bitrate);
    let (bitrate, file_id) = files.into_iter().next().ok_or_else(|| {
        error(
            "full-playback-unavailable",
            "Spotify returned no playable audio file",
            false,
        )
    })?;
    let duration_ms = entry
        .pointer("/item/metadata/duration")
        .and_then(Value::as_u64)
        .or_else(|| {
            entry
                .pointer("/item/metadata/duration")
                .and_then(Value::as_f64)
                .map(|value| value.max(0.0) as u64)
        })
        .unwrap_or(0);
    let license_url = web_player_license_url(&access)?;
    Ok(web_player_playback_source(
        &access,
        id,
        quality,
        &file_id,
        bitrate,
        duration_ms,
        &license_url,
    )
    .to_string())
}

fn web_player_license_url(access: &WebAccess) -> Result<String, String> {
    let url = format!(
        "{SPCLIENT}/melody/v1/license_url?keysystem=com.widevine.alpha&mediatype=audio&sdk_name=harmony&sdk_version={WEB_PLAYER_SDK_VERSION}"
    );
    let response = request_with_meta_headers_and_credentials_as::<Value>(
        "GET",
        &url,
        None,
        None,
        None,
        None,
        &[
            ("accept", "*/*".to_owned()),
            ("app-platform", "WebPlayer".to_owned()),
            ("origin", OPEN_SPOTIFY.to_owned()),
            ("referer", "https://open.spotify.com/".to_owned()),
            ("user-agent", BROWSER_AGENT.to_owned()),
        ],
        &[
            (&access.media_access_handle, "authorization", "Bearer "),
            (&access.media_client_token_handle, "client-token", ""),
        ],
    )?;
    let uri = response
        .body
        .get("uri")
        .and_then(Value::as_str)
        .unwrap_or_default();
    normalize_spotify_license_url(uri)
}

fn normalize_spotify_license_url(uri: &str) -> Result<String, String> {
    let uri = uri.trim();
    let absolute_prefixes = [format!("{WEBGATE_SPCLIENT}/"), format!("{SPCLIENT}/")];
    let (origin, path) = absolute_prefixes
        .iter()
        .find_map(|prefix| {
            uri.strip_prefix(prefix)
                .map(|path| (&prefix[..prefix.len() - 1], path))
        })
        // Spotify resolves relative license URIs against @webgate, whose
        // credentials are bound separately from the media origin.
        .unwrap_or((WEBGATE_SPCLIENT, uri.trim_start_matches('/')));
    if path.starts_with("widevine-license/")
        && path
            .bytes()
            .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
    {
        Ok(format!("{origin}/{path}"))
    } else {
        Err(error(
            "malformed-response",
            "Spotify returned an unsupported license endpoint",
            false,
        ))
    }
}

fn web_player_playback_source(
    access: &WebAccess,
    id: &str,
    quality: &str,
    file_id: &str,
    bitrate: u64,
    duration_ms: u64,
    license_url: &str,
) -> Value {
    let manifest_url = format!("{SPCLIENT}/sneaktables/v2/hls/1/{file_id}/audio.m3u8");
    let certificate_url = format!("{SPCLIENT}/widevine-license/v1/application-certificate");
    let protocol_headers = json!({
        "accept": "application/json",
        "app-platform": "WebPlayer",
        "referer": "https://open.spotify.com/",
        "user-agent": BROWSER_AGENT,
    });
    let credential_headers = json!([
        {
            "handle": access.media_access_handle,
            "name": "authorization",
            "prefix": "Bearer ",
        },
        {
            "handle": access.media_client_token_handle,
            "name": "client-token",
            "prefix": "",
        },
    ]);
    let manifest_request = json!({
        "method": "GET",
        "url": manifest_url,
        "headers": protocol_headers,
        "credentialHeaders": credential_headers,
    });
    let (license_access_handle, license_client_token_handle) =
        if license_url.starts_with(&format!("{SPCLIENT}/")) {
            (
                &access.media_access_handle,
                &access.media_client_token_handle,
            )
        } else {
            (
                &access.license_access_handle,
                &access.license_client_token_handle,
            )
        };
    let license_credential_headers = json!([
        {
            "handle": license_access_handle,
            "name": "authorization",
            "prefix": "Bearer ",
        },
        {
            "handle": license_client_token_handle,
            "name": "client-token",
            "prefix": "",
        },
    ]);
    let license_request = json!({
        "method": "POST",
        "url": license_url,
        "headers": {
            "accept": "*/*",
            "origin": OPEN_SPOTIFY,
            "referer": "https://open.spotify.com/",
            "user-agent": BROWSER_AGENT,
        },
        "credentialHeaders": license_credential_headers,
    });
    let certificate_request = json!({
        "method": "GET",
        "url": certificate_url,
        "headers": {
            "app-platform": "WebPlayer",
            "referer": "https://open.spotify.com/",
            "user-agent": BROWSER_AGENT,
        },
        "credentialHeaders": [{
            "handle": access.media_client_token_handle,
            "name": "client-token",
            "prefix": "",
        }],
    });
    let quality_capabilities = ["standard", "high", "lossless", "hi-res", "master"]
        .iter()
        .map(|candidate| {
            json!({
                "quality": candidate,
                "entitlement": "unknown",
                "resource": if *candidate == "standard" { "available" } else { "unknown" },
                "client": if matches!(*candidate, "standard" | "high") { "supported" } else { "unsupported" },
                "playable": *candidate == "standard",
            })
        })
        .collect::<Vec<_>>();
    json!({
        "source": {
            "kind": "protected-hls",
            "browserPlayer": {
                "entry": "assets/spotify-player.html",
                "scripts": ["assets/hls.min.js"],
            },
            "manifestRequest": manifest_request,
            "licenseRequest": license_request,
            "certificateRequest": certificate_request,
        },
        "cacheKey": format!("spotify:{id}:{quality}"),
        "format": "aac",
        "mimeType": "audio/mp4",
        "qualityLabel": "standard",
        "bitrateKbps": (bitrate / 1000) as u32,
        "expiresAtMs": access.expires_ms,
        "timelineEndMs": duration_ms,
        "isPreview": false,
        "selection": {
            "requestedQuality": quality,
            "resolvedQuality": "standard",
            "fallbackReason": if quality == "automatic" || quality == "standard" { Value::Null } else { json!("web-player-standard") },
            "preview": false,
            "qualityCapabilities": quality_capabilities,
        }
    })
}

pub(super) fn account(profile: &str) -> Result<AccountProfile, String> {
    let response = pathfinder_query(
        profile,
        "profileAttributes",
        PROFILE_ATTRIBUTES_HASH,
        json!({}),
    )?;
    account_profile_from_response(&response)
}

pub(super) fn authenticate_with_cookie(
    profile: &str,
    cookie_handle: &str,
) -> Result<AuthenticatedWebSession, String> {
    let web_token = web_access_token(cookie_handle)?;
    let client_token = client_token(profile, &web_token.client_id)?;
    let access_handle =
        crate::yaqmc::provider::credentials::create(API_PARTNER, &web_token.access_token).map_err(
            |_| {
                error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                )
            },
        )?;
    let preview_access_handle =
        match crate::yaqmc::provider::credentials::create(SPOTIFY_API, &web_token.access_token) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
                return Err(error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                ));
            }
        };
    let client_token_handle =
        match crate::yaqmc::provider::credentials::create(API_PARTNER, &client_token.token) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
                return Err(error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                ));
            }
        };
    let media_access_handle =
        match crate::yaqmc::provider::credentials::create(SPCLIENT, &web_token.access_token) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&client_token_handle);
                return Err(error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                ));
            }
        };
    let media_client_token_handle =
        match crate::yaqmc::provider::credentials::create(SPCLIENT, &client_token.token) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&client_token_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&media_access_handle);
                return Err(error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                ));
            }
        };
    let license_access_handle = match crate::yaqmc::provider::credentials::create(
        WEBGATE_SPCLIENT,
        &web_token.access_token,
    ) {
        Ok(handle) => handle,
        Err(_) => {
            let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&client_token_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&media_access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&media_client_token_handle);
            return Err(error(
                "storage-failure",
                "Spotify web credentials could not be saved",
                true,
            ));
        }
    };
    let license_client_token_handle =
        match crate::yaqmc::provider::credentials::create(WEBGATE_SPCLIENT, &client_token.token) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = crate::yaqmc::provider::credentials::delete(&access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&client_token_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&media_access_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&media_client_token_handle);
                let _ = crate::yaqmc::provider::credentials::delete(&license_access_handle);
                return Err(error(
                    "storage-failure",
                    "Spotify web credentials could not be saved",
                    true,
                ));
            }
        };
    let access = WebAccess {
        access_handle: access_handle.clone(),
        client_token_handle: client_token_handle.clone(),
        media_access_handle: media_access_handle.clone(),
        media_client_token_handle: media_client_token_handle.clone(),
        license_access_handle: license_access_handle.clone(),
        license_client_token_handle: license_client_token_handle.clone(),
        expires_ms: web_token.expires_ms,
    };
    let response = pathfinder_request(
        &access,
        "profileAttributes",
        PROFILE_ATTRIBUTES_HASH,
        json!({}),
    );
    let account = match response.and_then(|response| account_profile_from_response(&response)) {
        Ok(account) => account,
        Err(reason) => {
            let _ = crate::yaqmc::provider::credentials::delete(&access.access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&access.client_token_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&access.media_access_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&access.media_client_token_handle);
            let _ = crate::yaqmc::provider::credentials::delete(&access.license_access_handle);
            let _ =
                crate::yaqmc::provider::credentials::delete(&access.license_client_token_handle);
            return Err(reason);
        }
    };
    let request_version_key = credential_key("spotify-web", profile, "token-request-version");
    if let Err(reason) = storage_set(&request_version_key, WEB_TOKEN_REQUEST_VERSION) {
        let _ = crate::yaqmc::provider::credentials::delete(&access.access_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&preview_access_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&access.client_token_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&access.media_access_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&access.media_client_token_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&access.license_access_handle);
        let _ = crate::yaqmc::provider::credentials::delete(&access.license_client_token_handle);
        return Err(reason);
    }
    Ok(AuthenticatedWebSession {
        access_handle: access.access_handle,
        preview_access_handle,
        client_token_handle: access.client_token_handle,
        media_access_handle: access.media_access_handle,
        media_client_token_handle: access.media_client_token_handle,
        license_access_handle: access.license_access_handle,
        license_client_token_handle: access.license_client_token_handle,
        expires_ms: web_token.expires_ms,
        client_token_expires_ms: client_token.expires_ms,
        profile: account,
    })
}

pub(super) fn favorite_songs(profile: &str, payload: &Value) -> Result<String, String> {
    let offset = super::account_cursor(payload)?;
    let page_limit = super::limit(payload);
    let response = pathfinder_query(
        profile,
        "fetchLibraryTracks",
        FETCH_LIBRARY_TRACKS_HASH,
        json!({"offset": offset, "limit": page_limit}),
    )?;
    let raw_items = response
        .pointer("/data/me/library/tracks/items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify liked songs are missing",
                true,
            )
        })?;
    let songs = raw_items
        .iter()
        .filter_map(|item| library_track(item, profile))
        .map(mark_favorite)
        .collect::<Vec<_>>();
    let total = response
        .pointer("/data/me/library/tracks/totalCount")
        .and_then(Value::as_u64);
    let next = total
        .map(|total| offset.saturating_add(raw_items.len() as u64) < total)
        .unwrap_or(raw_items.len() as u64 >= page_limit)
        .then(|| offset.saturating_add(raw_items.len() as u64).to_string());
    Ok(super::account_page(songs, next, total))
}

fn library_track(item: &Value, profile: &str) -> Option<Value> {
    let track = item
        .pointer("/track/data")
        .filter(|track| track.is_object())?;
    let id = item
        .pointer("/track/_uri")
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("spotify:track:"));
    map_track(track, profile, id, true)
}

pub(super) fn account_playlists(profile: &str, payload: &Value) -> Result<String, String> {
    let offset = super::account_cursor(payload)?;
    // Counts require a metadata request per playlist; keep cold pages within the host deadline.
    let page_limit = super::limit(payload).min(5);
    let owner_id = current_user_id(profile)?;
    let response = pathfinder_query(
        profile,
        "libraryV3",
        LIBRARY_V3_HASH,
        json!({
            "filters": [],
            "order": Value::Null,
            "textFilter": "",
            "features": ["LIKED_SONGS"],
            "limit": page_limit,
            "offset": offset,
            "flatten": true,
            "expandedFolders": [],
            "folderUri": Value::Null,
            "includeFoldersWhenFlattening": false
        }),
    )?;
    let library = response.pointer("/data/me/libraryV3").ok_or_else(|| {
        error(
            "malformed-response",
            "Spotify library playlists are missing",
            true,
        )
    })?;
    let raw_items = library
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify library playlists are missing",
                true,
            )
        })?;
    let mut items = Vec::new();
    for item in raw_items
        .iter()
        .filter_map(|item| item.pointer("/item/data"))
    {
        if let Some(mut summary) = graphql_playlist_summary(item, profile, &owner_id) {
            // libraryV3 omits content counts; resolve metadata instead of claiming an empty list.
            if playlist_track_count(item).is_none() {
                summary["trackCount"] = json!(account_playlist_count(profile, item)?);
            }
            items.push(summary);
        }
    }
    let total = library.get("totalCount").and_then(Value::as_u64);
    let next = total
        .map(|total| offset.saturating_add(raw_items.len() as u64) < total)
        .unwrap_or(raw_items.len() as u64 >= page_limit)
        .then(|| offset.saturating_add(raw_items.len() as u64).to_string());
    Ok(super::account_page(items, next, total))
}

#[cfg(test)]
fn find_track_preview(value: &Value, id: &str) -> Option<String> {
    if value.get("uri").and_then(Value::as_str) == Some(format!("spotify:track:{id}").as_str()) {
        if let Some(url) = value
            .pointer("/previews/audioPreviews/items/0/url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://p.scdn.co/"))
        {
            return Some(url.to_owned());
        }
    }
    match value {
        Value::Object(values) => values
            .values()
            .find_map(|value| find_track_preview(value, id)),
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_track_preview(value, id)),
        _ => None,
    }
}

fn web_account_signed_in(profile: &str) -> Result<bool, String> {
    if super::spotify_transport() != super::SpotifyTransport::WebPlayer {
        return Ok(false);
    }
    Ok(credential_handle(&credential_key("spotify-web", profile, "web-cookie"))?.is_some())
}

fn pathfinder_query(
    profile: &str,
    operation: &str,
    hash: &str,
    variables: Value,
) -> Result<Value, String> {
    let cookie_key = credential_key("spotify-web", profile, "web-cookie");
    let authenticated = credential_handle(&cookie_key)?.is_some();
    let (access, temporary) = if authenticated {
        (ensure_web_access(profile)?, false)
    } else {
        (anonymous_access(profile)?, true)
    };
    let mut response = pathfinder_request(&access, operation, hash, variables.clone());
    if temporary {
        clear_web_access(profile);
    } else if response
        .as_ref()
        .is_err_and(|reason| error_code(reason).as_deref() == Some("authentication-expired"))
    {
        clear_web_access(profile);
        let refreshed = ensure_web_access(profile)?;
        response = pathfinder_request(&refreshed, operation, hash, variables);
    }
    response.map_err(|reason| {
        if error_code(&reason).as_deref() == Some("authentication-expired") && authenticated {
            error(
                "authentication-expired",
                "Spotify web login has expired",
                false,
            )
        } else if error_code(&reason).as_deref() == Some("authentication-expired") {
            error(
                "offline",
                "Spotify Web Player rejected its anonymous session",
                true,
            )
        } else {
            reason
        }
    })
}

fn pathfinder_request(
    access: &WebAccess,
    operation: &str,
    hash: &str,
    variables: Value,
) -> Result<Value, String> {
    let body = json!({
        "variables": variables,
        "operationName": operation,
        "extensions": {"persistedQuery": {"version": 1, "sha256Hash": hash}}
    })
    .to_string();
    let client_credentials = [(access.client_token_handle.as_str(), "client-token", "")];
    let response = request_with_meta_headers_and_credentials_as::<Value>(
        "POST",
        PATHFINDER,
        Some(&body),
        Some("application/json;charset=UTF-8"),
        Some((&access.access_handle, "authorization", "Bearer ")),
        None,
        &[
            ("accept", "application/json".to_owned()),
            ("user-agent", BROWSER_AGENT.to_owned()),
            ("app-platform", "WebPlayer".to_owned()),
        ],
        &client_credentials,
    )?;
    let errors = response
        .body
        .get("errors")
        .and_then(Value::as_array)
        .filter(|errors| !errors.is_empty());
    if let Some(errors) = errors {
        if errors.iter().any(graphql_authentication_error) {
            return Err(error(
                "authentication-expired",
                "Spotify web login has expired",
                false,
            ));
        }
        if errors.iter().any(graphql_stale_query_error) {
            return Err(error(
                "schema-changed",
                "Spotify Web Player's persisted query is out of date",
                false,
            ));
        }
        let has_data = response
            .body
            .get("data")
            .and_then(Value::as_object)
            .is_some_and(|data| data.values().any(|value| !value.is_null()));
        if !has_data {
            return Err(error(
                "provider-failure",
                "Spotify Web Player rejected the request",
                false,
            ));
        }
    }
    Ok(response.body)
}

fn graphql_stale_query_error(error: &Value) -> bool {
    ["message", "code"]
        .into_iter()
        .filter_map(|field| error.get(field).and_then(Value::as_str))
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("persistedquerynotfound")
                || value.contains("invalid query hash")
                || value.contains("stale query")
        })
}

fn graphql_authentication_error(error: &Value) -> bool {
    ["message", "code"]
        .into_iter()
        .filter_map(|field| error.get(field).and_then(Value::as_str))
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("unauthor")
                || value.contains("authentication")
                || value.contains("session expired")
                || value.contains("token expired")
                || value.contains("invalid token")
                || value.contains("401")
        })
}

fn ensure_web_access(profile: &str) -> Result<WebAccess, String> {
    let access_key = credential_key("spotify-web", profile, "access-token");
    let preview_key = credential_key("spotify-web", profile, "preview-access-token");
    let client_token_key = credential_key("spotify-web", profile, "client-token");
    let media_access_key = credential_key("spotify-web", profile, "media-access-token");
    let media_client_token_key = credential_key("spotify-web", profile, "media-client-token");
    let license_access_key = credential_key("spotify-web", profile, "license-access-token");
    let license_client_token_key = credential_key("spotify-web", profile, "license-client-token");
    let expiry_key = credential_key("spotify-web", profile, "access-expires");
    let client_token_expiry_key = credential_key("spotify-web", profile, "client-token-expires");
    let token_request_version_key = credential_key("spotify-web", profile, "token-request-version");
    let existing = credential_handle(&access_key)?;
    let existing_preview = credential_handle(&preview_key)?;
    let existing_client_token = credential_handle(&client_token_key)?;
    let existing_media_access = credential_handle(&media_access_key)?;
    let existing_media_client_token = credential_handle(&media_client_token_key)?;
    let existing_license_access = credential_handle(&license_access_key)?;
    let existing_license_client_token = credential_handle(&license_client_token_key)?;
    let expires = storage_get(&expiry_key)?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let client_token_expires = storage_get(&client_token_expiry_key)?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if let (
        Some(access),
        Some(_),
        Some(client_token),
        Some(media_access),
        Some(media_client_token),
        Some(license_access),
        Some(license_client_token),
    ) = (
        existing,
        existing_preview,
        existing_client_token,
        existing_media_access,
        existing_media_client_token,
        existing_license_access,
        existing_license_client_token,
    ) {
        if storage_get(&token_request_version_key)?.as_deref() == Some(WEB_TOKEN_REQUEST_VERSION)
            && expires > now_ms().saturating_add(30_000)
            && client_token_expires > now_ms().saturating_add(30_000)
        {
            return Ok(WebAccess {
                access_handle: access,
                client_token_handle: client_token,
                media_access_handle: media_access,
                media_client_token_handle: media_client_token,
                license_access_handle: license_access,
                license_client_token_handle: license_client_token,
                expires_ms: expires,
            });
        }
    }
    let cookie_key = credential_key("spotify-web", profile, "web-cookie");
    let cookie = credential_handle(&cookie_key)?.ok_or_else(|| {
        error(
            "authentication-expired",
            "Spotify web login is required",
            false,
        )
    })?;
    let web_token = web_access_token(&cookie)?;
    let client_token = client_token(profile, &web_token.client_id)?;
    let access_handle = replace_credential(&access_key, API_PARTNER, &web_token.access_token)?;
    if let Err(reason) = replace_credential(&preview_key, SPOTIFY_API, &web_token.access_token) {
        clear_web_access(profile);
        return Err(reason);
    }
    let client_token_handle =
        match replace_credential(&client_token_key, API_PARTNER, &client_token.token) {
            Ok(handle) => handle,
            Err(reason) => {
                clear_web_access(profile);
                return Err(reason);
            }
        };
    let media_access_handle =
        match replace_credential(&media_access_key, SPCLIENT, &web_token.access_token) {
            Ok(handle) => handle,
            Err(reason) => {
                clear_web_access(profile);
                return Err(reason);
            }
        };
    let media_client_token_handle =
        match replace_credential(&media_client_token_key, SPCLIENT, &client_token.token) {
            Ok(handle) => handle,
            Err(reason) => {
                clear_web_access(profile);
                return Err(reason);
            }
        };
    let license_access_handle = match replace_credential(
        &license_access_key,
        WEBGATE_SPCLIENT,
        &web_token.access_token,
    ) {
        Ok(handle) => handle,
        Err(reason) => {
            clear_web_access(profile);
            return Err(reason);
        }
    };
    let license_client_token_handle = match replace_credential(
        &license_client_token_key,
        WEBGATE_SPCLIENT,
        &client_token.token,
    ) {
        Ok(handle) => handle,
        Err(reason) => {
            clear_web_access(profile);
            return Err(reason);
        }
    };
    if let Err(reason) = storage_set(&expiry_key, &web_token.expires_ms.to_string()) {
        clear_web_access(profile);
        return Err(reason);
    }
    if let Err(reason) = storage_set(
        &client_token_expiry_key,
        &client_token.expires_ms.to_string(),
    ) {
        clear_web_access(profile);
        return Err(reason);
    }
    if let Err(reason) = storage_set(&token_request_version_key, WEB_TOKEN_REQUEST_VERSION) {
        clear_web_access(profile);
        return Err(reason);
    }
    Ok(WebAccess {
        access_handle,
        client_token_handle,
        media_access_handle,
        media_client_token_handle,
        license_access_handle,
        license_client_token_handle,
        expires_ms: web_token.expires_ms,
    })
}

fn anonymous_access(profile: &str) -> Result<WebAccess, String> {
    let web_token = anonymous_token()?;
    let client_token = client_token(profile, &web_token.client_id)?;
    let access_key = credential_key("spotify-web", profile, "access-token");
    let client_token_key = credential_key("spotify-web", profile, "client-token");
    let media_access_key = credential_key("spotify-web", profile, "media-access-token");
    let media_client_token_key = credential_key("spotify-web", profile, "media-client-token");
    let license_access_key = credential_key("spotify-web", profile, "license-access-token");
    let license_client_token_key = credential_key("spotify-web", profile, "license-client-token");
    let access_handle = replace_credential(&access_key, API_PARTNER, &web_token.access_token)?;
    let client_token_handle =
        match replace_credential(&client_token_key, API_PARTNER, &client_token.token) {
            Ok(handle) => handle,
            Err(reason) => {
                let _ = clear_credential(&access_key);
                return Err(reason);
            }
        };
    let media_access_handle =
        replace_credential(&media_access_key, SPCLIENT, &web_token.access_token)?;
    let media_client_token_handle =
        replace_credential(&media_client_token_key, SPCLIENT, &client_token.token)?;
    let license_access_handle = replace_credential(
        &license_access_key,
        WEBGATE_SPCLIENT,
        &web_token.access_token,
    )?;
    let license_client_token_handle = replace_credential(
        &license_client_token_key,
        WEBGATE_SPCLIENT,
        &client_token.token,
    )?;
    Ok(WebAccess {
        access_handle,
        client_token_handle,
        media_access_handle,
        media_client_token_handle,
        license_access_handle,
        license_client_token_handle,
        expires_ms: web_token.expires_ms,
    })
}

fn clear_web_access(profile: &str) {
    for kind in [
        "access-token",
        "preview-access-token",
        "client-token",
        "media-access-token",
        "media-client-token",
        "license-access-token",
        "license-client-token",
        "access-expires",
        "client-token-expires",
        "token-request-version",
    ] {
        let key = credential_key("spotify-web", profile, kind);
        let _ = clear_credential(&key);
    }
}

fn web_access_token(cookie_handle: &str) -> Result<WebToken, String> {
    let now = now_ms();
    let server = server_time_ms().unwrap_or(now);
    let mut last_failure = None;
    for (masked, version) in TOTP_SECRETS {
        let key = unmask_secret(masked);
        let query = token_query(&key, version, now / 1000, server / 1000);
        let url = format!("{OPEN_SPOTIFY}/api/token?{query}");
        match request_with_meta_headers_as::<Value>(
            "GET",
            &url,
            None,
            None,
            Some((cookie_handle, "cookie", "")),
            None,
            &[
                ("accept", "application/json".to_owned()),
                ("user-agent", BROWSER_AGENT.to_owned()),
                ("referer", format!("{OPEN_SPOTIFY}/")),
            ],
        ) {
            Ok(response)
                if response.body.get("isAnonymous").and_then(Value::as_bool) == Some(false) =>
            {
                match parse_web_token(&response.body) {
                    Ok(token) => return Ok(token),
                    Err(reason) => return Err(reason),
                }
            }
            Ok(_) => {
                return Err(error(
                    "authentication-expired",
                    "Spotify web cookies are signed out",
                    false,
                ))
            }
            Err(reason) if error_code(&reason).as_deref() == Some("authentication-expired") => {
                return Err(error(
                    "authentication-expired",
                    "Spotify web session has expired",
                    false,
                ))
            }
            Err(reason) => {
                if !token_request_can_retry(&reason) {
                    return Err(reason);
                }
                last_failure = Some(reason);
            }
        }
    }
    Err(last_failure
        .unwrap_or_else(|| error("offline", "Spotify web account access is unavailable", true)))
}

fn parse_web_token(response: &Value) -> Result<WebToken, String> {
    parse_web_token_at(response, now_ms())
}

fn parse_web_token_at(response: &Value, now: u64) -> Result<WebToken, String> {
    let access_token = response
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify returned an incomplete account token",
                true,
            )
        })?;
    let client_id = response
        .get("clientId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify returned no Web Player client ID",
                true,
            )
        })?;
    let expires_ms = response
        .get("accessTokenExpirationTimestampMs")
        .and_then(Value::as_u64)
        .filter(|expires| *expires > now)
        .unwrap_or_else(|| now.saturating_add(30 * 60 * 1000));
    Ok(WebToken {
        access_token: access_token.to_owned(),
        client_id: client_id.to_owned(),
        expires_ms,
    })
}

fn client_token(profile: &str, client_id: &str) -> Result<ClientToken, String> {
    if client_id.is_empty() {
        return Err(error(
            "authentication-expired",
            "Spotify Web Player did not identify its client",
            false,
        ));
    }
    let device_id = client_device_id(profile)?;
    let body = json!({
        "client_data": {
            "client_version": CLIENT_VERSION,
            "client_id": client_id,
            "js_sdk_data": {
                "device_brand": "unknown",
                "device_model": "unknown",
                "os": "windows",
                "os_version": "10.0",
                "container_version": "unknown",
                "device_id": device_id,
                "device_type": "computer",
                "platform_identifier": "web_player"
            }
        }
    })
    .to_string();
    let response = request_with_meta_headers_as::<Value>(
        "POST",
        &format!("{CLIENT_TOKEN}/v1/clienttoken"),
        Some(&body),
        Some("application/json"),
        None,
        None,
        &[
            ("accept", "application/json".to_owned()),
            ("user-agent", BROWSER_AGENT.to_owned()),
        ],
    )?;
    parse_client_token(&response.body)
}

fn parse_client_token(response: &Value) -> Result<ClientToken, String> {
    parse_client_token_at(response, now_ms())
}

fn parse_client_token_at(response: &Value, now: u64) -> Result<ClientToken, String> {
    let token = response
        .pointer("/granted_token/token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify returned no client token",
                true,
            )
        })?;
    let expires_ms = response
        .pointer("/granted_token/expires_after_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(14 * 24 * 60 * 60)
        .saturating_mul(1000)
        .saturating_add(now);
    Ok(ClientToken {
        token: token.to_owned(),
        expires_ms,
    })
}

fn client_device_id(profile: &str) -> Result<String, String> {
    let key = credential_key("spotify-web", profile, "client-device-id");
    if let Some(value) = storage_get(&key)?
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Ok(value);
    }
    let mut value = String::with_capacity(32);
    for byte in random_bytes(16)? {
        value.push(super::hex(byte >> 4));
        value.push(super::hex(byte & 0x0f));
    }
    storage_set(&key, &value)?;
    Ok(value)
}

fn account_profile_from_response(response: &Value) -> Result<AccountProfile, String> {
    let profile = response.pointer("/data/me/profile").ok_or_else(|| {
        error(
            "authentication-expired",
            "Spotify web cookies are signed out",
            false,
        )
    })?;
    let username = profile
        .get("username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error(
                "authentication-expired",
                "Spotify web cookies are signed out",
                false,
            )
        })?;
    let nickname = profile
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(username);
    Ok(AccountProfile {
        username: username.to_owned(),
        nickname: nickname.to_owned(),
        avatar_url: best_image(profile.get("avatar").unwrap_or(&Value::Null)),
    })
}

fn current_user_id(profile: &str) -> Result<String, String> {
    let key = credential_key("spotify-web", profile, "user-id");
    if let Some(username) = storage_get(&key)?.filter(|value| !value.is_empty()) {
        return Ok(username);
    }
    let account = account(profile)?;
    storage_set(&key, &account.username)?;
    Ok(account.username)
}

fn graphql_playlist_summary(item: &Value, profile: &str, owner_id: &str) -> Option<Value> {
    if item.get("__typename").and_then(Value::as_str) != Some("Playlist") {
        return None;
    }
    let id = item
        .get("uri")
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("spotify:playlist:"))?
        .to_owned();
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled playlist")
        .to_owned();
    let item_owner_id = item
        .pointer("/ownerV2/data/username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(owner_id)
        .to_owned();
    let item_owner_name = item
        .pointer("/ownerV2/data/name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("Spotify")
        .to_owned();
    let artwork_value = artwork_from_value(item)
        .and_then(|artwork| artwork.get("src").cloned())
        .unwrap_or_else(|| json!(""));
    Some(playlist_summary(
        "spotify",
        profile,
        id,
        title,
        item.get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        item_owner_id.clone(),
        item_owner_name,
        Some(&artwork_value),
        if item_owner_id == owner_id {
            "owned"
        } else {
            "collected"
        },
        playlist_track_count(item).unwrap_or(0),
    ))
}

fn playlist_track_count(item: &Value) -> Option<u64> {
    item.pointer("/content/totalCount")
        .or_else(|| item.get("count"))
        .and_then(Value::as_u64)
}

fn account_playlist_count(profile: &str, item: &Value) -> Result<u64, String> {
    let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
    let revision = item.get("revisionId").and_then(Value::as_str);
    let key = credential_key("spotify-web", profile, &format!("playlist-count:{uri}"));
    if let Some(raw) = storage_get(&key)? {
        if let Ok(cached) = serde_json::from_str::<Value>(&raw) {
            if revision.is_some() && cached.get("revision").and_then(Value::as_str) == revision {
                if let Some(count) = cached.get("count").and_then(Value::as_u64) {
                    return Ok(count);
                }
            }
        }
    }
    let response = pathfinder_query(
        profile,
        "fetchPlaylist",
        FETCH_PLAYLIST_HASH,
        json!({"uri":uri,"offset":0,"limit":1,"enableWatchFeedEntrypoint":false}),
    )?;
    let count = response
        .pointer("/data/playlistV2")
        .and_then(playlist_track_count)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify playlist track count is missing",
                false,
            )
        })?;
    storage_set(
        &key,
        &json!({"revision":revision,"count":count}).to_string(),
    )?;
    Ok(count)
}

fn best_image(image: &Value) -> Option<String> {
    image
        .get("sources")?
        .as_array()?
        .iter()
        .max_by_key(|source| {
            source
                .get("width")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_mul(source.get("height").and_then(Value::as_u64).unwrap_or(0))
        })?
        .get("url")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn anonymous_token() -> Result<WebToken, String> {
    let now = now_ms();
    let server = server_time_ms().unwrap_or(now);
    let mut last_failure = None;
    for (masked, version) in TOTP_SECRETS {
        let key = unmask_secret(masked);
        let query = token_query(&key, version, now / 1000, server / 1000);
        let url = format!("{OPEN_SPOTIFY}/api/token?{query}");
        match request_with_meta_headers_as::<Value>(
            "GET",
            &url,
            None,
            None,
            None,
            None,
            &[
                ("accept", "application/json".to_owned()),
                ("user-agent", BROWSER_AGENT.to_owned()),
            ],
        ) {
            Ok(response)
                if response.body.get("isAnonymous").and_then(Value::as_bool) == Some(true) =>
            {
                match parse_web_token(&response.body) {
                    Ok(token) => return Ok(token),
                    Err(reason) => return Err(reason),
                }
            }
            Ok(_) => {
                return Err(error(
                    "malformed-response",
                    "Spotify returned a non-anonymous session",
                    false,
                ));
            }
            Err(reason) => {
                if !token_request_can_retry(&reason) {
                    return Err(reason);
                }
                last_failure = Some(reason);
            }
        }
    }
    Err(last_failure
        .unwrap_or_else(|| error("offline", "Spotify anonymous access is unavailable", true)))
}

fn token_request_can_retry(reason: &str) -> bool {
    serde_json::from_str::<Value>(reason)
        .ok()
        .and_then(|failure| failure.get("retryable").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn server_time_ms() -> Option<u64> {
    let response = request_with_meta_headers_as::<Value>(
        "GET",
        &format!("{OPEN_SPOTIFY}/api/server-time"),
        None,
        None,
        None,
        None,
        &[
            ("accept", "application/json".to_owned()),
            ("user-agent", BROWSER_AGENT.to_owned()),
        ],
    )
    .ok()?;
    response
        .body
        .get("serverTime")
        .and_then(Value::as_u64)
        .map(|seconds| seconds.saturating_mul(1000))
}

fn error_code(reason: &str) -> Option<String> {
    let value: Value = serde_json::from_str(reason).ok()?;
    value
        .get("code")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn unmask_secret(masked: &str) -> Vec<u8> {
    let mut key = String::new();
    for (index, byte) in masked.bytes().enumerate() {
        let value = byte ^ ((index % 33) as u8 + 9);
        key.push_str(&value.to_string());
    }
    key.into_bytes()
}

fn hotp(key: &[u8], counter: u64) -> String {
    let mut mac = <Hmac<Sha1>>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let value = (u32::from_be_bytes([
        digest[offset],
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]) & 0x7fff_ffff)
        % 1_000_000;
    format!("{value:06}")
}

fn totp(key: &[u8], milliseconds: u64) -> String {
    hotp(key, milliseconds / 1000 / 30)
}

fn token_query(key: &[u8], version: u32, now_seconds: u64, server_seconds: u64) -> String {
    format!(
        "reason=init&productType=web-player&totp={}&totpServer={}&totpVer={version}",
        totp(key, now_seconds.saturating_mul(1000)),
        totp(key, server_seconds.saturating_mul(1000))
    )
}

fn map_track(
    item: &Value,
    profile: &str,
    expected_id: Option<&str>,
    playable: bool,
) -> Option<Value> {
    if item.get("__typename").and_then(Value::as_str) == Some("Episode") {
        return None;
    }
    let uri = item
        .get("uri")
        .or_else(|| item.get("_uri"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = expected_id
        .map(ToOwned::to_owned)
        .or_else(|| uri.strip_prefix("spotify:track:").map(ToOwned::to_owned))
        .or_else(|| {
            item.get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })?;
    if !uri.is_empty() && !uri.starts_with("spotify:track:") {
        return None;
    }
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
    let artists = track_artists(item);
    let album = item.get("albumOfTrack").cloned().unwrap_or(Value::Null);
    let artwork = best_cover(&album)
        .map(|source| source.get("url").cloned().unwrap_or(json!("")))
        .unwrap_or(json!(""));
    let duration = item
        .pointer("/duration/totalMilliseconds")
        .and_then(Value::as_u64)
        .or_else(|| {
            item.pointer("/trackDuration/totalMilliseconds")
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    let mut song = song_value(
        "spotify",
        profile,
        &id,
        title,
        artists,
        album
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown album"),
        artwork,
        duration,
        Value::Null,
        false,
    );
    if playable {
        song["availability"] = json!({"status":"available"});
        if let Some(object) = song.as_object_mut() {
            object.remove("playbackCapability");
        }
    }
    Some(song)
}

fn track_artists(item: &Value) -> Vec<Value> {
    let mut artists = Vec::new();
    for path in [
        "/artists/items",
        "/firstArtist/items",
        "/otherArtists/items",
    ] {
        let Some(values) = item.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        for artist in values {
            let id = artist
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|uri| uri.strip_prefix("spotify:artist:"))
                .or_else(|| artist.get("id").and_then(Value::as_str))
                .unwrap_or("unknown-artist");
            artists.push(json!({
                "id": id,
                "name": artist.pointer("/profile/name").and_then(Value::as_str).unwrap_or("Unknown")
            }));
        }
    }
    artists
}

fn best_cover(album: &Value) -> Option<&Value> {
    album
        .pointer("/coverArt/sources")
        .and_then(Value::as_array)?
        .iter()
        .max_by_key(|source| {
            source
                .get("width")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_mul(source.get("height").and_then(Value::as_u64).unwrap_or(0))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liked_song_identity_comes_from_the_library_wrapper() {
        let item = json!({"track":{"_uri":"spotify:track:liked123", "data":{
            "__typename":"Track","name":"Saved song","duration":{"totalMilliseconds":123000}
        }}});
        let song = library_track(&item, "default").unwrap();
        assert_eq!(song["id"], "liked123");
        assert_eq!(song["provider"]["trackId"], "liked123");
        assert_eq!(song["durationMs"], 123000);
        assert!(library_track(
            &json!({"track":{"_uri":"spotify:track:deleted","data":null}}),
            "default"
        )
        .is_none());
    }

    #[test]
    fn custom_and_mosaic_playlist_covers_remain_usable_urls() {
        let custom = json!({"name":"Custom", "images":{"items":[{"sources":[{
            "url":"https://image-cdn-fa.spotifycdn.com/image/custom","width":null,"height":null
        }]}]}});
        assert_eq!(
            artwork_from_value(&custom).unwrap()["src"],
            "https://image-cdn-fa.spotifycdn.com/image/custom"
        );
        let mosaic = json!({"images":{"items":[{"sources":[{"url":"spotify:mosaic:aa:bb:cc:dd","width":640}]}]}});
        assert_eq!(
            artwork_from_value(&mosaic).unwrap()["src"],
            "https://mosaic.scdn.co/640/aabbccdd"
        );
        assert_eq!(
            playlist_track_count(&json!({"content":{"totalCount":42}})),
            Some(42)
        );
        assert_eq!(playlist_track_count(&json!({"name":"unknown count"})), None);
        assert_eq!(
            playlist_track_count(&json!({"content":{"totalCount":0}})),
            Some(0)
        );
    }

    #[test]
    fn web_preview_must_match_the_requested_recording_and_audio_origin() {
        let response = json!({"data":{"trackUnion":{"firstArtist":{"items":[{"discography":{"topTracks":{"items":[
            {"track":{"uri":"spotify:track:other", "previews":{"audioPreviews":{"items":[{"url":"https://p.scdn.co/mp3-preview/other"}]}}}},
            {"track":{"uri":"spotify:track:wanted", "previews":{"audioPreviews":{"items":[{"url":"https://p.scdn.co/mp3-preview/wanted"}]}}}}
        ]}}}]}}}});
        assert_eq!(
            find_track_preview(&response, "wanted").as_deref(),
            Some("https://p.scdn.co/mp3-preview/wanted")
        );
        assert!(find_track_preview(&response, "missing").is_none());
        assert!(find_track_preview(&json!({"uri":"spotify:track:wanted", "previews":{"audioPreviews":{"items":[{"url":"https://example.com/audio"}]}}}),"wanted").is_none());
    }

    #[test]
    fn album_rejects_an_empty_id_before_requesting_metadata() {
        let reason = album("default", &json!({})).unwrap_err();
        assert!(reason.contains("invalid-request"));
        assert!(reason.contains("album ID is empty"));
    }

    #[test]
    fn home_cards_keep_navigable_playlist_and_album_ids() {
        let playlist = playlist_card(
            &json!({"uri":"spotify:playlist:list123","name":"Mix","content":{"totalCount":10}}),
        )
        .unwrap();
        assert_eq!(playlist["id"], "list123");
        assert!(playlist["tracks"].as_array().unwrap().is_empty());
        let album = album_card(
            &json!({"uri":"spotify:album:album123","name":"Album","date":{"isoString":"2025-03-01"},
            "artists":{"items":[{"uri":"spotify:artist:artist123","profile":{"name":"Artist"}}]}}),
        )
        .unwrap();
        assert_eq!(album["id"], "album123");
        assert_eq!(album["artist"]["id"], "artist123");
        assert_eq!(album["releaseYear"], 2025);
        assert!(album_card(&json!({"uri":"spotify:playlist:wrong"})).is_none());
    }

    #[test]
    fn token_requests_preserve_terminal_host_and_protocol_failures() {
        for code in [
            "client-unsupported",
            "provider-failure",
            "malformed-response",
            "authorization-rejected",
        ] {
            let failure = error(code, "synthetic rejection", false);
            assert!(!token_request_can_retry(&failure), "{code}");
        }
        assert!(token_request_can_retry(&error(
            "offline",
            "connection failed",
            true
        )));
        assert!(!token_request_can_retry("unstructured failure"));
    }

    #[test]
    fn secret_unmasks_to_the_current_digit_key() {
        let key = unmask_secret(TOTP_SECRETS[0].0);
        assert_eq!(
            core::str::from_utf8(&key).unwrap(),
            "376136387538459893883312310911992847112448894410210511297108"
        );
    }

    #[test]
    fn hotp_matches_rfc_4226_vectors() {
        let key = b"12345678901234567890";
        for (counter, expected) in [
            (0, "755224"),
            (1, "287082"),
            (2, "359152"),
            (3, "969429"),
            (4, "338314"),
        ] {
            assert_eq!(hotp(key, counter), expected);
        }
    }

    #[test]
    fn token_query_contains_both_codes_and_version() {
        assert_eq!(
            token_query(b"12345678901234567890", 61, 30, 33),
            "reason=init&productType=web-player&totp=287082&totpServer=287082&totpVer=61"
        );
    }

    #[test]
    fn web_player_playlist_preview_keeps_identity_owner_artwork_and_count() {
        let preview = playlist_preview(&json!({
            "uri": "spotify:playlist:playlist123",
            "name": "Road Trip",
            "ownerV2": {"data": {"name": "Spotify"}},
            "images": {"items": [{"sources": [
                {"url": "https://mosaic.scdn.co/cover", "width": 640, "height": 640}
            ]}]},
            "content": {"totalCount": 31}
        }))
        .unwrap();
        assert_eq!(preview["id"], "playlist123");
        assert_eq!(preview["title"], "Road Trip");
        assert_eq!(preview["creator"], "Spotify");
        assert_eq!(preview["artwork"]["src"], "https://mosaic.scdn.co/cover");
        assert_eq!(preview["trackCount"], 31);
        assert!(playlist_preview(&json!({"name": "missing URI"})).is_none());
    }

    #[test]
    fn web_player_track_maps_to_the_provider_shape() {
        let mapped = map_track(
            &json!({
                "__typename": "Track",
                "uri": "spotify:track:track123",
                "name": "Song",
                "duration": {"totalMilliseconds": 123000},
                "artists": {"items": [{
                    "uri": "spotify:artist:artist123",
                    "profile": {"name": "Artist"}
                }]},
                "albumOfTrack": {
                    "name": "Album",
                    "coverArt": {"sources": [
                        {"url": "https://i.scdn.co/image/small", "width": 64, "height": 64},
                        {"url": "https://i.scdn.co/image/large", "width": 640, "height": 640}
                    ]}
                }
            }),
            "work",
            None,
            false,
        )
        .unwrap();
        assert_eq!(mapped["id"], "track123");
        assert_eq!(mapped["title"], "Song");
        assert_eq!(mapped["durationMs"], 123000);
        assert_eq!(mapped["artists"][0]["name"], "Artist");
        assert_eq!(mapped["artwork"]["src"], "https://i.scdn.co/image/large");
        assert_eq!(mapped["availability"]["status"], "unavailable");
    }

    #[test]
    fn library_track_uri_fallback_preserves_favorite_song_ids() {
        let mapped = map_track(
            &json!({
                "__typename": "Track",
                "_uri": "spotify:track:track456",
                "name": "Saved Song",
                "duration": {"totalMilliseconds": 90000}
            }),
            "work",
            None,
            true,
        )
        .unwrap();
        assert_eq!(mapped["id"], "track456");
        assert_eq!(mapped["availability"]["status"], "available");
        assert!(mapped.get("playbackCapability").is_none());
    }

    #[test]
    fn graphql_errors_distinguish_authentication_from_partial_data() {
        assert!(graphql_authentication_error(
            &json!({"message": "The access token expired"})
        ));
        assert!(graphql_authentication_error(
            &json!({"message": "401 Unauthorized"})
        ));
        assert!(!graphql_authentication_error(
            &json!({"message": "PersistedQueryNotFound"})
        ));
        assert!(graphql_stale_query_error(&json!({
            "message": "PersistedQueryNotFound"
        })));
        assert!(graphql_stale_query_error(&json!({
            "message": "412 Invalid query hash"
        })));
        assert!(!graphql_stale_query_error(&json!({
            "message": "User is not authorized"
        })));
    }

    #[test]
    fn web_token_parser_requires_the_client_id_needed_for_pathfinder() {
        let now = 1_700_000_000_000;
        let parsed = parse_web_token_at(
            &json!({
                "accessToken": "account-token",
                "clientId": "web-client",
                "accessTokenExpirationTimestampMs": now + 60_000,
                "isAnonymous": false
            }),
            now,
        )
        .unwrap();
        assert_eq!(parsed.access_token, "account-token");
        assert_eq!(parsed.client_id, "web-client");
        assert!(parse_web_token_at(
            &json!({
                "accessToken": "account-token",
                "accessTokenExpirationTimestampMs": now + 60_000
            }),
            now
        )
        .is_err());
    }

    #[test]
    fn relative_license_urls_use_the_webgate_credential_origin() {
        for uri in [
            "widevine-license/v1/audio/license",
            "/widevine-license/v1/audio/license",
            "widevine-license/v1/audio/license?token=synthetic",
        ] {
            assert_eq!(
                normalize_spotify_license_url(uri).unwrap(),
                format!("{WEBGATE_SPCLIENT}/{}", uri.trim_start_matches('/'))
            );
        }
    }

    #[test]
    fn web_player_recipe_exchanges_the_spotify_widevine_application_certificate() {
        let access = WebAccess {
            access_handle: "api-token".to_owned(),
            client_token_handle: "api-client-token".to_owned(),
            media_access_handle: "media-token".to_owned(),
            media_client_token_handle: "media-client-token".to_owned(),
            license_access_handle: "license-token".to_owned(),
            license_client_token_handle: "license-client-token".to_owned(),
            expires_ms: 1_700_000_000_000,
        };
        let license_url = normalize_spotify_license_url(
            "https://guc3-spclient.spotify.com/widevine-license/v1/audio/license?token=x",
        )
        .unwrap();
        assert_eq!(
            license_url,
            format!("{WEBGATE_SPCLIENT}/widevine-license/v1/audio/license?token=x")
        );
        for invalid in [
            "",
            "https://example.invalid/widevine-license/v1/audio/license",
            "not-a-license-endpoint",
        ] {
            assert!(normalize_spotify_license_url(invalid).is_err());
        }
        let source = web_player_playback_source(
            &access,
            "track123",
            "automatic",
            "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
            128_000,
            181_000,
            &license_url,
        );
        assert_eq!(source["source"]["kind"], "protected-hls");
        assert_eq!(
            source["source"]["browserPlayer"]["entry"],
            "assets/spotify-player.html"
        );
        assert_eq!(
            source["source"]["browserPlayer"]["scripts"],
            json!(["assets/hls.min.js"])
        );
        assert_eq!(source["source"]["licenseRequest"]["url"], license_url);
        assert_eq!(source["source"]["licenseRequest"]["method"], "POST");
        assert_eq!(
            source["source"]["licenseRequest"]["headers"]["origin"],
            OPEN_SPOTIFY
        );
        assert_eq!(
            source["source"]["licenseRequest"]["headers"]["accept"],
            "*/*"
        );
        assert!(source["source"]["licenseRequest"]["headers"]
            .get("app-platform")
            .is_none());
        assert!(source["source"]["licenseRequest"]["headers"]
            .get("content-type")
            .is_none());
        assert_eq!(
            source["source"]["licenseRequest"]["credentialHeaders"][0]["handle"],
            "license-token"
        );
        assert_eq!(
            source["source"]["licenseRequest"]["credentialHeaders"][1]["handle"],
            "license-client-token"
        );
        let media_origin_license = web_player_playback_source(
            &access,
            "track123",
            "automatic",
            "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
            128_000,
            181_000,
            &normalize_spotify_license_url(&format!(
                "{SPCLIENT}/widevine-license/v1/audio/license"
            ))
            .unwrap(),
        );
        assert_eq!(
            media_origin_license["source"]["licenseRequest"]["credentialHeaders"][0]["handle"],
            "media-token"
        );
        assert_eq!(
            media_origin_license["source"]["licenseRequest"]["credentialHeaders"][1]["handle"],
            "media-client-token"
        );
        let certificate = &source["source"]["certificateRequest"];
        assert_eq!(certificate["method"], "GET");
        assert!(certificate["url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/widevine-license/v1/application-certificate")));
        assert_eq!(
            certificate["credentialHeaders"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            certificate["credentialHeaders"][0]["handle"],
            "media-client-token"
        );
        assert_eq!(certificate["credentialHeaders"][0]["name"], "client-token");
        assert_eq!(source["isPreview"], false);
        assert_eq!(source["timelineEndMs"], 181_000);
    }

    #[test]
    fn client_token_parser_keeps_the_granted_token_and_expiry() {
        let now = 1_700_000_000_000;
        let parsed = parse_client_token_at(
            &json!({
                "granted_token": {
                    "token": "device-token",
                    "expires_after_seconds": 120
                }
            }),
            now,
        )
        .unwrap();
        assert_eq!(parsed.token, "device-token");
        assert_eq!(parsed.expires_ms, now + 120_000);
        assert!(parse_client_token_at(&json!({"granted_token": {}}), now).is_err());
    }

    #[test]
    fn signed_in_profile_requires_a_username_and_chooses_the_largest_avatar() {
        let profile = account_profile_from_response(&json!({
            "data": {"me": {"profile": {
                "name": "Listener",
                "username": "listener_1",
                "avatar": {"sources": [
                    {"url": "https://i.scdn.co/small", "width": 64, "height": 64},
                    {"url": "https://i.scdn.co/large", "width": 300, "height": 300}
                ]}
            }}}
        }))
        .unwrap();
        assert_eq!(profile.username, "listener_1");
        assert_eq!(profile.nickname, "Listener");
        assert_eq!(
            profile.avatar_url.as_deref(),
            Some("https://i.scdn.co/large")
        );
        assert!(account_profile_from_response(&json!({
            "data": {"me": {"profile": {"name": "Visitor", "username": ""}}}
        }))
        .is_err());
    }

    #[test]
    fn account_library_playlist_maps_to_the_frozen_account_summary() {
        let summary = graphql_playlist_summary(
            &json!({
                "__typename": "Playlist",
                "uri": "spotify:playlist:playlist123",
                "name": "Saved Mix",
                "description": "Songs",
                "ownerV2": {"data": {"username": "someone_else", "name": "Owner"}},
                "images": {"items": [{"sources": [
                    {"url": "https://i.scdn.co/cover", "width": 640, "height": 640}
                ]}]},
                "content": {"totalCount": 12}
            }),
            "work",
            "listener_1",
        )
        .unwrap();
        assert_eq!(summary["id"], "playlist123");
        assert_eq!(summary["ownership"], "collected");
        assert_eq!(
            summary["reference"],
            json!({"kind": "collected", "tid": "playlist123"})
        );
        assert_eq!(summary["artwork"]["src"], "https://i.scdn.co/cover");
        assert_eq!(summary["trackCount"], 12);
    }
}
