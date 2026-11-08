use ratatui::style::{Color, Modifier, Style};
use crate::library_cache::{TuiAccent, TuiTheme};

// kanae dark palette (mirrors the qml palette)
const CLR_SURF2: Color = Color::Rgb(30, 30, 30);
const CLR_BORDER: Color = Color::Rgb(40, 40, 40);
const CLR_BG: Color = Color::Rgb(10, 10, 10);
const CLR_TEXT: Color = Color::Rgb(223, 223, 223);
const CLR_TEXT2: Color = Color::Rgb(140, 140, 140);
const CLR_MUTED: Color = Color::Rgb(88, 88, 88);
const CLR_ACCENT: Color = Color::Rgb(191, 191, 191);

/// resolved styles for one frame. the terminal theme never names an rgb
/// colour: text uses the terminal's default foreground, hierarchy comes from
/// bold/dim, and the cursor row is drawn reversed, so every scheme the user
/// picked (light, dark, solarized...) keeps its own contrast.
#[derive(Clone, Copy)]
pub(super) struct Theme {
    /// painted behind the whole frame; None leaves the terminal background.
    pub bg:        Option<Color>,
    pub text:      Style,
    pub text2:     Style,
    pub muted:     Style,
    pub border:    Style,
    pub accent:    Style,
    /// style of the cursor row / selected tile.
    pub sel:       Style,
    /// when set, spans on a selected row drop their colours so the reversed
    /// row stays a single solid block.
    flat_sel:      bool,
    /// toasts and popups.
    pub surface:   Style,
    pub bar_fill:  Style,
    pub bar_empty: Style,
    shimmer:       [Style; 4],
    /// default foreground and background as rgb, when known, so text can be
    /// blended toward the background for gradients.
    fg_bg:         Option<((u8, u8, u8), (u8, u8, u8))>,
}

impl Theme {
    /// `term_colors` is the terminal's reported (foreground, background), used
    /// by the terminal theme for fades; without it fades fall back to dim.
    pub fn new(theme: TuiTheme, accent: TuiAccent, term_colors: Option<((u8, u8, u8), (u8, u8, u8))>) -> Self {
        match theme {
            TuiTheme::Terminal => {
                let accent = match ansi_accent(accent) {
                    Some(c) => Style::default().fg(c),
                    None    => Style::default().add_modifier(Modifier::BOLD),
                };
                let dim = Style::default().add_modifier(Modifier::DIM);
                Theme {
                    bg:        None,
                    text:      Style::default(),
                    text2:     Style::default(),
                    muted:     dim,
                    border:    dim,
                    accent,
                    sel:       Style::default().add_modifier(Modifier::REVERSED),
                    flat_sel:  true,
                    surface:   Style::default().add_modifier(Modifier::REVERSED),
                    bar_fill:  accent,
                    bar_empty: dim,
                    shimmer: [
                        Style::default().add_modifier(Modifier::BOLD),
                        Style::default(),
                        dim,
                        dim,
                    ],
                    fg_bg: term_colors,
                }
            }
            TuiTheme::Kanae => {
                let accent = Style::default().fg(match accent {
                    TuiAccent::Blue    => Color::Rgb(122, 162, 247),
                    TuiAccent::Cyan    => Color::Rgb(125, 207, 255),
                    TuiAccent::Green   => Color::Rgb(158, 206, 106),
                    TuiAccent::Yellow  => Color::Rgb(224, 175, 104),
                    TuiAccent::Magenta => Color::Rgb(187, 154, 247),
                    TuiAccent::Red     => Color::Rgb(247, 118, 142),
                    TuiAccent::Mono    => CLR_ACCENT,
                });
                Theme {
                    bg:        Some(CLR_BG),
                    text:      Style::default().fg(CLR_TEXT),
                    text2:     Style::default().fg(CLR_TEXT2),
                    muted:     Style::default().fg(CLR_MUTED),
                    border:    Style::default().fg(CLR_BORDER),
                    accent,
                    sel:       Style::default().bg(CLR_SURF2),
                    flat_sel:  false,
                    surface:   Style::default().fg(CLR_TEXT).bg(CLR_SURF2),
                    bar_fill:  Style::default().fg(Color::White),
                    bar_empty: Style::default().fg(CLR_BORDER),
                    shimmer: [
                        Style::default().fg(CLR_TEXT),
                        Style::default().fg(Color::Rgb(191, 191, 191)),
                        Style::default().fg(Color::Rgb(160, 160, 160)),
                        Style::default().fg(CLR_TEXT2),
                    ],
                    fg_bg: Some(((223, 223, 223), (10, 10, 10))),
                }
            }
        }
    }

    /// `s` as it should appear on a row that may be selected.
    pub fn on_row(&self, s: Style, selected: bool) -> Style {
        if selected && self.flat_sel {
            Style { fg: None, bg: None, ..s }
        } else {
            s
        }
    }

    /// default text at `amount` of its full strength (1.0 = unchanged, 0.0 =
    /// background). real blending when the colours are known, dim otherwise.
    pub fn fade(&self, amount: f32) -> Style {
        let amount = amount.clamp(0.0, 1.0);
        match self.fg_bg {
            Some(((fr, fg, fb), (br, bg, bb))) => {
                let mix = |f: u8, b: u8| (b as f32 + (f as f32 - b as f32) * amount).round() as u8;
                Style::default().fg(Color::Rgb(mix(fr, br), mix(fg, bg), mix(fb, bb)))
            }
            None if amount >= 0.75 => self.text,
            None => self.muted,
        }
    }

    /// shimmer style `dist` characters away from the sweep's peak.
    pub fn shimmer(&self, dist: usize) -> Style {
        self.shimmer[dist.min(3)]
    }
}

fn ansi_accent(a: TuiAccent) -> Option<Color> {
    match a {
        TuiAccent::Blue    => Some(Color::Blue),
        TuiAccent::Cyan    => Some(Color::Cyan),
        TuiAccent::Green   => Some(Color::Green),
        TuiAccent::Yellow  => Some(Color::Yellow),
        TuiAccent::Magenta => Some(Color::Magenta),
        TuiAccent::Red     => Some(Color::Red),
        TuiAccent::Mono    => None,
    }
}
