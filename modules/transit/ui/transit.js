// Transit's island tab: the next departures from the stops the user saved, from their city's
// timetable and live feed through the module's backend (src/main.rs). Picking the city, adding
// stops, which routes to show and when to put the next departure on the pill all happen here.
//
// The UI loads at start (`eager`) for the pill: around the times a stop is set for (a commute),
// its next departure shows beside the clock as a `custom` activity, read again every minute.

let ctx, root = null, shown = false, T = 0, P = 0;
const st = { status: null, board: null, view: 'main', results: null, q: '', open: '', routes: {}, busy: '', error: '' };
const $ = s => root && root.querySelector(s);
const errText = e => (e && e.message) || String(e);
const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const MOBILITY_DB = 'https://mobilitydatabase.org/';

function note(msg) { if (root) ctx.ui.note($('#note'), msg); }
const iconFor = kind => (kind === 2 || kind === 1 ? 'train' : 'bus');
const minsTo = (at, now) => Math.round((at - now) / 60);
function when(d, now) {
  const m = minsTo(d.at, now);
  if (m <= 0) return 'Due';
  if (m < 60) return `${m} min`;
  return new Date(d.at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}
function how(d) {
  if (d.canceled) return 'Canceled';
  if (d.live) {
    const m = Math.round(d.delay / 60);
    return m >= 1 ? `live · ${m} min late` : m <= -1 ? `live · ${-m} min early` : 'live · on time';
  }
  return d.approx ? 'timetable (estimated)' : 'timetable';
}
const chip = (r, esc) => `<span class="tr-chip" style="${r.color ? `background:#${esc(r.color)};color:#${esc(r.text || 'ffffff')}` : ''}">${esc(r.label)}</span>`;

/* ---------- loading ---------- */

async function status() {
  st.status = await ctx.call('status');
  ctx.store.set('city', st.status.name || '');
  return st.status;
}

async function load() {
  clearTimeout(T);
  try {
    const s = await status();
    const missing = s.feeds.filter(f => f.state === 'missing' || f.stale);
    if (s.city && missing.length && !st.busy) { prepare(false); return; }
    if (s.stops.length) { st.board = await ctx.call('departures'); remember(); }
    note('');
  } catch (e) { note(errText(e)); }
  render();
  if (shown) T = setTimeout(load, 30000);
}

/** Fetches and reads the timetables, showing what it's doing. */
async function prepare(force) {
  st.busy = 'Getting the timetable'; render();
  const off = ctx.on('progress', p => { if (p.text) { st.busy = p.text; render(); } });
  try { st.status = await ctx.call('prepare', { force }); st.busy = ''; await load(); }
  catch (e) { st.busy = ''; note(errText(e)); render(); }
  finally { off(); }
}

/** What the pill and search show between reads. */
function remember() {
  const now = st.board.at, next = {};
  for (const s of st.board.stops) { const d = s.departures.find(x => !x.canceled); if (d) next[s.key] = { label: d.label, at: d.at, headsign: d.headsign, kind: d.kind }; }
  ctx.store.set('next', { at: now, next });
}

/* ---------- views ---------- */

function cityView() {
  const { esc, icon } = ctx.ui;
  const p = (st.status.presets || [])[0], c = st.status.custom;
  return `
  <div class="card k-setup">
    <div class="k-head"><span class="k-logo">${icon('bus')}</span><div class="k-title"><b>Where do you ride?</b><small>The next departures from your stops, live when your agency shares them</small></div>${st.status.city ? '<button class="btn" type="button" data-back>Cancel</button>' : ''}</div>
    ${p ? `<button class="tr-preset" type="button" data-city="${esc(p.id)}"><b>${esc(p.name)}</b><small>${esc(p.sub)}</small><span class="btn primary">Use</span></button>` : ''}
    <details class="tr-custom"${st.status.city === 'custom' ? ' open' : ''}>
      <summary>Another city</summary>
      <ol class="k-steps">
        <li>Find your transit agency's <b>GTFS</b> timetable (a .zip) and, if it has one, its <b>GTFS-realtime trip updates</b> feed. Agencies list them on their developer page, or look yours up on <button class="k-link" type="button" data-open-mdb>the Mobility Database</button>.</li>
        <li>Paste the two addresses. If the live feed needs an API key, give the header it goes in and the key.</li>
      </ol>
      <form class="tr-form" id="cityForm" autocomplete="off">
        <input class="field" name="name" placeholder="City or agency (shown in the tab)" value="${esc((c && c.name) || '')}">
        <input class="field" name="schedule" placeholder="Timetable: https://…/gtfs.zip" spellcheck="false">
        <input class="field" name="live" placeholder="Live trip updates (optional): https://…" spellcheck="false">
        <div class="tr-row"><input class="field" name="header" placeholder="Key header (optional), like x-api-key" spellcheck="false" value="${esc((c && c.header) || '')}"><input class="field" name="key" type="password" placeholder="API key (optional)" spellcheck="false"></div>
        <button class="btn primary" type="submit">Use these</button>
      </form>
      <div class="k-fine"><span>The addresses and key are stored encrypted for your Windows account only.</span></div>
    </details>
  </div>`;
}

function busyView() {
  const { esc } = ctx.ui;
  const feeds = (st.status && st.status.feeds) || [];
  return `<div class="card tr-busy"><span class="tr-spin" aria-hidden="true"></span><div><b>${esc(st.busy)}…</b><small>${feeds.map(f => esc(f.name)).join(' · ')}. Once a week; it takes a few seconds after the download.</small></div></div>`;
}

function stopCard(s, b, now) {
  const { esc, icon } = ctx.ui;
  const deps = (b && b.departures) || [];
  const open = st.open === s.key;
  const rows = deps.length ? deps.map(d => `
      <div class="tr-dep${d.canceled ? ' off' : ''}">${chip(d, esc)}<span class="tr-to" title="${esc(d.headsign)}">${esc(d.headsign || d.label)}</span><span class="tr-when"><b>${esc(when(d, now))}</b><small class="${d.live && !d.canceled ? 'live' : ''}">${esc(how(d))}</small></span></div>`).join('')
    : `<p class="empty">${b && b.error ? esc(b.error) : 'Nothing in the next 3 hours.'}</p>`;
  const w = s.window;
  return `
  <div class="card tr-stop" data-key="${esc(s.key)}">
    <header><span class="tr-ic">${icon(iconFor(s.kind))}</span><div class="tr-name"><b title="${esc(s.name)}">${esc(s.name)}</b><small>${esc([s.code ? `Stop ${s.code}` : '', s.routes.length ? `${s.routes.length} route${s.routes.length === 1 ? '' : 's'}` : '', w ? `on the pill ${w.from}–${w.to}` : ''].filter(Boolean).join(' · '))}</small></div>
      <button class="ib sm" type="button" data-edit title="Routes and pill times" aria-label="Routes and pill times" aria-expanded="${open}">${icon('gear')}</button></header>
    ${open ? editView(s) : `<div class="tr-deps">${rows}</div>`}
    ${b && !b.live && b.liveError ? `<p class="tr-why">Live times aren’t coming through: ${esc(b.liveError)}</p>` : ''}
  </div>`;
}

function editView(s) {
  const { esc } = ctx.ui;
  const routes = st.routes[s.key];
  const w = s.window || { days: [0, 1, 2, 3, 4], from: '07:30', to: '09:00' };
  const on = id => !s.routes.length || s.routes.includes(id);
  return `
    <div class="tr-edit">
      <div class="tr-sub">Routes</div>
      <div class="tr-routes">${routes ? routes.map(r => `<button class="tr-rt${on(r.id) ? ' on' : ''}" type="button" data-route="${esc(r.id)}" title="${esc(r.long || r.label)}">${chip(r, esc)}</button>`).join('') : '<small class="tr-why">Loading…</small>'}</div>
      <label class="switch"><input type="checkbox" data-pill${s.window ? ' checked' : ''}><span>Show the next departure on the pill</span></label>
      <div class="tr-window"${s.window ? '' : ' hidden'}>
        <div class="tr-days">${DAYS.map((d, i) => `<button class="chip" type="button" data-day="${i}" aria-pressed="${w.days.includes(i)}">${d}</button>`).join('')}</div>
        <div class="tr-times"><label>From <input class="field" type="time" data-from value="${esc(w.from)}"></label><label>to <input class="field" type="time" data-to value="${esc(w.to)}"></label></div>
      </div>
      <div class="tr-actions"><button class="btn primary" type="button" data-done>Done</button><span class="grow"></span><button class="k-link" type="button" data-remove>Remove this stop</button></div>
    </div>`;
}

function addView() {
  const { esc } = ctx.ui;
  const res = st.results;
  return `
  <div class="card tr-add">
    <header><b>Add a stop</b><button class="link-btn" type="button" data-back>Done</button></header>
    <form class="search" id="find" autocomplete="off"><input id="q" placeholder="Stop name, like “Broad & Walnut”, or its number" value="${esc(st.q)}" aria-label="Find a stop"></form>
    <div class="tr-results">${res == null ? '' : res.length ? res.map((r, i) => `
      <button class="tr-res" type="button" data-res="${i}"${r.saved ? ' disabled' : ''}><div><b>${esc(r.name)}</b><small>${esc([r.code ? `Stop ${r.code}` : '', r.station ? 'Station' : '', r.feedName].filter(Boolean).join(' · '))}</small></div><span class="tr-chips">${r.routes.map(x => chip(x, esc)).join('')}</span>${r.saved ? '<em>Saved</em>' : ''}</button>`).join('') : '<p class="empty">No stops by that name.</p>'}</div>
  </div>`;
}

function mainView() {
  const { esc, icon } = ctx.ui;
  const s = st.status, now = st.board ? st.board.at : Math.floor(Date.now() / 1000);
  const errors = s.feeds.filter(f => f.state === 'error');
  return `
  <div class="v-head"><b>${esc(s.name || 'Transit')}</b><small>${st.board ? `updated ${new Date(st.board.at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}` : ''}</small><span class="grow"></span>
    <button class="btn" type="button" data-add>${icon('plus')}Add a stop</button>
    <button class="ib" type="button" data-city-edit title="Change city" aria-label="Change city">${icon('globe')}</button></div>
  ${errors.map(f => `<p class="note">${esc(f.name)}: ${esc(f.text)} <button class="k-link" type="button" data-retry>Try again</button></p>`).join('')}
  ${s.stops.length ? `<div class="tr-grid">${s.stops.map(x => stopCard(x, st.board && st.board.stops.find(b => b.key === x.key), now)).join('')}</div>`
    : '<div class="card"><p class="empty">No stops yet. Add the ones you use, and their next departures show here.</p></div>'}`;
}

function render() {
  if (!root) return;
  const s = st.status;
  let html;
  if (!s) html = '<p class="empty">Loading Transit…</p>';
  else if (st.busy) html = busyView();
  else if (!s.city || st.view === 'city') html = cityView();
  else if (st.view === 'add') html = addView();
  else html = mainView();
  const q = $('#q'), focused = q && root.getRootNode().activeElement === q;
  ctx.ui.setHtml($('#body'), html);
  if (focused) { const n = $('#q'); n.focus(); n.setSelectionRange(n.value.length, n.value.length); }
  ctx.island.layout();
}

/* ---------- actions ---------- */

function setCity(params) {
  st.busy = 'Saving'; render();
  ctx.call('city', params).then(s => { st.status = s; st.busy = ''; st.view = 'main'; st.board = null; return prepare(false); })
    .catch(e => { st.busy = ''; note(errText(e)); render(); });
}

let findT = 0;
function find(q) {
  st.q = q; clearTimeout(findT);
  if (q.trim().length < 2) { st.results = null; render(); return; }
  findT = setTimeout(() => ctx.call('search', { q }).then(r => { if (st.q === q) { st.results = r; render(); } }).catch(e => note(errText(e))), 180);
}

function add(i) {
  const r = st.results && st.results[i]; if (!r) return;
  st.busy = 'Reading this stop’s timetable'; render();
  ctx.call('add', { feed: r.feed, id: r.id }).then(s => { st.status = s; st.busy = ''; st.view = 'main'; st.results = null; st.q = ''; return load(); })
    .catch(e => { st.busy = ''; note(errText(e)); render(); });
}

function update(key, change) {
  return ctx.call('update', { key, ...change }).then(s => { st.status = s; schedulePill(true); render(); }).catch(e => note(errText(e)));
}

function edit(key) {
  st.open = st.open === key ? '' : key;
  if (st.open && !st.routes[key]) ctx.call('routes', { key }).then(r => { st.routes[key] = r; render(); }).catch(e => note(errText(e)));
  render();
}

function stopOf(el) {
  const k = el.closest('[data-key]'); return k && st.status.stops.find(s => s.key === k.dataset.key);
}

function windowOf(card) {
  const days = [...card.querySelectorAll('[data-day][aria-pressed="true"]')].map(b => +b.dataset.day);
  return { days, from: card.querySelector('[data-from]').value || '07:30', to: card.querySelector('[data-to]').value || '09:00' };
}

function mount(r) {
  root = r;
  root.innerHTML = '<div id="body"></div><p class="note" id="note" hidden></p>';
  root.addEventListener('click', e => {
    const t = e.target;
    const city = t.closest('[data-city]'); if (city) { setCity({ city: city.dataset.city }); return; }
    if (t.closest('[data-open-mdb]')) { ctx.openWeb(MOBILITY_DB).catch(err => note(errText(err))); return; }
    if (t.closest('[data-back]')) { st.view = 'main'; st.results = null; st.q = ''; render(); return; }
    if (t.closest('[data-add]')) { st.view = 'add'; render(); const q = $('#q'); if (q) q.focus(); return; }
    if (t.closest('[data-city-edit]')) { st.view = 'city'; render(); return; }
    if (t.closest('[data-retry]')) { prepare(true); return; }
    const res = t.closest('[data-res]'); if (res) { add(+res.dataset.res); return; }
    const s = stopOf(t); if (!s) return;
    if (t.closest('[data-edit]') || t.closest('[data-done]')) { edit(s.key); return; }
    const rt = t.closest('[data-route]');
    if (rt) {
      const all = (st.routes[s.key] || []).map(x => x.id);
      let on = s.routes.length ? [...s.routes] : [...all];
      on = on.includes(rt.dataset.route) ? on.filter(x => x !== rt.dataset.route) : [...on, rt.dataset.route];
      // Every route (or none) means no filter.
      update(s.key, { routes: on.length === all.length || !on.length ? [] : on });
      return;
    }
    const day = t.closest('[data-day]');
    if (day) { day.setAttribute('aria-pressed', day.getAttribute('aria-pressed') !== 'true'); update(s.key, { window: windowOf(t.closest('.tr-stop')) }); return; }
    if (t.closest('[data-remove]')) {
      const b = t.closest('[data-remove]');
      if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to remove'; return; }
      st.open = ''; ctx.call('remove', { key: s.key }).then(x => { st.status = x; schedulePill(true); return load(); });
    }
  });
  root.addEventListener('change', e => {
    const s = stopOf(e.target); if (!s) return;
    if (e.target.matches('[data-pill]')) update(s.key, { window: e.target.checked ? windowOf(e.target.closest('.tr-stop')) : null });
    else if (e.target.matches('[data-from], [data-to]')) update(s.key, { window: windowOf(e.target.closest('.tr-stop')) });
  });
  root.addEventListener('input', e => { if (e.target.id === 'q') find(e.target.value); });
  root.addEventListener('submit', e => {
    e.preventDefault();
    if (e.target.id === 'find') { find($('#q').value); return; }
    if (e.target.id === 'cityForm') {
      const f = new FormData(e.target);
      setCity({ city: 'custom', name: f.get('name'), schedule: f.get('schedule'), live: f.get('live'), header: f.get('header'), key: f.get('key') });
    }
  });
  render();
}

/* ---------- the pill: the next departure around commute times ---------- */

const hm = s => { const [h, m] = String(s || '0:0').split(':').map(Number); return (h || 0) * 60 + (m || 0); };
/** Saved stops whose pill window is now (the PC's clock). */
export function inWindow(stops, d = new Date()) {
  const day = (d.getDay() + 6) % 7, mins = d.getHours() * 60 + d.getMinutes();
  return (stops || []).filter(s => s.window && s.window.days.includes(day) && hm(s.window.from) <= mins && mins < hm(s.window.to));
}

async function pillTick() {
  clearTimeout(P);
  const stops = inWindow(ctx.store.get('stops', []));
  if (!stops.length) { ctx.activity.clear('next'); P = setTimeout(pillTick, 60000); return; }
  try {
    const b = await ctx.call('departures');
    const now = b.at;
    let best = null;
    for (const s of stops) {
      const d = ((b.stops.find(x => x.key === s.key) || {}).departures || []).find(x => !x.canceled && x.at >= now);
      if (d && (!best || d.at < best.d.at)) best = { s, d };
    }
    if (best) {
      const m = minsTo(best.d.at, now);
      ctx.activity.set({ id: 'next', kind: 'custom', priority: 15, icon: iconFor(best.d.kind), text: `${best.d.label} · ${m <= 0 ? 'Due' : m < 60 ? `${m} min` : when(best.d, now)}`, sub: `${best.s.name}: ${best.d.headsign}${best.d.live ? '' : ' (timetable)'}`, tone: m <= 2 ? 'alert' : '', action: 'tab:transit' });
    } else ctx.activity.clear('next');
  } catch (e) { ctx.activity.clear('next'); }
  P = setTimeout(pillTick, 60000);
}
/** The saved stops (and their windows) changed: remember them and check the pill now. */
function schedulePill(now) {
  if (st.status) ctx.store.set('stops', st.status.stops);
  if (now) pillTick();
}

export function activate(c) {
  ctx = c;
  ctx.tabs.register({ id: 'transit', mount, show() { shown = true; load().then(() => schedulePill(false)); }, hide() { shown = false; clearTimeout(T); } });
  ctx.setup.card(() => ({ state: ctx.store.get('city', '') }));
  // The Command Center's cube: the soonest departure from the saved stops (as last read), else what
  // its sheet also holds, the Transit app's trip planner.
  ctx.cubes.register({
    id: 'transit',
    stat: () => {
      const now = Date.now() / 1000, next = Object.values((ctx.store.get('next', {}) || {}).next || {}).filter(d => d.at >= now - 30).sort((a, b) => a.at - b.at)[0];
      if (!next || minsTo(next.at, now) > 90) return 'Trip planner';
      const m = minsTo(next.at, now);
      return `${next.label} · ${m <= 0 ? 'Due' : `${m} min`}`;
    }
  });
  ctx.search.register({
    id: 'stops', kind: 'stop', label: 'Transit', icon: 'bus',
    items: () => {
      const saved = ctx.store.get('stops', []), next = (ctx.store.get('next', {}) || {}).next || {};
      return saved.map(s => {
        const n = next[s.key];
        return { id: s.key, name: s.name, sub: n ? `Next: ${n.label} to ${n.headsign} at ${new Date(n.at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}` : 'Transit stop', keys: `transit stop ${s.code || ''}`, actions: [{ label: 'Departures', icon: iconFor(s.kind), run: () => ctx.open('tab:transit') }] };
      });
    }
  });
  pillTick();
}

export function deactivate() {
  clearTimeout(T); clearTimeout(P);
  ctx.activity.clear('next');
}
