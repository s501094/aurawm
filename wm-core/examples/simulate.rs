//! Interactive engine simulator. Drives the exact same reducer the
//! daemon will use, rendering placements as an ASCII grid.
//!
//!   cargo run --example simulate
//!
//! Commands: `open` / `close` plus the full wmctl grammar
//! (focus left, move right, resize down --amount 0.1, workspace 2,
//! send 3, toggle-float, toggle-split, retile, quit).

use std::io::{self, BufRead, Write};
use wm_core::{Command, Effect, Rect, WmState};

const SCREEN: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };
const COLS: usize = 96; // 20px per cell
const ROWS: usize = 27; // 40px per cell

fn render(st: &WmState) {
    let mut grid = vec![vec![' '; COLS]; ROWS];
    // Re-derive current placements by cloning state? The reducer owns
    // the cache; simplest is to ask for a retile on a scratch clone.
    let placements = {
        // Workspace::arrange mutates the rect cache, so work on the
        // real state — retile is idempotent.
        let mut tmp: Vec<(u64, Rect)> = Vec::new();
        // Safe: render is only called from the REPL loop with &st,
        // so gather from the active workspace snapshot instead.
        for (w, r) in st_placements(st) {
            tmp.push((w, r));
        }
        tmp
    };
    for (w, r) in &placements {
        let x0 = (r.x as usize * COLS) / SCREEN.w as usize;
        let x1 = (((r.x + r.w) as usize) * COLS) / SCREEN.w as usize;
        let y0 = (r.y as usize * ROWS) / SCREEN.h as usize;
        let y1 = (((r.y + r.h) as usize) * ROWS) / SCREEN.h as usize;
        let x1 = x1.clamp(x0 + 1, COLS);
        let y1 = y1.clamp(y0 + 1, ROWS);
        let focused = st.focused_window() == Some(*w);
        for y in y0..y1 {
            for x in x0..x1 {
                let edge = y == y0 || y == y1 - 1 || x == x0 || x == x1 - 1;
                grid[y][x] = if edge {
                    if focused { '#' } else { '.' }
                } else {
                    ' '
                };
            }
        }
        // window id label at top-left interior
        let label = format!("{w}{}", if focused { "*" } else { "" });
        for (i, ch) in label.chars().enumerate() {
            if x0 + 2 + i < x1 - 1 {
                grid[y0 + 1][x0 + 2 + i] = ch;
            }
        }
    }
    let mut out = String::with_capacity((COLS + 1) * ROWS);
    for row in &grid {
        out.extend(row.iter());
        out.push('\n');
    }
    println!("{out}");
    println!(
        "workspace {} | focused: {:?} | windows here: {:?}",
        st.active + 1,
        st.focused_window(),
        st_placements(st).iter().map(|(w, _)| *w).collect::<Vec<_>>()
    );
}

/// Current placements for the active workspace, computed fresh.
fn st_placements(st: &WmState) -> Vec<(u64, Rect)> {
    let mut ws = st.active_ws().clone();
    let screen = st.monitors.first().copied().unwrap_or(SCREEN);
    ws.arrange(screen, st.gaps)
}

fn main() {
    let mut st = WmState::new();
    let mut next_id: u64 = 1;
    let stdin = io::stdin();

    println!("tilekit engine simulator — 'open', 'close', or any wmctl command. 'quit' exits.\n");
    render(&st);
    print!("> ");
    io::stdout().flush().ok();

    for line in stdin.lock().lines() {
        let line = line.unwrap_or_default();
        let line = line.trim();
        let fx: Vec<Effect> = match line {
            "" => Vec::new(),
            "quit" | "exit" | "q" => break,
            "open" => {
                let id = next_id;
                next_id += 1;
                st.on_window_created(id)
            }
            "close" => match st.focused_window() {
                Some(w) => st.on_window_destroyed(w),
                None => Vec::new(),
            },
            other => match Command::parse(other) {
                Ok(cmd) => st.apply(cmd),
                Err(e) => {
                    println!("parse error: {e}");
                    Vec::new()
                }
            },
        };
        for e in &fx {
            match e {
                Effect::Lua(name, args) => {
                    println!("→ would dispatch to Lua: {name} {args:?}")
                }
                Effect::Close(w) => {
                    println!("→ OS close request for {w} (simulating destroy)");
                    let destroyed = st.on_window_destroyed(*w);
                    drop(destroyed);
                }
                _ => {}
            }
        }
        render(&st);
        print!("> ");
        io::stdout().flush().ok();
    }
}
