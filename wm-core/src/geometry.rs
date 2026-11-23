//! Geometry primitives. All layout math bottoms out here.
//! Pixel coordinates are i32 (origin top-left, y-down), matching
//! Win32, AppKit (after flip), and X11 conventions at the boundary.

/// Split axis of a BSP node.
///
/// `Horizontal` = children sit side-by-side (split along a vertical line).
/// `Vertical`   = children are stacked top/bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    pub fn flip(self) -> Self {
        match self {
            Orientation::Horizontal => Orientation::Vertical,
            Orientation::Vertical => Orientation::Horizontal,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "left" | "west" | "h" => Some(Direction::Left),
            "right" | "east" | "l" => Some(Direction::Right),
            "up" | "north" | "k" => Some(Direction::Up),
            "down" | "south" | "j" => Some(Direction::Down),
            _ => None,
        }
    }

    /// The BSP split axis a directional operation acts along.
    pub fn axis(self) -> Orientation {
        match self {
            Direction::Left | Direction::Right => Orientation::Horizontal,
            Direction::Up | Direction::Down => Orientation::Vertical,
        }
    }

    /// Sign of the operation along its axis: Right/Down grow (+), Left/Up shrink (-).
    pub fn sign(self) -> f32 {
        match self {
            Direction::Right | Direction::Down => 1.0,
            Direction::Left | Direction::Up => -1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Shrink uniformly by `m` on every side (outer gap). Never inverts.
    pub fn shrink(&self, m: i32) -> Rect {
        let m = m.min(self.w / 2 - 1).min(self.h / 2 - 1).max(0);
        Rect::new(self.x + m, self.y + m, self.w - 2 * m, self.h - 2 * m)
    }

    /// Split into two children along `orient` with `gap` pixels between them.
    /// `ratio` is the fraction of the *available* space given to the first child.
    pub fn split(&self, orient: Orientation, ratio: f32, gap: i32) -> (Rect, Rect) {
        let ratio = ratio.clamp(0.05, 0.95);
        match orient {
            Orientation::Horizontal => {
                let avail = (self.w - gap).max(2);
                let lw = ((avail as f32) * ratio).round() as i32;
                let lw = lw.clamp(1, avail - 1);
                (
                    Rect::new(self.x, self.y, lw, self.h),
                    Rect::new(self.x + lw + gap, self.y, avail - lw, self.h),
                )
            }
            Orientation::Vertical => {
                let avail = (self.h - gap).max(2);
                let th = ((avail as f32) * ratio).round() as i32;
                let th = th.clamp(1, avail - 1);
                (
                    Rect::new(self.x, self.y, self.w, th),
                    Rect::new(self.x, self.y + th + gap, self.w, avail - th),
                )
            }
        }
    }

    /// Auto-split heuristic: split wide rects side-by-side, tall rects stacked.
    pub fn aspect_orientation(&self) -> Orientation {
        if self.w >= self.h {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        }
    }
}

/// Squared distance between two points (for nearest-neighbor focus nav).
pub fn dist2(a: (i32, i32), b: (i32, i32)) -> i64 {
    let dx = (a.0 - b.0) as i64;
    let dy = (a.1 - b.1) as i64;
    dx * dx + dy * dy
}

/// Is `candidate` in `dir` relative to `from`, using centers, with a
/// perpendicular-overlap requirement so nav doesn't jump diagonally
/// unless nothing overlaps.
pub fn in_direction(from: &Rect, candidate: &Rect, dir: Direction) -> bool {
    let (fx, fy) = from.center();
    let (cx, cy) = candidate.center();
    match dir {
        Direction::Left => cx < fx,
        Direction::Right => cx > fx,
        Direction::Up => cy < fy,
        Direction::Down => cy > fy,
    }
}

/// True if the two rects overlap on the axis perpendicular to `dir`.
pub fn overlaps_perpendicular(a: &Rect, b: &Rect, dir: Direction) -> bool {
    match dir.axis() {
        Orientation::Horizontal => a.y < b.y + b.h && b.y < a.y + a.h,
        Orientation::Vertical => a.x < b.x + b.w && b.x < a.x + a.w,
    }
}
