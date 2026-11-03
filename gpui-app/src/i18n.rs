//! Minimal i18n layer for the native UI.
//!
//! The translation catalog was exported from the original TypeScript locales
//! and is bundled as JSON. Keys use the same dotted paths as the old app
//! (`"sidebar.heading"`, `"common.savedCount"`, …) so the rewrite keeps every
//! existing translation, including `{{name}}` interpolation placeholders.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const LANGUAGES: [Lang; 4] = [Lang::En, Lang::Zh, Lang::Ja, Lang::Ko];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    Zh,
    Ja,
    Ko,
}

impl Lang {
    pub const fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
        }
    }

    /// Native language name shown in the language picker.
    pub const fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "中文",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim().to_ascii_lowercase();
        if code.starts_with("zh") {
            Some(Lang::Zh)
        } else if code.starts_with("ja") {
            Some(Lang::Ja)
        } else if code.starts_with("ko") {
            Some(Lang::Ko)
        } else if code.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    fn raw(self) -> &'static str {
        match self {
            Lang::En => include_str!("locales/en.json"),
            Lang::Zh => include_str!("locales/zh.json"),
            Lang::Ja => include_str!("locales/ja.json"),
            Lang::Ko => include_str!("locales/ko.json"),
        }
    }

    fn dict(self) -> &'static HashMap<String, String> {
        static DICTS: OnceLock<[HashMap<String, String>; 4]> = OnceLock::new();
        let dicts = DICTS.get_or_init(|| {
            LANGUAGES.map(|lang| {
                let value: serde_json::Value =
                    serde_json::from_str(lang.raw()).expect("bundled translation JSON must parse");
                let mut flat = HashMap::new();
                flatten("", &value, &mut flat);
                flat
            })
        });
        let index = LANGUAGES.iter().position(|lang| *lang == self).unwrap_or(0);
        &dicts[index]
    }
}

fn flatten(prefix: &str, value: &serde_json::Value, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let next = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&next, child, out);
            }
        }
        serde_json::Value::String(text) => {
            out.insert(prefix.to_string(), text.clone());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// Translate a key, falling back to English and then to the key itself.
pub fn t(lang: Lang, key: &str) -> String {
    if let Some(value) = lang.dict().get(key) {
        return value.clone();
    }
    if let Some(value) = Lang::En.dict().get(key) {
        return value.clone();
    }
    key.to_string()
}

/// Translate a key and replace `{{name}}` placeholders with the given values.
pub fn tp(lang: Lang, key: &str, params: &[(&str, &str)]) -> String {
    let mut text = t(lang, key);
    for (name, value) in params {
        text = text.replace(&format!("{{{{{name}}}}}"), value);
    }
    text
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn ui_state_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LOCALPROXY_DATA_DIR") {
        return Some(PathBuf::from(path).join("ui.json"));
    }
    dirs::data_dir().map(|dir| dir.join("LocalProxy").join("ui.json"))
}

/// Load the saved language, or infer it from the system locale.
pub fn detect_language() -> Lang {
    if let Some(path) = ui_state_path() {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(code) = value.get("language").and_then(|value| value.as_str()) {
                    if let Some(lang) = Lang::from_code(code) {
                        return lang;
                    }
                }
            }
        }
    }
    if let Some(locale) = sys_locale::get_locale() {
        if let Some(lang) = Lang::from_code(&locale) {
            return lang;
        }
    }
    Lang::En
}

/// Persist the language choice next to the rest of LocalProxy's data.
pub fn save_language(lang: Lang) {
    let Some(path) = ui_state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({ "language": lang.code() });
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, text);
    }
}
