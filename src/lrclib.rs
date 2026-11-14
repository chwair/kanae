use serde::Deserialize;

#[derive(Clone, Default)]
pub struct LyricLine {
    pub time_secs: f64,
    pub text: String,
    /// per-word timings, empty for line-synced lyrics. when present, `text` is
    /// exactly the concatenation of the word texts.
    pub words: Vec<LyricWord>,
    /// when the line stops being sung, if the source says so
    pub end_secs: Option<f64>,
    /// set when the line is still sung after a later line starts (a second
    /// voice comes in over it), so it stays current until then
    pub held_until: Option<f64>,
    /// the line starts while an earlier one is still sung: a second voice
    pub second_voice: bool,
}

#[derive(Clone)]
pub struct LyricWord {
    pub time_secs: f64,
    /// when the word is fully sung; always >= time_secs once parsing is done
    pub end_secs: f64,
    /// includes the trailing space, except on the last word of a line
    pub text: String,
}

/// the parsed lyrics of one track.
#[derive(Clone, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// false for plain lyrics, whose lines carry no usable timing
    pub synced: bool,
    /// the track is known to have no vocals; `lines` is then empty
    pub instrumental: bool,
    /// iso 639-1 code, when a lyricsfile states one
    pub language: Option<String>,
}

impl Lyrics {
    fn synced(lines: Vec<LyricLine>) -> Self {
        Self { synced: !lines.is_empty(), lines, ..Default::default() }
    }

    fn instrumental() -> Self {
        Self { instrumental: true, ..Default::default() }
    }

    /// true when there is nothing to show, not even an instrumental notice
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && !self.instrumental
    }

    pub fn word_synced(&self) -> bool {
        self.lines.iter().any(|l| !l.words.is_empty())
    }

    pub fn kind(&self) -> &'static str {
        if self.instrumental { "instrumental" }
        else if self.word_synced() { "word synced" }
        else if self.synced { "line synced" }
        else { "plain" }
    }
}

// cached in place of real lyrics for a track lrclib marks as instrumental
const INSTRUMENTAL_DOC: &str = "version: '1.0'\nmetadata:\n  instrumental: true\nlines: []\n";

// a line's last word with no known end is assumed sung within this long
const MAX_LAST_WORD_SECS: f64 = 1.0;

/// an empty line lasting at least this long is an instrumental break, shown
/// with a progress bar. qml/main.qml mirrors this value.
pub const BREAK_MIN_SECS: f64 = 6.0;

/// length of the break starting at line `i`, when that line is an empty one
/// lasting at least `BREAK_MIN_SECS` before the next line.
pub fn break_len(times: &[f64], texts: &[String], i: usize) -> Option<f64> {
    if !texts.get(i)?.trim().is_empty() { return None; }
    let len = times.get(i + 1)? - times.get(i)?;
    (len >= BREAK_MIN_SECS).then_some(len)
}

#[derive(Deserialize)]
struct SearchResult {
    id: u64,
    #[serde(rename = "trackName")]
    track_name: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    duration: Option<f64>,
    #[serde(rename = "hasWordSync", default)]
    has_word_sync: bool,
    #[serde(rename = "hasSyncedLyrics", default)]
    has_synced_lyrics: bool,
    #[serde(rename = "hasPlainLyrics", default)]
    has_plain_lyrics: bool,
    #[serde(default)]
    instrumental: bool,
}

// v2 search results are summaries without lyrics text, so ranking goes by the
// availability flags and the chosen record is then fetched by id
impl SearchResult {
    fn word_synced(&self) -> bool {
        self.has_word_sync
    }

    fn is_synced(&self) -> bool {
        self.has_synced_lyrics || self.has_word_sync
    }

    fn is_instrumental(&self) -> bool {
        self.instrumental
    }

    fn has_plain(&self) -> bool {
        self.has_plain_lyrics
    }

    /// ranking bonus for richer lyrics. lrclib often has several near identical
    /// uploads of a song; among plausible ones this prefers word over line sync
    /// over plain text, but cannot rescue a poor match.
    fn richness(&self) -> Option<f64> {
        if self.is_synced() {
            Some(if self.word_synced() { 0.4 } else { 0.3 })
        } else if self.is_instrumental() {
            Some(0.0)
        } else if self.has_plain() {
            Some(0.15)
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
struct GetResult {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    lyricsfile: Option<String>,
    #[serde(rename = "instrumental")]
    instrumental: Option<bool>,
}

// lyricsfile (yaml) documents, see lrcget's LYRICSFILE_CONCEPT.md. all times are
// integer milliseconds, read as f64 to be lenient.
#[derive(Deserialize)]
struct Lyricsfile {
    #[serde(default)]
    metadata: LyricsfileMeta,
    #[serde(default)]
    lines: Option<Vec<LyricsfileLine>>,
    #[serde(default)]
    plain: Option<String>,
}

#[derive(Deserialize, Default)]
struct LyricsfileMeta {
    offset_ms: Option<f64>,
    instrumental: Option<bool>,
    language: Option<String>,
}

#[derive(Deserialize)]
struct LyricsfileLine {
    #[serde(default)]
    text: String,
    start_ms: f64,
    end_ms: Option<f64>,
    #[serde(default)]
    words: Vec<LyricsfileWord>,
}

#[derive(Deserialize)]
struct LyricsfileWord {
    #[serde(default)]
    text: String,
    start_ms: f64,
    end_ms: Option<f64>,
}

const USER_AGENT: &str = concat!("Kanae v", env!("CARGO_PKG_VERSION"), " (https://github.com/chwair/kanae)");

/// load a sidecar `.lrc` file that sits next to the audio file (same name,
/// `.lrc` extension, lrc or lyricsfile content). returns None when there is no
/// such file or it has neither timed lines nor an instrumental mark - callers
/// then fall back to the cache/web path.
pub fn load_local_lrc(audio_path: &str) -> Option<Lyrics> {
    if audio_path.is_empty() { return None; }
    let lrc_path = std::path::Path::new(audio_path).with_extension("lrc");
    if !lrc_path.is_file() { return None; }
    let raw = std::fs::read_to_string(&lrc_path).ok()?;
    let lyrics = parse_lyrics(raw.trim_start_matches('\u{feff}'));
    if lyrics.synced || lyrics.instrumental {
        eprintln!("[lrclib] using sidecar lyrics: {} ({}, {} line(s))", lrc_path.display(), lyrics.kind(), lyrics.lines.len());
        Some(lyrics)
    } else {
        eprintln!("[lrclib] sidecar {} has no timestamped lines - ignoring", lrc_path.display());
        None
    }
}

/// fetch lyrics for a known lrclib id.  returns `(raw, parsed)` on success.
pub fn fetch_by_id(id: u64) -> Option<(String, Lyrics)> {
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
    let found = best_lyrics(
        result.lyricsfile.as_deref(),
        result.synced_lyrics.as_deref(),
        result.plain_lyrics.as_deref(),
        result.instrumental.unwrap_or(false),
    );
    match &found {
        Some((_, lyrics)) => eprintln!("[lrclib] {} lyrics from id {}", lyrics.kind(), id),
        None => eprintln!("[lrclib] id {} has no parseable lyrics", id),
    }
    found
}

/// picks the richest lyrics of a record: the lyricsfile when it parses (it may
/// carry word timings, an instrumental mark or plain text), otherwise the
/// legacy fields. returns the raw text to cache alongside the parsed lyrics;
/// `parse_lyrics` tells the formats apart.
fn best_lyrics(
    lyricsfile: Option<&str>,
    synced: Option<&str>,
    plain: Option<&str>,
    instrumental: bool,
) -> Option<(String, Lyrics)> {
    if let Some(doc) = lyricsfile.filter(|s| !s.trim().is_empty()) {
        let lyrics = parse_lyricsfile(doc);
        if !lyrics.is_empty() {
            return Some((doc.to_string(), lyrics));
        }
    }
    if instrumental {
        return Some((INSTRUMENTAL_DOC.to_string(), Lyrics::instrumental()));
    }
    if let Some(lrc) = synced.filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(lrc);
        if !lines.is_empty() {
            return Some((lrc.to_string(), Lyrics::synced(lines)));
        }
    }
    let plain = plain.filter(|s| !s.trim().is_empty())?;
    let lyrics = parse_plain(plain);
    if lyrics.is_empty() { None } else { Some((plain.to_string(), lyrics)) }
}

/// search for lyrics, preferring synced ones; plain or instrumental records are
/// only used when no query finds synced lyrics. returns `(lrclib_id, raw, parsed)`.
pub fn fetch_lyrics(
    track_name: &str,
    artist_name: &str,
    album_name: &str,
    duration_secs: f64,
) -> Option<(u64, String, Lyrics)> {
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

    let mut fallback: Option<(f64, (u64, String, Lyrics))> = None;
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

        let Some((raw, lyrics)) = fetch_by_id(best.id) else {
            continue;
        };
        if lyrics.synced {
            return Some((best.id, raw, lyrics));
        }
        if fallback.as_ref().is_none_or(|(s, _)| score > *s) {
            fallback = Some((score, (best.id, raw, lyrics)));
        }
    }

    if fallback.is_none() {
        eprintln!("[lrclib] no acceptable lyrics for \"{}\" / \"{}\"", track_name, artist_name);
    }
    fallback.map(|(_, found)| found)
}

fn search(query: &str) -> Vec<SearchResult> {
    let url = format!("https://lrclib.net/api/v2/search?{}", query);
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
        .filter_map(|r| {
            let bonus = r.richness()?;
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
            score.map(|s| (r, s + bonus))
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

/// parses a lyricsfile (yaml) document, plain/enhanced lrc text, or failing
/// both, untimed plain lyrics.
pub fn parse_lyrics(raw: &str) -> Lyrics {
    if is_lyricsfile(raw) {
        return parse_lyricsfile(raw);
    }
    let lines = parse_lrc(raw);
    if lines.is_empty() { parse_plain(raw) } else { Lyrics::synced(lines) }
}

/// untimed lyrics, one line per row. lrc id tags like `[ar:...]` are dropped and
/// blank lines kept only between verses.
fn parse_plain(raw: &str) -> Lyrics {
    let mut lines: Vec<LyricLine> = Vec::new();
    for l in raw.lines().map(str::trim) {
        let is_tag = l.starts_with('[') && l.ends_with(']') && l.contains(':');
        if is_tag || (l.is_empty() && lines.last().is_none_or(|p| p.text.is_empty())) {
            continue;
        }
        lines.push(LyricLine { text: l.to_string(), ..Default::default() });
    }
    while lines.last().is_some_and(|l| l.text.is_empty()) {
        lines.pop();
    }
    Lyrics { lines, ..Default::default() }
}

fn is_lyricsfile(raw: &str) -> bool {
    raw.lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty() && !l.starts_with('#') && l != &"---")
        .is_some_and(|l| l.starts_with("version:") || l.starts_with("metadata:") || l.starts_with("lines:"))
}

pub fn parse_lyricsfile(doc: &str) -> Lyrics {
    let file: Lyricsfile = match serde_yaml_ng::from_str(doc) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[lrclib] failed to parse lyricsfile: {}", e);
            return Lyrics::default();
        }
    };
    if file.metadata.instrumental.unwrap_or(false) {
        return Lyrics::instrumental();
    }
    let language = file.metadata.language.filter(|l| !l.trim().is_empty());
    // same sign as lrc's [offset:] tag: a positive offset shows lyrics sooner
    let offset = file.metadata.offset_ms.unwrap_or(0.0);
    let secs = |ms: f64| ((ms - offset) / 1000.0).max(0.0);

    let mut lines: Vec<LyricLine> = file
        .lines
        .unwrap_or_default()
        .into_iter()
        .map(|l| {
            let line_end = l.end_ms.map(secs).unwrap_or(f64::NAN);
            let raw: Vec<LyricsfileWord> = l.words.into_iter().filter(|w| !w.text.is_empty()).collect();
            // a word without an end lasts until the next one starts, or the line ends
            let mut words: Vec<LyricWord> = raw
                .iter()
                .enumerate()
                .map(|(i, w)| LyricWord {
                    time_secs: secs(w.start_ms),
                    end_secs: w.end_ms.map(secs)
                        .or_else(|| raw.get(i + 1).map(|n| secs(n.start_ms)))
                        .unwrap_or(line_end),
                    text: w.text.clone(),
                })
                .collect();
            let text = if words.is_empty() {
                l.text.trim().to_string()
            } else {
                normalize_words(&mut words)
            };
            LyricLine { time_secs: secs(l.start_ms), text, words, end_secs: l.end_ms.map(secs), ..Default::default() }
        })
        .collect();

    if lines.is_empty() {
        // unsynced document: fall back to its plain text
        let mut plain = parse_plain(file.plain.as_deref().unwrap_or(""));
        plain.language = language;
        return plain;
    }
    sort_and_finish(&mut lines);
    Lyrics { lines, synced: true, instrumental: false, language }
}

/// orders lines by time, settles word ends left open by the parsers and turns
/// long silences into explicit empty lines, so they can show as breaks.
fn sort_and_finish(lines: &mut Vec<LyricLine>) {
    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap_or(std::cmp::Ordering::Equal));
    for i in 0..lines.len() {
        let next_line = lines.get(i + 1).map(|l| l.time_secs);
        let line = &mut lines[i];
        for w in line.words.iter_mut() {
            if !w.end_secs.is_finite() {
                let cap = w.time_secs + MAX_LAST_WORD_SECS;
                w.end_secs = next_line.map_or(cap, |n| n.min(cap));
            }
            w.end_secs = w.end_secs.max(w.time_secs);
        }
        if line.end_secs.is_none() {
            line.end_secs = line.words.last().map(|w| w.end_secs);
        }
    }
    insert_breaks(lines);
    mark_voices(lines);
}

// shorter overlaps are timing slop between consecutive lines, not two voices
const MIN_OVERLAP_SECS: f64 = 0.3;

/// finds overlapping lines, where a second voice sings over the first. only
/// known line ends count, so plain lrc (which has none) never overlaps.
fn mark_voices(lines: &mut [LyricLine]) {
    let sung = |l: &LyricLine| !l.text.trim().is_empty();
    let mut busy_until = f64::NEG_INFINITY;
    for i in 0..lines.len() {
        let next = lines.get(i + 1).map(|l| l.time_secs);
        let line = &mut lines[i];
        if !sung(line) { continue; }
        line.second_voice = line.time_secs + MIN_OVERLAP_SECS < busy_until;
        line.held_until = line.end_secs.filter(|&end| next.is_some_and(|n| end > n + MIN_OVERLAP_SECS));
        if let Some(end) = line.end_secs {
            busy_until = busy_until.max(end);
        }
    }
}

/// whether line `i` is being sung at time `t`, given the index of the last line
/// that started (`latest`). an earlier line stays current while it is held.
pub fn line_active(lines: &[LyricLine], i: usize, latest: i32, t: f64) -> bool {
    if latest < 0 || i > latest as usize { return false; }
    i == latest as usize || lines[i].held_until.is_some_and(|h| t < h)
}

fn break_line(at: f64) -> LyricLine {
    LyricLine { time_secs: at, ..Default::default() }
}

/// adds an empty line where a sung line ends long before the next one starts,
/// and before a late first line (the intro). lines without a known end are
/// left alone, since their silence can't be told from a long held note.
fn insert_breaks(lines: &mut Vec<LyricLine>) {
    let sung = |l: &LyricLine| !l.text.trim().is_empty();
    let mut out: Vec<LyricLine> = Vec::with_capacity(lines.len());
    if lines.first().is_some_and(|l| sung(l) && l.time_secs >= BREAK_MIN_SECS) {
        out.push(break_line(0.0));
    }
    // with overlapping voices the silence starts when the last of them ends
    let mut sung_until: Option<f64> = None;
    for i in 0..lines.len() {
        let line = lines[i].clone();
        if let Some(end) = line.end_secs.filter(|_| sung(&line)) {
            sung_until = Some(sung_until.map_or(end, |u| u.max(end)));
        }
        let gap_from = sung_until.filter(|_| sung(&line) && line.end_secs.is_some());
        out.push(line);
        if let (Some(end), Some(next)) = (gap_from, lines.get(i + 1)) {
            if sung(next) && next.time_secs - end >= BREAK_MIN_SECS {
                out.push(break_line(end));
            }
        }
    }
    *lines = out;
}

/// trims the outer whitespace of a word run and returns the line text, which
/// then equals the concatenated words so renderers can count sung characters.
fn normalize_words(words: &mut Vec<LyricWord>) -> String {
    words.retain(|w| !w.text.is_empty());
    // leading spaces belong to the previous word, so a word starts where it is sung
    for i in 1..words.len() {
        if words[i].text.starts_with(char::is_whitespace) {
            words[i].text = words[i].text.trim_start().to_string();
            if !words[i - 1].text.ends_with(char::is_whitespace) {
                words[i - 1].text.push(' ');
            }
        }
    }
    words.retain(|w| !w.text.is_empty());
    if let Some(first) = words.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = words.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    words.retain(|w| !w.text.is_empty());
    words.iter().map(|w| w.text.as_str()).collect()
}

/// splits enhanced lrc word tags (`<mm:ss.xx>word <mm:ss.xx>word`) out of the
/// text after a line's timestamps. text before the first tag stays untimed and
/// joins the first word; a trailing bare tag only marks the end of the line.
fn parse_enhanced_words(rest: &str) -> (String, Vec<LyricWord>) {
    let mut words: Vec<LyricWord> = Vec::new();
    let mut lead = String::new();
    let mut s = rest;
    while !s.is_empty() {
        let tag = s.starts_with('<').then(|| s.find('>')).flatten()
            .and_then(|end| parse_timestamp(&s[1..end]).map(|t| (t, end)));
        if let Some((t, end)) = tag {
            s = &s[end + 1..];
            let next = s.find('<').unwrap_or(s.len());
            // the previous word ends where this tag starts, even an empty end marker
            if let Some(prev) = words.last_mut() {
                prev.end_secs = t;
            }
            words.push(LyricWord { time_secs: t, end_secs: f64::NAN, text: s[..next].to_string() });
            s = &s[next..];
            continue;
        }
        // not a tag: copy up to the next '<' as plain text
        let next = s[1..].find('<').map(|i| i + 1).unwrap_or(s.len());
        match words.last_mut() {
            Some(w) => w.text.push_str(&s[..next]),
            None => lead.push_str(&s[..next]),
        }
        s = &s[next..];
    }
    if words.is_empty() {
        return (lead.trim().to_string(), words);
    }
    if !lead.trim().is_empty() {
        words[0].text.insert_str(0, &lead);
    }
    let text = normalize_words(&mut words);
    (text, words)
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

        let (text, words) = parse_enhanced_words(&raw_line[pos..]);

        // enhanced word tags are absolute, so they only fit a line with a single time
        let words = if times.len() == 1 { words } else { Vec::new() };
        for t in times {
            lines.push(LyricLine { time_secs: t, text: text.clone(), words: words.clone(), ..Default::default() });
        }
    }

    sort_and_finish(&mut lines);
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
    fn parses_lyricsfile_words() {
        let doc = "version: '1.0'\nmetadata:\n  title: 'x'\n  artist: 'y'\n  offset_ms: 0\nlines:\n  - text: 'The club'\n    start_ms: 12450\n    end_ms: 13500\n    words:\n      - text: 'The '\n        start_ms: 12450\n      - text: 'club'\n        start_ms: 12900\n  - text: 'plain line'\n    start_ms: 15000\n";
        assert!(is_lyricsfile(doc));
        let lyrics = parse_lyrics(doc);
        assert!(lyrics.synced && !lyrics.instrumental);
        // the first line comes 12 s in, so an intro break leads
        assert_eq!(lyrics.lines[0].text, "");
        let lines = &lyrics.lines[1..];
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "The club");
        assert_eq!(lines[0].words.len(), 2);
        assert!((lines[0].words[1].time_secs - 12.9).abs() < 1e-9);
        // the last word without an end takes the line's end
        assert!((lines[0].words[0].end_secs - 12.9).abs() < 1e-9);
        assert!((lines[0].words[1].end_secs - 13.5).abs() < 1e-9);
        assert!(lines[1].words.is_empty());
        assert_eq!(lines[1].text, "plain line");
    }

    #[test]
    fn parses_enhanced_lrc_words() {
        let lines = parse_lrc("[00:12.00] <00:12.00> The <00:12.50> club <00:13.00>\n[00:14.00]plain");
        assert_eq!(lines[0].text, "");
        let lines = &lines[1..];
        assert_eq!(lines[0].text, "The club");
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[0].words[0].text, "The ");
        assert!((lines[0].words[1].time_secs - 12.5).abs() < 1e-9);
        // a trailing bare tag closes the last word
        assert!((lines[0].words[1].end_secs - 13.0).abs() < 1e-9);
        assert!(lines[1].words.is_empty());
        assert_eq!(lines[1].text, "plain");
    }

    fn result(id: u64, artist: &str, word_sync: bool) -> SearchResult {
        SearchResult {
            id,
            track_name: Some("Testify".into()),
            artist_name: Some(artist.into()),
            duration: Some(210.0),
            has_word_sync: word_sync,
            has_synced_lyrics: true,
            has_plain_lyrics: true,
            instrumental: false,
        }
    }

    #[test]
    fn prefers_word_synced_among_equals() {
        let results = vec![
            result(2, "void (Mournfinale) feat. Hoshikuma Minami", true),
            result(1, "void (Mournfinale) feat. \u{661f}\u{718a}\u{5357}\u{5deb}", false),
        ];
        let (best, _) = pick_best(&results, "Testify (feat. \u{661f}\u{718a}\u{5357}\u{5deb})", "void (Mournfinale)", 210.0).unwrap();
        assert_eq!(best.id, 2);
    }

    #[test]
    fn parses_instrumental_and_plain_lyricsfiles() {
        let inst = parse_lyrics("version: '1.0'\nmetadata:\n  title: x\n  artist: y\n  instrumental: true\nlines: []\nplain: ''\n");
        assert!(inst.instrumental && inst.lines.is_empty() && !inst.is_empty());
        assert!(parse_lyrics(INSTRUMENTAL_DOC).instrumental);

        let plain = parse_lyrics("version: '1.0'\nmetadata:\n  title: x\n  artist: y\n  language: zh\nlines: []\nplain: |\n  one\n\n  two\n");
        assert!(!plain.synced && !plain.instrumental);
        assert_eq!(plain.language.as_deref(), Some("zh"));
        let texts: Vec<&str> = plain.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["one", "", "two"]);
    }

    #[test]
    fn plain_text_cache_entries_parse_as_plain() {
        let lyrics = parse_lyrics("[ar:someone]\nfirst line\nsecond line\n");
        assert!(!lyrics.synced);
        assert_eq!(lyrics.lines.len(), 2);
    }

    #[test]
    fn inserts_breaks_on_long_silences() {
        let doc = "version: '1.0'\nmetadata:\n  title: x\n  artist: y\nlines:\n- text: a\n  start_ms: 8000\n  end_ms: 9000\n- text: b\n  start_ms: 20000\n  end_ms: 21000\n- text: c\n  start_ms: 23000\n";
        let lyrics = parse_lyrics(doc);
        let got: Vec<(f64, &str)> = lyrics.lines.iter().map(|l| (l.time_secs, l.text.as_str())).collect();
        assert_eq!(got, [(0.0, ""), (8.0, "a"), (9.0, ""), (20.0, "b"), (23.0, "c")]);
        let times: Vec<f64> = lyrics.lines.iter().map(|l| l.time_secs).collect();
        let texts: Vec<String> = lyrics.lines.iter().map(|l| l.text.clone()).collect();
        assert_eq!(break_len(&times, &texts, 0), Some(8.0));
        assert_eq!(break_len(&times, &texts, 2), Some(11.0));
        assert_eq!(break_len(&times, &texts, 1), None);
    }

    #[test]
    fn keeps_first_voice_current_under_second() {
        // trimmed from "Used to be": a backing line sung over the end of a lead line
        let doc = "version: '1.0'\nmetadata:\n  title: x\n  artist: y\nlines:\n- text: How could you leave?\n  start_ms: 141827\n  end_ms: 148375\n- text: (Now I can see)\n  start_ms: 145101\n  end_ms: 148366\n- text: You used to be me\n  start_ms: 148666\n  end_ms: 154169\n";
        let lines = parse_lyrics(doc).lines;
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["", "How could you leave?", "(Now I can see)", "You used to be me"]);
        assert!((lines[1].held_until.unwrap() - 148.375).abs() < 1e-9);
        assert!(!lines[1].second_voice && lines[2].second_voice && !lines[3].second_voice);
        assert!(lines[2].held_until.is_none());
        // both voices are current while they overlap, then only the latest
        assert!(line_active(&lines, 1, 2, 146.0) && line_active(&lines, 2, 2, 146.0));
        assert!(!line_active(&lines, 1, 2, 148.5));
        assert!(!line_active(&lines, 1, 3, 149.0));
    }

    #[test]
    fn ignores_tiny_overlaps() {
        let doc = "version: '1.0'\nmetadata:\n  title: x\n  artist: y\nlines:\n- text: a\n  start_ms: 1000\n  end_ms: 2100\n- text: b\n  start_ms: 2000\n  end_ms: 3000\n";
        let lines = parse_lyrics(doc).lines;
        assert!(lines.iter().all(|l| l.held_until.is_none() && !l.second_voice));
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
