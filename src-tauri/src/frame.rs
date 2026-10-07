//! Window geometry (DESIGN §11.3): resizing keeps the window's top edge where it
//! is. AppKit frames are bottom-left-origin (y grows upward), so a plain size change keeps the
//! *bottom* edge — expanding grows the window upward, and shrinking a hidden window drops its
//! top edge: every peek from hidden moved the pill down by (panel − pill) height. Pure math,
//! in AppKit points.

/// A rectangle in AppKit screen coordinates: origin bottom-left, y upward, points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    pub fn top(&self) -> f64 {
        self.y + self.h
    }
}

/// The smallest strip of the window that must stay on screen (the pill).
pub const MIN_VISIBLE_H: f64 = 56.0;

/// `frame` resized to `w × h` with its top-left corner kept. If `screen` (the visible frame,
/// without menu bar and Dock) is given, the corner is only nudged as far as needed to keep the
/// top below the menu bar, the pill on screen, and the window within the screen's width.
pub fn keep_top_edge(frame: Frame, w: f64, h: f64, screen: Option<Frame>) -> Frame {
    let mut x = frame.x;
    let mut top = frame.top();
    if let Some(s) = screen {
        top = top.min(s.top()).max(s.y + MIN_VISIBLE_H.min(h));
        x = x.min(s.x + s.w - w).max(s.x);
    }
    Frame {
        x,
        y: top - h,
        w,
        h,
    }
}

/// Initial placement: horizontally centred, `margin` points below the top of `screen`.
pub fn top_center(screen: Frame, w: f64, h: f64, margin: f64) -> Frame {
    let top = screen.top() - margin;
    Frame {
        x: screen.x + ((screen.w - w) / 2.0).max(0.0),
        y: top - h,
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1512×944 screen whose visible frame ends under a 38pt menu bar.
    const SCREEN: Frame = Frame {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 944.0,
    };

    fn pill_at(x: f64, top: f64) -> Frame {
        Frame {
            x,
            y: top - 56.0,
            w: 340.0,
            h: 56.0,
        }
    }

    #[test]
    fn expanding_keeps_the_top_left_corner() {
        let pill = pill_at(586.0, 934.0);
        let panel = keep_top_edge(pill, 340.0, 325.0, Some(SCREEN));
        assert_eq!((panel.x, panel.top()), (586.0, 934.0));
        assert_eq!(panel.y, 934.0 - 325.0);
    }

    #[test]
    fn collapsing_keeps_the_top_left_corner_too() {
        let panel = Frame {
            x: 586.0,
            y: 934.0 - 325.0,
            w: 340.0,
            h: 325.0,
        };
        assert_eq!(
            keep_top_edge(panel, 340.0, 56.0, Some(SCREEN)),
            pill_at(586.0, 934.0)
        );
    }

    #[test]
    fn ten_peeks_do_not_drift() {
        let start = pill_at(586.0, 934.0);
        let mut f = start;
        for _ in 0..10 {
            f = keep_top_edge(f, 340.0, 325.0, Some(SCREEN));
            f = keep_top_edge(f, 340.0, 56.0, Some(SCREEN));
        }
        assert_eq!(f, start);
    }

    #[test]
    fn a_plain_appkit_resize_is_what_drifted() {
        // AppKit's default keeps the origin (bottom-left): collapsing the panel drops the top
        // edge by the height difference — 269pt per peek with a 325pt panel.
        let panel = Frame {
            x: 586.0,
            y: 934.0 - 325.0,
            w: 340.0,
            h: 325.0,
        };
        let naive = Frame { h: 56.0, ..panel };
        assert_eq!(panel.top() - naive.top(), 269.0);
        assert_eq!(keep_top_edge(panel, 340.0, 56.0, None).top(), panel.top());
    }

    #[test]
    fn a_window_left_off_screen_is_pulled_back_minimally() {
        // Drifted below the screen by the old bug: the pill comes back at the bottom.
        let lost = pill_at(586.0, -1000.0);
        let f = keep_top_edge(lost, 340.0, 56.0, Some(SCREEN));
        assert_eq!((f.x, f.top()), (586.0, 56.0));
        // Above the menu bar: tucked under it. Off the right edge: back inside.
        let high = keep_top_edge(pill_at(1400.0, 1200.0), 340.0, 56.0, Some(SCREEN));
        assert_eq!((high.x, high.top()), (1512.0 - 340.0, 944.0));
    }

    #[test]
    fn without_a_screen_the_corner_is_exactly_kept() {
        let f = keep_top_edge(pill_at(-50.0, 2000.0), 340.0, 400.0, None);
        assert_eq!((f.x, f.top()), (-50.0, 2000.0));
    }

    #[test]
    fn initial_placement_is_top_center() {
        let f = top_center(SCREEN, 340.0, 56.0, 10.0);
        assert_eq!(f, pill_at(586.0, 934.0));
    }
}
