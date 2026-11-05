//! 多語系（繁體中文／English／日本語）。
//!
//! 作法：**以繁體中文原文當作鍵**。程式碼裡寫 `tr("背景")`，英文與日文的對照表放在 `en.rs`、`ja.rs`。
//! - 目前語言是繁體中文，或表中查不到時，直接回傳原文（因此漏翻譯時畫面仍是可讀的中文，不會空白）。
//! - 含變數的句子用 `tr_fmt("已載入設定檔：{}", &[path])`：先翻譯樣板，再依序把 `{}` 換成參數。
//! - 語言由設定檔 `language` 決定；`Auto` 時依 Windows 的「顯示語言」判斷（中文→繁中、日文→日本語、其他→English）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

mod en;
mod ja;

/// 使用者在設定裡選的語言
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    /// 依系統顯示語言
    #[default]
    Auto,
    ZhTw,
    En,
    Ja,
}

/// 實際使用中的語言（已解析 Auto）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhTw,
    En,
    Ja,
}

static CURRENT: AtomicU8 = AtomicU8::new(0); // 0=ZhTw 1=En 2=Ja

#[cfg(windows)]
unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

/// 由 Windows 語言 ID（LANGID）推得語言：主要語言 0x04＝中文、0x11＝日文，其餘一律英文
pub fn lang_from_langid(langid: u16) -> Lang {
    match langid & 0x3FF {
        0x04 => Lang::ZhTw,
        0x11 => Lang::Ja,
        _ => Lang::En,
    }
}

/// 偵測系統顯示語言（非 Windows 平台一律回傳英文）
pub fn detect_system() -> Lang {
    #[cfg(windows)]
    {
        // SAFETY：GetUserDefaultUILanguage 無參數、無副作用，純查詢
        return lang_from_langid(unsafe { GetUserDefaultUILanguage() });
    }
    #[cfg(not(windows))]
    Lang::En
}

/// 把設定值解析成實際語言
pub fn resolve(lang: Language) -> Lang {
    match lang {
        Language::Auto => detect_system(),
        Language::ZhTw => Lang::ZhTw,
        Language::En => Lang::En,
        Language::Ja => Lang::Ja,
    }
}

/// 設定目前語言（程式啟動與使用者切換語言時呼叫）
pub fn set_language(lang: Language) {
    let l = match resolve(lang) {
        Lang::ZhTw => 0,
        Lang::En => 1,
        Lang::Ja => 2,
    };
    CURRENT.store(l, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Ja,
        _ => Lang::ZhTw,
    }
}

fn table(lang: Lang) -> Option<&'static HashMap<&'static str, &'static str>> {
    static EN: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static JA: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    match lang {
        Lang::ZhTw => None,
        Lang::En => Some(EN.get_or_init(|| en::TABLE.iter().copied().collect())),
        Lang::Ja => Some(JA.get_or_init(|| ja::TABLE.iter().copied().collect())),
    }
}

/// 翻譯：傳入繁體中文原文，回傳目前語言的文字；查不到就回傳原文
pub fn tr(zh: &'static str) -> &'static str {
    match table(current()) {
        Some(t) => t.get(zh).copied().unwrap_or(zh),
        None => zh,
    }
}

/// 翻譯含變數的句子：樣板中的 `{}` 會依序換成 `args`
pub fn tr_fmt(zh: &'static str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = tr(zh);
    for a in args {
        match rest.find("{}") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(a);
                rest = &rest[i + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// 對照表（供測試與工具檢查完整性）
pub fn tables() -> [(&'static str, &'static [(&'static str, &'static str)]); 2] {
    [("en", en::TABLE), ("ja", ja::TABLE)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langid_mapping() {
        assert_eq!(lang_from_langid(0x0404), Lang::ZhTw); // zh-TW
        assert_eq!(lang_from_langid(0x0804), Lang::ZhTw); // zh-CN 也歸中文
        assert_eq!(lang_from_langid(0x0411), Lang::Ja);
        assert_eq!(lang_from_langid(0x0409), Lang::En);
        assert_eq!(lang_from_langid(0x040C), Lang::En); // 法文 → 英文
    }

    #[test]
    fn placeholders_and_fallback() {
        set_language(Language::En);
        assert_eq!(tr("這句話不在表裡"), "這句話不在表裡"); // 查不到回傳原文
        assert_eq!(tr_fmt("A{}B{}C", &["1", "2"]), "A1B2C");
        set_language(Language::ZhTw);
        assert_eq!(tr("設定…"), "設定…");
    }

    #[test]
    fn tables_have_no_duplicate_keys_and_matching_placeholders() {
        for (name, t) in tables() {
            let mut seen = std::collections::HashSet::new();
            for (k, v) in t.iter() {
                assert!(seen.insert(*k), "{name} 表有重複鍵：{k}");
                assert!(!v.trim().is_empty(), "{name} 表的翻譯是空的：{k}");
                assert_eq!(k.matches("{}").count(), v.matches("{}").count(), "{name} 表的 {{}} 數量與原文不同：{k}");
            }
        }
    }
}
