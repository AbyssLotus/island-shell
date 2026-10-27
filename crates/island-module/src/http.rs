//! HTTP for module backends, through Windows' own HttpClient (no TLS stack of our own). Requests go
//! only to the hosts the module's manifest lists in `hosts`: discipline, not a sandbox, but it keeps
//! the privacy table honest. Modules with their own HttpClient code call [`check`] first.
//!
//! A `{settings.<key>}` host is the one the user entered, read from the module's settings.json at
//! the time of the request (so the module writes it there before calling it).

use std::path::PathBuf;
use std::sync::RwLock;

use island_module_proto::manifest::{host_allowed, resolve_hosts, settings_key};
use serde_json::Value;

static HOSTS: RwLock<Vec<String>> = RwLock::new(Vec::new());
static SETTINGS: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set from `island.init`.
pub(crate) fn set_hosts(hosts: Vec<String>, data_dir: PathBuf) {
    *HOSTS.write().unwrap_or_else(|e| e.into_inner()) = hosts;
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(data_dir.join("settings.json"));
}

/// Ok when `url` is https and its host is one the manifest declares.
pub fn check(url: &str) -> Result<(), String> {
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let settings = SETTINGS.read().unwrap_or_else(|e| e.into_inner()).clone();
    check_against(url, &hosts, settings.as_deref())
}

/// [`check`] against a given host list, with `{settings.<key>}` read from `settings` (a settings.json).
fn check_against(url: &str, hosts: &[String], settings: Option<&std::path::Path>) -> Result<(), String> {
    let rest = url.strip_prefix("https://").ok_or_else(|| format!("{url} isn't an https link"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("");
    let mut hosts = hosts.to_vec();
    if hosts.iter().any(|h| settings_key(h).is_some()) {
        let values = settings.and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        hosts = resolve_hosts(&hosts, &values);
    }
    if host_allowed(&hosts, host) {
        Ok(())
    } else {
        Err(format!("This module doesn't declare {host} in its manifest's hosts"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// (content type, text).
    pub body: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    /// The body as JSON, or `null` when it isn't.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

impl Request {
    pub fn get(url: &str) -> Self {
        Request { method: "GET".into(), url: url.into(), ..Default::default() }
    }
    pub fn post(url: &str, content_type: &str, body: impl Into<String>) -> Self {
        Request { method: "POST".into(), url: url.into(), body: Some((content_type.into(), body.into())), ..Default::default() }
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn send(&self) -> Result<Response, String> {
        check(&self.url)?;
        send(self)
    }
}

pub fn get(url: &str) -> Result<Response, String> {
    Request::get(url).send()
}

#[cfg(windows)]
fn send(r: &Request) -> Result<Response, String> {
    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Storage::Streams::{DataReader, UnicodeEncoding};
    use windows::Web::Http::Filters::{HttpBaseProtocolFilter, HttpCacheReadBehavior};
    use windows::Web::Http::{HttpClient, HttpMethod, HttpRequestMessage, HttpStringContent};

    // WinRT needs COM on whichever thread calls (a no-op when it's already set up).
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let e = |e: windows::core::Error| e.message().to_string();
    let filter = HttpBaseProtocolFilter::new().map_err(e)?;
    let _ = filter.CacheControl().and_then(|c| c.SetReadBehavior(HttpCacheReadBehavior::NoCache));
    let client = HttpClient::Create(&filter).map_err(e)?;
    let method = HttpMethod::Create(&HSTRING::from(r.method.as_str())).map_err(e)?;
    let uri = Uri::CreateUri(&HSTRING::from(r.url.as_str())).map_err(e)?;
    let req = HttpRequestMessage::Create(&method, &uri).map_err(e)?;
    let headers = req.Headers().map_err(e)?;
    let _ = headers.TryAppendWithoutValidation(&HSTRING::from("User-Agent"), &HSTRING::from(concat!("IslandShellModule/", env!("CARGO_PKG_VERSION"))));
    for (k, v) in &r.headers {
        headers.TryAppendWithoutValidation(&HSTRING::from(k.as_str()), &HSTRING::from(v.as_str())).map_err(e)?;
    }
    if let Some((ty, text)) = &r.body {
        let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(&HSTRING::from(text.as_str()), UnicodeEncoding::Utf8, &HSTRING::from(ty.as_str())).map_err(e)?;
        req.SetContent(&content).map_err(e)?;
    }
    let host = r.url.split('/').nth(2).unwrap_or("").to_string();
    let resp = client.SendRequestAsync(&req).and_then(|op| op.get()).map_err(|err| format!("Couldn't reach {host}: {}", err.message()))?;
    let status = resp.StatusCode().map_err(e)?.0 as u16;
    let buf = resp.Content().and_then(|c| c.ReadAsBufferAsync()).and_then(|op| op.get()).map_err(e)?;
    let n = buf.Length().map_err(e)? as usize;
    let mut body = vec![0u8; n];
    if n > 0 {
        DataReader::FromBuffer(&buf).and_then(|rd| rd.ReadBytes(&mut body)).map_err(e)?;
    }
    Ok(Response { status, body })
}

#[cfg(not(windows))]
fn send(_r: &Request) -> Result<Response, String> {
    Err("HTTP from modules needs Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_declared_hosts() {
        // Against its own list: tests run side by side, and others set the module's real one.
        let dir = std::env::temp_dir().join(format!("island-http-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let hosts = vec!["*.bungie.net".to_string(), "lrclib.net".to_string(), "{settings.domain}".to_string()];
        let check = |url: &str| check_against(url, &hosts, Some(&file));
        assert!(check("https://www.bungie.net/Platform/Destiny2/Manifest/").is_ok());
        assert!(check("https://lrclib.net/api/get?x=1").is_ok());
        assert!(check("https://user@lrclib.net:443/api").is_ok());
        assert!(check("http://lrclib.net/").is_err());
        assert!(check("https://evil.example/https://lrclib.net").is_err());
        assert!(check("https://lrclib.net.evil.example/").is_err());
        assert!(check("https://example.com/").unwrap_err().contains("doesn't declare"));
        // The user's own host, once it's in settings.json.
        assert!(check("https://canvas.school.edu/api/v1/users/self").is_err());
        std::fs::write(&file, r#"{ "domain": "canvas.school.edu" }"#).unwrap();
        assert!(check("https://canvas.school.edu/api/v1/users/self").is_ok());
        assert!(check("https://school.edu/").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
