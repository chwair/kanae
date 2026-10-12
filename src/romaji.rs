//! romanization of japanese lyric lines (kana and kanji to hepburn romaji).
//! uses the pure-rust `kakasi` crate, so it works offline with no runtime data files.

use crate::lrclib::LyricLine;
use kakasi::IsJapanese;

/// true if the line contains any japanese kana, or kanji that are plausibly
/// japanese. pure-latin lines (e.g. an english chorus) are left untouched.
pub fn is_japanese(text: &str) -> bool {
    kakasi::is_japanese(text) != IsJapanese::False
}

/// romanize a single line if it looks japanese; otherwise return it verbatim.
pub fn romanize_line(text: &str) -> String {
    if is_japanese(text) {
        kakasi::convert(text).romaji
    } else {
        text.to_string()
    }
}

/// romanize each lyric line's text in place when `enabled` is set. timings are
/// left alone so synced highlighting keeps lining up with playback.
pub fn romanize_lines(lines: &mut [LyricLine], enabled: bool) {
    if !enabled {
        return;
    }
    for line in lines.iter_mut() {
        if is_japanese(&line.text) {
            line.text = kakasi::convert(&line.text).romaji;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_japanese() {
        assert!(is_japanese("こんにちは"));        // hiragana
        assert!(is_japanese("カタカナ"));          // katakana
        assert!(is_japanese("日本語"));            // kanji
        assert!(!is_japanese("hello world"));      // latin
        assert!(!is_japanese(""));
    }

    #[test]
    fn romanizes_japanese_only() {
        // kana/kanji become romaji...
        let r = romanize_line("こんにちは世界");
        assert!(r.to_lowercase().contains("konnichiha"));
        assert!(!r.contains('こ'));
        // ...while non-japanese lines pass through untouched.
        assert_eq!(romanize_line("Hello, world"), "Hello, world");
    }
}
