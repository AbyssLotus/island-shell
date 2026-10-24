// The Command Center's calendar (the Today cube): month, week, day and agenda views over your own
// events, calendar feeds you add (Google / iCloud / Outlook iCal links), Canvas due dates and
// Destiny's weekly reset. Data comes from calendar.rs; times arrive as UTC ms and are shown in this
// PC's time zone.

const H = 48; // px per hour in the week and day views
const SWATCHES = ['#c9e9ff', '#7fb2ff', '#ff9a8a', '#9be38a', '#f5c26b', '#c79bff', '#6ee0d0', '#ff8fc8'];
const DAYMS = 864e5;

const startOfDay = t => { const d = new Date(t); d.setHours(0, 0, 0, 0); return d.getTime(); };
const addDays = (t, n) => { const d = new Date(t); d.setDate(d.getDate() + n); return d.getTime(); };
const startOfWeek = t => { const d = new Date(startOfDay(t)); d.setDate(d.getDate() - d.getDay()); return d.getTime(); };
const startOfMonth = t => { const d = new Date(t); d.setHours(0, 0, 0, 0); d.setDate(1); return d.getTime(); };
const sameDay = (a, b) => startOfDay(a) === startOfDay(b);
const fmtTime = t => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }).replace(':00', '').replace(' ', ' ');
const fmtDay = (t, o) => new Date(t).toLocaleDateString([], o || { weekday: 'short', month: 'short', day: 'numeric' });
const pad = n => String(n).padStart(2, '0');
const dateInput = t => { const d = new Date(t); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
const timeInput = t => { const d = new Date(t); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; };
const fromInputs = (date, time) => { const [y, m, d] = date.split('-').map(Number); const [hh, mm] = (time || '00:00').split(':').map(Number); return new Date(y, m - 1, d, hh, mm).getTime(); };

export function createCalendar({ invoke, esc, ic, store, S, flash }) {
  let el = null, view = store.get('calView', 'month'), cursor = startOfDay(Date.now());
  let events = [], sources = [], loading = 0, nowT = 0, adding = false;

  function rangeFor() {
    if (view === 'month') { const s = startOfWeek(startOfMonth(cursor)); return [s, addDays(s, 42)]; }
    if (view === 'week') { const s = startOfWeek(cursor); return [s, addDays(s, 7)]; }
    if (view === 'day') return [cursor, addDays(cursor, 1)];
    return [startOfDay(Date.now()), addDays(startOfDay(Date.now()), 60)];
  }
  async function load() {
    const r = rangeFor(), now = Date.now(), id = ++loading;
    const from = Math.min(r[0], startOfDay(now)), to = Math.max(r[1], addDays(startOfDay(now), 14));
    try {
      const [ev, src] = await Promise.all([invoke('calendar_events', { from, to }), invoke('calendar_sources')]);
      if (id !== loading || !el) return;
      events = ev; sources = src; render();
    } catch (e) { flash(String(e)); }
  }
  const evsOn = day => events.filter(e => e.start < addDays(day, 1) && Math.max(e.end, e.start + 1) > day);
  const srcName = id => (sources.find(s => s.id === id) || {}).name || '';

  /* ---------- rendering ---------- */
  function title() {
    if (view === 'month') return fmtDay(cursor, { month: 'long', year: 'numeric' });
    if (view === 'week') { const s = startOfWeek(cursor), e = addDays(s, 6); return `${fmtDay(s, { month: 'short', day: 'numeric' })} – ${fmtDay(e, { month: new Date(s).getMonth() === new Date(e).getMonth() ? undefined : 'short', day: 'numeric' })}, ${new Date(e).getFullYear()}`; }
    if (view === 'day') return fmtDay(cursor, { weekday: 'long', month: 'long', day: 'numeric' });
    return 'Next 60 days';
  }
  function render() {
    if (!el) return;
    const keepScroll = el.querySelector('.cal-scroll') ? el.querySelector('.cal-scroll').scrollTop : null;
    el.innerHTML = `<div class="cal-main">
        <div class="cal-bar">
          <button class="cc-btn" type="button" data-cal="today">Today</button>
          <button class="cal-ib" type="button" data-cal="prev" aria-label="Previous">${ic('back')}</button>
          <button class="cal-ib flip" type="button" data-cal="next" aria-label="Next">${ic('back')}</button>
          <h2>${esc(title())}</h2>
          <span class="cal-grow"></span>
          <div class="cal-views" role="group" aria-label="View">${[['month', 'Month'], ['week', 'Week'], ['day', 'Day'], ['agenda', 'Agenda']].map(([k, n]) => `<button type="button" data-view="${k}" aria-pressed="${k === view}">${n}</button>`).join('')}</div>
          <button class="cc-btn primary" type="button" data-cal="new">${ic('plus')}New event</button>
        </div>
        <div class="cal-body">${view === 'month' ? monthHtml() : view === 'agenda' ? agendaHtml() : gridHtml(view === 'week' ? 7 : 1)}</div>
      </div>
      <aside class="cal-rail">${railHtml()}</aside>
      <div class="cal-pop" hidden></div>
      <div class="cal-drawer" hidden></div>`;
    const sc = el.querySelector('.cal-scroll');
    if (sc) sc.scrollTop = keepScroll != null ? keepScroll : 7 * H - 10;
    tickNow();
  }
  function chip(e, day) {
    const allDay = e.all_day || e.end - e.start >= DAYMS;
    const cont = allDay && !sameDay(e.start, day);
    return `<button class="cal-chip${allDay ? ' allday' : ''}" type="button" data-ev="${esc(e.id)}" style="--c:${esc(e.color)}" title="${esc(e.title)}">${allDay ? '' : `<i></i><time>${fmtTime(e.start)}</time>`}<span>${cont ? '↳ ' : ''}${esc(e.title)}</span></button>`;
  }
  function monthHtml() {
    const s = startOfWeek(startOfMonth(cursor)), month = new Date(cursor).getMonth(), today = startOfDay(Date.now());
    let h = `<div class="cal-month"><div class="cal-dows">${[0, 1, 2, 3, 4, 5, 6].map(i => `<span>${fmtDay(addDays(s, i), { weekday: 'short' })}</span>`).join('')}</div><div class="cal-cells">`;
    for (let i = 0; i < 42; i++) {
      const day = addDays(s, i), list = evsOn(day), more = list.length - 4;
      h += `<div class="cal-cell${new Date(day).getMonth() !== month ? ' is-out' : ''}${day === today ? ' is-today' : ''}" data-day="${day}"><b data-goto="${day}">${new Date(day).getDate()}</b>${list.slice(0, 4).map(e => chip(e, day)).join('')}${more > 0 ? `<button class="cal-more" type="button" data-goto="${day}">+${more} more</button>` : ''}</div>`;
    }
    return h + '</div></div>';
  }
  /** Lays out overlapping timed events side by side. */
  function columns(list) {
    const out = [], sorted = list.slice().sort((a, b) => a.start - b.start || b.end - a.end);
    let cluster = [], clusterEnd = 0;
    const flush = () => { const cols = []; for (const e of cluster) { let c = cols.findIndex(end => end <= e.start); if (c < 0) { c = cols.length; cols.push(0); } cols[c] = Math.max(e.end, e.start + 30 * 60e3); out.push({ e, c }); } for (const o of out.slice(-cluster.length)) o.n = cols.length; cluster = []; };
    for (const e of sorted) { if (cluster.length && e.start >= clusterEnd) flush(); cluster.push(e); clusterEnd = Math.max(clusterEnd, e.end, e.start + 30 * 60e3); }
    if (cluster.length) flush();
    return out;
  }
  function gridHtml(n) {
    const s = n === 7 ? startOfWeek(cursor) : cursor, today = startOfDay(Date.now());
    const days = Array.from({ length: n }, (_, i) => addDays(s, i));
    const allDay = days.map(d => evsOn(d).filter(e => e.all_day || e.end - e.start >= DAYMS));
    // The day headers and all-day row sit inside the scroller (sticky), so their columns line up
    // with the hours below whatever the scrollbar's width.
    let h = `<div class="cal-grid" style="--n:${n}"><div class="cal-scroll"><div class="cal-sticky"><div class="cal-ghead"><span></span>${days.map(d => `<button type="button" class="cal-dh${d === today ? ' is-today' : ''}" data-goto="${d}"><small>${fmtDay(d, { weekday: 'short' })}</small><b>${new Date(d).getDate()}</b></button>`).join('')}</div>`;
    h += `<div class="cal-allday"><span>all day</span>${allDay.map((l, i) => `<div data-day="${days[i]}">${l.map(e => chip(e, days[i])).join('')}</div>`).join('')}</div></div>`;
    h += `<div class="cal-tgrid"><div class="cal-times">${Array.from({ length: 24 }, (_, i) => `<span style="top:${i * H}px">${i ? fmtTime(new Date(2000, 0, 1, i).getTime()) : ''}</span>`).join('')}</div>`;
    h += days.map(d => {
      const timed = evsOn(d).filter(e => !(e.all_day || e.end - e.start >= DAYMS));
      const blocks = columns(timed).map(({ e, c, n: cols }) => {
        const st = Math.max(e.start, d), en = Math.min(Math.max(e.end, e.start + 30 * 60e3), addDays(d, 1));
        const top = (st - d) / 36e5 * H, ht = Math.max(22, (en - st) / 36e5 * H - 2);
        return `<button class="cal-block" type="button" data-ev="${esc(e.id)}" style="--c:${esc(e.color)};top:${top}px;height:${ht}px;left:calc(${(c / cols * 100).toFixed(3)}% + 2px);width:calc(${(100 / cols).toFixed(3)}% - 4px)"><b>${esc(e.title)}</b><small>${e.end > e.start ? `${fmtTime(e.start)} – ${fmtTime(e.end)}` : fmtTime(e.start)}${e.location ? ' · ' + esc(e.location) : ''}</small></button>`;
      }).join('');
      return `<div class="cal-col${d === today ? ' is-today' : ''}" data-col="${d}">${blocks}${d === today ? '<i class="cal-now"></i>' : ''}</div>`;
    }).join('');
    return h + '</div></div></div>';
  }
  function agendaHtml() {
    const start = startOfDay(Date.now());
    let h = '<div class="cal-agenda">', any = false;
    for (let i = 0; i < 60; i++) {
      const d = addDays(start, i), list = evsOn(d);
      if (!list.length) continue;
      any = true;
      h += `<section><h3><b>${new Date(d).getDate()}</b><span>${i === 0 ? 'Today' : i === 1 ? 'Tomorrow' : fmtDay(d, { weekday: 'long' })}<small>${fmtDay(d, { month: 'long', year: 'numeric' })}</small></span></h3><div class="cal-arows">${list.map(e => `<button class="cal-arow" type="button" data-ev="${esc(e.id)}" style="--c:${esc(e.color)}"><i></i><time>${e.all_day ? 'All day' : fmtTime(e.start)}</time><span><b>${esc(e.title)}</b><small>${esc([e.location, srcName(e.source)].filter(Boolean).join(' · '))}</small></span></button>`).join('')}</div></section>`;
    }
    return h + (any ? '' : '<p class="cal-empty">Nothing in the next 60 days.</p>') + '</div>';
  }
  function railHtml() {
    const now = Date.now(), ms = startOfMonth(cursor), s = startOfWeek(ms), today = startOfDay(now);
    const w = store.get('weather', null), unit = S.unit === 'C' ? 'C' : 'F';
    const wx = w ? `${Math.round(unit === 'C' ? w.temp : w.temp * 9 / 5 + 32)}°${unit} · ${esc(w.name || '')}` : '';
    const next = events.filter(e => (e.all_day ? Math.max(e.end, e.start + DAYMS) : Math.max(e.end, e.start)) > now).slice(0, 6);
    let mini = `<div class="cal-mini"><div class="cal-mini-h">${esc(fmtDay(ms, { month: 'long', year: 'numeric' }))}</div><div class="cal-mini-g">${['S', 'M', 'T', 'W', 'T', 'F', 'S'].map(x => `<span>${x}</span>`).join('')}`;
    for (let i = 0; i < 42; i++) { const d = addDays(s, i), has = evsOn(d).length; mini += `<button type="button" data-goto="${d}" class="${new Date(d).getMonth() !== new Date(ms).getMonth() ? 'is-out' : ''}${d === today ? ' is-today' : ''}${sameDay(d, cursor) && view !== 'agenda' ? ' is-sel' : ''}">${new Date(d).getDate()}${has ? '<i></i>' : ''}</button>`; }
    mini += '</div></div>';
    return `<div class="cal-now-card"><b>${esc(fmtDay(now, { weekday: 'long' }))}</b><span>${esc(fmtDay(now, { month: 'long', day: 'numeric' }))}${wx ? ' · ' + wx : ''}</span></div>${mini}
      <h4>Up next</h4><div class="cal-next">${next.length ? next.map(e => `<button type="button" data-ev="${esc(e.id)}" style="--c:${esc(e.color)}"><i></i><span><b>${esc(e.title)}</b><small>${sameDay(e.start, now) ? 'Today' : fmtDay(e.start)}${e.all_day ? '' : ' · ' + fmtTime(e.start)}</small></span></button>`).join('') : '<p class="cal-empty">Nothing coming up.</p>'}</div>
      <h4>Calendars</h4><div class="cal-srcs">${sources.map(s2 => `<div class="cal-src${s2.on ? '' : ' off'}"><button type="button" data-src="${esc(s2.id)}" style="--c:${esc(s2.color)}" aria-pressed="${s2.on}" title="Show or hide"><i></i>${esc(s2.name)}</button>${s2.error ? `<small class="cal-err" title="${esc(s2.error)}">!</small>` : ''}${s2.kind === 'feed' ? `<button class="cal-x" type="button" data-rmfeed="${esc(s2.id)}" title="Remove this calendar">${ic('x')}</button>` : ''}</div>`).join('')}</div>
      ${adding ? `<form class="cal-add" data-addfeed><input type="text" name="name" placeholder="Name (optional)" spellcheck="false"><input type="password" name="url" placeholder="iCal link (https:// or webcal://)" spellcheck="false" required><div><button class="cc-btn primary" type="submit">Add</button><button class="cc-btn" type="button" data-cal="cancel-add">Cancel</button></div><p><b>Google Calendar:</b> Settings → your calendar → Integrate calendar → <em>Secret address in iCal format</em>.<br><b>iCloud:</b> Calendar → share next to the calendar → Public Calendar → copy the link.<br><b>Outlook:</b> Settings → Calendar → Shared calendars → Publish a calendar → ICS link.<br>The link is stored encrypted on this PC.</p></form>` : `<button class="cc-btn wide" type="button" data-cal="add">${ic('plus')}Add a calendar</button>`}`;
  }
  function tickNow() {
    if (!el) return;
    const line = el.querySelector('.cal-now'); if (!line) return;
    const d = startOfDay(Date.now()); line.style.top = ((Date.now() - d) / 36e5 * H) + 'px';
  }

  /* ---------- event details and editing ---------- */
  function showPop(e, anchor) {
    const pop = el.querySelector('.cal-pop'), r = anchor.getBoundingClientRect(), box = el.getBoundingClientRect();
    const when = e.all_day ? (e.end - e.start > DAYMS ? `${fmtDay(e.start)} – ${fmtDay(e.end - 1)}` : `${fmtDay(e.start, { weekday: 'long', month: 'long', day: 'numeric' })} · all day`) : `${fmtDay(e.start, { weekday: 'long', month: 'long', day: 'numeric' })} · ${fmtTime(e.start)}${e.end > e.start ? ' – ' + fmtTime(e.end) : ''}`;
    pop.innerHTML = `<div class="cal-pop-h" style="--c:${esc(e.color)}"><i></i><b>${esc(e.title)}</b><button class="cal-ib" type="button" data-cal="close-pop" aria-label="Close">${ic('x')}</button></div>
      <p class="cal-when">${esc(when)}</p>${e.location ? `<p class="cal-loc">${esc(e.location)}</p>` : ''}<p class="cal-from">${esc(srcName(e.source))}</p>
      ${e.notes ? `<p class="cal-notes">${esc(e.notes)}</p>` : ''}
      <div class="cal-pop-a">${e.url ? `<button class="cc-btn" type="button" data-openurl="${esc(e.url)}">${ic('globe')}Open</button>` : ''}${e.editable ? `<button class="cc-btn" type="button" data-edit="${esc(e.id)}">Edit</button><button class="cc-btn danger" type="button" data-del="${esc(e.id)}">Delete</button>` : ''}</div>`;
    pop.hidden = false;
    // Beside the event, or under it for wide rows (agenda, Up next).
    const pw = 320, wide = r.width > 360;
    let left = wide ? r.left - box.left + 110 : r.right - box.left + 8, top = wide ? r.bottom - box.top + 6 : r.top - box.top;
    if (!wide && left + pw > box.width - 12) left = r.left - box.left - pw - 8;
    pop.style.left = Math.max(12, Math.min(box.width - pw - 12, left)) + 'px';
    pop.style.top = Math.max(12, Math.min(box.height - pop.offsetHeight - 12, top)) + 'px';
  }
  function openEditor(ev) {
    const e = ev || { id: '', title: '', start: 0, end: 0, all_day: false, color: '', notes: '', location: '' };
    const dr = el.querySelector('.cal-drawer');
    el.querySelector('.cal-pop').hidden = true;
    const endDay = e.all_day ? e.end - DAYMS : e.end;
    dr.innerHTML = `<form data-editor><h3>${e.id ? 'Edit event' : 'New event'}</h3>
      <input class="cal-in big" name="title" placeholder="Event name" value="${esc(e.title)}" required spellcheck="false">
      <label class="cal-check"><input type="checkbox" name="all_day" ${e.all_day ? 'checked' : ''}>All day</label>
      <div class="cal-row"><label>Starts<input class="cal-in" type="date" name="sd" value="${dateInput(e.start)}" required></label><label class="cal-t">at<input class="cal-in" type="time" name="st" value="${timeInput(e.start)}"></label></div>
      <div class="cal-row"><label>Ends<input class="cal-in" type="date" name="ed" value="${dateInput(endDay)}" required></label><label class="cal-t">at<input class="cal-in" type="time" name="et" value="${timeInput(e.end)}"></label></div>
      <input class="cal-in" name="location" placeholder="Where" value="${esc(e.location)}" spellcheck="false">
      <textarea class="cal-in" name="notes" rows="4" placeholder="Notes">${esc(e.notes)}</textarea>
      <div class="cal-sw">${SWATCHES.map(c => `<label style="--c:${c}"><input type="radio" name="color" value="${c}" ${(e.color || SWATCHES[0]) === c ? 'checked' : ''}><i></i></label>`).join('')}</div>
      <div class="cal-row end">${e.id ? `<button class="cc-btn danger" type="button" data-del="${esc(e.id)}">Delete</button><span class="cal-grow"></span>` : '<span class="cal-grow"></span>'}<button class="cc-btn" type="button" data-cal="close-editor">Cancel</button><button class="cc-btn primary" type="submit">Save</button></div>
      <input type="hidden" name="id" value="${esc(e.id)}"></form>`;
    dr.hidden = false;
    const f = dr.querySelector('form'), syncAll = () => f.querySelectorAll('.cal-t').forEach(l => { l.hidden = f.all_day.checked; });
    f.all_day.addEventListener('change', syncAll); syncAll();
    setTimeout(() => f.title.focus(), 30);
  }
  async function saveEditor(f) {
    const all = f.all_day.checked;
    const start = all ? fromInputs(f.sd.value, '00:00') : fromInputs(f.sd.value, f.st.value);
    let end = all ? addDays(fromInputs(f.ed.value, '00:00'), 1) : fromInputs(f.ed.value, f.et.value);
    if (!all && end < start) end = start;
    const event = { id: f.id.value, title: f.title.value, start, end, all_day: all, color: (f.querySelector('[name="color"]:checked') || {}).value || '', notes: f.notes.value, location: f.location.value };
    try { await invoke('calendar_save', { event }); el.querySelector('.cal-drawer').hidden = true; flash(event.id ? 'Event saved' : 'Event added'); load(); } catch (e) { flash(String(e)); }
  }

  /* ---------- input ---------- */
  function go(n) {
    if (view === 'month') { const d = new Date(cursor); d.setDate(1); d.setMonth(d.getMonth() + n); cursor = d.getTime(); }
    else if (view === 'week') cursor = addDays(cursor, 7 * n);
    else if (view === 'day') cursor = addDays(cursor, n);
    load(); render();
  }
  function onClick(e) {
    const t = e.target;
    const a = t.closest('[data-cal]');
    if (a) {
      const k = a.dataset.cal;
      if (k === 'today') { cursor = startOfDay(Date.now()); load(); render(); }
      else if (k === 'prev') go(-1); else if (k === 'next') go(1);
      else if (k === 'new') { const s = Math.max(Date.now(), cursor); const st = new Date(s); st.setMinutes(0, 0, 0); st.setHours(st.getHours() + 1); openEditor({ id: '', title: '', start: st.getTime(), end: st.getTime() + 36e5, all_day: false, color: '', notes: '', location: '' }); }
      else if (k === 'add') { adding = true; render(); setTimeout(() => { const i = el.querySelector('.cal-add [name="url"]'); if (i) i.focus(); }, 30); }
      else if (k === 'cancel-add') { adding = false; render(); }
      else if (k === 'close-pop') el.querySelector('.cal-pop').hidden = true;
      else if (k === 'close-editor') el.querySelector('.cal-drawer').hidden = true;
      return;
    }
    const v = t.closest('[data-view]'); if (v) { view = v.dataset.view; store.set('calView', view); load(); render(); return; }
    const ev = t.closest('[data-ev]'); if (ev) { const x = events.find(z => z.id === ev.dataset.ev); if (x) showPop(x, ev); return; }
    const gd = t.closest('[data-goto]'); if (gd) { cursor = +gd.dataset.goto; if (view === 'month' && !t.closest('.cal-mini')) { view = 'day'; store.set('calView', view); } else if (view === 'agenda') { view = 'day'; } load(); render(); return; }
    const src = t.closest('[data-src]'); if (src) { invoke('calendar_set_source', { id: src.dataset.src, on: src.getAttribute('aria-pressed') !== 'true' }).then(() => load()); return; }
    const rm = t.closest('[data-rmfeed]');
    if (rm) { if (!rm.dataset.sure) { rm.dataset.sure = 1; rm.classList.add('sure'); rm.title = 'Click again to remove'; return; } invoke('calendar_remove_feed', { id: rm.dataset.rmfeed }).then(() => { flash('Calendar removed'); load(); }); return; }
    const ed = t.closest('[data-edit]'); if (ed) { const x = events.find(z => z.id === ed.dataset.edit); if (x) openEditor(x); return; }
    const del = t.closest('[data-del]'); if (del) { if (!del.dataset.sure) { del.dataset.sure = 1; del.textContent = 'Click to confirm'; return; } invoke('calendar_delete', { id: del.dataset.del }).then(() => { el.querySelector('.cal-pop').hidden = true; el.querySelector('.cal-drawer').hidden = true; flash('Event deleted'); load(); }); return; }
    const url = t.closest('[data-openurl]'); if (url) { invoke('calendar_open', { url: url.dataset.openurl }).catch(err => flash(String(err))); return; }
    // An empty slot in the week/day grid: new event there (on the half hour).
    const col = t.closest('.cal-col');
    if (col && !t.closest('.cal-block')) { const r = col.getBoundingClientRect(), mins = Math.floor((e.clientY - r.top) / H * 2) * 30, s = +col.dataset.col + mins * 60e3; openEditor({ id: '', title: '', start: s, end: s + 36e5, all_day: false, color: '', notes: '', location: '' }); return; }
    const cell = t.closest('.cal-cell'); if (cell && !t.closest('button')) { const s = +cell.dataset.day + 9 * 36e5; openEditor({ id: '', title: '', start: s, end: s + 36e5, all_day: false, color: '', notes: '', location: '' }); return; }
    if (!t.closest('.cal-pop')) { const p = el.querySelector('.cal-pop'); if (p) p.hidden = true; }
  }
  function onSubmit(e) {
    const f = e.target;
    if (f.matches('[data-editor]')) { e.preventDefault(); saveEditor(f); }
    if (f.matches('[data-addfeed]')) {
      e.preventDefault();
      const btn = f.querySelector('[type="submit"]'); btn.disabled = true; btn.textContent = 'Adding…';
      invoke('calendar_add_feed', { name: f.name.value, url: f.url.value }).then(() => { adding = false; flash('Calendar added'); load(); }).catch(err => { flash(String(err)); btn.disabled = false; btn.textContent = 'Add'; });
    }
  }
  /** Keys while the calendar shows; true when it used the key. */
  function key(e) {
    if (!el) return false;
    const typing = e.target.matches && e.target.matches('input, textarea, select');
    if (e.key === 'Escape') {
      const dr = el.querySelector('.cal-drawer'), pop = el.querySelector('.cal-pop');
      if (!dr.hidden) { dr.hidden = true; return true; }
      if (!pop.hidden) { pop.hidden = true; return true; }
      return false;
    }
    if (typing) return false;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') { go(e.key === 'ArrowRight' ? 1 : -1); return true; }
    if (e.key === 't' || e.key === 'T') { cursor = startOfDay(Date.now()); load(); render(); return true; }
    const v = { m: 'month', w: 'week', d: 'day', a: 'agenda' }[e.key.toLowerCase()];
    if (v && !e.ctrlKey && !e.altKey) { view = v; store.set('calView', view); load(); render(); return true; }
    if (e.key === 'n' || e.key === 'N') { el.querySelector('[data-cal="new"]').click(); return true; }
    return false;
  }

  return {
    mount(host) {
      el = document.createElement('div'); el.className = 'cal';
      el.addEventListener('click', onClick); el.addEventListener('submit', onSubmit);
      host.append(el); render(); load();
      clearInterval(nowT); nowT = setInterval(tickNow, 30000);
    },
    unmount() { clearInterval(nowT); if (el) el.remove(); el = null; },
    key
  };
}
