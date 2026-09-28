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

/// Load a sidecar `.lrc` file that sits next to the audio file (same name,
/// `.lrc` extension). Returns parsed lines, or None when there is no such file
/// or it contains no timestamped lines — callers then fall back to the
/// cache/web path.
pub fn load_local_lrc(audio_path: &str) -> Option<Vec<LyricLine>> {
    if audio_path.is_empty() { return None; }
    let lrc_path = std::path::Path::new(audio_path).with_extension("lrc");
    if !lrc_path.is_file() { return None; }
    let raw = std::fs::read_to_string(&lrc_path).ok()?;
    let lines = parse_lrc(raw.trim_start_matches('\u{feff}'));
    if lines.is_empty() {
        eprintln!("[lrclib] sidecar {} has no timestamped lines — ignoring", lrc_path.display());
        None
    } else {
        eprintln!("[lrclib] using sidecar lyrics: {} ({} line(s))", lrc_path.display(), lines.len());
        Some(lines)
    }
}

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

    // each query is only tried when the previous ones produced no acceptable match
    let mut queries = vec![format!(
        "track_name={}&artist_name={}",
        url_encode(track_name),
        url_encode(artist_name),
    )];
    if !album_name.is_empty() {
        queries.push(format!("track_name={}&album_name={}", url_encode(track_name), url_encode(album_name)));
    }
    if !artist_name.is_empty() {
        queries.push(format!("q={}", url_encode(&format!("{} {}", track_name, artist_name))));
    }

    for query in &queries {
        let results = search(query);
        let Some((best, score)) = pick_best(&results, track_name, artist_name, duration_secs) else {
            continue;
        };

        eprintln!(
            "[lrclib] best match: \"{}\" by \"{}\" (duration: {:?}s, score: {:.2})",
            best.track_name.as_deref().unwrap_or("?"),
            best.artist_name.as_deref().unwrap_or("?"),
            best.duration,
            score,
        );

        let Some(lrc) = best.synced_lyrics.as_deref() else { continue };
        let lines = parse_lrc(lrc);
        if lines.is_empty() {
            eprintln!("[lrclib] matched entry has no parseable LRC lines");
            continue;
        }
        eprintln!("[lrclib] {} lyric line(s) parsed (id {})", lines.len(), best.id);
        return Some((best.id, lrc.to_string(), lines));
    }

    eprintln!("[lrclib] no acceptable synced lyrics for \"{}\" / \"{}\"", track_name, artist_name);
    None
}

fn search(query: &str) -> Vec<SearchResult> {
    let url = format!("https://lrclib.net/api/search?{}", query);
    eprintln!("[lrclib] GET {}", url);
    let results: Vec<SearchResult> = match ureq::get(&url).header("User-Agent", USER_AGENT).call() {
        Ok(r) => match r.into_body().read_json() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[lrclib] failed to parse response: {}", e);
                vec![]
            }
        },
        Err(e) => {
            eprintln!("[lrclib] request failed: {}", e);
            vec![]
        }
    };
    eprintln!("[lrclib] {} result(s) returned", results.len());
    results
}

fn pick_best<'a>(
    results: &'a [SearchResult],
    track_name: &str,
    artist_name: &str,
    duration: f64,
) -> Option<(&'a SearchResult, f64)> {
    results
        .iter()
        .filter(|r| {
            !r.instrumental.unwrap_or(false)
                && r.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
        })
        .filter_map(|r| {
            let score = score_result(
                r.track_name.as_deref(),
                r.artist_name.as_deref(),
                r.duration,
                track_name,
                artist_name,
                duration,
            );
            if score.is_none() {
                eprintln!(
                    "[lrclib] rejected \"{}\" by \"{}\" (duration: {:?}s)",
                    r.track_name.as_deref().unwrap_or("?"),
                    r.artist_name.as_deref().unwrap_or("?"),
                    r.duration,
                );
            }
            score.map(|s| (r, s))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

// a different length almost always means a different song or edit, and even the
// right song would have its timestamps out of sync
const MAX_DURATION_DIFF: f64 = 10.0;
const MIN_TITLE_SCORE: f64 = 0.6;
const MIN_ARTIST_SCORE: f64 = 0.5;

/// returns None when the candidate is not plausibly the same song, otherwise a
/// score used to rank the plausible ones
fn score_result(
    cand_track: Option<&str>,
    cand_artist: Option<&str>,
    cand_duration: Option<f64>,
    track_name: &str,
    artist_name: &str,
    duration: f64,
) -> Option<f64> {
    let dur_diff = match cand_duration {
        Some(d) if duration > 0.0 && d > 0.0 => Some((d - duration).abs()),
        _ => None,
    };
    if dur_diff.is_some_and(|d| d > MAX_DURATION_DIFF) {
        return None;
    }

    let title_score = title_similarity(cand_track?, track_name);
    if title_score < MIN_TITLE_SCORE {
        return None;
    }

    let artist_score = if artist_name.is_empty() {
        0.5
    } else {
        let cand_artist = cand_artist.unwrap_or("");
        let score = artist_similarity(cand_artist, artist_name);
        if score < MIN_ARTIST_SCORE {
            // names in different scripts (e.g. romaji vs kana) can't be compared, so
            // only there a near exact title and duration may stand in for the artist
            let comparable = has_non_latin(cand_artist) == has_non_latin(artist_name);
            if comparable || title_score < 0.9 || !dur_diff.is_some_and(|d| d <= 2.0) {
                return None;
            }
        }
        score
    };

    let dur_score = match dur_diff {
        Some(diff) if diff <= 2.0 => 1.0,
        Some(diff) => 1.0 - (diff - 2.0) / (MAX_DURATION_DIFF - 2.0),
        None => 0.5,
    };

    Some(title_score * 0.45 + artist_score * 0.35 + dur_score * 0.2)
}

/// lowercases, drops punctuation and collapses whitespace
fn normalize(s: &str) -> String {
    let cleaned: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// removes bracketed parts like "(feat. x)" or "[remastered]"
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '\u{3010}' | '\u{ff08}' => depth += 1,
            ')' | ']' | '}' | '\u{3011}' | '\u{ff09}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn title_similarity(a: &str, b: &str) -> f64 {
    let full = similarity(&normalize(a), &normalize(b));
    let bare = similarity(&normalize(&strip_brackets(a)), &normalize(&strip_brackets(b)));
    full.max(bare)
}

/// compares the best pair of individual artists, so "a, b" still matches "b"
fn artist_similarity(a: &str, b: &str) -> f64 {
    let full = similarity(&normalize(a), &normalize(b));
    let ta = split_artists(a);
    let tb = split_artists(b);
    ta.iter()
        .flat_map(|x| tb.iter().map(move |y| similarity(x, y)))
        .fold(full, f64::max)
}

fn split_artists(s: &str) -> Vec<String> {
    let mut s = format!(" {} ", s.to_lowercase());
    for sep in [" feat. ", " feat ", " ft. ", " ft ", " featuring ", " and ", " with ", " x ", " vs. ", " vs "] {
        s = s.replace(sep, ",");
    }
    s.split(|c: char| matches!(c, ',' | '&' | '/' | ';' | '+' | '\u{3001}' | '\u{00d7}'))
        .map(normalize)
        .filter(|t| !t.is_empty())
        .collect()
}

fn has_non_latin(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic() && (c as u32) >= 0x250)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn score(track: &str, artist: &str, dur: f64) -> Option<f64> {
        score_result(Some(track), Some(artist), Some(dur), "Enigma", "sanmal", 168.0)
    }

    #[test]
    fn rejects_same_title_different_artist() {
        assert!(score("Enigma", "AMI, Tata Vlad", 168.0).is_none());
    }

    #[test]
    fn rejects_far_duration() {
        assert!(score("Enigma", "sanmal", 200.0).is_none());
    }

    #[test]
    fn accepts_collab_and_feat_suffix() {
        assert!(score("Enigma (feat. someone)", "someone & sanmal", 169.0).is_some());
    }

    #[test]
    fn accepts_other_script_only_on_tight_match() {
        assert!(score("Enigma", "\u{3055}\u{3093}\u{307e}\u{308b}", 168.5).is_some());
        assert!(score("Enigma", "\u{3055}\u{3093}\u{307e}\u{308b}", 175.0).is_none());
    }
}
