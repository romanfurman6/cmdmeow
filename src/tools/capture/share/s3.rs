//! S3-compatible storage (Cloudflare R2, Hetzner, AWS S3, MinIO…): request
//! signing (AWS Signature V4), uploading a file and the link to it.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Cloudflare R2's S3 endpoints end with this.
pub const R2_HOST: &str = ".r2.cloudflarestorage.com";

/// Where files go.
#[derive(Clone, Debug, PartialEq)]
pub struct Bucket {
    /// "fsn1.your-objectstorage.com" (no scheme, no bucket).
    pub host: String,
    /// "fsn1"; signing needs it.
    pub region: String,
    pub name: String,
    pub access_key: String,
    pub secret: String,
    /// Where public files are read from, when that isn't the bucket's own
    /// host ("https://pub-….r2.dev", a custom domain). Empty: the bucket.
    pub public_url: String,
}

impl Bucket {
    /// From what a person types: the endpoint may come with or without
    /// https:// and a slash; the region is the endpoint's first label
    /// unless given ("fsn1.your-objectstorage.com" → "fsn1"; AWS's
    /// "s3.eu-central-1.amazonaws.com" → "eu-central-1").
    pub fn new(endpoint: &str, region: &str, name: &str, access_key: &str, secret: &str) -> Bucket {
        let name = name.trim().trim_matches('/');
        // "https://fsn1.your-objectstorage.com/", a pasted bucket URL
        // ("https://clips.fsn1…/", "https://fsn1…/clips") or a bare host
        let host: String = endpoint.trim().trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("").to_lowercase();
        let host = match host.strip_prefix(&format!("{}.", name.to_lowercase())) {
            Some(rest) if !name.is_empty() && rest.contains('.') => rest.to_string(),
            _ => host,
        };
        let region = if region.trim().is_empty() && host.ends_with(R2_HOST) {
            "auto".to_string()
        } else if region.trim().is_empty() {
            let labels: Vec<&str> = host.split('.').collect();
            match labels.as_slice() {
                ["s3", r, ..] if host.ends_with("amazonaws.com") => r.to_string(),
                [first, ..] => first.to_string(),
                _ => "us-east-1".into(),
            }
        } else {
            region.trim().to_string()
        };
        Bucket { host, region, name: name.to_string(), access_key: access_key.trim().to_string(), secret: secret.trim().to_string(), public_url: String::new() }
    }

    /// With public files read from `url` ("pub-….r2.dev" or a domain; the
    /// scheme is optional). Empty keeps the bucket's own host.
    pub fn public_at(mut self, url: &str) -> Bucket {
        let u = url.trim().trim_end_matches('/');
        self.public_url = if u.is_empty() || u.contains("://") { u.to_string() } else { format!("https://{u}") };
        self
    }

    /// Cloudflare R2: no ACLs (public files come from the bucket's public
    /// URL) and path-style requests.
    pub fn is_r2(&self) -> bool {
        self.host.ends_with(R2_HOST)
    }

    /// The bucket's own host ("bucket.fsn1.your-objectstorage.com"); R2
    /// keeps the bucket in the path instead.
    pub fn bucket_host(&self) -> String {
        if self.is_r2() {
            self.host.clone()
        } else {
            format!("{}.{}", self.name, self.host)
        }
    }

    /// The request path of `key` in the bucket.
    fn path(&self, key: &str) -> String {
        if self.is_r2() {
            format!("/{}/{}", self.name, encode(key, false))
        } else {
            format!("/{}", encode(key, false))
        }
    }

    /// The request path of the bucket itself (for `?cors`).
    fn root(&self) -> String {
        if self.is_r2() {
            format!("/{}", self.name)
        } else {
            "/".into()
        }
    }

    /// The plain link to `key` (works when the object is public).
    pub fn url(&self, key: &str) -> String {
        if self.public_url.is_empty() {
            format!("https://{}{}", self.bucket_host(), self.path(key))
        } else {
            format!("{}/{}", self.public_url, encode(key, false))
        }
    }

    /// Public links need a public URL on R2 (it has no per-file ACL).
    pub fn public_ready(&self) -> bool {
        !self.is_r2() || !self.public_url.is_empty()
    }

    pub fn ready(&self) -> bool {
        !self.host.is_empty() && !self.name.is_empty() && !self.access_key.is_empty() && !self.secret.is_empty()
    }
}

/// How an upload is going; `cancel` stops it.
#[derive(Default)]
pub struct Progress {
    pub sent: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 {
            0.0
        } else {
            (self.sent.load(Ordering::Relaxed) as f64 / t as f64).min(1.0) as f32
        }
    }
}

/// Put the file at `key` (replacing what was there), readable by anyone
/// with the link when `public`. Blocks.
pub fn put_file(b: &Bucket, key: &str, file: &Path, content_type: &str, public: bool, p: &Progress) -> Result<(), String> {
    let len = std::fs::metadata(file).map_err(|e| format!("Can't read the file: {e}"))?.len();
    p.total.store(len, Ordering::Relaxed);
    let hash = file_sha256(file, &p.cancel)?;
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let name = file.file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_default();
    let mut headers: Vec<(String, String)> = vec![
        ("content-disposition".into(), format!("inline; filename=\"{}\"", ascii_only(&name))),
        ("content-length".into(), len.to_string()),
        ("content-type".into(), content_type.into()),
        ("host".into(), b.bucket_host()),
        ("x-amz-content-sha256".into(), hash.clone()),
        ("x-amz-date".into(), amz_date.clone()),
    ];
    if public && !b.is_r2() {
        headers.push(("x-amz-acl".into(), "public-read".into()));
    }
    let path = b.path(key);
    let auth = authorization(b, "PUT", &path, "", &mut headers, &hash, &amz_date);
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).timeout_connect(Some(Duration::from_secs(20))).build());
    let mut req = agent.put(&format!("https://{}{}", b.bucket_host(), path));
    for (k, v) in &headers {
        if k != "host" {
            req = req.header(k, v);
        }
    }
    req = req.header("authorization", &auth);
    let f = std::fs::File::open(file).map_err(|e| format!("Can't read the file: {e}"))?;
    let mut body = Counting { inner: f, p };
    let resp = req.send(ureq::SendBody::from_reader(&mut body)).map_err(|e| {
        if p.cancel.load(Ordering::Relaxed) {
            "Stopped".to_string()
        } else if e.to_string().contains("lookup address") {
            format!("Upload failed: {} doesn't exist; check Endpoint and Bucket", b.bucket_host())
        } else {
            format!("Upload failed: {e}")
        }
    })?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let text = resp.into_body().read_to_string().unwrap_or_default();
    Err(explain(status, &text))
}

/// Remove `key` from the bucket. Blocks.
pub fn delete(b: &Bucket, key: &str) -> Result<(), String> {
    // SHA-256 of an empty body
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut headers: Vec<(String, String)> = vec![("host".into(), b.bucket_host()), ("x-amz-content-sha256".into(), EMPTY.into()), ("x-amz-date".into(), amz_date.clone())];
    let path = b.path(key);
    let auth = authorization(b, "DELETE", &path, "", &mut headers, EMPTY, &amz_date);
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).timeout_connect(Some(Duration::from_secs(20))).build());
    let mut req = agent.delete(&format!("https://{}{}", b.bucket_host(), path));
    for (k, v) in &headers {
        if k != "host" {
            req = req.header(k, v);
        }
    }
    let resp = req.header("authorization", &auth).call().map_err(|e| format!("Couldn't delete: {e}"))?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) || status == 404 {
        return Ok(());
    }
    let text = resp.into_body().read_to_string().unwrap_or_default();
    Err(explain(status, &text))
}

/// A link that works for `secs` even when the bucket is private (S3 allows
/// up to 7 days).
pub fn presign_get(b: &Bucket, key: &str, secs: u64, now: chrono::DateTime<chrono::Utc>) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = &amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", b.region);
    let mut q: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
        ("X-Amz-Credential".into(), format!("{}/{scope}", b.access_key)),
        ("X-Amz-Date".into(), amz_date.clone()),
        ("X-Amz-Expires".into(), secs.min(604_800).to_string()),
        ("X-Amz-SignedHeaders".into(), "host".into()),
    ];
    q.sort();
    let query: String = q.iter().map(|(k, v)| format!("{}={}", encode(k, true), encode(v, true))).collect::<Vec<_>>().join("&");
    let path = b.path(key);
    let canonical = format!("GET\n{path}\n{query}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD", b.bucket_host());
    let sig = signature(b, &amz_date, &canonical);
    format!("https://{}{path}?{query}&X-Amz-Signature={sig}", b.bucket_host())
}

/// Let the share page (`origins`, e.g. "https://cmdmeow.com") read files
/// in the bucket from the browser, so it can edit a copy. Rules the bucket
/// already has stay. Returns whether the bucket was changed. Blocks.
pub fn ensure_cors(b: &Bucket, origins: &[&str]) -> Result<bool, String> {
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(20))).build());
    let empty = hex(&Sha256::digest(b""));
    // Read what is there now.
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut headers: Vec<(String, String)> = vec![("host".into(), b.bucket_host()), ("x-amz-content-sha256".into(), empty.clone()), ("x-amz-date".into(), amz_date.clone())];
    let root = b.root();
    let auth = authorization(b, "GET", &root, "cors=", &mut headers, &empty, &amz_date);
    let mut req = agent.get(&format!("https://{}{root}?cors", b.bucket_host()));
    for (k, v) in headers.iter().filter(|(k, _)| k != "host") {
        req = req.header(k, v);
    }
    let resp = req.header("authorization", &auth).call().map_err(|e| format!("CORS check failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.into_body().read_to_string().unwrap_or_default();
    let existing = match status {
        200 => Some(text),
        404 => None, // NoSuchCORSConfiguration
        _ => return Err(explain(status, &text).replace("Upload failed", "CORS check failed")),
    };
    let Some(body) = cors_config(existing.as_deref(), origins) else { return Ok(false) };

    let hash = hex(&Sha256::digest(body.as_bytes()));
    let md5 = base64(&md5::Md5::digest(body.as_bytes()));
    let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut headers: Vec<(String, String)> = vec![
        ("content-md5".into(), md5),
        ("content-type".into(), "application/xml".into()),
        ("host".into(), b.bucket_host()),
        ("x-amz-content-sha256".into(), hash.clone()),
        ("x-amz-date".into(), amz_date.clone()),
    ];
    let auth = authorization(b, "PUT", &root, "cors=", &mut headers, &hash, &amz_date);
    let mut req = agent.put(&format!("https://{}{root}?cors", b.bucket_host()));
    for (k, v) in headers.iter().filter(|(k, _)| k != "host") {
        req = req.header(k, v);
    }
    let resp = req.header("authorization", &auth).send(body.as_bytes()).map_err(|e| format!("CORS setup failed: {e}"))?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(true);
    }
    let text = resp.into_body().read_to_string().unwrap_or_default();
    Err(explain(status, &text).replace("Upload failed", "CORS setup failed"))
}

/// The bucket's CORS document with one read rule added for the `origins`
/// it doesn't allow yet, or None when all of them are allowed.
fn cors_config(existing: Option<&str>, origins: &[&str]) -> Option<String> {
    let existing = existing.map(str::trim).filter(|x| x.contains("</CORSConfiguration>"));
    let allowed = |o: &str| existing.is_some_and(|x| x.contains(&format!("<AllowedOrigin>{o}</AllowedOrigin>")) || x.contains("<AllowedOrigin>*</AllowedOrigin>"));
    let missing: String = origins.iter().filter(|o| !allowed(o)).map(|o| format!("<AllowedOrigin>{o}</AllowedOrigin>")).collect();
    if missing.is_empty() {
        return None;
    }
    let rule = format!(
        "<CORSRule>{missing}<AllowedMethod>GET</AllowedMethod><AllowedMethod>HEAD</AllowedMethod><AllowedHeader>*</AllowedHeader><ExposeHeader>Content-Length</ExposeHeader><ExposeHeader>Content-Range</ExposeHeader><ExposeHeader>Content-Type</ExposeHeader><ExposeHeader>Accept-Ranges</ExposeHeader><MaxAgeSeconds>86400</MaxAgeSeconds></CORSRule>"
    );
    match existing {
        Some(x) => {
            let at = x.rfind("</CORSConfiguration>")?;
            Some(format!("{}{rule}{}", &x[..at], &x[at..]))
        }
        None => Some(format!("<CORSConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{rule}</CORSConfiguration>")),
    }
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// The Authorization header for a request whose headers (lowercase names)
/// are all signed.
fn authorization(b: &Bucket, method: &str, path: &str, query: &str, headers: &mut [(String, String)], payload_hash: &str, amz_date: &str) -> String {
    headers.sort();
    let canon_headers: String = headers.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed: String = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical = format!("{method}\n{path}\n{query}\n{canon_headers}\n{signed}\n{payload_hash}");
    let sig = signature(b, amz_date, &canonical);
    format!("AWS4-HMAC-SHA256 Credential={}/{}/{}/s3/aws4_request, SignedHeaders={signed}, Signature={sig}", b.access_key, &amz_date[..8], b.region)
}

fn signature(b: &Bucket, amz_date: &str, canonical: &str) -> String {
    let date = &amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", b.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex(&Sha256::digest(canonical.as_bytes())));
    let k = hmac(format!("AWS4{}", b.secret).as_bytes(), date.as_bytes());
    let k = hmac(&k, b.region.as_bytes());
    let k = hmac(&k, b"s3");
    let k = hmac(&k, b"aws4_request");
    hex(&hmac(&k, to_sign.as_bytes()))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// URI-encode as S3 wants: everything but A–Z a–z 0–9 - _ . ~ (and `/`
/// unless `slash`).
pub(super) fn encode(s: &str, slash: bool) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(c as char),
            b'/' if !slash => out.push('/'),
            _ => out.push_str(&format!("%{c:02X}")),
        }
    }
    out
}

fn ascii_only(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '_' }).collect()
}

fn file_sha256(file: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let mut f = std::fs::File::open(file).map_err(|e| format!("Can't read the file: {e}"))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        let n = f.read(&mut buf).map_err(|e| format!("Can't read the file: {e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// The file, counting what was sent; a cancel ends the body early.
struct Counting<'a, R> {
    inner: R,
    p: &'a Progress,
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.p.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped"));
        }
        let n = self.inner.read(buf)?;
        self.p.sent.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// S3's XML error, said plainly.
fn explain(status: u16, xml: &str) -> String {
    let tag = |t: &str| xml.split(&format!("<{t}>")).nth(1).and_then(|r| r.split(&format!("</{t}>")).next()).unwrap_or("").to_string();
    let code = tag("Code");
    let why = match code.as_str() {
        "InvalidAccessKeyId" => "the access key is wrong".to_string(),
        "SignatureDoesNotMatch" => "the secret key doesn't match the access key".to_string(),
        "NoSuchBucket" => "there is no such bucket".to_string(),
        "AccessDenied" => "these keys may not write to the bucket".to_string(),
        "RequestTimeTooSkewed" => "the Mac's clock is off".to_string(),
        "" => format!("the storage answered {status}"),
        c => {
            let m = tag("Message");
            if m.is_empty() {
                c.to_string()
            } else {
                format!("{c}: {m}")
            }
        }
    };
    format!("Upload failed: {why}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Bucket {
        Bucket { host: "s3.amazonaws.com".into(), region: "us-east-1".into(), name: "examplebucket".into(), access_key: "AKIAIOSFODNN7EXAMPLE".into(), secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(), public_url: String::new() }
    }

    #[test]
    fn presigned_link_matches_the_aws_example() {
        let now = chrono::DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let url = presign_get(&example(), "test.txt", 86_400, now);
        assert!(url.starts_with("https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request"), "{url}");
        assert!(url.ends_with("X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"), "{url}");
    }

    #[test]
    fn signed_put_matches_the_aws_example() {
        let mut h: Vec<(String, String)> = vec![
            ("host".into(), "examplebucket.s3.amazonaws.com".into()),
            ("date".into(), "Fri, 24 May 2013 00:00:00 GMT".into()),
            ("x-amz-date".into(), "20130524T000000Z".into()),
            ("x-amz-storage-class".into(), "REDUCED_REDUNDANCY".into()),
            ("x-amz-content-sha256".into(), "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072".into()),
        ];
        let auth = authorization(&example(), "PUT", &format!("/{}", encode("test$file.text", false)), "", &mut h, "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072", "20130524T000000Z");
        assert!(auth.ends_with("SignedHeaders=date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class, Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"), "{auth}");
    }

    #[test]
    fn endpoints_and_regions_from_what_people_type() {
        let b = Bucket::new("https://fsn1.your-objectstorage.com/", "", "clips", "k", "s");
        assert_eq!((b.host.as_str(), b.region.as_str()), ("fsn1.your-objectstorage.com", "fsn1"));
        assert_eq!(b.url("a b/c.mp4"), "https://clips.fsn1.your-objectstorage.com/a%20b/c.mp4");
        assert_eq!(Bucket::new("s3.eu-central-1.amazonaws.com", "", "x", "k", "s").region, "eu-central-1");
        assert_eq!(Bucket::new("nbg1.your-objectstorage.com", " nbg1 ", "x", "k", "s").region, "nbg1");
        // a pasted bucket URL, either style
        assert_eq!(Bucket::new("https://clips.fsn1.your-objectstorage.com/", "", "clips", "k", "s").host, "fsn1.your-objectstorage.com");
        assert_eq!(Bucket::new("https://fsn1.your-objectstorage.com/clips", "", " clips ", "k", "s").bucket_host(), "clips.fsn1.your-objectstorage.com");
    }

    #[test]
    fn r2_signs_path_style_with_region_auto_and_links_to_its_public_url() {
        let b = Bucket::new("https://0123abcd.r2.cloudflarestorage.com", "", "clips", "k", "s");
        assert!(b.is_r2());
        assert_eq!((b.region.as_str(), b.bucket_host().as_str()), ("auto", "0123abcd.r2.cloudflarestorage.com"));
        assert_eq!(b.path("a b/c.mp4"), "/clips/a%20b/c.mp4");
        assert_eq!(b.root(), "/clips");
        assert!(!b.public_ready());
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-14T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
        assert!(presign_get(&b, "x.png", 60, now).starts_with("https://0123abcd.r2.cloudflarestorage.com/clips/x.png?X-Amz-Algorithm"));
        let b = b.public_at("pub-9f.r2.dev/");
        assert!(b.public_ready());
        assert_eq!(b.url("a b/c.mp4"), "https://pub-9f.r2.dev/a%20b/c.mp4");
        // a region typed by hand still wins; other storages keep their own host
        assert_eq!(Bucket::new("x.r2.cloudflarestorage.com", "weur", "c", "k", "s").region, "weur");
        assert!(Bucket::new("fsn1.your-objectstorage.com", "", "c", "k", "s").public_ready());
    }

    #[test]
    fn errors_read_plainly() {
        assert_eq!(explain(403, "<Error><Code>SignatureDoesNotMatch</Code></Error>"), "Upload failed: the secret key doesn't match the access key");
        assert_eq!(explain(500, ""), "Upload failed: the storage answered 500");
    }

    #[test]
    fn cors_rule_is_added_once_and_keeps_other_rules() {
        let o = ["https://cmdmeow.com", "https://cmdmeow.furman.cat"];
        let new = cors_config(None, &o).unwrap();
        assert!(new.starts_with("<CORSConfiguration") && new.contains("<AllowedOrigin>https://cmdmeow.com</AllowedOrigin>") && new.contains("<AllowedOrigin>https://cmdmeow.furman.cat</AllowedOrigin>") && new.contains("<AllowedHeader>*</AllowedHeader>"), "{new}");
        assert_eq!(cors_config(Some(&new), &o), None);
        let other = "<?xml version=\"1.0\"?><CORSConfiguration><CORSRule><AllowedOrigin>https://a.b</AllowedOrigin><AllowedMethod>PUT</AllowedMethod></CORSRule></CORSConfiguration>";
        let merged = cors_config(Some(other), &o).unwrap();
        assert!(merged.contains("https://a.b") && merged.contains(o[0]) && merged.contains(o[1]) && merged.ends_with("</CORSRule></CORSConfiguration>"), "{merged}");
        assert_eq!(cors_config(Some("<CORSConfiguration><CORSRule><AllowedOrigin>*</AllowedOrigin></CORSRule></CORSConfiguration>"), &o), None);
    }

    #[test]
    fn cors_adds_only_the_new_site_to_a_bucket_set_up_before_the_move() {
        let before = cors_config(None, &["https://cmdmeow.furman.cat"]).unwrap();
        let after = cors_config(Some(&before), &["https://cmdmeow.com", "https://cmdmeow.furman.cat"]).unwrap();
        assert_eq!(after.matches("<AllowedOrigin>https://cmdmeow.furman.cat</AllowedOrigin>").count(), 1, "{after}");
        assert_eq!(after.matches("<AllowedOrigin>https://cmdmeow.com</AllowedOrigin>").count(), 1, "{after}");
        assert_eq!(after.matches("<CORSRule>").count(), 2, "{after}");
    }

    #[test]
    fn base64_and_md5_as_s3_wants() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&md5::Md5::digest(b"")), "1B2M2Y8AsgTpgAmY7PhCfg==");
    }
}
