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
    // SAFETY: Windows supplies this callback with its window/message arguments; the close branch destroys that window, and other messages are forwarded unchanged.
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
        // SAFETY: The module, class name and window title remain live during registration/creation; the returned HWND is owned by Window.
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
            })?;
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            Ok(window)
        }
    }
    pub fn inner_size(&self) -> Result<Size> {
        // SAFETY: The Window owner retains the live HWND and supplies initialized writable RECT storage.
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                return Ok(Size {
                    width: 0,
                    height: 0,
                });
            }
            let mut r = RECT::default();
            GetClientRect(self.hwnd, &raw mut r)?;
            Ok(Size {
                width: (r.right - r.left) as u32,
                height: (r.bottom - r.top) as u32,
            })
        }
    }
    pub fn request_inner_size(&self, s: Size) -> Result {
        // SAFETY: The HWND is retained and checked client dimensions are converted to a bounded outer-window size before SetWindowPos.
        unsafe {
            // Calculate the frame insets separately, then add the requested
            // client extent in wide arithmetic so neither RECT nor subtraction
            // can wrap before SetWindowPos receives its signed dimensions.
            let mut r = RECT::default();
            AdjustWindowRectEx(
                &raw mut r,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            )?;
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                (i64::from(s.width) + i64::from(r.right) - i64::from(r.left)).try_into()?,
                (i64::from(s.height) + i64::from(r.bottom) - i64::from(r.top)).try_into()?,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
            Ok(())
        }
    }
    pub fn pump(&self) -> bool {
        // SAFETY: MSG storage is initialized; the main-thread loop dispatches only messages returned by Windows.
        unsafe {
            let mut m = MSG::default();
            while PeekMessageW(&raw mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const m);
                DispatchMessageW(&raw const m);
            }
            IsWindow(Some(self.hwnd)).as_bool()
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: This owner destroys its HWND once when the window wrapper is dropped.
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
        // SAFETY: The returned borrowed handle cannot outlive the Window that owns its HWND.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

#[cfg(test)]
mod tests;
