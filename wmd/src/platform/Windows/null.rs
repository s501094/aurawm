//! State-only backend for OSes whose real backend isn't built yet.
//! Lets the daemon+IPC+reducer stack run and be tested anywhere.

use super::Platform;
use std::sync::mpsc::Sender;
use wm_core::{Effect, Rect, WindowId};

pub struct Null;

impl Platform for Null {
    fn spawn_event_source(&self, _tx: Sender<crate::Msg>) {}

    fn existing_windows(&self) -> Vec<(WindowId, Rect)> {
        Vec::new()
    }

    fn monitors(&self) -> Vec<Rect> {
        vec![Rect::new(0, 0, 1920, 1080)]
    }

    fn run_effect(&self, fx: &Effect) {
        tracing::debug!(effect = ?fx, "null backend");
    }
}
