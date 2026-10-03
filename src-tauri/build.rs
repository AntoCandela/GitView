//! Declares native command permissions and the Windows activation dependency needed by test executables.

fn main() {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let windows = if windows_msvc {
        // The linker supplies one manifest to binaries and the library test harness.
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new()
    };
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows).app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace_snapshot",
            "open_chosen_repository",
            "select_context",
            "rename_repository",
            "remove_repository",
            "refresh_entry_availability",
            "observe_selected_context",
            "review_file",
            "history_page",
            "list_contexts",
            "select_worktree",
            "commit_files",
            "review_commit_file",
            "list_repository_files",
            "review_repository_file",
            "record_renderer_diagnostic",
            "diagnostic_health",
        ]),
    ))
    .expect("failed to build GitView host");
    if windows_msvc {
        // Tauri embeds this dependency for binaries, but not the library test harness.
        // https://github.com/orgs/tauri-apps/discussions/11179
        let manifest = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("missing Cargo output directory"))
            .join("common-controls.manifest");
        std::fs::write(&manifest, r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#).expect("failed to write Windows activation manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
