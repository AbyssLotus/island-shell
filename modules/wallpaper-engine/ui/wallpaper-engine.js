// Wallpaper Engine's library as a view of the Wallpaper tab (picked in the tab's header beside the
// island's own scenes): search, filter by type, put one on, pause or mute it. The backend
// (src/main.rs) reads the library and makes the thumbnails; it all stays on this PC.

let ctx, root;
const st = { lib: null, at: 0, kind: '', q: '', shown: 0, paused: false, muted: false, hover: null, obs: null, qT: 0 };
const $ = s => root.querySelector(s), $$ = s => [...root.querySelectorAll(s)];
const PAGE = 48;
const KIND_ICON = { scene: 'layers', video: 'film', web: 'globe' };

function view() {
  const { icon } = ctx.ui;
  return `
  <div class="we-bar">
    <label class="search we-search">${icon('search')}<input id="q" type="search" placeholder="Search wallpapers" autocomplete="off" aria-label="Search wallpapers"></label>
    <div class="we-kinds" id="kinds" role="group" aria-label="Type">
      <button type="button" class="chip" data-kind="" aria-pressed="true">All</button>
      <button type="button" class="chip" data-kind="scene" aria-pressed="false">Scenes</button>
      <button type="button" class="chip" data-kind="video" aria-pressed="false">Videos</button>
      <button type="button" class="chip" data-kind="web" aria-pressed="false">Web</button>
    </div>
    <span class="grow"></span>
    <button class="ib" type="button" id="play" aria-label="Pause the wallpaper" title="Pause the wallpaper"></button>
    <button class="ib" type="button" id="mute" aria-label="Mute the wallpaper" title="Mute the wallpaper"></button>
  </div>
  <div class="we-grid" id="grid" role="list"></div>
  <p class="empty" id="empty" hidden></p>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => ctx.ui.note($('#note'), msg);
const errText = e => (e && e.message) || String(e);
const thumbUrl = w => ctx.cacheUrl(w.thumb);

/** Shows the library, re-read at most once a minute. */
function show() {
  renderBar();
  if (st.lib && Date.now() - st.at < 60000) { renderGrid(); return; }
  ctx.call('library').then(lib => {
    st.lib = lib; st.at = Date.now();
    renderBar(); renderGrid();
  }).catch(err => note(errText(err)));
}

function renderBar() {
  const { setHtml, icon } = ctx.ui;
  setHtml($('#play'), icon(st.paused ? 'play' : 'pause'));
  $('#play').title = $('#play').ariaLabel = st.paused ? 'Resume the wallpaper' : 'Pause the wallpaper';
  setHtml($('#mute'), icon(st.muted ? 'mute' : 'speaker'));
  $('#mute').title = $('#mute').ariaLabel = st.muted ? 'Unmute the wallpaper' : 'Mute the wallpaper';
  if (st.lib) $('#q').placeholder = st.lib.installed ? `Search ${st.lib.wallpapers.length} wallpapers` : 'Search wallpapers';
}

/** Current wallpaper first, then recently used, then newest; filtered by type and search. */
function list() {
  const lib = st.lib, q = st.q.trim().toLowerCase();
  const rank = w => w.dir === lib.current ? 0 : lib.recent.includes(w.dir) ? 1 + lib.recent.indexOf(w.dir) : 1000;
  return lib.wallpapers
    .filter(w => (!st.kind || w.kind === st.kind) && (!q || w.title.toLowerCase().includes(q) || w.tags.some(t => t.toLowerCase().includes(q))))
    .sort((a, b) => rank(a) - rank(b) || b.added - a.added);
}

function tile(w) {
  const { esc, icon, grad } = ctx.ui;
  const cur = w.dir === st.lib.current, mature = w.rating === 'Mature';
  const img = w.thumb ? `<img src="${esc(thumbUrl(w))}" alt="" loading="lazy" decoding="async">` : '';
  return `<button type="button" role="listitem" class="we-tile${cur ? ' on' : ''}${mature ? ' mature' : ''}" data-dir="${esc(w.dir)}" title="${esc(w.title)}${w.tags.length ? ' · ' + esc(w.tags.join(', ')) : ''}"${w.thumb ? '' : ` style="background:${grad(w.title)}"`}>${img}${cur ? '<span class="we-badge">On</span>' : mature ? '<span class="we-badge r18">18+</span>' : ''}${KIND_ICON[w.kind] ? `<span class="we-kind">${icon(KIND_ICON[w.kind])}</span>` : ''}<span class="we-t">${esc(w.title)}</span></button>`;
}

/** Builds the grid a page at a time as you scroll, so a 600-wallpaper library stays light. */
function renderGrid(more) {
  const grid = $('#grid'), empty = $('#empty');
  if (!st.lib) return;
  if (!st.lib.installed) {
    grid.innerHTML = ''; empty.hidden = false;
    empty.textContent = 'Wallpaper Engine isn’t installed. It’s on Steam; once it’s there, its wallpapers show up here.';
    ctx.island.layout();
    return;
  }
  const all = list();
  if (!more) { st.shown = 0; clear(); grid.scrollTop = 0; }
  const next = all.slice(st.shown, st.shown + PAGE);
  grid.querySelector('.we-more')?.remove();
  grid.insertAdjacentHTML('beforeend', next.map(tile).join('') + (st.shown + next.length < all.length ? '<i class="we-more"></i>' : ''));
  st.shown += next.length;
  const sentinel = grid.querySelector('.we-more');
  if (sentinel) { st.obs?.disconnect(); st.obs = new IntersectionObserver(es => { if (es.some(e => e.isIntersecting)) renderGrid(true); }, { root: grid, rootMargin: '200px' }); st.obs.observe(sentinel); }
  empty.hidden = all.length > 0; empty.textContent = st.q ? `Nothing matches “${st.q}”.` : 'No wallpapers of that type.';
  if (!more) ctx.island.layout();
}
function clear() { st.obs?.disconnect(); st.obs = null; st.shown = 0; $('#grid').innerHTML = ''; }
const wallOf = el => st.lib && st.lib.wallpapers.find(w => w.dir === el.dataset.dir);

// Hovering plays that one wallpaper's real (often animated) preview; only one at a time.
let hoverSeq = 0;
function hover(t) {
  unhover(); const w = wallOf(t), img = t.querySelector('img');
  if (!w || !w.animated || !img) return;
  st.hover = t; const seq = ++hoverSeq;
  (w.previewUrl ? Promise.resolve(w.previewUrl) : ctx.call('preview', { dir: w.dir }).then(u => (w.previewUrl = u)))
    .then(u => { if (u && st.hover === t && seq === hoverSeq) img.src = u; }).catch(() => {});
}
function unhover() {
  const t = st.hover; st.hover = null; hoverSeq++; if (!t) return;
  const w = wallOf(t), img = t.querySelector('img'); if (w && img && w.thumb) img.src = thumbUrl(w);
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  const grid = $('#grid');
  grid.addEventListener('pointerover', e => { const t = e.target.closest('.we-tile'); if (t && t !== st.hover) hover(t); });
  grid.addEventListener('pointerleave', unhover);
  grid.addEventListener('click', e => {
    const t = e.target.closest('.we-tile'); const w = t && wallOf(t); if (!w || t.classList.contains('busy')) return;
    note(''); t.classList.add('busy');
    ctx.call('apply', { dir: w.dir }).then(() => {
      st.lib.current = w.dir; st.lib.recent = [w.dir, ...st.lib.recent.filter(d => d !== w.dir)].slice(0, 10);
      st.paused = false; renderBar();
      $$('.we-tile.on').forEach(x => { x.classList.remove('on'); x.querySelector('.we-badge:not(.r18)')?.remove(); });
      t.classList.add('on'); if (!t.querySelector('.we-badge')) t.insertAdjacentHTML('beforeend', '<span class="we-badge">On</span>');
      if (w.thumb) ctx.island.accentFrom(thumbUrl(w));
    }).catch(err => note(errText(err))).finally(() => t.classList.remove('busy'));
  });
  $('#kinds').addEventListener('click', e => {
    const b = e.target.closest('[data-kind]'); if (!b) return;
    st.kind = b.dataset.kind; $$('#kinds .chip').forEach(c => c.setAttribute('aria-pressed', c === b)); renderGrid();
  });
  $('#q').addEventListener('input', e => { st.q = e.target.value; clearTimeout(st.qT); st.qT = setTimeout(() => renderGrid(), 120); });
  $('#play').addEventListener('click', () => {
    st.paused = !st.paused; renderBar();
    ctx.call('control', { cmd: st.paused ? 'pause' : 'play' }).catch(err => { st.paused = !st.paused; renderBar(); note(errText(err)); });
  });
  $('#mute').addEventListener('click', () => {
    st.muted = !st.muted; renderBar();
    ctx.call('control', { cmd: st.muted ? 'mute' : 'unmute' }).catch(err => { st.muted = !st.muted; renderBar(); note(errText(err)); });
  });
}

export function activate(c) {
  ctx = c;
  // Thumbnails arrive one by one the first time the library is opened.
  ctx.on('thumb', ({ dir, thumb }) => {
    const w = st.lib && st.lib.wallpapers.find(x => x.dir === dir); if (!w || !root) return;
    w.thumb = thumb;
    const t = root.querySelector(`.we-tile[data-dir="${CSS.escape(w.dir)}"]`);
    if (t && !t.querySelector('img')) { t.style.background = ''; t.insertAdjacentHTML('afterbegin', `<img src="${ctx.ui.esc(thumbUrl(w))}" alt="" decoding="async">`); }
  });
  ctx.sections.register({ id: 'library', mount, show, hide: () => { unhover(); clear(); } });
}

export function deactivate() {
  if (root) clear();
}
