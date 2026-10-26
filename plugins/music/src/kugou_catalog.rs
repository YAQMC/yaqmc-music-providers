use super::*;

const PAGE_SIZE: u64 = 20;

// Keep display and routing fields while skipping skins, tracking metadata and
// unrelated nested resources before allocating them in the Wasm sandbox.
struct CompactCatalog(Value);
impl<'de> Deserialize<'de> for CompactCatalog {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = CompactCatalog;
            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("a KuGou catalog value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut fields = Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    if matches!(
                        key.as_str(),
                        "data"
                            | "status"
                            | "error_code"
                            | "errcode"
                            | "err_code"
                            | "errmsg"
                            | "msg"
                            | "message"
                            | "error"
                            | "error_msg"
                            | "total"
                            | "lists"
                            | "info"
                            | "songs"
                            | "songlist"
                            | "chn"
                            | "special_list"
                            | "count"
                            | "has_next"
                            | "author_id"
                            | "AuthorId"
                            | "author_name"
                            | "AuthorName"
                            | "Avatar"
                            | "sizable_avatar"
                            | "avatar"
                            | "album_id"
                            | "albumid"
                            | "album_name"
                            | "albumname"
                            | "singerid"
                            | "singer_id"
                            | "singername"
                            | "singer"
                            | "singers"
                            | "authors"
                            | "name"
                            | "id"
                            | "sizable_cover"
                            | "cover"
                            | "imgurl"
                            | "img"
                            | "pic"
                            | "flexible_cover"
                            | "intro"
                            | "language"
                            | "publish_date"
                            | "publish_time"
                            | "publishtime"
                            | "global_collection_id"
                            | "gid"
                            | "list_create_gid"
                            | "specialname"
                            | "nickname"
                            | "list_create_username"
                            | "list_create_userid"
                            | "suid"
                            | "song_count"
                            | "rankid"
                            | "rankname"
                            | "img_9"
                            | "bannerurl"
                            | "update_frequency"
                            | "base"
                            | "audio_info"
                            | "album_info"
                            | "albuminfo"
                            | "audio_name"
                            | "songname"
                            | "songName"
                            | "filename"
                            | "fileName"
                            | "FileHash"
                            | "hash"
                            | "hash_128"
                            | "duration"
                            | "duration_128"
                            | "timelength"
                            | "timelen"
                            | "album_audio_id"
                            | "mixsongid"
                            | "add_mixsongid"
                            | "trans_param"
                            | "union_cover"
                            | "pay_type"
                            | "deprecated"
                            | "hash_offset"
                            | "clip_hash"
                            | "start_ms"
                            | "end_ms"
                    ) {
                        fields.insert(key, access.next_value::<CompactCatalog>()?.0);
                    } else {
                        access.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(CompactCatalog(Value::Object(fields)))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = access.next_element::<CompactCatalog>()? {
                    values.push(value.0);
                }
                Ok(CompactCatalog(Value::Array(values)))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(CompactCatalog(json!(value)))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(CompactCatalog(Value::String(value)))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(CompactCatalog(json!(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(CompactCatalog(json!(value)))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(CompactCatalog(json!(value)))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(CompactCatalog(json!(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(CompactCatalog(Value::Null))
            }
        }
        deserializer.deserialize_any(Fields)
    }
}

fn malformed() -> String {
    error(
        "malformed-response",
        "KuGou catalog response is incomplete",
        true,
    )
}
fn numeric_id(payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    if id.parse::<u64>().is_ok_and(|id| id > 0) {
        Ok(id)
    } else {
        Err(error(
            "invalid-request",
            "KuGou catalog ID is invalid",
            false,
        ))
    }
}
fn text(item: &Value, fields: &[&str]) -> String {
    fields
        .iter()
        .filter_map(|field| item.get(*field).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
        .chars()
        .take(4000)
        .collect()
}
fn id(item: &Value, fields: &[&str]) -> String {
    fields
        .iter()
        .filter_map(|field| item.get(*field))
        .map(|value| json_id_string(Some(value), String::new()))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}
fn art(item: &Value, fields: &[&str], title: &str) -> Value {
    let src = fields
        .iter()
        .filter_map(|field| item.get(*field).and_then(Value::as_str))
        .map(normalize_kugou_artwork)
        .find(|url| artwork_request_url("kugou", url).is_some())
        .unwrap_or_default();
    json!({"src":src,"alt":title,"dominantColor":"#334155","variants":[]})
}
fn rows<'a>(root: &'a Value, paths: &[&str]) -> Result<&'a [Value], String> {
    paths
        .iter()
        .find_map(|path| root.pointer(path).and_then(Value::as_array))
        .map(Vec::as_slice)
        .ok_or_else(malformed)
}
fn post(path: &str, router: Option<&str>, body: Value) -> Result<Value, String> {
    let response = kugou_request_inner_as::<CompactCatalog>(
        "POST",
        KUGOU_API,
        path,
        &[],
        None,
        Some(&body.to_string()),
        KugouSignature::Android,
        router,
        false,
    )?
    .0;
    require_kugou_success(&response, path)?;
    Ok(response)
}
fn get(path: &str, router: Option<&str>, params: &[(&str, String)]) -> Result<Value, String> {
    let response = kugou_request_inner_as::<CompactCatalog>(
        "GET",
        KUGOU_API,
        path,
        params,
        None,
        None,
        KugouSignature::Android,
        router,
        false,
    )?
    .0;
    require_kugou_success(&response, path)?;
    Ok(response)
}
fn base_body() -> Result<Value, String> {
    let time = now_ms() / 1000;
    Ok(
        json!({"appid":1005,"clientver":20489,"clienttime":time,"mid":kugou_device_mid()?,"key":md5_hex(&format!("1005OIlwieks28dk2k092lksi2UIkp20489{time}"))}),
    )
}
fn year(item: &Value) -> u64 {
    text(item, &["publish_date", "publish_time", "publishtime"])
        .get(..4)
        .and_then(|value| value.parse().ok())
        .filter(|value| (1900..=9999).contains(value))
        .unwrap_or(0)
}
fn author(item: &Value) -> Value {
    let first = item
        .get("authors")
        .or_else(|| item.get("singers"))
        .and_then(Value::as_array)
        .and_then(|values| values.first());
    let author_id = first
        .map(|value| id(value, &["author_id", "id"]))
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| id(item, &["author_id", "singerid", "singer_id"]));
    let name = first
        .map(|value| text(value, &["author_name", "name"]))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| text(item, &["author_name", "singername", "singer"]));
    json!({"id":author_id,"name":name})
}
fn album_value(item: &Value, tracks: Vec<Value>) -> Option<Value> {
    let album_id = id(item, &["album_id", "albumid"]);
    let title = text(item, &["album_name", "albumname"]);
    if album_id.is_empty() || title.is_empty() {
        return None;
    }
    Some(
        json!({"id":album_id,"title":title,"artist":author(item),"artwork":art(item,&["sizable_cover","imgurl","img","cover"],&title),
        "releaseYear":year(item),"genre":text(item,&["language"]),"description":text(item,&["intro"]),"tracks":tracks}),
    )
}
fn album_preview(item: &Value) -> Option<Value> {
    let mut album = album_value(item, Vec::new())?;
    album["artist"]["artwork"] = art(
        item,
        &["sizable_avatar", "avatar"],
        album["artist"]["name"].as_str().unwrap_or_default(),
    );
    Some(album)
}
fn collection_id(item: &Value) -> String {
    id(item, &["global_collection_id", "gid", "list_create_gid"])
}
fn valid_collection_id(value: &str) -> bool {
    value.len() <= 128
        && value.starts_with("collection_")
        && value
            .split('_')
            .skip(1)
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
fn playlist_value(item: &Value) -> Option<Value> {
    let collection = collection_id(item);
    if !valid_collection_id(&collection) {
        return None;
    }
    let title = text(item, &["specialname", "name"]);
    if title.is_empty() {
        return None;
    }
    Some(
        json!({"id":collection,"title":title,"description":text(item,&["intro"]),
        "owner":{"id":id(item,&["list_create_userid","suid"]),"displayName":text(item,&["nickname","list_create_username"])},
        "artwork":art(item,&["flexible_cover","imgurl","img","pic"],&title),"updatedLabel":"","tracks":[]}),
    )
}
fn preview(item: &Value) -> Option<Value> {
    let value = playlist_value(item)?;
    Some(
        json!({"id":value["id"],"title":value["title"],"creator":value["owner"]["displayName"],"artwork":value["artwork"],
        "trackCount":json_u64(item,&["song_count"]).or_else(||json_u64(item,&["count"])).unwrap_or(0).min(u64::from(u32::MAX))}),
    )
}
fn artist_preview(item: &Value) -> Option<Value> {
    let artist_id = id(item, &["AuthorId", "author_id"]);
    let name = text(item, &["AuthorName", "author_name"]);
    if artist_id.is_empty() || name.is_empty() {
        return None;
    }
    Some(json!({"id":artist_id,"name":name,"artwork":art(item,&["Avatar","sizable_avatar"],&name)}))
}

pub(super) fn search(payload: &Value) -> Result<String, String> {
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song");
    let source = match kind {
        "artist" => "author",
        "album" => "album",
        "playlist" => "special",
        _ => return Err(unsupported("catalog.search kind")),
    };
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 1024)
        .ok_or_else(|| error("invalid-request", "search query is invalid", false))?;
    let page = page_number(payload);
    let size = limit(payload);
    let response = get(
        &format!("/v1/search/{source}"),
        Some("complexsearch.kugou.com"),
        &[
            ("keyword", query.to_owned()),
            ("page", page.to_string()),
            ("pagesize", size.to_string()),
            ("platform", "AndroidFilter".to_owned()),
            ("category", "1".to_owned()),
        ],
    )?;
    let raw = rows(&response, &["/data/lists", "/data/info"])?;
    let items = raw
        .iter()
        .take(size as usize)
        .filter_map(|item| match kind {
            "artist" => artist_preview(item),
            "album" => album_preview(item),
            _ => preview(item),
        })
        .collect::<Vec<_>>();
    if !raw.is_empty() && items.is_empty() {
        return Err(malformed());
    }
    let more = json_u64(&response, &["data", "total"])
        .map(|total| page.saturating_mul(size) < total)
        .unwrap_or(raw.len() as u64 >= size);
    Ok(json!({"kind":kind,"query":query,"page":page,"hasMore":more,"items":items}).to_string())
}
fn new_albums() -> Result<Vec<Value>, String> {
    let response = post(
        "/musicadservice/v1/mobile_newalbum_sp",
        None,
        json!({"apiver":6,"token":"","page":1,"pagesize":8,"withpriv":1}),
    )?;
    let raw = rows(&response, &["/data/chn"])?;
    let albums = raw
        .iter()
        .take(8)
        .filter_map(|item| album_value(item, Vec::new()))
        .collect::<Vec<_>>();
    if albums.is_empty() {
        return Err(malformed());
    }
    Ok(albums)
}
fn recommended() -> Result<Vec<Value>, String> {
    let mut body = base_body()?;
    let object = body.as_object_mut().ok_or_else(malformed)?;
    for (key,value) in json!({"platform":"android","userid":0,"module_id":1,"page":1,"pagesize":8,"special_recommend":{"withtag":1,"withsong":1,"sort":1,"ugc":1,"is_selected":0,"withrecommend":1,"area_code":1,"categoryid":0},"req_multi":1,"retrun_min":5,"return_special_falg":1}).as_object().ok_or_else(malformed)? { object.insert(key.clone(),value.clone()); }
    let response = post(
        "/v2/special_recommend",
        Some("specialrec.service.kugou.com"),
        body,
    )?;
    Ok(rows(&response, &["/data/special_list"])?
        .iter()
        .take(8)
        .filter_map(playlist_value)
        .collect())
}
pub(super) fn home(_payload: &Value) -> Result<String, String> {
    let albums = new_albums()?;
    let playlists = recommended()?;
    Ok(json!({"featured":{"eyebrow":"新碟上架","album":albums[0]},"recentlyPlayed":[],"madeForYou":[],"newReleases":albums,"recommendedSonglists":playlists}).to_string())
}
fn rank_value(item: &Value) -> Option<Value> {
    let rank = id(item, &["rankid"]);
    if rank.is_empty() {
        return None;
    }
    let title = text(item, &["rankname"]);
    Some(
        json!({"id":format!("rank:{rank}"),"title":title,"description":text(item,&["intro"]),"owner":{"id":"kugou","displayName":"酷狗音乐"},
        "artwork":art(item,&["imgurl","img_9","bannerurl"],&title),"updatedLabel":text(item,&["update_frequency"]),"tracks":[]}),
    )
}
pub(super) fn discover(_payload: &Value) -> Result<String, String> {
    let response = get(
        "/ocean/v6/rank/list",
        None,
        &[
            ("plat", "2".to_owned()),
            ("withsong", "0".to_owned()),
            ("parentid", "0".to_owned()),
        ],
    )?;
    let charts = rows(&response, &["/data/info"])?
        .iter()
        .take(12)
        .filter_map(rank_value)
        .collect::<Vec<_>>();
    Ok(
        json!({"charts":charts,"popularSonglists":recommended()?,"newAlbums":new_albums()?})
            .to_string(),
    )
}

fn catalog_song(item: &Value, profile: &str) -> Option<Value> {
    let base = item.get("base").unwrap_or(item);
    let audio = item.get("audio_info");
    let mut projected = json!({"hash":audio.map(|value|id(value,&["hash_128","hash"])).filter(|value| !value.is_empty()).unwrap_or_else(||id(item,&["hash","FileHash"])),
        "songname":text(base,&["audio_name","songname","songName","name","filename"]),"author_name":text(base,&["author_name","singername"]),
        "timelength":audio.and_then(|value|json_u64(value,&["duration_128"]).or_else(||json_u64(value,&["duration"]))).or_else(||json_u64(item,&["timelength"]).or_else(||json_u64(item,&["timelen"]))).unwrap_or(0),
        "album_id":id(base,&["album_id"]),"album_audio_id":id(base,&["album_audio_id","mixsongid","add_mixsongid"]),
        "album_name":text(item,&["album_name"]),"authors":item.get("authors"),"trans_param":{"union_cover":item.pointer("/trans_param/union_cover"),"hash_offset":item.pointer("/trans_param/hash_offset")}});
    if let Some(album) = item.get("album_info").or_else(|| item.get("albuminfo")) {
        projected["album_name"] = json!(text(album, &["album_name", "name"]));
        if projected["album_id"].as_str() == Some("") {
            projected["album_id"] = json!(id(album, &["id", "album_id"]));
        }
        if projected["trans_param"]["union_cover"].is_null() {
            projected["album_img"] = json!(text(album, &["sizable_cover", "cover"]));
        }
    }
    let mut song = map_kugou_song(&projected, profile)?;
    if projected["hash"]
        .as_str()
        .is_none_or(|hash| hash.is_empty())
    {
        return None;
    }
    let fee = json_u64(item, &["pay_type"]).or_else(|| json_u64(item, &["deprecated", "pay_type"]));
    if fee == Some(3) {
        song["availability"] = json!({"status":"entitlement-required","requiredTier":"music-vip"});
    }
    Some(song)
}
fn song_rows(response: &Value, profile: &str) -> Result<Vec<Value>, String> {
    let raw = rows(
        response,
        &["/data/songs", "/data/songlist", "/data/info", "/data"],
    )?;
    let songs = raw
        .iter()
        .take(50)
        .filter_map(|item| catalog_song(item, profile))
        .collect::<Vec<_>>();
    if !raw.is_empty() && songs.is_empty() {
        return Err(malformed());
    }
    Ok(songs)
}
fn page_offset(payload: &Value, resource: &str) -> Result<u64, String> {
    let Some(raw) = payload.get("cursor").filter(|value| !value.is_null()) else {
        return Ok(0);
    };
    let raw = raw
        .as_str()
        .filter(|value| value.len() <= 1024)
        .ok_or_else(malformed)?;
    let cursor: Value = serde_json::from_str(raw).map_err(|_| malformed())?;
    if cursor["id"] != resource || cursor["profile"] != platform_id(payload) {
        return Err(error(
            "invalid-request",
            "catalog cursor scope is invalid",
            false,
        ));
    }
    cursor["offset"]
        .as_u64()
        .filter(|offset| *offset <= 10_000 && offset % PAGE_SIZE == 0)
        .ok_or_else(malformed)
}
pub(super) fn playlist(payload: &Value) -> Result<String, String> {
    let resource = track_id(payload);
    let profile = platform_id(payload);
    let offset = page_offset(payload, &resource)?;
    let (mut value, response) = if let Some(rank) = resource
        .strip_prefix("rank:")
        .filter(|value| value.parse::<u64>().is_ok_and(|value| value > 0))
    {
        let meta = get(
            "/ocean/v6/rank/info",
            None,
            &[
                ("rankid", rank.to_owned()),
                ("rank_cid", "0".to_owned()),
                ("with_album_img", "1".to_owned()),
            ],
        )?;
        let mut meta = meta.get("data").cloned().ok_or_else(malformed)?;
        meta["rankid"] = json!(rank);
        (
            rank_value(&meta).ok_or_else(malformed)?,
            post(
                "/openapi/kmr/v2/rank/audio",
                None,
                json!({"show_portrait_mv":1,"show_type_total":1,"filter_original_remarks":1,"area_code":1,"pagesize":PAGE_SIZE,"rank_cid":0,"type":1,"page":offset/PAGE_SIZE+1,"rank_id":rank}),
            )?,
        )
    } else {
        if !valid_collection_id(&resource) {
            return Err(error(
                "invalid-request",
                "KuGou playlist ID is invalid",
                false,
            ));
        }
        let meta = post(
            "/v3/get_list_info",
            Some("pubsongs.kugou.com"),
            json!({"data":[{"global_collection_id":resource}],"userid":0,"token":""}),
        )?;
        let meta = rows(&meta, &["/data"])?.first().ok_or_else(malformed)?;
        (
            playlist_value(meta).ok_or_else(malformed)?,
            get(
                "/pubsongs/v2/get_other_list_file_nofilt",
                None,
                &[
                    ("area_code", "1".to_owned()),
                    ("begin_idx", offset.to_string()),
                    ("plat", "1".to_owned()),
                    ("type", "1".to_owned()),
                    ("mode", "1".to_owned()),
                    ("personal_switch", "1".to_owned()),
                    ("pagesize", PAGE_SIZE.to_string()),
                    ("global_collection_id", resource.clone()),
                ],
            )?,
        )
    };
    let tracks = song_rows(&response, profile)?;
    let raw = rows(&response, &["/data/songs", "/data/songlist"])?;
    let total = json_u64(&response, &["data", "count"])
        .or_else(|| json_u64(&response, &["data", "total"]))
        .or_else(|| json_u64(&response, &["total"]));
    let more = total
        .map(|total| offset + (raw.len() as u64) < total)
        .unwrap_or(raw.len() as u64 >= PAGE_SIZE);
    if more && raw.is_empty() {
        return Err(malformed());
    }
    value["tracks"] = json!(tracks);
    value["nextCursor"] = if more {
        json!(json!({"id":resource,"profile":profile,"offset":offset+PAGE_SIZE}).to_string())
    } else {
        Value::Null
    };
    Ok(value.to_string())
}
fn artist_albums(artist_id: &str, page: u64, size: u64) -> Result<Value, String> {
    post(
        "/kmr/v1/author/albums",
        Some("openapi.kugou.com"),
        json!({"author_id":artist_id,"pagesize":size,"page":page,"sort":3,"category":1,"area_code":"all"}),
    )
}
fn artist_songs(artist_id: &str, page: u64, size: u64) -> Result<Value, String> {
    let mut body = base_body()?;
    for (key, value) in
        json!({"author_id":artist_id,"pagesize":size,"page":page,"sort":1,"area_code":"all"})
            .as_object()
            .ok_or_else(malformed)?
    {
        body[key] = value.clone();
    }
    post(
        "/kmr/v1/audio_group/author",
        Some("openapi.kugou.com"),
        body,
    )
}
pub(super) fn artist(payload: &Value) -> Result<String, String> {
    let artist_id = numeric_id(payload)?;
    let profile = platform_id(payload);
    let response = post(
        "/kmr/v3/author",
        Some("openapi.kugou.com"),
        json!({"author_id":artist_id}),
    )?;
    let item = response.get("data").ok_or_else(malformed)?;
    let name = text(item, &["author_name"]);
    let albums = artist_albums(&artist_id, 1, 4)?;
    Ok(json!({"id":artist_id,"name":name,"artwork":art(item,&["sizable_avatar"],&name),"description":text(item,&["intro"]),"topSongs":song_rows(&artist_songs(&artist_id,1,10)?,profile)?,
        "albums":rows(&albums,&["/data"])?.iter().take(4).filter_map(album_preview).collect::<Vec<_>>()} ).to_string())
}
pub(super) fn artist_page(payload: &Value) -> Result<String, String> {
    let artist_id = numeric_id(payload)?;
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song");
    let page = page_number(payload);
    let size = limit(payload);
    let response = match kind {
        "song" => artist_songs(&artist_id, page, size)?,
        "album" => artist_albums(&artist_id, page, size)?,
        _ => return Err(unsupported("catalog.artist-page kind")),
    };
    let items = if kind == "song" {
        song_rows(&response, platform_id(payload))?
    } else {
        rows(&response, &["/data"])?
            .iter()
            .take(size as usize)
            .filter_map(album_preview)
            .collect()
    };
    let more = json_u64(&response, &["total"])
        .map(|total| page * size < total)
        .unwrap_or(items.len() as u64 >= size);
    Ok(
        json!({"kind":kind,"artistId":artist_id,"page":page,"hasMore":more,"items":items})
            .to_string(),
    )
}
pub(super) fn album(payload: &Value) -> Result<String, String> {
    let album_id = numeric_id(payload)?;
    let profile = platform_id(payload);
    let meta = post(
        "/kmr/v2/albums",
        Some("openapi.kugou.com"),
        json!({"data":[{"album_id":album_id}],"is_buy":0,"fields":"album_id,album_name,publish_date,sizable_cover,intro,language,is_publish,authors,author_name"}),
    )?;
    let item = rows(&meta, &["/data"])?.first().ok_or_else(malformed)?;
    let mut tracks = Vec::new();
    for page in 1..=10 {
        let response = post(
            "/v1/album_audio/lite",
            Some("openapi.kugou.com"),
            json!({"album_id":album_id,"is_buy":"","page":page,"pagesize":50}),
        )?;
        let raw = rows(&response, &["/data/songs"])?;
        tracks.extend(song_rows(&response, profile)?);
        let total =
            json_u64(&response, &["data", "total"]).or_else(|| json_u64(&response, &["total"]));
        if total.is_some_and(|total| page * 50 >= total) || raw.len() < 50 {
            return Ok(album_value(item, tracks).ok_or_else(malformed)?.to_string());
        }
    }
    Err(error(
        "provider-unavailable",
        "KuGou album exceeds the supported size",
        false,
    ))
}

fn favorite_collection(item: &Value) -> bool {
    json_u64(item, &["is_def"]) == Some(1)
        && (matches!(
            text(item, &["name"]).as_str(),
            "我喜欢" | "我喜欢的音乐" | "喜欢的音乐"
        ) || json_u64(item, &["listid"]) == Some(1))
}
pub(super) fn favorite_songs(profile: &str, payload: &Value) -> Result<String, String> {
    for page in 1..=10 {
        let response = kugou_request_body(
            profile,
            "/v7/get_all_list",
            &json!({"total_ver":979,"type":2,"page":page,"pagesize":50}).to_string(),
        )?;
        let raw = kugou_list_items(&response)?;
        if let Some(item) = raw.iter().find(|item| favorite_collection(item)) {
            let summary = kugou_playlist_summary(item, profile).ok_or_else(malformed)?;
            let list_id = summary["id"].as_str().ok_or_else(malformed)?.to_owned();
            let detail = kugou_playlist_tracks(profile, summary, &list_id, payload)?;
            let mut detail: Value = serde_json::from_str(&detail).map_err(|_| malformed())?;
            if let Some(items) = detail["tracks"]["items"].as_array_mut() {
                for song in items {
                    song["isFavorite"] = json!(true);
                }
            }
            return Ok(detail["tracks"].to_string());
        }
        if raw.len() < 50
            || kugou_collection_total(&response).is_some_and(|total| page * 50 >= total)
        {
            return Err(error(
                "not-found",
                "KuGou did not return its favorite collection",
                false,
            ));
        }
    }
    Err(error(
        "provider-unavailable",
        "KuGou account exceeds the supported playlist lookup size",
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_catalog_retains_playback_fields_and_skips_unrelated_resources() {
        let input = json!({"status":1,"data":{"songs":[{
            "base":{"audio_name":"Song","album_id":3,"album_audio_id":42},
            "audio_info":{"hash_128":"0123456789abcdef0123456789abcdef","duration_128":217756},
            "trans_param":{"hash_offset":{"clip_hash":"abcdef0123456789abcdef0123456789","start_ms":30000,"end_ms":90000}},
            "skin":{"data":{"songs":[{"name":"unrelated"}]}}
        }]},"tracking":{"data":"discard"}});
        let compact: CompactCatalog = serde_json::from_str(&input.to_string()).unwrap();
        assert!(compact.0.get("tracking").is_none());
        assert!(compact.0["data"]["songs"][0].get("skin").is_none());
        let song = song_rows(&compact.0, "work").unwrap().remove(0);
        assert_eq!(song["provider"]["numericId"], 42);
        assert_eq!(song["durationMs"], 217756);
        assert_eq!(
            song["provider"]["mediaId"],
            "abcdef0123456789abcdef0123456789"
        );
    }
    #[test]
    fn maps_all_search_preview_types_and_granted_images() {
        let artist=artist_preview(&json!({"AuthorId":4490,"AuthorName":"Singer","Avatar":"http://singerimg.kugou.com/a.jpg"})).unwrap();
        assert_eq!(
            artist["artwork"]["src"],
            "https://singerimg.kugou.com/a.jpg"
        );
        let album=album_preview(&json!({"albumid":1,"albumname":"Album","singers":[{"id":4490,"name":"Singer"}],"publish_time":"2023-11-26","img":"http://imge.kugou.com/a.jpg"})).unwrap();
        assert_eq!(album["releaseYear"], 2023);
        assert_eq!(album["artist"]["id"], "4490");
        let playlist=preview(&json!({"gid":"collection_3_7_1_0","specialname":"List","song_count":101,"nickname":"Owner","img":"http://c1.kgimg.com/a.jpg"})).unwrap();
        assert_eq!(playlist["trackCount"], 101);
        assert_eq!(playlist["artwork"]["src"], "https://c1.kgimg.com/a.jpg");
        assert!(!valid_collection_id("collection_3_7_../1"));
        assert!(playlist_value(&json!({"global_collection_id":"bad"})).is_none());
    }
    #[test]
    fn maps_kmr_nested_audio_without_losing_identifiers_or_units() {
        let song=catalog_song(&json!({"base":{"audio_name":"Song","author_name":"Singer","album_audio_id":42,"album_id":3},"audio_info":{"hash_128":"HASH","duration_128":217756},"album_info":{"album_name":"Album","sizable_cover":"http://imge.kugou.com/a.jpg"}}),"work").unwrap();
        assert_eq!(song["durationMs"], 217756);
        assert_eq!(song["title"], "Song");
        assert_eq!(song["provider"]["numericId"], 42);
        assert_eq!(song["provider"]["albumId"], "3");
        assert_eq!(song["provider"]["profileId"], "work");
    }
    #[test]
    fn favorite_lookup_uses_the_system_collection_not_arbitrary_names() {
        assert!(favorite_collection(
            &json!({"is_def":1,"listid":1,"name":"我喜欢"})
        ));
        assert!(!favorite_collection(
            &json!({"is_def":0,"listid":2,"name":"我喜欢"})
        ));
    }
    #[test]
    fn playlist_cursors_are_bound_to_resource_and_profile() {
        let cursor = json!({"id":"collection_3_7_1_0","profile":"work","offset":20}).to_string();
        let payload = json!({"profileId":"work","cursor":cursor});
        assert_eq!(page_offset(&payload, "collection_3_7_1_0").unwrap(), 20);
        assert!(page_offset(&payload, "collection_3_7_2_0").is_err());
    }
}
