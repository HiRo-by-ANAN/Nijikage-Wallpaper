//! 多語系覆蓋率檢查：掃描各 crate 的原始碼，找出所有 `tr("…")`／`tr_fmt("…", …)` 的字面值，
//! 確認英文與日文對照表都有對應的翻譯；反過來也列出表中沒被使用的鍵（只警告，不失敗）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "target" || n == "bin") {
                    continue;
                }
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
}

/// 解析從 `start`（指向開頭的雙引號之後）開始的一個 Rust 字串字面值，回傳（內容，結尾後位置）
fn parse_literal(s: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut buf: Vec<u8> = Vec::new();
    while i < s.len() {
        match s[i] {
            b'"' => return Some((String::from_utf8_lossy(&buf).into_owned(), i + 1)),
            b'\\' => {
                i += 1;
                match *s.get(i)? {
                    b'n' => buf.push(b'\n'),
                    b't' => buf.push(b'\t'),
                    b'r' => buf.push(b'\r'),
                    b'"' => buf.push(b'"'),
                    b'\'' => buf.push(b'\''),
                    b'\\' => buf.push(b'\\'),
                    b'\n' | b'\r' => {
                        // 行尾反斜線：略過換行與下一行開頭的空白
                        while i + 1 < s.len() && matches!(s[i + 1], b' ' | b'\t' | b'\n' | b'\r') {
                            i += 1;
                        }
                    }
                    other => {
                        buf.push(b'\\');
                        buf.push(other);
                    }
                }
                i += 1;
            }
            c => {
                buf.push(c);
                i += 1;
            }
        }
    }
    None
}

fn used_keys() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for krate in ["nijikage-wallpaper-core", "nijikage-wallpaper-settings", "nijikage-wallpaper-host"] {
        rust_files(&root.join(krate).join("src"), &mut files);
    }
    let mut keys = BTreeSet::new();
    for f in files {
        let text = std::fs::read(&f).unwrap();
        // i18n.rs 本身的說明文件與單元測試用範例字串，不屬於介面文字
        if f.file_name().is_some_and(|n| n == "i18n.rs") {
            continue;
        }
        for pat in [&b"tr("[..], &b"tr_fmt("[..]] {
            let mut from = 0;
            while let Some(pos) = text[from..].windows(pat.len()).position(|w| w == pat) {
                let at = from + pos;
                // 前一個字元必須不是識別字的一部分（避免把 `str(`、`attr(` 之類誤判）
                let prev_ok = at == 0 || !(text[at - 1].is_ascii_alphanumeric() || text[at - 1] == b'_');
                // 括號後允許空白與換行，再接雙引號
                let mut q = at + pat.len();
                while q < text.len() && text[q].is_ascii_whitespace() {
                    q += 1;
                }
                if prev_ok && q < text.len() && text[q] == b'"' {
                    if let Some((lit, end)) = parse_literal(&text, q + 1) {
                        // 只收含有非 ASCII 字元（也就是中文原文）的鍵，純 ASCII 的多半是測試或範例
                        if !lit.is_ascii() {
                            keys.insert(lit);
                        }
                        from = end;
                        continue;
                    }
                }
                from = at + pat.len();
            }
        }
    }
    keys
}

#[test]
fn every_translatable_string_has_english_and_japanese() {
    let keys = used_keys();
    assert!(keys.len() > 100, "掃描到的字串太少（{}），掃描邏輯可能壞了", keys.len());
    let mut problems = Vec::new();
    for (name, table) in nijikage_wallpaper_core::i18n::tables() {
        let have: BTreeSet<&str> = table.iter().map(|(k, _)| *k).collect();
        for k in &keys {
            if !have.contains(k.as_str()) {
                problems.push(format!("[{name}] 缺少翻譯：{k}"));
            }
        }
    }
    assert!(problems.is_empty(), "共 {} 筆缺少翻譯：\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn unused_table_entries_are_reported() {
    let keys = used_keys();
    let mut unused = Vec::new();
    for (name, table) in nijikage_wallpaper_core::i18n::tables() {
        for (k, _) in table.iter() {
            if !keys.contains(*k) {
                unused.push(format!("[{name}] {k}"));
            }
        }
    }
    // 只列出來供人檢視（可能是以變數或巨集組出的鍵）；不讓測試失敗
    if !unused.is_empty() {
        eprintln!("表中有 {} 筆未在原始碼找到對應 tr(\"…\")：\n{}", unused.len(), unused.join("\n"));
    }
}

/// 找出設定視窗原始碼裡「含中文字元、卻沒有被 tr()／tr_fmt() 包住」的字串字面值（不含註解）。
/// 語言選單中各語言的自稱（繁體中文、English、日本語）是刻意不翻譯的，列在白名單。
#[test]
fn settings_ui_has_no_untranslated_chinese_literals() {
    let allow = ["繁體中文", "日本語"];
    let keys = used_keys();
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("nijikage-wallpaper-settings").join("src"), &mut files);
    let mut leftovers = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (ln, line) in text.lines().enumerate() {
            let code = match line.find("//") {
                // 簡易判斷：`//` 之前出現偶數個雙引號才算註解開頭（不在字串內）
                Some(i) if line[..i].matches('"').count() % 2 == 0 => &line[..i],
                _ => line,
            };
            let bytes = code.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if let Some((lit, end)) = parse_literal(bytes, i + 1) {
                        let has_cjk = lit.chars().any(|c| ('\u{3040}'..='\u{9FFF}').contains(&c));
                        if has_cjk && !keys.contains(&lit) && !allow.contains(&lit.as_str()) {
                            leftovers.push(format!("{}:{}  \"{}\"", f.file_name().unwrap().to_string_lossy(), ln + 1, lit));
                        }
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }
    assert!(leftovers.is_empty(), "設定視窗有 {} 處中文字串沒有包 tr()：\n{}", leftovers.len(), leftovers.join("\n"));
}
