//! A workspace owns one BSP tree, a floating set, and the last-arranged
//! rect cache (used for geometric focus navigation and auto-orientation).

use crate::geometry::{
    dist2, in_direction, overlaps_perpendicular, Direction, Orientation, Rect,
};
use crate::tree::{Node, ResizeResult, WindowId};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub struct Gaps {
    pub inner: i32,
    pub outer: i32,
}

impl Default for Gaps {
    fn default() -> Self {
        Gaps { inner: 8, outer: 12 }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Workspace {
    root: Option<Node>,
    pub focused: Option<WindowId>,
    floating: HashSet<WindowId>,
    last_rects: HashMap<WindowId, Rect>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tiled windows only, in tree order.
    pub fn tiled_windows(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.windows(&mut out);
        }
        out
    }

    /// Every window this workspace owns (tiled + floating), for
    /// hide/show effects on workspace switches.
    pub fn all_windows(&self) -> Vec<WindowId> {
        let mut out = self.tiled_windows();
        out.extend(self.floating.iter().copied());
        out
    }

    pub fn contains(&self, w: WindowId) -> bool {
        self.floating.contains(&w)
            || self.root.as_ref().map_or(false, |r| r.contains(w))
    }

    pub fn is_floating(&self, w: WindowId) -> bool {
        self.floating.contains(&w)
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none() && self.floating.is_empty()
    }

    /// Insert a window, splitting the focused leaf. Orientation:
    /// explicit > aspect of the focused window's last rect > Horizontal.
    pub fn insert(&mut self, w: WindowId, orient: Option<Orientation>) {
        if self.contains(w) {
            return;
        }
        match &mut self.root {
            None => self.root = Some(Node::leaf(w)),
            Some(root) => {
                let target = self.focused.filter(|f| root.contains(*f)).or_else(|| {
                    let mut ws = Vec::new();
                    root.windows(&mut ws);
                    ws.first().copied()
                });
                match target {
                    Some(t) => {
                        let o = orient
                            .or_else(|| {
                                self.last_rects.get(&t).map(|r| r.aspect_orientation())
                            })
                            .unwrap_or(Orientation::Horizontal);
                        root.insert_at(t, w, o);
                    }
                    // Tree exists but empty focus and no tiled windows
                    // shouldn't happen; be defensive anyway.
                    None => self.root = Some(Node::leaf(w)),
                }
            }
        }
        self.focused = Some(w);
    }

    pub fn remove(&mut self, w: WindowId) {
        self.floating.remove(&w);
        self.last_rects.remove(&w);
        if let Some(root) = &mut self.root {
            if matches!(root, Node::Leaf(id) if *id == w) {
                self.root = None;
            } else {
                root.remove(w);
            }
        }
        if self.focused == Some(w) {
            self.focused = self
                .tiled_windows()
                .first()
                .copied()
                .or_else(|| self.floating.iter().next().copied());
        }
    }

    /// Pop a window out of the tree into the floating set, or re-tile it.
    /// Returns true if the window is floating after the call.
    pub fn toggle_float(&mut self, w: WindowId) -> bool {
        if self.floating.remove(&w) {
            self.insert(w, None);
            false
        } else if self.root.as_ref().map_or(false, |r| r.contains(w)) {
            if let Some(root) = &mut self.root {
                if matches!(root, Node::Leaf(id) if *id == w) {
                    self.root = None;
                } else {
                    root.remove(w);
                }
            }
            self.floating.insert(w);
            self.focused = Some(w);
            true
        } else {
            false
        }
    }

    /// Recompute placements for tiled windows within `screen`.
    /// Floating windows are untouched (the daemon leaves them where
    /// the user put them).
    pub fn arrange(&mut self, screen: Rect, gaps: Gaps) -> Vec<(WindowId, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.layout(screen.shrink(gaps.outer), gaps.inner, &mut out);
        }
        // Refresh the cache but keep floating rects if we ever track them.
        for (w, r) in &out {
            self.last_rects.insert(*w, *r);
        }
        out
    }

    fn neighbor(&self, dir: Direction) -> Option<WindowId> {
        let f = self.focused?;
        let fr = self.last_rects.get(&f)?;
        let candidates: Vec<(WindowId, &Rect)> = self
            .last_rects
            .iter()
            .filter(|(id, r)| **id != f && in_direction(fr, r, dir))
            .map(|(id, r)| (*id, r))
            .collect();
        // Prefer candidates that overlap on the perpendicular axis
        // (so Left from a tall stack picks the adjacent column, not a
        // diagonal window); fall back to any candidate in-direction.
        let overlapping: Vec<_> = candidates
            .iter()
            .filter(|(_, r)| overlaps_perpendicular(fr, r, dir))
            .collect();
        let pool: Vec<(WindowId, &Rect)> = if overlapping.is_empty() {
            candidates.clone()
        } else {
            overlapping.into_iter().cloned().collect()
        };
        pool.into_iter()
            .min_by_key(|(_, r)| dist2(fr.center(), r.center()))
            .map(|(id, _)| id)
    }

    /// Move focus geometrically. Returns the newly focused window, or
    /// None at a screen edge (the Lua layer can use that to wrap).
    pub fn focus_direction(&mut self, dir: Direction) -> Option<WindowId> {
        let target = self.neighbor(dir)?;
        self.focused = Some(target);
        Some(target)
    }

    /// Swap the focused window with its geometric neighbor.
    pub fn swap_direction(&mut self, dir: Direction) -> bool {
        let (Some(f), Some(t)) = (self.focused, self.neighbor(dir)) else {
            return false;
        };
        if let Some(root) = &mut self.root {
            root.swap(f, t);
            return true;
        }
        false
    }

    /// Resize the focused window along the direction's axis.
    pub fn resize(&mut self, dir: Direction, amount: f32) -> bool {
        let Some(f) = self.focused else { return false };
        let Some(root) = &mut self.root else {
            return false;
        };
        root.resize_along(f, dir.axis(), amount * dir.sign()) == ResizeResult::Adjusted
    }

    pub fn toggle_split(&mut self) -> bool {
        let Some(f) = self.focused else { return false };
        self.root
            .as_mut()
            .map_or(false, |r| r.toggle_split(f))
    }
}
