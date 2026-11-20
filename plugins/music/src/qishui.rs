//! Qishui Music (汽水音乐) public catalog.
//!
//! Endpoints and response shapes were identified with the MIT-licensed
//! guowenye/qishui-api reference (see THIRD_PARTY_NOTICES.md); this adapter is
//! a separate Rust implementation. It only uses anonymous public routes and
//! never decrypts PlayAuth/spade protected media.

use super::*;

pub(super) const LUNA: &str = "https://beta-luna.douyin.com";
const PC: &str = "https://api.qishui.com";
// Exact origins granted in the manifest; the host has no wildcard grants.
const MEDIA_ORIGINS: [&str; 4] = [
    "https://v3-luna.douyinvod.com",
    "https://v5-ex-luna.douyinvod.com",
    "https://v11-luna.douyinvod.com",
    "https://v26-luna.douyinvod.com",
];
pub(super) const IMAGE_HOSTS: [&str; 2] = ["p3-luna.douyinpic.com", "p6-luna.douyinpic.com"];
const PLAYLIST_PAGE: u64 = 50;
// Public pages carry comments and related lists; the host trims them to the
// fields read here so parsing stays within the sandbox fuel budget.
pub(super) const LUNA_FIELDS: [&str; 36] = [
    "seo_track",
    "track",
    "lyric",
    "content",
    "track_player",
    "video_model",
    "playlist",
    "media_resources",
    "has_more",
    "next_cursor",
    "entity",
    "track_wrapper",
    "inner_block",
    "resources",
    "result_groups",
    "data",
    "id",
    "name",
    "title",
    "desc",
    "description",
    "count_tracks",
    "owner",
    "nickname",
    "duration",
    "artists",
    "simple_display_name",
    "album",
    "url_cover",
    "uri",
    "urls",
    "template_prefix",
    "bit_rates",
    "preview",
    "label_info",
    "only_vip_playable",
];

fn malformed() -> String {
    error(
        "malformed-response",
        "Qishui Music response is incomplete",
        true,
    )
}

fn numeric_id(value: &str) -> Option<&str> {
    (!value.is_empty() && value.len() <= 32 && value.bytes().all(|b| b.is_ascii_digit()))
        .then_some(value)
}

fn id_of(value: &Value) -> String {
    json_id_string(value.get("id"), String::new())
}

fn artwork(cover: Option<&Value>, title: &str) -> Value {
    let src = cover
        .and_then(|cover| {
            let uri = cover.get("uri")?.as_str()?;
            let prefix = cover.get("template_prefix")?.as_str()?;
            let base = cover.get("urls")?.as_array()?.iter().find_map(|url| {
                let url = url.as_str()?;
                let host = url.strip_prefix("https://")?.split('/').next()?;
                IMAGE_HOSTS.contains(&host).then_some(url)
            })?;
            (!uri.is_empty() && !uri.contains(['?', '#', '~']))
                .then(|| format!("{base}{uri}~{prefix}-resize:400:400.jpg"))
        })
        .unwrap_or_default();
    json!({"src": src, "alt": title, "dominantColor": "#334155", "variants": []})
}

pub(super) fn map_track(track: &Value, profile: &str) -> Option<Value> {
    let id = id_of(track);
    numeric_id(&id)?;
    let title = track
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
    let artists = track
        .get("artists")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    let name = first_string(artist, &[&["name"], &["simple_display_name"]])
                        .unwrap_or_else(|| "Unknown".to_owned());
                    json!({"id": json_id_string(artist.get("id"), format!("{id}-artist-{index}")), "name": name})
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let album = track.get("album");
    let has_source = track
        .get("bit_rates")
        .and_then(Value::as_array)
        .is_some_and(|rates| !rates.is_empty())
        || track.get("preview").is_some_and(Value::is_object);
    let mut song = song_value(
        "qishui",
        profile,
        &id,
        title,
        artists,
        album
            .and_then(|album| album.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Unknown album"),
        artwork(album.and_then(|album| album.get("url_cover")), title)["src"].clone(),
        json_u64(track, &["duration"]).unwrap_or(0),
        Value::Null,
        has_source,
    );
    if has_source {
        // Catalog labels describe the track, not what the public player returns.
        song.as_object_mut()?.remove("playbackCapability");
        if track.pointer("/label_info/only_vip_playable") == Some(&Value::Bool(true)) {
            song["availability"] =
                json!({"status":"entitlement-required", "requiredTier":"music-vip"});
        }
    }
    if let Some(album_id) = album.map(id_of).filter(|id| numeric_id(id).is_some()) {
        song["album"]["id"] = json!(album_id);
        song["provider"]["albumId"] = json!(album_id);
    }
    Some(song)
}

fn playlist_preview(playlist: &Value) -> Option<Value> {
    let id = id_of(playlist);
    numeric_id(&id)?;
    let title = first_string(playlist, &[&["title"], &["name"]]).unwrap_or_default();
    Some(json!({
        "id": id,
        "title": title,
        "creator": json_string(playlist, &["owner", "nickname"]).unwrap_or_default(),
        "artwork": artwork(playlist.get("url_cover"), &title),
        "trackCount": json_u64(playlist, &["count_tracks"]).unwrap_or(0),
    }))
}

fn playlist_value(playlist: &Value, tracks: Vec<Value>) -> Option<Value> {
    let preview = playlist_preview(playlist)?;
    Some(json!({
        "id": preview["id"],
        "title": preview["title"],
        "description": first_string(playlist, &[&["desc"], &["description"]]).unwrap_or_default(),
        "owner": {
            "id": json_id_string(playlist.pointer("/owner/id"), String::new()),
            "displayName": json_string(playlist, &["owner", "nickname"]).unwrap_or_else(|| "汽水音乐".to_owned()),
        },
        "artwork": preview["artwork"],
        "updatedLabel": "",
        "tracks": tracks,
    }))
}

fn resource_track(resource: &Value) -> Option<&Value> {
    let entity = resource.get("entity")?;
    entity
        .pointer("/track_wrapper/track")
        .or_else(|| entity.get("track"))
        .filter(|track| track.is_object())
}

fn luna_post(path: &str, body: &Value) -> Result<Value, String> {
    request(
        "POST",
        &format!("{LUNA}{path}"),
        Some(&body.to_string()),
        Some("application/json; charset=utf-8"),
        None,
        None,
    )
}

fn h5_track(id: &str) -> Result<Value, String> {
    let id = numeric_id(id)
        .ok_or_else(|| error("invalid-request", "Qishui track ID is invalid", false))?;
    request(
        "GET",
        &query_url(
            LUNA,
            "/luna/h5/seo_track",
            &[
                ("track_id", id.to_owned()),
                ("device_platform", "web".to_owned()),
            ],
        ),
        None,
        None,
        None,
        None,
    )
}

pub(super) fn search_page(
    body: &Value,
    kind: &str,
    profile: &str,
    page: u64,
    query: &str,
) -> Result<String, String> {
    // Unsigned PC searches come back as an empty body or `empty_search` with
    // no groups; that is a refusal, not an empty result set.
    let groups = body
        .get("result_groups")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "provider-unavailable",
                "Qishui Music did not answer this search; its search API requires signed client requests",
                false,
            )
        })?;
    let resources = groups
        .iter()
        .filter_map(|group| group.get("data").and_then(Value::as_array))
        .flatten();
    let items = if kind == "playlist" {
        resources
            .filter_map(|resource| resource.pointer("/entity/playlist"))
            .filter_map(playlist_preview)
            .collect::<Vec<_>>()
    } else {
        resources
            .filter_map(resource_track)
            .filter_map(|track| map_track(track, profile))
            .collect::<Vec<_>>()
    };
    let has_more = body.get("has_more").and_then(Value::as_bool) == Some(true);
    Ok(
        json!({"kind": kind, "query": query, "page": page, "hasMore": has_more, "items": items})
            .to_string(),
    )
}

pub(super) fn search(payload: &Value) -> Result<String, String> {
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song");
    let path = match kind {
        "song" => "/luna/pc/search/track",
        "playlist" => "/luna/pc/search/playlist",
        _ => return Err(unsupported("catalog.search kind")),
    };
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if query.trim().is_empty() {
        return Err(error("invalid-request", "search query is empty", false));
    }
    let page = page_number(payload);
    let cursor = page.saturating_sub(1).saturating_mul(limit(payload));
    let body = request(
        "GET",
        &query_url(
            PC,
            path,
            &[
                ("aid", "386088".to_owned()),
                ("app_name", "luna_pc".to_owned()),
                ("device_platform", "windows".to_owned()),
                ("q", query.to_owned()),
                ("cursor", cursor.to_string()),
                ("search_method", "input".to_owned()),
            ],
        ),
        None,
        None,
        None,
        None,
    )?;
    search_page(&body, kind, platform_id(payload), page, query)
}

pub(super) fn playlist_page(body: &Value, profile: &str, cursor: &str) -> Result<String, String> {
    let tracks = body
        .get("media_resources")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?
        .iter()
        .filter_map(resource_track)
        .filter_map(|track| map_track(track, profile))
        .collect::<Vec<_>>();
    let mut value = playlist_value(body.get("playlist").ok_or_else(malformed)?, tracks)
        .ok_or_else(malformed)?;
    value["nextCursor"] = body
        .get("next_cursor")
        .and_then(Value::as_str)
        .filter(|_| body.get("has_more").and_then(Value::as_bool) == Some(true))
        .and_then(numeric_id)
        .filter(|next| *next != cursor)
        .map_or(Value::Null, |next| json!(next));
    Ok(value.to_string())
}

pub(super) fn playlist(payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    let id = numeric_id(&id)
        .ok_or_else(|| error("invalid-request", "Qishui playlist ID is invalid", false))?;
    let cursor = match payload.get("cursor").and_then(Value::as_str) {
        None => "",
        Some(cursor) => numeric_id(cursor)
            .ok_or_else(|| error("invalid-request", "playlist cursor is invalid", false))?,
    };
    let mut body = json!({"playlist_id": id, "count": PLAYLIST_PAGE});
    if !cursor.is_empty() {
        body["cursor"] = json!(cursor);
    }
    let response = luna_post("/luna/playlist/detail", &body)?;
    playlist_page(&response, platform_id(payload), cursor)
}

pub(super) fn discover() -> Result<String, String> {
    let response = luna_post("/luna/discover/mix", &json!({"count": 10}))?;
    let playlists = response
        .get("inner_block")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?
        .iter()
        .filter_map(|block| block.get("resources").and_then(Value::as_array))
        .flatten()
        .filter_map(|resource| resource.pointer("/entity/playlist"))
        .filter_map(|playlist| playlist_value(playlist, Vec::new()))
        .collect::<Vec<_>>();
    Ok(
        json!({"charts": [], "popularSonglists": playlists, "newSongs": null, "newAlbums": [],
        "categories": [], "podcasts": [], "newMvs": [], "featured": []})
        .to_string(),
    )
}

pub(super) fn song(payload: &Value) -> Result<String, String> {
    let response = h5_track(&track_id(payload))?;
    let track = response.pointer("/seo_track/track").ok_or_else(malformed)?;
    map_track(track, platform_id(payload))
        .map(|song| song.to_string())
        .ok_or_else(malformed)
}

pub(super) fn lyrics(payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    let response = h5_track(&id)?;
    lyric_document(&response, &id)
}

pub(super) fn lyric_document(response: &Value, id: &str) -> Result<String, String> {
    let Some(content) = json_string(response, &["lyric", "content"]).filter(|v| !v.is_empty())
    else {
        return Ok("null".to_owned());
    };
    let mut document = kugou_lyrics::parse(&content, id).map_err(|_| {
        error(
            "malformed-response",
            "Qishui Music returned invalid timed lyrics",
            true,
        )
    })?;
    document["metadata"]["sourceLabel"] = json!("汽水音乐");
    Ok(document.to_string())
}

/// Picks the unencrypted public-player source on a granted CDN origin.
/// Returns (url, declared size, is preview).
pub(super) fn media_source(response: &Value) -> Result<(String, u64, bool), String> {
    let unavailable = |message: &str| error("full-playback-unavailable", message, false);
    let model: Value = response
        .pointer("/track_player/video_model")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str(raw).ok())
        .ok_or_else(|| unavailable("Qishui Music returned no public media source"))?;
    let item = model
        .pointer("/video_list/0")
        .ok_or_else(|| unavailable("Qishui Music returned no public media source"))?;
    // Protected media needs PlayAuth/spade decryption, which is not supported.
    if ["play_auth", "spade_a"]
        .iter()
        .any(|key| item.get(*key).is_some_and(|value| !value.is_null()))
    {
        return Err(unavailable(
            "Qishui Music only returned a protected media source",
        ));
    }
    let url = ["main_url", "backup_url"]
        .iter()
        .filter_map(|key| item.get(*key).and_then(Value::as_str))
        .find(|url| {
            url.len() <= 4096
                && MEDIA_ORIGINS.iter().any(|origin| {
                    url.strip_prefix(origin)
                        .is_some_and(|rest| rest.starts_with('/'))
                })
        })
        .ok_or_else(|| unavailable("Qishui Music media CDN is not among the granted origins"))?;
    let size = json_u64(item, &["video_meta", "size"]).unwrap_or(0);
    let clip_ms = model
        .get("video_duration")
        .and_then(Value::as_f64)
        .map_or(0, |seconds| (seconds * 1000.0) as u64);
    let track_ms = json_u64(response, &["seo_track", "track", "duration"]).unwrap_or(0);
    let preview = clip_ms > 0 && clip_ms.saturating_add(2000) < track_ms;
    Ok((url.to_owned(), size, preview))
}

pub(super) fn playback(id: &str, quality: &str) -> Result<String, String> {
    let (url, size, preview) = media_source(&h5_track(id)?)?;
    let (length, mime) = if size > 0 {
        (size, "audio/mp4".to_owned())
    } else {
        media_metadata(&url)?
    };
    let mut value: Value = serde_json::from_str(&playback_value(
        "qishui", id, quality, "standard", &url, length, &mime, preview, false,
    ))
    .map_err(|_| malformed())?;
    // The CDN redirects browser user agents to other edges; keep the one that was checked.
    value["source"]["request"]["headers"] = json!({"user-agent": "YAQMC/1.0 music-provider"});
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_track(id: &str) -> Value {
        json!({
            "id": id, "name": "Fixture Song", "duration": 218133,
            "artists": [{"id": "6669580815212484610", "name": "Fixture Band"}, {"simple_display_name": "Guest"}],
            "album": {"id": "6732580533626210306", "name": "Fixture Album", "url_cover": {
                "uri": "tos-cn-v-2774c002/cover", "template_prefix": "tplv-b829550vbb",
                "urls": ["https://evil.example/img/", "https://p6-luna.douyinpic.com/img/"]}},
            "bit_rates": [{"br": 132178, "quality": "higher"}],
            "label_info": {"only_vip_playable": true},
        })
    }

    #[test]
    fn tracks_map_native_ids_artwork_and_unplayable_metadata() {
        let song = map_track(&fixture_track("6732580534901278722"), "work").unwrap();
        assert_eq!(song["id"], "6732580534901278722");
        assert_eq!(
            song["provider"],
            json!({"providerId": "org.yaqmc.providers.music.qishui", "profileId": "work",
                "trackId": "6732580534901278722", "albumId": "6732580533626210306"})
        );
        assert_eq!(song["durationMs"], 218133);
        assert_eq!(song["artists"][1]["name"], "Guest");
        assert_eq!(
            song["artwork"]["src"],
            "https://p6-luna.douyinpic.com/img/tos-cn-v-2774c002/cover~tplv-b829550vbb-resize:400:400.jpg"
        );
        assert_eq!(song["availability"]["status"], "entitlement-required");
        assert!(song.get("playbackCapability").is_none());

        let mut silent = fixture_track("7");
        silent["bit_rates"] = json!([]);
        let song = map_track(&silent, "default").unwrap();
        assert_eq!(song["availability"]["status"], "unavailable");
        assert_eq!(song["playbackCapability"]["status"], "unavailable");
        assert!(map_track(&fixture_track("not-numeric"), "default").is_none());
    }

    #[test]
    fn playlist_detail_maps_tracks_and_cursor_pages() {
        let body = json!({
            "has_more": true, "next_cursor": "50",
            "playlist": {"id": "7096700219496368135", "title": "Fixture List", "desc": "About",
                "count_tracks": 60, "owner": {"id": "105175843370", "nickname": "Curator"}},
            "media_resources": [
                {"entity": {"track_wrapper": {"track": fixture_track("1")}}},
                {"entity": {"video": {"id": "2"}}},
                {"entity": {"track": fixture_track("3")}},
            ],
        });
        let page: Value =
            serde_json::from_str(&playlist_page(&body, "default", "").unwrap()).unwrap();
        assert_eq!(page["id"], "7096700219496368135");
        assert_eq!(page["owner"]["displayName"], "Curator");
        assert_eq!(page["tracks"].as_array().unwrap().len(), 2);
        assert_eq!(page["nextCursor"], "50");
        let mut last = body.clone();
        last.as_object_mut().unwrap().remove("has_more");
        let page: Value =
            serde_json::from_str(&playlist_page(&last, "default", "").unwrap()).unwrap();
        assert!(page["nextCursor"].is_null());
        // A cursor that does not advance must end paging instead of looping.
        let page: Value =
            serde_json::from_str(&playlist_page(&body, "default", "50").unwrap()).unwrap();
        assert!(page["nextCursor"].is_null());
        assert!(playlist_page(&json!({"media_resources": []}), "default", "").is_err());
    }

    #[test]
    fn search_maps_tracks_and_playlists_but_reports_refusals() {
        let body = json!({"has_more": true, "result_groups": [{"data": [
            {"entity": {"track_wrapper": {"track": fixture_track("11")}}},
            {"entity": {"playlist": {"id": "22", "title": "Found", "count_tracks": 9, "owner": {"nickname": "Me"}}}},
        ]}]});
        let songs: Value =
            serde_json::from_str(&search_page(&body, "song", "default", 2, "q").unwrap()).unwrap();
        assert_eq!(songs["page"], 2);
        assert_eq!(songs["hasMore"], true);
        assert_eq!(songs["items"].as_array().unwrap().len(), 1);
        assert_eq!(songs["items"][0]["id"], "11");
        let lists: Value =
            serde_json::from_str(&search_page(&body, "playlist", "default", 1, "q").unwrap())
                .unwrap();
        assert_eq!(
            lists["items"],
            json!([{"id": "22", "title": "Found", "creator": "Me", "trackCount": 9,
                "artwork": {"src": "", "alt": "Found", "dominantColor": "#334155", "variants": []}}])
        );
        for refused in [Value::Null, json!({"extra": {"empty_search": 1}})] {
            let reason = search_page(&refused, "song", "default", 1, "q").unwrap_err();
            assert!(reason.contains("provider-unavailable"));
        }
    }

    fn h5(video: Value, duration: f64) -> Value {
        json!({
            "seo_track": {"track": fixture_track("1")},
            "track_player": {"video_model": json!({"video_duration": duration, "video_list": [video]}).to_string()},
        })
    }

    #[test]
    fn media_source_requires_unprotected_granted_cdn_urls() {
        let full = h5(
            json!({"main_url": "https://v95-se-x-luna.douyinvod.com/a", "backup_url": "https://v26-luna.douyinvod.com/a?sig=1", "video_meta": {"size": 470932}}),
            218.1,
        );
        assert_eq!(
            media_source(&full).unwrap(),
            (
                "https://v26-luna.douyinvod.com/a?sig=1".to_owned(),
                470932,
                false
            )
        );
        let clip = h5(
            json!({"main_url": "https://v11-luna.douyinvod.com/b"}),
            29.0,
        );
        assert!(media_source(&clip).unwrap().2);
        for blocked in [
            h5(
                json!({"main_url": "https://v26-luna.douyinvod.com/c", "play_auth": "x"}),
                30.0,
            ),
            h5(
                json!({"main_url": "https://v26-luna.douyinvod.com.evil.example/c"}),
                30.0,
            ),
            h5(
                json!({"main_url": "https://v95-se-x-luna.douyinvod.com/c"}),
                30.0,
            ),
            json!({"seo_track": {"track": fixture_track("1")}}),
        ] {
            assert!(media_source(&blocked)
                .unwrap_err()
                .contains("full-playback-unavailable"));
        }
    }

    #[test]
    fn account_surface_is_a_permanent_guest_without_private_features() {
        assert_eq!(login_methods("qishui").unwrap(), "[]");
        let advertised = capabilities("qishui", false);
        assert!(advertised
            .as_object()
            .unwrap()
            .values()
            .all(|value| value == &Value::Bool(false)));
        assert!(account_profile_is_authenticated("qishui", "default").is_err());
    }

    #[test]
    fn word_timed_lyrics_reuse_the_krc_parser_with_a_qishui_label() {
        let response = json!({"lyric": {"content": "[12080,3850]<0,1110,0>毎<1110,900,0>日"}});
        let document: Value =
            serde_json::from_str(&lyric_document(&response, "1").unwrap()).unwrap();
        assert_eq!(document["syncMode"], "word");
        assert_eq!(document["metadata"]["sourceLabel"], "汽水音乐");
        assert_eq!(document["lines"][0]["text"], "毎日");
        assert_eq!(lyric_document(&json!({}), "1").unwrap(), "null");
    }
}
