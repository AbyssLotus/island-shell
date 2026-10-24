//! Discord, through the Discord desktop app's local RPC pipe (`\\.\pipe\discord-ipc-N`).
//!
//! - Voice: the call you're in, who's in it and who's talking; mute, deafen, leave.
//! - Notifications: Discord's message notifications, shown by the island.
//! - Rich Presence: "Listening to Apple Music" with the song, artist, album art and a time bar.
//!
//! Discord only lets an app its owner made and authorized read voice state or notifications, so
//! this runs as the user's own Discord application (Client ID + Secret from the developer portal).
//! The first time, Discord shows its own "authorize" prompt; after that the island signs in by
//! itself. The secret and tokens are stored encrypted with Windows' DPAPI (only this Windows
//! account can read them). Presence needs only the Client ID, no authorization.
//!
//! One thread owns the pipe. Windows serializes reads and writes on a synchronous pipe handle,
//! so instead of a blocking reader on its own thread, the loop peeks for data and writes in turn.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Storage::Streams::UnicodeEncoding;
use windows::Web::Http::{HttpClient, HttpStringContent};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Pipes::PeekNamedPipe;

use crate::util::{log_line, protect, unprotect};
use crate::Shared;

const SCOPES: [&str; 5] = ["rpc", "rpc.voice.read", "rpc.voice.write", "rpc.notifications.read", "identify"];
/// Must be listed under OAuth2 → Redirects in the user's Discord application.
pub const REDIRECT: &str = "http://localhost";
const UA: &str = "IslandShell/0.1 (personal Windows desktop shell)";

// ---------- what the page sees ----------

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub mute: bool,
    pub deaf: bool,
    pub speaking: bool,
}

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct Voice {
    pub channel_id: String,
    pub name: String,
    pub guild: String,
    pub members: Vec<Member>,
}

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct Status {
    /// A Client ID has been entered.
    pub configured: bool,
    pub client_id: String,
    /// A Client Secret is saved (needed for voice and notifications).
    pub has_secret: bool,
    /// Talking to the Discord app right now.
    pub connected: bool,
    /// Signed in with voice/notification access.
    pub authorized: bool,
    pub user: Option<Member>,
    pub voice: Option<Voice>,
    /// Your own mic mute and deafen.
    pub mute: bool,
    pub deaf: bool,
    pub presence: bool,
    pub notify: bool,
    /// What's on the status right now ("" when nothing).
    pub presence_now: String,
    pub error: String,
}

// ---------- saved settings ----------

#[derive(Serialize, Deserialize, Clone)]
struct Saved {
    #[serde(default)]
    client_id: String,
    /// DPAPI-encrypted, base64.
    #[serde(default)]
    secret: String,
    #[serde(default)]
    access: String,
    #[serde(default)]
    refresh: String,
    #[serde(default)]
    expires_at: u64,
    #[serde(default = "yes")]
    presence: bool,
    #[serde(default = "yes")]
    notify: bool,
}

fn yes() -> bool {
    true
}

impl Default for Saved {
    fn default() -> Self {
        Saved { client_id: String::new(), secret: String::new(), access: String::new(), refresh: String::new(), expires_at: 0, presence: true, notify: true }
    }
}

fn saved_path() -> PathBuf {
    crate::util::data_dir().join("discord.json")
}

fn load() -> Saved {
    std::fs::read(saved_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(saved_path(), b);
    }
}

// ---------- commands from the page ----------

pub enum Cmd {
    Configure { client_id: String, secret: String },
    Authorize,
    Mute(bool),
    Deaf(bool),
    Leave,
    Options { presence: Option<bool>, notify: Option<bool> },
    Forget,
}

static TX: OnceLock<Mutex<Sender<Cmd>>> = OnceLock::new();
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

pub fn status() -> Status {
    STATUS.lock().unwrap().clone().unwrap_or_default()
}

pub fn send(cmd: Cmd) -> Result<(), String> {
    TX.get().ok_or("Discord integration isn't running")?.lock().unwrap().send(cmd).map_err(|_| "Discord integration stopped".to_string())
}

pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    let (tx, rx) = channel();
    if TX.set(Mutex::new(tx)).is_ok() {
        std::thread::spawn(move || {
            crate::util::com_mta();
            run(app, shared, rx)
        });
    }
}

// ---------- the pipe ----------

struct Pipe(File);

impl Pipe {
    fn open() -> Option<Pipe> {
        (0..10).find_map(|i| std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok()).map(Pipe)
    }

    fn write(&mut self, op: u32, body: &Value) -> std::io::Result<()> {
        let data = serde_json::to_vec(body).unwrap_or_default();
        let mut frame = Vec::with_capacity(8 + data.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
        frame.extend_from_slice(&data);
        self.0.write_all(&frame)
    }

    /// One frame if one has arrived; never blocks waiting for Discord to say something.
    fn read(&mut self) -> std::io::Result<Option<(u32, Value)>> {
        let mut avail = 0u32;
        let h = HANDLE(self.0.as_raw_handle());
        if unsafe { PeekNamedPipe(h, None, 0, None, Some(&mut avail), None) }.is_err() {
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Discord closed the connection"));
        }
        if avail < 8 {
            return Ok(None);
        }
        let mut head = [0u8; 8];
        self.0.read_exact(&mut head)?;
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let mut body = vec![0u8; len];
        self.0.read_exact(&mut body)?;
        Ok(Some((op, serde_json::from_slice(&body).unwrap_or(Value::Null))))
    }
}

// ---------- HTTP (OAuth token exchange, album art lookup) ----------

fn http(url: &str, form: Option<&str>) -> Result<Value, String> {
    let e = |e: windows::core::Error| e.message().to_string();
    let client = HttpClient::new().map_err(e)?;
    let _ = client.DefaultRequestHeaders().and_then(|h| h.UserAgent()).and_then(|ua| ua.TryParseAdd(&HSTRING::from(UA)));
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(e)?;
    let resp = match form {
        Some(body) => {
            let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(&HSTRING::from(body), UnicodeEncoding::Utf8, &HSTRING::from("application/x-www-form-urlencoded")).map_err(e)?;
            client.PostAsync(&uri, &content).and_then(|op| op.get()).map_err(e)?
        }
        None => client.GetAsync(&uri).and_then(|op| op.get()).map_err(e)?,
    };
    let status = resp.StatusCode().map_err(e)?.0;
    let text = resp.Content().and_then(|c| c.ReadAsStringAsync()).and_then(|op| op.get()).map_err(e)?.to_string();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let why = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("").to_string();
        return Err(format!("HTTP {status}{}", if why.is_empty() { String::new() } else { format!(": {why}") }));
    }
    Ok(v)
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------- the session ----------

#[derive(Clone)]
enum Pending {
    Authorize,
    Authenticate,
    SelectedChannel,
    Channel,
    Guild(String),
    VoiceSettings,
    Other,
}

struct Client {
    app: AppHandle,
    shared: Arc<Shared>,
    saved: Saved,
    st: Status,
    pipe: Option<Pipe>,
    nonce: u64,
    pending: HashMap<String, Pending>,
    channel_subs: Option<String>,
    /// Album art + Apple Music link per song, so each is looked up once.
    art: HashMap<String, (String, String)>,
    activity_key: String,
    activity_started: i64,
    last_presence: Instant,
}

fn run(app: AppHandle, shared: Arc<Shared>, rx: Receiver<Cmd>) {
    let saved = load();
    let mut c = Client {
        app,
        shared,
        st: Status { presence: saved.presence, notify: saved.notify, ..Default::default() },
        saved,
        pipe: None,
        nonce: 0,
        pending: HashMap::new(),
        channel_subs: None,
        art: HashMap::new(),
        activity_key: String::new(),
        activity_started: 0,
        last_presence: Instant::now(),
    };
    c.refresh_flags();
    c.publish();
    let mut next_try = Instant::now();
    loop {
        // Commands work whether or not Discord is open (setup, options).
        while let Ok(cmd) = rx.try_recv() {
            c.command(cmd);
        }
        if c.pipe.is_none() && !c.saved.client_id.is_empty() && Instant::now() >= next_try {
            c.connect();
            next_try = Instant::now() + Duration::from_secs(5);
        }
        if c.pipe.is_some() {
            loop {
                let frame = match c.pipe.as_mut().map(Pipe::read) {
                    Some(Ok(Some(f))) => f,
                    Some(Ok(None)) | None => break,
                    Some(Err(e)) => {
                        c.disconnected(&e.to_string());
                        break;
                    }
                };
                c.frame(frame.0, frame.1);
            }
            if c.last_presence.elapsed() > Duration::from_secs(2) {
                c.last_presence = Instant::now();
                c.presence_tick();
            }
        }
        std::thread::sleep(Duration::from_millis(if c.pipe.is_some() { 25 } else { 250 }));
    }
}

impl Client {
    fn refresh_flags(&mut self) {
        self.st.configured = !self.saved.client_id.is_empty();
        self.st.client_id = self.saved.client_id.clone();
        self.st.has_secret = !self.saved.secret.is_empty();
        self.st.presence = self.saved.presence;
        self.st.notify = self.saved.notify;
    }

    fn publish(&mut self) {
        let mut cur = STATUS.lock().unwrap();
        if cur.as_ref() != Some(&self.st) {
            *cur = Some(self.st.clone());
            let _ = self.app.emit("discord:status", &self.st);
        }
    }

    fn send(&mut self, cmd: &str, args: Value, evt: Option<&str>, why: Pending) {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let mut body = json!({ "cmd": cmd, "args": args, "nonce": nonce });
        if let Some(e) = evt {
            body["evt"] = json!(e);
        }
        self.pending.insert(nonce, why);
        let failed = match self.pipe.as_mut() {
            Some(p) => p.write(1, &body).err(),
            None => return,
        };
        if let Some(e) = failed {
            self.disconnected(&e.to_string());
        }
    }

    fn subscribe(&mut self, evt: &str, args: Value) {
        self.send("SUBSCRIBE", args, Some(evt), Pending::Other);
    }

    fn connect(&mut self) {
        let Some(mut pipe) = Pipe::open() else {
            self.st.connected = false;
            self.publish();
            return;
        };
        if pipe.write(0, &json!({ "v": 1, "client_id": self.saved.client_id })).is_err() {
            return;
        }
        self.pipe = Some(pipe);
        // READY arrives as the first frame.
    }

    fn disconnected(&mut self, why: &str) {
        if self.pipe.take().is_some() {
            log_line(&format!("discord: disconnected ({why})"));
        }
        self.pending.clear();
        self.channel_subs = None;
        self.activity_key.clear();
        self.st.connected = false;
        self.st.authorized = false;
        self.st.voice = None;
        self.st.presence_now.clear();
        self.publish();
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Configure { client_id, secret } => {
                let id = client_id.trim().to_string();
                if id != self.saved.client_id {
                    // A different app: old tokens don't apply.
                    self.saved = Saved { client_id: id, presence: self.saved.presence, notify: self.saved.notify, ..Default::default() };
                    self.disconnected("new application");
                }
                if !secret.trim().is_empty() {
                    self.saved.secret = protect(secret.trim());
                }
                save(&self.saved);
                self.st.error.clear();
                self.refresh_flags();
                self.publish();
            }
            Cmd::Authorize => {
                if self.pipe.is_none() {
                    self.st.error = "Discord isn't open (or the Client ID is wrong).".into();
                } else if self.saved.secret.is_empty() {
                    self.st.error = "Add the Client Secret first.".into();
                } else {
                    self.st.error.clear();
                    let args = json!({ "client_id": self.saved.client_id, "scopes": SCOPES });
                    self.send("AUTHORIZE", args, None, Pending::Authorize);
                }
                self.publish();
            }
            Cmd::Mute(on) => self.send("SET_VOICE_SETTINGS", json!({ "mute": on }), None, Pending::VoiceSettings),
            Cmd::Deaf(on) => self.send("SET_VOICE_SETTINGS", json!({ "deaf": on }), None, Pending::VoiceSettings),
            Cmd::Leave => self.send("SELECT_VOICE_CHANNEL", json!({ "channel_id": null }), None, Pending::Other),
            Cmd::Options { presence, notify } => {
                if let Some(p) = presence {
                    self.saved.presence = p;
                    if !p {
                        self.clear_activity();
                    }
                    self.activity_key.clear();
                }
                if let Some(n) = notify {
                    let was = self.saved.notify;
                    self.saved.notify = n;
                    if n && !was && self.st.authorized {
                        self.subscribe("NOTIFICATION_CREATE", json!({}));
                    } else if !n && was && self.st.authorized {
                        self.send("UNSUBSCRIBE", json!({}), Some("NOTIFICATION_CREATE"), Pending::Other);
                    }
                }
                save(&self.saved);
                self.refresh_flags();
                self.publish();
            }
            Cmd::Forget => {
                self.clear_activity();
                self.saved = Saved::default();
                save(&self.saved);
                self.disconnected("forgotten");
                self.st = Status::default();
                self.refresh_flags();
                self.publish();
            }
        }
    }

    fn frame(&mut self, op: u32, v: Value) {
        match op {
            2 => {
                let why = v["message"].as_str().unwrap_or("closed").to_string();
                self.st.error = if why.to_lowercase().contains("client id") { "Discord didn't recognize that Client ID.".into() } else { String::new() };
                self.disconnected(&why);
            }
            3 => {
                if let Some(p) = self.pipe.as_mut() {
                    let _ = p.write(4, &v);
                }
            }
            1 => self.message(v),
            _ => {}
        }
    }

    fn message(&mut self, v: Value) {
        let evt = v["evt"].as_str().unwrap_or("");
        let data = &v["data"];
        let why = v["nonce"].as_str().and_then(|n| self.pending.remove(n));
        if evt == "ERROR" {
            let msg = data["message"].as_str().unwrap_or("error").to_string();
            match why {
                Some(Pending::Authorize) => self.st.error = format!("Not authorized: {msg}"),
                Some(Pending::Authenticate) => {
                    // The saved login no longer works; try the refresh token once.
                    if !self.try_refresh() {
                        self.saved.access.clear();
                        self.saved.refresh.clear();
                        save(&self.saved);
                        self.st.authorized = false;
                        self.st.error = "Discord signed the island out; authorize again.".into();
                    }
                }
                _ => log_line(&format!("discord: {} failed: {msg}", v["cmd"].as_str().unwrap_or("?"))),
            }
            self.publish();
            return;
        }
        match (v["cmd"].as_str().unwrap_or(""), evt) {
            ("DISPATCH", "READY") => {
                self.st.connected = true;
                self.st.error.clear();
                self.st.user = Some(member_of(&data["user"], None));
                log_line("discord: connected");
                self.authenticate();
            }
            ("DISPATCH", "VOICE_CHANNEL_SELECT") => match data["channel_id"].as_str() {
                Some(id) => self.send("GET_CHANNEL", json!({ "channel_id": id }), None, Pending::Channel),
                None => self.set_voice(&Value::Null),
            },
            ("DISPATCH", "VOICE_STATE_CREATE") | ("DISPATCH", "VOICE_STATE_UPDATE") => {
                if let Some(vc) = self.st.voice.as_mut() {
                    let m = member_of(&data["user"], Some(data));
                    match vc.members.iter_mut().find(|x| x.id == m.id) {
                        Some(x) => *x = Member { speaking: x.speaking, ..m },
                        None => vc.members.push(m),
                    }
                }
            }
            ("DISPATCH", "VOICE_STATE_DELETE") => {
                let id = data["user"]["id"].as_str().unwrap_or("").to_string();
                if let Some(vc) = self.st.voice.as_mut() {
                    vc.members.retain(|m| m.id != id);
                }
            }
            ("DISPATCH", "SPEAKING_START") | ("DISPATCH", "SPEAKING_STOP") => {
                let id = data["user_id"].as_str().unwrap_or("").to_string();
                let on = evt == "SPEAKING_START";
                if let Some(m) = self.st.voice.as_mut().and_then(|vc| vc.members.iter_mut().find(|m| m.id == id)) {
                    m.speaking = on;
                }
                // Speaking flips many times a second; send just that, not the whole status.
                let _ = self.app.emit("discord:speaking", json!({ "id": id, "speaking": on }));
                if let Some(s) = STATUS.lock().unwrap().as_mut() {
                    *s = self.st.clone();
                }
                return;
            }
            ("DISPATCH", "VOICE_SETTINGS_UPDATE") => self.voice_settings(data),
            ("DISPATCH", "NOTIFICATION_CREATE") => {
                // Only messages that @mention you yourself (not @everyone, @here or a role).
                let me = self.st.user.as_ref().map(|u| u.id.as_str()).unwrap_or("");
                if !mentions(&data["message"], me) {
                    return;
                }
                let _ = self.app.emit(
                    "discord:notify",
                    json!({ "title": data["title"], "body": data["body"], "icon": data["icon_url"], "channel": data["channel_id"] }),
                );
                return;
            }
            ("AUTHORIZE", _) => {
                if let Some(code) = data["code"].as_str() {
                    match self.exchange(&format!("grant_type=authorization_code&code={}&redirect_uri={}", enc(code), enc(REDIRECT))) {
                        Ok(()) => self.authenticate(),
                        Err(e) => self.st.error = format!("Couldn't finish signing in: {e}{}", hint(&e)),
                    }
                }
            }
            ("AUTHENTICATE", _) => {
                self.st.authorized = true;
                self.st.error.clear();
                if data["user"].is_object() {
                    self.st.user = Some(member_of(&data["user"], None));
                }
                log_line("discord: signed in");
                self.subscribe("VOICE_CHANNEL_SELECT", json!({}));
                self.subscribe("VOICE_SETTINGS_UPDATE", json!({}));
                if self.saved.notify {
                    self.subscribe("NOTIFICATION_CREATE", json!({}));
                }
                self.send("GET_SELECTED_VOICE_CHANNEL", json!({}), None, Pending::SelectedChannel);
                self.send("GET_VOICE_SETTINGS", json!({}), None, Pending::VoiceSettings);
            }
            (_, _) => match why {
                Some(Pending::SelectedChannel) | Some(Pending::Channel) => self.set_voice(data),
                Some(Pending::Guild(channel)) => {
                    if let Some(vc) = self.st.voice.as_mut().filter(|vc| vc.channel_id == channel) {
                        vc.guild = data["name"].as_str().unwrap_or("").to_string();
                    }
                }
                Some(Pending::VoiceSettings) => self.voice_settings(data),
                _ => {}
            },
        }
        self.publish();
    }

    fn voice_settings(&mut self, data: &Value) {
        if let Some(m) = data["mute"].as_bool() {
            self.st.mute = m;
        }
        if let Some(d) = data["deaf"].as_bool() {
            self.st.deaf = d;
        }
    }

    /// Signs in with the saved token (refreshing it first when it has expired).
    fn authenticate(&mut self) {
        if self.saved.access.is_empty() {
            return;
        }
        if crate::util::now_ms() + 60_000 > self.saved.expires_at && !self.try_refresh() {
            return;
        }
        let token = unprotect(&self.saved.access);
        self.send("AUTHENTICATE", json!({ "access_token": token }), None, Pending::Authenticate);
    }

    fn try_refresh(&mut self) -> bool {
        let refresh = unprotect(&self.saved.refresh);
        if refresh.is_empty() {
            return false;
        }
        match self.exchange(&format!("grant_type=refresh_token&refresh_token={}", enc(&refresh))) {
            Ok(()) => true,
            Err(e) => {
                log_line(&format!("discord: token refresh failed: {e}"));
                false
            }
        }
    }

    /// OAuth token request with the app's id and secret; stores the new tokens.
    fn exchange(&mut self, grant: &str) -> Result<(), String> {
        let secret = unprotect(&self.saved.secret);
        let body = format!("client_id={}&client_secret={}&{grant}", enc(&self.saved.client_id), enc(&secret));
        let v = http("https://discord.com/api/oauth2/token", Some(&body))?;
        let access = v["access_token"].as_str().ok_or("no access token in Discord's reply")?;
        self.saved.access = protect(access);
        if let Some(r) = v["refresh_token"].as_str() {
            self.saved.refresh = protect(r);
        }
        self.saved.expires_at = crate::util::now_ms() + v["expires_in"].as_u64().unwrap_or(604_800) * 1000;
        save(&self.saved);
        Ok(())
    }

    /// The call we're in (from GET_CHANNEL / GET_SELECTED_VOICE_CHANNEL), or none.
    fn set_voice(&mut self, ch: &Value) {
        let id = ch["id"].as_str().map(String::from);
        if self.channel_subs != id {
            if let Some(old) = self.channel_subs.take() {
                for evt in ["VOICE_STATE_CREATE", "VOICE_STATE_UPDATE", "VOICE_STATE_DELETE", "SPEAKING_START", "SPEAKING_STOP"] {
                    self.send("UNSUBSCRIBE", json!({ "channel_id": old }), Some(evt), Pending::Other);
                }
            }
            if let Some(new) = &id {
                for evt in ["VOICE_STATE_CREATE", "VOICE_STATE_UPDATE", "VOICE_STATE_DELETE", "SPEAKING_START", "SPEAKING_STOP"] {
                    self.subscribe(evt, json!({ "channel_id": new }));
                }
            }
            self.channel_subs = id.clone();
        }
        let Some(id) = id else {
            self.st.voice = None;
            return;
        };
        let members = ch["voice_states"].as_array().map(|a| a.iter().map(|s| member_of(&s["user"], Some(s))).collect()).unwrap_or_default();
        let guild = self.st.voice.as_ref().filter(|v| v.channel_id == id).map(|v| v.guild.clone()).unwrap_or_default();
        self.st.voice = Some(Voice { channel_id: id.clone(), name: ch["name"].as_str().unwrap_or("Voice").to_string(), guild, members });
        if let Some(g) = ch["guild_id"].as_str() {
            self.send("GET_GUILD", json!({ "guild_id": g }), None, Pending::Guild(id));
        } else if let Some(vc) = self.st.voice.as_mut() {
            vc.guild = "Direct call".into();
        }
    }

    // ---------- Rich Presence ----------

    fn clear_activity(&mut self) {
        if self.pipe.is_some() {
            self.send("SET_ACTIVITY", json!({ "pid": std::process::id() }), None, Pending::Other);
        }
        self.activity_key.clear();
        self.st.presence_now.clear();
    }

    /// Every couple of seconds: put what Apple Music is playing on the status, or take it off.
    fn presence_tick(&mut self) {
        if !self.st.connected || !self.saved.presence {
            return;
        }
        let media = self.shared.status.lock().unwrap().media.clone();
        let playing = media.filter(|m| m.source == "apple" && m.playing && !m.title.is_empty());
        let Some(m) = playing else {
            if !self.activity_key.is_empty() {
                self.clear_activity();
                self.publish();
            }
            return;
        };
        // Where the song started, from the current position; a jump of more than 3 s means a seek.
        let started = m.sampled_at as i64 - (m.position * 1000.0) as i64;
        let moved = (started - self.activity_started).abs() > 3000;
        if self.activity_key == m.key && !moved {
            return;
        }
        self.activity_key = m.key.clone();
        self.activity_started = started;
        let (art, link) = self.art_for(&m.artist, &m.title);
        let text = |s: &str| {
            let t: String = s.chars().take(127).collect();
            if t.chars().count() < 2 { format!("{t}  ") } else { t }
        };
        let mut activity = json!({
            "type": 2,
            "details": text(&m.title),
            "state": text(&if m.album.is_empty() { format!("by {}", m.artist) } else { format!("by {} · {}", m.artist, m.album) }),
            "instance": false,
        });
        if m.duration > 0.0 {
            activity["timestamps"] = json!({ "start": started, "end": started + (m.duration * 1000.0) as i64 });
        }
        if !art.is_empty() {
            activity["assets"] = json!({ "large_image": art, "large_text": text(if m.album.is_empty() { &m.title } else { &m.album }) });
        }
        if !link.is_empty() {
            activity["buttons"] = json!([{ "label": "Listen on Apple Music", "url": link }]);
        }
        self.send("SET_ACTIVITY", json!({ "pid": std::process::id(), "activity": activity }), None, Pending::Other);
        self.st.presence_now = format!("{} · {}", m.title, m.artist);
        self.publish();
    }

    /// Album art URL and Apple Music link from Apple's public iTunes catalog (looked up once per song).
    fn art_for(&mut self, artist: &str, title: &str) -> (String, String) {
        let key = format!("{artist}\n{title}");
        if let Some(v) = self.art.get(&key) {
            return v.clone();
        }
        let url = format!("https://itunes.apple.com/search?media=music&entity=song&limit=1&term={}", enc(&format!("{artist} {title}")));
        let found = http(&url, None)
            .ok()
            .and_then(|v| {
                let r = v["results"].get(0)?.clone();
                let art = r["artworkUrl100"].as_str()?.replace("100x100bb", "512x512bb");
                Some((art, r["trackViewUrl"].as_str().unwrap_or("").to_string()))
            })
            .unwrap_or_default();
        self.art.insert(key, found.clone());
        found
    }
}

/// "portal" opens Discord's developer portal in the default browser (never Edge, see
/// util::open_web); "app" brings up the Discord app.
pub fn open(what: &str) -> Result<(), String> {
    match what {
        "portal" => crate::util::open_web("https://discord.com/developers/applications"),
        "app" => crate::util::shell_open("discord://-/channels/@me"),
        _ => Err("unknown target".into()),
    }
}

/// Whether a message @mentions this user directly (a reply that pings you counts too).
fn mentions(msg: &Value, me: &str) -> bool {
    if me.is_empty() {
        return false;
    }
    let listed = msg["mentions"].as_array().is_some_and(|a| a.iter().any(|u| u["id"].as_str() == Some(me)));
    let content = msg["content"].as_str().unwrap_or("");
    listed || content.contains(&format!("<@{me}>")) || content.contains(&format!("<@!{me}>"))
}

/// Hints for the setup mistakes Discord reports.
fn hint(e: &str) -> &'static str {
    if e.contains("invalid_client") {
        " (check the Client Secret)"
    } else if e.contains("redirect_uri") {
        " (add http://localhost under OAuth2 → Redirects in your Discord app)"
    } else {
        ""
    }
}

fn avatar(user: &Value) -> String {
    let id = user["id"].as_str().unwrap_or("0");
    match user["avatar"].as_str() {
        Some(h) if !h.is_empty() => format!("https://cdn.discordapp.com/avatars/{id}/{h}.png?size=64"),
        _ => {
            let n = id.parse::<u64>().map(|i| (i >> 22) % 6).unwrap_or(0);
            format!("https://cdn.discordapp.com/embed/avatars/{n}.png")
        }
    }
}

/// A person from a user object, plus their voice state when there is one.
fn member_of(user: &Value, vs: Option<&Value>) -> Member {
    let name = vs
        .and_then(|v| v["nick"].as_str())
        .filter(|n| !n.is_empty())
        .or(user["global_name"].as_str().filter(|n| !n.is_empty()))
        .or(user["username"].as_str())
        .unwrap_or("Someone")
        .to_string();
    let st = vs.map(|v| &v["voice_state"]);
    let flag = |k: &str| st.and_then(|s| s[k].as_bool()).unwrap_or(false);
    Member {
        id: user["id"].as_str().unwrap_or("").to_string(),
        name,
        avatar: avatar(user),
        mute: flag("mute") || flag("self_mute") || vs.and_then(|v| v["mute"].as_bool()).unwrap_or(false),
        deaf: flag("deaf") || flag("self_deaf"),
        speaking: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_avatars() {
        let user = json!({ "id": "80351110224678912", "username": "nelly", "global_name": "Nelly", "avatar": "8342729096ea3675442027381ff50dfe" });
        let vs = json!({ "nick": "", "mute": false, "voice_state": { "self_mute": true, "self_deaf": false } });
        let m = member_of(&user, Some(&vs));
        assert_eq!(m.name, "Nelly");
        assert!(m.mute && !m.deaf);
        assert_eq!(m.avatar, "https://cdn.discordapp.com/avatars/80351110224678912/8342729096ea3675442027381ff50dfe.png?size=64");
        let plain = member_of(&json!({ "id": "80351110224678912", "username": "x" }), None);
        assert!(plain.avatar.starts_with("https://cdn.discordapp.com/embed/avatars/"));
    }

    /// Against the running Discord app: a made-up Client ID is refused with a CLOSE frame.
    #[test]
    #[ignore]
    fn live_pipe_framing() {
        let mut p = Pipe::open().expect("Discord isn't running");
        p.write(0, &json!({ "v": 1, "client_id": "1" })).unwrap();
        let start = Instant::now();
        loop {
            match p.read() {
                Ok(Some((op, v))) => {
                    println!("op {op}: {v}");
                    assert_eq!(op, 2);
                    break;
                }
                Ok(None) => assert!(start.elapsed() < Duration::from_secs(5), "no reply"),
                Err(e) => panic!("{e}"),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn only_direct_mentions() {
        let me = "80351110224678912";
        assert!(mentions(&json!({ "content": "hey <@80351110224678912> you on?", "mentions": [{ "id": me }] }), me));
        assert!(mentions(&json!({ "content": "replying", "mentions": [{ "id": me }] }), me));
        assert!(mentions(&json!({ "content": "old style <@!80351110224678912>" }), me));
        assert!(!mentions(&json!({ "content": "@everyone raid at 9", "mention_everyone": true, "mentions": [] }), me));
        assert!(!mentions(&json!({ "content": "<@&123> role ping", "mention_roles": ["123"], "mentions": [] }), me));
        assert!(!mentions(&json!({ "content": "hi <@1>", "mentions": [{ "id": "1" }] }), me));
        assert!(!mentions(&json!({ "content": "<@80351110224678912>" }), ""));
    }

    #[test]
    fn dpapi_round_trip() {
        let enc = protect("s3cret value");
        assert!(!enc.is_empty() && !enc.contains("s3cret"));
        assert_eq!(unprotect(&enc), "s3cret value");
        assert_eq!(unprotect(""), "");
    }
}
