//! Just enough property-list reading for app bundles (Info.plist) and launchd
//! jobs: the XML form is parsed here; a binary plist is first turned into XML
//! with macOS's own `plutil`.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Num(String),
    Array(Vec<Value>),
    Dict(BTreeMap<String, Value>),
    Other,
}

impl Value {
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.get(key),
            _ => None,
        }
    }
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.str())
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        }
    }
    /// Every string in the value, nested ones too.
    pub fn strings(&self) -> Vec<&str> {
        let mut out = Vec::new();
        fn walk<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
            match v {
                Value::Str(s) => out.push(s),
                Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                Value::Dict(d) => d.values().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        walk(self, &mut out);
        out
    }
}

/// Read a plist file, XML or binary.
pub fn read(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(b"bplist") {
        return parse(&to_xml(path)?);
    }
    parse(&String::from_utf8_lossy(&bytes))
}

#[cfg(target_os = "macos")]
fn to_xml(path: &Path) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn to_xml(_path: &Path) -> Option<String> {
    None
}

/// Parse an XML property list.
pub fn parse(xml: &str) -> Option<Value> {
    let mut p = Parser { s: xml, i: 0 };
    loop {
        let (name, _) = p.next_open()?;
        if name == "plist" {
            continue;
        }
        return p.value(&name);
    }
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl Parser<'_> {
    /// The next opening tag: (name, self-closing). Skips declarations,
    /// comments and closing tags.
    fn next_open(&mut self) -> Option<(String, bool)> {
        loop {
            let rest = &self.s[self.i..];
            let lt = rest.find('<')?;
            self.i += lt;
            let rest = &self.s[self.i..];
            if rest.starts_with("<!--") {
                self.i += rest.find("-->")? + 3;
                continue;
            }
            if rest.starts_with("<?") || rest.starts_with("<!") || rest.starts_with("</") {
                self.i += rest.find('>')? + 1;
                continue;
            }
            let gt = rest.find('>')?;
            let tag = &rest[1..gt];
            self.i += gt + 1;
            let closed = tag.ends_with('/');
            let name = tag
                .trim_end_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            return Some((name, closed));
        }
    }

    /// Text up to `</name>`, consuming the closing tag.
    fn text(&mut self, name: &str) -> Option<String> {
        let end = format!("</{name}>");
        let rest = &self.s[self.i..];
        let k = rest.find(&end)?;
        let t = unescape(&rest[..k]);
        self.i += k + end.len();
        Some(t)
    }

    /// Whether `</name>` comes before the next opening tag.
    fn at_close(&self, name: &str) -> bool {
        let rest = &self.s[self.i..];
        let close = format!("</{name}>");
        let mut j = 0;
        while let Some(k) = rest[j..].find('<') {
            let at = &rest[j + k..];
            if at.starts_with("<!--") {
                match at.find("-->") {
                    Some(e) => j += k + e + 3,
                    None => return true,
                }
                continue;
            }
            return at.starts_with(&close);
        }
        true
    }

    fn skip_close(&mut self, name: &str) {
        let close = format!("</{name}>");
        if let Some(k) = self.s[self.i..].find(&close) {
            self.i += k + close.len();
        }
    }

    fn value(&mut self, name: &str) -> Option<Value> {
        self.value_closed(name, false)
    }

    fn value_closed(&mut self, name: &str, closed: bool) -> Option<Value> {
        Some(match name {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "string" if closed => Value::Str(String::new()),
            "string" => Value::Str(self.text("string")?),
            "integer" | "real" if !closed => Value::Num(self.text(name)?.trim().to_string()),
            "dict" if closed => Value::Dict(BTreeMap::new()),
            "dict" => {
                let mut d = BTreeMap::new();
                while !self.at_close("dict") {
                    let (k, kc) = self.next_open()?;
                    if k != "key" || kc {
                        return None;
                    }
                    let key = self.text("key")?;
                    let (vn, vc) = self.next_open()?;
                    d.insert(key, self.value_closed(&vn, vc)?);
                }
                self.skip_close("dict");
                Value::Dict(d)
            }
            "array" if closed => Value::Array(Vec::new()),
            "array" => {
                let mut a = Vec::new();
                while !self.at_close("array") {
                    let (vn, vc) = self.next_open()?;
                    a.push(self.value_closed(&vn, vc)?);
                }
                self.skip_close("array");
                Value::Array(a)
            }
            _ if closed => Value::Other,
            other => {
                self.text(other)?;
                Value::Other
            }
        })
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<!-- a comment -->
	<key>CFBundleIdentifier</key>
	<string>com.docker.docker</string>
	<key>CFBundleName</key>
	<string>Docker &amp; Co</string>
	<key>CFBundleShortVersionString</key>
	<string>4.36.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>Empty</key>
	<string/>
	<key>Nested</key>
	<dict>
		<key>List</key>
		<array>
			<string>a</string>
			<integer>3</integer>
			<dict/>
		</array>
	</dict>
	<key>Blob</key>
	<data>AAEC</data>
</dict>
</plist>"#;

    #[test]
    fn reads_info_plist_keys() {
        let v = parse(INFO).unwrap();
        assert_eq!(v.get_str("CFBundleIdentifier"), Some("com.docker.docker"));
        assert_eq!(v.get_str("CFBundleName"), Some("Docker & Co"));
        assert_eq!(v.get_str("CFBundleShortVersionString"), Some("4.36.0"));
        assert_eq!(v.get_bool("LSUIElement"), Some(true));
        assert_eq!(v.get_str("Empty"), Some(""));
        assert_eq!(v.get("Blob"), Some(&Value::Other));
        let list = v.get("Nested").and_then(|n| n.get("List")).unwrap();
        assert_eq!(
            list,
            &Value::Array(vec![
                Value::Str("a".into()),
                Value::Num("3".into()),
                Value::Dict(BTreeMap::new())
            ])
        );
        assert!(v.strings().contains(&"a"));
    }

    #[test]
    fn launchd_job() {
        let v = parse("<plist><dict><key>Label</key><string>com.x.agent</string><key>ProgramArguments</key><array><string>/Applications/X.app/Contents/MacOS/agent</string><string>--run</string></array></dict></plist>").unwrap();
        assert_eq!(v.get_str("Label"), Some("com.x.agent"));
        assert!(v
            .strings()
            .iter()
            .any(|s| s.starts_with("/Applications/X.app/")));
    }

    #[test]
    fn garbage_is_none() {
        assert!(parse("not a plist").is_none());
        assert!(parse("<plist><dict><key>A</key>").is_none());
    }
}
