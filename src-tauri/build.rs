//! Declares native command permissions and the Windows activation dependency needed by test executables.

fn main() {
    generate_picker_titles();
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
            "preferred_languages",
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

fn generate_picker_titles() {
    let catalogs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/i18n/locales");
    let mut source = String::from("impl Locale {\n    pub(super) fn picker_title(self) -> &'static str {\n        match self {\n");
    for (tag, variant) in [
        ("pt-BR", "PtBr"), ("pt-PT", "PtPt"), ("it", "It"),
        ("es", "Es"), ("en-US", "EnUs"), ("en-GB", "EnGb"),
    ] {
        let path = catalogs.join(format!("{tag}.json"));
        println!("cargo:rerun-if-changed={}", path.display());
        let catalog: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("missing locale catalog for native picker"),
        ).expect("invalid locale catalog for native picker");
        let title = catalog.get("native.pickerTitle").and_then(serde_json::Value::as_str)
            .filter(|title| !title.trim().is_empty() && !title.contains(['{', '}', '\0']))
            .expect("native.pickerTitle must be nonempty plain text in every locale");
        source.push_str(&format!("            Self::{variant} => {title:?},\n"));
    }
    source.push_str("        }\n    }\n}\n");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("missing Cargo output directory"));
    std::fs::write(output.join("picker_titles.rs"), source).expect("failed to generate native picker titles");
}
