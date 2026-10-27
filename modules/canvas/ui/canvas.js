// Canvas's island tab: what's due, missing work, grades and announcements from the user's school
// Canvas, through the module's backend (src/main.rs) with their own token or calendar feed link.
// Its Command Center cube docks Canvas itself (from the manifest's sites) once it's set up.

let ctx, root, T = 0;
const st = { status: null, data: null, edit: false, setup: 'feed', picked: false };
const $ = s => root.querySelector(s), $$ = s => [...root.querySelectorAll(s)];

// Courses without a Canvas color (the calendar feed has none) get one from this set.
const COLORS = ['#4f8cf7', '#e8704a', '#7fd99a', '#c07fe0', '#f5b45b', '#5bc8d8', '#f07aa0', '#a3b86c'];
const course = id => (st.data && st.data.courses.find(c => c.id === id)) || {};
const colorOf = id => course(id).color || (id ? COLORS[id % COLORS.length] : 'var(--faint)');
const KIND = { assignment: 'Assignment', quiz: 'Quiz', discussion_topic: 'Discussion', wiki_page: 'Page', planner_note: 'To-do', calendar_event: 'Event', assessment_request: 'Peer review' };

function view() {
  const { icon } = ctx.ui;
  return `
  <div id="setup" hidden>
    <div class="card k-setup">
      <div class="k-head">
        <span class="k-logo">${icon('cap')}</span>
        <div class="k-title"><b>Connect Canvas</b><small>Due dates, grades and announcements from your school's Canvas</small></div>
        <button class="btn" type="button" id="back" hidden>Cancel</button>
      </div>
      <div class="seg" id="mode" role="group" aria-label="How to connect">
        <button type="button" data-mode="feed">Calendar feed</button>
        <button type="button" data-mode="token">Access token</button>
      </div>
      <ol class="k-steps" data-cv="feed">
        <li>In Canvas, open <b>Calendar</b> and click <b>Calendar Feed</b> (bottom right of the page).</li>
        <li>Copy the link it shows (it ends in <code>.ics</code>) and paste it here. No access token or admin approval needed.</li>
      </ol>
      <form class="k-form" id="feedForm" data-cv="feed" autocomplete="off">
        <input class="field" id="feed" type="password" placeholder="https://school.instructure.com/feeds/calendars/user_….ics" spellcheck="false" aria-label="Canvas calendar feed link">
        <button class="btn primary" type="submit" id="feedSave">Connect</button>
      </form>
      <ol class="k-steps" data-cv="token">
        <li>Your school's <b>Canvas address</b>: what's in the browser's address bar on Canvas, like <code>school.instructure.com</code>.</li>
        <li>In Canvas, open <b>Account → Settings</b>, click <b>+ New Access Token</b>, name it "Island Shell" and copy the token (Canvas shows it only once).</li>
      </ol>
      <form class="k-form" id="form" data-cv="token" autocomplete="off">
        <input class="field" id="domain" placeholder="school.instructure.com" spellcheck="false" aria-label="Canvas address">
        <input class="field" id="token" type="password" placeholder="Access token" spellcheck="false" aria-label="Canvas access token">
        <button class="btn primary" type="submit" id="save">Connect</button>
      </form>
      <div class="k-fine"><span data-cv="feed">The feed has due dates and course events only: no grades, submission status or announcements, and Done ticks are kept on this PC. The link is stored encrypted for your Windows account only.</span><span data-cv="token">The token is stored encrypted for your Windows account only. No token button in Settings means your school has turned them off; use the calendar feed.</span><span class="grow"></span><button class="k-link" type="button" id="forget" hidden>Forget</button></div>
    </div>
  </div>
  <div id="main" hidden>
    <div class="v-head">
      <b>Canvas</b><small id="who"></small><span class="grow"></span>
      <button class="ib" type="button" id="editBtn" title="Canvas settings" aria-label="Canvas settings">${icon('gear')}</button>
    </div>
    <div class="cv-grid">
      <div class="cv-col">
        <div class="card cv-list cv-missing" id="missing" hidden></div>
        <div class="card cv-list" id="up"></div>
      </div>
      <div class="cv-col">
        <div class="card cv-grades" id="courses" hidden></div>
        <div class="card cv-grades" id="grades"></div>
        <div class="card cv-ann" id="ann"></div>
      </div>
    </div>
  </div>
  <p class="empty" id="wait" hidden>Loading Canvas…</p>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => ctx.ui.note($('#note'), msg);
const showing = () => ctx.island.showing('canvas');
const errText = e => (e && e.message) || String(e);

/** What the cube shows while the tab hasn't loaded anything this session. */
function remember() {
  ctx.store.set('configured', !!(st.status && st.status.configured));
  const n = st.data && st.data.upcoming.find(t => !t.done && !t.submitted);
  ctx.store.set('next', n ? n.title : '');
}

async function loop() {
  clearTimeout(T);
  if (!showing()) return;
  try { st.status = await ctx.call('status'); } catch (e) { note(errText(e)); return; }
  if (st.status.configured && !st.edit) {
    if (!st.data) { render(); $('#wait').hidden = false; ctx.island.layout(); }
    try { st.data = await ctx.call('overview'); note(''); } catch (e) { note(errText(e)); }
    $('#wait').hidden = true;
  }
  remember();
  render();
  T = setTimeout(loop, 120000);
}

function status(t) {
  if (t.graded) return ['Graded', 'ok'];
  if (t.submitted) return [t.late ? 'Submitted late' : 'Submitted', 'ok'];
  if (t.missing) return ['Missing', 'bad'];
  const left = Date.parse(t.due) - Date.now();
  if (left > 0 && left < 864e5 && !t.done) return [left < 36e5 ? `${Math.max(1, Math.round(left / 6e4))} min left` : `${Math.round(left / 36e5)}h left`, 'warn'];
  return null;
}
function row(t, i, list) {
  const { esc, icon } = ctx.ui;
  const d = new Date(t.due), c = course(t.course_id), s = status(t);
  const when = isNaN(d) ? '' : t.all_day ? 'All day' : d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const sub = [KIND[t.kind] || '', c.code || c.name || '', list === 'missing' && !isNaN(d) ? d.toLocaleDateString([], { month: 'short', day: 'numeric' }) : when, t.points ? `${t.points} pts` : ''].filter(Boolean).join(' · ');
  return `<div class="cv-row${t.done ? ' done' : ''}" data-list="${list}" data-i="${i}" title="${esc(t.title)}"><button class="cv-check" type="button" aria-pressed="${!!t.done}" title="${t.done ? 'Mark not done' : 'Mark done'}${st.data && st.data.limited ? ' (on this PC)' : ''}" aria-label="Done">${icon('check')}</button><span class="cv-bar" style="background:${esc(colorOf(t.course_id))}"></span><div><b>${esc(t.title)}</b><small>${esc(sub)}</small></div>${s ? `<span class="cv-chip ${s[1]}">${esc(s[0])}</span>` : '<span></span>'}</div>`;
}
const dayLabel = d => { const a = new Date(); a.setHours(0, 0, 0, 0); const b = new Date(d); b.setHours(0, 0, 0, 0); const n = Math.round((b - a) / 864e5); return n === 0 ? 'Today' : n === 1 ? 'Tomorrow' : n === -1 ? 'Yesterday' : b.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' }); };

function render() {
  const { esc, setHtml, txt, ago } = ctx.ui;
  const s = st.status || {}, setup = !s.configured || st.edit;
  $('#setup').hidden = !setup; $('#main').hidden = setup || !st.data;
  if (setup) {
    if (s.configured && s.mode) st.setup = s.mode === 'feed' && !st.picked ? 'feed' : st.setup;
    $$('#mode button').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === st.setup));
    $$('#setup [data-cv]').forEach(el => { el.hidden = el.dataset.cv !== st.setup; });
    const dom = $('#domain'); if (!dom.value && s.domain) dom.value = s.domain;
    $('#feed').placeholder = s.configured && s.mode === 'feed' ? 'Feed link (saved; paste a new one to replace it)' : 'https://school.instructure.com/feeds/calendars/user_….ics';
    $('#token').placeholder = s.configured ? 'Access token (saved; blank keeps it)' : 'Access token';
    $('#back').hidden = !s.configured; $('#forget').hidden = !s.configured;
    if (showing()) ctx.island.layout();
    return;
  }
  const o = st.data; if (!o) return;
  txt($('#who'), [o.name, o.domain, o.limited ? 'calendar feed: due dates only' : ''].filter(Boolean).join(' · '));
  $('#grades').hidden = $('#ann').hidden = o.limited;
  $('#courses').hidden = !o.limited;
  if (o.limited) {
    // The feed has no grades: show each course's load for the coming week instead.
    const week = Date.now() + 7 * 864e5;
    setHtml($('#courses'), `<header><b>Courses</b><small>next 7 days</small></header>` + (o.courses.length ? o.courses.map(c => {
      const due = o.upcoming.filter(t => t.course_id === c.id && !t.done && Date.parse(t.due) < week), next = due[0];
      return `<div class="cv-course" data-url="${esc(c.url)}" title="${esc(c.name)}"><span class="cv-bar" style="background:${esc(colorOf(c.id))}"></span><div><b>${esc(c.name)}</b><small>${next ? `next: ${esc(next.title)} · ${esc(dayLabel(next.due))}` : 'nothing due this week'}</small></div><em>${due.length || ''}</em></div>`;
    }).join('') : '<p class="empty">No courses in the feed.</p>'));
  }
  const miss = $('#missing');
  miss.hidden = !o.missing.length;
  setHtml(miss, o.missing.length ? `<header><b>Missing</b><small>${o.missing.length}</small></header><div class="cv-scroll">${o.missing.map((t, i) => row(t, i, 'missing')).join('')}</div>` : '');
  let last = '', rows = '';
  o.upcoming.forEach((t, i) => { const l = dayLabel(t.due); if (l !== last) { rows += `<div class="cv-day">${esc(l)}</div>`; last = l; } rows += row(t, i, 'upcoming'); });
  const open = o.upcoming.filter(t => !t.done && !t.submitted && !t.graded).length;
  setHtml($('#up'), `<header><b>Coming up</b><small>${open ? `${open} to do` : 'All caught up'}</small></header>` + (rows ? `<div class="cv-scroll">${rows}</div>` : '<p class="empty">Nothing due in the next three weeks.</p>'));
  setHtml($('#grades'), `<header><b>Grades</b><small>current</small></header>` + (o.courses.length ? o.courses.map(c => `<div class="cv-course" data-url="${esc(c.url + '/grades')}" title="${esc(c.name)}"><span class="cv-bar" style="background:${esc(c.color || 'var(--accent)')}"></span><div><b>${esc(c.code || c.name)}</b><small>${esc(c.code ? c.name : '')}</small></div><em>${c.score != null ? `${c.score.toFixed(1)}%` : '–'}${c.grade ? `<small> ${esc(c.grade)}</small>` : ''}</em></div>`).join('') : '<p class="empty">No active courses.</p>'));
  setHtml($('#ann'), `<header><b>Announcements</b><small>last 3 weeks</small></header>` + (o.announcements.length ? o.announcements.slice(0, 5).map(a => { const c = course(a.course_id); return `<div class="cv-an" data-url="${esc(a.url)}"><div><i style="background:${esc(c.color || 'var(--accent)')}"></i><b>${esc(a.title)}</b></div><small>${esc(c.code || c.name || '')} · ${esc(ago(a.posted))}</small>${a.preview ? `<p>${esc(a.preview)}</p>` : ''}</div>`; }).join('') : '<p class="empty">No recent announcements.</p>'));
  if (showing()) ctx.island.layout();
}

/** Opens a page on the user's own Canvas in the default browser (never anything else). */
function openPage(url) {
  const domain = (st.status && st.status.domain) || '';
  if (!domain || !String(url).startsWith(`https://${domain}/`)) { note('That isn\'t a page on your Canvas.'); return; }
  ctx.openWeb(url).then(() => ctx.island.closePanel(true)).catch(err => note(errText(err)));
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#main').addEventListener('click', e => {
    const check = e.target.closest('.cv-check'), r = e.target.closest('.cv-row');
    if (check && r) {
      const t = st.data[r.dataset.list][+r.dataset.i]; if (!t) return;
      t.done = !t.done; render();
      ctx.call('done', { key: t.key, kind: t.kind, id: t.id, override_id: t.override_id, done: t.done })
        .then(id => { if (id) t.override_id = id; remember(); }).catch(err => { t.done = !t.done; render(); note(errText(err)); });
      return;
    }
    const url = r ? (st.data[r.dataset.list][+r.dataset.i] || {}).url : (e.target.closest('[data-url]') || {}).dataset?.url;
    if (url) openPage(url);
  });
  $('#form').addEventListener('submit', e => {
    e.preventDefault();
    const domain = $('#domain').value.trim(), token = $('#token').value.trim();
    if (!domain) { note('Add your school’s Canvas address first.'); return; }
    note(''); $('#save').disabled = true;
    ctx.call('configure', { domain, token }).then(s => {
      $('#token').value = ''; st.status = s; st.edit = false; st.data = null; loop();
    }).catch(err => note(errText(err))).finally(() => { $('#save').disabled = false; });
  });
  $('#feedForm').addEventListener('submit', e => {
    e.preventDefault();
    const url = $('#feed').value.trim();
    if (!url) { note('Paste the calendar feed link first.'); return; }
    note(''); $('#feedSave').disabled = true;
    ctx.call('configure_feed', { url }).then(s => {
      $('#feed').value = ''; st.status = s; st.edit = false; st.data = null; loop();
    }).catch(err => note(errText(err))).finally(() => { $('#feedSave').disabled = false; });
  });
  $('#mode').addEventListener('click', e => {
    const b = e.target.closest('[data-mode]'); if (!b) return;
    st.setup = b.dataset.mode; st.picked = true; ctx.store.set('setup', st.setup); note(''); render();
  });
  $('#editBtn').addEventListener('click', () => { st.edit = true; render(); });
  $('#back').addEventListener('click', () => { st.edit = false; $('#token').value = ''; render(); });
  $('#forget').addEventListener('click', () => {
    // A second click within 3 s confirms.
    const b = $('#forget');
    if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget'; }, 3000); return; }
    delete b.dataset.sure; b.textContent = 'Forget';
    ctx.call('forget').then(() => { st.edit = false; st.data = null; $('#domain').value = ''; $('#feed').value = ''; loop(); });
  });
}

export function activate(c) {
  ctx = c;
  // How to connect, kept from 0.1.x's island.cvSetup the first time.
  let legacy = 'feed';
  try { legacy = JSON.parse(localStorage.getItem('island.cvSetup') || '"feed"'); } catch (e) {}
  st.setup = ctx.store.get('setup', legacy);
  ctx.tabs.register({ id: 'canvas', mount, show: loop, hide: () => clearTimeout(T) });
  // Whether the docked site can open yet (the Command Center loads this UI for the cube's stat).
  ctx.call('status').then(s => { st.status = s; ctx.store.set('configured', !!s.configured); }).catch(() => {});
  ctx.cubes.register({
    id: 'canvas',
    stat: () => {
      const n = st.data ? (st.data.upcoming.find(t => !t.done && !t.submitted) || {}).title : ctx.store.get('next', '');
      return n ? `Next: ${n}` : 'Due dates';
    },
    siteReady: () => (st.status ? !!st.status.configured : ctx.store.get('configured', false)),
    siteWhy: 'Connect Canvas in its island view first'
  });
}

export function deactivate() {
  clearTimeout(T);
}
