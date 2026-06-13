use cd_da_reader::{CdReader, Toc, CdReaderError};
use std::io;

use crate::musicbrainz::AlbumMetadata;

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub path: String,
    pub has_audio_cd: bool,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub track_number: u8,
    pub duration_seconds: f64,
}

/// Result produced by the background disc-load thread and consumed by poll_load.
pub enum PendingDiscResult {
    /// TOC read successfully; tracks, durations and optional metadata are ready.
    Loaded { tracks: Vec<TrackInfo>, durations: Vec<String>, metadata: Option<AlbumMetadata>, disc_id: String },
    /// Drive opened but disc absent or unreadable.
    Empty { status: String },
    /// Could not open the drive at all.
    Unavailable { status: String },
}

pub fn scan_drives() -> Vec<DriveInfo> {
    match CdReader::list_drives() {
        Ok(drives) => drives.iter().map(|drive| {

            let display_name = if drive.path.contains("\\\\") {
                drive.path.split('\\').last().unwrap_or(&drive.path).to_string()
            } else {
                drive.path.clone()
            };
            
            let status = if drive.has_audio_cd {
                format!("{} (Audio CD)", display_name)
            } else {
                format!("{} (Empty)", display_name)
            };
            
            DriveInfo {
                path: drive.path.clone(),
                has_audio_cd: drive.has_audio_cd,
                display_name: status,
            }
        }).collect(),
        Err(e) => {
            eprintln!("Failed to scan drives: {}", e);
            Vec::new()
        }
    }
}

pub fn open_drive(path: &str) -> io::Result<CdReader> {
    CdReader::open(path)
}

pub fn read_toc(reader: &CdReader) -> Result<Toc, CdReaderError> {
    reader.read_toc()
}

pub fn get_track_info(toc: &Toc) -> Vec<TrackInfo> {
    let mut tracks = Vec::new();
    
    for track_num in toc.first_track..=toc.last_track {
        if let Some(track) = toc.tracks.iter().find(|t| t.number == track_num) {
            let start_lba = track.start_lba;
            let end_lba = if track_num == toc.last_track {
                toc.leadout_lba
            } else {
                toc.tracks.iter()
                    .find(|t| t.number == track_num + 1)
                    .map(|t| t.start_lba)
                    .unwrap_or(toc.leadout_lba)
            };
            
            let sector_count = end_lba.saturating_sub(start_lba);
            let duration_seconds = sector_count as f64 / 75.0; // 75 sectors/sec
            
            tracks.push(TrackInfo {
                track_number: track_num,
                duration_seconds,
            });
        }
    }
    
    tracks
}

pub fn format_duration(seconds: f64) -> String {
    let mins = (seconds / 60.0).floor() as u32;
    let secs = (seconds % 60.0).floor() as u32;
    format!("{:02}:{:02}", mins, secs)
}
