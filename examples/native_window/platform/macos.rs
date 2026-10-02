use super::Size;
use crate::app::Result;
use objc2::{MainThreadMarker, msg_send, rc::Retained};
use objc2_app_kit::*;
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::*;
pub struct Window {
    window: Retained<NSWindow>,
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
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        let view = NSView::initWithFrame(mtm.alloc(), rect);
        view.setWantsLayer(true);
        window.setContentView(Some(&view));
        let this = Self { window, view, app };
        this.request_inner_size(Size {
            width: 640,
            height: 360,
        });
        this.window.center();
        if visible {
            this.window.makeKeyAndOrderFront(None);
            #[allow(deprecated)]
            this.app.activateIgnoringOtherApps(true);
        }
        Ok(this)
    }
    pub fn inner_size(&self) -> Size {
        let s = self.view.convertSizeToBacking(self.view.bounds().size);
        Size {
            width: s.width.round() as u32,
            height: s.height.round() as u32,
        }
    }
    pub fn request_inner_size(&self, s: Size) {
        let scale = self.window.backingScaleFactor();
        self.window
            .setContentSize(NSSize::new(s.width as f64 / scale, s.height as f64 / scale));
    }
    pub fn pump(&self) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let until = NSDate::dateWithTimeIntervalSinceNow(0.016);
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
        self.window.isVisible()
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.window, close];
        }
    }
}
impl HasWindowHandle for Window {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let ptr = std::ptr::NonNull::from(&*self.view).cast();
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr)))
        })
    }
}
