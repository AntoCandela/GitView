//! Owns the nonactivating panel while Tauri retains the companion's IPC identity and webview.
//! Only WKWebView moves; the hidden Tao/Wry window and its content root remain intact.

use std::cell::RefCell;
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSFloatingWindowLevel, NSPanel, NSView,
    NSWindow, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{ns_string, NSObjectProtocol, NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use super::{native_window, NativeError};

define_class!(
    // NSPanel supports subclassing; all instances and retained owners stay on the main thread.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "GitViewCompanionPanel"]
    struct CompanionPanel;

    impl CompanionPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool { false }
    }
);

impl CompanionPanel {
    fn new(marker: MainThreadMarker) -> Result<Retained<Self>, NativeError> {
        let allocated = Self::alloc(marker).set_ivars(());
        // The nonactivating style must reach NSPanel's initializer: setting it later does not
        // establish the window-server focus behavior needed over another application's Space.
        let panel: Option<Retained<Self>> = unsafe {
            msg_send![super(allocated),
                initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(760.0, 560.0)),
                styleMask: NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView
                    | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        let panel = panel.ok_or(NativeError::PanelFailed)?;
        // Rust's retained owner releases the panel; close must not consume its retain count.
        unsafe { panel.setReleasedWhenClosed(false); }
        panel.setTitle(ns_string!("GitView"));
        // Use the desktop window's system corner shape without adding visible title-bar chrome.
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        panel.setTitlebarAppearsTransparent(true);
        panel.setMovable(false);
        for button in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(button) = panel.standardWindowButton(button) { button.setHidden(true); }
        }
        panel.setFloatingPanel(true);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setHidesOnDeactivate(false);
        let mut behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        if objc2::available!(macos = 13.0) {
            behavior |= NSWindowCollectionBehavior::CanJoinAllApplications;
        }
        panel.setCollectionBehavior(behavior);
        Ok(panel)
    }
}

struct PanelOwner {
    panel: Retained<CompanionPanel>,
    webview: Retained<NSView>,
}

impl PanelOwner {
    fn attach(host: &NSWindow, marker: MainThreadMarker) -> Result<Self, NativeError> {
        let root = host.contentView().ok_or(NativeError::PanelFailed)?;
        let class = AnyClass::get(c"WKWebView").ok_or(NativeError::PanelFailed)?;
        let webview = find_webview(&root, class).ok_or(NativeError::PanelFailed)?;
        if !host.makeFirstResponder(None) { return Err(NativeError::PanelFailed); }
        let panel = CompanionPanel::new(marker)?;
        let Some(content) = panel.contentView() else {
            panel.close();
            return Err(NativeError::PanelFailed);
        };
        content.setAutoresizesSubviews(true);
        webview.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable);
        // AppKit removes the previous parent as part of addSubview, just as Wry's native
        // reparent implementation does. Never replace/move the hidden host's content root.
        content.addSubview(&webview);
        webview.setFrame(content.bounds());
        Ok(Self { panel, webview })
    }
}

impl Drop for PanelOwner {
    fn drop(&mut self) {
        self.panel.orderOut(None);
        self.panel.makeFirstResponder(None);
        // Wry still retains this same view and detaches it again during hidden-host destruction.
        self.webview.removeFromSuperview();
        self.panel.close();
    }
}

thread_local! { static PANEL: RefCell<Option<PanelOwner>> = const { RefCell::new(None) }; }

/// Prewarms the hidden Tauri host before tray availability, while main owns activation.
/// The caller destroys the hidden host on failure, and after destroying this native owner.
pub(super) fn prepare(app: &AppHandle) -> Result<(), NativeError> {
    let marker = MainThreadMarker::new().ok_or(NativeError::PanelFailed)?;
    if PANEL.with(|slot| slot.borrow().is_some()) { return Ok(()); }
    let host = match app.get_webview_window("companion") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, "companion", WebviewUrl::App("index.html".into()))
            .title("GitView").inner_size(760.0, 560.0).decorations(false).resizable(false)
            .visible(false).focused(false).focusable(true).skip_taskbar(true)
            .build().map_err(|_| NativeError::PanelFailed)?,
    };
    // Wry unconditionally activates NSApplication during construction, even for visible(false).
    // Keeping this out of show/focus prevents that activation on a fullscreen tray click.
    let native = native_window(&host)?;
    native.orderOut(None);
    let owner = PanelOwner::attach(&native, marker)?;
    let previous = PANEL.with(|slot| slot.borrow_mut().replace(owner));
    drop(previous);
    Ok(())
}

pub(super) fn window() -> Result<Retained<NSWindow>, NativeError> {
    MainThreadMarker::new().ok_or(NativeError::PanelFailed)?;
    PANEL.with(|slot| slot.borrow().as_ref().map(|owner| owner.panel.clone().into_super().into_super()))
        .ok_or(NativeError::PanelFailed)
}

/// The caller orders the panel front first; taking keyboard focus never activates NSApplication.
pub(super) fn focus() -> Result<(), NativeError> {
    MainThreadMarker::new().ok_or(NativeError::FocusFailed)?;
    let (panel, webview) = PANEL.with(|slot| slot.borrow().as_ref()
        .map(|owner| (owner.panel.clone(), owner.webview.clone())))
        .ok_or(NativeError::PanelFailed)?;
    panel.makeKeyWindow();
    if !panel.makeFirstResponder(Some(&webview)) || !panel.isKeyWindow() {
        return Err(NativeError::FocusFailed);
    }
    Ok(())
}

/// Clears the owner before AppKit callbacks; the caller clears monitors and destroys Tauri next.
pub(super) fn destroy() {
    let owner = PANEL.with(|slot| slot.borrow_mut().take());
    drop(owner);
}

fn find_webview(view: &NSView, class: &AnyClass) -> Option<Retained<NSView>> {
    if view.isKindOfClass(class) { return Some(view.retain()); }
    for child in view.subviews() {
        if let Some(webview) = find_webview(&child, class) { return Some(webview); }
    }
    None
}
