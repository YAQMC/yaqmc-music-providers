use super::*;

const MAX_LYRIC_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 4000;
const MAX_WORDS: usize = 40_000;
const XOR_KEY: [u8; 16] = [
    64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105,
];

fn malformed() -> String {
    error(
        "malformed-response",
        "KuGou returned invalid timed lyrics",
        true,
    )
}

fn decode(encoded: &str) -> Result<String, String> {
    if encoded.len() > MAX_LYRIC_BYTES {
        return Err(malformed());
    }
    let mut data = STANDARD.decode(encoded).map_err(|_| malformed())?;
    if !data.starts_with(b"krc1") {
        return Err(malformed());
    }
    for (i, byte) in data[4..].iter_mut().enumerate() {
        *byte ^= XOR_KEY[i % XOR_KEY.len()];
    }
    let plain =
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&data[4..], MAX_LYRIC_BYTES)
            .map_err(|_| malformed())?;
    String::from_utf8(plain).map_err(|_| malformed())
}

fn offset(raw: &str) -> i64 {
    raw.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("[offset:")?
                .strip_suffix(']')?
                .parse::<i64>()
                .ok()
        })
        .find(|v| v.unsigned_abs() <= 86_400_000)
        .unwrap_or(0)
}

fn parse(raw: &str, song_id: &str) -> Result<Value, String> {
    if raw.len() > MAX_LYRIC_BYTES {
        return Err(malformed());
    }
    let mut lines = Vec::new();
    let mut total_words = 0;
    for line in raw.lines() {
        let Some((header, mut content)) = line
            .trim_start_matches('\u{feff}')
            .strip_prefix('[')
            .and_then(|line| line.split_once(']'))
        else {
            continue;
        };
        let Some((start, duration)) = header.split_once(',') else {
            continue;
        };
        let (Ok(start), Ok(duration)) = (start.parse::<u64>(), duration.parse::<u64>()) else {
            return Err(malformed());
        };
        let end = start
            .checked_add(duration)
            .filter(|end| *end <= 86_400_000)
            .ok_or_else(malformed)?;
        let mut line_end = end;
        let mut words = Vec::new();
        let mut text = String::new();
        while !content.is_empty() {
            let Some((timing, rest)) = content
                .strip_prefix('<')
                .and_then(|text| text.split_once('>'))
            else {
                return Err(malformed());
            };
            let mut timing = timing.split(',');
            let relative = timing
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(malformed)?;
            let length = timing
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(malformed)?;
            let word_start = start.checked_add(relative).ok_or_else(malformed)?;
            let word_end = word_start
                .checked_add(length)
                // The official KRC response can round a relative word start
                // up by 1 ms while keeping the original line duration.
                .filter(|end_ms| *end_ms <= end.saturating_add(10))
                .ok_or_else(malformed)?;
            line_end = line_end.max(word_end);
            let next = rest.find('<').unwrap_or(rest.len());
            let word = &rest[..next];
            text.push_str(word);
            words.push(json!({"startMs":word_start,"endMs":word_end,"text":word}));
            total_words += 1;
            if total_words > MAX_WORDS || text.len() > 16_000 {
                return Err(malformed());
            }
            content = &rest[next..];
        }
        if !text.trim().is_empty() && end > start {
            lines.push(json!({"id":format!("line-{}",lines.len()),"startMs":start,"endMs":line_end,"text":text,"words":words}));
        }
        if lines.len() > MAX_LINES {
            return Err(malformed());
        }
    }
    if lines.is_empty() {
        return Err(malformed());
    }
    Ok(
        json!({"songId":song_id,"syncMode":"word","metadata":{"sourceLabel":"酷狗音乐","offsetMs":offset(raw)},"vocalists":[],"lines":lines}),
    )
}

/// LRC has start times only. Give AMLL finite line intervals so it does not
/// discard every line and fall back to the unsynchronized static-text view.
pub(super) fn lrc_lines(raw: &str, duration_ms: u64) -> Vec<Value> {
    let mut timed = parse_lrc(raw);
    timed.truncate(MAX_LINES);
    timed.sort_by_key(|line| line.0);
    let mut ends = vec![0; timed.len()];
    let mut next = None;
    for i in (0..timed.len()).rev() {
        if i + 1 < timed.len() && timed[i + 1].0 > timed[i].0 {
            next = Some(timed[i + 1].0);
        }
        ends[i] = next.unwrap_or_else(|| {
            if duration_ms > timed[i].0 {
                duration_ms
            } else {
                timed[i].0.saturating_add(5000)
            }
        });
    }
    timed.into_iter().enumerate().map(|(i,(start,text))| json!({"id":format!("line-{i}"),"startMs":start,"endMs":ends[i],"text":text,"words":[]})).collect()
}

fn download(lyric_id: &str, access_key: &str, format: &str) -> Result<Value, String> {
    kugou_request(
        "GET",
        KUGOU_LYRICS,
        "/download",
        &[
            ("ver", "1".to_owned()),
            ("client", "android".to_owned()),
            ("id", lyric_id.to_owned()),
            ("accesskey", access_key.to_owned()),
            ("fmt", format.to_owned()),
            ("charset", "utf8".to_owned()),
        ],
        None,
        KugouSignature::Android,
        None,
        false,
    )
}

pub(super) fn get(payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    let detail = parse_object(&song_detail("kugou", payload)?)?;
    let title = json_string(&detail, &["title"]).unwrap_or_default();
    let duration = json_u64(&detail, &["durationMs"]).unwrap_or(0);
    let search = kugou_request(
        "GET",
        KUGOU_LYRICS,
        "/search",
        &[
            ("ver", "1".to_owned()),
            ("client", "pc".to_owned()),
            ("hash", id.clone()),
            (
                "album_audio_id",
                json_u64(&detail, &["provider", "numericId"])
                    .unwrap_or(0)
                    .to_string(),
            ),
            ("duration", duration.to_string()),
            ("keyword", title.clone()),
            ("lrctxt", "1".to_owned()),
            ("man", "no".to_owned()),
        ],
        None,
        KugouSignature::Android,
        None,
        false,
    )?;
    let Some((lyric_id, access_key)) = kugou_lyric_candidate(&search, &title, duration) else {
        return Ok("null".to_owned());
    };
    let word_document = download(&lyric_id, &access_key, "krc")
        .and_then(|response| decode(response["content"].as_str().ok_or_else(malformed)?))
        .and_then(|plain| parse(&plain, &id));
    if let Ok(document) = word_document {
        return Ok(document.to_string());
    }
    let response = download(&lyric_id, &access_key, "lrc")?;
    let encoded = response["content"]
        .as_str()
        .filter(|value| value.len() <= MAX_LYRIC_BYTES)
        .ok_or_else(malformed)?;
    let plain = String::from_utf8(STANDARD.decode(encoded).map_err(|_| malformed())?)
        .map_err(|_| malformed())?;
    let lines = lrc_lines(&plain, duration);
    Ok(json!({"songId":id,"syncMode":if lines.is_empty(){"unsynchronized"}else{"line"},"metadata":{"sourceLabel":"酷狗音乐","offsetMs":offset(&plain)},"vocalists":[],"lines":lines}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_real_word_intervals_without_synthesizing_character_timing() {
        let doc = parse("[offset:-20]\n[1000,900]<0,300,0>你<300,600,0>好", "song").unwrap();
        assert_eq!(doc["syncMode"], "word");
        assert_eq!(doc["metadata"]["offsetMs"], -20);
        assert_eq!(doc["lines"][0]["endMs"], 1900);
        assert_eq!(
            doc["lines"][0]["words"][1],
            json!({"startMs":1300,"endMs":1900,"text":"好"})
        );
        assert!(parse("[1000,900]<800,200,0>bad", "song").is_err());
        let rounded = parse("[14407,2359]<1571,789,0>乐", "song").unwrap();
        assert_eq!(rounded["lines"][0]["endMs"], 16767);
        assert!(parse("[1000,900]<300,611,0>bad", "song").is_err());
        assert!(parse("[18446744073709551615,1]<0,1,0>x", "song").is_err());
    }
    #[test]
    fn krc_decode_checks_header_corruption_and_output_limit() {
        let raw = "[1000,900]<0,300,0>你<300,600,0>好";
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(raw.as_bytes(), 6);
        let mut data = b"krc1".to_vec();
        data.extend(
            compressed
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ XOR_KEY[i % 16]),
        );
        assert_eq!(decode(&STANDARD.encode(&data)).unwrap(), raw);
        data[0] = 0;
        assert!(decode(&STANDARD.encode(&data)).is_err());
        assert!(decode("not base64").is_err());
        let bomb = miniz_oxide::deflate::compress_to_vec_zlib(&vec![b'a'; MAX_LYRIC_BYTES + 1], 6);
        let mut data = b"krc1".to_vec();
        data.extend(bomb.iter().enumerate().map(|(i, b)| b ^ XOR_KEY[i % 16]));
        assert!(decode(&STANDARD.encode(data)).is_err());
    }
    #[test]
    fn lrc_rows_have_finite_intervals_including_repeated_starts_and_final_line() {
        let rows = lrc_lines("[00:01.00]one\n[00:01.00]same\n[00:03.00]two", 5000);
        assert_eq!(
            rows.iter()
                .map(|line| line["endMs"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![3000, 3000, 5000]
        );
        assert_eq!(lrc_lines("[00:01.00]last", 0)[0]["endMs"], 6000);
    }
}
