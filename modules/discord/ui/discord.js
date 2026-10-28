// Discord, as a fireteam: your call on the island's Discord tab (ui/tab.js) and in the Command
// Center's Discord sheet (ui/sheet.js), both drawn from `core` here; the call on the pill and the
// top bar (who's in it, a ring on whoever's talking), quick commands, the cube's line and the setup
// guide's card. The backend (src/main.rs) runs while Discord does; it sends the whole `status` when
// it changes, `speaking` when someone starts or stops talking, and `browse` (your servers and their
// channels) after the sheet asks. @mentions it shows by itself.

import { createTab } from './tab.js';
import { createSheet } from './sheet.js';

let ctx, dc = null, servers = null, err = '', order = [];
const subs = new Set();
const errText = e => (e && e.message) || String(e);
const DC_CDN = /^https:\/\/(cdn\.discordapp\.com|media\.discordapp\.net)\//;
const avatarOf = m => (m && DC_CDN.test(m.avatar || '') ? m.avatar : '');

/** Tells the tab and the sheet: 'status', 'speaking', 'browse' or 'error'. */
function changed(what) {
  for (const f of subs) try { f(what); } catch (e) { console.error(e); }
}

/** Keeps the order people show in, adding newcomers at the end. */
function keepOrder(v) {
  const ids = v.members.map(m => m.id);
  order = order.filter(id => ids.includes(id));
  for (const id of ids) if (!order.includes(id)) order.push(id);
}

/** Up to three people for the pill and the bar: the order sticks, except someone out of sight who starts talking moves up. */
function visible(v) {
  keepOrder(v);
  return order.slice(0, 3).map(id => v.members.find(m => m.id === id));
}

/** Up to `n` cards for the fireteam: you first, then everyone else in the same order. */
function roster(v, n) {
  keepOrder(v);
  const me = dc && dc.user ? dc.user.id : '';
  const you = v.members.find(m => m.id === me);
  const rest = order.filter(id => id !== me).map(id => v.members.find(m => m.id === id));
  return (you ? [you, ...rest] : rest).slice(0, n);
}

/** The call on the pill and the top bar, while you're in one with somebody there. */
function renderCall() {
  const d = dc, v = d && d.voice;
  if (!(v && v.members.length)) { ctx.activity.clear('call'); ctx.chips.remove('call'); return; }
  const avatars = visible(v).map(m => ({ id: m.id, src: avatarOf(m), talk: !!m.speaking && !m.muted }));
  const self = d.deaf ? 'headoff' : d.mute ? 'micoff' : '';
  const where = `${v.name}${v.guild ? ' · ' + v.guild : ''}`;
  ctx.activity.set({ id: 'call', kind: 'call', text: v.name, sub: v.guild || 'Discord', avatars, icon: self || undefined, action: 'tab:discord' });
  ctx.chips.set({ id: 'call', side: 'right', order: 5, tone: 'call-chip', avatars, icon: self || '', title: `${where} (Discord)`, opens: 'tab:discord' });
}

function renderCommands() {
  const d = dc || {}, list = [];
  if (d.authorized) {
    list.push({ id: 'mute', title: d.mute ? 'Unmute in Discord' : 'Mute in Discord', icon: d.mute ? 'mic' : 'micoff', sub: 'Discord · your mic', keys: 'discord mic voice', run: () => core.mute() });
    list.push({ id: 'deaf', title: d.deaf ? 'Undeafen in Discord' : 'Deafen in Discord', icon: 'headphones', sub: 'Discord · your sound', keys: 'discord deafen', run: () => core.deafen() });
    if (!d.voice && d.last) list.push({ id: 'rejoin', title: `Rejoin ${d.last.name}`, icon: 'headphones', sub: `Discord · ${d.last.guild || 'your last call'}`, keys: 'discord rejoin join voice call', run: () => core.rejoin() });
  }
  if (d.voice) list.push({ id: 'leave', title: 'Leave the Discord call', icon: 'phoneoff', sub: `Discord · ${d.voice.name}`, keys: 'discord hang up disconnect voice', run: () => core.leave() });
  ctx.commands.set(list);
}

function setStatus(s) {
  const prev = dc, was = prev && prev.voice;
  dc = s || null;
  const now = dc && dc.voice, { clip } = ctx.ui;
  if (prev && now && (!was || was.channel_id !== now.channel_id)) ctx.notify({ title: clip(now.name, 40), body: 'Joined voice', icon: 'headphones', sound: 'voice' });
  else if (prev && was && !now && dc && dc.connected) ctx.notify({ title: clip(was.name, 40), body: 'Left voice', icon: 'phoneoff', sound: 'voice' });
  if (core.autoAuth && dc && dc.connected && !dc.authorized && dc.has_secret && !dc.error) { core.autoAuth = false; core.action('authorize'); }
  renderCall(); renderCommands();
  changed('status');
}

function setError(msg) {
  if (msg === err) return;
  err = msg;
  changed('error');
}

/** A backend call; a failure shows on the tab and the sheet (and resolves to false) instead of throwing. */
function call(method, params) {
  setError('');
  return ctx.call(method, params).catch(e => { setError(errText(e)); return false; });
}

/** Volume sliders send at most one change per 70 ms, the latest. */
const later = new Map();
function soon(key, fn) {
  const t = later.get(key);
  if (t) { t.fn = fn; return; }
  const entry = { fn };
  later.set(key, entry);
  setTimeout(() => { later.delete(key); entry.fn(); }, 70);
}

/** What the tab and the sheet share. */
const core = {
  get ctx() { return ctx; },
  autoAuth: false,
  status: () => dc,
  servers: () => servers,
  error: () => err,
  me: () => (dc && dc.user ? dc.user.id : ''),
  avatarOf, roster,
  on(fn) { subs.add(fn); return () => subs.delete(fn); },
  load: () => ctx.call('status').then(setStatus).catch(() => {}),
  action: a => call('action', { action: a }),
  mute() { if (!dc) return; dc.mute = !dc.mute; renderCall(); changed('status'); return core.action(dc.mute ? 'mute' : 'unmute'); },
  deafen() { if (!dc) return; dc.deaf = !dc.deaf; renderCall(); changed('status'); return core.action(dc.deaf ? 'deafen' : 'undeafen'); },
  leave: () => core.action('leave'),
  join: id => call('join', { channel_id: id }),
  rejoin: () => (dc && dc.last ? core.join(dc.last.channel_id) : Promise.resolve()),
  /** Someone's volume (0–200) or mute, for you. */
  userVoice(id, o) {
    const m = dc && dc.voice && dc.voice.members.find(x => x.id === id);
    if (m) Object.assign(m, 'volume' in o ? { volume: o.volume } : {}, 'mute' in o ? { muted: o.mute } : {});
    soon('user:' + id, () => call('user_voice', { id, ...o }));
  },
  /** Discord's voice settings: { input, output, ptt, input_volume, output_volume }. */
  voice(o) {
    soon('voice:' + Object.keys(o).join(), () => call('voice', o));
  },
  /** That place in the Discord app: { guild, channel, message } or { user }; resolves to whether it opened. */
  open: target => call('open', target).then(r => r !== false),
  openApp: () => call('open_app').then(r => r !== false),
  options: o => call('options', o),
  pingsSeen() { if (dc && dc.pings.some(p => p.new)) call('pings', { seen: true }); },
  clearPings: () => call('pings', { clear: true }),
  /** Your servers (and `guild`'s channels; with `all`, everyone's): what's known now, the rest as `browse` events. */
  browse(o = {}) {
    if (!(dc && dc.authorized)) return Promise.resolve(servers);
    return ctx.call('browse', o).then(b => { if (b) { servers = b; changed('browse'); } return servers; }).catch(() => servers);
  },
  configure: (client_id, secret) => call('configure', { client_id, secret }),
  openWeb: url => ctx.openWeb(url).catch(e => setError(errText(e)))
};

export function activate(c) {
  ctx = c;
  // The tab asks the backend for the status (starting it if Discord isn't open, to set it up).
  ctx.tabs.register({ id: 'discord', ...createTab(core) });
  ctx.cubes.register({
    id: 'discord',
    stat() {
      const v = dc && dc.voice;
      if (!v) return !(dc && dc.connected) ? 'Not connected' : dc.authorized ? 'Not in voice' : 'Connected';
      const talking = v.members.find(m => m.speaking && !m.muted && m.id !== core.me());
      return talking ? `${talking.name} talking · ${v.name}` : `${v.members.length} in ${v.name}`;
    },
    sheet: createSheet(core)
  });
  ctx.setup.card(() => ({ state: dc && dc.authorized ? 'Connected' : '' }));
  ctx.on('status', setStatus);
  ctx.on('browse', b => { servers = b; changed('browse'); });
  ctx.on('speaking', ({ id, speaking }) => {
    const v = dc && dc.voice; if (!v) return;
    const m = v.members.find(x => x.id === id); if (!m) return;
    m.speaking = speaking;
    // Someone out of sight who starts talking comes into view (on the pill, the tab and the sheet).
    const i = order.indexOf(id), moved = speaking && i >= 3;
    if (moved) { order.splice(i, 1); order.unshift(id); }
    renderCall();
    changed(moved ? 'status' : 'speaking');
  });
  // The backend comes and goes with Discord.
  ctx.backend.on(st => {
    if (st === 'running') core.load();
    else if (st !== 'starting' && dc && (dc.connected || dc.voice)) { servers = null; setStatus({ ...dc, connected: false, authorized: false, voice: null }); }
  });
  if (ctx.backend.running()) core.load();
}

export function deactivate() {
  ctx.activity.clear('call');
  ctx.chips.remove('call');
  ctx.commands.set([]);
}
