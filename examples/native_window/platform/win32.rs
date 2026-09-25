use super::Size;
use crate::app::Result;
use raw_window_handle::*;
use windows::{
    Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
    core::w,
};
pub struct Window {
    hwnd: HWND,
    instance: HINSTANCE,
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_CLOSE {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        } else {
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}
impl Window {
    pub fn new(title: &str, visible: bool) -> Result<Self> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = w!("TileinkNativeHost");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            });
            let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                windows::core::PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                640,
                360,
                None,
                None,
                Some(instance),
                None,
            )?;
            let window = Self { hwnd, instance };
            window.request_inner_size(Size {
                width: 640,
                height: 360,
            });
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            Ok(window)
        }
    }
    pub fn inner_size(&self) -> Size {
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                return Size {
                    width: 0,
                    height: 0,
                };
            }
            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            Size {
                width: (r.right - r.left) as u32,
                height: (r.bottom - r.top) as u32,
            }
        }
    }
    pub fn request_inner_size(&self, s: Size) {
        unsafe {
            let mut r = RECT {
                right: s.width as i32,
                bottom: s.height as i32,
                ..Default::default()
            };
            let _ = AdjustWindowRectEx(
                &mut r,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            );
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    pub fn pump(&self) -> bool {
        unsafe {
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
            IsWindow(Some(self.hwnd)).as_bool()
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
impl HasWindowHandle for Window {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let mut h = Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.hwnd.0 as isize).ok_or(HandleError::Unavailable)?,
        );
        h.hinstance = std::num::NonZeroIsize::new(self.instance.0 as isize);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}
