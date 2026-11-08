use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use ratatui_image::picker::Picker;

use crate::library::{LibraryAlbum, LibraryScanResult, LibrarySettings};
use crate::library_cache::{TuiLibraryView, TuiSettings, TuiTheme};
use super::*;

/// an app over a made-up library, touching no drives, caches or os services.
fn fixture(view: TuiLibraryView, theme: TuiTheme) -> TuiApp {
    let root = PathBuf::from("/music");
    let cover = std::env::temp_dir().join("kanae-tui-test-cover.png");
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90])).save(&cover).unwrap();
    let cover_url = format!("file://{}", cover.to_string_lossy());

    let mut albums = Vec::new();
    for i in 0..40 {
        let dir = root.join(format!("Artist {}", i % 7)).join(format!("Album {}", i));
        albums.push(LibraryAlbum {
            id: format!("{}\u{1f}{}", dir.display(), i),
            dir: dir.clone(),
            album: if i == 3 { "\u{5B87}\u{591A}\u{7530}\u{30D2}\u{30AB}\u{30EB} very long album title that never ends".into() } else { format!("Album {}", i) },
            album_artist: format!("Artist {}", i % 7),
            year: if i % 3 == 0 { String::new() } else { format!("{}", 1990 + i) },
            cover_url: if i % 2 == 0 { Some(cover_url.clone()) } else { None },
            track_paths: (0..10).map(|t| dir.join(format!("{:02} Song {}.flac", t + 1, t))).collect(),
            is_single: false,
        });
    }
    let dirs = (0..7).map(|i| root.join(format!("Artist {}", i))).collect();

    let mut library = TuiLibraryState::default();
    // merged, so the root shows albums (and their covers) rather than folders.
    library.settings = LibrarySettings { search_paths: vec![root], merge_all_folders: true, ..LibrarySettings::default() };
    library.scan_result = Some(LibraryScanResult { albums, dirs, dir_mtimes: Default::default() });
    library.scan_gen = 1;
    library.refresh_nodes(None);

    let settings = TuiSettings { library_view: view, theme, ..TuiSettings::default() };
    let mut app = TuiApp::with_state(TuiPlayerState::default(), library, settings);
    app.picker = Some(Picker::halfblocks());
    // queue the cover decodes once and let them land, so frames exercise the
    // drawn-thumbnail path as well as the placeholder one.
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| render::render(&mut app, f)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    app.thumbs.poll(app.picker.as_ref());
    app
}

fn draw_all_sizes(app: &mut TuiApp) {
    let sizes = [(1, 1), (10, 3), (39, 9), (40, 10), (55, 14), (64, 20), (80, 24), (100, 30), (140, 45), (220, 70)];
    for (w, h) in sizes {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render::render(app, f)).unwrap();
    }
}

fn key(app: &mut TuiApp, code: KeyCode) {
    input::handle_key(app, code, KeyModifiers::NONE);
}

#[test]
fn renders_every_view_at_every_size() {
    for theme in [TuiTheme::Terminal, TuiTheme::Kanae] {
        for view in [TuiLibraryView::Grid, TuiLibraryView::List] {
            let mut app = fixture(view, theme);
            draw_all_sizes(&mut app);

            // walk the grid / list and into a folder.
            for code in [KeyCode::Down, KeyCode::Right, KeyCode::End, KeyCode::PageUp, KeyCode::Home] {
                key(&mut app, code);
                draw_all_sizes(&mut app);
            }

            // search, including romaji matching of japanese titles.
            key(&mut app, KeyCode::Char('/'));
            for c in "utada".chars() { key(&mut app, KeyCode::Char(c)); }
            app.tick();
            draw_all_sizes(&mut app);
            assert_eq!(app.visible_nodes().len(), 1, "romaji search should find the japanese album");
            key(&mut app, KeyCode::Esc);

            key(&mut app, KeyCode::Char('/'));
            for c in "zzzz".chars() { key(&mut app, KeyCode::Char(c)); }
            app.tick();
            draw_all_sizes(&mut app);
            key(&mut app, KeyCode::Esc);

            // queue pane, toasts, play-order toggles, help, settings.
            key(&mut app, KeyCode::Char('u'));
            key(&mut app, KeyCode::Char('z'));
            key(&mut app, KeyCode::Char('r'));
            key(&mut app, KeyCode::Char('C'));
            draw_all_sizes(&mut app);
            key(&mut app, KeyCode::Char('u'));
            key(&mut app, KeyCode::Char('?'));
            draw_all_sizes(&mut app);
            key(&mut app, KeyCode::Esc);
            key(&mut app, KeyCode::Char('s'));
            for _ in 0..30 {
                key(&mut app, KeyCode::Down);
                draw_all_sizes(&mut app);
            }
            assert_eq!(app.view, View::Settings);
            key(&mut app, KeyCode::Esc);
            assert_eq!(app.view, View::Library);
        }
    }
}

#[test]
fn search_matches_track_names_and_all_terms() {
    let mut app = fixture(TuiLibraryView::List, TuiTheme::Terminal);
    key(&mut app, KeyCode::Char('/'));
    for c in "album 9".chars() { key(&mut app, KeyCode::Char(c)); }
    app.tick();
    // a title match on the whole query ranks ahead of scattered term matches.
    assert_eq!(app.visible_nodes()[0].name(), "Album 9");

    app.search.as_mut().unwrap().query = "artist 6 zz".into();
    app.tick();
    assert!(app.visible_nodes().is_empty());

    app.search.as_mut().unwrap().query = "song 3".into();
    app.tick();
    // every album has a track called "Song 3".
    assert_eq!(app.visible_nodes().len(), 40);
}

#[test]
fn grid_navigation_moves_by_rows() {
    let mut app = fixture(TuiLibraryView::Grid, TuiTheme::Terminal);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| render::render(&mut app, f)).unwrap();
    let cols = app.grid_cols;
    assert!(cols > 1);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.content_selected, cols);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.content_selected, cols + 1);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.content_selected, 1);
}

#[test]
fn repeat_and_shuffle_pick_the_next_track() {
    let mut p = TuiPlayerState::default();
    p.total_tracks = 4;
    p.current_track = 3;
    assert_eq!(p.pick_next_index(), None);
    p.repeat = RepeatMode::All;
    assert_eq!(p.pick_next_index(), Some(0));
    p.repeat = RepeatMode::One;
    assert_eq!(p.pick_next_index(), Some(3));

    // a shuffle pass visits every track once before stopping.
    p.repeat = RepeatMode::Off;
    p.toggle_shuffle();
    p.current_track = 0;
    let mut seen = vec![0];
    while let Some(i) = p.pick_next_index() {
        assert!(!seen.contains(&i), "track {} came up twice in one pass", i);
        seen.push(i);
        p.current_track = i;
    }
    seen.sort();
    assert_eq!(seen, vec![0, 1, 2, 3]);
}

#[test]
fn parses_osc_colour_replies() {
    let reply = "\x1b]10;rgb:ffff/8080/0000\x1b\\\x1b]11;rgb:1a/2b/3c\x07\x1b[?62;22c";
    assert_eq!(parse_osc_color(reply, "10"), Some((255, 128, 0)));
    assert_eq!(parse_osc_color(reply, "11"), Some((26, 43, 60)));
    assert_eq!(parse_osc_color("\x1b[?62c", "11"), None);
}
