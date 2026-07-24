use base64::{engine::general_purpose, Engine as _};
use cd_da_reader::Toc;
use serde::Deserialize;
use sha1::{Digest, Sha1};

pub fn calculate_disc_id(toc: &Toc) -> String {
    let s = format_toc_string(toc);
    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let hash = hasher.finalize();
    general_purpose::STANDARD
        .encode(hash)
        .replace('+', ".")
        .replace('/', "_")
        .replace('=', "-")
}

fn format_toc_string(toc: &Toc) -> String {
    let mut s = String::new();
    s.push_str(&format!("{:02X}", toc.first_track));
    s.push_str(&format!("{:02X}", toc.last_track));
    s.push_str(&format!("{:08X}", toc.leadout_lba + 150));
    for track_num in 1u8..=99u8 {
        match toc.tracks.iter().find(|t| t.number == track_num) {
            Some(track) => s.push_str(&format!("{:08X}", track.start_lba + 150)),
            None => s.push_str("00000000"),
        }
    }
    s
}

#[derive(Debug, Deserialize)]
struct MbResponse {
    releases: Option<Vec<MbRelease>>,
}

#[derive(Debug, Deserialize)]
struct MbRelease {
    id: String,
    title: String,
    date: Option<String>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCredit>>,
    #[serde(rename = "cover-art-archive")]
    cover_art_archive: Option<MbCoverArtArchive>,
    media: Option<Vec<MbMedium>>,
}

#[derive(Debug, Deserialize)]
struct MbCoverArtArchive {
    front: bool,
}

#[derive(Debug, Deserialize)]
struct MbArtistCredit {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbMedium {
    format: Option<String>,
    position: Option<u32>,
    discs: Option<Vec<MbDisc>>,
    tracks: Option<Vec<MbTrack>>,
}

#[derive(Debug, Deserialize)]
struct MbDisc {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbTrack {
    title: Option<String>,
    recording: Option<MbRecording>,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCredit>>,
}

#[derive(Debug, Clone)]
pub struct AlbumMetadata {
    pub title: String,
    pub artist: String,
    pub year: String,
    pub track_titles: Vec<String>,
    pub track_artists: Vec<String>,
    pub cover_art_url: Option<String>,
    /// Position of this disc within the release (1-based), e.g. disc 2 of a
    /// 3-CD box set. 0 when unknown.
    pub disc_number: u32,
    /// Total number of media in the release. 0 when unknown.
    pub disc_count: u32,
}

const USER_AGENT: &str = "kanae-player/0.1.0 (https://github.com/user/kanae)";

fn retry<T, F: Fn() -> Option<T>>(attempts: u32, f: F) -> Option<T> {
    for i in 0..attempts {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(800 * (1 << (i - 1))));
        }
        if let Some(result) = f() {
            return Some(result);
        }
    }
    None
}

pub fn lookup_metadata(toc: &Toc) -> Option<AlbumMetadata> {
    let disc_id = calculate_disc_id(toc);
    let url = format!(
        "https://musicbrainz.org/ws/2/discid/{}?inc=recordings+artist-credits&fmt=json",
        disc_id
    );
    eprintln!("[mb] looking up disc ID {}", disc_id);

    let response: MbResponse = retry(3, || {
        match ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .call()
        {
            Ok(mut resp) => match resp.body_mut().read_json::<MbResponse>() {
                Ok(r)  => Some(r),
                Err(e) => { eprintln!("[mb] JSON parse error: {}", e); None }
            },
            Err(e) => { eprintln!("[mb] request error: {}", e); None }
        }
    })?;

    let releases = response.releases?;
    eprintln!("[mb] {} release(s) found", releases.len());
    let release = releases.into_iter().next()?;

    let title = release.title.clone();
    let year = release
        .date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .unwrap_or("")
        .to_string();
    let artist = parse_artist_credit(release.artist_credit.as_deref());

    // Pick the medium that actually contains our disc ID — on multi-CD
    // releases the first medium would otherwise give the wrong track list.
    // Fall back to a "CD"-format medium, then the first medium.
    let media = release.media.as_deref()?;
    let cd_medium = media
        .iter()
        .find(|m| {
            m.discs
                .as_deref()
                .is_some_and(|ds| ds.iter().any(|d| d.id.as_deref() == Some(disc_id.as_str())))
        })
        .or_else(|| media.iter().find(|m| m.format.as_deref() == Some("CD")))
        .or_else(|| media.first())?;

    let disc_number = cd_medium.position.unwrap_or(0);
    let disc_count  = media.len() as u32;

    let (track_titles, track_artists) =
        parse_tracks(cd_medium.tracks.as_deref(), &artist);

    let cover_art_url = if release.cover_art_archive.as_ref().map(|c| c.front).unwrap_or(false) {
        eprintln!("[mb] fetching cover art for {}", release.id);
        let result = fetch_cover_art(&release.id);
        if result.is_none() { eprintln!("[mb] cover art fetch failed"); }
        result
    } else {
        eprintln!("[mb] no front cover in CAA");
        None
    };

    Some(AlbumMetadata {
        title,
        artist,
        year,
        track_titles,
        track_artists,
        cover_art_url,
        disc_number,
        disc_count,
    })
}

fn parse_artist_credit(credits: Option<&[MbArtistCredit]>) -> String {
    match credits {
        None | Some([]) => "Unknown Artist".to_string(),
        Some(ac) => ac.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
    }
}

fn parse_tracks(
    tracks: Option<&[MbTrack]>,
    album_artist: &str,
) -> (Vec<String>, Vec<String>) {
    let Some(tracks) = tracks else {
        return (Vec::new(), Vec::new());
    };
    let mut titles = Vec::with_capacity(tracks.len());
    let mut artists = Vec::with_capacity(tracks.len());
    for t in tracks {
        titles.push(
            t.title
                .clone()
                .unwrap_or_else(|| "Unknown Track".to_string()),
        );
        let ta = t
            .recording
            .as_ref()
            .and_then(|r| r.artist_credit.as_deref())
            .map(|ac| parse_artist_credit(Some(ac)))
            .unwrap_or_default();
        artists.push(if ta == album_artist { String::new() } else { ta });
    }
    (titles, artists)
}

fn fetch_cover_art(release_id: &str) -> Option<String> {
    let url = format!(
        "https://coverartarchive.org/release/{}/front",
        release_id
    );
    retry(3, || {
        match ureq::get(&url).header("User-Agent", USER_AGENT).call() {
            Ok(mut resp) => {
                match resp.body_mut().read_to_vec() {
                    Ok(bytes) if !bytes.is_empty() => {
                        let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
                        // Per-release filename: a shared one makes every disc's cover
                        // look like the same source to consumers that key off the path
                        // (Discord's upload cache, Qt's image cache).
                        let path = std::env::temp_dir()
                            .join(format!("kanae_cover_{}.{}", release_id, ext));
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => {
                                let url_path = path.to_string_lossy().replace('\\', "/");
                                Some(format!("file:///{}", url_path))
                            }
                            Err(e) => { eprintln!("[mb] cover art write error: {}", e); None }
                        }
                    }
                    Ok(_) => { eprintln!("[mb] cover art empty response"); None }
                    Err(e) => { eprintln!("[mb] cover art read error: {}", e); None }
                }
            }
            Err(e) => { eprintln!("[mb] cover art request error: {}", e); None }
        }
    })
}
