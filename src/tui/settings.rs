use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::library_cache::TuiImageMethod;
use super::render::{display_width, truncate_left, truncate_to_cols};
use super::{resolve_protocol, Icons, TuiApp, View};

#[derive(Default)]
pub(super) struct SettingsState {
    /// index into `items()`.
    pub selected:       usize,
    scroll:             usize,
    /// Some while a new library folder is being typed.
    pub input:          Option<String>,
    item_rects:         Vec<(Rect, usize)>,
    /// a path removal armed by the first press, confirmed by a second.
    pending_remove:     Option<(usize, Instant)>,
    /// (lrc entries, no-lyrics entries), loaded on demand: reading the cache
    /// file every frame is wasteful.
    lyric_counts:       Option<(usize, usize)>,
    /// the view settings was opened from, returned to on close.
    return_to:          Option<View>,
}

impl SettingsState {
    /// forget derived state so the screen reads fresh values when opened.
    pub fn on_open(&mut self, from: View) {
        self.return_to = Some(from);
        self.lyric_counts = None;
        self.pending_remove = None;
        self.input = None;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Key {
    Path(usize),
    AddPath,
    MergeAll,
    AlbumSort,
    Rescan,
    Theme,
    Accent,
    LibraryView,
    IconSet,
    ShowSidebar,
    ShowHints,
    ImageMethod,
    GridCovers,
    Romanize,
    LrcLimit,
    PurgeLrc,
    PurgeNoLyrics,
    Discord,
}

enum Row {
    Header(&'static str),
    Item(Key),
}

enum Value {
    Toggle(bool),
    Choice(String),
    /// an action, with an optional status badge.
    Action(String),
    Path,
}

fn rows(app: &TuiApp) -> Vec<Row> {
    let mut r = vec![Row::Header("Library")];
    for i in 0..app.library.settings.search_paths.len() {
        r.push(Row::Item(Key::Path(i)));
    }
    r.extend([
        Row::Item(Key::AddPath),
        Row::Item(Key::MergeAll),
        Row::Item(Key::AlbumSort),
        Row::Item(Key::Rescan),
        Row::Header("Appearance"),
        Row::Item(Key::Theme),
        Row::Item(Key::Accent),
        Row::Item(Key::LibraryView),
        Row::Item(Key::IconSet),
        Row::Item(Key::ShowSidebar),
        Row::Item(Key::ShowHints),
        Row::Header("Album Art"),
        Row::Item(Key::ImageMethod),
        Row::Item(Key::GridCovers),
        Row::Header("Lyrics"),
        Row::Item(Key::Romanize),
        Row::Item(Key::LrcLimit),
        Row::Item(Key::PurgeLrc),
        Row::Item(Key::PurgeNoLyrics),
        Row::Header("Integrations"),
        Row::Item(Key::Discord),
    ]);
    r
}

fn items(app: &TuiApp) -> Vec<Key> {
    rows(app).into_iter().filter_map(|r| match r { Row::Item(k) => Some(k), _ => None }).collect()
}

fn image_method_unavailable(app: &TuiApp) -> bool {
    matches!(
        app.tui_settings.image_method,
        TuiImageMethod::Sixel | TuiImageMethod::Kitty | TuiImageMethod::Iterm2
    ) && !app.supported_protos.contains(&resolve_protocol(app.tui_settings.image_method))
}

fn label(app: &TuiApp, key: Key) -> String {
    match key {
        Key::Path(i)       => app.library.settings.search_paths.get(i)
            .map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        Key::AddPath       => format!("{} Add folder", app.icons.add),
        Key::MergeAll      => "Merge all folders".into(),
        Key::AlbumSort     => "Sort albums by".into(),
        Key::Rescan        => format!("{} Rescan library", app.icons.rescan),
        Key::Theme         => "Theme".into(),
        Key::Accent        => "Accent colour".into(),
        Key::LibraryView   => "Library layout".into(),
        Key::IconSet       => "Icon set".into(),
        Key::ShowSidebar   => "Now-playing sidebar".into(),
        Key::ShowHints     => "Key hints".into(),
        Key::ImageMethod   => "Image method".into(),
        Key::GridCovers    => "Covers in grid".into(),
        Key::Romanize      => "Romanize Japanese lyrics".into(),
        Key::LrcLimit      => "Unlimited lyric cache".into(),
        Key::PurgeLrc      => format!("{} Clear cached lyrics", app.icons.trash),
        Key::PurgeNoLyrics => format!("{} Clear no-lyrics list", app.icons.trash),
        Key::Discord       => "Discord Rich Presence".into(),
    }
}

fn value(app: &mut TuiApp, key: Key) -> Value {
    let lib = &app.library.settings;
    let tui = &app.tui_settings;
    match key {
        Key::Path(_)       => Value::Path,
        Key::AddPath       => Value::Action(String::new()),
        Key::MergeAll      => Value::Toggle(lib.merge_all_folders),
        Key::AlbumSort     => Value::Choice(crate::library::album_sort_label(&lib.album_sort).to_string()),
        Key::Rescan        => Value::Action(if app.library.is_scanning { "scanning".into() } else { String::new() }),
        Key::Theme         => Value::Choice(tui.theme.label().into()),
        Key::Accent        => Value::Choice(tui.accent.label().into()),
        Key::LibraryView   => Value::Choice(tui.library_view.label().into()),
        Key::IconSet       => Value::Choice(tui.icon_set.label().into()),
        Key::ShowSidebar   => Value::Toggle(tui.show_sidebar),
        Key::ShowHints     => Value::Toggle(tui.show_hints),
        Key::ImageMethod   => {
            let mut v = tui.image_method.label().to_string();
            if image_method_unavailable(app) { v.push_str(" (?)"); }
            Value::Choice(v)
        }
        Key::GridCovers    => Value::Toggle(tui.grid_covers),
        Key::Romanize      => Value::Toggle(lib.romanize_lyrics),
        Key::LrcLimit      => Value::Toggle(lib.lrc_limit_disabled),
        Key::PurgeLrc      => Value::Action(format!("{} saved", lyric_counts(app).0)),
        Key::PurgeNoLyrics => Value::Action(format!("{} songs", lyric_counts(app).1)),
        Key::Discord       => Value::Toggle(lib.discord_rpc),
    }
}

fn lyric_counts(app: &mut TuiApp) -> (usize, usize) {
    *app.settings.lyric_counts.get_or_insert_with(|| {
        let c = crate::lyric_cache::LyricContentCache::load();
        (c.lrc_count(), c.no_lyrics_count())
    })
}

fn describe(app: &TuiApp, key: Key) -> String {
    match key {
        Key::Path(_)       => "Scanned for music. Press x twice to remove it from the library.".into(),
        Key::AddPath       => "Type or paste a folder path. Tab completes, Enter adds, Esc cancels.".into(),
        Key::MergeAll      => "Show every album at the library root and hide folder tiles.".into(),
        Key::AlbumSort     => "Order of albums in the library and in search results.".into(),
        Key::Rescan        => "Re-read every folder now. New and changed files are also picked up automatically.".into(),
        Key::Theme         => "Terminal follows your terminal's own colours, so light and dark schemes both keep their contrast. Kanae Dark matches the desktop app.".into(),
        Key::Accent        => "Highlight colour. In the Terminal theme it comes from your terminal's palette.".into(),
        Key::LibraryView   => "Cover grid or compact list. Press v anywhere in the library to switch.".into(),
        Key::IconSet       => "Nerd Fonts needs a patched terminal font. Plain Text works everywhere.".into(),
        Key::ShowSidebar   => "Cover, album details and synced lyrics on the left. Press b to toggle.".into(),
        Key::ShowHints     => "The key hint line under the player. Press ? for the full list any time.".into(),
        Key::ImageMethod   => {
            let base = "How covers are drawn. Auto picks the best protocol your terminal reports.";
            if image_method_unavailable(app) {
                format!("{} Your terminal did not report support for this one, so it may not render.", base)
            } else {
                base.into()
            }
        }
        Key::GridCovers    => "Draw album covers on grid tiles. Off shows icons, which is lighter on slow terminals.".into(),
        Key::Romanize      => "Show Japanese lyrics in romaji (kana and kanji converted).".into(),
        Key::LrcLimit      => "Keep every downloaded lyric instead of only the most recent 100.".into(),
        Key::PurgeLrc      => "Delete all downloaded lyrics. They are fetched again when needed.".into(),
        Key::PurgeNoLyrics => "Forget songs marked as having no lyrics, so they are looked up again.".into(),
        Key::Discord       => "Show what you're listening to on your Discord profile.".into(),
    }
}

/// apply a setting. `step` is 0 for Enter / click, or -1 / +1 for left / right.
fn activate(app: &mut TuiApp, key: Key, step: isize) {
    let fwd = step >= 0;
    let save_lib = |app: &mut TuiApp| crate::library_cache::save_settings(&app.library.settings);
    match key {
        Key::Path(_) => {
            if step == 0 { app.toast("Press x twice to remove this folder"); }
        }
        Key::AddPath => {
            if step == 0 { app.settings.input = Some(String::new()); }
        }
        Key::MergeAll => {
            app.library.settings.merge_all_folders = !app.library.settings.merge_all_folders;
            save_lib(app);
            app.library.nodes_dirty = true;
            app.library.scan_gen += 1;
        }
        Key::AlbumSort => {
            let cur = &app.library.settings.album_sort;
            app.library.settings.album_sort = if fwd {
                crate::library::album_sort_next(cur)
            } else {
                crate::library::album_sort_prev(cur)
            }.to_string();
            save_lib(app);
            app.library.nodes_dirty = true;
            app.library.scan_gen += 1;
        }
        Key::Rescan => {
            if step == 0 {
                if app.library.settings.search_paths.is_empty() {
                    app.toast("Add a folder first");
                } else {
                    app.library.start_scan();
                }
            }
        }
        Key::Theme => {
            let t = app.tui_settings.theme;
            app.tui_settings.theme = if fwd { t.next() } else { t.prev() };
            app.save_tui_settings();
            app.apply_theme();
        }
        Key::Accent => {
            let a = app.tui_settings.accent;
            app.tui_settings.accent = if fwd { a.next() } else { a.prev() };
            app.save_tui_settings();
            app.apply_theme();
        }
        Key::LibraryView => {
            let v = app.tui_settings.library_view;
            app.tui_settings.library_view = if fwd { v.next() } else { v.prev() };
            app.save_tui_settings();
        }
        Key::IconSet => {
            let s = app.tui_settings.icon_set;
            app.tui_settings.icon_set = if fwd { s.next() } else { s.prev() };
            app.save_tui_settings();
            app.icons = Icons::from_set(app.tui_settings.icon_set);
            app.needs_full_redraw = true;
        }
        Key::ShowSidebar => {
            app.tui_settings.show_sidebar = !app.tui_settings.show_sidebar;
            app.save_tui_settings();
            app.needs_full_redraw = true;
        }
        Key::ShowHints => {
            app.tui_settings.show_hints = !app.tui_settings.show_hints;
            app.save_tui_settings();
            app.needs_full_redraw = true;
        }
        Key::ImageMethod => {
            let m = app.tui_settings.image_method;
            app.tui_settings.image_method = if fwd { m.next() } else { m.prev() };
            app.save_tui_settings();
            app.apply_image_method();
        }
        Key::GridCovers => {
            app.tui_settings.grid_covers = !app.tui_settings.grid_covers;
            app.save_tui_settings();
            app.needs_full_redraw = true;
        }
        Key::Romanize => {
            app.library.settings.romanize_lyrics = !app.library.settings.romanize_lyrics;
            save_lib(app);
            app.player.apply_romanization(app.library.settings.romanize_lyrics);
        }
        Key::LrcLimit => {
            app.library.settings.lrc_limit_disabled = !app.library.settings.lrc_limit_disabled;
            save_lib(app);
        }
        Key::PurgeLrc => {
            if step == 0 {
                let mut c = crate::lyric_cache::LyricContentCache::load();
                c.purge_lrc();
                c.save();
                app.settings.lyric_counts = None;
                app.toast("Cached lyrics cleared");
            }
        }
        Key::PurgeNoLyrics => {
            if step == 0 {
                let mut c = crate::lyric_cache::LyricContentCache::load();
                c.purge_no_lyrics();
                c.save();
                app.settings.lyric_counts = None;
                app.toast("No-lyrics list cleared");
            }
        }
        Key::Discord => {
            app.library.settings.discord_rpc = !app.library.settings.discord_rpc;
            save_lib(app);
        }
    }
}

fn remove_path(app: &mut TuiApp, i: usize) {
    let armed = app.settings.pending_remove
        .is_some_and(|(j, t)| j == i && t.elapsed() < Duration::from_secs(3));
    let Some(path) = app.library.settings.search_paths.get(i).cloned() else { return };
    if !armed {
        app.settings.pending_remove = Some((i, Instant::now()));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        app.toast(format!("Press x again to remove {}", name));
        return;
    }
    app.settings.pending_remove = None;
    app.library.settings.search_paths.remove(i);
    crate::library_cache::save_settings(&app.library.settings);
    if app.library.settings.search_paths.is_empty() {
        app.library.scan_result = None;
        app.library.nodes_dirty = true;
        app.library.scan_gen += 1;
    } else {
        app.library.start_scan();
    }
    app.toast("Folder removed");
}

/// `~` to the home directory, so typed paths behave like they do in a shell.
fn expand_home(p: &str) -> String {
    if p == "~" || p.starts_with("~/") || p.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return format!("{}{}", home.to_string_lossy(), &p[1..]);
        }
    }
    p.to_string()
}

/// a pasted or dropped path: drop quotes and shell escaping around spaces.
pub(super) fn clean_pasted_path(text: &str) -> String {
    let t = text.trim().trim_matches(|c| c == '\'' || c == '"');
    if cfg!(windows) { t.to_string() } else { t.replace("\\ ", " ") }
}

/// complete the last component of a directory path. returns the new text and
/// every candidate name that matched.
fn complete_path(input: &str) -> Option<(String, Vec<String>)> {
    let expanded = expand_home(input);
    let split = expanded.rfind(|c| c == '/' || c == std::path::MAIN_SEPARATOR)?;
    let (dir, prefix) = (&expanded[..=split], &expanded[split + 1..]);
    let lc = prefix.to_lowercase();
    let mut names: Vec<String> = std::fs::read_dir(dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| (prefix.starts_with('.') || !n.starts_with('.')) && n.to_lowercase().starts_with(&lc))
        .collect();
    if names.is_empty() { return None; }
    names.sort_by_key(|n| n.to_lowercase());
    let completed = if names.len() == 1 {
        format!("{}{}{}", dir, names[0], std::path::MAIN_SEPARATOR)
    } else {
        // longest prefix shared by every candidate, compared case-insensitively.
        let first: Vec<char> = names[0].chars().collect();
        let mut n = first.len();
        for other in &names[1..] {
            let common = first.iter().zip(other.chars())
                .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                .count();
            n = n.min(common);
        }
        format!("{}{}", dir, first[..n].iter().collect::<String>())
    };
    Some((completed, names))
}

fn commit_input(app: &mut TuiApp) {
    let Some(text) = app.settings.input.clone() else { return };
    let mut path = expand_home(text.trim());
    while path.len() > 1 && (path.ends_with('/') || path.ends_with(std::path::MAIN_SEPARATOR)) {
        path.pop();
    }
    if path.is_empty() {
        app.settings.input = None;
        return;
    }
    let p = std::path::PathBuf::from(&path);
    if !p.is_dir() {
        app.toast("That folder doesn't exist");
        return;
    }
    if app.library.settings.search_paths.contains(&p) {
        app.toast("Already in the library");
        app.settings.input = None;
        return;
    }
    app.library.settings.search_paths.push(p);
    crate::library_cache::save_settings(&app.library.settings);
    app.library.start_scan();
    app.settings.input = None;
    app.toast("Folder added, scanning");
}

pub(super) fn handle_paste(app: &mut TuiApp, text: &str) -> bool {
    let Some(ref mut input) = app.settings.input else { return false };
    input.push_str(&clean_pasted_path(text));
    true
}

/// keys while the settings screen is open. returns false for keys it leaves
/// to the global bindings.
pub(super) fn handle_key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) -> bool {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    if let Some(ref mut input) = app.settings.input {
        match code {
            KeyCode::Esc => { app.settings.input = None; }
            KeyCode::Enter => commit_input(app),
            KeyCode::Tab => {
                match complete_path(input) {
                    Some((done, names)) => {
                        *input = done;
                        if names.len() > 1 {
                            let shown: Vec<&str> = names.iter().take(4).map(|s| s.as_str()).collect();
                            let more = if names.len() > 4 { format!(" +{}", names.len() - 4) } else { String::new() };
                            app.toast(format!("{}{}", shown.join("  "), more));
                        }
                    }
                    None => app.toast("No matching folders"),
                }
            }
            KeyCode::Backspace if ctrl || mods.contains(KeyModifiers::ALT) => {
                pop_component(input);
            }
            KeyCode::Char('w') if ctrl => pop_component(input),
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Backspace => { input.pop(); }
            KeyCode::Char(c) if !ctrl => input.push(c),
            _ => {}
        }
        return true;
    }

    let list = items(app);
    if list.is_empty() { return false; }
    let sel = app.settings.selected.min(list.len() - 1);
    let key = list[sel];
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.settings.selected = sel.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.settings.selected = (sel + 1).min(list.len() - 1);
        }
        KeyCode::Home | KeyCode::Char('g') => app.settings.selected = 0,
        KeyCode::End | KeyCode::Char('G') => app.settings.selected = list.len() - 1,
        KeyCode::PageUp => app.settings.selected = sel.saturating_sub(8),
        KeyCode::PageDown => app.settings.selected = (sel + 8).min(list.len() - 1),
        KeyCode::Left | KeyCode::Char('h') => activate(app, key, -1),
        KeyCode::Right | KeyCode::Char('l') => activate(app, key, 1),
        KeyCode::Enter => activate(app, key, 0),
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Key::Path(i) = key { remove_path(app, i); }
        }
        KeyCode::Esc | KeyCode::Char('s') | KeyCode::Backspace => close(app),
        _ => return false,
    }
    true
}

pub(super) fn close(app: &mut TuiApp) {
    app.settings.input = None;
    app.view = match app.settings.return_to.take() {
        Some(View::Album) if app.browse_dir.is_some() || app.player.total_tracks > 0 => View::Album,
        _ => View::Library,
    };
}

fn pop_component(input: &mut String) {
    let trimmed = input.trim_end_matches(|c| c == '/' || c == std::path::MAIN_SEPARATOR).len();
    input.truncate(trimmed);
    match input.rfind(|c| c == '/' || c == std::path::MAIN_SEPARATOR) {
        Some(i) => input.truncate(i + 1),
        None    => input.clear(),
    }
}

/// a left click inside the settings list.
pub(super) fn handle_click(app: &mut TuiApp, x: u16, y: u16) -> bool {
    let hit = app.settings.item_rects.iter()
        .find(|(r, _)| super::rect_contains(*r, x, y))
        .map(|(_, i)| *i);
    let Some(i) = hit else { return false };
    app.settings.selected = i;
    if let Some(&key) = items(app).get(i) {
        if !matches!(key, Key::Path(_)) && app.settings.input.is_none() {
            activate(app, key, 0);
        }
    }
    true
}

pub(super) fn scroll(app: &mut TuiApp, down: bool) {
    let n = items(app).len();
    if down {
        app.settings.selected = (app.settings.selected + 1).min(n.saturating_sub(1));
    } else {
        app.settings.selected = app.settings.selected.saturating_sub(1);
    }
}

pub(super) fn render(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    app.settings.item_rects.clear();
    if area.height == 0 || area.width < 4 { return; }

    let rows = rows(app);
    let n_items = rows.iter().filter(|r| matches!(r, Row::Item(_))).count();
    if app.settings.selected >= n_items { app.settings.selected = n_items.saturating_sub(1); }
    let sel = app.settings.selected;

    let desc_h = if area.height >= 12 { 3 } else { 0 };
    let [list_area, desc_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(desc_h),
    ]).areas(area);

    let w = list_area.width as usize;
    // build the full line list first, remembering which line each item sits
    // on, then scroll so the selection stays visible.
    let mut lines: Vec<(Line, Option<usize>)> = Vec::new();
    let mut item_idx = 0usize;
    let mut sel_line = 0usize;
    for row in &rows {
        match row {
            Row::Header(name) => {
                if !lines.is_empty() { lines.push((Line::raw(""), None)); }
                // let title = format!(" {} ", name.to_uppercase());
                let title = format!(" {} ", name);
                let rule = "\u{2500}".repeat(w.saturating_sub(display_width(&title) + 1));
                lines.push((Line::from(vec![
                    Span::styled(title, t.accent.add_modifier(Modifier::BOLD)),
                    Span::styled(rule, t.border),
                ]), None));
            }
            Row::Item(key) => {
                let selected = item_idx == sel;
                if selected { sel_line = lines.len(); }
                let line = item_line(app, *key, selected, w);
                lines.push((line, Some(item_idx)));
                item_idx += 1;
            }
        }
    }

    let h = list_area.height as usize;
    let scroll = &mut app.settings.scroll;
    // keep a line of context above (the section header, usually) and below.
    if sel_line < *scroll + 1 { *scroll = sel_line.saturating_sub(1); }
    if sel_line + 2 > *scroll + h { *scroll = (sel_line + 2).saturating_sub(h); }
    *scroll = (*scroll).min(lines.len().saturating_sub(h));
    let scroll = *scroll;

    for (row, (line, item)) in lines.into_iter().skip(scroll).take(h).enumerate() {
        let rect = Rect { x: list_area.x, y: list_area.y + row as u16, width: list_area.width, height: 1 };
        let para = Paragraph::new(line);
        if item == Some(sel) {
            frame.render_widget(para.style(t.sel), rect);
        } else {
            frame.render_widget(para, rect);
        }
        if let Some(i) = item { app.settings.item_rects.push((rect, i)); }
    }

    if desc_h > 0 {
        let key = items(app).get(sel).copied();
        let text = match (&app.settings.input, key) {
            (Some(input), _) => {
                let p = std::path::PathBuf::from(expand_home(input.trim()));
                if input.trim().is_empty() {
                    describe(app, Key::AddPath)
                } else if p.is_dir() {
                    "Folder found. Enter adds it, Tab completes.".to_string()
                } else {
                    "No folder at that path yet. Tab completes, Esc cancels.".to_string()
                }
            }
            (None, Some(k)) => describe(app, k),
            _ => String::new(),
        };
        let [rule, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(desc_area);
        frame.render_widget(Paragraph::new("\u{2500}".repeat(rule.width as usize)).style(t.border), rule);
        let body = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
        frame.render_widget(Paragraph::new(text).style(t.muted).wrap(Wrap { trim: true }), body);
    }
}

fn item_line(app: &mut TuiApp, key: Key, selected: bool, w: usize) -> Line<'static> {
    let t = app.theme;
    let row = |s| t.on_row(s, selected);
    let marker = if selected { app.icons.sel_marker } else { " " };
    let lead = format!(" {} ", marker);
    let lead_w = display_width(&lead);

    // the folder being typed replaces the "Add folder" row.
    if key == Key::AddPath {
        if let Some(ref input) = app.settings.input {
            let prompt = "Path: ";
            let avail = w.saturating_sub(lead_w + prompt.len() + 2);
            return Line::from(vec![
                Span::styled(lead, row(t.accent)),
                Span::styled(prompt, row(t.text2)),
                Span::styled(truncate_left(input, avail), row(t.text.add_modifier(Modifier::BOLD))),
                Span::styled(" ", t.text.add_modifier(Modifier::REVERSED)),
            ]);
        }
    }

    let (val_text, val_style) = match value(app, key) {
        Value::Toggle(on) => {
            let icon = if on { app.icons.checked } else { app.icons.unchecked };
            (format!("{} {}", icon, if on { "On " } else { "Off" }), if on { t.accent } else { t.muted })
        }
        Value::Choice(v) => (format!("{} {} {}", app.icons.cycle_left, v, app.icons.cycle_right), t.text2),
        Value::Action(badge) => (badge, t.muted),
        Value::Path => ("x".to_string(), t.muted),
    };
    let val_w = display_width(&val_text);
    let label = label(app, key);
    let label_avail = w.saturating_sub(lead_w + val_w + 3);
    let label_txt = if matches!(key, Key::Path(_)) {
        truncate_left(&label, label_avail)
    } else {
        truncate_to_cols(&label, label_avail)
    };
    let pad = w.saturating_sub(lead_w + display_width(&label_txt) + val_w + 1);
    let label_style = if selected { t.text.add_modifier(Modifier::BOLD) } else { t.text };
    Line::from(vec![
        Span::styled(lead, row(t.accent)),
        Span::styled(label_txt, row(label_style)),
        Span::raw(" ".repeat(pad)),
        Span::styled(val_text, row(val_style)),
        Span::raw(" "),
    ])
}
