//! Main-thread AppKit adapter for one retained template tray and a lazy local NSWindow.
//! Companion dismissal monitors live only while shown; main visibility observers live until Quit.

use std::{cell::RefCell, ptr::NonNull};
use block2::RcBlock;
use objc2::{rc::Retained, runtime::{AnyClass, AnyObject, ProtocolObject}, MainThreadMarker, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSApplicationDidResignActiveNotification,
    NSApplicationDidHideNotification, NSApplicationDidUnhideNotification,
    NSEvent, NSEventMask, NSEventType, NSFloatingWindowLevel, NSScreen, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowDidResignKeyNotification,
    NSWindowDidMiniaturizeNotification, NSWindowDidDeminiaturizeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize};
use tauri::{
    image::Image, menu::{Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use super::{CompanionController, MenuLabels, NativeError};

const TRAY_ID: &str = "gitview-companion";
const OPEN_ID: &str = "companion-open-main";
const QUIT_ID: &str = "companion-quit";

#[derive(Clone, Copy)]
pub(super) struct Anchor { pub x: f64, pub y: f64 }

struct Monitors {
    event: Retained<AnyObject>,
    notifications: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    anchor: Anchor,
    generation: uuid::Uuid,
}

impl Drop for Monitors {
    fn drop(&mut self) {
        // Every owner and removal runs on the AppKit main thread.
        unsafe {
            NSEvent::removeMonitor(&self.event);
            let center = NSNotificationCenter::defaultCenter();
            for observer in &self.notifications { center.removeObserver(observer); }
        }
    }
}

thread_local! { static MONITORS: RefCell<Option<Monitors>> = const { RefCell::new(None) }; }
thread_local! { static ACCESS_OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(Vec::new()) }; }

fn clear_access_observers() {
    ACCESS_OBSERVERS.with(|slot| {
        let center = NSNotificationCenter::defaultCenter();
        for observer in slot.borrow_mut().drain(..) {
            // Tokens were created on this thread and remain retained until removal.
            unsafe { center.removeObserver(&observer); }
        }
    });
}

fn install_access_observers(controller: CompanionController) -> Result<(), NativeError> {
    clear_access_observers();
    let main = controller.app.get_webview_window("main").ok_or(NativeError::MainUnavailable)?;
    let main = native_window(&main).map_err(|_| NativeError::MainUnavailable)?;
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        let controller = controller.clone();
        // Native show/focus may post synchronously while a lifecycle transition owns its lock.
        tauri::async_runtime::spawn(async move {
            let _ = controller.on_main(|controller| controller.refresh_main_visibility()).await;
        });
    });
    let center = NSNotificationCenter::defaultCenter();
    let observers = unsafe { vec![
        center.addObserverForName_object_queue_usingBlock(Some(NSWindowDidMiniaturizeNotification), Some(&main), None, &block),
        center.addObserverForName_object_queue_usingBlock(Some(NSWindowDidDeminiaturizeNotification), Some(&main), None, &block),
        center.addObserverForName_object_queue_usingBlock(Some(NSApplicationDidHideNotification), None, None, &block),
        center.addObserverForName_object_queue_usingBlock(Some(NSApplicationDidUnhideNotification), None, None, &block),
    ] };
    ACCESS_OBSERVERS.with(|slot| *slot.borrow_mut() = observers);
    Ok(())
}

pub(super) fn main_visible(app: &AppHandle) -> bool {
    let Some(marker) = MainThreadMarker::new() else { return false; };
    !NSApplication::sharedApplication(marker).isHidden()
        && app.get_webview_window("main").and_then(|window| native_window(&window).ok())
            .is_some_and(|window| window.isVisible() && !window.isMiniaturized())
}

pub(super) fn initialize(controller: CompanionController) -> Result<(), NativeError> {
    let visibility = controller.clone();
    let app = controller.app.clone();
    // Tauri menu listeners are host-global, not tray-owned: register once, never on re-enable.
    app.on_menu_event(move |_, event| {
        let state = controller.state();
        if !state.enabled || !has_tray(&controller.app) { return; }
        if event.id.as_ref() == OPEN_ID {
            if controller.reveal_main_now().is_err() { controller.main_reveal_failed(); }
            else if let Err(error) = controller.dismiss_now() { controller.native_failure(error); }
        } else if event.id.as_ref() == QUIT_ID { controller.request_quit(); }
    });
    install_access_observers(visibility)
}

fn menu(app: &AppHandle, labels: &MenuLabels) -> Result<Menu<tauri::Wry>, NativeError> {
    let open = MenuItem::with_id(app, OPEN_ID, &labels.open_git_view, true, None::<&str>)
        .map_err(|_| NativeError::TrayFailed)?;
    let quit = MenuItem::with_id(app, QUIT_ID, &labels.quit, true, None::<&str>)
        .map_err(|_| NativeError::TrayFailed)?;
    Menu::with_items(app, &[&open, &quit]).map_err(|_| NativeError::TrayFailed)
}

pub(super) fn has_tray(app: &AppHandle) -> bool { app.tray_by_id(TRAY_ID).is_some() }

pub(super) fn create_tray(app: &AppHandle, controller: CompanionController, labels: &MenuLabels) -> Result<(), NativeError> {
    if has_tray(app) {
        // Explicit retry may follow a failed translated-menu update.
        return update_menu(app, labels);
    }
    let menu = menu(app, labels)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(template_icon()).icon_as_template(true).menu(&menu).show_menu_on_left_click(false)
        .on_tray_icon_event(move |_, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                // Tauri delivers tray callbacks on the native event loop. Never convert its physical rect.
                if MainThreadMarker::new().is_none() { return; }
                let pointer = NSEvent::mouseLocation();
                controller.toggle_now(Anchor { x: pointer.x, y: pointer.y });
            }
        })
        .build(app).map_err(|_| NativeError::TrayFailed)?;
    Ok(())
}

pub(super) fn update_menu(app: &AppHandle, labels: &MenuLabels) -> Result<(), NativeError> {
    let tray = app.tray_by_id(TRAY_ID).ok_or(NativeError::TrayFailed)?;
    tray.set_menu(Some(menu(app, labels)?)).map_err(|_| NativeError::TrayFailed)
}

fn panel(app: &AppHandle) -> Result<WebviewWindow, NativeError> {
    if let Some(window) = app.get_webview_window("companion") { return Ok(window); }
    WebviewWindowBuilder::new(app, "companion", WebviewUrl::App("index.html".into()))
        .title("GitView").inner_size(760.0, 560.0).decorations(false).resizable(false)
        .visible(false).focused(false).focusable(true).skip_taskbar(true)
        .build().map_err(|_| NativeError::PanelFailed)
}

fn native_window(window: &WebviewWindow) -> Result<Retained<NSWindow>, NativeError> {
    MainThreadMarker::new().ok_or(NativeError::PanelFailed)?;
    let pointer = window.ns_window().map_err(|_| NativeError::PanelFailed)?;
    // Tauri owns an NSWindow (not an NSPanel); retain it only for this main-thread operation.
    unsafe { Retained::retain(pointer.cast::<NSWindow>()).ok_or(NativeError::PanelFailed) }
}

pub(super) fn show(app: &AppHandle, controller: CompanionController, anchor: Anchor) -> Result<(), NativeError> {
    let window = panel(app)?;
    let native = native_window(&window)?;
    native.setLevel(NSFloatingWindowLevel);
    native.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
    position(&native, anchor)?;
    window.show().map_err(|_| NativeError::ShowFailed)?;
    if let Err(error) = focus(app) {
        native.orderOut(None);
        return Err(error);
    }
    if let Err(error) = install_monitors(controller, &native, anchor) {
        native.orderOut(None);
        return Err(error);
    }
    Ok(())
}

pub(super) fn focus(app: &AppHandle) -> Result<(), NativeError> {
    let window = app.get_webview_window("companion").ok_or(NativeError::PanelFailed)?;
    window.show().map_err(|_| NativeError::ShowFailed)?;
    window.set_focus().map_err(|_| NativeError::FocusFailed)?;
    let native = native_window(&window)?;
    let content = native.contentView().ok_or(NativeError::FocusFailed)?;
    let class = AnyClass::get(c"WKWebView").ok_or(NativeError::FocusFailed)?;
    let webview = find_webview(&content, class).ok_or(NativeError::FocusFailed)?;
    if !native.makeFirstResponder(Some(&webview)) { return Err(NativeError::FocusFailed); }
    Ok(())
}

fn find_webview(view: &NSView, class: &AnyClass) -> Option<Retained<NSView>> {
    if view.isKindOfClass(class) { return Some(view.retain()); }
    for child in view.subviews() {
        if let Some(webview) = find_webview(&child, class) { return Some(webview); }
    }
    None
}

pub(super) fn clear_monitors() {
    MONITORS.with(|slot| { slot.borrow_mut().take(); });
}

pub(super) fn hide(app: &AppHandle) -> Result<(), NativeError> {
    clear_monitors();
    if let Some(window) = app.get_webview_window("companion") {
        let native = native_window(&window)?;
        native.orderOut(None);
        if native.isVisible() { return Err(NativeError::ShowFailed); }
    }
    Ok(())
}

pub(super) fn remove(app: &AppHandle) {
    // Destruction follows even when ordering out is refused; main was revealed before disable.
    let _ = hide(app);
    app.remove_tray_by_id(TRAY_ID);
    if let Some(window) = app.get_webview_window("companion") { let _ = window.destroy(); }
}

pub(super) fn shutdown(app: &AppHandle) {
    clear_access_observers();
    remove(app);
}

pub(super) fn clamped_frame(anchor: Anchor, visible: NSRect) -> NSRect {
    let width = 760.0_f64.min(visible.size.width);
    let height = 560.0_f64.min(visible.size.height);
    let x = (anchor.x - width / 2.0).clamp(visible.origin.x, visible.origin.x + visible.size.width - width);
    let top = anchor.y.min(visible.origin.y + visible.size.height);
    let y = (top - height).clamp(visible.origin.y, visible.origin.y + visible.size.height - height);
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn position(window: &NSWindow, anchor: Anchor) -> Result<(), NativeError> {
    let marker = MainThreadMarker::new().ok_or(NativeError::PanelFailed)?;
    let screens = NSScreen::screens(marker);
    // Nearest remaining screen also handles a removed display and negative desktop origins.
    let screen = screens.iter().min_by(|left, right| {
        distance_to_frame(anchor, left.frame()).total_cmp(&distance_to_frame(anchor, right.frame()))
    }).ok_or(NativeError::ShowFailed)?;
    let visible = screen.visibleFrame();
    if visible.size.width <= 0.0 || visible.size.height <= 0.0 { return Err(NativeError::ShowFailed); }
    window.setFrame_display(clamped_frame(anchor, visible), true);
    Ok(())
}

fn distance_to_frame(anchor: Anchor, frame: NSRect) -> f64 {
    let x = anchor.x.clamp(frame.origin.x, frame.origin.x + frame.size.width);
    let y = anchor.y.clamp(frame.origin.y, frame.origin.y + frame.size.height);
    (anchor.x - x).powi(2) + (anchor.y - y).powi(2)
}

fn is_owned_window(window: &NSWindow, panel: usize) -> bool {
    if std::ptr::from_ref(window) as usize == panel { return true; }
    if let Some(parent) = window.parentWindow() { return is_owned_window(&parent, panel); }
    if let Some(parent) = window.sheetParent() { return is_owned_window(&parent, panel); }
    false
}

fn enqueue_focus_loss(controller: CompanionController, panel: usize, generation: uuid::Uuid) {
    // The resign notification precedes AppKit's new key-window assignment. Reconcile afterward,
    // including popup ownership, rather than dismissing during that partial transition.
    tauri::async_runtime::spawn(async move {
        let _ = controller.on_main(move |controller| {
            if !current_monitor(generation) { return; }
            let Some(marker) = MainThreadMarker::new() else { return; };
            let application = NSApplication::sharedApplication(marker);
            let key = application.keyWindow();
            if application.isActive() && key.as_ref().is_some_and(|window| is_owned_window(window, panel)) { return; }
            let main_focused = application.isActive() && controller.app.get_webview_window("main")
                .and_then(|window| native_window(&window).ok())
                .is_some_and(|main| key.as_ref().is_some_and(|key| is_owned_window(key, std::ptr::from_ref(&*main) as usize)));
            controller.focus_lost_now(main_focused);
        }).await;
    });
}

fn current_monitor(generation: uuid::Uuid) -> bool {
    MONITORS.with(|slot| slot.borrow().as_ref().is_some_and(|monitors| monitors.generation == generation))
}

fn install_monitors(controller: CompanionController, panel: &NSWindow, anchor: Anchor) -> Result<(), NativeError> {
    clear_monitors();
    let generation = uuid::Uuid::new_v4();
    let tray = controller.app.tray_by_id(TRAY_ID).ok_or(NativeError::TrayFailed)?;
    let tray_window = tray.with_inner_tray_icon(|tray| {
        let marker = MainThreadMarker::new()?;
        tray.ns_status_item()?.button(marker)?.window()
            .map(|window| std::ptr::from_ref(&*window) as usize)
    }).map_err(|_| NativeError::TrayFailed)?.ok_or(NativeError::TrayFailed)?;
    let panel_id = std::ptr::from_ref(panel) as usize;
    let events = controller.clone();
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // AppKit invokes local monitors on the main thread with a live NSEvent.
        let event_ref = unsafe { event.as_ref() };
        let Some(marker) = MainThreadMarker::new() else { return event.as_ptr(); };
        let event_window = event_ref.window(marker);
        let owned = event_window.as_ref().is_some_and(|window| is_owned_window(window, panel_id));
        let tray_event = event_window.as_ref().is_some_and(|window| std::ptr::from_ref(&**window) as usize == tray_window);
        if event_ref.r#type() == NSEventType::KeyDown {
            if event_ref.keyCode() == 53 {
                if let Err(error) = events.dismiss_now() { events.native_failure(error); }
                if owned { return std::ptr::null_mut(); }
            }
        } else if !owned && !tray_event {
            if let Err(error) = events.dismiss_now() { events.native_failure(error); }
        }
        event.as_ptr()
    });
    let event = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::KeyDown | NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown,
            &block,
        )
    }.ok_or(NativeError::PanelFailed)?;
    let center = NSNotificationCenter::defaultCenter();
    let deactivation = controller.clone();
    let deactivate_block = RcBlock::new(move |_: NonNull<NSNotification>| {
        enqueue_focus_loss(deactivation.clone(), panel_id, generation);
    });
    let key_loss = controller.clone();
    let key_block = RcBlock::new(move |_: NonNull<NSNotification>| {
        enqueue_focus_loss(key_loss.clone(), panel_id, generation);
    });
    let displays = controller;
    let display_block = RcBlock::new(move |_: NonNull<NSNotification>| {
        let controller = displays.clone();
        tauri::async_runtime::spawn(async move {
            let _ = controller.on_main(move |controller| {
                if !current_monitor(generation) { return; }
                let anchor = MONITORS.with(|slot| slot.borrow().as_ref().map(|monitors| monitors.anchor));
                if let Some(anchor) = anchor {
                    let result = controller.app.get_webview_window("companion").ok_or(NativeError::PanelFailed)
                        .and_then(|window| native_window(&window)).and_then(|window| position(&window, anchor));
                    if let Err(error) = result { controller.native_failure(error); }
                }
            }).await;
        });
    });
    // nil queue uses the posting thread; these AppKit notifications are delivered on main.
    let notifications = unsafe { vec![
        center.addObserverForName_object_queue_usingBlock(Some(NSApplicationDidResignActiveNotification), None, None, &deactivate_block),
        center.addObserverForName_object_queue_usingBlock(Some(NSWindowDidResignKeyNotification), Some(panel), None, &key_block),
        center.addObserverForName_object_queue_usingBlock(Some(NSApplicationDidChangeScreenParametersNotification), None, None, &display_block),
    ] };
    MONITORS.with(|slot| *slot.borrow_mut() = Some(Monitors { event, notifications, anchor, generation }));
    Ok(())
}

/// Rasterizes the checked-in favicon's rounded frame, branch and three nodes in template alpha.
/// Coordinates intentionally match public/favicon.svg; no colored background becomes a solid tray tile.
fn template_icon() -> Image<'static> {
    static RGBA: std::sync::LazyLock<[u8; 32 * 32 * 4]> = std::sync::LazyLock::new(rasterize_template);
    Image::new(&*RGBA, 32, 32)
}

fn rasterize_template() -> [u8; 32 * 32 * 4] {
    let mut rgba = [0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let mut coverage = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f64 * 2.0 + (sx as f64 + 0.5) / 2.0;
                    let py = y as f64 * 2.0 + (sy as f64 + 0.5) / 2.0;
                    if artwork_contains(px, py) { coverage += 1; }
                }
            }
            rgba[(y * 32 + x) * 4 + 3] = (coverage * 255 / 16) as u8;
        }
    }
    rgba
}

fn artwork_contains(x: f64, y: f64) -> bool {
    let qx = (x - 32.0).abs() - 12.0;
    let qy = (y - 32.0).abs() - 12.0;
    let frame_distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - 10.0;
    frame_distance.abs() <= 2.5
        || segment_distance(x, y, (22.0, 21.0), (22.0, 29.0)) <= 2.0
        || segment_distance(x, y, (22.0, 29.0), (32.0, 43.0)) <= 2.0
        || segment_distance(x, y, (32.0, 43.0), (42.0, 29.0)) <= 2.0
        || segment_distance(x, y, (42.0, 29.0), (42.0, 21.0)) <= 2.0
        || (x - 22.0).hypot(y - 21.0) <= 4.0
        || (x - 42.0).hypot(y - 21.0) <= 4.0
        || (x - 32.0).hypot(y - 43.0) <= 4.0
}

fn segment_distance(x: f64, y: f64, start: (f64, f64), end: (f64, f64)) -> f64 {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let t = (((x - start.0) * dx + (y - start.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (x - start.0 - t * dx).hypot(y - start.1 - t * dy)
}
