// Games: your Steam library on the island (a shelf of covers, most recently played first, with
// your other launchers under it), full size in the Command Center's Games sheet (ui/sheet.js), and
// in its search. Win + G opens the tab (the manifest's hotkey). The backend (src/main.rs) reads
// Steam's own files; covers and heroes come from its cache folder or Steam's art folders.

import { createSheet } from './sheet.js';

let ctx, root = null, lib = null, T = 0, releaseT = 0;
const $ = s => root.querySelector(s);
const showing = () => ctx.island.showing('games');
const errText = e => (e && e.message) || String(e);

// Other launchers, shown as app icons under the covers (matched against Start menu names).
const LAUNCHERS = [/^steam$/i, /^epic games launcher$/i, /^ea( app)?$/i, /^ubisoft connect$/i, /^riot client$/i, /^rockstar games launcher$/i, /^rsi launcher$/i,
  /^hytale launcher$/i, /^battle\.net$/i, /^gog galaxy/i, /^xbox$/i, /^minecraft launcher$/i, /^prism launcher$/i, /^modrinth app$/i, /^kyber launcher$/i, /^roblox player$/i];
/** The other game launchers that are installed (Start menu apps), in LAUNCHERS order. */
function launchers() {
  const apps = ctx.island.apps.list(), seen = new Set();
  return LAUNCHERS.map(re => apps.find(a => re.test(a.name.trim()) && !seen.has(a.id) && seen.add(a.id))).filter(Boolean);
}

/** A picture from the backend: a small copy in the cache ("cache:…") or a file in Steam's art folders. */
const art = p => (!p ? '' : p.startsWith('cache:') ? ctx.cacheUrl(p.slice(6)) : ctx.imageUrl(p));

async function load() {
  try { lib = await ctx.call('library'); note(''); } catch (e) { note(errText(e)); }
  // Search results follow the library (registering again tells the Command Center).
  registerSearch();
  render();
  return lib;
}
/** Refreshes every 6 s while the tab is open, so a game you start shows as playing. */
function loop() {
  clearTimeout(T); clearTimeout(releaseT);
  load().finally(() => { if (showing()) T = setTimeout(loop, 6000); });
}

const note = msg => { if (root) ctx.ui.note($('#note'), msg); };

function coverHtml(g, i, running) {
  const { esc, icon, grad } = ctx.ui;
  const img = g.cover || g.header;
  const pic = img ? `<img src="${esc(art(img))}" alt="" loading="${i < 8 ? 'eager' : 'lazy'}" draggable="false">` : `<span class="cv-name">${esc(g.name)}</span>`;
  const tag = running ? '<span class="cv-tag">Playing</span>' : g.needs_update ? '<span class="cv-tag up">Update</span>' : '';
  return `<button type="button" role="listitem" class="cover${i === 0 ? ' big' : ''}${running ? ' playing' : ''}${!g.cover && g.header ? ' wide' : ''}" data-appid="${g.appid}" title="${esc(g.name)}" aria-label="Play ${esc(g.name)}"${img ? '' : ` style="background:${grad(g.name)}"`}>${pic}${tag}<span class="cv-play">${icon('play')}</span></button>`;
}
function appTile(a) {
  const { esc, grad, glyph } = ctx.ui;
  const ico = a.icon ? `<span class="ico"><img src="${esc(a.icon)}" alt="" loading="lazy"></span>` : `<span class="glyph" style="background:${grad(a.name)}">${esc(glyph(a.name))}</span>`;
  return `<button class="app" type="button" data-id="${esc(a.id)}" title="${esc(a.name)}">${ico}<span class="nm">${esc(a.name)}</span>${a.running ? '<i class="dot" title="Running"></i>' : ''}</button>`;
}
function render() {
  if (!root || !lib) return;
  const { setHtml } = ctx.ui;
  const shelf = $('#shelf');
  // Covers (some animated) are only in the page while the tab shows (see hide()).
  if (!showing()) setHtml(shelf, '');
  else if (!lib.installed) setHtml(shelf, '<p class="empty">Steam isn’t installed.</p>');
  else if (!lib.games.length) setHtml(shelf, '<p class="empty">No Steam games installed yet.</p>');
  else {
    // A running game goes first, then most recently played.
    const games = [...lib.games].sort((a, b) => (b.appid === lib.running) - (a.appid === lib.running));
    setHtml(shelf, games.map((g, i) => coverHtml(g, i, g.appid === lib.running)).join(''));
  }
  setHtml($('#launchers'), launchers().map(appTile).join(''));
  if (showing()) ctx.island.layout();
}

function play(g) {
  ctx.island.closePanel(true);
  ctx.call('action', { appid: g.appid, what: 'play' })
    .then(() => setTimeout(() => ctx.notify({ title: g.name, body: 'Starting through Steam…', icon: 'gamepad', img: g.cover ? art(g.cover) : '', ms: 2600, sound: 'small' }), 260))
    .catch(e => setTimeout(() => ctx.notify({ title: `Couldn't start ${g.name}`, body: errText(e), icon: 'x', sound: 'problem' }), 260));
}
const gameOf = el => lib && lib.games.find(g => String(g.appid) === el.dataset.appid);

function mount(r) {
  root = r;
  root.innerHTML = `
    <div class="shelf" id="shelf" role="list" aria-label="Steam games, most recently played first"></div>
    <div class="g-rule" aria-hidden="true"></div>
    <div class="apps launchers" id="launchers" aria-label="Launchers"></div>
    <p class="note" id="note" hidden></p>`;
  root.addEventListener('click', e => {
    const c = e.target.closest('.cover[data-appid]');
    if (c) { const g = gameOf(c); if (g) play(g); return; }
    const tile = e.target.closest('.app[data-id]');
    if (tile) ctx.island.apps.open(tile.dataset.id);
  });
  root.addEventListener('contextmenu', e => {
    const c = e.target.closest('.cover[data-appid]'), tile = e.target.closest('.app[data-id]');
    if (!c && !tile) return;
    e.preventDefault();
    if (tile) { ctx.island.apps.menu(tile.dataset.id, e.clientX, e.clientY); return; }
    const g = gameOf(c); if (!g) return;
    const act = what => ctx.call('action', { appid: g.appid, what }).then(() => ctx.island.closePanel(true)).catch(err => note(errText(err)));
    ctx.island.menu(e.clientX, e.clientY, [
      { heading: g.name },
      { label: 'Play', icon: 'play', action: () => play(g) },
      { label: 'Show in Steam', icon: 'gamepad', action: () => act('library') },
      { label: 'Open install folder', icon: 'folder', action: () => act('folder') }
    ]);
  });
  // A cover that won't load falls back to the wide header art, then to a name card.
  $('#shelf').addEventListener('error', e => {
    const img = e.target; if (img.tagName !== 'IMG') return;
    const c = img.closest('.cover'), g = c && gameOf(c); if (!g) return;
    if (g.header && !img.dataset.fallback) { img.dataset.fallback = '1'; c.classList.add('wide'); img.src = art(g.header); }
    else { c.style.background = ctx.ui.grad(g.name); img.replaceWith(Object.assign(document.createElement('span'), { className: 'cv-name', textContent: g.name })); }
  }, true);
  // The mouse wheel scrolls the covers and launchers sideways (and doesn't switch tabs or close the panel).
  for (const el of [$('#shelf'), $('#launchers')]) el.addEventListener('wheel', e => {
    e.preventDefault(); e.stopPropagation();
    el.scrollLeft += (Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX) * 1.1;
  }, { passive: false });
}

/** Leaving the tab drops the covers a moment later, so their decoded images don't sit in memory. */
function hide() {
  clearTimeout(T); clearTimeout(releaseT);
  releaseT = setTimeout(() => { if (root && !showing()) ctx.ui.setHtml($('#shelf'), ''); }, 900);
}

const agoText = ms => { const d = Math.floor((Date.now() - ms) / 864e5); return d <= 0 ? 'today' : d === 1 ? 'yesterday' : d < 30 ? `${d} days ago` : d < 365 ? `${Math.round(d / 30)} months ago` : 'over a year ago'; };
function registerSearch() {
  ctx.search.register({
    id: 'steam', kind: 'game', label: 'Games', icon: 'gamepad',
    items: () => (lib ? lib.games : []).map(g => ({
      id: String(g.appid), name: g.name, sub: g.last_played ? `Steam · played ${agoText(g.last_played * 1000)}` : 'Steam', keys: 'steam game', cover: art(g.cover || g.header) || undefined,
      actions: [
        { label: 'Play', icon: 'play', run: () => ctx.call('action', { appid: g.appid, what: 'play' }) },
        { label: 'Show in Steam', icon: 'gamepad', run: () => ctx.call('action', { appid: g.appid, what: 'library' }) },
        { label: 'Open install folder', icon: 'folder', run: () => ctx.call('action', { appid: g.appid, what: 'folder' }) }
      ]
    }))
  });
}

export function activate(c) {
  ctx = c;
  // The sheet's filter and sort, kept from 0.1.x's island.gmFilter / island.gmSort the first time.
  for (const [old, now] of [['gmFilter', 'filter'], ['gmSort', 'sort']]) {
    try { const v = localStorage.getItem(`island.${old}`); if (v != null && localStorage.getItem(`mod.games.${now}`) == null) localStorage.setItem(`mod.games.${now}`, v); } catch (e) {}
  }
  ctx.tabs.register({ id: 'games', mount, show: loop, hide });
  ctx.cubes.register({
    id: 'games',
    stat: () => `${lib ? lib.games.length : 0} games`,
    sheet: createSheet({ ctx, art, library: () => lib, load, play, launchers })
  });
  registerSearch();
  // The library a little after start, so search, the cube and the tab have it at once.
  T = setTimeout(() => { if (!lib) load(); }, 7000);
}

export function deactivate() {
  clearTimeout(T); clearTimeout(releaseT);
}
