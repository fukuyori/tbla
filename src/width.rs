//! Display-width helpers with East Asian Ambiguous width support.
//!
//! Plain CJK characters (あ, 漢, カ) are unambiguously double-width and were
//! always handled, but East Asian *Ambiguous* characters (①, ○, →, ─, ※,
//! Greek letters, …) render as double width in many Japanese terminal
//! setups and single width elsewhere. Which one is in effect is probed at
//! startup (see `main`), and every display-width computation in the app
//! goes through these helpers so the whole UI agrees on one answer.

use std::sync::atomic::{AtomicBool, Ordering};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static AMBIGUOUS_WIDE: AtomicBool = AtomicBool::new(false);

pub fn set_ambiguous_wide(wide: bool) {
    AMBIGUOUS_WIDE.store(wide, Ordering::Relaxed);
}

pub fn ambiguous_wide() -> bool {
    AMBIGUOUS_WIDE.load(Ordering::Relaxed)
}

/// Display width of a single char under the current ambiguous-width mode.
pub fn char_width(c: char) -> usize {
    if ambiguous_wide() {
        UnicodeWidthChar::width_cjk(c).unwrap_or(1)
    } else {
        UnicodeWidthChar::width(c).unwrap_or(1)
    }
}

/// Display width of a string under the current ambiguous-width mode.
pub fn str_width(s: &str) -> usize {
    if ambiguous_wide() {
        UnicodeWidthStr::width_cjk(s)
    } else {
        UnicodeWidthStr::width(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single test so the global flag isn't raced by parallel test threads.
    #[test]
    fn ambiguous_mode_switches_widths() {
        set_ambiguous_wide(false);
        assert_eq!(str_width("あいう"), 6); // plain CJK: always wide
        assert_eq!(str_width("abc"), 3);
        assert_eq!(str_width("①○→…"), 4); // ambiguous: narrow by default

        set_ambiguous_wide(true);
        assert_eq!(str_width("あいう"), 6);
        assert_eq!(str_width("abc"), 3);
        assert_eq!(str_width("①○→…"), 8); // ambiguous: wide in CJK mode
        assert_eq!(char_width('…'), 2);

        set_ambiguous_wide(false);
    }
}
