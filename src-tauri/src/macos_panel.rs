//! macOS: make the popup a non-activating floating panel so it can open over
//! full-screen apps.
//!
//! A full-screen app lives in its own Space. A normal window that takes focus
//! activates our app, and macOS answers by switching back to the desktop
//! Space, so the popup appears out of sight. Menu-bar utilities avoid this
//! with an `NSPanel` using `NSWindowStyleMaskNonactivatingPanel`: it can be
//! clicked and typed into without activating the app. Tauri creates a normal
//! `NSWindow`, so we switch its class to an `NSPanel` subclass at startup (the
//! same technique the tauri-nspanel plugin uses).

use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, ClassType, MainThreadOnly};
use objc2_app_kit::{NSPanel, NSResponder, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use tauri::WebviewWindow;

define_class!(
    // SAFETY: plain NSPanel subclass with no ivars; it only overrides two
    // BOOL getters and is only used on the main thread.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CopilotPacerPanel"]
    struct PacerPanel;

    impl PacerPanel {
        /// Borderless windows can't become key by default; we need keyboard
        /// input for the settings fields.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }

    unsafe impl NSObjectProtocol for PacerPanel {}
);

/// Call once, on the main thread, before the popup is first shown.
pub fn convert_to_panel(w: &WebviewWindow) {
    let Ok(ptr) = w.ns_window() else { return };
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr` is the window's live NSWindow. PacerPanel adds no ivars, and
    // NSPanel has the same layout as NSWindow, so switching the class is sound
    // (tauri-nspanel does the same). We're on the main thread (app setup).
    unsafe {
        objc2::ffi::object_setClass(ptr.cast::<AnyObject>(), PacerPanel::class() as *const AnyClass);
        let panel: &NSPanel = &*ptr.cast::<NSPanel>();

        panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        // Make sure the non-activating behaviour takes effect for a window
        // that was created as a normal NSWindow (private but long-standing).
        let responds: bool = msg_send![panel, respondsToSelector: sel!(_setPreventsActivation:)];
        if responds {
            let _: () = msg_send![panel, _setPreventsActivation: true];
        }
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setHidesOnDeactivate(false);

        let mut behavior = panel.collectionBehavior();
        behavior.remove(NSWindowCollectionBehavior::MoveToActiveSpace);
        behavior.insert(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
        panel.setCollectionBehavior(behavior);
        // NSStatusWindowLevel: above full-screen app content.
        panel.setLevel(25);
    }
}

/// Bring the panel forward and give it keyboard focus without activating the
/// app (which would switch Spaces).
pub fn show_panel(w: &WebviewWindow) {
    let win = w.clone();
    let _ = w.run_on_main_thread(move || {
        let Ok(ptr) = win.ns_window() else { return };
        if ptr.is_null() {
            return;
        }
        // SAFETY: live NSWindow, main thread.
        let window: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
        window.orderFrontRegardless();
        window.makeKeyWindow();
    });
}
