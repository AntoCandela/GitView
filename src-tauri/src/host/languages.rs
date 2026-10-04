//! Reads only OS UI-language preferences and maps the closed picker locale to bundled titles.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) enum Locale {
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "pt-PT")]
    PtPt,
    #[serde(rename = "it")]
    It,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "en-GB")]
    EnGb,
}

include!(concat!(env!("OUT_DIR"), "/picker_titles.rs"));

#[derive(Serialize)]
pub(super) struct PreferredLanguages {
    pub languages: Vec<String>,
}

pub(super) fn preferred_languages() -> PreferredLanguages {
    PreferredLanguages { languages: platform_preferences() }
}

pub(super) fn normalize_language(value: &str) -> Option<String> {
    let value = value.trim().split(['.', '@']).next()?;
    if value.eq_ignore_ascii_case("C") || value.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    let mut parts = value.split(['-', '_']);
    let language = parts.next()?;
    if !(2..=8).contains(&language.len()) || !language.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let mut normalized = language.to_ascii_lowercase();
    for part in parts {
        if part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        normalized.push('-');
        if part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            normalized.push_str(&part.to_ascii_uppercase());
        } else if part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            normalized.push_str(&part[..1].to_ascii_uppercase());
            normalized.push_str(&part[1..].to_ascii_lowercase());
        } else {
            normalized.push_str(&part.to_ascii_lowercase());
        }
    }
    Some(normalized)
}

#[cfg(any(target_os = "linux", test))]
fn linux_preferences(language: Option<&str>, lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> Vec<String> {
    let effective = [lc_all, lc_messages, lang].into_iter().flatten().find(|value| !value.trim().is_empty());
    // LC_ALL overrides LC_MESSAGES/LANG; the C locale also disables LANGUAGE translations.
    let Some(fallback) = effective.and_then(normalize_language) else { return Vec::new(); };
    language.into_iter().flat_map(|value| value.split(':')).filter_map(normalize_language)
        .chain(std::iter::once(fallback)).collect()
}

#[cfg(target_os = "linux")]
fn platform_preferences() -> Vec<String> {
    let language = std::env::var("LANGUAGE").ok();
    let lc_all = std::env::var("LC_ALL").ok();
    let lc_messages = std::env::var("LC_MESSAGES").ok();
    let lang = std::env::var("LANG").ok();
    linux_preferences(language.as_deref(), lc_all.as_deref(), lc_messages.as_deref(), lang.as_deref())
}

#[cfg(target_os = "macos")]
fn platform_preferences() -> Vec<String> {
    objc2_foundation::NSLocale::preferredLanguages().iter()
        .filter_map(|language| normalize_language(&language.to_string())).collect()
}

#[cfg(any(target_os = "windows", test))]
fn windows_preferences(buffer: &[u16]) -> Vec<String> {
    buffer.split(|word| *word == 0).take_while(|words| !words.is_empty())
        .filter_map(|words| String::from_utf16(words).ok())
        .filter_map(|language| normalize_language(&language)).collect()
}

#[cfg(target_os = "windows")]
fn platform_preferences() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0;
    let mut length = 0;
    // The first call obtains the UTF-16 buffer length, including its double-NUL terminator.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut length) } == 0 || length == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0; length as usize];
    // Windows writes at most the provided length. A preferences race fails closed to no preferences.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Vec::new();
    }
    windows_preferences(&buffer)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_preferences() -> Vec<String> { Vec::new() }

#[cfg(test)]
#[path = "../../tests/unit/languages.rs"]
mod unit_tests;
