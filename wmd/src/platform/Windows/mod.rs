//! Platform abstraction for the daemon. The reducer (wm-core) never
//! sees any of this; backends translate Effects into OS calls and OS
//! events into PlatformEvents.

use std::sync::mpsc::Sender;
use wm_core::{Effect, Rect, WindowId};

#[derive(Debug)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum PlatformEvent {
    Created(WindowId),
    Destroyed(WindowId),
    Focused(WindowId),
}

pub trait Platform {
    /// Start delivering OS window events into the daemon loop.
    fn spawn_event_source(&self, tx: Sender<crate::Msg>);

    /// Manageable windows that existed before we started, WITH their
    /// current frame rects so the state can adopt them in place.
    fn existing_windows(&self) -> Vec<(WindowId, Rect)>;

    /// Work-area rect of every display. Index 0 MUST be the primary
    /// monitor (the Win32 backend enforces this via MONITORINFOF_PRIMARY).
    fn monitors(&self) -> Vec<Rect>;

    /// Execute one reducer effect against the OS.
    fn run_effect(&self, fx: &Effect);
}
#[cfg(target_os = "windows")]
mod overlay;
#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "windows")]
pub fn backend() -> impl Platform {
    win32::Win32::new()
}

#[cfg(not(target_os = "windows"))]
mod null;
#[cfg(not(target_os = "windows"))]
pub fn backend() -> impl Platform {
    null::Null
}
