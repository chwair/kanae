use std::hash::{Hash, Hasher};
use std::time::Duration;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use ratatui_image::{FilterType, Resize, StatefulImage};
use unicode_width::UnicodeWidthChar;

use crate::library_cache::TuiLibraryView;
use super::theme::Theme;
use super::thumbs::{cover_url_to_path, Thumb};
use super::{RepeatMode, TuiApp, TuiLibraryNode, View};

/// below this the layout cannot hold together, so a notice is shown instead.
const MIN_W: u16 = 40;
const MIN_H: u16 = 10;
/// the sidebar only appears when the content beside it keeps this much room.
const SIDEBAR_MIN_BODY: u16 = 64;
/// the queue docks on the right from this width; below it, it fills the body.
const QUEUE_DOCK_MIN: u16 = 72;
const QUEUE_W: u16 = 32;
const GRID_GAP: u16 = 2;
const GRID_TILE_PREF: u16 = 18;

// text helpers

pub(super) fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// "mm:ss" or "h:mm:ss" back to seconds; unparseable strings count as 0.
fn parse_duration(s: &str) -> u64 {
    s.split(':').try_fold(0u64, |acc, part| part.trim().parse::<u64>().ok().map(|v| acc * 60 + v))
        .unwrap_or(0)
}

/// display width of a string (cjk chars count as 2 columns).
pub(super) fn display_width(text: &str) -> usize {
    text.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// truncate a string to at most `max_cols` display columns.
pub(super) fn truncate_to_cols(text: &str, max_cols: usize) -> String {
    let mut result = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if col + cw > max_cols { break; }
        result.push(ch);
        col += cw;
    }
    result
}

/// keep the end of `text` (the useful part of a path), prefixed with an
/// ellipsis when it had to be cut.
pub(super) fn truncate_left(text: &str, max_cols: usize) -> String {
    if display_width(text) <= max_cols { return text.to_string(); }
    if max_cols == 0 { return String::new(); }
    let mut tail: Vec<char> = Vec::new();
    let mut col = 1usize;
    for ch in text.chars().rev() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if col + cw > max_cols { break; }
        tail.push(ch);
        col += cw;
    }
    let mut out = String::from("\u{2026}");
    out.extend(tail.into_iter().rev());
    out
}

/// `text` in exactly `width` columns: padded, or cut with an ellipsis.
fn fit(text: &str, width: usize) -> String {
    if width == 0 { return String::new(); }
    let dw = display_width(text);
    if dw <= width {
        return format!("{}{}", text, " ".repeat(width - dw));
    }
    let cut = truncate_to_cols(text, width - 1);
    let pad = width - 1 - display_width(&cut);
    format!("{}\u{2026}{}", cut, " ".repeat(pad))
}

/// scrolling marquee: if text fits in width return padded; otherwise scroll char-by-char.
/// uses display width so cjk characters are counted as 2 columns.
fn marquee(text: &str, width: usize, phase: usize) -> String {
    if width == 0 { return String::new(); }
    let dw = display_width(text);
    if dw <= width {
        return format!("{}{}", text, " ".repeat(width - dw));
    }
    // build (char, col_width) pairs for the text + a 3-space gap.
    let chars: Vec<(char, usize)> = text.chars()
        .chain("   ".chars())
        .map(|c| (c, UnicodeWidthChar::width(c).unwrap_or(1)))
        .collect();
    let start = phase % chars.len();
    let mut result = String::new();
    let mut col = 0usize;
    for &(ch, cw) in chars.iter().cycle().skip(start) {
        if col + cw > width { break; }
        result.push(ch);
        col += cw;
        if col >= width { break; }
    }
    // pad any remaining columns (can happen with wide chars near the edge).
    if col < width { result.push_str(&" ".repeat(width - col)); }
    result
}

/// the selected row scrolls long text; every other row is cut with an ellipsis,
/// so only one thing on screen moves at a time.
fn cell(text: &str, width: usize, selected: bool, phase: usize) -> String {
    if selected { marquee(text, width, phase) } else { fit(text, width) }
}

/// centre a string within `width` display columns.
fn center_text(text: &str, width: usize) -> String {
    let dw = display_width(text);
    if dw >= width { return truncate_to_cols(text, width); }
    let pad = (width - dw) / 2;
    format!("{}{}", " ".repeat(pad), text)
}

/// hard-wrap `text` into chunks of at most `width` display columns.
/// splits on spaces where possible; hard-wraps lone long words.
fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![String::new()]; }
    if display_width(text) <= width { return vec![text.to_owned()]; }
    let mut rows: Vec<String> = Vec::new();
    let mut cur  = String::new();
    let mut col  = 0usize;
    for word in text.split(' ') {
        let ww = display_width(word);
        if cur.is_empty() {
            if ww <= width {
                cur.push_str(word);
                col = ww;
            } else {
                hard_push_chars(word, width, &mut rows, &mut cur, &mut col);
            }
        } else if col + 1 + ww <= width {
            cur.push(' ');
            cur.push_str(word);
            col += 1 + ww;
        } else {
            rows.push(std::mem::take(&mut cur));
            col = 0;
            if ww <= width {
                cur.push_str(word);
                col = ww;
            } else {
                hard_push_chars(word, width, &mut rows, &mut cur, &mut col);
            }
        }
    }
    if !cur.is_empty() { rows.push(cur); }
    if rows.is_empty()  { rows.push(String::new()); }
    rows
}

/// append `text` char-by-char into `cur`, flushing to `rows` when `width` is reached.
fn hard_push_chars(text: &str, width: usize, rows: &mut Vec<String>, cur: &mut String, col: &mut usize) {
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if *col + cw > width {
            rows.push(std::mem::take(cur));
            *col = 0;
        }
        cur.push(ch);
        *col += cw;
    }
}

/// per-character shimmer: a highlight sweeps left to right across `text`
/// (the "Loading lyrics..." effect). `phase` advances the sweep.
fn shimmer_spans(t: &Theme, text: &str, phase: usize) -> Vec<Span<'static>> {
    let n = text.chars().count().max(1);
    let peak = phase % n;
    text.chars().enumerate().map(|(i, ch)| {
        let d = (i as isize - peak as isize).unsigned_abs();
        let dist = d.min(n.saturating_sub(d));
        Span::styled(ch.to_string(), t.shimmer(dist))
    }).collect()
}

fn hline(t: &Theme, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }
    frame.render_widget(
        Paragraph::new("\u{2500}".repeat(area.width as usize)).style(t.border),
        Rect { height: 1, ..area },
    );
}

fn vline(t: &Theme, frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("\u{2502}", t.border)))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// clamp the diff width of the cells the image widget just wrote. the graphics
/// protocols pack a whole escape sequence into one cell's symbol (sixel and iterm2 use
/// a single cell, kitty one per row), so ratatui measures that symbol's width, gets a
/// five-figure answer and advances the diff iterator past the end of the buffer,
/// silently dropping every later widget. `ForcedWidth` exists for exactly this; without
/// it the panel next to the album art never repaints.
fn pin_escape_cell_widths(frame: &mut Frame, area: Rect) {
    use ratatui::buffer::CellDiffOption;
    // any legitimate single-cell grapheme is far shorter than this; only the
    // protocol escape blobs clear the bar.
    const ESCAPE_MIN_LEN: usize = 32;
    let one = std::num::NonZeroU16::new(1).unwrap();
    let buf = frame.buffer_mut();
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                // leave the covered cells alone - they carry the widget's own
                // skip marker, and overriding it would paint over the image.
                if cell.diff_option == CellDiffOption::None
                    && cell.symbol().len() >= ESCAPE_MIN_LEN
                {
                    cell.set_diff_option(CellDiffOption::ForcedWidth(one));
                }
            }
        }
    }
}

/// covers are scaled both ways to fill their slot: the default `Fit` only ever
/// shrinks, which leaves small thumbnails floating in a big tile.
fn cover_widget() -> StatefulImage<ratatui_image::protocol::StatefulProtocol> {
    StatefulImage::new().resize(Resize::Scale(Some(FilterType::Triangle)))
}

/// the largest rect inside `area` holding an image of `aspect` (w/h), centred.
fn fit_image(area: Rect, aspect: f32, px_ratio: f32) -> Rect {
    let aspect = if aspect.is_finite() && aspect > 0.01 { aspect } else { 1.0 };
    let mut h = area.height;
    let mut w = ((h as f32) * aspect / px_ratio).round() as u16;
    if w > area.width {
        w = area.width;
        h = ((w as f32) * px_ratio / aspect).round().clamp(1.0, area.height as f32) as u16;
    }
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w.max(1),
        height: h.max(1),
    }
}

/// cell width / cell height in pixels; 0.5 when the terminal never said.
fn px_ratio(app: &TuiApp) -> f32 {
    app.picker.as_ref()
        .map(|p| { let s = p.font_size(); (s.0 as f32, s.1 as f32) })
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .map(|(w, h)| w / h)
        .unwrap_or(0.5)
}

// frame

pub(super) fn render(app: &mut TuiApp, frame: &mut Frame) {
    let area = frame.area();
    let t = app.theme;

    // hit-test rects are rebuilt every frame; anything not drawn must not
    // keep catching clicks from where it used to be.
    app.cd_eject_rect = None;
    app.content_item_rects.clear();
    app.lyric_item_rects.clear();
    app.lyric_row_lyric_idx.clear();
    app.breadcrumb_rects.clear();
    app.breadcrumb_nav_targets.clear();
    app.queue_item_rects.clear();
    app.queue_fullscreen = false;
    for r in [&mut app.btn_prev, &mut app.btn_play, &mut app.btn_next, &mut app.btn_back,
              &mut app.btn_fwd, &mut app.btn_search, &mut app.btn_view, &mut app.seek_bar_rect,
              &mut app.vol_bar_rect, &mut app.divider_rect, &mut app.queue_rect, &mut app.btn_shuffle,
              &mut app.btn_repeat, &mut app.btn_queue_clear, &mut app.btn_queue_close, &mut app.btn_queue_toggle, &mut app.lyrics_rect, &mut app.btn_lyrics_resync] {
        *r = Rect::default();
    }

    if let Some(bg) = t.bg {
        frame.render_widget(Block::default().style(t.text.bg(bg)), area);
    }

    if area.width < MIN_W || area.height < MIN_H {
        render_too_small(&t, frame, area);
        return;
    }

    let hints_h = if app.tui_settings.show_hints && area.height >= 16 { 1 } else { 0 };
    let [body, seek, controls, hints] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(hints_h),
    ]).areas(area);

    render_main(app, frame, body);
    render_seek_bar(app, frame, seek);
    render_controls(app, frame, controls);
    if hints_h > 0 { render_hints(app, frame, hints); }
    render_toasts(app, frame, body);
    if app.help_open { render_help(app, frame, area); }
}

fn render_too_small(t: &Theme, frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled("Terminal too small", t.text.add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("{}x{}, needs {}x{}", area.width, area.height, MIN_W, MIN_H), t.muted)),
    ];
    let y = area.y + area.height.saturating_sub(2) / 2;
    frame.render_widget(
        Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center),
        Rect { y, height: area.height.min(2), ..area },
    );
}

fn render_main(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let mut body = area;

    if app.queue_open {
        if area.width >= QUEUE_DOCK_MIN {
            let [rest, div, queue] = Layout::horizontal([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(QUEUE_W),
            ]).areas(area);
            vline(&t, frame, div);
            app.queue_rect = queue;
            render_queue_pane(app, frame, queue);
            body = rest;
        } else {
            // no room to dock: the queue takes over and owns the keys.
            app.queue_focus = true;
            app.queue_fullscreen = true;
            app.queue_rect = area;
            render_queue_pane(app, frame, area);
            return;
        }
    }

    let show_sidebar = app.tui_settings.show_sidebar && body.width >= SIDEBAR_MIN_BODY;
    if show_sidebar {
        let max_sw = (body.width / 2).min(body.width.saturating_sub(41));
        let sw = app.sidebar_w.clamp(18, max_sw.max(18));
        let [side, div, content] = Layout::horizontal([
            Constraint::Length(sw),
            Constraint::Length(1),
            Constraint::Min(0),
        ]).areas(body);
        app.divider_rect = div;
        vline(&t, frame, div);
        render_sidebar(app, frame, side);
        render_right_panel(app, frame, content);
    } else {
        render_right_panel(app, frame, body);
    }
}

// queue

fn render_queue_pane(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let n = app.player.queue.entries().len();
    if app.queue_selected >= n { app.queue_selected = n.saturating_sub(1); }

    let [header, rule, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ]).areas(area);

    let title_style = if app.queue_focus { t.accent.add_modifier(Modifier::BOLD) } else { t.text.add_modifier(Modifier::BOLD) };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Up Next", title_style),
            Span::styled(if n == 0 { String::new() } else { format!("  {}", n) }, t.muted),
        ])),
        header,
    );
    // header buttons, right-aligned: [Clear] [close]
    let close = format!(" {} ", app.icons.close);
    let close_w = display_width(&close) as u16;
    app.btn_queue_close = Rect { x: header.right().saturating_sub(close_w), width: close_w, ..header };
    frame.render_widget(Paragraph::new(Span::styled(close, t.text2)), app.btn_queue_close);
    if n > 0 {
        let label = " Clear ";
        let w = label.len() as u16;
        if header.width > w + close_w + 12 {
            app.btn_queue_clear = Rect { x: app.btn_queue_close.x - w, width: w, ..header };
            frame.render_widget(Paragraph::new(Span::styled(label, t.text2)), app.btn_queue_clear);
        }
    }
    hline(&t, frame, rule);

    if n == 0 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(" Nothing queued.", t.text2)),
                Line::raw(""),
                Line::from(vec![Span::styled(" a", t.accent), Span::styled(" adds the selected", t.muted)]),
                Line::from(Span::styled(" album or track.", t.muted)),
            ]),
            list_area,
        );
        return;
    }

    // two rows per entry (title, then artist/duration), scrolled to keep the
    // selection visible.
    let rows_per = 2usize;
    let visible = (list_area.height as usize / rows_per).max(1);
    let first = app.queue_selected.saturating_sub(visible.saturating_sub(1));
    let inner_w = list_area.width.saturating_sub(1) as usize;
    app.queue_item_base = first;

    let entries = app.player.queue.entries();
    for (row, (i, e)) in entries.iter().enumerate().skip(first).take(visible).enumerate() {
        let selected = app.queue_focus && i == app.queue_selected;
        let y = list_area.y + (row * rows_per) as u16;
        if y >= list_area.bottom() { break; }
        let rect = Rect { x: list_area.x, y, width: list_area.width, height: (rows_per as u16).min(list_area.bottom() - y) };
        let num = format!(" {:>2} ", i + 1);
        let title_w = inner_w.saturating_sub(display_width(&num));
        let sub = if e.artist.is_empty() { e.duration.clone() } else { format!("{}  {}", e.artist, e.duration) };
        let lines = vec![
            Line::from(vec![
                Span::styled(num, t.on_row(t.muted, selected)),
                Span::styled(cell(&e.title, title_w, selected, app.marquee_phase), t.on_row(t.text, selected)),
            ]),
            Line::from(Span::styled(format!("    {}", fit(&sub, inner_w.saturating_sub(4))), t.on_row(t.muted, selected))),
        ];
        let para = Paragraph::new(lines);
        frame.render_widget(if selected { para.style(t.sel) } else { para }, rect);
        app.queue_item_rects.push(rect);
    }
}

// sidebar

fn render_sidebar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let nothing_loaded = app.player.total_tracks <= 0;

    // cover: terminal cells are not square, so rows = cols x (cell_px_w /
    // cell_px_h) / cover_aspect. only drawn when there is room for lyrics too.
    let ratio = px_ratio(app);
    let inner_w = area.width.saturating_sub(2) as f32;
    // a degenerate aspect (a 1-pixel-wide cover) makes this ratio infinite,
    // which saturates the cast and overflows the `+ 2` below.
    let aspect = if app.cover_aspect.is_finite() && app.cover_aspect > 0.01 { app.cover_aspect } else { 1.0 };
    let want_cover = app.picker.is_some() && area.height >= 14 && !nothing_loaded;
    let cover_h = if want_cover {
        let inner_h = (inner_w * ratio / aspect).round().clamp(0.0, 512.0) as u16;
        (inner_h + 2).min(area.height * 11 / 20)
    } else { 0 };

    let meta_h = 3u16;
    let [cover_area, meta_area, rule, lyrics_area] = Layout::vertical([
        Constraint::Length(cover_h),
        Constraint::Length(meta_h),
        Constraint::Length(1),
        Constraint::Min(0),
    ]).areas(area);

    if cover_h > 0 {
        let block = Block::default().borders(Borders::ALL).border_style(t.border);
        let inner = block.inner(cover_area);
        frame.render_widget(block, cover_area);
        // during a sidebar drag, skip the stateful widget to avoid re-encoding on
        // every intermediate size; ratatui-image will re-encode once drag ends.
        // a zero-sized area makes the resize step divide by zero, so require real
        // room before handing the rect to the image widget.
        let can_draw = inner.width >= 2 && inner.height >= 2 && !app.divider_dragging;
        if let Some(ref mut proto) = app.cover_protocol {
            if can_draw {
                frame.render_stateful_widget(cover_widget(), inner, proto);
                pin_escape_cell_widths(frame, inner);
            }
        } else if inner.height >= 1 && inner.width >= 3 {
            let mid = Rect { y: inner.y + inner.height / 2, height: 1, ..inner };
            frame.render_widget(Paragraph::new(center_text(app.icons.album_ph, inner.width as usize)).style(t.muted), mid);
        }
    }

    let w = meta_area.width.saturating_sub(1) as usize;
    let meta = if nothing_loaded {
        vec![
            Line::raw(""),
            Line::from(Span::styled(fit(" Nothing playing", w), t.text2)),
            Line::from(Span::styled(fit(" Pick an album to start", w), t.muted)),
        ]
    } else {
        let p = &app.player;
        let sub = if app.cd_view_active || !p.is_file_mode { format!("{} {}", p.album_year, p.cd_tag()).trim().to_string() } else { p.album_year.clone() };
        let ph = app.marquee_phase;
        vec![
            Line::from(Span::styled(format!(" {}", marquee(&p.album_title, w.saturating_sub(1), ph)), t.text.add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(format!(" {}", marquee(&p.album_artist, w.saturating_sub(1), ph)), t.text2)),
            Line::from(Span::styled(format!(" {}", fit(&sub, w.saturating_sub(1))), t.muted)),
        ]
    };
    frame.render_widget(Paragraph::new(meta), meta_area);
    hline(&t, frame, rule);
    render_lyrics(app, frame, lyrics_area);
}

fn render_lyrics(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 || area.width < 4 { return; }
    let t = app.theme;
    app.lyrics_rect = area;

    // a new track, or a hand scroll left alone for a while, resumes following.
    if app.player.current_track != app.lyrics_track {
        app.lyrics_track = app.player.current_track;
        app.lyrics_user_scroll = None;
    }
    if app.lyrics_user_scroll.is_some_and(|at| at.elapsed() > Duration::from_secs(8)) {
        app.lyrics_user_scroll = None;
    }

    let pad = if area.width >= 16 { 2 } else { 1 };
    let inner = Rect { x: area.x + pad, width: area.width.saturating_sub(pad * 2), ..area };
    let w = inner.width as usize;
    let h = inner.height as usize;

    if app.player.lyric_lines.is_empty() {
        let mid = Rect { y: inner.y + inner.height / 2, height: 1, ..inner };
        if app.player.lyric_fetch_thread.is_some() {
            const LOADING_TEXT: &str = "Loading lyrics...";
            let lead = w.saturating_sub(LOADING_TEXT.len()) / 2;
            let mut spans = vec![Span::raw(" ".repeat(lead))];
            spans.extend(shimmer_spans(&t, LOADING_TEXT, app.lyric_shimmer_phase));
            frame.render_widget(Paragraph::new(Line::from(spans)), mid);
        } else if app.player.lyrics_fetch_done && app.player.total_tracks > 0 {
            frame.render_widget(Paragraph::new(center_text("No lyrics found", w)).style(t.muted), mid);
        }
        return;
    }

    // rows: (lyric index, text). a blank row separates lines when there is
    // room, like the gui's spacing; empty lyric lines (instrumental breaks)
    // show a note so the gap reads as intentional.
    let gap = h >= 10;
    let mut rows: Vec<(Option<usize>, String)> = Vec::new();
    for (li, line) in app.player.lyric_lines.iter().enumerate() {
        if gap && li > 0 { rows.push((None, String::new())); }
        if line.trim().is_empty() {
            rows.push((Some(li), app.icons.lyric_gap.to_string()));
        } else {
            for chunk in wrap_to_width(line.trim(), w) { rows.push((Some(li), chunk)); }
        }
    }

    let active = app.player.active_lyric_idx();
    let follow = active.max(0) as usize;
    if app.lyrics_user_scroll.is_none() {
        app.sidebar_scroll = rows.iter().position(|(li, _)| *li == Some(follow)).unwrap_or(0);
    }
    app.sidebar_scroll = app.sidebar_scroll.min(rows.len().saturating_sub(1));
    // the followed line sits on the middle row, with blank space above the
    // first line so it can centre too.
    let start = app.sidebar_scroll as isize - (h as isize - 1) / 2;

    let manual = app.lyrics_user_scroll.is_some();
    let visible_h = if manual && h > 3 { h - 1 } else { h };
    let fade_rows = (visible_h / 4).clamp(1, 4);
    let mut lines: Vec<Line> = Vec::with_capacity(visible_h);
    for screen_row in 0..visible_h {
        let idx = start + screen_row as isize;
        let row = if idx >= 0 { rows.get(idx as usize) } else { None };
        let Some((li, text)) = row else { lines.push(Line::raw("")); continue };
        let Some(li) = *li else { lines.push(Line::raw("")); continue };
        // lines fade with their distance from the current one, both ways, and
        // the rows nearest the pane's top and bottom edges fade further still.
        let dist = if active >= 0 { (li as i32 - active).unsigned_abs() } else { li as u32 + 1 };
        let mut strength: f32 = match dist { 0 => 1.0, 1 => 0.62, 2 => 0.52, _ => 0.44 };
        if text == app.icons.lyric_gap { strength = strength.min(0.45); }
        let edge = screen_row.min(visible_h - 1 - screen_row);
        if edge < fade_rows {
            strength *= (edge as f32 + 1.0) / (fade_rows as f32 + 1.0);
        }
        let mut style = t.fade(strength);
        if dist == 0 && active >= 0 { style = style.add_modifier(Modifier::BOLD); }
        lines.push(Line::from(Span::styled(center_text(text, w), style)));
        app.lyric_item_rects.push(Rect { x: inner.x, y: inner.y + screen_row as u16, width: inner.width, height: 1 });
        app.lyric_row_lyric_idx.push(li);
    }
    frame.render_widget(Paragraph::new(lines), inner);

    if manual && h > 3 {
        let label = " Back to current line ";
        let lw = (display_width(label) as u16).min(inner.width);
        let r = Rect { x: inner.x + (inner.width - lw) / 2, y: inner.bottom() - 1, width: lw, height: 1 };
        app.btn_lyrics_resync = r;
        frame.render_widget(Paragraph::new(Span::styled(truncate_to_cols(label, lw as usize), t.surface)), r);
    }
}

fn render_right_panel(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let [header, rule, content] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ]).areas(area);

    if app.search.is_some() && app.view == View::Library {
        render_search_bar(app, frame, header);
    } else {
        render_path_bar(app, frame, header);
    }
    hline(&t, frame, rule);

    match app.view {
        View::Library  => render_library(app, frame, content),
        View::Album    => render_track_list(app, frame, content),
        View::Settings => super::settings::render(app, frame, content),
    }
}

/// right-hand side of the header: the cursor position.
fn header_status(app: &TuiApp) -> Vec<Span<'static>> {
    let t = app.theme;
    let n = match app.view {
        View::Library  => app.visible_nodes().len(),
        View::Album    => app.effective_tracklist().len(),
        View::Settings => 0,
    };
    if n == 0 { return vec![]; }
    vec![Span::styled(format!("{}/{}", app.content_selected.min(n - 1) + 1, n), t.muted)]
}

fn render_search_bar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    app.btn_search = area;
    let Some(ref search) = app.search else { return };
    let editing = search.editing;
    let count = if search.query.trim().is_empty() {
        String::new()
    } else if search.total > search.results.len() {
        format!("{} of {} ", search.results.len(), search.total)
    } else {
        format!("{} result{} ", search.total, if search.total == 1 { "" } else { "s" })
    };
    let lead = format!(" {} ", app.icons.search);
    let avail = (area.width as usize).saturating_sub(display_width(&lead) + display_width(&count) + 2);
    let query = truncate_left(&search.query, avail);
    let mut spans = vec![
        Span::styled(lead, if editing { t.accent } else { t.muted }),
    ];
    if search.query.is_empty() && !editing {
        spans.push(Span::styled("Search", t.muted));
    } else {
        spans.push(Span::styled(query.clone(), t.text.add_modifier(Modifier::BOLD)));
    }
    if editing {
        spans.push(Span::styled(" ", t.text.add_modifier(Modifier::REVERSED)));
    }
    if search.query.is_empty() && editing {
        spans.push(Span::styled(" albums, artists, tracks", t.muted));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let cw = display_width(&count) as u16;
    if cw > 0 && cw < area.width {
        frame.render_widget(
            Paragraph::new(Span::styled(count, t.muted)),
            Rect { x: area.right() - cw, width: cw, ..area },
        );
    }
}

fn render_path_bar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let can_back = app.library.can_go_back() || app.view != View::Library;
    let can_fwd  = app.library.can_go_forward() && app.view == View::Library;

    // right side: status, then the search / layout buttons.
    let mut right: Vec<Span> = header_status(app);
    let mut right_w: u16 = right.iter().map(|s| display_width(&s.content) as u16).sum();
    let mut buttons: Vec<(Span, bool)> = Vec::new();
    if app.view == View::Library {
        let view_label = match app.tui_settings.library_view {
            TuiLibraryView::Grid => format!(" {} Grid ", app.icons.grid),
            TuiLibraryView::List => format!(" {} List ", app.icons.list),
        };
        buttons.push((Span::styled(format!(" {} ", app.icons.search), t.text2), true));
        buttons.push((Span::styled(view_label, t.text2), false));
    }
    let buttons_w: u16 = buttons.iter().map(|(s, _)| display_width(&s.content) as u16).sum();
    if right_w > 0 && buttons_w > 0 { right.push(Span::raw("  ")); right_w += 2; }

    let back_txt = format!(" {} ", app.icons.nav_back);
    let fwd_txt  = format!("{} ", app.icons.nav_fwd);
    let back_w   = display_width(&back_txt) as u16;
    let fwd_w    = display_width(&fwd_txt) as u16;
    app.btn_back = Rect { x: area.x, y: area.y, width: back_w, height: 1 };
    app.btn_fwd  = Rect { x: area.x + back_w, y: area.y, width: fwd_w, height: 1 };

    // breadcrumbs: (label, nav target). `Some(None)` = library root,
    // `Some(Some(i))` = nav stack entry, `None` = not clickable.
    let mut crumbs: Vec<(String, Option<Option<usize>>)> = vec![("Library".into(), Some(None))];
    match app.view {
        View::Library => {
            for i in 0..=app.library.nav_idx.min(app.library.nav_stack.len().saturating_sub(1)) {
                if app.library.nav_stack.is_empty() { break; }
                let name = app.library.nav_stack[i].file_name()
                    .and_then(|n| n.to_str()).unwrap_or("?").to_string();
                crumbs.push((name, Some(Some(i))));
            }
        }
        View::Album => {
            if app.cd_view_active {
                let label = if app.player.disc_count > 1 && app.player.disc_number > 0 {
                    format!("Audio CD ({}/{})", app.player.disc_number, app.player.disc_count)
                } else {
                    "Audio CD".to_string()
                };
                crumbs.push((label, None));
            } else {
                let album = if app.browse_dir.is_some() { app.browse_album_name.clone() } else { app.player.album_title.clone() };
                if app.is_external_file { crumbs.push(("Files".into(), None)); }
                if !album.is_empty() { crumbs.push((album, None)); }
            }
        }
        View::Settings => crumbs.push(("Settings".into(), None)),
    }

    let sep = format!(" {} ", app.icons.crumb);
    let sep_w = display_width(&sep);
    let avail = (area.width.saturating_sub(back_w + fwd_w + right_w + buttons_w + 1)) as usize;
    // drop crumbs after "Library" until the trail fits, marking the gap.
    let total = |c: &[(String, Option<Option<usize>>)]| -> usize {
        c.iter().map(|(l, _)| display_width(l)).sum::<usize>() + sep_w * c.len().saturating_sub(1)
    };
    let mut elided = false;
    while crumbs.len() > 2 && total(&crumbs) + if elided { 1 + sep_w } else { 0 } > avail {
        crumbs.remove(1);
        elided = true;
    }
    if elided { crumbs.insert(1, ("\u{2026}".into(), None)); }
    // still too long: shorten the last crumb.
    let over = total(&crumbs).saturating_sub(avail);
    if over > 0 {
        if let Some(last) = crumbs.last_mut() {
            let keep = display_width(&last.0).saturating_sub(over);
            last.0 = fit(&last.0, keep).trim_end().to_string();
        }
    }

    let mut spans: Vec<Span> = vec![
        Span::styled(back_txt, if can_back { t.text } else { t.muted }),
        Span::styled(fwd_txt,  if can_fwd  { t.text } else { t.muted }),
    ];
    let mut x = area.x + back_w + fwd_w;
    let last = crumbs.len() - 1;
    for (i, (label, target)) in crumbs.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep.clone(), t.muted));
            x += sep_w as u16;
        }
        let w = display_width(&label) as u16;
        let style = if i == last { t.text.add_modifier(Modifier::BOLD) } else if target.is_some() { t.text2 } else { t.muted };
        if let Some(target) = target {
            app.breadcrumb_rects.push(Rect { x, y: area.y, width: w, height: 1 });
            app.breadcrumb_nav_targets.push(target);
        }
        spans.push(Span::styled(label, style));
        x += w;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    // right-aligned status and buttons.
    let mut rx = area.right().saturating_sub(right_w + buttons_w);
    if right_w + buttons_w < area.width {
        frame.render_widget(Paragraph::new(Line::from(right)), Rect { x: rx, width: right_w, ..area });
        rx += right_w;
        for (span, is_search) in buttons {
            let w = display_width(&span.content) as u16;
            let r = Rect { x: rx, width: w, ..area };
            if is_search { app.btn_search = r; } else { app.btn_view = r; }
            frame.render_widget(Paragraph::new(span), r);
            rx += w;
        }
    }
}

// library

/// what a library item needs for drawing, copied out so the node list is not
/// borrowed while the thumbnail cache is.
struct ItemView {
    name:    String,
    sub:     String,
    year:    String,
    icon:    &'static str,
    cover:   Option<String>,
    is_cd:   bool,
    is_folder: bool,
    loading: bool,
}

fn item_view(app: &TuiApp, node: &TuiLibraryNode) -> ItemView {
    let cover = match node {
        TuiLibraryNode::Album { album } => album.cover_url.clone().filter(|c| !c.is_empty()),
        TuiLibraryNode::Cd { info } if !info.loading && !app.player.is_file_mode
            && !app.player.current_cover_url.is_empty() => Some(app.player.current_cover_url.clone()),
        _ => None,
    };
    ItemView {
        name:    node.name().to_string(),
        sub:     node.sub().to_string(),
        year:    node.year().to_string(),
        icon:    node.icon(&app.icons),
        cover:   cover.map(|c| cover_url_to_path(&c)),
        is_cd:   matches!(node, TuiLibraryNode::Cd { .. }),
        is_folder: matches!(node, TuiLibraryNode::Folder { .. }),
        loading: matches!(node, TuiLibraryNode::Cd { info } if info.loading),
    }
}

fn render_library(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let n = app.visible_nodes().len();
    if n == 0 {
        let searching = app.search.as_ref().is_some_and(|s| !s.query.trim().is_empty());
        let lines: Vec<Line> = if searching {
            let q = app.search.as_ref().map(|s| s.query.trim().to_string()).unwrap_or_default();
            vec![
                Line::from(Span::styled(format!("No matches for \"{}\"", q), t.text2)),
                Line::from(Span::styled("Try fewer words, or part of a name", t.muted)),
            ]
        } else if app.library.is_scanning {
            let spin = app.icons.spinner[app.lyric_shimmer_phase % app.icons.spinner.len()];
            vec![Line::from(vec![Span::styled(format!("{} ", spin), t.accent), Span::styled(app.library.scan_message.clone(), t.text2)])]
        } else if app.library.settings.search_paths.is_empty() {
            vec![
                Line::from(Span::styled("No music folder yet", t.text.add_modifier(Modifier::BOLD))),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Press ", t.muted), Span::styled("s", t.accent),
                    Span::styled(" for settings and add one, or paste a folder path here.", t.muted),
                ]),
            ]
        } else {
            vec![Line::from(Span::styled("No music found in your folders", t.text2))]
        };
        let y = area.y + area.height.saturating_sub(lines.len() as u16) / 3;
        frame.render_widget(
            Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center).wrap(ratatui::widgets::Wrap { trim: true }),
            Rect { y, height: area.bottom().saturating_sub(y), ..area },
        );
        return;
    }
    app.content_selected = app.content_selected.min(n - 1);
    match app.tui_settings.library_view {
        TuiLibraryView::List => render_library_list(app, frame, area),
        TuiLibraryView::Grid => render_library_grid(app, frame, area),
    }
}

fn render_library_list(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let n = app.visible_nodes().len();
    let visible_rows = (area.height as usize).max(1);
    let sel = app.content_selected;
    if sel < app.content_scroll { app.content_scroll = sel; }
    if sel >= app.content_scroll + visible_rows { app.content_scroll = sel + 1 - visible_rows; }
    app.content_scroll = app.content_scroll.min(n.saturating_sub(visible_rows));
    let scroll = app.content_scroll;
    app.content_item_base = scroll;
    app.page_items = visible_rows;
    app.grid_cols = 1;

    let views: Vec<ItemView> = app.visible_nodes().iter().skip(scroll).take(visible_rows)
        .map(|node| item_view(app, node)).collect();
    let w = area.width as usize;
    let wide = w >= 56;

    for (row, v) in views.into_iter().enumerate() {
        let idx = scroll + row;
        let y = area.y + row as u16;
        let rect = Rect { x: area.x, y, width: area.width, height: 1 };
        app.content_item_rects.push(rect);
        let selected = idx == sel;
        let r = |s| t.on_row(s, selected);

        let icon = format!(" {} ", v.icon);
        let icon_w = display_width(&icon);
        let tail = if v.is_cd { "Eject ".to_string() } else if v.year.is_empty() { String::new() } else { format!("{} ", v.year) };
        let tail_w = if v.is_cd { 6 } else { 5 };
        let text_w = w.saturating_sub(icon_w + tail_w + 1);
        let (name_w, sub_w) = if wide && !v.sub.is_empty() { (text_w * 11 / 20, text_w - text_w * 11 / 20) } else { (text_w, 0) };

        let mut spans = vec![Span::styled(icon, r(if v.is_cd || selected { t.accent } else { t.muted }))];
        let name = cell(&v.name, name_w, selected, app.marquee_phase);
        if v.loading {
            spans.extend(shimmer_spans(&t, &name, app.lyric_shimmer_phase));
        } else {
            spans.push(Span::styled(name, r(if selected { t.text.add_modifier(Modifier::BOLD) } else { t.text })));
        }
        if sub_w > 0 {
            spans.push(Span::styled(cell(&v.sub, sub_w, selected, app.marquee_phase), r(t.text2)));
        }
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("{:>width$}", tail, width = tail_w), r(if v.is_cd && selected { t.accent } else { t.muted })));
        if v.is_cd {
            app.cd_eject_rect = Some(Rect { x: area.right().saturating_sub(tail_w as u16), y, width: tail_w as u16, height: 1 });
        }
        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(if selected { para.style(t.sel) } else { para }, rect);
    }
}

fn render_library_grid(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let n = app.visible_nodes().len();
    let ratio = px_ratio(app);
    // one cell of margin all round, so the selected tile's outline has room.
    let area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    };
    if area.width < 4 || area.height < 3 { return; }

    let text_h = 2u16;
    // covers are square, so a tile's cover height follows from its width.
    // when that gets too tall for two rows to show, narrower tiles (more
    // columns) keep the covers square instead of squashing them.
    let two_rows = ((area.height + 1) / 2).saturating_sub(text_h + 1);
    let mut pref = GRID_TILE_PREF;
    if two_rows >= 5 {
        let max_w = ((two_rows as f32) / ratio).floor() as u16;
        pref = pref.min(max_w.max(10));
    }
    let mut cols = ((area.width + GRID_GAP) / (pref + GRID_GAP)).max(1);
    let mut tile_w = ((area.width + GRID_GAP) / cols).saturating_sub(GRID_GAP).max(1);
    // rounding the column count down widens tiles again; add a column back
    // when that pushes the cover past two rows.
    while two_rows >= 5 && ((tile_w as f32) * ratio).round() as u16 > two_rows && tile_w > 10 {
        cols += 1;
        tile_w = ((area.width + GRID_GAP) / cols).saturating_sub(GRID_GAP).max(1);
    }
    // leftover columns are split evenly to the sides so the grid is centred.
    let used = cols * tile_w + (cols - 1) * GRID_GAP;
    let left = area.x + area.width.saturating_sub(used) / 2;
    let mut cover_h = ((tile_w as f32) * ratio).round().max(2.0) as u16;
    if cover_h + text_h > area.height {
        cover_h = area.height.saturating_sub(text_h).max(1);
    }
    let tile_h = cover_h + text_h;
    let stride = tile_h + 1;
    let vis_rows = ((area.height + 1) / stride).max(1) as usize;
    let cols_us = cols as usize;
    let total_rows = n.div_ceil(cols_us);

    let sel_row = app.content_selected / cols_us;
    // content_scroll is a row index in this layout.
    if sel_row < app.content_scroll { app.content_scroll = sel_row; }
    if sel_row >= app.content_scroll + vis_rows { app.content_scroll = sel_row + 1 - vis_rows; }
    app.content_scroll = app.content_scroll.min(total_rows.saturating_sub(vis_rows));
    let first = app.content_scroll * cols_us;
    app.content_item_base = first;
    app.grid_cols = cols_us;
    app.page_items = vis_rows * cols_us;

    let views: Vec<ItemView> = app.visible_nodes().iter().skip(first).take(vis_rows * cols_us)
        .map(|node| item_view(app, node)).collect();
    let covers_on = app.tui_settings.grid_covers && app.picker.is_some();

    // graphics protocols leave stale pictures behind when tiles move; note
    // what is on screen so a change triggers a full repaint.
    if covers_on && app.uses_graphics() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (area, cols, cover_h, first).hash(&mut hasher);
        for v in &views { v.cover.hash(&mut hasher); }
        let sig = hasher.finish();
        if sig != app.grid_signature {
            app.grid_signature = sig;
            app.needs_full_redraw = true;
        }
    }

    for (i, v) in views.into_iter().enumerate() {
        let idx = first + i;
        let (r, c) = ((i / cols_us) as u16, (i % cols_us) as u16);
        let x = left + c * (tile_w + GRID_GAP);
        let y = area.y + r * stride;
        if y + tile_h > area.bottom() { break; }
        let selected = idx == app.content_selected;
        let tile = Rect { x, y, width: tile_w, height: tile_h };
        app.content_item_rects.push(tile);

        if selected {
            // outline in the margin around the tile, never over the cover.
            let outline = Rect { x: x - 1, y: y - 1, width: tile_w + 2, height: tile_h + 2 }
                .intersection(frame.area());
            frame.render_widget(
                Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(t.accent),
                outline,
            );
        }

        let cover_rect = Rect { x, y, width: tile_w, height: cover_h };
        let mut drew = false;
        if covers_on && !app.divider_dragging {
            if let Some(ref path) = v.cover {
                if let Thumb::Ready { proto, aspect } = app.thumbs.get(path) {
                    let fit_rect = fit_image(cover_rect, *aspect, ratio);
                    if fit_rect.width >= 2 && fit_rect.height >= 1 {
                        frame.render_stateful_widget(cover_widget(), fit_rect, proto);
                        pin_escape_cell_widths(frame, fit_rect);
                        drew = true;
                    }
                }
            }
        }
        if !drew {
            // placeholder: a quiet frame with the item's icon.
            // the selection outline already frames a selected tile.
            let inner = if selected {
                cover_rect
            } else {
                let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(t.border);
                let inner = block.inner(cover_rect);
                frame.render_widget(block, cover_rect);
                inner
            };
            if inner.height >= 1 {
                let icon_style = if selected || v.is_cd { t.accent } else { t.muted };
                let mid = inner.y + inner.height.saturating_sub(1) / 2;
                frame.render_widget(
                    Paragraph::new(center_text(v.icon, inner.width as usize)).style(icon_style),
                    Rect { y: mid, height: 1, ..inner },
                );
            }
        }

        let tw = tile_w as usize;
        let title = cell(&v.name, tw, selected, app.marquee_phase);
        let title_line = if v.loading {
            Line::from(shimmer_spans(&t, &title, app.lyric_shimmer_phase))
        } else if selected {
            Line::from(Span::styled(title, t.accent.add_modifier(Modifier::BOLD)))
        } else {
            Line::from(Span::styled(title, t.text))
        };
        let sub = match (v.sub.is_empty(), v.year.is_empty()) {
            _ if v.is_folder           => "Folder".to_string(),
            (false, false) if tw >= 16 => format!("{} \u{00B7} {}", v.sub, v.year),
            (false, _)                 => v.sub.clone(),
            (true, false)              => v.year.clone(),
            (true, true)               => String::new(),
        };
        let sub_line = Line::from(Span::styled(cell(&sub, tw, selected, app.marquee_phase), if selected { t.text2 } else { t.muted }));
        frame.render_widget(Paragraph::new(title_line), Rect { x, y: y + cover_h, width: tile_w, height: 1 });
        frame.render_widget(Paragraph::new(sub_line), Rect { x, y: y + cover_h + 1, width: tile_w, height: 1 });
    }
}

fn render_track_list(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;

    if app.player.show_loading() && app.browse_dir.is_none() && app.cd_view_active {
        let spin = app.icons.spinner[app.lyric_shimmer_phase % app.icons.spinner.len()];
        let mut spans = vec![Span::styled(format!(" {} ", spin), t.accent)];
        spans.extend(shimmer_spans(&t, "Reading CD...", app.lyric_shimmer_phase));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    let tracks = app.effective_tracklist();
    if tracks.is_empty() {
        let msg = if app.cd_view_active { app.player.disc_status.clone() } else { "This album has no playable tracks".into() };
        frame.render_widget(Paragraph::new(format!(" {}", msg)).style(t.text2), area);
        return;
    }

    // album header: title, then artist / year / count / length.
    let (title, artist, year) = if let (Some(_), Some(a)) = (&app.browse_dir, &app.browse_album) {
        (a.album.clone(), a.album_artist.clone(), a.year.clone())
    } else {
        (app.player.album_title.clone(), app.player.album_artist.clone(), app.player.album_year.clone())
    };
    let header_h = if area.height >= 8 { 3 } else { 0 };
    let [head, list] = Layout::vertical([Constraint::Length(header_h), Constraint::Min(0)]).areas(area);
    if header_h > 0 {
        let total: u64 = tracks.iter().map(|(_, _, d)| parse_duration(d)).sum();
        let mut sub: Vec<String> = Vec::new();
        if !artist.is_empty() { sub.push(artist.clone()); }
        if !year.is_empty() { sub.push(year); }
        if app.cd_view_active { sub.push(app.player.cd_tag()); }
        sub.push(format!("{} track{}", tracks.len(), if tracks.len() == 1 { "" } else { "s" }));
        if total > 0 { sub.push(fmt_time(total as f64)); }
        let w = head.width.saturating_sub(2) as usize;
        frame.render_widget(Paragraph::new(vec![
            Line::from(Span::styled(format!(" {}", fit(&title, w)), t.text.add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(format!(" {}", fit(&sub.join(" \u{00B7} "), w)), t.muted)),
        ]), head);
    }

    let n = tracks.len();
    let visible_rows = (list.height as usize).max(1);
    let sel = app.content_selected.min(n - 1);
    app.content_selected = sel;
    if sel < app.content_scroll { app.content_scroll = sel; }
    if sel >= app.content_scroll + visible_rows { app.content_scroll = sel + 1 - visible_rows; }
    app.content_scroll = app.content_scroll.min(n.saturating_sub(visible_rows));
    let scroll = app.content_scroll;
    app.content_item_base = scroll;
    app.page_items = visible_rows;
    app.grid_cols = 1;

    let cur = app.player.current_track;
    let has_browse = app.browse_dir.is_some();
    let num_w = n.to_string().len().max(2);
    let w = list.width as usize;

    for (row, (title, track_artist, dur)) in tracks.iter().enumerate().skip(scroll).take(visible_rows) {
        let y = list.y + (row - scroll) as u16;
        let rect = Rect { x: list.x, y, width: list.width, height: 1 };
        app.content_item_rects.push(rect);
        let is_current = !has_browse && row as i32 == cur;
        let selected = row == sel;
        let r = |s| t.on_row(s, selected);

        // the track artist only earns a column when it differs from the album's.
        let show_artist = !track_artist.is_empty() && *track_artist != artist && w >= 50;
        let lead = if is_current {
            let icon = if app.player.is_playing { app.icons.now_playing } else { app.icons.pause };
            format!(" {} ", fit(icon, num_w))
        } else {
            format!(" {:>width$} ", row + 1, width = num_w)
        };
        let lead_w = display_width(&lead);
        let dur_w = display_width(dur).max(5);
        let avail = w.saturating_sub(lead_w + dur_w + 2);
        let (title_w, artist_w) = if show_artist { (avail * 3 / 5, avail - avail * 3 / 5) } else { (avail, 0) };

        let title_style = if is_current {
            t.accent.add_modifier(Modifier::BOLD)
        } else if selected {
            t.text.add_modifier(Modifier::BOLD)
        } else {
            t.text
        };
        let mut spans = vec![
            Span::styled(lead, r(if is_current { t.accent } else { t.muted })),
            Span::styled(cell(title, title_w, selected, app.marquee_phase), r(title_style)),
        ];
        if artist_w > 0 {
            spans.push(Span::styled(cell(track_artist, artist_w, selected, app.marquee_phase), r(t.text2)));
        }
        spans.push(Span::styled(format!(" {:>width$} ", dur, width = dur_w), r(t.muted)));
        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(if selected { para.style(t.sel) } else { para }, rect);
    }
}

// footer

fn render_seek_bar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }
    let t = app.theme;
    let loaded = app.player.total_tracks > 0 && app.player.current_track >= 0;
    let cur   = app.player.current_time();
    let total = app.player.total_time.max(0.0);
    let frac  = if total > 0.0 { (cur / total).clamp(0.0, 1.0) } else { 0.0 };

    let (cur_s, tot_s) = if loaded { (fmt_time(cur), fmt_time(total)) } else { ("--:--".into(), "--:--".into()) };
    let side = display_width(&cur_s).max(display_width(&tot_s));
    let bar_w = (area.width as usize).saturating_sub(side * 2 + 4);
    let filled = ((frac * bar_w as f64) as usize).min(bar_w);
    app.seek_bar_rect = Rect { x: area.x + side as u16 + 2, y: area.y, width: bar_w as u16, height: 1 };

    let (head, rest) = if loaded && filled < bar_w { ("\u{257A}", bar_w - filled - 1) } else { ("", bar_w - filled) };
    let line = Line::from(vec![
        Span::styled(format!(" {:>width$} ", cur_s, width = side), if loaded { t.text } else { t.muted }),
        Span::styled("\u{2501}".repeat(filled), t.bar_fill),
        Span::styled(head, t.bar_empty),
        Span::styled("\u{2500}".repeat(rest), t.bar_empty),
        Span::styled(format!(" {:<width$} ", tot_s, width = side), t.muted),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_controls(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }
    let t = app.theme;
    let p = &app.player;
    let has_track = p.total_tracks > 0 && p.current_track >= 0;

    let can_prev = p.current_track > 0 || p.current_time() > 4.0;
    let can_next = has_track && (p.current_track + 1 < p.total_tracks || !p.queue.is_empty()
        || p.shuffle || p.repeat != RepeatMode::Off);
    let pp = if p.is_playing { app.icons.pause } else { app.icons.play };

    let prev_txt = format!(" {} ", app.icons.skip_prev);
    let pp_txt   = format!(" {} ", pp);
    let next_txt = format!(" {} ", app.icons.skip_next);
    let (prev_w, pp_w, next_w) = (display_width(&prev_txt) as u16, display_width(&pp_txt) as u16, display_width(&next_txt) as u16);
    app.btn_prev = Rect { x: area.x, y: area.y, width: prev_w, height: 1 };
    app.btn_play = Rect { x: area.x + prev_w, y: area.y, width: pp_w, height: 1 };
    app.btn_next = Rect { x: area.x + prev_w + pp_w, y: area.y, width: next_w, height: 1 };
    let transport_w = (prev_w + pp_w + next_w) as usize;

    // play order toggles, then volume; both dropped when the row gets tight.
    let shuffle_txt = format!(" {} ", app.icons.shuffle);
    let repeat_icon = if p.repeat == RepeatMode::One { app.icons.repeat_one } else { app.icons.repeat };
    let repeat_txt = format!(" {} ", repeat_icon);
    let queue_btn = format!(" {} ", app.icons.queue);
    let modes_w = display_width(&shuffle_txt) + display_width(&repeat_txt) + display_width(&queue_btn) + 1;

    let vol = p.volume_f64();
    let vol_bar_w = 8usize;
    let vol_filled = ((vol * vol_bar_w as f64).round() as usize).min(vol_bar_w);
    let vol_icon = format!("{} ", app.icons.volume);
    let pct = format!(" {:>3}% ", (vol * 100.0).round() as u32);
    let vol_w = display_width(&vol_icon) + vol_bar_w + pct.len();
    let show_vol = area.width as usize >= transport_w + modes_w + vol_w + 20;
    let show_modes = area.width as usize >= transport_w + modes_w + 16;
    let right_w = if show_vol { vol_w } else { 0 } + if show_modes { modes_w } else { 0 };

    let queued = p.queue.len();
    let queue_txt = if queued > 0 { format!("  +{} queued", queued) } else { String::new() };
    let label_w = (area.width as usize).saturating_sub(transport_w + 1 + right_w + display_width(&queue_txt));

    let mut spans = vec![
        Span::styled(prev_txt, if can_prev { t.text } else { t.muted }),
        Span::styled(pp_txt, if has_track { t.accent.add_modifier(Modifier::BOLD) } else { t.muted }),
        Span::styled(next_txt, if can_next { t.text } else { t.muted }),
        Span::raw(" "),
    ];
    if has_track {
        let i = p.current_track as usize;
        let title = p.track_titles.get(i).cloned().unwrap_or_default();
        let artist = p.track_artists.get(i).filter(|a| !a.is_empty()).cloned()
            .unwrap_or_else(|| p.album_artist.clone());
        let title_w = display_width(&title);
        if artist.is_empty() || title_w + 3 >= label_w {
            spans.push(Span::styled(marquee(&title, label_w, app.marquee_phase), t.text.add_modifier(Modifier::BOLD)));
        } else {
            spans.push(Span::styled(title, t.text.add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" \u{00B7} {}", fit(&artist, label_w - title_w - 3)), t.text2));
        }
    } else {
        spans.push(Span::styled(fit("Nothing playing", label_w), t.muted));
    }
    spans.push(Span::styled(queue_txt, t.muted));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let mut x = area.right() - right_w as u16;
    if show_modes {
        let on = |b: bool| if b { t.accent.add_modifier(Modifier::BOLD) } else { t.muted };
        let sw = display_width(&shuffle_txt) as u16;
        let rw = display_width(&repeat_txt) as u16;
        let qw = display_width(&queue_btn) as u16;
        app.btn_shuffle = Rect { x, width: sw, ..area };
        app.btn_repeat = Rect { x: x + sw, width: rw, ..area };
        app.btn_queue_toggle = Rect { x: x + sw + rw, width: qw, ..area };
        frame.render_widget(Paragraph::new(Line::from(vec![
            Span::styled(shuffle_txt, on(p.shuffle)),
            Span::styled(repeat_txt, on(p.repeat != RepeatMode::Off)),
            Span::styled(queue_btn, on(app.queue_open)),
        ])), Rect { x, width: sw + rw + qw, ..area });
        x += modes_w as u16;
    }
    if show_vol {
        app.vol_bar_rect = Rect { x: x + display_width(&vol_icon) as u16, y: area.y, width: vol_bar_w as u16, height: 1 };
        frame.render_widget(Paragraph::new(Line::from(vec![
            Span::styled(vol_icon, t.muted),
            Span::styled("\u{2501}".repeat(vol_filled), t.bar_fill),
            Span::styled("\u{2500}".repeat(vol_bar_w - vol_filled), t.bar_empty),
            Span::styled(pct, t.muted),
        ])), Rect { x, width: vol_w as u16, ..area });
    }
}

fn hint_pairs(app: &TuiApp) -> Vec<(&'static str, &'static str)> {
    if app.search.as_ref().is_some_and(|s| s.editing) && app.view == View::Library {
        return vec![("Enter", "results"), ("\u{2193}", "results"), ("Esc", "close"), ("Ctrl+U", "clear")];
    }
    if app.view == View::Settings && app.settings.input.is_some() {
        return vec![("Tab", "complete"), ("Enter", "add"), ("Esc", "cancel"), ("Ctrl+W", "up a folder")];
    }
    if app.queue_open && app.queue_focus {
        return vec![("\u{2191}\u{2193}", "select"), ("Enter", "play"), ("J/K", "reorder"), ("x", "remove"), ("C", "clear"), ("Tab", "back"), ("u", "hide")];
    }
    let queue_hint = ("u", if app.queue_open { "hide queue" } else { "show queue" });
    match app.view {
        View::Settings => vec![("\u{2191}\u{2193}", "move"), ("\u{2190}\u{2192}", "change"), ("Enter", "select"), ("Esc", "back")],
        View::Album => vec![("Space", "play/pause"), ("Enter", "play"), ("\u{2190}\u{2192}", "seek"), ("n/p", "next/prev"), ("a", "queue"), queue_hint, ("Bksp", "back"), ("z/r", "shuffle/repeat")],
        View::Library => {
            let toggle = match app.tui_settings.library_view {
                TuiLibraryView::Grid => ("v", "list"),
                TuiLibraryView::List => ("v", "grid"),
            };
            let mut v = vec![("Space", "play/pause"), ("Enter", "open"), ("/", "search"), toggle, ("a", "queue"), queue_hint, ("s", "settings")];
            if app.search.is_some() { v.insert(2, ("Esc", "clear search")); }
            v
        }
    }
}

fn render_hints(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let tail = [("?", "help"), ("q", "quit")];
    let pair_w = |(k, l): &(&str, &str)| display_width(k) + display_width(l) + 3;
    let tail_w: usize = tail.iter().map(pair_w).sum();
    let mut budget = (area.width as usize).saturating_sub(tail_w + 1);
    let mut spans = vec![Span::raw(" ")];
    for pair in hint_pairs(app) {
        let pw = pair_w(&pair);
        if pw > budget { break; }
        budget -= pw;
        spans.push(Span::styled(pair.0, t.accent));
        spans.push(Span::styled(format!(" {}  ", pair.1), t.muted));
    }
    spans.push(Span::raw(" ".repeat(budget)));
    for (k, l) in tail {
        spans.push(Span::styled(k, t.accent));
        spans.push(Span::styled(format!(" {}  ", l), t.muted));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// overlays

/// toasts in the gui's style: framed boxes stacked from the bottom-left,
/// the scan progress (with its spinner) lowest and messages above it.
fn render_toasts(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let mut items: Vec<Vec<Span>> = Vec::new();
    if app.library.is_scanning && !app.library.scan_message.is_empty() {
        let spin = app.icons.spinner[app.lyric_shimmer_phase % app.icons.spinner.len()];
        items.push(vec![
            Span::styled(format!("{} ", spin), t.accent),
            Span::styled(app.library.scan_message.clone(), t.accent),
        ]);
    }
    for (msg, at) in app.toasts.iter().rev() {
        if at.elapsed() >= Duration::from_secs(3) { continue; }
        items.push(vec![Span::styled(msg.clone(), t.text)]);
    }

    let max_w = area.width.saturating_sub(2) as usize;
    let fits = (area.height.saturating_sub(1) / 3) as usize;
    let mut sig = std::collections::hash_map::DefaultHasher::new();
    let mut y = area.bottom();
    for spans in items.into_iter().take(fits) {
        let text_w: usize = spans.iter().map(|s| display_width(&s.content)).sum();
        let w = (text_w + 4).min(max_w) as u16;
        if w < 6 { break; }
        y -= 3;
        let rect = Rect { x: area.x + 1, y, width: w, height: 3 };
        (rect, text_w).hash(&mut sig);
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(t.border);
        if let Some(bg) = t.surface.bg { block = block.style(Style::default().bg(bg)); }
        let inner = block.inner(rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(block, rect);
        frame.render_widget(Paragraph::new(Line::from(spans)), Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner });
    }

    // a toast drawn over (or vanishing from over) a graphic leaves pixels
    // the cell diff cannot fix.
    let sig = sig.finish();
    if sig != app.toast_signature {
        app.toast_signature = sig;
        if app.uses_graphics() { app.needs_full_redraw = true; }
    }
}

const HELP: &[(&str, &[(&str, &str)])] = &[
    ("Playback", &[
        ("Space", "play / pause"),
        ("n  p", "next / previous track"),
        (",  .", "seek 5s back / forward"),
        ("\u{2190}  \u{2192}", "seek (track list)"),
        ("+  -", "volume"),
        ("z", "shuffle"),
        ("r", "repeat off / all / one"),
        ("e", "eject disc"),
    ]),
    ("Browse", &[
        ("\u{2191}\u{2193}\u{2190}\u{2192}  hjkl", "move"),
        ("Enter", "open / play"),
        ("Bksp  Esc", "back"),
        ("Shift+\u{2190}\u{2192}", "history back / forward"),
        ("PgUp PgDn g G", "jump"),
        ("/", "search the library"),
        ("v", "grid / list layout"),
        ("b", "show / hide sidebar"),
    ]),
    ("Queue", &[
        ("a", "queue selection"),
        ("u", "show / hide queue"),
        ("Tab", "focus queue"),
        ("J  K", "reorder"),
        ("x", "remove from queue"),
        ("C", "clear queue"),
    ]),
    ("App", &[
        ("s", "settings"),
        ("?", "this help"),
        ("q", "quit"),
    ]),
];

fn help_lines(t: &Theme, sections: &[(&str, &[(&str, &str)])]) -> Vec<Line<'static>> {
    let key_w = 16usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, (section, keys)) in sections.iter().enumerate() {
        if i > 0 { lines.push(Line::raw("")); }
        lines.push(Line::from(Span::styled(format!(" {}", section.to_uppercase()), t.accent.add_modifier(Modifier::BOLD))));
        for (k, d) in keys.iter() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", fit(k, key_w)), t.text.add_modifier(Modifier::BOLD)),
                Span::styled(d.to_string(), t.text2),
            ]));
        }
    }
    lines
}

fn render_help(app: &TuiApp, frame: &mut Frame, area: Rect) {
    let t = app.theme;
    let single = help_lines(&t, HELP);
    // side by side when the terminal is wide but too short for one column.
    let two_col = area.width >= 96 && single.len() as u16 + 2 > area.height.saturating_sub(2);
    let (left, right) = if two_col {
        (help_lines(&t, &HELP[..2]), help_lines(&t, &HELP[2..]))
    } else {
        (single, Vec::new())
    };
    let col_w = 46u16;
    let w = if two_col { col_w * 2 + 2 } else { (area.width.saturating_sub(4)).min(col_w + 2) };
    let h = (area.height.saturating_sub(2)).min(left.len().max(right.len()) as u16 + 2);
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(t.accent)
        .title(Span::styled(" Keys ", t.text.add_modifier(Modifier::BOLD)))
        .title_bottom(Span::styled(" any key closes ", t.muted));
    if let Some(bg) = t.bg { block = block.style(Style::default().bg(bg)); }
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    if two_col {
        let [l, r] = Layout::horizontal([Constraint::Length(col_w), Constraint::Min(0)]).areas(inner);
        frame.render_widget(Paragraph::new(left), l);
        frame.render_widget(Paragraph::new(right), r);
    } else {
        frame.render_widget(Paragraph::new(left), inner);
    }
}
