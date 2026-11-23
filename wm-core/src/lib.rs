//! wm-core — the pure tiling engine.
//!
//! Invariant: this crate never touches an OS handle, never does I/O,
//! never spawns a thread. Input: commands + platform events. Output:
//! `Effect` lists. Everything here runs identically on all three
//! targets and under `cargo test` on any machine.

pub mod command;
pub mod geometry;
pub mod state;
pub mod tree;
pub mod workspace;

pub use command::{Command, ParseError, WsSelector};
pub use geometry::{Direction, Orientation, Rect};
pub use state::{Effect, WmState, WORKSPACE_COUNT};
pub use tree::{Node, WindowId};
pub use workspace::{Gaps, Workspace};

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    const NO_GAPS: Gaps = Gaps { inner: 0, outer: 0 };

    fn rect_of(placements: &[(WindowId, Rect)], w: WindowId) -> Rect {
        placements
            .iter()
            .find(|(id, _)| *id == w)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("window {w} not placed"))
    }

    // ---- geometry ----------------------------------------------------

    #[test]
    fn split_respects_gap_and_ratio() {
        let r = Rect::new(0, 0, 1000, 500);
        let (a, b) = r.split(Orientation::Horizontal, 0.5, 10);
        assert_eq!(a, Rect::new(0, 0, 495, 500));
        assert_eq!(b, Rect::new(505, 0, 495, 500));
        assert_eq!(a.w + b.w + 10, 1000);

        let (t, btm) = r.split(Orientation::Vertical, 0.25, 0);
        assert_eq!(t, Rect::new(0, 0, 1000, 125));
        assert_eq!(btm, Rect::new(0, 125, 1000, 375));
    }

    #[test]
    fn shrink_never_inverts() {
        let r = Rect::new(0, 0, 10, 10);
        let s = r.shrink(500);
        assert!(s.w > 0 && s.h > 0);
    }

    // ---- workspace layout ---------------------------------------------

    #[test]
    fn single_window_fills_screen_minus_outer_gap() {
        let mut ws = Workspace::new();
        ws.insert(1, None);
        let p = ws.arrange(SCREEN, Gaps { inner: 8, outer: 12 });
        assert_eq!(p.len(), 1);
        assert_eq!(rect_of(&p, 1), Rect::new(12, 12, 1896, 1056));
    }

    #[test]
    fn auto_orientation_alternates_with_aspect() {
        let mut ws = Workspace::new();
        ws.insert(1, None);
        ws.arrange(SCREEN, NO_GAPS);

        // w1 is 1920x1080 (wide) -> horizontal split
        ws.insert(2, None);
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 1), Rect::new(0, 0, 960, 1080));
        assert_eq!(rect_of(&p, 2), Rect::new(960, 0, 960, 1080));

        // w2 is 960x1080 (tall) -> vertical split under it
        ws.insert(3, None);
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 1), Rect::new(0, 0, 960, 1080));
        assert_eq!(rect_of(&p, 2), Rect::new(960, 0, 960, 540));
        assert_eq!(rect_of(&p, 3), Rect::new(960, 540, 960, 540));
    }

    #[test]
    fn remove_collapses_parent_split() {
        let mut ws = Workspace::new();
        for w in 1..=3 {
            ws.insert(w, None);
            ws.arrange(SCREEN, NO_GAPS);
        }
        ws.remove(3);
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(p.len(), 2);
        assert_eq!(rect_of(&p, 1), Rect::new(0, 0, 960, 1080));
        assert_eq!(rect_of(&p, 2), Rect::new(960, 0, 960, 1080));

        ws.remove(2);
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 1), Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn remove_root_leaf_empties_workspace() {
        let mut ws = Workspace::new();
        ws.insert(1, None);
        ws.remove(1);
        assert!(ws.is_empty());
        assert_eq!(ws.focused, None);
    }

    // ---- focus navigation ----------------------------------------------

    fn three_window_ws() -> Workspace {
        // Layout: 1 = left half; 2 = top-right; 3 = bottom-right
        let mut ws = Workspace::new();
        for w in 1..=3 {
            ws.insert(w, None);
            ws.arrange(SCREEN, NO_GAPS);
        }
        ws
    }

    #[test]
    fn focus_direction_moves_geometrically() {
        let mut ws = three_window_ws();
        assert_eq!(ws.focused, Some(3)); // last inserted

        assert_eq!(ws.focus_direction(Direction::Up), Some(2));
        assert_eq!(ws.focus_direction(Direction::Left), Some(1));
        // from left half, right lands on an overlapping right-side window
        let right = ws.focus_direction(Direction::Right);
        assert!(right == Some(2) || right == Some(3));
        // screen edge returns None (Lua wrap hook uses this)
        ws.focused = Some(1);
        assert_eq!(ws.focus_direction(Direction::Left), None);
    }

    #[test]
    fn swap_direction_exchanges_tree_positions() {
        let mut ws = three_window_ws();
        ws.focused = Some(1); // left half
        assert!(ws.swap_direction(Direction::Right));
        let p = ws.arrange(SCREEN, NO_GAPS);
        // window 1 now occupies a right-side slot; its swap partner
        // took the left half.
        let r1 = rect_of(&p, 1);
        assert!(r1.x >= 960, "expected 1 on the right, got {r1:?}");
    }

    // ---- resize ---------------------------------------------------------

    #[test]
    fn resize_adjusts_nearest_matching_ancestor_and_clamps() {
        let mut ws = three_window_ws();
        ws.focused = Some(1);
        assert!(ws.resize(Direction::Right, 0.10)); // grow right = wider
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 1).w, 1152); // 0.6 * 1920

        // clamp at 0.9
        for _ in 0..20 {
            ws.resize(Direction::Right, 0.10);
        }
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 1).w, 1728); // 0.9 * 1920
    }

    #[test]
    fn resize_vertical_targets_the_inner_split() {
        let mut ws = three_window_ws();
        ws.focused = Some(2); // top-right
        assert!(ws.resize(Direction::Down, 0.20)); // grow downward
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(rect_of(&p, 2).h, 756); // 0.7 * 1080
        assert_eq!(rect_of(&p, 3).h, 324);
    }

    // ---- floating --------------------------------------------------------

    #[test]
    fn toggle_float_removes_from_tiling_and_back() {
        let mut ws = three_window_ws();
        ws.focused = Some(3);
        assert!(ws.toggle_float(3));
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(p.len(), 2); // 3 no longer tiled
        assert!(ws.is_floating(3));
        assert!(ws.all_windows().contains(&3));

        assert!(!ws.toggle_float(3)); // re-tile
        let p = ws.arrange(SCREEN, NO_GAPS);
        assert_eq!(p.len(), 3);
    }

    // ---- command parsing ---------------------------------------------------

    #[test]
    fn parses_glazewm_style_commands() {
        assert_eq!(
            Command::parse("focus --direction left").unwrap(),
            Command::Focus(Direction::Left)
        );
        assert_eq!(
            Command::parse("focus right").unwrap(),
            Command::Focus(Direction::Right)
        );
        assert_eq!(
            Command::parse("resize --direction down --amount 0.1").unwrap(),
            Command::Resize(Direction::Down, 0.1)
        );
        assert_eq!(
            Command::parse("workspace 3").unwrap(),
            Command::Workspace(WsSelector::Index(3))
        );
        assert_eq!(
            Command::parse("send 2").unwrap(),
            Command::MoveToWorkspace(WsSelector::Index(2))
        );
        // GlazeWM-style selectors
        assert_eq!(
            Command::parse("focus --next-workspace").unwrap(),
            Command::Workspace(WsSelector::Next)
        );
        assert_eq!(
            Command::parse("move --prev-workspace").unwrap(),
            Command::MoveToWorkspace(WsSelector::Prev)
        );
        assert_eq!(
            Command::parse("focus --recent-workspace").unwrap(),
            Command::Workspace(WsSelector::Recent)
        );
        assert_eq!(
            Command::parse("move --workspace 5").unwrap(),
            Command::MoveToWorkspace(WsSelector::Index(5))
        );
        assert_eq!(Command::parse("toggle-fullscreen").unwrap(), Command::ToggleFullscreen);
        assert_eq!(Command::parse("toggle-minimized").unwrap(), Command::Minimize);
        assert_eq!(Command::parse("wm-exit").unwrap(), Command::Exit);
        assert_eq!(Command::parse("wm-redraw").unwrap(), Command::Retile);
        assert_eq!(Command::parse("toggle-floating").unwrap(), Command::ToggleFloat);
        assert_eq!(
            Command::parse("call focus-or-wrap left").unwrap(),
            Command::Custom("focus-or-wrap".into(), vec!["left".into()])
        );
        // unknown verbs fall through to Lua instead of erroring
        assert_eq!(
            Command::parse("my-plugin-verb a b").unwrap(),
            Command::Custom("my-plugin-verb".into(), vec!["a".into(), "b".into()])
        );
        assert!(Command::parse("").is_err());
        assert!(Command::parse("focus --direction sideways").is_err());
        assert!(Command::parse("workspace 0").is_err());
    }

}
