// The Command Center (Win + Space): Island Shell's full-screen mode for going anywhere on the PC.
// A black eclipse leaking plasma and Taken light, with the clock and search inside it; the island's
// sections float around it as cubes, and each opens as a full-size glass sheet: the island's own view
// of it, or the real website docked in (Discord's web app, or a module's site like your Canvas or DIM). It grows out of the pill:
// while it's open the island's window covers the whole monitor (cc_mode) and stays on top
// (island_rect with urgent).
//
// Some sections are their own apps here instead of the island's view: Today is a full calendar
// (cc-calendar.js) and Apps the Start menu (cc-apps.js); modules bring their own (Games: the
// Steam library with each game's hero banner; Music: a full-size player with synced lyrics). The Stats cube opens
// the Task Manager (cc-taskmgr.js), a window over the eclipse.
//
// Search covers Start apps, open windows, drives and folders, recently opened files, what modules add (Steam games),
// Windows settings and quick commands. What you open from here is remembered (localStorage) and
// ranked higher next time, since Windows' own recent-items list may be switched off.

import { createCalendar } from './cc-calendar.js';
import { createTaskManager } from './cc-taskmgr.js';
import { createApps } from './cc-apps.js';
import { createSetup } from './setup.js';
import { placeCubes } from './registry.js';

const TAU = Math.PI * 2;
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

/* Icons the island doesn't have (Lucide-style paths). */
const OWN = {
  file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M8 13h8M8 17h5"/>',
  bolt: '<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>',
  window: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18"/>',
  ext: '<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  terminal: '<path d="m4 17 6-6-6-6M12 19h8"/>',
  calc: '<rect x="4" y="2" width="16" height="20" rx="2"/><path d="M8 6h8M8 11h.01M12 11h.01M16 11h.01M8 15h.01M12 15h.01M16 15h.01M8 19h8"/>',
  minimize: '<path d="M8 3v3a2 2 0 0 1-2 2H3M21 8h-3a2 2 0 0 1-2-2V3M3 16h3a2 2 0 0 1 2 2v3M16 21v-3a2 2 0 0 1 2-2h3"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  brush: '<path d="M9.06 11.9 17 4a2.83 2.83 0 1 1 4 4l-7.9 7.94"/><path d="M7.07 14.94c-1.66 0-3 1.35-3 3.02 0 1.33-2.5 1.52-2 2.02 1.08 1.1 2.49 2.02 4 2.02 2.2 0 4-1.8 4-4.04a3.01 3.01 0 0 0-3-3.02z"/>',
  battery: '<rect x="2" y="7" width="16" height="10" rx="2"/><path d="M22 11v2M6 10v4M10 10v4"/>'
};

const SETTINGS = [
  ['Display', 'monitor', 'ms-settings:display', 'screen resolution scale hdr refresh'], ['Sound', 'speaker', 'ms-settings:sound', 'audio output input volume'],
  ['Bluetooth & devices', 'bt', 'ms-settings:bluetooth', 'pair mouse keyboard headphones'], ['Network & internet', 'wifi', 'ms-settings:network', 'wifi ethernet vpn proxy'],
  ['Personalization', 'brush', 'ms-settings:personalization', 'background theme colors lock screen'], ['Apps & features', 'grid', 'ms-settings:appsfeatures', 'uninstall programs'],
  ['Default apps', 'grid', 'ms-settings:defaultapps', 'browser open with'], ['Storage', 'drive', 'ms-settings:storagesense', 'disk space cleanup'],
  ['Power & battery', 'battery', 'ms-settings:powersleep', 'sleep screen timeout power plan'], ['Windows Update', 'restart', 'ms-settings:windowsupdate', 'updates patch'],
  ['Privacy & security', 'shield', 'ms-settings:privacy', 'permissions camera microphone defender'], ['Night light', 'moon', 'ms-settings:nightlight', 'blue light warm'],
  ['Mouse', 'gamepad', 'ms-settings:mousetouchpad', 'pointer speed cursor'], ['Keyboard & typing', 'text', 'ms-settings:typing', 'language input'],
  ['Accounts', 'lock', 'ms-settings:yourinfo', 'sign in password'], ['Date & time', 'clock', 'ms-settings:dateandtime', 'time zone clock'],
  ['Notifications', 'bellon', 'ms-settings:notifications', 'focus do not disturb'], ['Multitasking', 'layers', 'ms-settings:multitasking', 'snap virtual desktops alt tab'],
  ['Startup apps', 'rocket', 'ms-settings:startupapps', 'run at login boot'], ['About this PC', 'cpu', 'ms-settings:about', 'specs system info rename']
];

/* Cubes come from the registry (src/registry.js) and sit in its named slots: [x, y] from the
   eclipse's center in radii, and a size. A cube opens its island tab in a sheet, its own app there
   (registry sheets), or a real website docked in (registry sites). */
const CSIZE = { L: [.9, .9], M: [.74, .74], S: [.52, .52], W: [1.14, .72] };

const KIND = { app: 'App', window: 'Window', game: 'Game', folder: 'Folder', drive: 'Drive', file: 'File', setting: 'Settings', command: 'Command', calc: 'Calculator' };
const GROUP = [['app', 'Apps'], ['window', 'Open windows'], ['game', 'Games'], ['folder', 'Folders & drives'], ['file', 'Files'], ['setting', 'Windows settings'], ['command', 'Commands']];
const SCOPES = [['all', 'All', 'search'], ['app', 'Apps', 'grid'], ['window', 'Windows', 'window'], ['game', 'Games', 'gamepad'], ['folder', 'Folders', 'folder'], ['file', 'Files', 'file'], ['setting', 'Settings', 'gear'], ['command', 'Commands', 'bolt']];

/** Arithmetic without eval: + - * / % ^ and parentheses. */
function calc(src) {
  const s = src.replace(/\s+/g, '').replace(/×/g, '*').replace(/÷/g, '/'); let i = 0;
  const num = () => { const m = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(s.slice(i)); if (!m) throw 0; i += m[0].length; return parseFloat(m[0]); };
  const atom = () => { if (s[i] === '(') { i++; const v = expr(); if (s[i++] !== ')') throw 0; return v; } if (s[i] === '-') { i++; return -atom(); } if (s[i] === '+') { i++; return atom(); } return num(); };
  const factor = () => { const b = atom(); if (s[i] === '^') { i++; return Math.pow(b, factor()); } return b; };
  const term = () => { let v = factor(); while (s[i] && '*/%'.includes(s[i])) { const op = s[i++], r = factor(); v = op === '*' ? v * r : op === '/' ? v / r : v % r; } return v; };
  const expr = () => { let v = term(); while (s[i] === '+' || s[i] === '-') { const op = s[i++], r = term(); v = op === '+' ? v + r : v - r; } return v; };
  const v = expr(); if (i !== s.length || !isFinite(v)) throw 0; return v;
}

/** How well a query matches an item: prefix > word start > substring > in-order letters. */
function score(q, name, keys) {
  const t = (name + ' ' + (keys || '')).toLowerCase();
  if (t.startsWith(q)) return 100 - name.length * .05;
  const w = t.split(/[\s\\/_\-.:·()#&]+/).findIndex(x => x.startsWith(q));
  if (w >= 0) return 80 - w;
  const i = t.indexOf(q);
  if (i >= 0) return 60 - i * .2;
  let j = 0, last = -1, gaps = 0;
  for (let k = 0; k < t.length && j < q.length; k++) if (t[k] === q[j]) { if (last >= 0) gaps += k - last - 1; last = k; j++; }
  return j === q.length && gaps < q.length * 3 ? 30 - gaps : -1;
}

export function createCommandCenter(ctx) {
  const { S, invoke, assetUrl, esc, store, gradFor, glyphOf } = ctx;
  const REG = ctx.R;
  const ic = (n, cls = 'i') => OWN[n] ? `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${OWN[n]}</svg>` : ctx.icon(n);
  const $ = s => root.querySelector(s), $$ = s => [...root.querySelectorAll(s)];

  /* ---------- DOM ---------- */
  const root = document.createElement('div');
  root.id = 'cc'; root.hidden = true; root.setAttribute('role', 'dialog'); root.setAttribute('aria-label', 'Command Center');
  root.innerHTML = `<canvas class="cc-canvas" aria-hidden="true"></canvas>
    <header class="cc-hdr"><span class="cc-brand"><span class="cc-dot" aria-hidden="true"><span class="eq"><i></i><i></i><i></i><i></i></span></span><b>Command Center</b></span>
      <button class="cc-ib" type="button" data-cc="close" title="Back to the island (Esc)" aria-label="Back to the island">${ic('minimize')}</button></header>
    <div class="cc-core">
      <div class="cc-time" aria-hidden="true"></div><div class="cc-date"></div>
      <label class="cc-search">${ic('search')}<input class="cc-q" type="text" autocomplete="off" spellcheck="false" placeholder="Go anywhere" aria-label="Search apps, windows, games, folders, files, settings and commands"></label>
      <div class="cc-hint"><kbd>&gt;</kbd>commands <kbd>=</kbd>math <kbd>Esc</kbd>island</div>
    </div>
    <nav class="cc-cubes" aria-label="Island sections"></nav>
    <section class="cc-results" hidden aria-label="Search results"><div class="cc-rwrap"><div class="cc-scopes"></div><div class="cc-rlist" role="listbox"></div></div><aside class="cc-detail"></aside></section>
    <section class="cc-sheet" hidden aria-label="Section">
      <header class="cc-sh"><button class="cc-ib" type="button" data-cc="back" title="Back to the eclipse (Esc)" aria-label="Back to the eclipse">${ic('x')}</button><nav class="cc-tabs" aria-label="Sections"></nav><div class="cc-modes" role="group" aria-label="Show"></div><button class="cc-ib cc-skin" type="button" data-cc="skin" hidden title="Island skin" aria-label="Island skin">${ic('brush')}</button></header>
      <div class="cc-sbody"><div class="cc-slot" hidden><span></span></div><div class="cc-host"></div></div>
    </section>
    <div class="cc-toast" role="status" aria-live="polite"></div>`;
  document.body.append(root);
  const cv = $('.cc-canvas'), g2 = cv.getContext('2d'), input = $('.cc-q');

  /* ---------- state ---------- */
  let isOpen = false, busy = false, wins = [], places = [], index = [], flat = [], sel = 0, scope = 'all', sheetTab = null, sheetMode = 'island', embedT = 0;
  let hist = store.get('ccHist', []), nextEv = null, nativeOn = null;
  const cal = createCalendar({ invoke, esc, ic, store, S, flash });
  /** Sections that are their own app in a sheet (mounted there instead of the island's view). */
  const aps = createApps({ S, invoke, esc, ic, assetUrl, store, flash, close: after => close(after, true), gradFor: ctx.gradFor, glyphOf: ctx.glyphOf, apps: ctx.apps });
  // The base's own cubes, sheets and docked sites; modules register theirs (src/modhost.js).
  [['apps', 'Apps', 'grid'], ['audio', 'Audio', 'speaker'], ['today', 'Today', 'calendar'],
    ['network', 'Network', 'wifi'], ['clipboard', 'Clipboard', 'clip'], ['stats', 'Stats', 'pulse']]
    .forEach(([id, title, icon]) => REG.cubes.add({ id, title, icon, stat: () => cubeStat(id) }));
  for (const [id, app] of Object.entries({ apps: aps, today: cal })) REG.sheets.add({ id, ...app, mount: app.mount, unmount: app.unmount, key: app.key });
  /** The cubes as placed in slots right now, in slot (keyboard) order. */
  const cubes = () => placeCubes(REG.cubes.all());
  const siteOf = tab => { const c = REG.cubes.get(tab); return c && c.site ? REG.sites.get(c.site) || null : null; };
  const NATIVE = new Proxy({}, { get: (_, k) => REG.sheets.get(k), has: (_, k) => REG.sheets.has(k) });
  REG.cubes.on(() => { if (isOpen) renderCubes(); });
  // A module's search results changed (its data arrived): index them now.
  REG.search.on(() => { if (isOpen) { buildIndex(); if (input.value.trim()) search(); } });
  const tm = createTaskManager({ invoke, esc, ic, store, flash, assetUrl, root });
  const setup = createSetup({ root, invoke, store, ic, esc, S, setDark: ctx.setDark, openTab: ctx.openTab, moduleCards: ctx.moduleCards || (() => []) });
  const EC = { x: 0, y: 0, r: 120 }, ET = { x: 0, y: 0, r: 120 };
  let R = 120, CX = 0, CY = 0, DPR = 1, running = false, lastT = 0, hoverA = null, hoverK = 0, reachA = 0;

  /* ---------- open / close: grows out of the pill ---------- */
  function pillRect() {
    const isl = document.getElementById('island');
    const r = isl && !isl.classList.contains('bar-hidden') ? isl.getBoundingClientRect() : null;
    return r && r.width ? r : { left: innerWidth / 2 - 104, top: 8, right: innerWidth / 2 + 104, bottom: 46, width: 208, height: 38 };
  }
  function clipTo(r) { root.style.setProperty('--cc-from', `inset(${r.top}px ${innerWidth - r.right}px ${innerHeight - r.bottom}px ${r.left}px round 19px)`); }
  const frames = n => new Promise(res => { const f = () => (n-- > 0 ? requestAnimationFrame(f) : res()); f(); });
  async function open() {
    if (isOpen || busy) return;
    busy = true; isOpen = true; S.ccOpen = true;
    if (S.mode === 'panel') ctx.closePanel(true);
    const w0 = innerWidth, h0 = innerHeight;
    try { await invoke('cc_mode', { on: true }); } catch (e) {}
    // Wait for the window to take the whole monitor before growing into it.
    for (let i = 0; i < 20 && innerWidth === w0 && innerHeight === h0; i++) await frames(1);
    await frames(1);
    resize();
    clipTo(pillRect());
    sheetTab = null; $('.cc-sheet').hidden = true; root.classList.remove('sheet-open');
    root.hidden = false; root.classList.remove('open'); void root.offsetWidth;
    document.body.classList.add('cc-on');
    root.classList.add('open');
    ctx.invoke('island_rect', { x: 0, y: 0, w: innerWidth, h: innerHeight, expanded: true, raised: true, bar: 0, urgent: true }).catch(() => {});
    Object.assign(EC, ET);
    input.value = ''; showResults(false);
    if (!running) { running = true; lastT = 0; requestAnimationFrame(frame); }
    tick(); renderCubes(); refresh();
    setTimeout(() => input.focus({ preventScroll: true }), 260);
    busy = false;
  }
  /** after: what to do once it's closed (e.g. open an island tab); otherwise focus goes back. */
  function close(after, keepFocus) {
    if (!isOpen || busy) return;
    if (sheetTab) closeSheet(true);
    tm.close(true);
    busy = true;
    clipTo(pillRect());
    root.classList.remove('open');
    setTimeout(async () => {
      root.hidden = true; running = false; isOpen = false; S.ccOpen = false; sleepCanvas();
      document.body.classList.remove('cc-on');
      try { await invoke('cc_mode', { on: false }); } catch (e) {}
      await frames(2);
      ctx.relayout();
      busy = false;
      if (after) after(); else if (!keepFocus) invoke('release_focus').catch(() => {});
    }, 480);
  }
  const toggle = () => (isOpen ? close() : open());

  /* ---------- data ---------- */
  async function refresh() {
    const [w, p] = await Promise.allSettled([invoke('windows_list'), invoke('cc_places')]);
    if (w.status === 'fulfilled') wins = w.value;
    if (p.status === 'fulfilled') places = p.value;
    buildIndex();
    loadNext();
    if (input.value.trim()) search();
  }
  /** The next thing on the calendar, for the Today cube. */
  async function loadNext() {
    const now = Date.now();
    try { const ev = await invoke('calendar_events', { from: now, to: now + 7 * 864e5 }); nextEv = ev.find(e => e.start >= now - 5 * 60e3 && !e.all_day) || ev.find(e => e.start >= now) || null; renderCubeStats(); } catch (e) {}
  }
  const exeName = p => (p || '').split('\\').pop();
  function appForWin(w) { return (S.apps || []).find(a => (a.exe && a.exe === w.exe) || (w.aumid && a.id === w.aumid)); }
  function commands() {
    const st = S.status || {}, list = [];
    // stay: keep the Command Center open afterwards ('quiet': and say nothing).
    const add = (name, icn, sub, keys, run, stay) => list.push({ kind: 'command', name, icon: icn, sub, keys, run, stay });
    if (st.wifi && st.wifi.present) add(st.wifi.on ? 'Turn Wi-Fi off' : 'Turn Wi-Fi on', 'wifi', 'Wireless', 'wifi wireless internet', () => invoke('set_radio', { kind: 'wifi', on: !st.wifi.on }));
    if (st.bluetooth && st.bluetooth.present) add(st.bluetooth.on ? 'Turn Bluetooth off' : 'Turn Bluetooth on', 'bt', 'Wireless', 'bluetooth', () => invoke('set_radio', { kind: 'bluetooth', on: !st.bluetooth.on }));
    add(st.taskbar_hidden ? 'Show the taskbar' : 'Hide the taskbar', 'taskbar', 'Island Shell', 'taskbar windows', () => invoke('set_taskbar', { hidden: !st.taskbar_hidden }));
    add('New virtual desktop', 'layers', 'Desktops', 'desktop workspace', () => invoke('desktop_new'));
    add('Task Manager', 'pulse', 'Processes, performance, startup apps', 'processes taskmgr end task kill startup performance memory', () => openTM(), 'quiet');
    add('Setup guide', 'rocket', 'Island Shell · the first-run tour', 'setup guide welcome tour help start onboarding how', () => openSetup(), 'quiet');
    add('Open Windows Task Manager', 'pulse', 'Windows · Ctrl + Shift + Esc', 'processes taskmgr', () => invoke('cc_tool', { name: 'taskmgr' }));
    add('Open Terminal', 'terminal', 'Windows Terminal', 'cmd powershell shell console', () => invoke('cc_tool', { name: 'terminal' }));
    add('Open File Explorer', 'folder', 'Windows', 'files explorer', () => invoke('cc_tool', { name: 'explorer' }));
    add('Open the Recycle Bin', 'trash', 'Windows', 'trash deleted', () => invoke('cc_tool', { name: 'recycle' }));
    add('Take a screenshot', 'image', 'Snipping Tool · region', 'snip capture screen', () => invoke('cc_tool', { name: 'snip' }));
    add('Open Control Panel', 'gear', 'Windows', 'control panel classic', () => invoke('cc_tool', { name: 'control' }));
    add('Lock this PC', 'lock', 'Power · Win + L', 'lock screen away', () => invoke('power', { action: 'lock' }));
    // Modules' own commands (src/modhost.js), and their search sources' items.
    for (const c of REG.commands.all()) list.push({ kind: 'command', name: c.title, icon: c.icon || 'bolt', sub: c.sub || '', keys: c.keys || '', run: c.run, stay: c.stay });
    return list;
  }
  function buildIndex() {
    const items = [];
    for (const a of S.apps || []) items.push({ kind: 'app', id: 'app:' + a.id, name: a.name, sub: a.exe || 'Start menu', img: a.icon, app: a });
    for (const w of wins) { const a = appForWin(w); items.push({ kind: 'window', id: 'win:' + w.hwnd, name: w.title, sub: `${a ? a.name : exeName(w.exe)}${w.minimized ? ' · minimized' : ''}`, keys: a ? a.name : exeName(w.exe), img: (a && a.icon) || w.icon, win: w }); }
    for (const p of places) {
      const kind = p.kind === 'file' ? 'file' : 'folder';
      const sub = p.kind === 'drive' ? `${p.sub}${p.free_gb != null ? ` · ${Math.round(p.free_gb)} GB free of ${Math.round(p.total_gb)}` : ''}` : p.kind === 'file' ? p.sub : p.path;
      items.push({ kind, id: 'path:' + p.path.toLowerCase(), name: p.kind === 'drive' ? `${p.name} ${p.sub}` : p.name, sub, keys: p.path, icon: p.kind === 'drive' ? 'drive' : kind === 'file' ? 'file' : 'folder', place: p });
    }
    for (const [name, icn, uri, keys] of SETTINGS) items.push({ kind: 'setting', id: 'set:' + uri, name, sub: uri, keys, icon: icn, uri });
    for (const c of commands()) items.push({ ...c, id: 'cmd:' + c.name });
    for (const src of REG.search.all()) {
      try { for (const it of src.items() || []) items.push({ kind: src.kind || 'command', icon: src.icon, ...it, id: `${src.id}:${it.id || it.name}`, module: src.module }); } catch (e) {}
    }
    // Files and folders opened from here before, even when Windows no longer lists them.
    for (const h of hist) if (h.path && !items.some(i => i.id === h.id)) items.push({ kind: h.kind, id: h.id, name: h.name, sub: h.sub, keys: h.path, icon: h.kind === 'file' ? 'file' : 'folder', place: { path: h.path, kind: h.kind } });
    index = items;
  }
  const agoText = ms => { const d = Math.floor((Date.now() - ms) / 864e5); return d <= 0 ? 'today' : d === 1 ? 'yesterday' : d < 30 ? `${d} days ago` : d < 365 ? `${Math.round(d / 30)} months ago` : 'over a year ago'; };
  /** Things opened often and lately rank higher. */
  function boost(id) { const h = hist.find(x => x.id === id); if (!h) return 0; const days = (Date.now() - h.t) / 864e5; return Math.min(36, h.n * 7) * Math.exp(-days / 21); }
  function remember(it) {
    if (it.kind === 'calc' || it.kind === 'web') return;
    let h = hist.find(x => x.id === it.id);
    if (!h) { h = { id: it.id, n: 0, kind: it.kind, name: it.name, sub: it.sub, path: it.place ? it.place.path : '' }; hist.unshift(h); }
    h.n++; h.t = Date.now(); hist = hist.sort((a, b) => b.t - a.t).slice(0, 200); store.set('ccHist', hist);
  }

  /* ---------- search ---------- */
  function showResults(on) {
    $('.cc-results').hidden = !on; root.classList.toggle('searching', on); layoutTargets();
  }
  function hl(name, q) { const i = name.toLowerCase().indexOf(q); return i < 0 || !q ? esc(name) : esc(name.slice(0, i)) + '<mark>' + esc(name.slice(i, i + q.length)) + '</mark>' + esc(name.slice(i + q.length)); }
  function itemIcon(it, big) {
    const cls = big ? 'cc-ic big' : 'cc-ic';
    if (it.img) return `<span class="${cls} img"><img src="${esc(assetUrl(it.img))}" alt=""></span>`;
    if (it.cover) return `<span class="${cls} cover"><img src="${esc(assetUrl(it.cover))}" alt=""></span>`;
    if (it.app) return `<span class="${cls} glyph" style="background:${gradFor(it.name)}">${esc(glyphOf(it.name))}</span>`;
    return `<span class="${cls}">${ic(it.kind === 'calc' ? 'calc' : it.kind === 'web' ? 'globe' : it.icon || 'file')}</span>`;
  }
  function search() {
    const raw = input.value, q = raw.trim().toLowerCase();
    showResults(!!q);
    if (!q) return;
    $('.cc-scopes').innerHTML = SCOPES.filter(([k]) => k === 'all' || k === scope || k !== 'game' || index.some(it => it.kind === k)).map(([k, n, icn]) => `<button class="cc-chip" type="button" data-scope="${k}" aria-pressed="${k === scope}">${ic(icn)}${n}</button>`).join('');
    flat = [];
    const groups = [];
    const mathSrc = q.startsWith('=') ? q.slice(1) : /^[\d\s.+\-*/%^()×÷]+$/.test(q) && /[+\-*/%^×÷]/.test(q) ? q : null;
    if (mathSrc) { try { const v = calc(mathSrc); groups.push(['Calculator', [{ kind: 'calc', name: Number.isInteger(v) ? v.toLocaleString() : (+v.toPrecision(12)).toLocaleString(undefined, { maximumFractionDigits: 10 }), sub: mathSrc.trim() + ' =', value: String(v) }]]); } catch (e) {} }
    const cmdOnly = q.startsWith('>'), qq = cmdOnly ? q.slice(1).trim() : (mathSrc ? '' : q);
    const pool = index.filter(it => (scope === 'all' || it.kind === scope) && (!cmdOnly || it.kind === 'command'));
    const hits = qq ? pool.map(it => [score(qq, it.name, it.keys), it]).filter(x => x[0] >= 0).map(x => [x[0] + boost(x[1].id), x[1]]).sort((a, b) => b[0] - a[0]).map(x => x[1]) : (mathSrc ? [] : pool.slice().sort((a, b) => boost(b.id) - boost(a.id)));
    const top = hits.length && qq && scope === 'all' && !cmdOnly ? hits[0] : null;
    if (top) groups.push(['Top hit', [top]]);
    const per = scope === 'all' && !cmdOnly ? 5 : 60;
    for (const [k, label] of GROUP) { const g = hits.filter(h => h.kind === k && h !== top).slice(0, per); if (g.length) groups.push([label, g]); }
    if (qq && !cmdOnly && !mathSrc) groups.push(['Web', [{ kind: 'web', name: `Search the web for “${raw.trim()}”`, sub: 'Default browser', query: raw.trim() }]]);
    let html = '';
    for (const [label, items] of groups) {
      html += `<div class="cc-rgroup">${esc(label)}</div>`;
      for (const it of items) { const n = flat.push(it) - 1; html += `<button class="cc-res${n === sel ? ' sel' : ''}" type="button" role="option" data-n="${n}" aria-selected="${n === sel}">${itemIcon(it)}<span class="cc-rt"><b>${it.kind === 'web' ? esc(it.name) : hl(it.name, qq)}</b><small>${esc(it.sub || '')}</small></span><span class="cc-kind">${esc(KIND[it.kind] || 'Web')}</span></button>`; }
    }
    $('.cc-rlist').innerHTML = html;
    if (sel >= flat.length) sel = Math.max(0, flat.length - 1);
    renderDetail();
  }
  function actionsFor(it) {
    if (it.actions) return it.actions.map(a => [a.label, a.icon || 'bolt', a.run, a.stay]);
    switch (it.kind) {
      case 'app': return [['Open', 'ext', () => invoke('app_launch', { id: it.app.id })], ...(it.app.exe ? [['Show in folder', 'folder', () => invoke('cc_open', { path: it.app.exe, reveal: true })]] : [])];
      case 'window': return [['Switch to', 'window', () => invoke('window_activate', { hwnd: it.win.hwnd })], ['Close the window', 'x', () => invoke('window_close', { hwnd: it.win.hwnd }), true]];
      case 'folder': return [['Open', 'folder', () => invoke('cc_open', { path: it.place.path })], ['Copy path', 'copy', () => invoke('copy_text', { text: it.place.path }), true]];
      case 'file': return [['Open', 'ext', () => invoke('cc_open', { path: it.place.path })], ['Show in folder', 'folder', () => invoke('cc_open', { path: it.place.path, reveal: true })], ['Copy path', 'copy', () => invoke('copy_text', { text: it.place.path }), true]];
      case 'setting': return [['Open Settings', 'gear', () => invoke('open_settings', { uri: it.uri })]];
      case 'command': return [['Run', 'bolt', it.run, it.stay]];
      case 'calc': return [['Copy result', 'copy', () => invoke('copy_text', { text: it.value }), true]];
      case 'web': return [['Search', 'globe', () => invoke('cc_web', { query: it.query })]];
      default: return [];
    }
  }
  function renderDetail() {
    const it = flat[sel], d = $('.cc-detail');
    if (!it) { d.innerHTML = `<p class="cc-empty">Nothing on this PC matches.</p>`; return; }
    const acts = actionsFor(it);
    d.innerHTML = `<div class="cc-dh">${itemIcon(it, true)}<div class="cc-dt"><h3>${it.kind === 'calc' ? '' : esc(it.name)}</h3><span>${esc(KIND[it.kind] || 'Web')}</span></div></div>`
      + (it.kind === 'calc' ? `<div class="cc-calc">${esc(it.name)}</div>` : '') + (it.sub ? `<div class="cc-path">${esc(it.sub)}</div>` : '')
      + `<div class="cc-acts">${acts.map(([label, icn], i) => `<button class="cc-act" type="button" data-act="${i}">${ic(icn)}<span>${esc(label)}</span>${i === 0 ? '<kbd>↵</kbd>' : i === 1 ? '<kbd>Ctrl ↵</kbd>' : ''}</button>`).join('')}</div>`;
  }
  /** Runs an item's action; most leave the Command Center (the thing opened takes over). */
  async function run(it, a = 0) {
    if (!it) return;
    const act = actionsFor(it)[a]; if (!act) return;
    const [label, , fn, stay] = act;
    try {
      await fn();
      remember(it);
      if (!stay) close(null, true);
      else if (stay !== 'quiet') { flash(label === 'Copy path' || label === 'Copy result' ? 'Copied' : `${label}: done`); if (it.kind === 'window') { wins = wins.filter(w => w !== it.win); buildIndex(); search(); } }
    } catch (e) { flash(String(e)); }
  }
  let flashT;
  function flash(msg) { const t = $('.cc-toast'); t.textContent = msg; t.classList.add('on'); clearTimeout(flashT); flashT = setTimeout(() => t.classList.remove('on'), 2200); }
  function setSel(n) {
    sel = clamp(n, 0, Math.max(0, flat.length - 1));
    $$('.cc-res').forEach(x => { const on = +x.dataset.n === sel; x.classList.toggle('sel', on); x.setAttribute('aria-selected', on); if (on) x.scrollIntoView({ block: 'nearest' }); });
    renderDetail();
  }

  /* ---------- cubes ---------- */
  function cubeStat(k) {
    const st = S.status || {};
    switch (k) {
      case 'apps': return `${(S.apps || []).length} apps · ${wins.length} open`;
      case 'audio': return S.audio ? `${S.audio.device} · ${S.audio.muted ? 'muted' : Math.round(S.audio.level * 100) + '%'}` : 'Sound';
      case 'today': { if (nextEv) { const d = new Date(nextEv.start), sameDay = d.toDateString() === new Date().toDateString(); return `${nextEv.all_day ? (sameDay ? 'Today' : d.toLocaleDateString([], { weekday: 'short' })) : (sameDay ? '' : d.toLocaleDateString([], { weekday: 'short' }) + ' ') + d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })} · ${nextEv.title}`; } const w = store.get('weather', null); return w ? `${Math.round(S.unit === 'C' ? w.temp : w.temp * 9 / 5 + 32)}° · ${w.name || ''}` : 'Weather & to-dos'; }
      case 'network': return st.net ? (st.net.name || (st.net.kind === 'ethernet' ? 'Ethernet' : 'Offline')) : 'Network';
      case 'clipboard': return `${(S.clips || []).length} items`;
      case 'stats': return `CPU ${Math.round(st.cpu || 0)}%${st.cpu_temp != null ? ` · ${Math.round(st.cpu_temp)}°` : ''}`;
      default: return '';
    }
  }
  const cubeIcon = n => (/\.svg$/.test(n || '') ? `<img class="i" src="${esc(n)}" alt="">` : ic(n));
  const statOf = id => { const c = REG.cubes.get(id); try { return c && c.stat ? String(c.stat() || '') : ''; } catch (e) { return ''; } };
  function renderCubes() {
    $('.cc-cubes').innerHTML = cubes().map(({ cube: c, slot }, i) => `<div class="cc-cw ${slot.x < 0 ? 'l' : slot.x > 0 ? 'r' : 'c'} sz-${slot.size}" data-cube="${esc(c.id)}" style="--dl:${(-i * 1.37).toFixed(2)}s;--fd:${(5.2 + (i % 4) * .95).toFixed(2)}s"><div class="cc-fl"><button class="cc-cube" type="button" aria-label="${esc(c.title)}"><span class="cc-f f-back"></span><span class="cc-f f-left"></span><span class="cc-f f-right"></span><span class="cc-f f-top"></span><span class="cc-f f-bottom"></span><span class="cc-f f-front">${cubeIcon(c.icon)}<b>${esc(c.title)}</b><small data-stat="${esc(c.id)}">${esc(statOf(c.id))}</small></span></button></div></div>`).join('');
    layout();
    // A module's cube stat comes from its UI, so the cubes on screen load theirs.
    for (const m of new Set(REG.cubes.all().map(c => c.module).filter(Boolean))) ctx.activateModule(m).then(renderCubeStats, () => {});
  }
  function renderCubeStats() { $$('[data-stat]').forEach(el => { const v = statOf(el.dataset.stat); if (el.textContent !== v) el.textContent = v; }); }

  /* ---------- layout ---------- */
  // The eclipse is all soft light, so its canvas is drawn at 3/4 of the screen's resolution and
  // stretched: a full-screen 2560×1440 canvas is ~15 MB a buffer and WebView2 keeps several.
  const CANVAS_Q = .75;
  function resize() {
    DPR = Math.min(devicePixelRatio || 1, 1.5) * CANVAS_Q;
    cv.width = Math.round(innerWidth * DPR); cv.height = Math.round(innerHeight * DPR);
    layout();
  }
  /** Lets go of the canvas's pixels while nothing shows it (a sheet covers it, or it's closed). */
  function sleepCanvas() { cv.width = cv.height = 1; }
  function wakeCanvas() { if (isOpen && !running) { resize(); running = true; lastT = 0; requestAnimationFrame(frame); } }
  function layout() {
    const W = innerWidth, H = innerHeight;
    R = clamp(Math.min(W / 9.8, H / 4.4), 84, 210); CX = W / 2; CY = H / 2 + 4;
    const at = new Map(cubes().map(p => [p.cube.id, p.slot]));
    $$('.cc-cw').forEach(el => {
      const s = at.get(el.dataset.cube); if (!s) return;
      const [sw, sh] = CSIZE[s.size], pw = sw * R, ph = sh * R;
      el.style.setProperty('--w', pw + 'px'); el.style.setProperty('--h', ph + 'px'); el.style.setProperty('--d', (Math.min(pw, ph) * .62) + 'px');
      el.style.left = (CX + s.x * R - pw / 2) + 'px'; el.style.top = (CY + s.y * R - ph / 2) + 'px';
    });
    layoutTargets();
  }
  function layoutTargets() {
    const W = innerWidth, H = innerHeight, res = $('.cc-results');
    if (root.classList.contains('searching')) {
      Object.assign(ET, { x: Math.max(R * 1.4 + 20, W * .24), y: H / 2, r: R });
      Object.assign(res.style, { left: (ET.x + R * 1.5) + 'px', right: '32px', top: '76px', bottom: '40px' });
    } else Object.assign(ET, { x: CX, y: CY, r: R });
  }
  addEventListener('resize', () => { if (isOpen) resize(); });

  /* ---------- the eclipse (canvas) ---------- */
  const RAYS = Array.from({ length: 150 }, (_, i) => ({ a: i / 150 * TAU + Math.random() * .03, ph: Math.random() * TAU, sp: .5 + Math.random() * 1.7, w: .5 + Math.random() * 1.7 }));
  const PROM = [{ a: 3.9, w: .11, h: .55, life: 1.4, dur: 5 }, { a: .5, w: .08, h: .35, life: .8, dur: 4 }, { a: 2.1, w: .14, h: .42, life: 2.2, dur: 6 }], MOTES = [];
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const noise = (a, t) => .48 + .22 * Math.sin(3 * a + t * .45) + .16 * Math.sin(7 * a - t * 1.05 + 1.7) + .1 * Math.sin(13 * a + t * 1.8 + .4) + .07 * Math.sin(29 * a - t * 2.6);
  const angDiff = (a, b) => ((a - b) % TAU + TAU * 1.5) % TAU - Math.PI; // signed, -π..π
  const angDist = (a, b) => Math.abs(angDiff(a, b));
  let coreKey = '';
  function placeCore() {
    const key = `${EC.x.toFixed(1)},${EC.y.toFixed(1)},${EC.r.toFixed(1)}`; if (key === coreKey) return; coreKey = key;
    const c = $('.cc-core');
    c.style.left = (EC.x - EC.r) + 'px'; c.style.top = (EC.y - EC.r) + 'px'; c.style.width = c.style.height = (EC.r * 2) + 'px';
    c.style.setProperty('--r', EC.r.toFixed(1));
  }
  function frame(now) {
    if (!running) return;
    if (sheetTab) { running = false; sleepCanvas(); return; } // under a sheet: nothing to see
    const t = now / 1000, dt = Math.min(.05, lastT ? t - lastT : .016); lastT = t;
    for (const k of ['x', 'y', 'r']) EC[k] += (ET[k] - EC[k]) * .1;
    // The corona's reach: drawn at reachA, which follows the hovered cube and stays where it was
    // while the reach fades after the pointer leaves (hoverA goes null at once).
    if (hoverA != null) {
      if (hoverK < .02) reachA = hoverA; // a fresh reach starts at its cube
      else reachA += angDiff(hoverA, reachA) * .14; // moving to another cube swings round the short way
    }
    hoverK += ((hoverA != null ? 1 : 0) - hoverK) * .08;
    placeCore();
    const W = innerWidth, H = innerHeight, { x, y, r } = EC, ctx2 = g2;
    ctx2.setTransform(DPR, 0, 0, DPR, 0, 0); ctx2.clearRect(0, 0, W, H);
    ctx2.globalCompositeOperation = 'lighter'; ctx2.globalAlpha = 1;
    let g = ctx2.createRadialGradient(x, y, r * .95, x, y, r * 3.4);
    g.addColorStop(0, 'rgba(180,220,255,.34)'); g.addColorStop(.25, 'rgba(120,150,255,.12)'); g.addColorStop(.6, 'rgba(90,70,200,.04)'); g.addColorStop(1, 'rgba(0,0,0,0)');
    ctx2.fillStyle = g; ctx2.fillRect(x - r * 3.5, y - r * 3.5, r * 7, r * 7);
    for (const ray of RAYS) {
      let f = noise(ray.a, t) + .2 * Math.sin(t * ray.sp + ray.ph);
      if (hoverK > .01) { const d = angDist(ray.a, reachA); f += hoverK * 1.15 * Math.exp(-d * d / .07); }
      f = Math.max(0, f);
      const len = r * (.08 + .95 * f * f), bend = .16 * Math.sin(t * .6 + ray.ph);
      const x0 = x + Math.cos(ray.a) * r * .99, y0 = y + Math.sin(ray.a) * r * .99;
      const a1 = ray.a + bend * (len / r), x1 = x + Math.cos(a1) * (r + len), y1 = y + Math.sin(a1) * (r + len);
      const lg = ctx2.createLinearGradient(x0, y0, x1, y1), k = Math.min(1, f);
      lg.addColorStop(0, `rgba(242,251,255,${.85 * k})`); lg.addColorStop(.3, `rgba(170,220,255,${.38 * k})`); lg.addColorStop(1, 'rgba(110,140,255,0)');
      ctx2.strokeStyle = lg; ctx2.lineWidth = ray.w * 1.5 * Math.max(.7, r / 170);
      ctx2.beginPath(); ctx2.moveTo(x0, y0);
      ctx2.quadraticCurveTo(x + Math.cos(ray.a + bend * .5) * (r + len * .55), y + Math.sin(ray.a + bend * .5) * (r + len * .55), x1, y1);
      ctx2.stroke();
    }
    if (!still && PROM.length < 5 && Math.random() < .03) PROM.push({ a: Math.random() * TAU, w: .07 + Math.random() * .13, h: .22 + Math.random() * .5, life: 0, dur: 3 + Math.random() * 4 });
    for (let i = PROM.length - 1; i >= 0; i--) {
      const p = PROM[i]; p.life += dt; const ph = p.life / p.dur; if (ph >= 1) { PROM.splice(i, 1); continue; }
      const al = Math.sin(ph * Math.PI), hh = r * p.h * (.55 + .45 * Math.sin(ph * Math.PI * .8));
      const pt = (a, rr) => [x + Math.cos(a) * rr, y + Math.sin(a) * rr];
      const [sx, sy] = pt(p.a - p.w, r * .99), [ex, ey] = pt(p.a + p.w, r * .99), [c1x, c1y] = pt(p.a - p.w * .9, r + hh * 1.35), [c2x, c2y] = pt(p.a + p.w * .7, r + hh * 1.25);
      for (const [lw, col] of [[12, `rgba(255,140,60,${.12 * al})`], [5, `rgba(255,186,110,${.36 * al})`], [1.6, `rgba(255,244,226,${.9 * al})`]]) {
        ctx2.lineWidth = lw * Math.max(.7, r / 170); ctx2.strokeStyle = col;
        ctx2.beginPath(); ctx2.moveTo(sx, sy); ctx2.bezierCurveTo(c1x, c1y, c2x, c2y, ex, ey); ctx2.stroke();
      }
      if (Math.random() < .25) MOTES.push({ x: c1x, y: c1y, vx: (Math.random() - .5) * 18, vy: -10 - Math.random() * 18, life: 0, dur: 1.5 + Math.random() * 2, s: .6 + Math.random() * 1.2, warm: 1 });
    }
    if (!still) for (let n = 0; n < 3; n++) if (MOTES.length < 300) {
      const a = Math.random() * TAU, rr = r * (1 + Math.random() * .12), sp = 8 + Math.random() * 34;
      MOTES.push({ x: x + Math.cos(a) * rr, y: y + Math.sin(a) * rr, vx: Math.cos(a) * sp - Math.sin(a) * 8, vy: Math.sin(a) * sp + Math.cos(a) * 8, life: 0, dur: 2 + Math.random() * 3.5, s: .5 + Math.random() * 1.8, dark: Math.random() < .3 });
    }
    const darks = [];
    for (let i = MOTES.length - 1; i >= 0; i--) {
      const m = MOTES[i]; m.life += dt; if (m.life > m.dur) { MOTES.splice(i, 1); continue; }
      m.vx += Math.sin(m.y * .012 + t * 1.3) * 14 * dt; m.vy += (Math.cos(m.x * .012 - t) * 10 - 7) * dt;
      m.x += m.vx * dt; m.y += m.vy * dt;
      const al = Math.sin(m.life / m.dur * Math.PI);
      if (m.dark) { darks.push([m, al]); continue; }
      ctx2.fillStyle = m.warm ? `rgba(255,190,120,${.16 * al})` : `rgba(150,205,255,${.13 * al})`;
      ctx2.beginPath(); ctx2.arc(m.x, m.y, m.s * 4, 0, TAU); ctx2.fill();
      ctx2.fillStyle = m.warm ? `rgba(255,236,210,${.9 * al})` : `rgba(240,250,255,${.85 * al})`;
      ctx2.beginPath(); ctx2.arc(m.x, m.y, m.s, 0, TAU); ctx2.fill();
    }
    ctx2.globalCompositeOperation = 'source-over';
    for (const [m, al] of darks) {
      ctx2.fillStyle = `rgba(0,0,0,${.9 * al})`; ctx2.beginPath(); ctx2.arc(m.x, m.y, m.s * 1.6, 0, TAU); ctx2.fill();
      ctx2.strokeStyle = `rgba(210,238,255,${.55 * al})`; ctx2.lineWidth = .8; ctx2.stroke();
    }
    ctx2.fillStyle = '#010103'; ctx2.beginPath(); ctx2.arc(x, y, r, 0, TAU); ctx2.fill();
    g = ctx2.createRadialGradient(x - r * .35, y - r * .4, r * .05, x, y, r);
    g.addColorStop(0, 'rgba(70,82,110,.16)'); g.addColorStop(1, 'rgba(0,0,0,0)');
    ctx2.fillStyle = g; ctx2.fill();
    ctx2.globalCompositeOperation = 'lighter';
    for (const [lw, al] of [[22, .06], [9, .18], [3, .6], [1.2, 1]]) {
      ctx2.lineWidth = lw * Math.max(.7, r / 170); ctx2.strokeStyle = `rgba(228,244,255,${al})`;
      ctx2.beginPath(); ctx2.arc(x, y, r + lw * .25, 0, TAU); ctx2.stroke();
    }
    for (const [k2, sz] of [[.13, 1], [-.08, .7]]) {
      const a = t * k2 + (k2 > 0 ? 2.2 : 4.4), bx = x + Math.cos(a) * r, by = y + Math.sin(a) * r;
      const bg = ctx2.createRadialGradient(bx, by, 0, bx, by, r * .16 * sz);
      bg.addColorStop(0, 'rgba(255,255,255,.95)'); bg.addColorStop(.25, 'rgba(200,232,255,.35)'); bg.addColorStop(1, 'rgba(0,0,0,0)');
      ctx2.fillStyle = bg; ctx2.fillRect(bx - r * .2, by - r * .2, r * .4, r * .4);
    }
    ctx2.globalCompositeOperation = 'source-over';
    if (still) { running = false; return; }
    requestAnimationFrame(frame);
  }

  /* ---------- clock ---------- */
  function tick() {
    if (!isOpen) return;
    const d = new Date(), h = d.getHours() % 12 || 12, mm = String(d.getMinutes()).padStart(2, '0'), ss = String(d.getSeconds()).padStart(2, '0');
    $('.cc-time').innerHTML = `${h}<span class="cc-colon">:</span>${mm}<small>${ss}</small>`;
    const w = store.get('weather', null);
    const wx = w ? ` · ${Math.round(S.unit === 'C' ? w.temp : w.temp * 9 / 5 + 32)}°${S.unit === 'C' ? 'C' : 'F'}` : '';
    $('.cc-date').textContent = `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} · ${d.getHours() < 12 ? 'AM' : 'PM'}${wx}`;
  }
  let statT = 0;
  setInterval(() => { if (isOpen) { tick(); if (statT++ % 3 === 0) renderCubeStats(); } }, 1000);

  /* ---------- input ---------- */
  input.addEventListener('input', () => { sel = 0; search(); });
  root.addEventListener('click', e => {
    if (tm.owns(e.target)) return;
    if (e.target.closest('[data-cc="close"]')) { close(); return; }
    const sc = e.target.closest('[data-scope]'); if (sc) { scope = sc.dataset.scope; sel = 0; search(); input.focus(); return; }
    const r = e.target.closest('.cc-res'); if (r) { sel = +r.dataset.n; run(flat[sel]); return; }
    const a = e.target.closest('[data-act]'); if (a) { run(flat[sel], +a.dataset.act); return; }
    const cw = e.target.closest('.cc-cw'); if (cw) { openSheet(cw.dataset.cube, cw); return; }
    if (e.target.closest('[data-cc="back"]')) { closeSheet(); return; }
    const st = e.target.closest('[data-sheet]'); if (st) { openSheet(st.dataset.sheet); return; }
    const md = e.target.closest('[data-mode]'); if (md) { setMode(md.dataset.mode); return; }
    if (e.target.closest('[data-cc="skin"]') && sheetTab && siteOf(sheetTab)) {
      const k = skinKey(sheetTab), on = !store.get(k, true), name = siteOf(sheetTab).label.replace(/ (app|website)$/, '');
      store.set(k, on);
      $('.cc-skin').setAttribute('aria-pressed', on); flash(on ? `${name} wears the island skin` : `${name} in its own colours`);
      placeEmbed(); return;
    }
    if (e.target.closest('.cc-sheet')) return;
    // A click on the empty void goes back to the search box.
    if (!e.target.closest('input, button, a')) input.focus({ preventScroll: true });
  });
  root.addEventListener('mousemove', e => { const r = e.target.closest('.cc-res'); if (r && +r.dataset.n !== sel) setSel(+r.dataset.n); });
  root.addEventListener('pointerover', e => {
    const w = e.target.closest('.cc-cw'); if (!w) return;
    const b = w.getBoundingClientRect(); hoverA = Math.atan2(b.top + b.height / 2 - EC.y, b.left + b.width / 2 - EC.x);
  });
  root.addEventListener('pointerout', e => { if (e.target.closest('.cc-cw') && !(e.relatedTarget && e.relatedTarget.closest && e.relatedTarget.closest('.cc-cw'))) hoverA = null; });
  // While it's open, keys belong to the Command Center (the island's own shortcuts wait).
  addEventListener('keydown', e => {
    if (!isOpen) return;
    // The setup guide has the keys while it shows (Enter, Space and Tab still work its buttons).
    if (setup.isOpen()) { if (setup.key(e)) e.preventDefault(); e.stopPropagation(); return; }
    if (tm.isOpen()) { const r = tm.key(e); if (r) { e.stopPropagation(); if (r === 'used') e.preventDefault(); return; } }
    if (sheetTab) {
      if (nativeOn && NATIVE[nativeOn].key(e)) { e.preventDefault(); e.stopPropagation(); return; }
      const typing = e.target.matches && e.target.matches('input, textarea, select') && e.target !== input;
      const order = cubes().map(p => p.cube.id), i = order.indexOf(sheetTab);
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (typing) e.target.blur(); else closeSheet(); return; }
      if (!typing && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) { e.preventDefault(); e.stopPropagation(); openSheet(order[(i + (e.key === 'ArrowRight' ? 1 : -1) + order.length) % order.length]); return; }
      if (!typing && /^[1-9]$/.test(e.key)) { e.stopPropagation(); if (order[+e.key - 1]) openSheet(order[+e.key - 1]); return; }
      return; // everything else belongs to the section
    }
    e.stopPropagation();
    const searching = !$('.cc-results').hidden;
    if (e.key === 'Escape') { e.preventDefault(); if (input.value) { input.value = ''; scope = 'all'; search(); } else close(); return; }
    if (searching && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) { e.preventDefault(); setSel(sel + (e.key === 'ArrowDown' ? 1 : -1)); return; }
    if (searching && e.key === 'Enter') { e.preventDefault(); run(flat[sel], e.ctrlKey && actionsFor(flat[sel] || {}).length > 1 ? 1 : 0); return; }
    if (searching && e.key === 'Tab') { e.preventDefault(); const i = SCOPES.findIndex(s => s[0] === scope) + (e.shiftKey ? -1 : 1); scope = SCOPES[(i + SCOPES.length) % SCOPES.length][0]; sel = 0; search(); return; }
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && document.activeElement !== input) input.focus();
  }, true);

  /* ---------- sheets: each section full size ---------- */
  function openSheet(tab, fromEl) {
    if (!isOpen || busy || !cubes().some(p => p.cube.id === tab)) return false;
    if (tab === 'stats') { openTM(); return true; }
    tm.close();
    const sheet = $('.cc-sheet'), first = !sheetTab;
    invoke('cc_embed_hide').catch(() => {}); clearTimeout(embedT);
    sheetTab = tab;
    $('.cc-tabs').innerHTML = cubes().map(({ cube: c }) => `<button class="cc-chip" type="button" data-sheet="${esc(c.id)}" aria-pressed="${c.id === tab}">${cubeIcon(c.icon)}${esc(c.title)}</button>`).join('');
    if (first) {
      const src = fromEl || $(`.cc-cw[data-cube="${tab}"]`);
      if (src) { const r = src.getBoundingClientRect(); sheet.style.setProperty('--ox', (r.left + r.width / 2 - 32) + 'px'); sheet.style.setProperty('--oy', (r.top + r.height / 2 - 72) + 'px'); }
      sheet.hidden = false; sheet.style.animation = 'none'; void sheet.offsetWidth; sheet.style.animation = '';
      root.classList.add('sheet-open'); input.blur();
    }
    const site = siteOf(tab);
    const want = site ? store.get('ccMode:' + tab, site.default ? 'web' : 'island') : 'island';
    $('.cc-modes').innerHTML = site ? `<button type="button" data-mode="island" aria-pressed="${want === 'island'}">Island view</button><button type="button" data-mode="web" aria-pressed="${want === 'web'}">${esc(site.label)}</button>` : '';
    // A module's cube may bring its own sheet: load the module first, then show it.
    const c = REG.cubes.get(tab);
    if (c && c.module && ctx.activateModule) ctx.activateModule(c.module).then(() => { if (sheetTab === tab) setMode(want, first ? 560 : 60); }, () => { if (sheetTab === tab) setMode(want, first ? 560 : 60); });
    else setMode(want, first ? 560 : 60);
    return true;
  }
  function setMode(mode, delay = 60) {
    const tab = sheetTab, site = siteOf(tab); if (!tab) return;
    if (!site) mode = 'island';
    // A site may need setting up in its island view first (Canvas: which school).
    if (site && mode === 'web' && site.ready && !site.ready()) { mode = 'island'; flash(site.why || `Set up ${site.label.replace(/ (app|website)$/, '')} in its island view first`); }
    sheetMode = mode;
    if (site) store.set('ccMode:' + tab, mode);
    $$('.cc-modes [data-mode]').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === mode));
    const body = $('.cc-sbody'), web = mode === 'web';
    // A `split` site keeps its island view beside it (Discord's call controls); the others take the whole sheet.
    const split = web && !!site.split;
    body.classList.toggle('web', web); body.classList.toggle('split', split);
    $('.cc-slot').hidden = !web; $('.cc-slot span').textContent = web ? `Opening ${site.label}…` : '';
    $('.cc-skin').hidden = !web; $('.cc-skin').setAttribute('aria-pressed', !!(site && store.get(skinKey(tab), true)));
    if (site) { const n = site.label.replace(/ (app|website)$/, ''); $('.cc-skin').title = $('.cc-skin').ariaLabel = `Island skin on ${n}`; }
    const app = NATIVE[tab];
    body.classList.toggle('native', !!app);
    if (nativeOn && nativeOn !== tab) unmountNative();
    if (app) { ctx.unhost(); if (nativeOn !== tab) { app.mount($('.cc-host')); nativeOn = tab; } }
    else if (!web || split) ctx.hostTab(tab, $('.cc-host')); else ctx.unhost();
    clearTimeout(embedT);
    if (web) embedT = setTimeout(placeEmbed, delay); else invoke('cc_embed_hide').catch(() => {});
  }
  /** Whether a docked site wears the island skin (Discord's switch keeps its original name). */
  const skinKey = tab => (tab === 'discord' ? 'ccSkin' : 'ccSkin:' + tab);
  function placeEmbed() {
    if (!isOpen || busy || !sheetTab || sheetMode !== 'web') return;
    const r = $('.cc-slot').getBoundingClientRect();
    if (r.width < 120 || r.height < 120) { embedT = setTimeout(placeEmbed, 150); return; }
    invoke('cc_embed', { site: siteOf(sheetTab).site, x: r.left, y: r.top, w: r.width, h: r.height, skin: store.get(skinKey(sheetTab), true) }).catch(e => { $('.cc-slot span').textContent = String(e); });
  }
  function unmountNative() {
    NATIVE[nativeOn].unmount();
    if (nativeOn === 'today') loadNext();
    nativeOn = null;
  }
  function closeSheet(quiet) {
    if (!sheetTab) return;
    clearTimeout(embedT); invoke('cc_embed_hide').catch(() => {});
    ctx.unhost();
    if (nativeOn) unmountNative();
    $('.cc-sbody').classList.remove('native');
    sheetTab = null;
    $('.cc-sheet').hidden = true; root.classList.remove('sheet-open');
    wakeCanvas();
    if (!quiet) { renderCubeStats(); setTimeout(() => input.focus({ preventScroll: true }), 60); }
  }
  /** The Task Manager window, over the eclipse (sheets and the search results step aside). */
  function openTM() {
    if (!isOpen || busy) return;
    if (sheetTab) closeSheet(true);
    if (input.value) { input.value = ''; scope = 'all'; showResults(false); }
    input.blur();
    tm.open();
  }
  /** The setup guide over the eclipse; afterwards it can open an island tab to set something up. */
  async function openSetup() {
    if (!isOpen) await open();
    if (sheetTab) closeSheet(true);
    tm.close();
    if (input.value) { input.value = ''; scope = 'all'; showResults(false); }
    input.blur();
    setup.open(tab => { if (tab) close(() => ctx.openTab(tab), true); else setTimeout(() => input.focus({ preventScroll: true }), 300); });
  }
  async function openTaskManager() {
    if (!isOpen) await open();
    openTM();
  }
  addEventListener('resize', () => { if (isOpen && sheetMode === 'web' && sheetTab) { clearTimeout(embedT); embedT = setTimeout(placeEmbed, 120); } });

  return { open, close, toggle, isOpen: () => isOpen, flash, openSheet, openTaskManager, openSetup };
}
