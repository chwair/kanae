use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::library_cache::TuiLibraryView;
use super::{rect_contains, search::Search, settings, RepeatMode, TuiApp, TuiLibraryNode, View};

pub(super) fn handle_key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    app.sync_nodes();
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    if ctrl && code == KeyCode::Char('c') {
        app.quitting = true;
        return;
    }
    if app.help_open {
        app.help_open = false;
        app.needs_full_redraw = true;
        return;
    }

    let searching = app.view == View::Library && app.search.as_ref().is_some_and(|s| s.editing);
    if searching && search_key(app, code, mods) { return; }
    if app.view == View::Settings && settings::handle_key(app, code, mods) { return; }
    if app.queue_open && app.queue_focus && queue_key(app, code) { return; }

    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') => app.quitting = true,
        KeyCode::Char(' ') => play_pause(app),
        KeyCode::Char('n') | KeyCode::Char(']') => next_track(app),
        KeyCode::Char('p') | KeyCode::Char('[') => prev_track(app),
        KeyCode::Char(',') | KeyCode::Char('<') => seek_by(app, -5.0),
        KeyCode::Char('.') | KeyCode::Char('>') => seek_by(app, 5.0),
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let v = app.player.volume_f64() + 0.05;
            app.player.set_volume(v);
        }
        KeyCode::Char('-') | KeyCode::Char('_') => {
            let v = app.player.volume_f64() - 0.05;
            app.player.set_volume(v);
        }
        KeyCode::Char('e') => eject(app),
        KeyCode::Char('s') => toggle_settings(app),
        KeyCode::Char('?') => {
            app.help_open = true;
            app.needs_full_redraw = true;
        }
        KeyCode::Char('u') => toggle_queue(app),
        KeyCode::Char('z') => toggle_shuffle(app),
        KeyCode::Char('r') => cycle_repeat(app),
        KeyCode::Char('C') if app.queue_open => clear_queue(app),
        KeyCode::Tab if app.queue_open => app.queue_focus = !app.queue_focus,
        KeyCode::Char('/') if !ctrl => open_search(app),
        KeyCode::Char('v') if app.view == View::Library => toggle_library_view(app),
        KeyCode::Char('b') => {
            app.tui_settings.show_sidebar = !app.tui_settings.show_sidebar;
            app.save_tui_settings();
            app.needs_full_redraw = true;
        }
        KeyCode::Char('a') if app.view != View::Settings => queue_selection(app),
        _ => content_key(app, code, mods),
    }
}

// global actions

fn play_pause(app: &mut TuiApp) {
    if app.player.total_tracks > 0 && app.player.current_track >= 0 {
        app.player.play_pause();
    } else if app.player.total_tracks > 0 {
        app.player.load_track(0);
        app.player.start_playback();
        app.player.fetch_lyrics_for_current();
    } else if app.view == View::Library {
        // nothing loaded yet: space plays the selected album from the top.
        if let Some(TuiLibraryNode::Album { .. }) = app.visible_nodes().get(app.content_selected) {
            app.open_album_view_for_library_item(app.content_selected);
            app.play_browsed_track(0);
        }
    }
}

fn next_track(app: &mut TuiApp) {
    let was = app.player.is_playing;
    app.player.next_track();
    if was { app.player.fetch_lyrics_for_current(); }
}

fn prev_track(app: &mut TuiApp) {
    let was = app.player.is_playing;
    app.player.prev_track();
    if was { app.player.fetch_lyrics_for_current(); }
}

fn seek_by(app: &mut TuiApp, delta: f64) {
    if app.player.current_track < 0 { return; }
    let to = (app.player.current_time() + delta).clamp(0.0, app.player.total_time.max(0.0));
    app.player.seek(to);
}

fn eject(app: &mut TuiApp) {
    if app.player.drives.is_empty() {
        app.toast("No disc drive found");
    } else {
        app.player.eject();
        app.toast("Ejecting disc");
    }
}

fn toggle_settings(app: &mut TuiApp) {
    if app.view == View::Settings {
        settings::close(app);
    } else {
        app.settings.on_open(app.view);
        app.view = View::Settings;
    }
}

fn toggle_shuffle(app: &mut TuiApp) {
    app.player.toggle_shuffle();
    app.toast(if app.player.shuffle { "Shuffle on" } else { "Shuffle off" });
}

fn cycle_repeat(app: &mut TuiApp) {
    app.player.cycle_repeat();
    app.toast(match app.player.repeat {
        RepeatMode::Off => "Repeat off",
        RepeatMode::All => "Repeat all",
        RepeatMode::One => "Repeat one",
    });
}

fn clear_queue(app: &mut TuiApp) {
    if app.player.queue.is_empty() {
        app.toast("Queue is already empty");
        return;
    }
    app.player.queue.clear();
    app.queue_selected = 0;
    app.toast("Queue cleared");
}

fn toggle_queue(app: &mut TuiApp) {
    app.queue_open = !app.queue_open;
    app.queue_focus = false;
    app.needs_full_redraw = true;
}

fn toggle_library_view(app: &mut TuiApp) {
    app.tui_settings.library_view = app.tui_settings.library_view.next();
    app.save_tui_settings();
    // the scroll offset means rows in one layout and tiles in the other.
    app.content_scroll = 0;
    app.needs_full_redraw = true;
}

fn open_search(app: &mut TuiApp) {
    if app.view != View::Library {
        app.back_to_library();
    }
    match app.search {
        Some(ref mut s) => s.editing = true,
        None => {
            app.search = Some(Search::new());
            app.reset_content_cursor();
        }
    }
}

fn close_search(app: &mut TuiApp) {
    app.search = None;
    app.reset_content_cursor();
    app.needs_full_redraw = true;
}

fn queue_selection(app: &mut TuiApp) {
    let paths = app.queueable_selection();
    if paths.is_empty() {
        app.toast("Nothing to queue here");
        return;
    }
    let n = app.player.enqueue_paths(&paths);
    if !app.queue_open {
        app.queue_open = true;
        app.needs_full_redraw = true;
    }
    app.toast(format!("Queued {} track{}", n, if n == 1 { "" } else { "s" }));
}

// search bar

fn search_key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) -> bool {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    let Some(ref mut s) = app.search else { return false };
    match code {
        KeyCode::Esc => close_search(app),
        KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
            s.editing = false;
            app.reset_content_cursor();
        }
        KeyCode::Backspace if ctrl || alt => delete_word(&mut s.query),
        KeyCode::Char('w') if ctrl => delete_word(&mut s.query),
        KeyCode::Char('u') if ctrl => s.query.clear(),
        KeyCode::Backspace => { s.query.pop(); }
        KeyCode::Char(c) if !ctrl => {
            s.query.push(c);
            app.reset_content_cursor();
        }
        KeyCode::Up | KeyCode::Left | KeyCode::Right => {}
        _ => return false,
    }
    true
}

fn delete_word(q: &mut String) {
    let end = q.trim_end().len();
    q.truncate(end);
    match q.rfind(' ') {
        Some(i) => q.truncate(i + 1),
        None    => q.clear(),
    }
}

// queue pane

fn queue_key(app: &mut TuiApp, code: KeyCode) -> bool {
    let len = app.player.queue.len();
    match code {
        KeyCode::Up | KeyCode::Char('k') => app.queue_selected = app.queue_selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            if app.queue_selected + 1 < len { app.queue_selected += 1; }
        }
        KeyCode::Char('K') => {
            if app.queue_selected > 0 && app.queue_selected < len {
                let to = app.queue_selected - 1;
                app.player.queue.move_entry(app.queue_selected, to);
                app.queue_selected = to;
            }
        }
        KeyCode::Char('J') => {
            let to = app.queue_selected + 1;
            if to < len {
                app.player.queue.move_entry(app.queue_selected, to);
                app.queue_selected = to;
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if app.queue_selected < len {
                app.player.queue.remove(app.queue_selected);
                let max = app.player.queue.len().saturating_sub(1);
                if app.queue_selected > max { app.queue_selected = max; }
            }
        }
        KeyCode::Char('C') | KeyCode::Char('c') => clear_queue(app),
        KeyCode::Enter => {
            if app.queue_selected < len {
                app.player.queue_play_at(app.queue_selected);
                app.player.fetch_lyrics_for_current();
                app.queue_selected = 0;
            }
        }
        KeyCode::Esc | KeyCode::Tab => {
            app.queue_focus = false;
            // too narrow to dock, so the queue was full screen: close it.
            if app.queue_fullscreen {
                app.queue_open = false;
                app.needs_full_redraw = true;
            }
        }
        _ => return false,
    }
    true
}

// library / album navigation

fn item_count(app: &TuiApp) -> usize {
    match app.view {
        View::Library  => app.visible_nodes().len(),
        View::Album    => app.effective_tracklist().len(),
        View::Settings => 0,
    }
}

fn is_grid(app: &TuiApp) -> bool {
    app.view == View::Library && app.tui_settings.library_view == TuiLibraryView::Grid
}

fn activate_selected(app: &mut TuiApp) {
    let sel = app.content_selected;
    if sel >= item_count(app) { return; }
    match app.view {
        View::Library  => app.open_album_view_for_library_item(sel),
        View::Album    => app.play_browsed_track(sel),
        View::Settings => {}
    }
}

fn go_back(app: &mut TuiApp) {
    match app.view {
        View::Album => app.back_to_library(),
        View::Library if app.search.is_some() => close_search(app),
        View::Library if app.library.can_go_back() => {
            app.library.navigate_back();
            app.library.nodes_dirty = true;
            app.reset_content_cursor();
        }
        _ => {}
    }
}

fn go_forward(app: &mut TuiApp) {
    if app.view == View::Library && app.library.can_go_forward() {
        app.library.navigate_forward();
        app.library.nodes_dirty = true;
        app.reset_content_cursor();
    }
}

fn content_key(app: &mut TuiApp, code: KeyCode, mods: KeyModifiers) {
    if app.view == View::Settings { return; }
    let n = item_count(app);
    let last = n.saturating_sub(1);
    let grid = is_grid(app);
    let cols = if grid { app.grid_cols.max(1) } else { 1 };
    let shift = mods.contains(KeyModifiers::SHIFT);
    let alt = mods.contains(KeyModifiers::ALT);
    let sel = app.content_selected.min(last);

    match code {
        KeyCode::Left if shift || alt => go_back(app),
        KeyCode::Right if shift || alt => go_forward(app),
        KeyCode::Up | KeyCode::Char('k') => {
            if sel < cols {
                // off the top of the results: back into the search bar.
                if app.view == View::Library {
                    if let Some(ref mut s) = app.search { s.editing = true; }
                }
                app.content_selected = 0;
            } else {
                app.content_selected = sel - cols;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // a partial last row still takes a down press, landing on its end.
            let next = sel + cols;
            if next <= last { app.content_selected = next; }
            else if sel / cols < last / cols { app.content_selected = last; }
        }
        KeyCode::Left | KeyCode::Char('h') if grid => app.content_selected = sel.saturating_sub(1),
        KeyCode::Right | KeyCode::Char('l') if grid => app.content_selected = (sel + 1).min(last),
        KeyCode::Left => seek_by(app, -5.0),
        KeyCode::Right => seek_by(app, 5.0),
        KeyCode::Char('h') => go_back(app),
        KeyCode::Char('l') | KeyCode::Enter => activate_selected(app),
        KeyCode::PageUp => app.content_selected = sel.saturating_sub(app.page_items.max(1)),
        KeyCode::PageDown => app.content_selected = (sel + app.page_items.max(1)).min(last),
        KeyCode::Home | KeyCode::Char('g') => app.content_selected = 0,
        KeyCode::End | KeyCode::Char('G') => app.content_selected = last,
        KeyCode::Backspace if app.view == View::Library && app.search.is_some() => {
            // backspace on results keeps editing the query.
            if let Some(ref mut s) = app.search {
                s.editing = true;
                s.query.pop();
            }
        }
        KeyCode::Backspace | KeyCode::Esc => go_back(app),
        _ => {}
    }
}

// paste / drop

pub(super) fn handle_paste(app: &mut TuiApp, text: &str) {
    app.sync_nodes();
    if app.view == View::Settings && settings::handle_paste(app, text) { return; }
    if let Some(s) = app.search.as_mut().filter(|s| s.editing) {
        s.query.push_str(text.lines().next().unwrap_or("").trim_end());
        app.reset_content_cursor();
        return;
    }
    // a file or folder dragged onto the terminal arrives as a paste: play it.
    let paths: Vec<String> = text.lines()
        .map(settings::clean_pasted_path)
        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
        .collect();
    if paths.is_empty() {
        app.toast("Paste a file or folder path to play it");
        return;
    }
    app.player.load_file_tracks(paths);
    if app.player.total_tracks <= 0 || !app.player.is_file_mode {
        app.toast("No playable audio there");
        return;
    }
    app.is_external_file  = true;
    app.playing_album_dir = None;
    app.file_mode_active  = true;
    app.cd_view_active    = false;
    app.browse_dir        = None;
    app.browse_album      = None;
    app.view              = View::Album;
    app.reset_content_cursor();
    app.player.load_track(0);
    app.player.start_playback();
    app.player.fetch_lyrics_for_current();
}

// mouse

pub(super) fn handle_mouse(app: &mut TuiApp, event: MouseEvent) {
    app.sync_nodes();
    let (x, y) = (event.column, event.row);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => left_click(app, x, y),
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.divider_dragging {
                app.sidebar_w = x.max(18);
            }
            if app.seek_dragging && app.player.total_tracks > 0 {
                let frac = x.saturating_sub(app.seek_bar_rect.x) as f64 / app.seek_bar_rect.width.max(1) as f64;
                app.player.seek(frac.clamp(0.0, 1.0) * app.player.total_time);
            }
            if app.vol_dragging {
                let frac = x.saturating_sub(app.vol_bar_rect.x) as f64 / app.vol_bar_rect.width.max(1) as f64;
                app.player.set_volume(frac.clamp(0.0, 1.0));
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.seek_dragging = false;
            app.vol_dragging  = false;
            if app.divider_dragging {
                // sidebar was resized - force cover reload at new size, and a
                // full redraw since the drag painted over the old graphic.
                app.cover_loaded_url.clear();
                app.needs_full_redraw = true;
            }
            app.divider_dragging = false;
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let down = event.kind == MouseEventKind::ScrollDown;
            let over_queue = app.queue_open && rect_contains(app.queue_rect, x, y);
            if rect_contains(app.lyrics_rect, x, y) && !app.player.lyric_lines.is_empty() {
                app.sidebar_scroll = if down { app.sidebar_scroll + 2 } else { app.sidebar_scroll.saturating_sub(2) };
                app.lyrics_user_scroll = Some(std::time::Instant::now());
            } else if over_queue {
                let len = app.player.queue.len();
                app.queue_selected = if down { (app.queue_selected + 1).min(len.saturating_sub(1)) } else { app.queue_selected.saturating_sub(1) };
            } else if app.view == View::Settings {
                settings::scroll(app, down);
            } else {
                let step = if is_grid(app) { app.grid_cols.max(1) } else { 1 };
                let last = item_count(app).saturating_sub(1);
                app.content_selected = if down {
                    (app.content_selected + step).min(last)
                } else {
                    app.content_selected.saturating_sub(step)
                };
            }
        }
        _ => {}
    }
}

fn left_click(app: &mut TuiApp, x: u16, y: u16) {
    if app.help_open {
        app.help_open = false;
        app.needs_full_redraw = true;
        return;
    }
    if rect_contains(app.divider_rect, x, y) {
        app.divider_dragging = true;
        return;
    }
    if rect_contains(app.seek_bar_rect, x, y) && app.player.total_tracks > 0 {
        let frac = x.saturating_sub(app.seek_bar_rect.x) as f64 / app.seek_bar_rect.width.max(1) as f64;
        app.player.seek(frac * app.player.total_time);
        app.seek_dragging = true;
        return;
    }
    if rect_contains(app.vol_bar_rect, x, y) {
        let frac = (x.saturating_sub(app.vol_bar_rect.x) + 1) as f64 / app.vol_bar_rect.width.max(1) as f64;
        app.player.set_volume(frac.clamp(0.0, 1.0));
        app.vol_dragging = true;
        return;
    }
    if rect_contains(app.btn_prev, x, y) { prev_track(app); return; }
    if rect_contains(app.btn_play, x, y) { play_pause(app); return; }
    if rect_contains(app.btn_next, x, y) { next_track(app); return; }
    if rect_contains(app.btn_shuffle, x, y) { toggle_shuffle(app); return; }
    if rect_contains(app.btn_repeat, x, y) { cycle_repeat(app); return; }
    if rect_contains(app.btn_queue_clear, x, y) { clear_queue(app); return; }
    if rect_contains(app.btn_queue_close, x, y) || rect_contains(app.btn_queue_toggle, x, y) {
        toggle_queue(app);
        return;
    }
    if rect_contains(app.btn_lyrics_resync, x, y) { app.lyrics_user_scroll = None; return; }
    if rect_contains(app.btn_back, x, y) { go_back(app); return; }
    if rect_contains(app.btn_fwd, x, y) { go_forward(app); return; }
    if rect_contains(app.btn_search, x, y) { open_search(app); return; }
    if rect_contains(app.btn_view, x, y) { toggle_library_view(app); return; }

    let crumb = app.breadcrumb_rects.iter().position(|r| rect_contains(*r, x, y));
    if let Some(i) = crumb {
        match app.breadcrumb_nav_targets.get(i).copied() {
            Some(None) => {
                app.library.navigate_to_root();
                app.library.nodes_dirty = true;
                app.search = None;
                app.back_to_library();
            }
            Some(Some(nav_i)) if nav_i < app.library.nav_stack.len() => {
                app.library.nav_idx = nav_i;
                app.library.nodes_dirty = true;
                app.back_to_library();
            }
            _ => {}
        }
        return;
    }

    if let Some(rect) = app.cd_eject_rect {
        if rect_contains(rect, x, y) { eject(app); return; }
    }
    if app.view == View::Settings && settings::handle_click(app, x, y) { return; }

    let queue_hit = app.queue_item_rects.iter().position(|r| rect_contains(*r, x, y));
    if let Some(i) = queue_hit {
        let idx = app.queue_item_base + i;
        if app.queue_focus && app.queue_selected == idx {
            app.player.queue_play_at(idx);
            app.player.fetch_lyrics_for_current();
            app.queue_selected = 0;
        } else {
            app.queue_focus = true;
            app.queue_selected = idx;
        }
        return;
    }

    let item_hit = app.content_item_rects.iter().position(|r| rect_contains(*r, x, y));
    if let Some(i) = item_hit {
        app.queue_focus = false;
        if let Some(ref mut s) = app.search { s.editing = false; }
        app.content_selected = app.content_item_base + i;
        activate_selected(app);
        return;
    }

    let lyric_hit = app.lyric_item_rects.iter().position(|r| rect_contains(*r, x, y));
    if let Some(i) = lyric_hit {
        let li = app.lyric_row_lyric_idx.get(i).copied().unwrap_or(i);
        if let Some(&ts) = app.player.lyric_times.get(li).filter(|_| app.player.lyric_synced) {
            app.player.seek(ts);
            app.lyrics_user_scroll = None;
        }
    }
}
