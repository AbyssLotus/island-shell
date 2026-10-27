// Canvas's island tab: what's due, missing work, grades and announcements from the user's school
// Canvas, through the module's backend (src/main.rs). Three ways in:
// - Signed in on the site: the user signs in to Canvas where it's docked in the Command Center, and
//   this reads it through the site's reader (readers/canvas-reader.js, run by the base in a hidden
//   copy of the site with the same sign-in) and hands what it brings back to the backend (`ingest`).
//   For schools that turn access tokens off. The backend's tick asks for a read every 10 minutes
//   (the `read` event), which is why this UI loads at start (`ui.eager`).
// - Their own access token, or the calendar feed link: the backend reads Canvas itself.
// Its Command Center cube docks Canvas itself (from the manifest's sites) once it's set up.

let ctx, root, T = 0;
const st = { status: null, data: null, edit: false, setup: 'site', picked: false, reading: null, signedOut: false, readErr: '' };
const $ = s => root.querySelector(s), $$ = s => [...root.querySelectorAll(s)];

// Courses without a Canvas color (the calendar feed has none) get one from this set.
const COLORS = ['#4f8cf7', '#e8704a', '#7fd99a', '#c07fe0', '#f5b45b', '#5bc8d8', '#f07aa0', '#a3b86c'];
const course = id => (st.data && st.data.courses.find(c => c.id === id)) || {};
const colorOf = id => course(id).color || (id ? COLORS[id % COLORS.length] : 'var(--faint)');
const KIND = { assignment: 'Assignment', quiz: 'Quiz', discussion_topic: 'Discussion', wiki_page: 'Page', planner_note: 'To-do', calendar_event: 'Event', assessment_request: 'Peer review' };
/** A read older than this is redone when the tab shows (the tick reads every 10 minutes anyway). */
const STALE = 5 * 6e4;
/** Whether this island can read a docked site (Island Shell 0.3); older ones offer the feed and the token. */
const canRead = () => !!(ctx.sites && ctx.sites.read);
const siteMode = () => !!(st.status && st.status.configured && st.status.mode === 'site');

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
        <button type="button" data-mode="site">Sign in</button>
        <button type="button" data-mode="feed">Calendar feed</button>
        <button type="button" data-mode="token">Access token</button>
      </div>
      <ol class="k-steps" data-cv="site">
        <li>Your school's <b>Canvas address</b>: what's in the browser's address bar on Canvas, like <code>school.instructure.com</code>.</li>
        <li>Click Connect and <b>sign in to Canvas</b> where it opens in the Command Center, the way you always do. No access token or feed link needed.</li>
      </ol>
      <form class="k-form" id="siteForm" data-cv="site" autocomplete="off">
        <input class="field" id="siteDomain" placeholder="school.instructure.com" spellcheck="false" aria-label="Canvas address">
        <button class="btn primary" type="submit" id="siteSave">Connect</button>
      </form>
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
      <div class="k-fine"><span data-cv="site">Everything the access token gives: grades, submissions, missing work, announcements, and Done ticks that show up in Canvas too. Island Shell reads your Canvas the way the Canvas website does, in the Canvas window you signed in to, and keeps it on this PC. There's no password or token to store.</span><span data-cv="feed">The feed has due dates and course events only: no grades, submission status or announcements, and Done ticks are kept on this PC. The link is stored encrypted for your Windows account only.</span><span data-cv="token">The token is stored encrypted for your Windows account only. No token button in Settings means your school has turned them off; use Sign in instead.</span><span class="grow"></span><button class="k-link" type="button" id="forget" hidden>Forget</button></div>
    </div>
  </div>
  <div id="main" hidden>
    <div class="v-head">
      <b>Canvas</b><small id="who"></small><span class="grow"></span>
      <button class="ib" type="button" id="readBtn" title="Read Canvas again" aria-label="Read Canvas again" hidden>${icon('restart')}</button>
      <button class="ib" type="button" id="editBtn" title="Canvas settings" aria-label="Canvas settings">${icon('gear')}</button>
    </div>
    <div class="card cv-signin" id="signin" hidden>
      <span class="cv-signin-ic">${icon('lock')}</span>
      <div><b id="signinTitle">Sign in to Canvas</b><small id="signinText"></small></div>
      <button class="btn primary" type="button" id="signinBtn">Sign in</button>
    </div>
    <div class="cv-grid" id="grid">
      <div class="cv-col">
        <div class="card cv-list cv-missing" id="missing" hidden></div>
        <div class="card cv-list" id="up"></div>
      </div>
      <div class="cv-col">
        <div class="card cv-grades" id="courses" hidden></div>
        <div class="card cv-grades" id="grades"></div>
        <div class="card cv-grades" id="recent" hidden></div>
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
    if (siteMode()) {
      // The last read straight away, then a new one if it's old.
      if (!st.data) st.data = await ctx.call('overview').catch(() => null);
      $('#wait').hidden = true;
      render();
      if (!st.data || Date.now() - st.data.at > STALE) await readSite();
    } else {
      try { st.data = await ctx.call('overview'); note(''); } catch (e) { note(errText(e)); }
    }
    $('#wait').hidden = true;
  }
  remember();
  render();
  T = setTimeout(loop, 120000);
}

/**
 * Reads Canvas through the docked site's sign-in and gives it to the backend. One at a time; a
 * sign-out shows the sign-in card over what was read last.
 */
function readSite() {
  if (st.reading) return st.reading;
  if (!canRead()) { st.readErr = 'Reading Canvas through its website needs a newer Island Shell. Update it in Power, or connect with the calendar feed.'; return Promise.resolve(); }
  st.reading = (async () => {
    if (showing()) render();
    try {
      const data = await ctx.sites.read('web', { action: 'read' });
      if (!data || data.signedIn !== true) { st.signedOut = true; return; }
      st.data = await ctx.call('ingest', { data });
      st.signedOut = false; st.readErr = '';
    } catch (e) {
      const m = errText(e);
      if (/sign in/i.test(m)) st.signedOut = true; else st.readErr = m;
    } finally {
      st.reading = null;
      ctx.store.set('signedOut', st.signedOut);
      remember();
      if (showing()) render();
    }
  })();
  return st.reading;
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
/** "27/30", "18.5/20", "A-", "complete": a recent grade as Canvas shows it, and how it went. */
function scoreOf(g) {
  const n = x => String(Math.round(x * 100) / 100);
  if (g.score != null && g.points) { const p = g.score / g.points; return [`${n(g.score)}/${n(g.points)}`, p >= 0.9 ? 'ok' : p >= 0.7 ? '' : p >= 0.6 ? 'warn' : 'bad']; }
  return [g.grade || (g.score != null ? n(g.score) : '–'), ''];
}

function render() {
  const { esc, setHtml, txt, ago } = ctx.ui;
  const s = st.status || {}, setup = !s.configured || st.edit;
  $('#setup').hidden = !setup;
  // Signed in on the site, the tab shows before the first read: its sign-in card is the way in.
  $('#main').hidden = setup || (!st.data && !siteMode());
  if (setup) {
    if (!canRead() && st.setup === 'site') st.setup = 'feed';
    if (s.configured && s.mode && !st.picked) st.setup = s.mode;
    $('[data-mode="site"]').hidden = !canRead();
    $$('#mode button').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === st.setup));
    $$('#setup [data-cv]').forEach(el => { el.hidden = el.dataset.cv !== st.setup; });
    for (const id of ['#domain', '#siteDomain']) { const dom = $(id); if (!dom.value && s.domain) dom.value = s.domain; }
    $('#feed').placeholder = s.configured && s.mode === 'feed' ? 'Feed link (saved; paste a new one to replace it)' : 'https://school.instructure.com/feeds/calendars/user_….ics';
    $('#token').placeholder = s.configured && s.mode === 'token' ? 'Access token (saved; blank keeps it)' : 'Access token';
    $('#back').hidden = !s.configured; $('#forget').hidden = !s.configured;
    if (showing()) ctx.island.layout();
    return;
  }
  const site = siteMode(), o = st.data;
  $('#readBtn').hidden = !site;
  $('#readBtn').classList.toggle('spin', !!st.reading);
  $('#readBtn').disabled = !!st.reading;
  // The sign-in card: before the first read, after a sign-out, or when the last read failed.
  const card = site && (!o || st.signedOut || st.readErr);
  $('#signin').hidden = !card; $('#grid').hidden = !o;
  if (card) {
    const first = !o;
    txt($('#signinTitle'), st.readErr && !st.signedOut ? 'Couldn\'t read Canvas' : first ? (st.reading ? 'Reading your Canvas…' : 'Sign in to Canvas') : 'Canvas signed you out');
    txt($('#signinText'), st.readErr && !st.signedOut
      ? `${st.readErr}${o ? ` What's here is from ${ago(new Date(o.at).toISOString())}.` : ''}`
      : first ? (st.reading ? 'This takes a few seconds.' : `Open Canvas in the Command Center and sign in the way you always do (${s.domain}). Your due dates, grades and announcements show up here after.`)
        : `Sign in again to bring this up to date. What's here is from ${ago(new Date(o.at).toISOString())}.`);
    $('#signinBtn').hidden = !!st.reading && first;
    txt($('#signinBtn'), st.readErr && !st.signedOut ? 'Try again' : 'Sign in');
  }
  if (!o) { txt($('#who'), [s.name, s.domain].filter(Boolean).join(' · ')); if (showing()) ctx.island.layout(); return; }
  const when = site ? (st.reading ? 'reading…' : `read ${ago(new Date(o.at).toISOString())}`) : '';
  txt($('#who'), [o.name, o.domain, o.limited ? 'calendar feed: due dates only' : '', when].filter(Boolean).join(' · '));
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
  setHtml($('#grades'), `<header><b>Grades</b><small>current</small></header>` + (o.courses.length ? o.courses.map(c => `<div class="cv-course" data-url="${esc(c.url + '/grades')}" title="${esc(c.name)}"><span class="cv-bar" style="background:${esc(c.color || 'var(--accent)')}"></span><div><b>${esc(c.code || c.name)}</b><small>${esc(c.code ? c.name : '')}</small></div><em>${c.score != null ? `${c.score.toFixed(1)}%` : '–'}${c.grade ? `<small> ${esc(c.grade)}</small>` : ''}</em>${c.score != null ? `<i class="cv-meter" style="--p:${Math.max(0, Math.min(100, c.score))}%;--c:${esc(c.color || 'var(--accent)')}"></i>` : ''}</div>`).join('') : '<p class="empty">No active courses.</p>'));
  const recent = o.recent || [];
  $('#recent').hidden = o.limited || !recent.length;
  setHtml($('#recent'), recent.length ? `<header><b>Recently graded</b><small>last 3 weeks</small></header>` + recent.slice(0, 6).map(g => {
    const c = course(g.course_id), [score, tone] = scoreOf(g);
    return `<div class="cv-course cv-graded" data-url="${esc(g.url)}" title="${esc(g.title)}"><span class="cv-bar" style="background:${esc(colorOf(g.course_id))}"></span><div><b>${esc(g.title)}</b><small>${esc([c.code || c.name || '', g.graded ? ago(g.graded) : '', g.late ? 'late' : ''].filter(Boolean).join(' · '))}</small></div><span class="cv-chip ${tone}">${esc(score)}</span></div>`;
  }).join('') : '');
  setHtml($('#ann'), `<header><b>Announcements</b><small>last 3 weeks</small></header>` + (o.announcements.length ? o.announcements.slice(0, 5).map(a => { const c = course(a.course_id); return `<div class="cv-an" data-url="${esc(a.url)}"><div><i style="background:${esc(c.color || 'var(--accent)')}"></i><b>${esc(a.title)}</b></div><small>${esc(c.code || c.name || '')} · ${esc(ago(a.posted))}</small>${a.preview ? `<p>${esc(a.preview)}</p>` : ''}</div>`; }).join('') : '<p class="empty">No recent announcements.</p>'));
  if (showing()) ctx.island.layout();
}

/** Opens a page on the user's own Canvas in the default browser (never anything else). */
function openPage(url) {
  const domain = (st.status && st.status.domain) || '';
  if (!domain || !String(url).startsWith(`https://${domain}/`)) { note('That isn\'t a page on your Canvas.'); return; }
  ctx.openWeb(url).then(() => ctx.island.closePanel(true)).catch(err => note(errText(err)));
}

/** Docked Canvas, where the user signs in (the sign-in is what the reader reads with). */
const openCanvas = () => ctx.open('cube:canvas:web');

/** Ticks an item off: signed in on the site, in Canvas through the reader, then in the last read. */
function toggleDone(t) {
  t.done = !t.done; render();
  const p = siteMode()
    ? ctx.sites.read('web', { action: 'done', kind: t.kind, id: t.id, override_id: t.override_id, done: t.done })
      .then(r => ctx.call('done', { key: t.key, kind: t.kind, id: t.id, override_id: (r && r.id) || t.override_id, done: t.done }))
    : ctx.call('done', { key: t.key, kind: t.kind, id: t.id, override_id: t.override_id, done: t.done });
  p.then(id => { if (id) t.override_id = id; remember(); }).catch(err => { t.done = !t.done; render(); note(errText(err)); });
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#main').addEventListener('click', e => {
    const check = e.target.closest('.cv-check'), r = e.target.closest('.cv-row');
    if (check && r) {
      const t = st.data[r.dataset.list][+r.dataset.i]; if (t) toggleDone(t);
      return;
    }
    const url = r ? (st.data[r.dataset.list][+r.dataset.i] || {}).url : (e.target.closest('[data-url]') || {}).dataset?.url;
    if (url) openPage(url);
  });
  $('#signinBtn').addEventListener('click', () => {
    if (st.readErr && !st.signedOut) { st.readErr = ''; readSite(); return; }
    openCanvas();
  });
  $('#readBtn').addEventListener('click', () => { note(''); readSite(); });
  $('#siteForm').addEventListener('submit', e => {
    e.preventDefault();
    const domain = $('#siteDomain').value.trim();
    if (!domain) { note('Add your school’s Canvas address first.'); return; }
    note(''); $('#siteSave').disabled = true;
    ctx.call('configure_site', { domain }).then(s => {
      st.status = s; st.edit = false; st.data = null; st.signedOut = false; st.readErr = '';
      ctx.store.set('configured', true);
      render(); openCanvas();
    }).catch(err => note(errText(err))).finally(() => { $('#siteSave').disabled = false; });
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
  $('#editBtn').addEventListener('click', () => { st.edit = true; st.picked = false; render(); });
  $('#back').addEventListener('click', () => { st.edit = false; $('#token').value = ''; render(); });
  $('#forget').addEventListener('click', () => {
    // A second click within 3 s confirms.
    const b = $('#forget');
    if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget'; }, 3000); return; }
    delete b.dataset.sure; b.textContent = 'Forget';
    ctx.call('forget').then(() => { st.edit = false; st.data = null; st.signedOut = false; st.readErr = ''; $('#domain').value = ''; $('#siteDomain').value = ''; $('#feed').value = ''; loop(); });
  });
}

export function activate(c) {
  ctx = c;
  // How to connect, kept from 0.1.x's island.cvSetup the first time; signing in is the default now.
  let legacy = canRead() ? 'site' : 'feed';
  try { legacy = JSON.parse(localStorage.getItem('island.cvSetup') || JSON.stringify(legacy)); } catch (e) {}
  st.setup = ctx.store.get('setup', legacy);
  st.signedOut = ctx.store.get('signedOut', false);
  ctx.tabs.register({ id: 'canvas', mount, show: loop, hide: () => clearTimeout(T) });
  // Whether the docked site can open yet (the Command Center loads this UI for the cube's stat).
  ctx.call('status').then(s => { st.status = s; ctx.store.set('configured', !!s.configured); }).catch(() => {});
  // Every 10 minutes the backend's tick asks for a read (it can't reach the site's sign-in itself).
  ctx.on('read', () => {
    ctx.call('status').then(s => { st.status = s; if (siteMode() && !(st.data && Date.now() - st.data.at < 6e4)) readSite(); }).catch(() => {});
  });
  ctx.cubes.register({
    id: 'canvas',
    stat: () => {
      if (siteMode() && st.signedOut) return 'Sign in to Canvas';
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
