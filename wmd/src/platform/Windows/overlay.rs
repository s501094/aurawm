//! The Ghost Window: A multi-monitor, transparent, click-through overlay.

use std::sync::Mutex;
use wm_core::Rect;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, Rectangle,
    SelectObject, GetStockObject, PAINTSTRUCT, PS_SOLID,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, RegisterClassW, SetLayeredWindowAttributes,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, LWA_COLORKEY, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, WM_PAINT,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    PostMessageW, WM_USER,
};

static FOCUS_RECT: Mutex<Option<Rect>> = Mutex::new(None);
static OVERLAY_HWND: Mutex<HWND> = Mutex::new(0);

// Catppuccin Mocha Mauve: #cba6f7 -> Win32 BGR format: 0x00F7A6CB
const BORDER_COLOR: u32 = 0x00F7A6CB;
const BORDER_WIDTH: i32 = 4;
// Magenta background to act as our alpha-mask key
const COLORKEY_MAGENTA: u32 = 0x00FF00FF;

// Custom Win32 Message to trigger redraws safely across threads
const WM_REDRAW_OVERLAY: u32 = WM_USER + 1;

/// Safely update the focal target and ask Windows to redraw the glass.
pub fn set_focus_rect(rect: Option<Rect>) {
    *FOCUS_RECT.lock().unwrap() = rect;
    let hwnd = *OVERLAY_HWND.lock().unwrap();
    if hwnd != 0 {
        unsafe {
            // Safely send the redraw message across threads to the pump
            PostMessageW(hwnd, WM_REDRAW_OVERLAY, 0, 0);
        }
    }
}

/// Called EXACTLY ONCE on the thread running the GetMessageW pump.
pub unsafe fn init_on_thread() {
    let class_name: Vec<u16> = "AuraWMOverlay\0".encode_utf16().collect();
    let bg_brush = CreateSolidBrush(COLORKEY_MAGENTA);

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: 0,
        hIcon: 0,
        hCursor: 0,
        hbrBackground: bg_brush,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    RegisterClassW(&wc);

    // Span all monitors perfectly, sitting above everything, ignoring clicks.
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        class_name.as_ptr(),
        std::ptr::null(),
        WS_POPUP,
        GetSystemMetrics(SM_XVIRTUALSCREEN),
        GetSystemMetrics(SM_YVIRTUALSCREEN),
        GetSystemMetrics(SM_CXVIRTUALSCREEN),
        GetSystemMetrics(SM_CYVIRTUALSCREEN),
        0,
        0,
        0,
        std::ptr::null(),
    );

    // Tell Windows: "Make all Magenta pixels 100% transparent."
    SetLayeredWindowAttributes(hwnd, COLORKEY_MAGENTA, 0, LWA_COLORKEY);
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    *OVERLAY_HWND.lock().unwrap() = hwnd;
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Intercept our custom message and trigger the actual paint
    if msg == WM_REDRAW_OVERLAY {
        InvalidateRect(hwnd, std::ptr::null(), 1);
        return 0;
    }

    if msg == WM_PAINT {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);

        let rect_guard = FOCUS_RECT.lock().unwrap();
        if let Some(r) = *rect_guard {
            let pen = CreatePen(PS_SOLID, BORDER_WIDTH, BORDER_COLOR);
            let old_pen = SelectObject(hdc, pen as _);
            // HOLLOW_BRUSH (value 5) prevents the center of our rect from filling with Mauve
            let old_brush = SelectObject(hdc, GetStockObject(5)); 

            // Compensate for multi-monitor setups where a monitor might be at a negative X/Y
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);

            // Pad by 2px so the 4px border perfectly frames the *outside* of the window
            // instead of overlapping your terminal text.
            let pad = 2;

            Rectangle(
                hdc,
                r.x - vx - pad,
                r.y - vy - pad,
                r.x + r.w - vx + pad,
                r.y + r.h - vy + pad,
            );

            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(pen as _);
        }

        EndPaint(hwnd, &mut ps);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}