//! Exercises persistence and native reachability policy without claiming AppKit proof.

use super::{EnableKind, Lifecycle, NativeAccess, NativeError, PersistenceError, Preferences};
use std::fs;

#[derive(Default)]
struct NativeFixture {
    tray_count: usize,
    main_revealed: bool,
    fail_tray: bool,
    fail_main: bool,
}

impl NativeAccess for NativeFixture {
    fn create_tray(&mut self) -> Result<(), NativeError> {
        if self.fail_tray { return Err(NativeError::TrayFailed); }
        self.tray_count += 1;
        Ok(())
    }
    fn reveal_main(&mut self) -> Result<(), NativeError> {
        if self.fail_main { return Err(NativeError::MainUnavailable); }
        self.main_revealed = true;
        Ok(())
    }
    fn remove_companion(&mut self) { self.tray_count = 0; }
}

fn lifecycle(path: std::path::PathBuf) -> Lifecycle {
    Lifecycle::new(true, Preferences::load(Some(path)))
}

#[test]
fn missing_preferences_start_disabled_and_explicit_choice_survives_reload() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("companion.json");
    let mut lifecycle = lifecycle(path.clone());
    assert!(!lifecycle.state.enabled);
    assert_eq!(lifecycle.state.persistence_error, None);
    let result = lifecycle.set_enabled(true, &mut NativeFixture::default());
    assert_eq!(result.kind, EnableKind::Applied);
    assert!(Preferences::load(Some(path)).enabled);
}

#[test]
fn malformed_bytes_are_preserved_even_after_session_enable_and_disable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("companion.json");
    fs::write(&path, b"broken-private-bytes").unwrap();
    let mut lifecycle = lifecycle(path.clone());
    let mut native = NativeFixture::default();
    assert_eq!(lifecycle.state.persistence_error, Some(PersistenceError::LoadFailed));
    assert!(lifecycle.set_enabled(true, &mut native).state.available);
    lifecycle.set_enabled(false, &mut native);
    assert_eq!(fs::read(path).unwrap(), b"broken-private-bytes");
    assert_eq!(lifecycle.state.persistence_error, Some(PersistenceError::LoadFailed));
}

#[test]
fn future_schema_is_classified_before_version_one_fields_and_never_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("companion.json");
    let bytes = br#"{"version":2,"newSchema":true}"#;
    fs::write(&path, bytes).unwrap();
    let mut lifecycle = lifecycle(path.clone());
    assert_eq!(lifecycle.state.persistence_error, Some(PersistenceError::UnsupportedVersion));
    lifecycle.set_enabled(true, &mut NativeFixture::default());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn unreadable_storage_is_protected_and_unknown_fields_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let preferences = Preferences::load(Some(directory.path().to_owned()));
    assert_eq!(preferences.error, Some(PersistenceError::LoadFailed));
    let path = directory.path().join("companion.json");
    fs::write(&path, br#"{"version":1,"enabled":true,"other":1}"#).unwrap();
    assert_eq!(Preferences::load(Some(path)).error, Some(PersistenceError::LoadFailed));
}

#[test]
fn save_failure_keeps_effective_session_choice_and_next_choice_can_save() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("blocked");
    let path = parent.join("companion.json");
    let mut lifecycle = lifecycle(path.clone());
    fs::write(&parent, b"not-a-directory").unwrap();
    let mut native = NativeFixture::default();
    let result = lifecycle.set_enabled(true, &mut native);
    assert_eq!(result.kind, EnableKind::Applied);
    assert!(result.state.enabled && result.state.available);
    assert_eq!(result.state.persistence_error, Some(PersistenceError::SaveFailed));
    fs::remove_file(parent).unwrap();
    let result = lifecycle.set_enabled(false, &mut native);
    assert_eq!(result.state.persistence_error, None);
    assert!(!Preferences::load(Some(path)).enabled);
}

#[test]
fn repeated_enable_owns_one_tray_and_native_failure_never_claims_availability() {
    let directory = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(directory.path().join("companion.json"));
    let mut native = NativeFixture { fail_tray: true, ..Default::default() };
    let failed = lifecycle.set_enabled(true, &mut native);
    assert_eq!(failed.kind, EnableKind::Unavailable);
    assert!(failed.state.enabled);
    assert!(!failed.state.available);
    assert_eq!(failed.state.native_error, Some(NativeError::TrayFailed));
    assert!(native.main_revealed);
    native.fail_tray = false;
    lifecycle.set_enabled(true, &mut native);
    lifecycle.set_enabled(true, &mut native);
    assert_eq!(native.tray_count, 1);
}

#[test]
fn disabling_without_main_access_retains_tray_and_saved_enabled_choice() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("companion.json");
    let mut lifecycle = lifecycle(path.clone());
    let mut native = NativeFixture::default();
    lifecycle.set_enabled(true, &mut native);
    native.fail_main = true;
    let failed = lifecycle.set_enabled(false, &mut native);
    assert_eq!(failed.kind, EnableKind::Unavailable);
    assert!(failed.state.enabled && failed.state.available);
    assert_eq!(failed.state.native_error, Some(NativeError::MainUnavailable));
    assert_eq!(native.tray_count, 1);
    assert!(Preferences::load(Some(path)).enabled);
}

#[test]
fn unsupported_platform_never_creates_native_access_or_writes_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("companion.json");
    let mut lifecycle = Lifecycle::new(false, Preferences::unsupported());
    let mut native = NativeFixture::default();
    let result = lifecycle.set_enabled(true, &mut native);
    assert_eq!(result.kind, EnableKind::Unavailable);
    assert!(!result.state.supported && !result.state.enabled);
    assert_eq!(native.tray_count, 0);
    assert!(!path.exists());
}

#[test]
fn missing_app_data_reports_storage_unavailable_without_blocking_session_enable() {
    let mut lifecycle = Lifecycle::new(true, Preferences::load(None));
    let result = lifecycle.set_enabled(true, &mut NativeFixture::default());
    assert_eq!(result.kind, EnableKind::Applied);
    assert!(result.state.available);
    assert_eq!(result.state.persistence_error, Some(PersistenceError::StorageUnavailable));
}

#[test]
fn focus_transfer_timeout_restores_only_current_not_explicitly_dismissed_source() {
    let mut lifecycle = Lifecycle::new(true, Preferences::load(None));
    lifecycle.opened("open-1".into());
    assert!(lifecycle.begin_focus_transfer("request-1".into(), "open-1".into()));
    assert!(lifecycle.suppress_focus_dismissal(true));
    assert!(!lifecycle.suppress_focus_dismissal(false));
    assert!(lifecycle.finish_focus_transfer("request-1", "open-1"));
    assert!(!lifecycle.suppress_focus_dismissal(true));
    lifecycle.begin_focus_transfer("request-2".into(), "open-1".into());
    lifecycle.hidden();
    assert!(!lifecycle.finish_focus_transfer("request-2", "open-1"));
    lifecycle.opened("open-2".into());
    assert!(!lifecycle.finish_focus_transfer("request-2", "open-1"));
}

#[cfg(target_os = "macos")]
#[test]
fn negative_origin_small_display_clamps_and_shrinks_in_appkit_points() {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let visible = NSRect::new(NSPoint::new(-640.0, -300.0), NSSize::new(600.0, 400.0));
    let frame = super::macos::clamped_frame(super::macos::Anchor { x: -50.0, y: 130.0 }, visible);
    assert_eq!(frame, visible);
}

#[cfg(target_os = "macos")]
#[test]
fn wide_display_places_panel_below_pointer_without_physical_pixel_conversion() {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let visible = NSRect::new(NSPoint::new(-1920.0, 0.0), NSSize::new(1920.0, 1055.0));
    let frame = super::macos::clamped_frame(super::macos::Anchor { x: -500.0, y: 1080.0 }, visible);
    assert_eq!(frame.origin, NSPoint::new(-880.0, 495.0));
    assert_eq!(frame.size, NSSize::new(760.0, 560.0));
}

#[test]
fn panel_failure_revokes_availability_but_preserves_explicit_enabled_intent() {
    let mut lifecycle = Lifecycle::new(true, Preferences::load(None));
    let mut native = NativeFixture::default();
    lifecycle.set_enabled(true, &mut native);
    lifecycle.native_failed(NativeError::PanelFailed);
    assert!(lifecycle.state.enabled);
    assert!(!lifecycle.state.available);
    assert_eq!(lifecycle.state.native_error, Some(NativeError::PanelFailed));
    assert_eq!(native.tray_count, 1);
}
