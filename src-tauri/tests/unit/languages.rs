//! Checks native language normalization without mutating process-global preferences.

use super::*;

#[test]
fn posix_preferences_keep_order_and_normalize_regions_and_scripts() {
    assert_eq!(linux_preferences(Some("fr_FR.UTF-8:pt_PT@euro:es_MX"), Some("en_GB.UTF-8"), Some("it_IT"), Some("pt_BR")),
        ["fr-FR", "pt-PT", "es-MX", "en-GB"]);
    assert_eq!(normalize_language("ZH_hant_tw.UTF-8"), Some("zh-Hant-TW".into()));
}

#[test]
fn c_posix_empty_and_invalid_preferences_do_not_become_languages() {
    assert_eq!(linux_preferences(Some("C:C.UTF-8:POSIX::not a tag:en--US"), Some("POSIX"), None, Some("C")), Vec::<String>::new());
    assert_eq!(normalize_language("/private/preferences"), None);
    assert_eq!(normalize_language("123-US"), None);
}

#[test]
fn effective_message_locale_overrides_lower_priority_environment() {
    assert!(linux_preferences(Some("it"), Some("C.UTF-8"), None, Some("it_IT.UTF-8")).is_empty());
    assert_eq!(linux_preferences(None, Some("de_DE.UTF-8"), Some("es_ES"), Some("it_IT")), ["de-DE"]);
    assert_eq!(linux_preferences(None, Some(""), Some("pt_PT.UTF-8"), Some("it_IT")), ["pt-PT"]);
    assert_eq!(linux_preferences(Some("fr:es_MX"), None, None, Some("it_IT")), ["fr", "es-MX", "it-IT"]);
}

#[test]
fn windows_multistring_preserves_preference_order_and_ignores_bad_utf16() {
    let words: Vec<u16> = "pt-PT\0en-GB\0\0".encode_utf16().collect();
    assert_eq!(windows_preferences(&words), ["pt-PT", "en-GB"]);
    assert_eq!(windows_preferences(&[0xd800, 0, 105, 116, 0, 0]), ["it"]);
}

#[test]
fn picker_locale_is_closed_and_titles_come_from_all_six_catalogs() {
    for (tag, catalog) in [
        ("pt-BR", include_str!("../../../src/i18n/locales/pt-BR.json")),
        ("pt-PT", include_str!("../../../src/i18n/locales/pt-PT.json")),
        ("it", include_str!("../../../src/i18n/locales/it.json")),
        ("es", include_str!("../../../src/i18n/locales/es.json")),
        ("en-US", include_str!("../../../src/i18n/locales/en-US.json")),
        ("en-GB", include_str!("../../../src/i18n/locales/en-GB.json")),
    ] {
        let locale: Locale = serde_json::from_value(serde_json::json!(tag)).unwrap();
        let catalog: serde_json::Value = serde_json::from_str(catalog).unwrap();
        assert_eq!(locale.picker_title(), catalog["native.pickerTitle"].as_str().unwrap());
    }
    assert!(serde_json::from_value::<Locale>(serde_json::json!("fr-FR")).is_err());
    assert!(serde_json::from_value::<Locale>(serde_json::json!("Choose anything")).is_err());
}
