//! wmd — the tiling window manager daemon.
//!
//! One loop: platform events and wmctl commands both funnel into the
//! wm-core reducer; resulting Effects go back out to the platform.
//! On Windows this manages real windows via the Win32 backend. On
//! other OSes it currently runs the null backend (state-only) until
//! their backends are built.

mod platform;

use platform::{Platform, PlatformEvent};
use std::sync::mpsc;
use wm_core::{Command, Effect, WmState};

enum Msg {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Event(PlatformEvent),
    Cmd(String, mpsc::Sender<String>),
}

fn main() {
    // 1. Tell Windows to stop scaling our math (Ty's DPI fix, now
    //    cfg-gated so the workspace still builds on macOS/Linux).
    //    2 = PROCESS_PER_MONITOR_DPI_AWARE.
    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwareness(2);
    }

    // 2. Telemetry: RUST_LOG controls verbosity, e.g.
    //      RUST_LOG=wmd=debug,wm_core=debug wmd
    //    Default is info for both. Feed these logs back when
    //    something misbehaves.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wmd=info,wm_core=info".into()),
        )
        .with_target(true)
        .init();

    let addr =
        std::env::var("TILEKIT_ADDR").unwrap_or_else(|_| wm_ipc::DEFAULT_ADDR.to_string());
    let (tx, rx) = mpsc::channel::<Msg>();

    // IPC: each request becomes a Msg::Cmd carrying its own reply channel.
    {
        let tx = tx.clone();
        wm_ipc::serve(&addr, move |line| {
            let (rtx, rrx) = mpsc::channel();
            if tx.send(Msg::Cmd(line, rtx)).is_err() {
                return "err daemon shutting down".into();
            }
            rrx.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap_or_else(|_| "err daemon did not respond".into())
        })
        .expect("cannot bind IPC address (is another wmd running?)");
    }

    let backend = platform::backend();
    backend.spawn_event_source(tx.clone());

    let mut st = WmState::new();
    
    // MULTI-MONITOR UPGRADE: Query all screens and feed them to the state
    let physical_screens = backend.monitors();
    st.update_monitors(physical_screens.clone());
    
    tracing::info!(monitors = ?physical_screens, %addr, "wmd started");

    // STATE-AWARE ADOPTION: existing windows join the workspace of the
    // monitor they're physically on — nothing gets vacuumed to the
    // primary display. (New windows created later still go to the
    // focused workspace via on_window_created.)
    for (w, rect) in backend.existing_windows() {
        tracing::debug!(window = w, ?rect, "adopting existing window");
        for fx in st.adopt_window(w, rect) {
            backend.run_effect(&fx);
        }
    }

    for msg in rx {
        match msg {
            Msg::Event(ev) => {
                let effects = match ev {
                    PlatformEvent::Created(w) => st.on_window_created(w),
                    PlatformEvent::Destroyed(w) => st.on_window_destroyed(w),
                    PlatformEvent::Focused(w) => st.on_window_focused(w),
                };
                for fx in &effects {
                    backend.run_effect(fx);
                }
            }
            Msg::Cmd(line, reply) => {
                // Introspection: `wmctl state` answers without mutating.
                if line == "state" {
                    let ws = st.active_ws();
                    let _ = reply.send(format!(
                        "ok active_ws={} visible={:?} monitors={} focused={:?} tiled={:?} gaps={}/{}",
                        st.active + 1,
                        st.visible.iter().map(|w| w + 1).collect::<Vec<_>>(),
                        st.monitors.len(),
                        st.focused_window(),
                        ws.tiled_windows(),
                        st.gaps.inner,
                        st.gaps.outer,
                    ));
                    continue;
                }
                let mut shutdown = false;
                let response = match Command::parse(&line) {
                    Err(e) => format!("err {e}"),
                    Ok(cmd) => {
                        let effects = st.apply(cmd);
                        let mut note = String::new();
                        for fx in &effects {
                            match fx {
                                Effect::Lua(name, _) => {
                                    // Lua host isn't built yet — be honest.
                                    note = format!(
                                        " (unknown verb '{name}': Lua layer not built yet)"
                                    );
                                }
                                Effect::Shutdown => shutdown = true,
                                _ => backend.run_effect(fx),
                            }
                        }
                        if effects.is_empty() {
                            "ok (no-op: screen edge or nothing to do)".to_string()
                        } else {
                            format!("ok{note}")
                        }
                    }
                };
                tracing::info!(command = %line, result = %response);
                let _ = reply.send(response);
                if shutdown {
                    tracing::info!("wm-exit: all windows restored, shutting down");
                    return;
                }
            }
        }
    }
}