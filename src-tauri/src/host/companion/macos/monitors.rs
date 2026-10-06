//! Owns per-opening AppKit event monitors and focus/display observers.
//! Native callbacks defer lifecycle mutations and fence them to the current opening.

use std::{cell::RefCell, ptr::NonNull};
use block2::RcBlock;
use objc2::{rc::Retained, runtime::{AnyObject, ProtocolObject}, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification,
    NSApplicationDidResignActiveNotification, NSEvent, NSEventMask,
    NSWindow, NSWindowDidResignKeyNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use tauri::Manager;

use super::{companion_window, native_window, position, Anchor, CompanionController, NativeError, TRAY_ID};

const MOUSE_DOWN: NSEventMask = NSEventMask::LeftMouseDown
    .union(NSEventMask::RightMouseDown).union(NSEventMask::OtherMouseDown);

#[derive(Clone, Copy)]
enum Dismissal {
    ReconcileFocus,
    Explicit,
}

struct Monitors {
    local: Option<Retained<AnyObject>>,
    global: Option<Retained<AnyObject>>,
    notifications: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    anchor: Anchor,
    generation: uuid::Uuid,
    pending_dismissal: Option<Dismissal>,
}

impl Drop for Monitors {
    fn drop(&mut self) {
        // Partially installed monitors have the same main-thread cleanup as a shown panel.
        unsafe {
            if let Some(token) = &self.local { NSEvent::removeMonitor(token); }
            if let Some(token) = &self.global { NSEvent::removeMonitor(token); }
            let center = NSNotificationCenter::defaultCenter();
            for observer in &self.notifications { center.removeObserver(observer.as_ref()); }
        }
    }
}

thread_local! {
    static MONITORS: RefCell<Option<Monitors>> = const { RefCell::new(None) };
}

pub(super) fn clear() {
    // Invalidate the generation before AppKit removal, without holding a reentrant TLS borrow.
    let monitors = MONITORS.with(|slot| slot.borrow_mut().take());
    drop(monitors);
}

/// Installs main-thread monitors after the actual panel has been ordered and focused.
/// Any installation failure removes every token already registered for this opening.
pub(super) fn install(controller: CompanionController, panel: &NSWindow, anchor: Anchor) -> Result<(), NativeError> {
    MainThreadMarker::new().ok_or(NativeError::PanelFailed)?;
    clear();
    let tray = controller.app.tray_by_id(TRAY_ID).ok_or(NativeError::TrayFailed)?;
    let tray_window = tray.with_inner_tray_icon(|tray| {
        let marker = MainThreadMarker::new()?;
        tray.ns_status_item()?.button(marker)?.window()
            .map(|window| std::ptr::from_ref(&*window) as usize)
    }).map_err(|_| NativeError::TrayFailed)?.ok_or(NativeError::TrayFailed)?;
    let generation = uuid::Uuid::new_v4();
    let panel_id = std::ptr::from_ref(panel) as usize;
    let mut monitors = Monitors {
        local: None,
        global: None,
        notifications: Vec::with_capacity(3),
        anchor,
        generation,
        pending_dismissal: None,
    };

    let events = controller.clone();
    let local_block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        if !current_monitor(generation) { return event.as_ptr(); }
        // AppKit invokes both kinds of event monitor on main with a live event.
        let event_ref = unsafe { event.as_ref() };
        let Some(marker) = MainThreadMarker::new() else { return event.as_ptr(); };
        let event_window = event_ref.window(marker);
        let owned = event_window.as_ref().is_some_and(|window| is_owned_window(window, panel_id));
        let tray_event = event_window.as_ref()
            .is_some_and(|window| std::ptr::from_ref(&**window) as usize == tray_window);
        if !owned && !tray_event {
            enqueue_dismissal(events.clone(), panel_id, generation, Dismissal::Explicit);
        }
        event.as_ptr()
    });
    monitors.local = Some(unsafe {
        // Keyboard events belong to WKWebView so nested controls can consume Escape first.
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(MOUSE_DOWN, &local_block)
    }.ok_or(NativeError::PanelFailed)?);

    let outside_clicks = controller.clone();
    let global_block = RcBlock::new(move |_: NonNull<NSEvent>| {
        // Global monitoring sees only OTHER applications. Mouse-only monitoring needs no
        // accessibility permission and cannot consume the destination application's event.
        enqueue_dismissal(outside_clicks.clone(), panel_id, generation, Dismissal::Explicit);
    });
    monitors.global = Some(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(MOUSE_DOWN, &global_block)
        .ok_or(NativeError::PanelFailed)?);

    let focus = controller.clone();
    let focus_block = RcBlock::new(move |_: NonNull<NSNotification>| {
        enqueue_dismissal(focus.clone(), panel_id, generation, Dismissal::ReconcileFocus);
    });
    let displays = controller;
    let display_block = RcBlock::new(move |_: NonNull<NSNotification>| {
        let controller = displays.clone();
        tauri::async_runtime::spawn(async move {
            let _ = controller.on_main(move |controller| {
                let anchor = MONITORS.with(|slot| slot.borrow().as_ref()
                    .filter(|monitors| monitors.generation == generation).map(|monitors| monitors.anchor));
                if let Some(anchor) = anchor {
                    let result = companion_window().and_then(|window| position(&window, anchor));
                    if let Err(error) = result { controller.native_failure(error); }
                }
            }).await;
        });
    });
    let center = NSNotificationCenter::defaultCenter();
    // These AppKit notifications are posted on main; nil queue preserves that affinity.
    unsafe {
        monitors.notifications.push(center.addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidResignActiveNotification), None, None, &focus_block,
        ));
        // Observe every key resignation, not just the panel's: an owned popup or sheet can
        // become key, then lose focus to another app without a second panel notification.
        monitors.notifications.push(center.addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidResignKeyNotification), None, None, &focus_block,
        ));
        monitors.notifications.push(center.addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidChangeScreenParametersNotification), None, None, &display_block,
        ));
    }
    MONITORS.with(|slot| *slot.borrow_mut() = Some(monitors));
    Ok(())
}

fn current_monitor(generation: uuid::Uuid) -> bool {
    MONITORS.with(|slot| slot.borrow().as_ref().is_some_and(|monitors| monitors.generation == generation))
}

fn is_owned_window(window: &NSWindow, panel: usize) -> bool {
    std::ptr::from_ref(window) as usize == panel
        || window.parentWindow().is_some_and(|parent| is_owned_window(&parent, panel))
        || window.sheetParent().is_some_and(|parent| is_owned_window(&parent, panel))
}

fn enqueue_dismissal(controller: CompanionController, panel: usize, generation: uuid::Uuid, dismissal: Dismissal) {
    let enqueue = MONITORS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(monitors) = slot.as_mut().filter(|monitors| monitors.generation == generation) else { return false; };
        let enqueue = monitors.pending_dismissal.is_none();
        // Outside clicks cannot be downgraded by a later resign-key notification.
        if enqueue || matches!(dismissal, Dismissal::Explicit) {
            monitors.pending_dismissal = Some(dismissal);
        }
        enqueue
    });
    if !enqueue { return; }
    // One queued dismissal per opening bounds repeated events and waits for AppKit to
    // finish synchronous focus changes before reconciling the replacement key window.
    tauri::async_runtime::spawn(async move {
        let _ = controller.on_main(move |controller| {
            let dismissal = MONITORS.with(|slot| slot.borrow_mut().as_mut()
                .filter(|monitors| monitors.generation == generation)
                .and_then(|monitors| monitors.pending_dismissal.take()));
            match dismissal {
                Some(Dismissal::Explicit) => {
                    if let Err(error) = controller.dismiss_now() { controller.native_failure(error); }
                }
                Some(Dismissal::ReconcileFocus) => reconcile_focus(controller, panel),
                None => {}
            }
        }).await;
    });
}

fn reconcile_focus(controller: CompanionController, panel: usize) {
    // Resignation precedes AppKit's replacement key assignment; inspect ownership afterward.
    let Some(marker) = MainThreadMarker::new() else { return; };
    let application = NSApplication::sharedApplication(marker);
    let key = application.keyWindow();
    // A nonactivating panel (or its popup/sheet) can own keyboard focus while GitView is inactive.
    if key.as_ref().is_some_and(|window| is_owned_window(window, panel)) { return; }
    let main_focused = application.isActive() && controller.app.get_webview_window("main")
        .and_then(|window| native_window(&window).ok())
        .is_some_and(|main| key.as_ref()
            .is_some_and(|key| is_owned_window(key, std::ptr::from_ref(&*main) as usize)));
    controller.focus_lost_now(main_focused);
}
