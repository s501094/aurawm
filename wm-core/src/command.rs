//! The command grammar spoken over IPC. `wmctl focus --direction left`
//! arrives here as a string; the daemon parses once and feeds the
//! reducer. Unknown verbs are surfaced as `Custom` so the Lua layer
//! gets a shot at them before we error.

use crate::geometry::Direction;

/// Workspace selector: by number or relative (GlazeWM-style).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WsSelector {
    Index(usize), // 1-based
    Next,
    Prev,
    Recent,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Focus(Direction),
    Move(Direction),                 // swap with neighbor
    Resize(Direction, f32),          // amount = ratio delta
    ToggleFloat,
    ToggleSplit,                     // aka toggle-tiling-direction
    ToggleFullscreen,
    Minimize,
    Workspace(WsSelector),           // focus workspace
    MoveToWorkspace(WsSelector),     // send focused window
    Close,                           // ask platform to close focused
    Retile,                          // aka wm-redraw
    Exit,                            // restore all windows, then quit
    Custom(String, Vec<String>),     // handed to Lua `wm.command` table
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Empty,
    MissingArg(&'static str),
    BadArg(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty command"),
            ParseError::MissingArg(a) => write!(f, "missing argument: {a}"),
            ParseError::BadArg(a) => write!(f, "bad argument: {a}"),
        }
    }
}

fn flag_value<'a>(tokens: &'a [&str], flag: &str) -> Option<&'a str> {
    tokens
        .iter()
        .position(|t| *t == flag)
        .and_then(|i| tokens.get(i + 1))
        .copied()
}

/// GlazeWM-style workspace selectors on focus/move verbs:
///   --next-workspace | --prev-workspace | --recent-workspace
///   --workspace <N>  (also bare `workspace N` / `send N`)
fn parse_ws_selector(tokens: &[&str]) -> Option<Result<WsSelector, ParseError>> {
    if tokens.contains(&"--next-workspace") {
        return Some(Ok(WsSelector::Next));
    }
    if tokens.contains(&"--prev-workspace") {
        return Some(Ok(WsSelector::Prev));
    }
    if tokens.contains(&"--recent-workspace") {
        return Some(Ok(WsSelector::Recent));
    }
    if let Some(raw) = flag_value(tokens, "--workspace") {
        return Some(parse_ws_index(raw));
    }
    None
}

fn parse_ws_index(raw: &str) -> Result<WsSelector, ParseError> {
    match raw {
        "next" => return Ok(WsSelector::Next),
        "prev" | "previous" => return Ok(WsSelector::Prev),
        "recent" => return Ok(WsSelector::Recent),
        _ => {}
    }
    let n: usize = raw
        .parse()
        .map_err(|_| ParseError::BadArg(raw.to_string()))?;
    if n == 0 {
        return Err(ParseError::BadArg("workspaces are 1-based".into()));
    }
    Ok(WsSelector::Index(n))
}

/// Accepts both flag style (`focus --direction left`) and bare
/// positional style (`focus left`) so keybind configs stay terse.
fn parse_direction(tokens: &[&str]) -> Result<Direction, ParseError> {
    let raw = flag_value(tokens, "--direction")
        .or_else(|| tokens.first().copied().filter(|t| !t.starts_with("--")))
        .ok_or(ParseError::MissingArg("--direction"))?;
    Direction::parse(raw).ok_or_else(|| ParseError::BadArg(raw.to_string()))
}

impl Command {
    pub fn parse(input: &str) -> Result<Command, ParseError> {
        let tokens: Vec<&str> = input.split_whitespace().collect();
        let (&verb, rest) = tokens.split_first().ok_or(ParseError::Empty)?;
        match verb {
            // Workspace selectors take precedence over directions, so
            // `focus --next-workspace` and `focus --direction left`
            // both work under one verb, GlazeWM-style.
            "focus" => match parse_ws_selector(rest) {
                Some(sel) => Ok(Command::Workspace(sel?)),
                None => Ok(Command::Focus(parse_direction(rest)?)),
            },
            "move" => match parse_ws_selector(rest) {
                Some(sel) => Ok(Command::MoveToWorkspace(sel?)),
                None => Ok(Command::Move(parse_direction(rest)?)),
            },
            "resize" => {
                let dir = parse_direction(rest)?;
                let amount = flag_value(rest, "--amount")
                    .map(|a| a.parse::<f32>().map_err(|_| ParseError::BadArg(a.into())))
                    .transpose()?
                    .unwrap_or(0.05);
                Ok(Command::Resize(dir, amount))
            }
            "toggle-float" | "float" | "toggle-floating" => Ok(Command::ToggleFloat),
            "toggle-split" | "toggle-tiling-direction" => Ok(Command::ToggleSplit),
            "toggle-fullscreen" | "fullscreen" => Ok(Command::ToggleFullscreen),
            "toggle-minimized" | "minimize" => Ok(Command::Minimize),
            "wm-exit" | "exit" => Ok(Command::Exit),
            "wm-redraw" | "redraw" => Ok(Command::Retile),
            "workspace" | "ws" => {
                let raw = rest
                    .first()
                    .ok_or(ParseError::MissingArg("workspace index"))?;
                Ok(Command::Workspace(parse_ws_index(raw)?))
            }
            "move-to-workspace" | "send" => {
                let raw = rest
                    .first()
                    .ok_or(ParseError::MissingArg("workspace index"))?;
                Ok(Command::MoveToWorkspace(parse_ws_index(raw)?))
            }
            "close" => Ok(Command::Close),
            "retile" => Ok(Command::Retile),
            // `wmctl call <verb> [args...]` routes to Lua-defined verbs.
            "call" => {
                let (&name, args) = rest
                    .split_first()
                    .ok_or(ParseError::MissingArg("custom verb"))?;
                Ok(Command::Custom(
                    name.to_string(),
                    args.iter().map(|s| s.to_string()).collect(),
                ))
            }
            other => Ok(Command::Custom(
                other.to_string(),
                rest.iter().map(|s| s.to_string()).collect(),
            )),
        }
    }
}
