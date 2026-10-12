use super::Size;
use crate::app::Result;
use objc2::{MainThreadMarker, msg_send, rc::Retained};
use objc2_app_kit::*;
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::*;
pub struct Window {
    native: Retained<NSWindow>,
    view: Retained<NSView>,
    app: Retained<NSApplication>,
}
impl Window {
    pub fn new(title: &str, visible: bool) -> Result<Self> {
        let mtm = MainThreadMarker::new().ok_or("AppKit requires main")?;
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();
        let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(640., 360.));
        // SAFETY: AppKit is initialized on the main thread; the allocated window and rectangle satisfy the initializer contract.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: Retained owns the window, so AppKit must not release it automatically on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        let view = NSView::initWithFrame(mtm.alloc(), rect);
        view.setWantsLayer(true);
        window.setContentView(Some(&view));
        let this = Self {
            native: window,
            view,
            app,
        };
        this.request_inner_size(Size {
            width: 640,
            height: 360,
        })?;
        this.native.center();
        if visible {
            this.native.makeKeyAndOrderFront(None);
            #[allow(
                deprecated,
                reason = "Required Metal reflection/AppKit APIs preserve supported macOS versions."
            )]
            this.app.activateIgnoringOtherApps(true);
        }
        Ok(this)
    }
    #[expect(
        clippy::unnecessary_wraps,
        reason = "shared window API is fallible on Win32"
    )]
    pub fn inner_size(&self) -> Result<Size> {
        let s = self.view.convertSizeToBacking(self.view.bounds().size);
        Ok(Size {
            width: s.width.round() as u32,
            height: s.height.round() as u32,
        })
    }
    #[expect(
        clippy::unnecessary_wraps,
        reason = "shared window API is fallible on Win32"
    )]
    pub fn request_inner_size(&self, s: Size) -> Result {
        let scale = self.native.backingScaleFactor();
        self.native.setContentSize(NSSize::new(
            f64::from(s.width) / scale,
            f64::from(s.height) / scale,
        ));
        Ok(())
    }
    pub fn pump(&self) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let until = NSDate::dateWithTimeIntervalSinceNow(0.016);
            // SAFETY: The main-thread event loop supplies a live application, date, and run-loop mode for this call.
            let event = unsafe {
                self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&until),
                    NSDefaultRunLoopMode,
                    true,
                )
            };
            if let Some(e) = event {
                self.app.sendEvent(&e);
            }
            self.app.updateWindows();
        });
        self.native.isVisible()
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: The window is still retained and is closed on its owning main thread.
        unsafe {
            let _: () = msg_send![&*self.native, close];
        }
    }
}
impl HasWindowHandle for Window {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let ptr = std::ptr::NonNull::from(&*self.view).cast();
        // SAFETY: The view remains owned by self for the borrowed window-handle lifetime.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr)))
        })
    }
}
