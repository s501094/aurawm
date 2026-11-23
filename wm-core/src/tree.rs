//! The BSP tree. Every interior node is a binary split with an
//! orientation and a ratio; every leaf is a window. All operations are
//! plain recursion — window counts are small, allocation churn is nil.

use crate::geometry::{Orientation, Rect};

/// Opaque window handle. Platform backends map HWND / AX element / X11
/// window ids into this at the boundary.
pub type WindowId = u64;

#[derive(Clone, Debug)]
pub enum Node {
    Leaf(WindowId),
    Split {
        orient: Orientation,
        ratio: f32,
        children: [Box<Node>; 2],
    },
}

/// Result of a resize walk. `Found` means the target leaf is in this
/// subtree but no ancestor with a matching axis has been found *yet* —
/// the walk keeps bubbling up until one adjusts its ratio.
#[derive(PartialEq, Eq, Debug)]
pub enum ResizeResult {
    NotFound,
    Found,
    Adjusted,
}

impl Node {
    pub fn leaf(w: WindowId) -> Self {
        Node::Leaf(w)
    }

    pub fn contains(&self, w: WindowId) -> bool {
        match self {
            Node::Leaf(id) => *id == w,
            Node::Split { children, .. } => children.iter().any(|c| c.contains(w)),
        }
    }

    pub fn windows(&self, out: &mut Vec<WindowId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { children, .. } => children.iter().for_each(|c| c.windows(out)),
        }
    }

    /// Replace the leaf holding `target` with a split of (target, new).
    /// Returns false if `target` isn't in this subtree.
    pub fn insert_at(&mut self, target: WindowId, new: WindowId, orient: Orientation) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let old = *id;
                *self = Node::Split {
                    orient,
                    ratio: 0.5,
                    children: [Box::new(Node::leaf(old)), Box::new(Node::leaf(new))],
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { children, .. } => children
                .iter_mut()
                .any(|c| c.insert_at(target, new, orient)),
        }
    }

    /// Remove `target`, collapsing its parent split into the sibling.
    /// The root-is-the-target-leaf case is handled by the workspace.
    pub fn remove(&mut self, target: WindowId) -> bool {
        if let Node::Split { children, .. } = self {
            let mut keep: Option<Box<Node>> = None;
            for i in 0..2 {
                if matches!(children[i].as_ref(), Node::Leaf(id) if *id == target) {
                    keep = Some(children[1 - i].clone());
                    break;
                }
            }
            if let Some(k) = keep {
                *self = *k;
                return true;
            }
            return children.iter_mut().any(|c| c.remove(target));
        }
        false
    }

    /// Swap the tree positions of windows `a` and `b`.
    pub fn swap(&mut self, a: WindowId, b: WindowId) {
        match self {
            Node::Leaf(id) => {
                if *id == a {
                    *id = b;
                } else if *id == b {
                    *id = a;
                }
            }
            Node::Split { children, .. } => {
                children[0].swap(a, b);
                children[1].swap(a, b);
            }
        }
    }

    /// Flip the orientation of the split directly containing `target`.
    pub fn toggle_split(&mut self, target: WindowId) -> bool {
        if let Node::Split {
            orient, children, ..
        } = self
        {
            let direct = children
                .iter()
                .any(|c| matches!(c.as_ref(), Node::Leaf(id) if *id == target));
            if direct {
                *orient = orient.flip();
                return true;
            }
            return children.iter_mut().any(|c| c.toggle_split(target));
        }
        false
    }

    /// Recursively carve `rect` into per-window placements.
    pub fn layout(&self, rect: Rect, gap: i32, out: &mut Vec<(WindowId, Rect)>) {
        match self {
            Node::Leaf(id) => out.push((*id, rect)),
            Node::Split {
                orient,
                ratio,
                children,
            } => {
                let (a, b) = rect.split(*orient, *ratio, gap);
                children[0].layout(a, gap, out);
                children[1].layout(b, gap, out);
            }
        }
    }

    /// Grow/shrink `target` along `axis` by `delta` (fraction of the
    /// nearest matching ancestor split). bspwm semantics: the closest
    /// ancestor whose orientation matches the axis absorbs the change.
    pub fn resize_along(&mut self, target: WindowId, axis: Orientation, delta: f32) -> ResizeResult {
        match self {
            Node::Leaf(id) => {
                if *id == target {
                    ResizeResult::Found
                } else {
                    ResizeResult::NotFound
                }
            }
            Node::Split {
                orient,
                ratio,
                children,
            } => {
                for i in 0..2 {
                    match children[i].resize_along(target, axis, delta) {
                        ResizeResult::NotFound => continue,
                        ResizeResult::Adjusted => return ResizeResult::Adjusted,
                        ResizeResult::Found => {
                            if *orient == axis {
                                let d = if i == 0 { delta } else { -delta };
                                *ratio = (*ratio + d).clamp(0.1, 0.9);
                                return ResizeResult::Adjusted;
                            }
                            return ResizeResult::Found;
                        }
                    }
                }
                ResizeResult::NotFound
            }
        }
    }
}
