// The Command Center's Games sheet: your Steam library, full size. The chosen game fills the top
// with its hero banner and logo (your custom artwork first, placed the way Steam places it), with
// Play, how long you've played, when you last did and its size; the covers sit below with search,
// filters and sorting, and your other launchers at the end. Data from games.rs (games_list),
// refreshed every 6 seconds while the sheet shows so a game you start shows as playing.

const DAY = 864e5;
const hours = m => (m >= 600 ? Math.round(m / 60).toLocaleString() : (m / 60).toFixed(1)) + ' h';
const size = b => (b >= 1e12 ? (b / 1e12).toFixed(2) + ' TB' : (b / 1e9).toFixed(1) + ' GB');
function when(sec) {
  if (!sec) return 'Never';
  const d = new Date(sec * 1000), days = Math.floor((Date.now() - d) / DAY);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 14) return `${days} days ago`;
  return d.toLocaleDateString([], { month: 'short', year: d.getFullYear() === new Date().getFullYear() ? undefined : 'numeric', day: d.getFullYear() === new Date().getFullYear() ? 'numeric' : undefined });
}
const FILTERS = [['all', 'All'], ['recent', 'Played lately'], ['most', 'Most played'], ['update', 'Needs an update']];
const SORTS = [['recent', 'Last played'], ['name', 'Name'], ['played', 'Hours played'], ['size', 'Size']];
/** Steam's logo pins → where the logo box sits on the hero. */
const PIN = { BottomLeft: 'bl', UpperLeft: 'tl', CenterCenter: 'cc', UpperCenter: 'tc', BottomCenter: 'bc' };

export function createGames({ S, invoke, esc, ic, assetUrl, store, flash, close, games }) {
  let el = null, lib = null, sel = 0, q = '', filter = store.get('gmFilter', 'all'), sort = store.get('gmSort', 'recent'), timer = 0, heroFor = -1;
  const $ = s => el && el.querySelector(s), $$ = s => (el ? [...el.querySelectorAll(s)] : []);

  function build() {
    el.innerHTML = `<section class="gms-hero" aria-live="polite">
        <div class="gms-art"><img class="gms-img a" alt="" draggable="false"><img class="gms-img b" alt="" draggable="false"></div>
        <div class="gms-in">
          <div class="gms-logo"></div>
          <div class="gms-acts">
            <button class="gms-play" type="button" data-g="play"></button>
            <button class="gms-ib" type="button" data-g="library" title="Show in Steam" aria-label="Show in Steam">${ic('gamepad')}</button>
            <button class="gms-ib" type="button" data-g="folder" title="Open install folder" aria-label="Open install folder">${ic('folder')}</button>
            <button class="gms-ib" type="button" data-g="screenshots" title="Screenshots" aria-label="Screenshots">${ic('image')}</button>
            <button class="gms-ib" type="button" data-g="store" title="Store page" aria-label="Store page">${ic('ext')}</button>
          </div>
          <div class="gms-stats"></div>
        </div>
      </section>
      <section class="gms-lib">
        <div class="gms-bar"><h3>Library <small></small></h3>
          <div class="gms-chips" role="group" aria-label="Show">${FILTERS.map(([k, n]) => `<button type="button" data-f="${k}" aria-pressed="${k === filter}">${n}</button>`).join('')}</div>
          <span class="grow"></span>
          <label class="gms-sort">Sort<select aria-label="Sort by">${SORTS.map(([k, n]) => `<option value="${k}"${k === sort ? ' selected' : ''}>${n}</option>`).join('')}</select></label>
          <label class="gms-search">${ic('search')}<input type="text" placeholder="Find a game" spellcheck="false" autocomplete="off" aria-label="Find a game"></label>
        </div>
        <div class="gms-grid" role="listbox" aria-label="Games"></div>
        <div class="gms-more"><small>Other launchers</small><div class="gms-apps"></div></div>
      </section>`;
    const qi = $('.gms-search input');
    qi.value = q;
    qi.addEventListener('input', () => { q = qi.value; renderGrid(); });
    $('.gms-sort select').addEventListener('change', e => { sort = e.target.value; store.set('gmSort', sort); renderGrid(); });
  }

  async function load() {
    try { lib = await invoke('games_list'); } catch (e) { flash(String(e)); }
    if (!el) return;
    if (lib && lib.games.length && !lib.games.some(g => g.appid === sel)) sel = (lib.games.find(g => g.appid === lib.running) || lib.games[0]).appid;
    render();
    clearTimeout(timer); timer = setTimeout(load, 6000);
  }
  const game = id => lib && lib.games.find(g => g.appid === id);
  function list() {
    if (!lib) return [];
    const ql = q.trim().toLowerCase();
    let gs = lib.games.filter(g => !ql || g.name.toLowerCase().includes(ql));
    const lately = g => g.playtime_2wks > 0 || (g.last_played && Date.now() / 1000 - g.last_played < 14 * 86400);
    if (filter === 'recent') gs = gs.filter(lately);
    else if (filter === 'most') gs = gs.filter(g => g.playtime > 0);
    else if (filter === 'update') gs = gs.filter(g => g.needs_update);
    const by = { recent: (a, b) => b.last_played - a.last_played, name: (a, b) => a.name.localeCompare(b.name), played: (a, b) => b.playtime - a.playtime, size: (a, b) => b.size - a.size }[filter === 'most' ? 'played' : sort];
    return gs.slice().sort((a, b) => (b.appid === lib.running) - (a.appid === lib.running) || by(a, b) || a.name.localeCompare(b.name));
  }

  /* ---------- the hero ---------- */
  function renderHero() {
    const g = game(sel), hero = $('.gms-hero');
    hero.classList.toggle('empty', !g);
    if (!g) { $('.gms-logo').innerHTML = `<h2>${lib && !lib.installed ? 'Steam isn’t installed' : 'No Steam games installed yet'}</h2>`; $('.gms-acts').hidden = true; $('.gms-stats').innerHTML = ''; return; }
    $('.gms-acts').hidden = false;
    if (heroFor !== g.appid) {
      heroFor = g.appid;
      // Two layers cross-fade, so changing games never flashes black.
      const [a, b] = [$('.gms-img.a'), $('.gms-img.b')], front = a.classList.contains('on') ? a : b, back = front === a ? b : a;
      const src = g.hero || g.header || g.cover;
      // Once the new banner has faded in, the old one lets go of its picture (a hero is ~10 MB decoded).
      if (src) { back.onload = () => { back.classList.add('on'); front.classList.remove('on'); setTimeout(() => { if (!front.classList.contains('on')) front.removeAttribute('src'); }, 700); }; back.src = assetUrl(src); } else { a.classList.remove('on'); b.classList.remove('on'); }
      const box = $('.gms-logo'), pos = g.logo_pos;
      box.className = 'gms-logo ' + (pos ? PIN[pos.pin] || 'bl' : 'bl');
      box.style.setProperty('--lw', (pos ? pos.w : 34).toFixed(1) + '%');
      box.style.setProperty('--lh', (pos ? pos.h : 46).toFixed(1) + '%');
      box.innerHTML = g.logo ? `<img src="${esc(assetUrl(g.logo))}" alt="${esc(g.name)}" draggable="false">` : `<h2>${esc(g.name)}</h2>`;
      const img = box.querySelector('img');
      if (img) img.onerror = () => { box.innerHTML = `<h2>${esc(g.name)}</h2>`; };
      hero.style.animation = 'none'; void hero.offsetWidth; hero.style.animation = '';
    }
    const running = lib.running === g.appid;
    const play = $('[data-g="play"]');
    play.className = 'gms-play' + (running ? ' running' : g.needs_update ? ' update' : '');
    play.innerHTML = running ? '<i class="gms-live"></i>Playing' : `${ic('play')}${g.needs_update ? 'Update' : 'Play'}`;
    play.disabled = running;
    $('.gms-stats').innerHTML = `<div><small>Last played</small><b>${running ? 'Now' : when(g.last_played)}</b></div>`
      + `<div><small>Played</small><b>${g.playtime ? hours(g.playtime) : 'Not yet'}</b>${g.playtime_2wks ? `<em>${hours(g.playtime_2wks)} in the last 2 weeks</em>` : ''}</div>`
      + `<div><small>On disk</small><b>${g.size ? size(g.size) : '—'}</b></div>`;
  }

  /* ---------- the covers ---------- */
  function cover(g, i) {
    const img = g.cover || g.header, running = lib.running === g.appid;
    return `<button type="button" role="option" class="gms-cv${g.appid === sel ? ' sel' : ''}${running ? ' playing' : ''}${!g.cover && g.header ? ' wide' : ''}" data-appid="${g.appid}" aria-selected="${g.appid === sel}" title="${esc(g.name)}">`
      + (img ? `<img src="${esc(assetUrl(img))}" alt="" loading="${i < 32 ? 'eager' : 'lazy'}" draggable="false">` : `<span class="gms-name">${esc(g.name)}</span>`)
      + (running ? '<span class="gms-tag">Playing</span>' : g.needs_update ? '<span class="gms-tag up">Update</span>' : '') + '</button>';
  }
  function renderGrid() {
    const gs = list(), grid = $('.gms-grid');
    const all = lib ? lib.games : [];
    $('.gms-bar small').textContent = all.length ? `${all.length} games · ${size(all.reduce((a, g) => a + g.size, 0))}` : '';
    grid.innerHTML = gs.length ? gs.map(cover).join('') : `<p class="gms-none">${lib && all.length ? 'No games match.' : 'Reading your Steam library…'}</p>`;
    $$('.gms-chips [data-f]').forEach(b => b.setAttribute('aria-pressed', b.dataset.f === filter));
    $('.gms-sort').hidden = filter === 'most';
  }
  function renderApps() {
    const apps = games.launchers();
    $('.gms-more').hidden = !apps.length;
    $('.gms-apps').innerHTML = apps.map(a => `<button type="button" class="gms-app" data-app="${esc(a.id)}" title="${esc(a.name)}">${a.icon ? `<img src="${esc(assetUrl(a.icon))}" alt="" draggable="false">` : `<span>${esc(a.name.charAt(0))}</span>`}<small>${esc(a.name)}</small></button>`).join('');
  }
  function render() { if (!el) return; renderHero(); renderGrid(); renderApps(); }
  function select(id, scroll) {
    if (!game(id)) return;
    sel = id; renderHero();
    $$('.gms-cv').forEach(c => { const on = +c.dataset.appid === id; c.classList.toggle('sel', on); c.setAttribute('aria-selected', on); if (on && scroll) c.scrollIntoView({ block: 'nearest' }); });
  }

  /* ---------- input ---------- */
  function act(what) {
    const g = game(sel); if (!g) return;
    if (what === 'play') { close(() => games.play(g)); return; }
    invoke('game_action', { appid: g.appid, what }).then(() => close()).catch(e => flash(String(e)));
  }
  let lastPress = { id: 0, t: 0 };
  function onPointerDown(e) {
    const c = e.button === 0 && e.target.closest('.gms-cv'); if (!c) return;
    const id = +c.dataset.appid;
    if (lastPress.id === id && e.timeStamp - lastPress.t < 400) { lastPress = { id: 0, t: 0 }; select(id); act('play'); return; } // double-press plays
    lastPress = { id, t: e.timeStamp };
    select(id);
  }
  function onClick(e) {
    const b = e.target.closest('[data-g]'); if (b) { act(b.dataset.g); return; }
    const f = e.target.closest('[data-f]'); if (f) { filter = f.dataset.f; store.set('gmFilter', filter); renderGrid(); return; }
    const a = e.target.closest('[data-app]');
    if (a) { const app = (S.apps || []).find(x => x.id === a.dataset.app); if (app) close(() => games.openApp(app)); }
  }
  function onContext(e) {
    const c = e.target.closest('.gms-cv'); if (!c) return;
    e.preventDefault(); select(+c.dataset.appid);
  }
  /** Keys while the sheet shows; true when it used the key. */
  function key(e) {
    if (!el) return false;
    const typing = e.target.matches && e.target.matches('input, select');
    if (e.key === 'Escape' && typing && e.target.value) { e.target.value = ''; q = ''; renderGrid(); return true; }
    const cards = $$('.gms-cv');
    if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key) && cards.length && !(typing && (e.key === 'ArrowLeft' || e.key === 'ArrowRight'))) {
      const i = Math.max(0, cards.findIndex(c => +c.dataset.appid === sel));
      const top = cards[0].offsetTop, cols = Math.max(1, cards.filter(c => c.offsetTop === top).length);
      const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -cols, ArrowDown: cols }[e.key];
      const j = Math.max(0, Math.min(cards.length - 1, i + step));
      if (typing) e.target.blur();
      select(+cards[j].dataset.appid, true);
      return true;
    }
    if (e.key === 'Enter' && !typing) { act('play'); return true; }
    if (!typing && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey && !/^[1-9]$/.test(e.key)) { $('.gms-search input').focus(); return false; }
    return false;
  }

  return {
    mount(host) {
      el = document.createElement('div'); el.className = 'gms';
      build(); host.append(el);
      el.addEventListener('pointerdown', onPointerDown); el.addEventListener('click', onClick); el.addEventListener('contextmenu', onContext);
      heroFor = -1;
      if (S.games) { lib = S.games; if (!lib.games.some(g => g.appid === sel) && lib.games.length) sel = (lib.games.find(g => g.appid === lib.running) || lib.games[0]).appid; render(); }
      load();
    },
    unmount() { clearTimeout(timer); if (el) el.remove(); el = null; },
    key
  };
}
