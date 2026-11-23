//! The reducer. `WmState` is the single source of truth; the daemon
//! feeds it commands and platform events, and gets back a list of
//! `Effect`s to hand to the platform backend and overlay.
//!
//! MULTI-MONITOR MODEL:
//! - `monitors` holds each display's work-area rect (index 0 = primary).
//! - Each workspace is pinned to a monitor (`ws_to_monitor`), assigned
//!   round-robin so with 3 monitors: ws 1,4,7 -> mon 0; 2,5,8 -> mon 1;
//!   3,6,9 -> mon 2.
//! - Each monitor always has exactly one VISIBLE workspace (`visible`);
//!   `active` is the focused workspace among the visible ones.
//! - Adoption is non-destructive: existing windows are matched to the
//!   monitor that contains their center and join that monitor's visible
//!   workspace, so nothing jumps displays on boot.

use crate::command::Command;
use crate::geometry::Rect;
use crate::tree::WindowId;
use crate::workspace::{Gaps, Workspace};

pub const WORKSPACE_COUNT: usize = 9;

/// What the outside world must do after a state transition.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Move/resize these windows (batched).
    Apply(Vec<(WindowId, Rect)>),
    /// Give this window input focus.
    Focus(WindowId),
    /// Hide (workspace switched away).
    Hide(Vec<WindowId>),
    /// Show (workspace switched to).
    Show(Vec<WindowId>),
    /// Ask the OS to close this window.
    Close(WindowId),
    /// Ask the OS to minimize this window.
    Minimize(WindowId),
    /// Verb the core doesn't know — hand to the Lua command table.
    Lua(String, Vec<String>),
    /// Redraw borders/indicators (focus or geometry changed).
    RedrawOverlay,
    /// Daemon should exit; always preceded by Show effects that un-hide
    /// every managed window so none are stranded.
    Shutdown,
}

#[derive(Debug)]
pub struct WmState {
    pub workspaces: Vec<Workspace>,
    pub active: usize, // focused workspace (0-based)
    pub gaps: Gaps,
    pub monitors: Vec<Rect>,
    pub ws_to_monitor: Vec<usize>,
    /// Per monitor: which workspace index is currently on-screen.
    pub visible: Vec<usize>,
    /// Previously active workspace (for `focus --recent-workspace`).
    pub recent: usize,
    /// At most one fullscreen window; arranged over its whole monitor.
    pub fullscreen: Option<WindowId>,
}

impl Default for WmState {
    fn default() -> Self {
        let mut st = WmState {
            workspaces: (0..WORKSPACE_COUNT).map(|_| Workspace::new()).collect(),
            active: 0,
            gaps: Gaps::default(),
            monitors: Vec::new(),
            ws_to_monitor: vec![0; WORKSPACE_COUNT],
            visible: Vec::new(),
            recent: 0,
            fullscreen: None,
        };
        st.update_monitors(vec![Rect::new(0, 0, 1920, 1080)]);
        st
    }
}

impl WmState {
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)declare the physical displays. Workspaces are redistributed
    /// round-robin and each monitor gets its lowest workspace visible.
    /// Called at boot and on display-change events.
    pub fn update_monitors(&mut self, new_monitors: Vec<Rect>) -> Vec<Effect> {
        if new_monitors.is_empty() {
            return Vec::new();
        }
        tracing::info!(count = new_monitors.len(), "monitor layout updated");
        self.monitors = new_monitors;
        let n = self.monitors.len();
        for (i, m) in self.ws_to_monitor.iter_mut().enumerate() {
            *m = i % n;
        }
        // Monitor m's lowest workspace is m itself under round-robin.
        self.visible = (0..n).collect();
        if !self.visible.contains(&self.active) {
            self.active = self.visible[0];
        }
        self.arrange_all_visible()
    }

    // ---- accessors --------------------------------------------------------

    pub fn active_ws(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    pub fn active_ws_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active]
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.active_ws().focused
    }

    pub fn workspace_of(&self, w: WindowId) -> Option<usize> {
        self.workspaces.iter().position(|ws| ws.contains(w))
    }

    fn monitor_rect_of_ws(&self, ws: usize) -> Rect {
        let m = self.ws_to_monitor[ws];
        self.monitors
            .get(m)
            .copied()
            .unwrap_or_else(|| Rect::new(0, 0, 1920, 1080))
    }

    fn is_visible(&self, ws: usize) -> bool {
        self.visible.contains(&ws)
    }

    /// Which monitor's rect contains this point? Falls back to primary.
    pub fn monitor_at(&self, x: i32, y: i32) -> usize {
        self.monitors
            .iter()
            .position(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
            .unwrap_or(0)
    }

    // ---- arranging --------------------------------------------------------

    fn arrange_ws(&mut self, ws: usize) -> Vec<Effect> {
        let screen = self.monitor_rect_of_ws(ws);
        let gaps = self.gaps;
        let mut placements = self.workspaces[ws].arrange(screen, gaps);
        // Fullscreen override: the fullscreen window (if it lives here)
        // covers its entire monitor, gaps and all.
        if let Some(fw) = self.fullscreen {
            if let Some(slot) = placements.iter_mut().find(|(w, _)| *w == fw) {
                slot.1 = screen;
            }
        }
        let mut fx = Vec::new();
        if !placements.is_empty() {
            fx.push(Effect::Apply(placements));
        }
        fx.push(Effect::RedrawOverlay);
        fx
    }

    fn arrange_active(&mut self) -> Vec<Effect> {
        self.arrange_ws(self.active)
    }

    fn arrange_all_visible(&mut self) -> Vec<Effect> {
        let visible = self.visible.clone();
        let mut fx = Vec::new();
        for ws in visible {
            fx.extend(self.arrange_ws(ws));
        }
        fx
    }

    // ---- adoption (state-aware startup) ------------------------------------

    /// Adopt a pre-existing window WITHOUT vacuuming it to the primary
    /// display: it joins the visible workspace of whichever monitor
    /// contains its center, and only that monitor re-tiles.
    pub fn adopt_window(&mut self, w: WindowId, current: Rect) -> Vec<Effect> {
        if self.workspace_of(w).is_some() {
            return Vec::new();
        }
        let (cx, cy) = current.center();
        let monitor = self.monitor_at(cx, cy);
        let ws = self.visible[monitor];
        tracing::debug!(window = w, monitor, workspace = ws + 1, "adopting in place");
        self.workspaces[ws].insert(w, None);
        self.arrange_ws(ws)
    }

    // ---- platform events ----------------------------------------------------

    /// A NEW window appeared: it joins the focused (active) workspace.
    pub fn on_window_created(&mut self, w: WindowId) -> Vec<Effect> {
        if self.workspace_of(w).is_some() {
            return Vec::new(); // duplicate event; platforms do that.
        }
        tracing::debug!(window = w, workspace = self.active + 1, "window created");
        self.active_ws_mut().insert(w, None);
        let mut fx = self.arrange_active();
        fx.push(Effect::Focus(w));
        fx
    }

    pub fn on_window_destroyed(&mut self, w: WindowId) -> Vec<Effect> {
        let Some(idx) = self.workspace_of(w) else {
            return Vec::new();
        };
        tracing::debug!(window = w, workspace = idx + 1, "window destroyed");
        self.workspaces[idx].remove(w);
        if self.fullscreen == Some(w) {
            self.fullscreen = None;
        }
        if !self.is_visible(idx) {
            return Vec::new(); // invisible workspace; re-arranged on switch
        }
        let mut fx = self.arrange_ws(idx);
        if idx == self.active {
            if let Some(f) = self.focused_window() {
                fx.push(Effect::Focus(f));
            }
        }
        fx
    }

    /// The OS told us focus changed (user clicked a window, possibly on
    /// another monitor) — follow it.
    pub fn on_window_focused(&mut self, w: WindowId) -> Vec<Effect> {
        if let Some(idx) = self.workspace_of(w) {
            self.workspaces[idx].focused = Some(w);
            if idx != self.active && self.is_visible(idx) {
                self.recent = self.active;
                self.active = idx;
            }
            if self.is_visible(idx) {
                return vec![Effect::RedrawOverlay];
            }
        }
        Vec::new()
    }

    // ---- commands -----------------------------------------------------------

    pub fn apply(&mut self, cmd: Command) -> Vec<Effect> {
        tracing::info!(?cmd, "applying command");
        match cmd {
            Command::Focus(dir) => match self.active_ws_mut().focus_direction(dir) {
                Some(w) => vec![Effect::Focus(w), Effect::RedrawOverlay],
                None => Vec::new(), // screen edge; Lua may wrap
            },
            Command::Move(dir) => {
                if self.active_ws_mut().swap_direction(dir) {
                    self.arrange_active()
                } else {
                    Vec::new()
                }
            }
            Command::Resize(dir, amount) => {
                if self.active_ws_mut().resize(dir, amount) {
                    self.arrange_active()
                } else {
                    Vec::new()
                }
            }
            Command::ToggleFloat => {
                let Some(f) = self.focused_window() else {
                    return Vec::new();
                };
                self.active_ws_mut().toggle_float(f);
                self.arrange_active()
            }
            Command::ToggleSplit => {
                if self.active_ws_mut().toggle_split() {
                    self.arrange_active()
                } else {
                    Vec::new()
                }
            }
            Command::ToggleFullscreen => {
                let Some(f) = self.focused_window() else {
                    return Vec::new();
                };
                self.fullscreen = if self.fullscreen == Some(f) { None } else { Some(f) };
                self.arrange_active()
            }
            Command::Minimize => {
                let Some(f) = self.focused_window() else {
                    return Vec::new();
                };
                // Leave the tree; the un-minimize event re-adopts it.
                let mut fx = self.on_window_destroyed(f);
                fx.insert(0, Effect::Minimize(f));
                fx
            }
            Command::Workspace(sel) => {
                let t = self.resolve(sel);
                self.switch_workspace(t)
            }
            Command::MoveToWorkspace(sel) => {
                let t = self.resolve(sel);
                self.send_to_workspace(t)
            }
            Command::Close => self
                .focused_window()
                .map(|w| vec![Effect::Close(w)])
                .unwrap_or_default(),
            Command::Retile => self.arrange_all_visible(),
            Command::Exit => {
                // Restore every managed window before dying — killing the
                // daemon must never strand hidden windows.
                let all: Vec<WindowId> = self
                    .workspaces
                    .iter()
                    .flat_map(|ws| ws.all_windows())
                    .collect();
                let mut fx = Vec::new();
                if !all.is_empty() {
                    fx.push(Effect::Show(all));
                }
                fx.push(Effect::Shutdown);
                fx
            }
            Command::Custom(name, args) => vec![Effect::Lua(name, args)],
        }
    }

    /// Turn a workspace selector into a 0-based index.
    fn resolve(&self, sel: crate::command::WsSelector) -> usize {
        use crate::command::WsSelector::*;
        match sel {
            Index(n) => (n - 1).min(WORKSPACE_COUNT - 1),
            Next => (self.active + 1) % WORKSPACE_COUNT,
            Prev => (self.active + WORKSPACE_COUNT - 1) % WORKSPACE_COUNT,
            Recent => self.recent,
        }
    }

    fn switch_workspace(&mut self, target: usize) -> Vec<Effect> {
        if target >= self.workspaces.len() || target == self.active {
            return Vec::new();
        }
        let mut fx = Vec::new();
        if self.is_visible(target) {
            // Already on-screen (another monitor): just move focus there.
            self.recent = self.active;
            self.active = target;
        } else {
            // Swap it onto its own monitor, hiding what that monitor shows.
            let monitor = self.ws_to_monitor[target];
            let old = self.visible[monitor];
            let hide = self.workspaces[old].all_windows();
            if !hide.is_empty() {
                fx.push(Effect::Hide(hide));
            }
            self.visible[monitor] = target;
            let show = self.workspaces[target].all_windows();
            if !show.is_empty() {
                fx.push(Effect::Show(show));
            }
            self.recent = self.active;
            self.active = target;
            fx.extend(self.arrange_ws(target));
        }
        if let Some(f) = self.focused_window() {
            fx.push(Effect::Focus(f));
        }
        fx.push(Effect::RedrawOverlay);
        fx
    }

    fn send_to_workspace(&mut self, target: usize) -> Vec<Effect> {
        if target >= self.workspaces.len() || target == self.active {
            return Vec::new();
        }
        let Some(w) = self.focused_window() else {
            return Vec::new();
        };
        self.active_ws_mut().remove(w);
        self.workspaces[target].insert(w, None);
        let mut fx = Vec::new();
        if self.is_visible(target) {
            fx.extend(self.arrange_ws(target)); // lands on another monitor
        } else {
            fx.push(Effect::Hide(vec![w]));
        }
        fx.extend(self.arrange_active());
        if let Some(f) = self.focused_window() {
            fx.push(Effect::Focus(f));
        }
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::WsSelector;
    use crate::geometry::Direction;

    // Three-monitor rig: two 1920x1080 side by side + a laptop panel.
    fn three_monitor_state() -> WmState {
        let mut st = WmState::new();
        st.gaps = Gaps { inner: 0, outer: 0 };
        st.update_monitors(vec![
            Rect::new(0, 0, 1920, 1080),     // primary
            Rect::new(1920, 0, 1920, 1080),  // right
            Rect::new(3840, 200, 1280, 720), // laptop
        ]);
        st
    }

    fn apply_rect(fx: &[Effect], w: WindowId) -> Option<Rect> {
        fx.iter().find_map(|e| match e {
            Effect::Apply(p) => p.iter().find(|(id, _)| *id == w).map(|(_, r)| *r),
            _ => None,
        })
    }

    // ---- insertion / deletion / floating (the requested foundation) ----

    #[test]
    fn insertion_tiles_and_focuses() {
        let mut st = WmState::new();
        st.gaps = Gaps { inner: 0, outer: 0 };
        let fx = st.on_window_created(10);
        assert_eq!(apply_rect(&fx, 10), Some(Rect::new(0, 0, 1920, 1080)));
        assert!(fx.contains(&Effect::Focus(10)));

        let fx = st.on_window_created(11);
        assert_eq!(apply_rect(&fx, 10), Some(Rect::new(0, 0, 960, 1080)));
        assert_eq!(apply_rect(&fx, 11), Some(Rect::new(960, 0, 960, 1080)));

        // duplicate create events are ignored
        assert!(st.on_window_created(10).is_empty());
    }

    #[test]
    fn deletion_retiles_and_refocuses() {
        let mut st = WmState::new();
        st.gaps = Gaps { inner: 0, outer: 0 };
        st.on_window_created(10);
        st.on_window_created(11);
        let fx = st.on_window_destroyed(11);
        assert_eq!(apply_rect(&fx, 10), Some(Rect::new(0, 0, 1920, 1080)));
        assert_eq!(st.focused_window(), Some(10));
        // destroying an unknown window is a no-op
        assert!(st.on_window_destroyed(999).is_empty());
    }

    #[test]
    fn floating_removes_from_tiling_and_back() {
        let mut st = WmState::new();
        st.gaps = Gaps { inner: 0, outer: 0 };
        st.on_window_created(10);
        st.on_window_created(11);
        st.apply(Command::ToggleFloat);
        assert!(st.active_ws().is_floating(11));
        let fx = st.apply(Command::Retile);
        assert_eq!(apply_rect(&fx, 10), Some(Rect::new(0, 0, 1920, 1080)));
        st.apply(Command::ToggleFloat); // re-tile 11
        assert!(!st.active_ws().is_floating(11));
    }

    // ---- multi-monitor + adoption -------------------------------------

    #[test]
    fn workspaces_distribute_round_robin() {
        let st = three_monitor_state();
        assert_eq!(st.ws_to_monitor, vec![0, 1, 2, 0, 1, 2, 0, 1, 2]);
        assert_eq!(st.visible, vec![0, 1, 2]);
    }

    #[test]
    fn adoption_keeps_windows_on_their_monitor() {
        let mut st = three_monitor_state();
        // window physically on the right monitor
        let fx = st.adopt_window(20, Rect::new(2000, 100, 800, 600));
        // it joins ws 2 (monitor 1's visible workspace) and tiles there
        assert_eq!(st.workspace_of(20), Some(1));
        assert_eq!(apply_rect(&fx, 20), Some(Rect::new(1920, 0, 1920, 1080)));
        // window on the laptop panel
        st.adopt_window(21, Rect::new(3900, 300, 640, 480));
        assert_eq!(st.workspace_of(21), Some(2));
        // window on primary — and active workspace never changed
        st.adopt_window(22, Rect::new(50, 50, 500, 500));
        assert_eq!(st.workspace_of(22), Some(0));
        assert_eq!(st.active, 0);
        // off-screen coordinates fall back to primary, never lost
        st.adopt_window(23, Rect::new(-9999, -9999, 100, 100));
        assert_eq!(st.workspace_of(23), Some(0));
    }

    #[test]
    fn switching_to_visible_workspace_only_moves_focus() {
        let mut st = three_monitor_state();
        st.adopt_window(20, Rect::new(2000, 100, 800, 600)); // ws2/mon1
        let fx = st.apply(Command::Workspace(WsSelector::Index(2)));
        // ws2 was already visible on monitor 1: no hide/show churn
        assert!(!fx
            .iter()
            .any(|e| matches!(e, Effect::Hide(_) | Effect::Show(_))));
        assert_eq!(st.active, 1);
        assert!(fx.contains(&Effect::Focus(20)));
    }

    #[test]
    fn switching_to_hidden_workspace_swaps_only_its_monitor() {
        let mut st = three_monitor_state();
        st.on_window_created(10); // ws1 on primary
        // ws4 is pinned to monitor 0; switching hides ws1 only
        let fx = st.apply(Command::Workspace(WsSelector::Index(4)));
        assert!(fx.iter().any(|e| matches!(e, Effect::Hide(ws) if ws == &vec![10])));
        assert_eq!(st.visible, vec![3, 1, 2]);
        assert_eq!(st.recent, 0);
        // recent-workspace bounces back
        st.apply(Command::Workspace(WsSelector::Recent));
        assert_eq!(st.active, 0);
        assert_eq!(st.visible, vec![0, 1, 2]);
    }

    #[test]
    fn next_prev_selectors_wrap() {
        let mut st = three_monitor_state();
        st.apply(Command::Workspace(WsSelector::Prev));
        assert_eq!(st.active, 8);
        st.apply(Command::Workspace(WsSelector::Next));
        assert_eq!(st.active, 0);
    }

    #[test]
    fn send_to_visible_workspace_lands_on_other_monitor() {
        let mut st = three_monitor_state();
        st.on_window_created(10);
        st.on_window_created(11);
        let fx = st.apply(Command::MoveToWorkspace(WsSelector::Index(2)));
        assert_eq!(st.workspace_of(11), Some(1));
        // target is visible on monitor 1 -> window appears there, not hidden
        assert!(!fx.iter().any(|e| matches!(e, Effect::Hide(_))));
        assert_eq!(apply_rect(&fx, 11), Some(Rect::new(1920, 0, 1920, 1080)));
    }

    // ---- fullscreen / minimize / exit ----------------------------------

    #[test]
    fn fullscreen_overrides_to_monitor_rect_and_toggles_back() {
        let mut st = three_monitor_state();
        st.on_window_created(10);
        st.on_window_created(11);
        let fx = st.apply(Command::ToggleFullscreen);
        assert_eq!(apply_rect(&fx, 11), Some(Rect::new(0, 0, 1920, 1080)));
        let fx = st.apply(Command::ToggleFullscreen);
        assert_eq!(apply_rect(&fx, 11), Some(Rect::new(960, 0, 960, 1080)));
    }

    #[test]
    fn minimize_emits_effect_and_leaves_tree() {
        let mut st = three_monitor_state();
        st.on_window_created(10);
        st.on_window_created(11);
        let fx = st.apply(Command::Minimize);
        assert_eq!(fx[0], Effect::Minimize(11));
        assert_eq!(st.workspace_of(11), None);
        assert_eq!(apply_rect(&fx, 10), Some(Rect::new(0, 0, 1920, 1080)));
    }

    #[test]
    fn exit_restores_everything_then_shuts_down() {
        let mut st = three_monitor_state();
        st.on_window_created(10);
        st.apply(Command::Workspace(WsSelector::Index(4))); // hides 10
        st.on_window_created(40);
        let fx = st.apply(Command::Exit);
        assert!(matches!(&fx[0], Effect::Show(ws) if ws.contains(&10) && ws.contains(&40)));
        assert_eq!(*fx.last().unwrap(), Effect::Shutdown);
    }

    #[test]
    fn focus_at_edge_is_silent_for_lua_wrap() {
        let mut st = three_monitor_state();
        st.on_window_created(10);
        assert!(st.apply(Command::Focus(Direction::Left)).is_empty());
    }
}
