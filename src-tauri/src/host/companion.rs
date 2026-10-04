//! Owns optional native access and synchronous surface invalidation, never repository state.
//! Native transitions and preference replacement are ordered on the host event thread.

mod preferences;
#[cfg(target_os = "macos")]
mod macos;

use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use preferences::Preferences;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PersistenceError { LoadFailed, UnsupportedVersion, SaveFailed, StorageUnavailable }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeError { TrayFailed, PanelFailed, ShowFailed, FocusFailed, MainUnavailable }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompanionState {
    pub supported: bool,
    pub enabled: bool,
    pub available: bool,
    pub visible: bool,
    pub revision: u64,
    pub persistence_error: Option<PersistenceError>,
    pub native_error: Option<NativeError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EnableKind { Applied, Unavailable }

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EnableResult { pub kind: EnableKind, pub state: CompanionState }

/// Only canonical, translated fixed labels may cross this host boundary.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MenuLabels { pub open_git_view: String, pub quit: String }

impl MenuLabels {
    fn valid(&self) -> bool {
        [&self.open_git_view, &self.quit].into_iter()
            .all(|label| !label.trim().is_empty() && label.len() <= 512 && !label.contains('\0'))
    }
}

/// Callbacks are synchronous authority updates. They must not call back into this controller.
#[derive(Clone)]
pub(crate) struct HostCallbacks {
    pub access_changed: Arc<dyn Fn(bool, bool) + Send + Sync>,
    pub open: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    pub hide: Arc<dyn Fn() + Send + Sync>,
    pub main_visibility: Arc<dyn Fn(bool) + Send + Sync>,
    pub quit: Arc<dyn Fn() + Send + Sync>,
}

trait NativeAccess {
    fn create_tray(&mut self) -> Result<(), NativeError>;
    fn reveal_main(&mut self) -> Result<(), NativeError>;
    fn remove_companion(&mut self);
}

struct FocusTransfer { request_id: String, source_open_epoch: String }

struct Lifecycle {
    state: CompanionState,
    preferences: Preferences,
    labels: Option<MenuLabels>,
    restore_pending: bool,
    open_epoch: String,
    transfer: Option<FocusTransfer>,
    quitting: bool,
}

impl Lifecycle {
    fn new(supported: bool, preferences: Preferences) -> Self {
        Self {
            restore_pending: supported && preferences.enabled,
            state: CompanionState {
                supported, enabled: supported && preferences.enabled, available: false,
                visible: false, revision: 0, persistence_error: preferences.error, native_error: None,
            },
            preferences, labels: None, open_epoch: String::new(), transfer: None, quitting: false,
        }
    }

    fn set_enabled(&mut self, enabled: bool, native: &mut impl NativeAccess) -> EnableResult {
        if !self.state.supported || self.quitting { return self.result(EnableKind::Unavailable); }
        let mut kind = EnableKind::Applied;
        if enabled {
            self.state.enabled = true;
            if !self.state.available {
                match native.create_tray() {
                    Ok(()) => { self.state.available = true; self.state.native_error = None; }
                    Err(error) => {
                        self.state.native_error = Some(error);
                        if native.reveal_main().is_err() {
                            self.state.native_error = Some(NativeError::MainUnavailable);
                        }
                        kind = EnableKind::Unavailable;
                    }
                }
            }
        } else {
            if let Err(error) = native.reveal_main() {
                self.state.native_error = Some(error);
                self.state.revision += 1;
                return self.result(EnableKind::Unavailable);
            }
            native.remove_companion();
            self.hidden();
            self.state.enabled = false;
            self.state.available = false;
            self.state.native_error = None;
        }
        self.preferences.save(enabled);
        self.state.persistence_error = self.preferences.error;
        self.state.revision += 1;
        self.result(kind)
    }

    fn result(&self, kind: EnableKind) -> EnableResult { EnableResult { kind, state: self.state.clone() } }

    fn native_failed(&mut self, error: NativeError) {
        self.state.available = false;
        self.state.native_error = Some(error);
        self.state.revision += 1;
    }

    fn opened(&mut self, epoch: String) {
        self.open_epoch = epoch;
        self.transfer = None;
        self.state.visible = true;
        self.state.native_error = None;
        self.state.revision += 1;
    }

    fn hidden(&mut self) {
        self.state.visible = false;
        self.transfer = None;
        self.open_epoch.clear();
        self.state.revision += 1;
    }

    fn begin_focus_transfer(&mut self, request_id: String, source_open_epoch: String) -> bool {
        if !self.state.visible || self.open_epoch != source_open_epoch || self.quitting { return false; }
        self.transfer = Some(FocusTransfer { request_id, source_open_epoch });
        true
    }

    fn suppress_focus_dismissal(&self, main_focused: bool) -> bool {
        main_focused && self.transfer.as_ref().is_some_and(|transfer| self.state.visible && transfer.source_open_epoch == self.open_epoch)
    }

    fn finish_focus_transfer(&mut self, request_id: &str, epoch: &str) -> bool {
        let current = self.transfer.as_ref().is_some_and(|transfer| {
            transfer.request_id == request_id && transfer.source_open_epoch == epoch
                && self.open_epoch == epoch && self.state.visible && !self.quitting
        });
        if current { self.transfer = None; }
        current
    }
}

#[derive(Clone)]
pub(crate) struct CompanionController {
    app: AppHandle,
    lifecycle: Arc<Mutex<Lifecycle>>,
    callbacks: HostCallbacks,
}

/// Restores intent only. The first authoritative translated presentation creates native access.
pub(crate) fn initialize(app: &AppHandle, callbacks: HostCallbacks) -> CompanionController {
    let supported = cfg!(target_os = "macos");
    let preferences = if supported {
        Preferences::load(app.path().app_data_dir().ok().map(|directory| directory.join("companion.json")))
    } else { Preferences::unsupported() };
    let controller = CompanionController { app: app.clone(), lifecycle: Arc::new(Mutex::new(Lifecycle::new(supported, preferences))), callbacks };
    #[cfg(target_os = "macos")]
    if let Err(error) = macos::initialize(controller.clone()) {
        controller.lifecycle.lock().native_failed(error);
    }
    controller.refresh_main_visibility();
    controller
}

impl CompanionController {
    pub fn state(&self) -> CompanionState { self.lifecycle.lock().state.clone() }

    async fn on_main<T: Send + 'static>(&self, operation: impl FnOnce(Self) -> T + Send + 'static) -> Result<T, NativeError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let controller = self.clone();
        self.app.run_on_main_thread(move || { let _ = sender.send(operation(controller)); })
            .map_err(|_| NativeError::MainUnavailable)?;
        receiver.await.map_err(|_| NativeError::MainUnavailable)
    }

    pub async fn set_enabled(&self, enabled: bool) -> EnableResult {
        match self.on_main(move |controller| controller.set_enabled_now(enabled)).await {
            Ok(result) => result,
            Err(error) => {
                let mut lifecycle = self.lifecycle.lock();
                lifecycle.state.native_error = Some(error);
                lifecycle.state.revision += 1;
                lifecycle.result(EnableKind::Unavailable)
            }
        }
    }

    fn set_enabled_now(&self, enabled: bool) -> EnableResult {
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.restore_pending = false;
        let labels = lifecycle.labels.clone();
        let mut native = HostAccess { controller: self, labels: labels.as_ref() };
        let result = lifecycle.set_enabled(enabled, &mut native);
        (self.callbacks.access_changed)(result.state.enabled, result.state.available);
        drop(lifecycle);
        self.refresh_main_visibility();
        result
    }

    pub async fn publish_menu_labels(&self, labels: MenuLabels) -> Result<(), NativeError> {
        if !labels.valid() { return Err(NativeError::TrayFailed); }
        self.on_main(move |controller| {
            let mut lifecycle = controller.lifecycle.lock();
            if lifecycle.quitting || !lifecycle.state.supported { return Ok(()); }
            #[cfg(target_os = "macos")]
            if lifecycle.labels.as_ref() != Some(&labels) && macos::has_tray(&controller.app) {
                if let Err(error) = macos::update_menu(&controller.app, &labels) {
                    lifecycle.labels = Some(labels);
                    drop(lifecycle);
                    controller.native_failure(error);
                    if controller.state().native_error != Some(NativeError::MainUnavailable) {
                        macos::remove(&controller.app);
                    }
                    return Err(error);
                }
            }
            lifecycle.labels = Some(labels);
            let restore = lifecycle.restore_pending;
            drop(lifecycle);
            if restore {
                let result = controller.set_enabled_now(true);
                if result.kind == EnableKind::Unavailable {
                    return Err(result.state.native_error.unwrap_or(NativeError::TrayFailed));
                }
            }
            Ok(())
        }).await?
    }

    pub async fn dismiss(&self) -> Result<(), NativeError> {
        self.on_main(|controller| controller.dismiss_now()).await?
    }

    fn dismiss_now(&self) -> Result<(), NativeError> {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.quitting || !lifecycle.state.visible { return Ok(()); }
        // Revoke native read authority before any queued renderer/native hide completion.
        (self.callbacks.hide)();
        lifecycle.hidden();
        #[cfg(target_os = "macos")]
        if let Err(error) = macos::hide(&self.app) {
            lifecycle.state.visible = true;
            lifecycle.state.native_error = Some(error);
            (self.callbacks.access_changed)(lifecycle.state.enabled, lifecycle.state.available);
            return Err(error);
        }
        Ok(())
    }

    pub async fn reveal_main(&self) -> Result<(), NativeError> {
        self.on_main(|controller| controller.reveal_main_now()).await?
    }

    fn reveal_main_now(&self) -> Result<(), NativeError> {
        if self.lifecycle.lock().quitting { return Err(NativeError::MainUnavailable); }
        reveal_main(&self.app, &self.callbacks)
    }

    /// Called before the host reveals main for one admitted handoff.
    pub fn begin_focus_transfer(&self, request_id: &str, source_open_epoch: &str) -> bool {
        self.lifecycle.lock().begin_focus_transfer(request_id.to_owned(), source_open_epoch.to_owned())
    }

    pub async fn finish_focus_transfer(&self, request_id: &str, source_open_epoch: &str, acknowledged: bool) -> Result<(), NativeError> {
        let request_id = request_id.to_owned();
        let source_open_epoch = source_open_epoch.to_owned();
        self.on_main(move |controller| {
            if !controller.lifecycle.lock().finish_focus_transfer(&request_id, &source_open_epoch) { return Ok(()); }
            if acknowledged { return controller.dismiss_now(); }
            #[cfg(target_os = "macos")]
            if let Err(error) = macos::focus(&controller.app) {
                controller.native_failure(error);
                return Err(error);
            }
            Ok(())
        }).await?
    }

    /// True means the native close event was replaced by a successful main hide.
    pub fn handle_main_close(&self) -> bool {
        let lifecycle = self.lifecycle.lock();
        if lifecycle.quitting || !lifecycle.state.enabled || !lifecycle.state.available { return false; }
        drop(lifecycle);
        if let Some(main) = self.app.get_webview_window("main") {
            (self.callbacks.main_visibility)(false);
            if main.hide().is_ok() { return true; }
            self.refresh_main_visibility();
        }
        false
    }

    pub fn handle_main_focus(&self, _focused: bool) {
        self.refresh_main_visibility();
    }

    /// Query actual shown/minimized state; losing keyboard focus does not hide a visible main.
    pub fn refresh_main_visibility(&self) {
        {
            let Some(lifecycle) = self.lifecycle.try_lock() else {
                let controller = self.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = controller.on_main(|controller| controller.refresh_main_visibility()).await;
                });
                return;
            };
            if lifecycle.quitting || !lifecycle.state.supported { return; }
        }
        #[cfg(target_os = "macos")]
        let visible = macos::main_visible(&self.app);
        #[cfg(not(target_os = "macos"))]
        let visible = true;
        (self.callbacks.main_visibility)(visible);
    }

    /// Called by the host's destruction hook before disposing the surface subscription.
    pub fn handle_companion_destroyed(&self) {
        (self.callbacks.hide)();
        if let Some(mut lifecycle) = self.lifecycle.try_lock() {
            lifecycle.hidden();
        } else {
            // Native destruction may synchronously reenter its owning enable/disable transition.
            // Its authority is already revoked above; never hide a newly created replacement.
            let controller = self.clone();
            tauri::async_runtime::spawn(async move {
                let _ = controller.on_main(|controller| {
                    if controller.app.get_webview_window("companion").is_none() {
                        controller.lifecycle.lock().hidden();
                    }
                }).await;
            });
        }
        #[cfg(target_os = "macos")]
        macos::clear_monitors();
    }

    pub fn handle_companion_close(&self) -> bool {
        {
            // A programmatic teardown already owns invalidation and must not be close-to-tray intercepted.
            let Some(lifecycle) = self.lifecycle.try_lock() else { return false; };
            if lifecycle.quitting { return false; }
        }
        self.dismiss_now().is_ok()
    }

    /// Blocks admission synchronously; normal host shutdown remains responsible for its drain.
    pub fn quit_cleanup(&self) {
        {
            let mut lifecycle = self.lifecycle.lock();
            if lifecycle.quitting { return; }
            lifecycle.quitting = true;
            (self.callbacks.hide)();
            (self.callbacks.main_visibility)(false);
            (self.callbacks.access_changed)(false, false);
            lifecycle.hidden();
            lifecycle.state.available = false;
        }
        #[cfg(target_os = "macos")]
        {
            let app = self.app.clone();
            let _ = self.app.run_on_main_thread(move || macos::shutdown(&app));
        }
    }

    #[cfg(target_os = "macos")]
    fn request_quit(&self) { self.quit_cleanup(); (self.callbacks.quit)(); }

    #[cfg(target_os = "macos")]
    fn toggle_now(&self, anchor: macos::Anchor) {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.quitting || !lifecycle.state.enabled || !lifecycle.state.available { return; }
        if lifecycle.state.visible {
            drop(lifecycle);
            if let Err(error) = self.dismiss_now() { self.native_failure(error); }
            return;
        }
        // No untranslated renderer is exposed before the authoritative main publisher is ready.
        if lifecycle.labels.is_none() { return; }
        match macos::show(&self.app, self.clone(), anchor) {
            Ok(()) => {
                if let Some(epoch) = (self.callbacks.open)() {
                    lifecycle.opened(epoch);
                } else {
                    (self.callbacks.hide)();
                    lifecycle.hidden();
                    lifecycle.state.available = false;
                    (self.callbacks.access_changed)(lifecycle.state.enabled, false);
                    macos::remove(&self.app);
                }
            }
            Err(error) => {
                drop(lifecycle);
                self.native_failure(error);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn native_failure(&self, error: NativeError) {
        if self.lifecycle.lock().quitting { return; }
        // Failure still revokes authority; a refused orderOut is reported while main stays reachable.
        let _ = self.dismiss_now();
        let revealed = reveal_main(&self.app, &self.callbacks).is_ok();
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.native_failed(if revealed { error } else { NativeError::MainUnavailable });
        (self.callbacks.access_changed)(lifecycle.state.enabled, lifecycle.state.available);
    }

    #[cfg(target_os = "macos")]
    fn main_reveal_failed(&self) {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.quitting { return; }
        lifecycle.state.native_error = Some(NativeError::MainUnavailable);
        lifecycle.state.revision += 1;
        (self.callbacks.access_changed)(lifecycle.state.enabled, lifecycle.state.available);
    }

    #[cfg(target_os = "macos")]
    fn focus_lost_now(&self, main_focused: bool) {
        if !self.lifecycle.lock().suppress_focus_dismissal(main_focused) {
            if let Err(error) = self.dismiss_now() { self.native_failure(error); }
        }
    }
}

struct HostAccess<'a> { controller: &'a CompanionController, labels: Option<&'a MenuLabels> }

impl NativeAccess for HostAccess<'_> {
    fn create_tray(&mut self) -> Result<(), NativeError> {
        #[cfg(target_os = "macos")]
        { macos::create_tray(&self.controller.app, self.controller.clone(), self.labels.ok_or(NativeError::TrayFailed)?) }
        #[cfg(not(target_os = "macos"))]
        { Err(NativeError::TrayFailed) }
    }
    fn reveal_main(&mut self) -> Result<(), NativeError> { reveal_main(&self.controller.app, &self.controller.callbacks) }
    fn remove_companion(&mut self) {
        (self.controller.callbacks.hide)();
        #[cfg(target_os = "macos")]
        macos::remove(&self.controller.app);
    }
}

fn reveal_main(app: &AppHandle, callbacks: &HostCallbacks) -> Result<(), NativeError> {
    let main = app.get_webview_window("main").ok_or(NativeError::MainUnavailable)?;
    main.unminimize().and_then(|_| main.show()).and_then(|_| main.set_focus())
        .map_err(|_| NativeError::MainUnavailable)?;
    (callbacks.main_visibility)(true);
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/integration/companion_lifecycle.rs"]
mod integration_tests;
