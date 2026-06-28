use serde::Deserialize;

#[derive(Clone)]
pub struct LyricLine {
    pub time_secs: f64,
    pub text: String,
}

#[derive(Deserialize)]
struct SearchResult {
    id: u64,
    #[serde(rename = "trackName")]
    track_name: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    duration: Option<f64>,
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "instrumental")]
    instrumental: Option<bool>,
}

#[derive(Deserialize)]
struct GetResult {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "instrumental")]
    instrumental: Option<bool>,
}

const USER_AGENT: &str = concat!("Kanae v", env!("CARGO_PKG_VERSION"), " (https://github.com/chwair/kanae)");

/// Fetch lyrics for a known lrclib ID.  Returns `(raw_lrc, parsed_lines)` on success.
pub fn fetch_by_id(id: u64) -> Option<(String, Vec<LyricLine>)> {
    let url = format!("https://lrclib.net/api/get/{}", id);
    eprintln!("[lrclib] GET {} (by id)", url);
    let response = match ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lrclib] fetch_by_id request failed: {}", e);
            return None;
        }
    };
    let result: GetResult = match response.into_body().read_json() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lrclib] fetch_by_id parse failed: {}", e);
            return None;
        }
    };
    if result.instrumental.unwrap_or(false) {
        eprintln!("[lrclib] id {} is instrumental — no lyrics", id);
        return None;
    }
    let lrc = result.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty())?;
    let lines = parse_lrc(lrc);
    if lines.is_empty() {
        eprintln!("[lrclib] id {} has no parseable LRC lines", id);
        None
    } else {
        eprintln!("[lrclib] {} lyric line(s) from id {}", lines.len(), id);
        Some((lrc.to_string(), lines))
    }
}

/// Search for synced lyrics.  Returns `(lrclib_id, raw_lrc, parsed_lines)` on success.
pub fn fetch_synced_lyrics(
    track_name: &str,
    artist_name: &str,
    album_name: &str,
    duration_secs: f64,
) -> Option<(u64, String, Vec<LyricLine>)> {
    if track_name.is_empty() {
        eprintln!("[lrclib] skipping fetch: track name is empty");
        return None;
    }

    let url = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        url_encode(track_name),
        url_encode(artist_name),
    );

    eprintln!("[lrclib] GET {}", url);

    let response = match ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lrclib] request failed: {}", e);
            return None;
        }
    };

    let results: Vec<SearchResult> = match response.into_body().read_json() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lrclib] failed to parse response: {}", e);
            return None;
        }
    };

    eprintln!("[lrclib] {} result(s) returned", results.len());

    let mut results = results;

    // Fallback: if the structured search returned nothing, retry with a more precise query.
    if results.is_empty() {
        if !album_name.is_empty() {
            let fallback_url = format!(
                "https://lrclib.net/api/search?track_name={}&album_name={}",
                url_encode(track_name),
                url_encode(album_name),
            );
            eprintln!("[lrclib] no results — retrying with album search: GET {}", fallback_url);
            results = match ureq::get(&fallback_url)
                .header("User-Agent", USER_AGENT)
                .call()
            {
                Ok(r) => r.into_body().read_json().unwrap_or_default(),
                Err(e) => { eprintln!("[lrclib] album search fallback request failed: {}", e); vec![] }
            };
            eprintln!("[lrclib] album search fallback: {} result(s) returned", results.len());
        } else if !artist_name.is_empty() {
            // No album name available — fall back to a free-text query.
            let q = format!("{} {}", track_name, artist_name);
            let fallback_url = format!(
                "https://lrclib.net/api/search?q={}",
                url_encode(&q),
            );
            eprintln!("[lrclib] no results — retrying with fallback query: GET {}", fallback_url);
            results = match ureq::get(&fallback_url)
                .header("User-Agent", USER_AGENT)
                .call()
            {
                Ok(r) => r.into_body().read_json().unwrap_or_default(),
                Err(e) => { eprintln!("[lrclib] fallback request failed: {}", e); vec![] }
            };
            eprintln!("[lrclib] fallback: {} result(s) returned", results.len());
        }
    }

    let candidates: Vec<&SearchResult> = results
        .iter()
        .filter(|r| {
            !r.instrumental.unwrap_or(false)
                && r.synced_lyrics
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
        })
        .collect();

    eprintln!("[lrclib] {} candidate(s) with synced lyrics", candidates.len());

    if candidates.is_empty() {
        eprintln!("[lrclib] no synced lyrics found for \"{}\" / \"{}\"", track_name, artist_name);
        return None;
    }

    // Similarity is only used to choose between multiple candidates — a single
    // result is always accepted regardless of how well it matches.
    let best = if candidates.len() == 1 {
        candidates[0]
    } else {
        candidates.iter().max_by(|a, b| {
            let sa = score_result(a, track_name, artist_name, duration_secs);
            let sb = score_result(b, track_name, artist_name, duration_secs);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        })?
    };

    eprintln!(
        "[lrclib] {} match: \"{}\" by \"{}\" (duration: {:?}s)",
        if candidates.len() == 1 { "only" } else { "best" },
        best.track_name.as_deref().unwrap_or("?"),
        best.artist_name.as_deref().unwrap_or("?"),
        best.duration,
    );

    let lrc = best.synced_lyrics.as_deref()?;
    let lines = parse_lrc(lrc);
    if lines.is_empty() {
        eprintln!("[lrclib] matched entry has no parseable LRC lines");
        None
    } else {
        eprintln!("[lrclib] {} lyric line(s) parsed (id {})", lines.len(), best.id);
        Some((best.id, lrc.to_string(), lines))
    }
}

fn score_result(r: &SearchResult, track_name: &str, artist_name: &str, duration: f64) -> f64 {
    let title_score = r
        .track_name
        .as_deref()
        .map(|t| similarity(&t.to_lowercase(), &track_name.to_lowercase()))
        .unwrap_or(0.0);

    let artist_score = if artist_name.is_empty() {
        0.5
    } else {
        r.artist_name
            .as_deref()
            .map(|a| similarity(&a.to_lowercase(), &artist_name.to_lowercase()))
            .unwrap_or(0.0)
    };

    let dur_score = r
        .duration
        .map(|d| {
            let diff = (d - duration).abs();
            if diff <= 2.0 {
                1.0_f64
            } else {
                (1.0_f64 - (diff - 2.0) / 28.0).max(0.0)
            }
        })
        .unwrap_or(0.5);

    title_score * 0.5 + artist_score * 0.3 + dur_score * 0.2
}

fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let dist = levenshtein(a, b);
    let max_len = a.chars().count().max(b.chars().count());
    1.0 - dist as f64 / max_len as f64
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            curr[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j - 1].min(prev[j]).min(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push('+'),
            c => {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf).as_bytes();
                for b in bytes {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines: Vec<LyricLine> = Vec::new();

    for raw_line in lrc.lines() {
        let raw_line = raw_line.trim();
        if raw_line.is_empty() {
            continue;
        }

        let mut pos = 0;
        let mut times: Vec<f64> = Vec::new();

        while pos < raw_line.len() && raw_line[pos..].starts_with('[') {
            match raw_line[pos..].find(']') {
                Some(end) => {
                    let tag = &raw_line[pos + 1..pos + end];
                    if let Some(secs) = parse_timestamp(tag) {
                        times.push(secs);
                        pos += end + 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }

        if times.is_empty() {
            continue;
        }

        let text = raw_line[pos..].trim().to_string();

        for t in times {
            lines.push(LyricLine {
                time_secs: t,
                text: text.clone(),
            });
        }
    }

    lines.sort_by(|a, b| {
        a.time_secs
            .partial_cmp(&b.time_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    lines
}

fn parse_timestamp(s: &str) -> Option<f64> {
    let colon = s.find(':')?
;    let mins: f64 = s[..colon].trim().parse().ok()?;
    let rest = s[colon + 1..].trim().replace(':', ".");
    let secs: f64 = rest.parse().ok()?;
    if mins < 0.0 || secs < 0.0 {
        return None;
    }
    Some(mins * 60.0 + secs)
}
