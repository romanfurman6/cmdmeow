//! Texts on screen come from `locales/<lang>.json` through rust-i18n
//! (set up in main.rs). English (`en.json`) is the source and the fallback.
//!
//! - `ts!("fan.mode.auto")` → `&'static str`, for labels, titles and the
//!   places that keep a `&'static str`
//! - `t!("fan.rpm_value", rpm = x)` → `Cow<str>`, with `%{rpm}` in the text
//!   filled in; `c = v : {:.0}` formats the value first
//! - `tn!("startup.items", n = k)` → `Cow<str>` from `startup.items_one`
//!   when k is 1, else `startup.items_other`; both texts get `%{n}`
//!
//! Keys are string literals, so the tests below can check them (the one
//! exception, [`data`], has its own test next to its data):
//! every key used exists in en.json, every en.json key is used, the
//! arguments match the text's `%{…}`, and other locales only hold English
//! keys with the same `%{…}`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Mutex;

/// A translated text as `&'static str`.
#[macro_export]
macro_rules! ts {
    ($key:literal) => {
        $crate::i18n::st(t!($key))
    };
}

/// A count: `key_one` for 1, `key_other` otherwise (English). Languages
/// with more forms get their own keys when they arrive.
#[macro_export]
macro_rules! tn {
    ($key:literal, n = $n:expr $(, $k:ident = $v:expr)* $(,)?) => {{
        let n = $n;
        if n == 1 {
            t!(concat!($key, "_one"), n = n $(, $k = $v)*)
        } else {
            t!(concat!($key, "_other"), n = n $(, $k = $v)*)
        }
    }};
}

/// A text whose key comes from data (Cleanup's rules.json), not code:
/// keys under `rules.` only, each checked by that data's own test.
pub fn data(key: &str) -> &'static str {
    debug_assert!(key.starts_with("rules."));
    match crate::_rust_i18n_try_translate(&rust_i18n::locale(), key) {
        Some(c) => st(c),
        None => st(Cow::Owned(key.to_string())),
    }
}

/// `t!` hands back texts borrowed from its static tables; an owned one
/// (only a missing key, which the tests rule out) is kept once.
pub fn st(c: Cow<'static, str>) -> &'static str {
    match c {
        Cow::Borrowed(s) => s,
        Cow::Owned(s) => {
            static KEPT: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
            let mut g = KEPT.lock().unwrap_or_else(|e| e.into_inner());
            let set = g.get_or_insert_with(HashSet::new);
            if let Some(k) = set.get(s.as_str()) {
                return k;
            }
            let k: &'static str = Box::leak(s.into_boxed_str());
            set.insert(k);
            k
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
    }

    /// "a.b.c" → text, from a nested locale file.
    fn flat(v: &serde_json::Value, path: &mut Vec<String>, out: &mut BTreeMap<String, String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    path.push(k.clone());
                    flat(x, path, out);
                    path.pop();
                }
            }
            serde_json::Value::String(s) => {
                out.insert(path.join("."), s.clone());
            }
            _ => panic!("{}: texts are strings", path.join(".")),
        }
    }

    fn locale(name: &str) -> BTreeMap<String, String> {
        let raw = std::fs::read_to_string(root().join("locales").join(name)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut out = BTreeMap::new();
        flat(&v, &mut Vec::new(), &mut out);
        out
    }

    /// `%{name}` names in a text.
    fn holes(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find("%{") {
            let end = rest[i..].find('}').map(|e| i + e).unwrap_or_else(|| panic!("unclosed %{{ in {s:?}"));
            out.insert(rest[i + 2..end].to_string());
            rest = &rest[end + 1..];
        }
        out
    }

    struct Call {
        file: String,
        key: Option<String>,
        args: BTreeSet<String>,
        /// a `tn!` text: it may leave `%{n}` out ("every hour")
        count: bool,
    }

    fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                sources(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("src/i18n.rs") {
                out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
    }

    /// The text between the parens that open at `start`, skipping string
    /// and char literals.
    fn group(s: &str, start: usize) -> &str {
        let b = s.as_bytes();
        let (mut i, mut depth) = (start, 0);
        while i < b.len() {
            match b[i] {
                b'"' => {
                    i += 1;
                    while b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'\'' if b.get(i + 2) == Some(&b'\'') || b.get(i + 1) == Some(&b'\\') => {
                    i += 1;
                    while b[i] != b'\'' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &s[start + 1..i];
                    }
                }
                _ => {}
            }
            i += 1;
        }
        panic!("unbalanced call")
    }

    /// Top-level comma-separated parts.
    fn parts(s: &str) -> Vec<&str> {
        let b = s.as_bytes();
        let (mut out, mut from, mut depth, mut i) = (Vec::new(), 0, 0, 0);
        while i < b.len() {
            match b[i] {
                b'"' => {
                    i += 1;
                    while b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(s[from..i].trim());
                    from = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        if !s[from..].trim().is_empty() {
            out.push(s[from..].trim());
        }
        out
    }

    fn calls() -> Vec<Call> {
        let mut files = Vec::new();
        sources(&root().join("src"), &mut files);
        let mut out = Vec::new();
        for (file, src) in &files {
            for mac in ["t!(", "ts!(", "tn!("] {
                let mut from = 0;
                while let Some(i) = src[from..].find(mac).map(|i| i + from) {
                    from = i + mac.len();
                    // `tkv!(`, `assert!(` … aren't ours
                    if src[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let open = i + mac.len() - 1;
                    let ps = parts(group(src, open));
                    let key = ps.first().and_then(|k| k.strip_prefix('"')).and_then(|k| k.strip_suffix('"')).map(str::to_string);
                    let args: BTreeSet<String> = ps.iter().skip(1).map(|a| a.split('=').next().unwrap().trim().to_string()).collect();
                    if mac == "tn!(" {
                        // one call, two texts
                        for form in ["_one", "_other"] {
                            out.push(Call { file: file.clone(), key: key.as_ref().map(|k| format!("{k}{form}")), args: args.clone(), count: true });
                        }
                    } else {
                        out.push(Call { file: file.clone(), key, args, count: false });
                    }
                }
            }
        }
        out
    }

    #[test]
    fn keys_match_en_json() {
        let en = locale("en.json");
        let calls = calls();
        let mut errors = Vec::new();
        for c in &calls {
            let Some(key) = &c.key else {
                errors.push(format!("{}: t!/ts! takes a string literal key", c.file));
                continue;
            };
            match en.get(key) {
                None => errors.push(format!("{}: `{key}` isn't in locales/en.json", c.file)),
                Some(text) if c.count && !(holes(text).is_subset(&c.args) && c.args.iter().all(|a| a == "n" || holes(text).contains(a))) => errors.push(format!("{}: `{key}` gets {:?}, the text uses {:?}", c.file, c.args, holes(text))),
                Some(text) if !c.count && holes(text) != c.args => errors.push(format!("{}: `{key}` gets {:?}, the text uses {:?}", c.file, c.args, holes(text))),
                _ => {}
            }
        }
        let used: BTreeSet<&String> = calls.iter().filter_map(|c| c.key.as_ref()).collect();
        // `rules.*` texts are named by Cleanup's rules.json (see its test)
        for k in en.keys().filter(|k| !used.contains(k) && !k.starts_with("rules.")) {
            errors.push(format!("locales/en.json: `{k}` isn't used"));
        }
        assert!(errors.is_empty(), "\n{}", errors.join("\n"));
    }

    #[test]
    fn locales_follow_english() {
        let en = locale("en.json");
        let mut errors = Vec::new();
        for e in std::fs::read_dir(root().join("locales")).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "en.json" || !name.ends_with(".json") {
                continue;
            }
            for (k, v) in locale(&name) {
                match en.get(&k) {
                    None => errors.push(format!("{name}: `{k}` isn't in en.json")),
                    Some(text) if holes(text) != holes(&v) => errors.push(format!("{name}: `{k}` uses {:?}, en.json uses {:?}", holes(&v), holes(text))),
                    _ => {}
                }
            }
        }
        assert!(errors.is_empty(), "\n{}", errors.join("\n"));
    }

    #[test]
    fn static_texts_stay_borrowed() {
        let a = ts!("fan.name");
        assert_eq!(a, "Fan Control");
        assert!(std::ptr::eq(a, ts!("fan.name")));
        assert_eq!(t!("unit.celsius", c = 71.6 : {:.0}), "72 °C");
        assert_eq!(tn!("startup.items", n = 1), "1 item");
        assert_eq!(tn!("startup.items", n = 3usize), "3 items");
    }
}
