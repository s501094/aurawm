#![cfg(target_os = "windows")]

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LMENU, VK_RMENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

static mut HHOOK: isize = 0;

unsafe fn is_key_down(vk: u16) -> bool {
    (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0
}

unsafe extern "system" fn hook_callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let is_key_press = wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN;

        if is_key_press {
            let kb_struct = *(lparam as *const KBDLLHOOKSTRUCT);
            let vk_code = kb_struct.vkCode as u16;

            let alt_down = is_key_down(VK_LMENU) || is_key_down(VK_RMENU);

            if alt_down {
                match vk_code {
                    0x48 => { println!("Focus Left"); return 1; } // H
                    0x4A => { println!("Focus Down"); return 1; } // J
                    0x4B => { println!("Focus Up"); return 1; }   // K
                    0x4C => { println!("Focus Right"); return 1; }// L
                    _ => {}
                }
            }
        }
    }
    CallNextHookEx(HHOOK, code, wparam, lparam)
}

pub fn start() {
    std::thread::spawn(|| unsafe {
        HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_callback), 0, 0);
        if HHOOK == 0 { panic!("Failed to install WH_KEYBOARD_LL hook"); }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {}
        
        UnhookWindowsHookEx(HHOOK);
    });
}
