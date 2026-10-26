// Music: what's playing, for the island and for the module's own views. The backend (src/main.rs)
// reads the player and sends `state` when it changes. This keeps it (what you just pressed wins
// over the next few readings, since players report late), carries the position forward, fetches the
// cover and the lyrics, and:
// - puts it on the island as a `media` activity: the pill's cover and equalizer, the peek, the
//   Controls card and the top bar are the island's own, drawn from it; their buttons come back here;
// - fills the Music tab (tab.js) and the Command Center's Music sheet (sheet.js) through `core`;
// - adds the quick commands, the cube's line and the setup guide's card;
// - keeps "pause the music when I step away" (with an eye tracker; the backend does the pausing).

import { createTab } from './tab.js';
import { createSheet } from './sheet.js';

let ctx, m = null, hold = null, seek = null, art = '', artFor = '', artTry = 0, tint = null, vol = null, volFor = '';
let ly = { key: '', state: 'idle', data: null }, lyWant = '', lyWantAt = 0, lyricsOn = true, lyLine = '', lyT = 0;
let settings = { pause_when_away: false, tracker: false };
const subs = new Set(), levelSubs = new Set();
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const changed = what => { for (const f of subs) { try { f(what); } catch (e) { console.error(e); } } };

/** Where the song is right now, carried forward from the last reading. */
function curPos() {
  if (!m) return 0;
  const now = performance.now();
  if (seek && now < seek.until) return seek.pos + (m.playing ? (now - seek.at) / 1000 : 0);
  const p = m.position + (m.playing ? Math.max(0, Date.now() - m.sampled_at) / 1000 : 0);
  return m.duration > 0 ? Math.min(p, m.duration) : p;
}

/** A reading from the backend, or null when nothing is playing. */
function setState(s) {
  if (s && hold && performance.now() < hold.until) for (const k in hold) if (k !== 'until') s[k] = hold[k];
  const newSong = !!s !== !!m || !!(s && m && s.key !== m.key);
  m = s || null;
  if (!m) { art = ''; artFor = ''; tint = null; vol = null; volFor = ''; }
  else {
    if (m.key !== artFor) { art = ''; tint = null; loadArt(); }
    else if (!art && Date.now() - artTry > 5000) loadArt(); // the cover sometimes comes after the title
    if (m.exe !== volFor) refreshVolume();
  }
  if (newSong) seek = null;
  maybeLyrics(); publish(); renderCommands();
  changed(newSong ? 'song' : 'state');
}

/** The cover, as the backend saved it in the module's cache folder. */
function loadArt() {
  const key = m.key;
  artFor = key; artTry = Date.now();
  ctx.call('thumb').then(name => {
    if (!m || m.key !== key || !name) return;
    art = ctx.cacheUrl(name);
    tintFrom(art); publish(); changed('art');
  }).catch(() => {});
}

/** The player's own volume (the island reads the app's audio session). */
function refreshVolume() {
  const exe = m ? m.exe : '';
  volFor = exe;
  if (!exe) { vol = null; changed('volume'); return; }
  ctx.island.audio.appVolume(exe).then(v => { if (volFor === exe) { vol = v == null ? null : v; changed('volume'); } }).catch(() => {});
}

/** What the island shows: the highest `media` activity (ours), with run() for its buttons. */
function publish() {
  if (!m) { ctx.activity.clear('now'); return; }
  const seeking = seek && performance.now() < seek.until;
  ctx.activity.set({
    id: 'now', kind: 'media', text: m.title, sub: m.artist || m.app, app: m.app, art, playing: m.playing,
    position: seeking ? curPos() : m.position, sampled_at: seeking ? Date.now() : m.sampled_at, duration: m.duration,
    can_next: m.can_next, can_prev: m.can_prev, can_seek: m.can_seek, key: m.key, exe: m.exe, lyric: lyLine,
    action: 'tab:music', run: act
  });
}

/** What you just pressed shows at once and wins over the next readings for a moment. */
function holdFor(fields, ms = 1800) {
  hold = { ...fields, until: performance.now() + ms };
  if ('playing' in fields) {
    // Carry the position over the switch, so it neither jumps nor freezes.
    m.position = curPos(); m.sampled_at = Date.now();
  }
  Object.assign(m, fields);
}

/** A transport button: prev, toggle, next, shuffle, repeat, favorite, open. */
function act(action) {
  if (action === 'open') return openPlayer();
  if (m) {
    if (action === 'toggle') holdFor({ playing: !m.playing }, 1500);
    if (action === 'shuffle' && m.shuffle != null) holdFor({ shuffle: !m.shuffle });
    if (action === 'repeat' && m.repeat != null) holdFor({ repeat: { off: 'all', all: 'one', one: 'off' }[m.repeat] || 'off' });
    if (action === 'favorite' && m.favorite != null) holdFor({ favorite: !m.favorite }, 2500);
    if (action === 'next' || action === 'prev') seek = null;
    publish(); changed('state');
  }
  return ctx.call('cmd', { action }).catch(err => { hold = null; throw err; });
}

function seekTo(sec) {
  if (!m || !m.can_seek) return Promise.resolve();
  sec = clamp(sec, 0, m.duration > 0 ? m.duration - 0.5 : sec);
  seek = { pos: sec, at: performance.now(), until: performance.now() + 2200 };
  publish(); changed('seek');
  return ctx.call('cmd', { action: 'seek', value: sec }).catch(err => { seek = null; changed('seek'); throw err; });
}

/** Brings the player forward (the island does it: it's the one in front when you ask). */
function openPlayer() {
  if (!m) return Promise.resolve();
  return ctx.island.focusApp({ id: m.id, exe: m.exe, hwnd: m.hwnd });
}

function setSource(id) {
  return ctx.call('source', { id }).then(() => ctx.call('state')).then(setState);
}

function setVolume(level) {
  if (!m || !m.exe) return Promise.resolve();
  vol = level;
  return ctx.island.audio.setAppVolume(m.exe, level);
}

/* ---------- lyrics (LRCLIB, looked up by the backend) ---------- */
function maybeLyrics() {
  const key = m ? m.key : '';
  if (key !== lyWant) { lyWant = key; lyWantAt = Date.now(); }
  if (ly.key && ly.key !== key) { ly = { key: '', state: 'idle', data: null }; lyLine = ''; changed('lyrics'); }
  if (!m || !lyricsOn || ly.key === key) return;
  // Wait a moment for the song's length: it makes the match exact.
  if (!(m.duration > 0) && Date.now() - lyWantAt < 2500) { setTimeout(maybeLyrics, 600); return; }
  ly = { key, state: 'loading', data: null };
  changed('lyrics');
  ctx.call('lyrics', { title: m.title, artist: m.artist, album: m.album || '', duration: m.duration || 0 })
    .then(d => { if (ly.key === key) { ly = { key, state: d ? 'ok' : 'none', data: d }; changed('lyrics'); followLyrics(); } })
    .catch(() => {
      if (ly.key !== key) return;
      ly = { key, state: 'error', data: null }; changed('lyrics');
      setTimeout(() => { if (ly.key === key && ly.state === 'error') { ly.key = ''; maybeLyrics(); } }, 30000); // try again later
    });
}
/** The line being sung at `pos` (-1 before the first), a beat early like Apple Music. */
function lineAt(pos) {
  const d = ly.data; if (!d || !d.lines.length) return -1;
  const t = pos + 0.3;
  let idx = -1, lo = 0, hi = d.lines.length - 1;
  while (lo <= hi) { const mid = (lo + hi) >> 1; if (d.lines[mid].t <= t) { idx = mid; lo = mid + 1; } else hi = mid - 1; }
  return idx;
}
/** The sung line for the island's peek, kept current while there are timed lyrics. */
function followLyrics() {
  clearTimeout(lyT);
  const d = ly.data, timed = !!(d && d.lines.length && m);
  const i = timed ? lineAt(curPos()) : -1;
  const text = i >= 0 ? d.lines[i].text : '';
  if (text !== lyLine) { lyLine = text; publish(); }
  if (timed) lyT = setTimeout(followLyrics, m.playing ? 250 : 1000);
}
function toggleLyrics() {
  lyricsOn = !lyricsOn; ctx.store.set('lyrics', lyricsOn);
  ly = { key: '', state: 'idle', data: null }; lyLine = '';
  publish(); changed('lyrics'); maybeLyrics();
}

/* ---------- the cover's color, for the Music views ---------- */
function tintFrom(url) {
  const img = new Image();
  img.crossOrigin = 'anonymous'; // the module's own cache: readable with CORS
  img.onload = () => {
    const c = document.createElement('canvas'); c.width = c.height = 24;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(img, 0, 0, 24, 24);
    let px;
    try { px = g.getImageData(0, 0, 24, 24).data; } catch (e) { return; }
    let r = 0, gr = 0, b = 0, sum = 0;
    for (let i = 0; i < px.length; i += 4) {
      const R = px[i], G = px[i + 1], B = px[i + 2], mx = Math.max(R, G, B), mn = Math.min(R, G, B);
      const w = (mx ? (mx - mn) / mx : 0) ** 2 * (mx / 255) + 0.015; // favor vivid, bright pixels
      r += R * w; gr += G * w; b += B * w; sum += w;
    }
    tint = [r / sum, gr / sum, b / sum];
    changed('tint');
  };
  img.src = url;
}
/** The tint as CSS, readable on the island: lighter on the dark skin, deeper on the light one. */
function tintCss() {
  if (!tint) return null;
  let [r, g, b] = tint.map(v => v / 255);
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
  let h = 0;
  if (d) h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h = Math.round(h * 60 + 360) % 360;
  const l0 = (mx + mn) / 2, s0 = d ? d / (1 - Math.abs(2 * l0 - 1)) : 0;
  const dark = ctx.island.state().dark !== false;
  const sat = Math.round(clamp(s0 * 100, d ? 38 : 0, 85));
  const lig = Math.round(dark ? clamp(l0 * 100, 58, 72) : clamp(l0 * 100, 36, 48));
  return { tint: `hsl(${h} ${sat}% ${lig}%)`, on: lig > 55 ? '#141218' : '#ffffff' };
}

/* ---------- the island's quick commands ---------- */
function renderCommands() {
  ctx.commands.set(m ? [
    { id: 'toggle', title: m.playing ? 'Pause the music' : 'Play the music', icon: m.playing ? 'pause' : 'play', sub: `${m.app || 'Music'} · ${m.title}`, keys: 'apple music media play pause song', run: () => act('toggle') },
    { id: 'next', title: 'Next track', icon: 'next', sub: m.app || 'Music', keys: 'skip song music', run: () => act('next') }
  ] : []);
}

function setPause(on) {
  return ctx.call('set', { pause_when_away: on }).then(s => { settings = s; changed('settings'); });
}

/** What the Music tab and sheet share. */
const core = {
  get ctx() { return ctx; },
  media: () => m, art: () => art, tint: tintCss, rgb: () => tint, lyrics: () => ly, lyricsOn: () => lyricsOn, volume: () => vol,
  settings: () => settings, curPos, lineAt, seekTo, act, setSource, setVolume, setPause, toggleLyrics, refreshVolume,
  on(fn) { subs.add(fn); return () => subs.delete(fn); },
  onLevel(fn) { levelSubs.add(fn); return () => levelSubs.delete(fn); }
};

export function activate(c) {
  ctx = c;
  // Lyrics on or off, kept from 0.1.x's island.lyrics the first time.
  try { const v = localStorage.getItem('island.lyrics'); if (v != null && localStorage.getItem('mod.music.lyrics') == null) localStorage.setItem('mod.music.lyrics', v); } catch (e) {}
  lyricsOn = ctx.store.get('lyrics', true);
  ctx.tabs.register({ id: 'music', ...createTab(core) });
  ctx.cubes.register({ id: 'music', stat: () => (m ? `${m.playing ? '' : 'Paused · '}${m.title || 'Unknown title'}` : 'Nothing playing'), sheet: createSheet(core) });
  ctx.setup.card(() => ({ state: m ? 'Playing now' : 'Automatic' }));
  ctx.on('state', setState);
  ctx.on('tracker', () => { settings = { ...settings, tracker: true }; changed('settings'); });
  ctx.island.on('skin', () => changed('tint'));
  ctx.island.on('level', v => { for (const f of levelSubs) f(v); });
  ctx.settings.mount(root => {
    root.innerHTML = '<label class="switch"><input type="checkbox"><span>Pause the music when I step away (with an eye tracker)</span></label>';
    const box = root.querySelector('input');
    box.checked = !!settings.pause_when_away;
    box.addEventListener('change', () => setPause(box.checked).catch(() => { box.checked = !box.checked; }));
  });
  ctx.call('settings').then(s => { settings = s; changed('settings'); }).catch(() => {});
  ctx.call('state').then(setState).catch(() => {});
}

export function deactivate() {
  clearTimeout(lyT);
  ctx.activity.clear('now');
  ctx.commands.set([]);
}
