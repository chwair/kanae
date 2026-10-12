//! gives the frameless qt window real frame styles on windows, so the shell treats it
//! like any other window: aero snap, restore when dragged while maximized, animations.

use std::sync::atomic::{AtomicIsize, Ordering};

use winapi::shared::minwindef::{BOOL, DWORD, LPARAM, LRESULT, TRUE, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::dwmapi::{DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY};
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::*;

const TITLE: &str = "Kanae";
const FRAME_STYLE: DWORD =
    WS_THICKFRAME | WS_CAPTION | WS_SYSMENU | WS_MAXIMIZEBOX | WS_MINIMIZEBOX;

/// the main window, 0 until `install` has found it.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// qt's own window procedure, called for everything not handled here.
static PREV_PROC: AtomicIsize = AtomicIsize::new(0);

/// find the main window on the calling (gui) thread and give it the frame styles.
/// must run after the qml engine has created the window.
pub fn install() {
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(enum_proc), 0); }
}

/// maximize or restore through the system. qt only resizes a frameless window to the
/// work area, which the shell does not count as maximized (no restore on drag).
pub fn toggle_maximize() {
    let hwnd = MAIN_HWND.load(Ordering::Relaxed) as HWND;
    if hwnd.is_null() { return; }
    unsafe {
        let cmd = if IsZoomed(hwnd) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
        PostMessageW(hwnd, WM_SYSCOMMAND, cmd, 0);
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, _: LPARAM) -> BOOL {
    let mut buf = [0u16; 32];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
    if String::from_utf16_lossy(&buf[..n]) != TITLE {
        return TRUE;
    }
    // qml draws the whole window, so keep dwm from adding its own border and shadow.
    let policy: DWORD = DWMNCRP_DISABLED;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_NCRENDERING_POLICY,
        &policy as *const DWORD as *const _,
        std::mem::size_of::<DWORD>() as DWORD,
    );
    // subclass before restyling so the frame never gets a non-client area.
    PREV_PROC.store(SetWindowLongPtrW(hwnd, GWLP_WNDPROC, frame_proc as *const () as isize), Ordering::Relaxed);
    SetWindowLongPtrW(hwnd, GWL_STYLE, GetWindowLongPtrW(hwnd, GWL_STYLE) | FRAME_STYLE as isize);
    SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        0, 0, 0, 0,
        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
    MAIN_HWND.store(hwnd as isize, Ordering::Relaxed);
    0
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let prev: WNDPROC = std::mem::transmute(PREV_PROC.load(Ordering::Relaxed));
    match msg {
        // no non-client area: the client rect stays the whole window, which is what qt
        // assumes for a frameless window. qml pads the maximized overhang itself.
        WM_NCCALCSIZE => 0,
        // nothing to paint there either.
        WM_NCPAINT => 0,
        // -1 stops the classic frame repaint on activation changes.
        WM_NCACTIVATE => CallWindowProcW(prev, hwnd, msg, wp, -1),
        // keep the frame styles if qt rewrites the window style later.
        WM_STYLECHANGING if wp as i32 == GWL_STYLE => {
            (*(lp as *mut STYLESTRUCT)).styleNew |= FRAME_STYLE;
            CallWindowProcW(prev, hwnd, msg, wp, lp)
        }
        _ => CallWindowProcW(prev, hwnd, msg, wp, lp),
    }
}
