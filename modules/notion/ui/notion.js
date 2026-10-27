// Notion's section in Today: the open to-dos from the user's Notion databases, ticked off right
// here, through the module's backend (src/main.rs) with their own integration secret. Setting up
// (the secret, then which databases are to-dos and which go in the calendar) happens in the same
// section. Dated pages reach the island's calendar from the backend.

let ctx, root = null, shown = false, T = 0;
const st = { status: null, data: null, edit: false, dbs: null, picks: {}, all: false, busy: new Set() };
const $ = s => root && root.querySelector(s), $$ = s => (root ? [...root.querySelectorAll(s)] : []);
const errText = e => (e && e.message) || String(e);
const INTEGRATIONS = 'https://www.notion.so/profile/integrations';
const SHOW = 8;
/** Its icon, drawn inline so it takes the text color (ui/icon.svg is the same drawing). */
const LOGO = '<svg class="i nt-logo" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5 15.5 3.6 19.5 6.4V19.6L8.8 20.4 5 17.2z"/><path d="M8.8 7.4v13M8.8 7.4l10.7-1"/><path d="M11.5 10.2v6.6l4.3-7v7"/></svg>';

function note(msg) { if (root) ctx.ui.note($('#note'), msg); }

/** Opens a Notion page in the default browser, and only a Notion page. */
function openPage(url) {
  if (!/^https:\/\/(www\.)?notion\.so\//.test(url || '')) { note('That isn’t a Notion page.'); return; }
  ctx.openWeb(url).then(() => ctx.island.closePanel(true)).catch(e => note(errText(e)));
}

/* ---------- dates ---------- */

const dayStart = d => { const x = new Date(d); x.setHours(0, 0, 0, 0); return x; };
/** A to-do's date: when it is, whether it has a time, and how many days from today. */
function when(due) {
  if (!due) return null;
  const allDay = due.length === 10;
  const d = allDay ? new Date(`${due}T00:00:00`) : new Date(due);
  if (isNaN(d)) return null;
  const days = Math.round((dayStart(d) - dayStart(Date.now())) / 864e5);
  const late = allDay ? days < 0 : d.getTime() < Date.now();
  return { d, allDay, days, late };
}
function dateText(w) {
  const day = w.days === 0 ? 'Today' : w.days === 1 ? 'Tomorrow' : w.days === -1 ? 'Yesterday' : w.d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
  return w.allDay ? day : `${day} ${w.d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`;
}
function group(w) {
  if (!w) return 'No date';
  if (w.late) return 'Overdue';
  if (w.days === 0) return 'Today';
  if (w.days < 7) return 'This week';
  return 'Later';
}

/* ---------- loading ---------- */

async function load(fresh) {
  clearTimeout(T);
  try {
    st.status = await ctx.call('status');
    ctx.store.set('configured', !!st.status.configured);
    if (st.status.configured && st.status.todos && !st.edit) {
      st.data = await ctx.call('todos', { fresh: !!fresh });
      ctx.store.set('open', st.data.todos.length);
    }
    note('');
  } catch (e) { note(errText(e)); }
  render();
  if (shown) T = setTimeout(() => load(false), 120000);
}

async function loadDbs() {
  st.dbs = null; render();
  try {
    st.dbs = await ctx.call('databases');
    st.picks = Object.fromEntries(st.dbs.map(d => [d.source.id, { ...d.source, todos: d.picked && d.source.todos, calendar: d.picked && d.source.calendar }]));
    note('');
  } catch (e) { st.dbs = []; note(errText(e)); }
  render();
}

/* ---------- views ---------- */

function setupView() {
  const { icon } = ctx.ui;
  return `
  <div class="card k-setup">
    <div class="k-head"><span class="k-logo">${LOGO}</span><div class="k-title"><b>Connect Notion</b><small>Your to-dos in Today, and pages with dates in the calendar</small></div></div>
    <ol class="k-steps">
      <li>Open <button class="k-link" type="button" data-open-integrations>Notion’s integrations page</button>, click <b>New integration</b>, pick your workspace, keep the type <b>Internal</b> and save.</li>
      <li>Under <b>Capabilities</b>, keep <b>Read content</b> and turn on <b>Update content</b>, so ticking a to-do here ticks it in Notion too.</li>
      <li>Copy the <b>Internal Integration Secret</b> and paste it here.</li>
    </ol>
    <form class="k-form" id="tokenForm" autocomplete="off">
      <input class="field" id="token" type="password" placeholder="ntn_…" spellcheck="false" aria-label="Notion integration secret">
      <button class="btn primary" type="submit" id="connect">Connect</button>
    </form>
    <div class="k-fine"><span>The secret is stored encrypted for your Windows account only, and the integration only sees the pages you share with it.</span></div>
  </div>`;
}

function pickView() {
  const { esc, icon } = ctx.ui;
  const select = (id, field, opts, cur, none) => `<select class="field nt-sel" data-db="${esc(id)}" data-f="${field}">${none ? `<option value="">${esc(none)}</option>` : ''}${opts.map(o => `<option value="${esc(o.id)}"${o.id === cur ? ' selected' : ''}>${esc(o.name)}</option>`).join('')}</select>`;
  const rows = (st.dbs || []).map(d => {
    const p = st.picks[d.source.id] || d.source;
    const done = d.props.filter(x => x.kind === 'checkbox' || x.kind === 'status');
    const dates = d.props.filter(x => x.kind === 'date');
    return `<div class="nt-db" data-db="${esc(p.id)}">
      <div class="nt-db-head"><b>${esc(p.title)}</b>
        <label class="switch"><input type="checkbox" data-db="${esc(p.id)}" data-f="todos"${p.todos ? ' checked' : ''}${done.length ? '' : ' disabled'}><span>To-dos</span></label>
        <label class="switch"><input type="checkbox" data-db="${esc(p.id)}" data-f="calendar"${p.calendar ? ' checked' : ''}${dates.length ? '' : ' disabled'}><span>Calendar</span></label>
      </div>
      ${p.todos || p.calendar ? `<div class="nt-db-props">
        ${p.todos ? `<label><span>Done when</span>${select(p.id, 'done_prop', done, p.done_prop)}</label>` : ''}
        <label><span>Date</span>${select(p.id, 'date_prop', dates, p.date_prop, p.calendar ? '' : 'None')}</label>
      </div>` : ''}
      ${!done.length && !dates.length ? '<small class="nt-why">No checkbox, status or date property, so there’s nothing to show from it.</small>' : ''}
    </div>`;
  }).join('');
  return `
  <div class="card nt-pick">
    <header><b>Notion databases</b><small>${esc(st.status.workspace || '')}</small></header>
    <p class="nt-hint">In Notion, open each database you want, click <b>•••</b> (top right) → <b>Connections</b> and add your integration. Then <button class="k-link" type="button" data-refresh-dbs>look again</button>.</p>
    ${st.dbs == null ? '<p class="empty">Looking for databases…</p>' : rows || '<p class="empty">No databases are shared with your integration yet.</p>'}
    <div class="nt-actions"><button class="btn primary" type="button" data-save>Save</button>${st.status.todos || st.status.calendar ? '<button class="btn" type="button" data-cancel>Cancel</button>' : ''}<span class="grow"></span><button class="k-link" type="button" data-forget>Disconnect</button></div>
  </div>`;
}

function listView() {
  const { esc, icon } = ctx.ui;
  const o = st.data || { todos: [], problems: [] };
  const all = o.todos, list = st.all ? all : all.slice(0, SHOW);
  let last = '', rows = '';
  for (const t of list) {
    const w = when(t.due), g = group(w);
    if (g !== last) { rows += `<div class="nt-group${g === 'Overdue' ? ' late' : ''}">${esc(g)}</div>`; last = g; }
    const sub = [w ? dateText(w) : '', t.list].filter(Boolean).join(' · ');
    rows += `<div class="nt-row${st.busy.has(t.id) ? ' done' : ''}" data-id="${esc(t.id)}"><button class="nt-check" type="button" aria-pressed="${st.busy.has(t.id)}" aria-label="Done" title="Mark done">${icon('check')}</button><div class="nt-text" data-url="${esc(t.url)}" title="Open in Notion"><b>${esc(t.title)}</b><small class="${w && w.late ? 'late' : ''}">${esc(sub)}</small></div></div>`;
  }
  const empty = !st.status.todos ? 'Pick a database for to-dos (the gear).' : 'Nothing left to do.';
  return `
  <div class="card nt-list">
    <header><b>Notion</b><small>${all.length ? `${all.length} open` : ''}</small><span class="grow"></span>
      <button class="ib sm" type="button" data-reload title="Read again" aria-label="Read again">${icon('restart')}</button>
      <button class="ib sm" type="button" data-edit title="Notion databases" aria-label="Notion databases">${icon('gear')}</button></header>
    ${rows ? `<div class="nt-rows">${rows}</div>` : `<p class="empty">${esc(empty)}</p>`}
    ${all.length > SHOW ? `<button class="link-btn nt-more" type="button" data-more>${st.all ? 'Show fewer' : `Show all ${all.length}`}</button>` : ''}
    ${o.problems.map(p => `<p class="note nt-problem">${esc(p.title)}: ${esc(p.error)}</p>`).join('')}
  </div>`;
}

function render() {
  if (!root) return;
  const s = st.status;
  let html;
  if (!s) html = '<p class="empty">Loading Notion…</p>';
  else if (!s.configured) html = setupView();
  else if (st.edit || (!s.todos && !s.calendar)) { if (!st.dbs && !st.loadingDbs) { st.loadingDbs = true; loadDbs().finally(() => { st.loadingDbs = false; }); } html = pickView(); }
  else html = listView();
  ctx.ui.setHtml($('#body'), html);
  ctx.island.layout();
}

/* ---------- actions ---------- */

function tick(id) {
  const t = st.data && st.data.todos.find(x => x.id === id);
  if (!t || st.busy.has(id)) return;
  st.busy.add(id); render();
  ctx.call('done', { id, source: t.source, done: true })
    .then(() => setTimeout(() => { st.busy.delete(id); st.data.todos = st.data.todos.filter(x => x.id !== id); ctx.store.set('open', st.data.todos.length); render(); }, 900))
    .catch(e => { st.busy.delete(id); render(); note(errText(e)); });
}

function save() {
  const sources = Object.values(st.picks).filter(p => p.todos || p.calendar);
  const btn = $('[data-save]'); if (btn) btn.disabled = true;
  ctx.call('choose', { sources }).then(s => { st.status = s; st.edit = false; st.dbs = null; st.data = null; return load(true); })
    .catch(e => note(errText(e))).finally(() => { if (btn) btn.disabled = false; });
}

/** The property that says a to-do is done; for a status, its done options and the one to go back
 * to, as the backend worked them out. */
function setDoneProp(p, d, propId) {
  const prop = d.props.find(x => x.id === propId);
  const s = prop && prop.kind === 'status' ? (d.statuses || {})[prop.id] : null;
  Object.assign(p, { done_prop: prop ? prop.id : '', done_kind: prop ? prop.kind : '', done_names: s ? s.done : [], open_name: s ? s.open : '' });
}

function pickChanged(el) {
  const id = el.dataset.db, f = el.dataset.f, d = (st.dbs || []).find(x => x.source.id === id);
  const p = st.picks[id]; if (!p || !d) return;
  if (f === 'todos' || f === 'calendar') p[f] = el.checked;
  else if (f === 'date_prop') p.date_prop = el.value;
  else if (f === 'done_prop') setDoneProp(p, d, el.value);
  // Turning a part on picks the guessed property (or the first that fits) if there's none yet.
  if (p.calendar && !p.date_prop) { const dp = d.props.find(x => x.kind === 'date'); if (dp) p.date_prop = dp.id; }
  if (p.todos && !p.done_prop) {
    const first = d.props.find(x => x.kind === 'checkbox' || x.kind === 'status');
    setDoneProp(p, d, d.source.done_prop || (first && first.id));
  }
  render();
}

function forget() {
  const b = $('[data-forget]');
  if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to disconnect'; setTimeout(() => { if (b.isConnected) { delete b.dataset.sure; b.textContent = 'Disconnect'; } }, 3000); return; }
  ctx.call('forget').then(() => { st.edit = false; st.dbs = null; st.data = null; ctx.store.set('configured', false); return load(false); });
}

function mount(r) {
  root = r;
  root.innerHTML = '<div id="body"></div><p class="note" id="note" hidden></p>';
  root.addEventListener('click', e => {
    const t = e.target;
    if (t.closest('[data-open-integrations]')) { ctx.openWeb(INTEGRATIONS).catch(err => note(errText(err))); return; }
    if (t.closest('.nt-check')) { tick(t.closest('.nt-row').dataset.id); return; }
    const text = t.closest('.nt-text'); if (text) { openPage(text.dataset.url); return; }
    if (t.closest('[data-reload]')) { load(true); return; }
    if (t.closest('[data-edit]')) { st.edit = true; st.dbs = null; render(); return; }
    if (t.closest('[data-more]')) { st.all = !st.all; render(); return; }
    if (t.closest('[data-refresh-dbs]')) { loadDbs(); return; }
    if (t.closest('[data-save]')) { save(); return; }
    if (t.closest('[data-cancel]')) { st.edit = false; st.dbs = null; render(); return; }
    if (t.closest('[data-forget]')) forget();
  });
  root.addEventListener('change', e => { if (e.target.dataset && e.target.dataset.db) pickChanged(e.target); });
  root.addEventListener('submit', e => {
    if (e.target.id !== 'tokenForm') return;
    e.preventDefault();
    const token = $('#token').value.trim();
    if (!token) { note('Paste the secret first.'); return; }
    $('#connect').disabled = true;
    ctx.call('configure', { token }).then(s => { st.status = s; ctx.store.set('configured', true); note(''); render(); })
      .catch(err => note(errText(err))).finally(() => { const b = $('#connect'); if (b) b.disabled = false; });
  });
  render();
}

export function activate(c) {
  ctx = c;
  ctx.sections.register({ id: 'todos', mount, show() { shown = true; load(false); }, hide() { shown = false; clearTimeout(T); } });
  ctx.setup.card(() => ({ state: ctx.store.get('configured', false) ? 'Connected' : '' }));
  ctx.search.register({
    id: 'todos', kind: 'todo', label: 'Notion', icon: 'check',
    items: () => ((st.data && st.data.todos) || []).map(t => {
      const w = when(t.due);
      return { id: t.id, name: t.title, sub: ['Notion', w ? dateText(w) : '', t.list].filter(Boolean).join(' · '), keys: `notion todo ${t.list}`, actions: [
        { label: 'Open in Notion', icon: 'link', run: () => openPage(t.url) },
        { label: 'Mark done', icon: 'check', run: () => ctx.call('done', { id: t.id, source: t.source, done: true }).then(() => { st.data.todos = st.data.todos.filter(x => x.id !== t.id); }) }
      ] };
    })
  });
  ctx.settings.mount(box => {
    const { esc } = ctx.ui;
    ctx.call('status').then(s => {
      ctx.ui.setHtml(box, `<div class="nt-settings"><p>${s.configured ? `Connected${s.workspace ? ` to <b>${esc(s.workspace)}</b>` : ''}: ${s.todos} database${s.todos === 1 ? '' : 's'} for to-dos, ${s.calendar} for the calendar.` : 'Not connected yet.'}</p><button class="btn" type="button" data-manage>${s.configured ? 'Change databases' : 'Connect'}</button></div>`);
      box.querySelector('[data-manage]').addEventListener('click', () => { st.edit = !!s.configured; st.dbs = null; ctx.open('tab:today'); });
    }).catch(e => { box.textContent = errText(e); });
  });
}

export function deactivate() {
  clearTimeout(T);
}
