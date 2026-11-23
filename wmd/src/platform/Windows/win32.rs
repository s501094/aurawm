//! Win32 backend. The only crate-external things this file touches are
//! windows-sys FFI items; all constants are defined locally (they're
//! ABI-stable numbers) to keep the import surface minimal.
//!
//! Event side: SetWinEventHook (out-of-context) on a dedicated thread
//! running a message pump. Placement side: SetWindowPos with
//! DWMWA_EXTENDED_FRAME_BOUNDS compensation so gaps are visually even
//! despite Windows 10/11 invisible resize borders.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use super::{Platform, PlatformEvent};
use crate::Msg;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use wm_core::{Effect, Rect, WindowId};

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::UI::Accessibility::SetWinEventHook;
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetAncestor, GetMessageW, GetWindow, GetWindowLongW,
    GetWindowRect, GetWindowTextLengthW, IsIconic, IsWindowVisible, PostMessageW,
    SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage, // FIX ME: removed SystemParametersInfoW, from the import list. 
    MSG,
};

use super::overlay;
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

// ---- constants (ABI-stable; defined locally on purpose) -----------------
const EVENT_OBJECT_DESTROY: u32 = 0x8001;
const EVENT_OBJECT_SHOW: u32 = 0x8002;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
const OBJID_WINDOW: i32 = 0;

const GA_ROOT: u32 = 2;
const GW_OWNER: u32 = 4;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;

const DWMWA_CLOAKED: u32 = 14;
const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;

const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;
const WM_CLOSE: u32 = 0x0010;
const SW_MINIMIZE: i32 = 6;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;

// The hook callback is a bare C fn — it reaches the daemon through this.
static EVENT_TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

pub struct Win32;

impl Win32 {
    pub fn new() -> Self {
        Win32
    }
}

fn hwnd(w: WindowId) -> HWND {
    w as HWND
}

/// GlazeWM/komorebi-style eligibility: top-level, visible, unowned,
/// titled, not a tool window, not cloaked (UWP ghosts), not minimized.
unsafe fn is_manageable(h: HWND) -> bool {
    if IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
        return false;
    }
    if GetAncestor(h, GA_ROOT) != h || GetWindow(h, GW_OWNER) != 0 {
        return false;
    }
    if GetWindowTextLengthW(h) == 0 {
        return false;
    }
    if GetWindowLongW(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW != 0 {
        return false;
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(
        h,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut _,
        std::mem::size_of::<u32>() as u32,
    ) == 0
        && cloaked != 0
    {
        return false;
    }
    true
}

unsafe extern "system" fn win_event_proc(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    h: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if h == 0 || id_object != OBJID_WINDOW {
        return;
    }
    let ev = match event {
        EVENT_OBJECT_SHOW if is_manageable(h) => PlatformEvent::Created(h as WindowId),
        EVENT_OBJECT_DESTROY => PlatformEvent::Destroyed(h as WindowId),
        EVENT_SYSTEM_FOREGROUND if is_manageable(h) => PlatformEvent::Focused(h as WindowId),
        // Minimize leaves the tree; un-minimize rejoins it. Keeps the
        // taskbar's minimize button and Alt+- consistent with the WM.
        EVENT_SYSTEM_MINIMIZESTART => PlatformEvent::Destroyed(h as WindowId),
        EVENT_SYSTEM_MINIMIZEEND if is_manageable(h) => PlatformEvent::Created(h as WindowId),
        _ => return,
    };
    tracing::trace!(?ev, "win event");
    if let Some(tx) = EVENT_TX.get() {
        let _ = tx.lock().unwrap().send(Msg::Event(ev));
    }
}

unsafe extern "system" fn enum_proc(h: HWND, lparam: LPARAM) -> i32 {
    let out = &mut *(lparam as *mut Vec<(WindowId, Rect)>);
    if is_manageable(h) {
        let (_, _, _, _, (x, y, cw, ch)) = frame_geometry(h);
        out.push((h as WindowId, Rect::new(x, y, cw, ch)));
    }
    1 // continue enumeration
}

/// Offsets between GetWindowRect and the DWM visible frame. Placing a
/// window "at" a rect means placing its *visible* frame there.
/// Offsets between GetWindowRect and the DWM visible frame, plus the
/// current visible-frame rect (x, y, w, h). Placing a window "at" a
/// rect means placing its visible frame there; the current rect feeds
/// startup adoption (which monitor is this window on?).
unsafe fn frame_geometry(h: HWND) -> (i32, i32, i32, i32, (i32, i32, i32, i32)) {
    let mut wr: RECT = std::mem::zeroed();
    let mut fr: RECT = std::mem::zeroed();
    GetWindowRect(h, &mut wr);
    let ok = DwmGetWindowAttribute(
        h,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut fr as *mut RECT as *mut _,
        std::mem::size_of::<RECT>() as u32,
    );
    if ok != 0 {
        fr = wr; // no DWM info; the raw rect is the frame
    }
    (
        fr.left - wr.left,   // invisible border on the left (>= 0)
        fr.top - wr.top,
        wr.right - fr.right, // and on the right
        wr.bottom - fr.bottom,
        (fr.left, fr.top, fr.right - fr.left, fr.bottom - fr.top),
    )
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> i32 {
    // Cast the raw memory pointer back into our Rust Vec
    let out = &mut *(lparam as *mut Vec<Rect>);
    
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;

    if GetMonitorInfoW(hmonitor, &mut info) != 0 {
        // rcWork gives us the usable area (subtracting the taskbar on that specific screen)
        let r = info.rcWork;
        let rect = Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        
        if info.dwFlags & 1 != 0 {
            // If the monitor is not primary, we can also get rcMonitor for the full area
            out.insert(0, rect); // Insert at the beginning to keep primary monitor last
        } else {
            out.push(rect);
        }
    }
    1 // Return 1 to tell Windows to keep searching for more monitors
}

impl Platform for Win32 {
    fn spawn_event_source(&self, tx: Sender<Msg>) {
        let _ = EVENT_TX.set(Mutex::new(tx));
        std::thread::spawn(|| unsafe {
            
            overlay::init_on_thread();

            // Hooks deliver on the installing thread's message queue,
            // so this thread does nothing but pump.
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                0,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_SHOW, // range covers DESTROY(0x8001)..SHOW(0x8002)
                0,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                0,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    }

    fn existing_windows(&self) -> Vec<(WindowId, Rect)> {
        let mut out: Vec<(WindowId, Rect)> = Vec::new();
        unsafe {
            EnumWindows(Some(enum_proc), &mut out as *mut Vec<(WindowId, Rect)> as LPARAM);
        }
        tracing::info!(count = out.len(), "enumerated existing windows");
        out
    }

    //fn work_area(&self) -> Rect {
    //    unsafe {
    //        let mut r: RECT = std::mem::zeroed();
    //        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0);
    //        Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
    //    }
    //}
    
    // We change this to return a Vec of Rects instead of just one
    fn monitors(&self) -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::new();
        unsafe {
            // This kicks off the search, passing our empty array to the callback above
            EnumDisplayMonitors(
                0, 
                std::ptr::null(), 
                Some(monitor_enum_proc), 
                &mut out as *mut Vec<Rect> as LPARAM
            );
        }
        out
    }


    fn run_effect(&self, fx: &Effect) {
        unsafe {
            match fx {
                Effect::Apply(placements) => {
                    for (w, r) in placements {
                        let h = hwnd(*w);
                        let (l, t, rr, b, _) = frame_geometry(h);
                        SetWindowPos(
                            h,
                            0,
                            r.x - l,
                            r.y - t,
                            r.w + l + rr,
                            r.h + t + b,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
                Effect::Focus(w) => {
                    SetForegroundWindow(hwnd(*w));
                }
                Effect::Hide(ws) => {
                    for w in ws {
                        ShowWindow(hwnd(*w), SW_HIDE);
                    }
                }
                Effect::Show(ws) => {
                    for w in ws {
                        ShowWindow(hwnd(*w), SW_SHOWNOACTIVATE);
                    }
                }
                Effect::Close(w) => {
                    PostMessageW(hwnd(*w), WM_CLOSE, 0, 0);
                }
                Effect::Minimize(w) => {
                    ShowWindow(hwnd(*w), SW_MINIMIZE);
                }
                Effect::Shutdown => {} // main loop handles exit
                Effect::RedrawOverlay => {
                    let fg = GetForegroundWindow();
                    if fg  != 0 && is_manageable(fg) {
                        let (_, _, _, _, (x, y, w, h)) = frame_geometry(fg);
                        overlay::set_focus_rect(Some(Rect::new(x,y,w,h)));
                    } else {
                        overlay::set_focus_rect(None);
                    }
                } // overlay.rs is milestone #2
                Effect::Lua(..) => {}       // handled in main loop
            }
        }
    }
}
