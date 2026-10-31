//! Downloads a pinned archive over HTTPS (redirects must stay on HTTPS) into a file, with a size cap, hashing it while
//! it is written. The caller compares the hash with the pin before anything is read from the file.

use crate::theme::engine::sha256::{self, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// The SHA-256 (hex) of `r` copied into `out`, at most `cap` bytes.
pub fn copy_hashed(mut r: impl Read, out: &Path, cap: u64) -> Result<(String, u64), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut h = Sha256::default();
    let mut n = 0u64;
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let k = r.read(&mut buf).map_err(|e| format!("download: {e}"))?;
        if k == 0 {
            break;
        }
        n += k as u64;
        if n > cap {
            return Err(format!("the archive is larger than {} MiB", cap >> 20));
        }
        h.update(&buf[..k]);
        f.write_all(&buf[..k]).map_err(|e| e.to_string())?;
    }
    f.sync_all().map_err(|e| e.to_string())?;
    Ok((sha256::hex(&h.finish()), n))
}

/// The SHA-256 (hex) of a file, at most `cap` bytes.
pub fn hash_file(p: &Path, cap: u64) -> Result<String, String> {
    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !m.is_file() {
        return Err(format!("{} is not a file", p.display()));
    }
    if m.len() > cap {
        return Err(format!("{} is larger than {} MiB", p.display(), cap >> 20));
    }
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::default();
    let mut buf = vec![0u8; 64 << 10];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf).map_err(|e| e.to_string())?;
        if k == 0 {
            return Ok(sha256::hex(&h.finish()));
        }
        // the file may grow while it is read: the cap holds for the bytes actually hashed
        n += k as u64;
        if n > cap {
            return Err(format!("{} is larger than {} MiB", p.display(), cap >> 20));
        }
        h.update(&buf[..k]);
    }
}

/// GETs `url` (https only) into `out`; returns the SHA-256 and the size.
pub fn download(url: &str, out: &Path, cap: u64) -> Result<(String, u64), String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing to download `{url}`: only https:// URLs are fetched"));
    }
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .redirects(5)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("decebal/", env!("CARGO_PKG_VERSION"), " asset-packs"))
        .build();
    let resp = agent.get(url).call().map_err(|e| format!("download {url}: {e}"))?;
    if let Some(len) = resp.header("Content-Length").and_then(|l| l.parse::<u64>().ok()) {
        if len > cap {
            return Err(format!("the archive is {} MiB, the limit is {} MiB", len >> 20, cap >> 20));
        }
    }
    copy_hashed(resp.into_reader(), out, cap)
}

/// git's smart HTTP over the built-in client (`gitpack::Transport`): https only, **no redirects** (a moved repository
/// is reported with where it went, never followed), no credentials, a deadline for each whole request.
pub struct GitHttps;

impl GitHttps {
    fn agent(deadline: Duration) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .https_only(true)
            .redirects(0)
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .timeout(deadline)
            .user_agent(concat!("git/2.0 (decebal/", env!("CARGO_PKG_VERSION"), " asset-packs)"))
            .build()
    }

    fn check(url: &str, r: Result<ureq::Response, ureq::Error>, ctype: &str) -> Result<ureq::Response, String> {
        let resp = match r {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => return Err(format!("{url}: HTTP {code} {}", r.status_text())),
            Err(e) => return Err(format!("{url}: {e}")),
        };
        if resp.status() != 200 {
            let to = resp.header("Location").map(|l| format!(" to {}", l.chars().take(200).collect::<String>())).unwrap_or_default();
            return Err(format!("{url}: HTTP {} (moved{to}? git_url must name the repository itself)", resp.status()));
        }
        if resp.content_type() != ctype {
            return Err(format!("{url}: the reply is `{}`, not `{ctype}` (not a smart git server)", resp.content_type()));
        }
        Ok(resp)
    }
}

impl super::gitpack::Transport for GitHttps {
    fn get(&self, url: &str, cap: u64) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing `{url}`: only https:// is fetched"));
        }
        let r = Self::agent(Duration::from_secs(60)).get(url).set("Git-Protocol", "version=2").call();
        let resp = Self::check(url, r, "application/x-git-upload-pack-advertisement")?;
        let mut v = Vec::new();
        resp.into_reader().take(cap + 1).read_to_end(&mut v).map_err(|e| format!("{url}: {e}"))?;
        if v.len() as u64 > cap {
            return Err(format!("{url}: the reply is larger than {} KiB", cap >> 10));
        }
        Ok(v)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Box<dyn Read>, String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing `{url}`: only https:// is fetched"));
        }
        let r = Self::agent(Duration::from_secs(15 * 60))
            .post(url)
            .set("Git-Protocol", "version=2")
            .set("Content-Type", "application/x-git-upload-pack-request")
            .set("Accept", "application/x-git-upload-pack-result")
            .send_bytes(body);
        Ok(Box::new(Self::check(url, r, "application/x-git-upload-pack-result")?.into_reader()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::Scratch;
    use super::*;

    #[test]
    fn only_https_is_fetched_and_copies_are_capped_and_hashed() {
        let s = Scratch::new("fetch");
        assert!(download("http://example.com/a.tar.gz", &s.path().join("a"), 1 << 20).unwrap_err().contains("https"));
        assert!(download("file:///etc/passwd", &s.path().join("b"), 1 << 20).unwrap_err().contains("https"));
        let (h, n) = copy_hashed(&b"abc"[..], &s.path().join("c"), 10).unwrap();
        assert_eq!((h.as_str(), n), ("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", 3));
        assert_eq!(hash_file(&s.path().join("c"), 10).unwrap(), h);
        assert!(copy_hashed(&[0u8; 100][..], &s.path().join("d"), 10).unwrap_err().contains("larger"));
        assert!(copy_hashed(&b"x"[..], &s.path().join("c"), 10).is_err(), "never overwrites");
    }
}
