// The Command Center's Task Manager (the Stats cube): a window over the eclipse with Summary,
// Performance, Processes, System Info, App history, Startup apps, Users and Services, then the deep
// dives: Power & Freq, Connections, Installed Apps, Disk Space and Benchmarks. Live data comes from
// taskmgr.rs (sampled once a second, only while this is open) and the Stats tab's sensors; each page
// asks for its own data while it shows. Drag it by its title bar, resize it from the corner,
// double-click the title bar to fill the screen.
//
// Pages other than Processes, Performance and Startup apps live in tm-overview.js, tm-system.js
// and tm-tools.js; shared formats and SVG are in tm-util.js.

import { N, GB, MB, clamp, fmtMem, fmtIO, fmtRate, fmtBits, fmtUp, fmtGHz, fmtLink, netKind, heat, makeHist, graph, spark, leds, kv, line } from './tm-util.js';
import { createOverview } from './tm-overview.js';
import { createSystemPages } from './tm-system.js';
import { createTools } from './tm-tools.js';

const SVG = p => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${p}</svg>`;
export const TI = {
  max: SVG('<rect x="4" y="4" width="16" height="16" rx="2.5"/>'),
  restore: SVG('<rect x="4" y="8" width="12" height="12" rx="2"/><path d="M8 8V6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2"/>'),
  leaf: SVG('<path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z"/><path d="M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12"/>'),
  chev: SVG('<path d="m9 18 6-6-6-6"/>'),
  home: SVG('<path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V20h14V9.5"/><path d="M10 20v-5h4v5"/>'),
  info: SVG('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'),
  history: SVG('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>'),
  users: SVG('<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0"/><path d="M16 4.6a3.5 3.5 0 0 1 0 6.8M18.5 20a6.5 6.5 0 0 0-3-5.5"/>'),
  wrench: SVG('<path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3-6 0 0-6 3 3 8-8Z" transform="scale(.9) translate(1.3 1.3)"/>'),
  bolt: SVG('<path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z"/>'),
  globe: SVG('<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3Z"/>'),
  list: SVG('<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>'),
  disk: SVG('<rect x="2" y="6" width="20" height="12" rx="2.5"/><path d="M6 12h.01M10 12h8"/>'),
  gauge: SVG('<path d="M12 14 16 9"/><path d="M4.9 19a9 9 0 1 1 14.2 0"/><circle cx="12" cy="14" r="1.5"/>'),
  play: SVG('<path d="M7 4v16l13-8z"/>'),
  stop: SVG('<rect x="6" y="6" width="12" height="12" rx="1.5"/>'),
  pause: SVG('<path d="M8 5v14M16 5v14"/>'),
  copy: SVG('<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M4 16V5a2 2 0 0 1 2-2h11"/>'),
  plus: SVG('<path d="M12 5v14M5 12h14"/>'),
  cols: SVG('<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>'),
  cpu: SVG('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>'),
  mem: SVG('<rect x="2" y="7" width="20" height="10" rx="1.5"/><path d="M6 7v10M10 7v10M14 7v10M18 7v10M2 20h20"/>'),
  monitor: SVG('<rect x="2" y="4" width="20" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>'),
  net: SVG('<path d="M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M2 9a15 15 0 0 1 20 0M12 20h.01"/>'),
  thermo: SVG('<path d="M14 14.8V4a2 2 0 0 0-4 0v10.8a4 4 0 1 0 4 0Z"/>'),
  trophy: SVG('<path d="M8 21h8M12 17v4M7 4h10v5a5 5 0 0 1-10 0Z"/><path d="M17 5h3v2a3 3 0 0 1-3 3M7 5H4v2a3 3 0 0 0 3 3"/>'),
  shield: SVG('<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/>'),
  ext: SVG('<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>')
};

const PAGES = [
  ['summary', 'Summary', 'home'], ['perf', 'Performance', 'pulse'], ['procs', 'Processes', 'grid'], ['system', 'System Info', 'info'],
  ['history', 'App history', 'history'], ['startup', 'Startup apps', 'rocket'], ['users', 'Users', 'users'], ['services', 'Services', 'wrench'],
  null,
  ['sensors', 'Power & Freq', 'bolt'], ['conns', 'Connections', 'globe'], ['installed', 'Installed Apps', 'list'], ['disk', 'Disk Space', 'disk'], ['bench', 'Benchmarks', 'gauge']
];
const SPEEDS = [[1000, 'Every second'], [3000, 'Every 3 seconds'], [0, 'Paused']];
const PCOLS = [
  ['name', 'Name', 'Name'], ['pid', 'PID', 'Process ID'], ['status', 'Status', 'Suspended or in efficiency mode'], ['user', 'User name', 'Who runs it'],
  ['cpu', 'CPU', 'Share of the processor'], ['mem', 'Memory', 'Private memory (what Task Manager calls Memory)'], ['gpu', 'GPU', 'Busiest graphics engine'],
  ['read', 'Disk read', 'Bytes read per second (from disks, devices and the network stack)'], ['write', 'Disk write', 'Bytes written per second'],
  ['threads', 'Threads', 'Threads'], ['handles', 'Handles', 'Handles'], ['cmd', 'Command', 'The command line it was started with']
];
const DEF_COLS = ['name', 'status', 'cpu', 'mem', 'gpu', 'read', 'write'];
const SECTIONS = [['app', 'Apps'], ['background', 'Background processes'], ['windows', 'Windows processes']];

export function createTaskManager({ invoke, esc, ic: icBase, store, flash, assetUrl, root }) {
  const ic = n => (TI[n] || icBase(n));
  const el = document.createElement('section');
  el.className = 'tmw'; el.hidden = true; el.setAttribute('role', 'dialog'); el.setAttribute('aria-label', 'Task Manager');
  el.innerHTML = `<header class="tmw-bar"><span class="tmw-ic">${ic('pulse')}</span><b>Task Manager</b><small class="tmw-sum"></small>
      <button class="tmw-b" type="button" data-tm="max" title="Fill the screen" aria-label="Fill the screen">${TI.max}</button><button class="tmw-b x" type="button" data-tm="close" title="Close (Esc)" aria-label="Close">${ic('x')}</button></header>
    <div class="tmw-body"><nav class="tm-nav" aria-label="Task Manager"><div class="tm-navl" role="tablist"></div>
        <label class="tm-speed" title="How often the numbers update">${ic('clock')}<select aria-label="Update speed">${SPEEDS.map(([v, n]) => `<option value="${v}">${n}</option>`).join('')}</select></label></nav>
      <div class="tm-main"></div></div>
    <footer class="tm-status"><span class="tm-live"><i></i><span></span></span><span class="grow"></span><span class="tm-clock" title="Up time"></span></footer>`;
  root.append(el);
  const $ = s => el.querySelector(s);
  const main = $('.tm-main');

  let isOpen = false, timer = 0, snap = null, stats = null, cpuInfo = null, sysInfo = null, sysWait = null, page = store.get('tmPage', 'summary'), perfSel = 'cpu';
  let speed = store.get('tmSpeed', 1000);
  let sort = store.get('tmSort', { k: 'cpu', d: -1 }), q = '', sel = null, confirm = false, hovering = false, order = [], footKey = '';
  let pcols = store.get('tmCols', DEF_COLS), flat = store.get('tmFlat', false), colsOpen = false, cpuView = store.get('tmCpuView', 'logical');
  let startup = null, built = '';
  const expanded = new Set();
  const H = makeHist(), hist = H.h, push = H.push;
  $('.tm-speed select').value = String(speed);

  /* ---------- shared with the pages ---------- */
  const ctx = {
    invoke, esc, ic, store, flash, assetUrl, hist,
    get snap() { return snap; }, get stats() { return stats; }, get cpu() { return cpuInfo; }, get open() { return isOpen; }, get sysInfo() { return sysInfo; },
    /** System Info, read once per opening (it's a few hundred ms of work). */
    system() { if (sysInfo) return Promise.resolve(sysInfo); if (!sysWait) sysWait = invoke('tm_system').then(s => (sysInfo = s)).catch(e => { sysWait = null; throw e; }); return sysWait; },
    openPage(id) { go(id); },
    showProcess(pid) {
      const g = snap && snap.groups.find(x => x.procs.some(p => p.pid === pid)); if (!g) { flash('That process has ended'); return; }
      q = ''; flat = false; expanded.add(g.id); sel = { g: g.id, pid: g.procs.length > 1 ? pid : null }; go('procs');
      setTimeout(() => { const r = el.querySelector('.tm-procs tr.sel'); if (r) r.scrollIntoView({ block: 'center' }); }, 30);
    },
    rerender() { render(); }
  };
  const pages = { ...createOverview(ctx), ...createSystemPages(ctx), ...createTools(ctx) };

  /* ---------- window ---------- */
  function place() {
    const r = store.get('tmRect2', null), W = innerWidth, H2 = innerHeight;
    const w = clamp(r ? r.w : Math.min(1480, W - 96), 640, W - 32), h = clamp(r ? r.h : Math.min(920, H2 - 150), 420, H2 - 32);
    const x = clamp(r ? r.x : (W - w) / 2, 16, W - w - 16), y = clamp(r ? r.y : 76, 16, H2 - h - 16);
    Object.assign(el.style, { left: x + 'px', top: y + 'px', width: w + 'px', height: h + 'px' });
  }
  const save = () => { if (el.classList.contains('max')) return; const b = el.getBoundingClientRect(); store.set('tmRect2', { x: Math.round(b.left), y: Math.round(b.top), w: Math.round(b.width), h: Math.round(b.height) }); };
  let saveT = 0;
  new ResizeObserver(() => { if (isOpen) { clearTimeout(saveT); saveT = setTimeout(save, 300); el.classList.toggle('narrow', el.clientWidth < 980); } }).observe(el);
  const bar = $('.tmw-bar');
  bar.addEventListener('pointerdown', e => {
    if (e.button !== 0 || e.target.closest('button') || el.classList.contains('max')) return;
    const b = el.getBoundingClientRect(), ox = e.clientX - b.left, oy = e.clientY - b.top;
    bar.setPointerCapture(e.pointerId); bar.classList.add('dragging');
    const move = ev => { el.style.left = clamp(ev.clientX - ox, 8 - b.width + 120, innerWidth - 120) + 'px'; el.style.top = clamp(ev.clientY - oy, 8, innerHeight - 48) + 'px'; };
    const up = () => { bar.removeEventListener('pointermove', move); bar.removeEventListener('pointerup', up); bar.classList.remove('dragging'); save(); };
    bar.addEventListener('pointermove', move); bar.addEventListener('pointerup', up);
  });
  bar.addEventListener('dblclick', e => { if (!e.target.closest('button')) toggleMax(); });
  function toggleMax() {
    const on = el.classList.toggle('max');
    const b = $('[data-tm="max"]'); b.innerHTML = on ? TI.restore : TI.max; b.title = on ? 'Back to a window' : 'Fill the screen';
  }

  function open() {
    if (isOpen) { el.classList.remove('flash'); void el.offsetWidth; el.classList.add('flash'); return; }
    isOpen = true; place();
    el.hidden = false; el.classList.remove('out'); el.style.animation = 'none'; void el.offsetWidth; el.style.animation = '';
    el.classList.toggle('narrow', el.clientWidth < 980);
    confirm = false; built = ''; navFor = '';
    if (!PAGES.some(p => p && p[0] === page)) page = 'summary';
    invoke('tm_cpu_info').then(c => { cpuInfo = c; if (isOpen) render(); }).catch(() => {});
    render();
    tick();
    setTimeout(() => { const qi = $('.tm-q'); if (qi && page === 'procs') qi.focus({ preventScroll: true }); else el.focus({ preventScroll: true }); }, 60);
  }
  /** instant: the Command Center itself is closing. */
  function close(instant) {
    if (!isOpen) return;
    isOpen = false; clearTimeout(timer); hovering = false;
    if (pages[page] && pages[page].leave) pages[page].leave();
    for (const p of Object.values(pages)) if (p.closed) p.closed();
    H.clear();
    snap = null; stats = null; startup = null; sel = null; confirm = false; sysInfo = null; sysWait = null; colsOpen = false;
    if (instant) { el.hidden = true; return; }
    el.classList.add('out');
    setTimeout(() => { if (!isOpen) el.hidden = true; }, 190);
  }
  function go(id) {
    if (page === id && built) { render(); return; }
    if (pages[page] && pages[page].leave) pages[page].leave();
    page = id; store.set('tmPage', page); confirm = false; built = ''; colsOpen = false;
    if (page === 'startup') startup = null;
    render();
  }

  /* ---------- data ---------- */
  async function tick() {
    if (!isOpen) return;
    clearTimeout(timer);
    if (speed > 0) {
      try {
        const [sn, st] = await Promise.all([invoke('tm_snapshot'), invoke('get_stats')]);
        if (!isOpen) return;
        if (!stats && st && st.cpu_hist) for (const v of st.cpu_hist.slice(-N)) push('cpu', v);
        stats = st;
        if (sn && (!snap || sn.at !== snap.at)) { snap = sn; record(); }
        render();
      } catch (e) {}
    }
    timer = setTimeout(tick, speed > 0 ? speed : 1000);
  }
  function record() {
    const P = snap.perf, sen = (stats && stats.sensors) || {};
    push('cpu', P.cpu); push('kernel', P.kernel); push('mem', P.mem_total ? P.mem_used / P.mem_total * 100 : 0); push('gpu', P.gpu);
    push('mhz', P.mhz || sen.cpu_mhz); push('temp', sen.cpu_temp);
    P.cores.forEach((v, i) => push('core' + i, v));
    (P.cores_kernel || []).forEach((v, i) => push('kcore' + i, v));
    let dr = 0, dw = 0, rx = 0, tx = 0;
    for (const d of P.disks) { push('disk:' + d.name, d.active); dr += d.read; dw += d.write; }
    for (const n of P.nets) { push('rx:' + n.name, n.rx); push('tx:' + n.name, n.tx); rx += n.rx; tx += n.tx; }
    push('diskr', dr); push('diskw', dw); push('rx', rx); push('tx', tx);
    push('diskact', P.disks.length ? Math.max(...P.disks.map(d => d.active)) : 0);
    const g = (sen.gpus || []).find(x => !x.integrated) || (sen.gpus || [])[0];
    if (g) { push('gputemp', g.temp); push('gpupow', g.power_pct); }
    for (const p of Object.values(pages)) if (p.sample) p.sample();
  }

  /* ---------- rendering ---------- */
  let navFor = '';
  function render() {
    if (navFor !== page) {
      navFor = page;
      $('.tm-navl').innerHTML = PAGES.map(p => (p ? `<button type="button" role="tab" data-page="${p[0]}" aria-selected="${p[0] === page}" title="${p[1]}">${ic(p[2])}<span>${p[1]}</span></button>` : '<div class="tm-navsep"><span>Deep dives</span></div>')).join('');
    }
    if (snap) {
      const P = snap.perf, s = stats || {}, t = s.sensors && s.sensors.cpu_temp;
      $('.tmw-sum').textContent = `CPU ${Math.round(P.cpu)}%${t != null ? ' · ' + Math.round(t) + '°C' : ''} · Memory ${Math.round(P.mem_used / (P.mem_total || 1) * 100)}% · GPU ${Math.round(P.gpu)}%`;
      $('.tm-live span').textContent = `${speed ? (speed === 1000 ? 'Live' : 'Every 3 s') : 'Paused'} · ${P.processes} processes · ${P.threads.toLocaleString()} threads · sample ${snap.gen.toLocaleString()}`;
      if (s.uptime) $('.tm-clock').textContent = fmtUp(s.uptime);
    } else $('.tm-live span').textContent = 'Starting…';
    $('.tm-live').classList.toggle('paused', !speed);
    if (page === 'procs') renderProcs();
    else if (page === 'perf') renderPerf();
    else if (page === 'startup') renderStartup();
    else if (pages[page]) { const first = built !== page; built = page; pages[page].render(main, first); }
  }
  /** The top of a page: its icon and big title, then whatever goes on the right. */
  const top = (id, extra = '') => { const p = PAGES.find(x => x && x[0] === id); return `<div class="tm-top"><h4>${ic(p[2])}${p[1]}</h4>${extra}</div>`; };
  ctx.top = top;

  /* ---------- Processes ---------- */
  function buildProcs() {
    built = 'procs';
    main.innerHTML = top('procs', `<small class="tm-n"></small><span class="grow"></span>
        <div class="tm-seg" role="group" aria-label="Show"><button type="button" data-flat="0" aria-pressed="${!flat}">Grouped</button><button type="button" data-flat="1" aria-pressed="${flat}">Every process</button></div>
        <button class="cc-btn" type="button" data-tm="cols" aria-expanded="false">${TI.cols}Columns</button>
        <button class="cc-btn" type="button" data-tm="run">${TI.plus}Run new task</button>
        <label class="tm-search">${ic('search')}<input class="tm-q" type="text" placeholder="Name, user, command or PID" spellcheck="false" autocomplete="off" aria-label="Search processes"></label>`)
      + `<div class="tm-colmenu" hidden></div>
      <div class="tm-tbl"><table class="tm-procs"><colgroup></colgroup><thead><tr></tr></thead><tbody></tbody></table></div>
      <div class="tm-foot tm-detail"></div>`;
    const qi = $('.tm-q'); qi.value = q;
    qi.addEventListener('input', () => { q = qi.value; renderProcs(); });
    const tbl = $('.tm-tbl');
    tbl.addEventListener('pointerenter', () => { hovering = true; });
    tbl.addEventListener('pointerleave', () => { hovering = false; });
    // Everything here redraws each second, so presses (not clicks) do the work: a click whose
    // press and release land on two different redraws of a row would be lost.
    let lastPress = { id: '', t: 0 };
    tbl.addEventListener('pointerdown', e => {
      if (e.button !== 0) return;
      const ex = e.target.closest('[data-exp]');
      if (ex) { const id = ex.dataset.exp; expanded.has(id) ? expanded.delete(id) : expanded.add(id); renderProcs(); return; }
      const th = e.target.closest('th[data-sort]');
      if (th) { const k = th.dataset.sort; sort = sort.k === k ? { k, d: -sort.d } : { k, d: ['name', 'user', 'cmd', 'status'].includes(k) ? 1 : -1 }; store.set('tmSort', sort); order = []; renderProcs(); return; }
      const tr = e.target.closest('tr[data-g]'); if (!tr) return;
      const pid = tr.dataset.pid ? +tr.dataset.pid : null, id = tr.dataset.g + ':' + pid;
      if (pid == null && lastPress.id === id && e.timeStamp - lastPress.t < 400) { const g = groupOf(tr.dataset.g); if (g && g.procs.length > 1) { expanded.has(g.id) ? expanded.delete(g.id) : expanded.add(g.id); } lastPress = { id: '', t: 0 }; }
      else lastPress = { id, t: e.timeStamp };
      sel = { g: tr.dataset.g, pid }; confirm = false;
      renderProcs();
    });
    footKey = '';
  }
  const groupOf = id => snap && snap.groups.find(g => g.id === id);
  const status = x => (x.suspended ? 'Suspended' : x.eco ? 'Efficiency mode' : '');
  const val = (x, k) => (k === 'status' ? status(x) : k === 'pid' ? (x.pid ?? (x.procs ? x.procs[0].pid : 0)) : k === 'cmd' ? (x.cmd ?? (x.procs ? x.procs[0].cmd : '')) : x[k]);
  function matches(x, ql) {
    return !ql || x.name.toLowerCase().includes(ql) || x.image.toLowerCase().includes(ql) || (x.user || '').toLowerCase().includes(ql) || x.procs.some(p => String(p.pid) === ql || p.image.toLowerCase().includes(ql) || (p.cmd || '').toLowerCase().includes(ql));
  }
  function visible() {
    const ql = q.trim().toLowerCase();
    const cmp = ['name', 'user', 'cmd', 'status'].includes(sort.k)
      ? (a, b) => String(val(a, sort.k) || '').localeCompare(String(val(b, sort.k) || ''), undefined, { sensitivity: 'base' }) * sort.d || a.name.localeCompare(b.name)
      : (a, b) => ((val(a, sort.k) || 0) - (val(b, sort.k) || 0)) * sort.d || a.name.localeCompare(b.name);
    let list;
    if (flat) {
      list = [];
      for (const g of snap.groups) for (const p of g.procs) {
        const x = { ...p, id: g.id + ':' + p.pid, g: g.id, name: p.image, kind: g.kind, icon: g.icon, procs: [p], locked: g.locked };
        if (matches(x, ql)) list.push(x);
      }
    } else list = snap.groups.filter(g => matches(g, ql));
    list.sort(cmp);
    // While the pointer is over the list, rows keep their places (numbers still update) so
    // nothing jumps out from under the mouse.
    if (hovering && order.length) { const pos = new Map(order.map((id, i) => [id, i])); list = list.slice().sort((a, b) => (pos.has(a.id) ? pos.get(a.id) : 1e9) - (pos.has(b.id) ? pos.get(b.id) : 1e9)); }
    order = list.map(g => g.id);
    return list;
  }
  const icon = g => (g.icon ? `<img class="tm-app" src="${esc(assetUrl(g.icon))}" alt="" draggable="false">` : `<span class="tm-app tm-glyph">${esc((g.name || '?').trim().charAt(0).toUpperCase())}</span>`);
  function renderProcs() {
    if (built !== 'procs') buildProcs();
    const tbody = $('.tm-procs tbody');
    const cols = PCOLS.filter(c => c[0] === 'name' || pcols.includes(c[0]));
    $('.tm-procs colgroup').innerHTML = cols.map(([k]) => `<col class="c-${k}">`).join('');
    if (!snap) { tbody.innerHTML = `<tr class="tm-wait"><td colspan="${cols.length}">Reading processes…</td></tr>`; renderFoot(); return; }
    const P = snap.perf, memTot = P.mem_total || 1;
    const sum = k => snap.groups.reduce((a, g) => a + g[k], 0);
    const tot = { cpu: Math.round(P.cpu) + '%', mem: Math.round(P.mem_used / memTot * 100) + '%', gpu: Math.round(P.gpu) + '%', read: fmtRate(sum('read')), write: fmtRate(sum('write')), threads: P.threads.toLocaleString(), handles: P.handles.toLocaleString() };
    $('.tm-procs thead tr').innerHTML = cols.map(([k, n, tip]) => `<th data-sort="${k}" class="c-${k}"${sort.k === k ? ` aria-sort="${sort.d > 0 ? 'ascending' : 'descending'}"` : ''} title="${esc(tip)}">${tot[k] ? `<b>${tot[k]}</b>` : ''}<span>${n}</span></th>`).join('');
    const list = visible();
    $('.tm-n').textContent = `${snap.groups.reduce((a, g) => a + g.procs.length, 0)} processes`;
    const cell = (x, k, many) => {
      switch (k) {
        case 'pid': return `<td class="c-pid">${many ? '' : x.pid ?? x.procs[0].pid}</td>`;
        case 'status': { const s = status(x); return `<td class="c-status${x.suspended ? ' susp' : ''}">${s ? (x.suspended ? `${TI.pause}${s}` : `<span class="tm-eco">${TI.leaf}</span>${s}`) : ''}</td>`; }
        case 'user': return `<td class="c-user" title="${esc(x.user || '')}">${x.user ? esc(x.user) : '<span class="faint">—</span>'}</td>`;
        case 'cpu': return `<td style="${heat(x.cpu, 30)}">${x.cpu.toFixed(1)}%</td>`;
        case 'mem': return `<td style="${heat(x.mem, memTot * .12)}">${fmtMem(x.mem)}</td>`;
        case 'gpu': return `<td style="${heat(x.gpu, 60)}">${x.gpu.toFixed(1)}%</td>`;
        case 'read': return `<td style="${heat(x.read, 20 * MB)}">${fmtRate(x.read)}</td>`;
        case 'write': return `<td style="${heat(x.write, 20 * MB)}">${fmtRate(x.write)}</td>`;
        case 'threads': return `<td>${x.threads.toLocaleString()}</td>`;
        case 'handles': return `<td>${x.handles.toLocaleString()}</td>`;
        case 'cmd': { const c = x.cmd ?? (x.procs.length === 1 ? x.procs[0].cmd : x.procs[0].cmd); return `<td class="c-cmd" title="${esc(c || '')}">${c ? esc(c) : '<span class="faint">—</span>'}</td>`; }
        default: return '';
      }
    };
    const rest = (x, many) => cols.slice(1).map(([k]) => cell(x, k, many)).join('');
    const row = g => {
      const many = !flat && g.procs.length > 1, open = many && expanded.has(g.id), on = sel && sel.g === (g.g || g.id) && (flat ? sel.pid === g.pid : sel.pid == null);
      const gid = g.g || g.id;
      let h = `<tr class="g${on ? ' sel' : ''}${g.locked ? ' locked' : ''}${g.suspended ? ' susp' : ''}" data-g="${esc(gid)}"${flat ? ` data-pid="${g.pid}"` : ''}><td class="c-name"><span class="nm">${many ? `<button class="tm-exp${open ? ' open' : ''}" type="button" data-exp="${esc(g.id)}" aria-expanded="${open}" aria-label="${open ? 'Hide' : 'Show'} its processes">${TI.chev}</button>` : flat ? '' : '<i class="tm-sp"></i>'}${icon(g)}<span class="tm-name">${esc(g.name)}</span>${many ? `<small>(${g.procs.length})</small>` : ''}${flat ? `<small>PID ${g.pid}</small>` : ''}${!flat && g.eco_some && !pcols.includes('status') ? `<span class="tm-eco${g.eco ? '' : ' some'}" title="${g.eco ? 'Efficiency mode' : 'Some of its processes are in efficiency mode'}">${TI.leaf}</span>` : ''}</span></td>${rest(g, many)}</tr>`;
      if (open) h += g.procs.map(p => `<tr class="c${sel && sel.g === g.id && sel.pid === p.pid ? ' sel' : ''}${p.suspended ? ' susp' : ''}" data-g="${esc(g.id)}" data-pid="${p.pid}"><td class="c-name"><span class="nm"><i class="tm-sp"></i><i class="tm-sp"></i><span class="tm-name">${esc(p.image)}</span><small>PID ${p.pid}</small></span></td>${rest(p, false)}</tr>`).join('');
      return h;
    };
    let html = '';
    for (const [k, n] of SECTIONS) {
      const part = list.filter(g => g.kind === k);
      if (part.length) html += `<tr class="sec"><td colspan="${cols.length}">${n} <small>${part.length}</small></td></tr>` + part.map(row).join('');
    }
    tbody.innerHTML = html || `<tr class="tm-wait"><td colspan="${cols.length}">Nothing running matches “${esc(q)}”.</td></tr>`;
    if (sel && !groupOf(sel.g)) { sel = null; confirm = false; }
    renderFoot();
  }
  function renderColMenu() {
    const m = $('.tm-colmenu'); if (!m) return;
    m.hidden = !colsOpen;
    $('[data-tm="cols"]').setAttribute('aria-expanded', colsOpen);
    if (colsOpen) m.innerHTML = PCOLS.filter(c => c[0] !== 'name').map(([k, n, tip]) => `<label title="${esc(tip)}"><input type="checkbox" data-col="${k}"${pcols.includes(k) ? ' checked' : ''}><span>${n}</span></label>`).join('') + `<button class="cc-btn" type="button" data-tm="cols-reset">Reset</button>`;
  }
  function targetsOf(g) {
    const p = sel && sel.pid != null ? g.procs.find(x => x.pid === sel.pid) : null;
    if (sel && sel.pid != null && !p) return null;
    return { p, eco: p ? p.eco : g.eco, procs: (p ? [p] : g.procs).map(x => [x.pid, x.created]), label: p ? `${p.image} (PID ${p.pid})` : g.procs.length > 1 ? `${g.name} (${g.procs.length} processes)` : g.name };
  }
  // FILETIME ms (since 1601) → a date.
  const started = c => new Date(c - 11644473600000);
  function renderFoot() {
    const foot = $('.tm-foot'); if (!foot) return;
    const g = sel && groupOf(sel.g), t = g && targetsOf(g);
    let key, html;
    if (!g || !t) {
      const P = snap && snap.perf;
      key = 'none'; html = `<span class="grow tm-none">Select a process for its details. ${P ? `${P.processes} processes · ${P.threads.toLocaleString()} threads · ${P.handles.toLocaleString()} handles` : ''}</span><small class="tm-hint">Click a row to choose it · Del ends it</small>`;
      if (P) key += P.processes + ':' + P.threads;
    } else {
      const explorer = g.image.toLowerCase() === 'explorer.exe' && !t.p;
      const x = t.p || (g.procs.length === 1 ? g.procs[0] : null);
      if (confirm) {
        key = 'confirm:' + t.label;
        html = `<span class="grow tm-confirm">${explorer ? 'Restart Windows Explorer? File Explorer windows close and the desktop blinks for a moment.' : `End ${esc(t.label)}? Anything unsaved in it is lost.`}</span><button class="cc-btn" type="button" data-tm="cancel">Cancel</button><button class="cc-btn danger-fill" type="button" data-tm="kill">${explorer ? 'Restart' : 'End task'}</button>`;
      } else {
        const facts = x
          ? [['PID', x.pid], ['Parent', x.ppid || '—'], ['User', x.user || 'Not readable'], ['Started', started(x.created).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })], ['Threads', x.threads.toLocaleString()], ['Handles', x.handles.toLocaleString()], ['Status', status(x) || 'Running']]
          : [['Processes', g.procs.length], ['User', g.user || 'Not readable'], ['Threads', g.threads.toLocaleString()], ['Handles', g.handles.toLocaleString()], ['Memory', fmtMem(g.mem)], ['Status', status(g) || 'Running']];
        const cmd = x ? x.cmd : '';
        key = [g.id, t.label, t.eco, g.locked, g.exe, x && x.threads, x && x.handles, g.procs.length, status(x || g)].join('|');
        html = `<div class="tm-dhead">${icon(g)}<span><b>${esc(t.label)}</b>${g.exe ? `<span class="tm-path" title="${esc(g.exe)}">${esc(g.exe)}</span>` : '<span class="tm-path">Windows doesn’t say where this one lives</span>'}</span></div>
          <div class="tm-dfacts">${facts.map(([k, v]) => `<div><small>${k}</small><b>${esc(String(v))}</b></div>`).join('')}</div>
          ${cmd ? `<div class="tm-dcmd"><small>Command line</small><code title="Click to copy" data-tm="copycmd">${esc(cmd)}</code></div>` : ''}
          <div class="tm-dacts"><button class="cc-btn" type="button" data-tm="loc"${g.exe ? '' : ' disabled'}>${ic('folder')}Open file location</button>
          <button class="cc-btn" type="button" data-tm="eco" aria-pressed="${t.eco}"${g.locked ? ` disabled title="${esc(g.locked)}"` : ' title="Runs it at low priority and lets Windows slow it to save power"'}>${TI.leaf}${t.eco ? 'Efficiency mode on' : 'Efficiency mode'}</button>
          <button class="cc-btn danger" type="button" data-tm="end"${g.locked ? ` disabled title="${esc(g.locked)}"` : ''}>${ic('x')}${explorer ? 'Restart' : 'End task'}</button></div>`;
      }
    }
    if (key !== footKey) { footKey = key; foot.innerHTML = html; foot.classList.toggle('on', !!(g && t && !confirm)); }
  }
  function moveSel(dir) {
    const rows = [...el.querySelectorAll('.tm-procs tbody tr[data-g]')];
    if (!rows.length) return;
    let i = rows.findIndex(r => sel && r.dataset.g === sel.g && (r.dataset.pid ? +r.dataset.pid : null) === sel.pid);
    i = i < 0 ? (dir > 0 ? 0 : rows.length - 1) : clamp(i + dir, 0, rows.length - 1);
    const r = rows[i]; sel = { g: r.dataset.g, pid: r.dataset.pid ? +r.dataset.pid : null }; confirm = false;
    renderProcs();
    const now = el.querySelector('.tm-procs tr.sel'); if (now) now.scrollIntoView({ block: 'nearest' });
  }
  async function act(kind) {
    const g = sel && groupOf(sel.g), t = g && targetsOf(g); if (!t) return;
    if (kind === 'loc') { invoke('cc_open', { path: g.exe, reveal: true }).catch(e => flash(String(e))); return; }
    if (kind === 'copycmd') { const x = t.p || g.procs[0]; if (x && x.cmd) navigator.clipboard.writeText(x.cmd).then(() => flash('Command line copied')).catch(() => {}); return; }
    if (kind === 'eco') {
      const on = !t.eco, who = t.p ? t.p.image : g.name;
      try { await invoke('tm_eco', { procs: t.procs, on }); for (const x of t.p ? [t.p] : g.procs) x.eco = on; g.eco = g.procs.every(x => x.eco); g.eco_some = g.procs.some(x => x.eco); flash(on ? `${who} is in efficiency mode` : `${who} runs normally again`); footKey = ''; renderProcs(); } catch (e) { flash(String(e)); }
      return;
    }
    if (kind === 'kill') {
      try {
        const n = await invoke('tm_end', { procs: t.procs });
        flash(g.image.toLowerCase() === 'explorer.exe' && !t.p ? 'Restarting Windows Explorer' : n ? `Ended ${t.label}` : 'It had already closed');
        sel = null; confirm = false; renderFoot();
      } catch (e) { confirm = false; flash(String(e)); renderFoot(); }
    }
  }

  /* ---------- Run new task ---------- */
  function runBox() {
    let box = el.querySelector('.tm-run');
    if (box) { box.querySelector('input').focus(); return; }
    box = document.createElement('div');
    box.className = 'tm-run';
    box.innerHTML = `<form><h5>${TI.plus}Run new task</h5><p>Type the name of a program, folder, document or website, as in the Run box.</p>
      <input type="text" spellcheck="false" autocomplete="off" placeholder="notepad, cmd, %temp%, https://…" aria-label="What to run">
      <label class="tm-chk"><input type="checkbox"><span>Run as administrator (Windows asks first)</span></label>
      <div class="tm-runb"><button class="cc-btn" type="button" data-tm="run-cancel">Cancel</button><button class="cc-btn primary" type="submit">Run</button></div></form>`;
    el.append(box);
    const inp = box.querySelector('input[type="text"]');
    inp.value = store.get('tmRunLast', '');
    inp.focus(); inp.select();
    box.querySelector('form').addEventListener('submit', async e => {
      e.preventDefault();
      const command = inp.value.trim(); if (!command) return;
      try { await invoke('tm_run', { command, admin: box.querySelector('input[type="checkbox"]').checked }); store.set('tmRunLast', command); box.remove(); flash(`Started ${command}`); } catch (err) { flash(String(err)); }
    });
  }
  ctx.runBox = runBox;

  /* ---------- Performance ---------- */
  const netMax = name => Math.max(64 * 1024, ...(hist['rx:' + name] || [0]), ...(hist['tx:' + name] || [0])) * 1.15;
  function perfItems() {
    const P = snap.perf, s = stats || {}, sen = s.sensors || {};
    const gpuSen = sen.gpus || [];
    const drives = sen.drives || [];
    const items = [
      { k: 'cpu', n: 'CPU', ic: 'cpu', sub: `${Math.round(P.cpu)}%  ${P.mhz ? fmtGHz(P.mhz) : sen.cpu_mhz ? fmtGHz(sen.cpu_mhz) : ''}${sen.cpu_temp != null ? '  (' + Math.round(sen.cpu_temp) + '°C)' : ''}`, h: hist.cpu, max: 100 },
      { k: 'mem', n: 'Memory', ic: 'mem', sub: `${(P.mem_used / GB).toFixed(1)}/${(P.mem_total / GB).toFixed(1)} GB (${Math.round(P.mem_used / (P.mem_total || 1) * 100)}%)`, h: hist.mem, max: 100, cls: 'mem' }
    ];
    // The graphics card first; Task Manager's GPU counters are its numbers.
    const gpus = gpuSen.length ? gpuSen.slice().sort((a, b) => a.integrated - b.integrated) : [{ name: s.gpu_name || 'GPU' }];
    gpus.forEach((g, i) => items.push({ k: 'gpu' + i, n: g.name ? g.name.replace(/^(AMD|NVIDIA|Intel\(R\)) /, '').replace(' Graphics', '') : 'GPU', ic: 'monitor', sub: `${!g.integrated ? Math.round(P.gpu) + '%' : ''}${g.temp != null ? '  (' + Math.round(g.temp) + '°C)' : ''}${g.integrated ? '  on the CPU' : ''}`, h: g.integrated ? null : hist.gpu, max: 100, gpu: g, cls: 'gpu' }));
    for (const d of P.disks) {
      const [num, ...letters] = d.name.split(' '), dr = drives.find(x => x.letters && x.letters.some(l => letters.includes(l.replace(/\\$/, ''))));
      items.push({ k: 'disk:' + d.name, n: `Disk ${num}${letters.length ? ' (' + letters.join(' ') + ')' : ''}`, ic: 'disk', sub: `${Math.round(d.active)}%${dr ? '  (' + Math.round(dr.temp) + '°C)' : ''}`, h: hist['disk:' + d.name], max: 100, disk: d, drive: dr, num: +num, cls: 'disk' });
    }
    for (const n of P.nets) items.push({ k: 'net:' + n.name, n: netKind(n.name), ic: 'net', sub: `S: ${fmtBits(n.tx)}  R: ${fmtBits(n.rx)}`, h: hist['rx:' + n.name], h2: hist['tx:' + n.name], max: netMax(n.name), net: n, cls: 'net' });
    return items;
  }
  function renderPerf() {
    if (!snap) { built = 'perf-wait'; main.innerHTML = top('perf') + `<p class="tm-empty">Reading…</p>`; return; }
    if (!sysInfo) ctx.system().then(() => { if (isOpen && page === 'perf') renderPerf(); }).catch(() => {});
    const items = perfItems(), cur = items.find(x => x.k === perfSel) || items[0];
    perfSel = cur.k;
    const P = snap.perf, s = stats || {}, sen = s.sensors || {}, C = cpuInfo || {}, SI = sysInfo || {};
    const K = pairs => kv(esc, pairs);
    let title = cur.n, sub = '', body = '', scale = '100%', head = '';
    if (cur.k === 'cpu') {
      title = 'CPU'; sub = C.name || s.cpu_name || '';
      head = `<div class="tm-ledhead">${leds(P.cpu / 100, 40, { horizontal: true })}<b>${P.cpu.toFixed(1)}%</b></div>`;
      const n = P.cores.length || 1, colsN = n <= 4 ? n : n <= 8 ? 4 : n <= 16 ? 4 : n <= 24 ? 6 : 8;
      const viewBtns = `<div class="tm-seg sm" role="group" aria-label="Graph"><button type="button" data-cpuview="overall" aria-pressed="${cpuView === 'overall'}">Overall</button><button type="button" data-cpuview="logical" aria-pressed="${cpuView === 'logical'}">Logical processors</button></div>`;
      body = `<div class="tm-gcap"><small>${cpuView === 'logical' ? '% utilization by logical processor' : '% utilization, last 60 seconds'}</small>${viewBtns}</div>`
        + (cpuView === 'logical'
          ? `<div class="tm-lps" style="--c:${colsN}">${P.cores.map((v, i) => `<div class="tm-lp"><small>CPU ${i}</small>${graph([{ h: hist['core' + i], cls: 'cpu' }, { h: hist['kcore' + i], cls: 'kern', fill: false }], 100, { W: 200, H: 120 })}</div>`).join('')}</div>`
          : graph([{ h: hist.cpu, cls: 'cpu' }, { h: hist.kernel, cls: 'kern', fill: false }], 100))
        + `<div class="tm-legend"><span><i></i>Utilization</span><span><i class="kern"></i>Kernel time</span></div>`
        + K([['Utilization', P.cpu.toFixed(1) + '%'], (P.mhz || sen.cpu_mhz) && ['Speed', fmtGHz(P.mhz || sen.cpu_mhz)], sen.cpu_temp != null && ['Temperature', Math.round(sen.cpu_temp) + '°C', 'Measured by the graphics on the same chip'], ['Kernel time', (P.kernel || 0).toFixed(1) + '%'], ['Processes', P.processes], ['Threads', P.threads.toLocaleString()], ['Handles', P.handles.toLocaleString()], s.uptime && ['Up time', fmtUp(s.uptime)]])
        + `<dl class="tm-specs">${[
          ['Base speed', C.base_mhz ? fmtGHz(C.base_mhz) : '—'], ['Sockets', C.sockets || 1], ['Cores', C.cores || '—'], ['Logical processors', C.logical || n],
          ['Virtualization', C.virtualization ? 'Enabled in firmware' : 'Off'], ['Virtual machine', C.vm ? 'Yes' : 'No'],
          ['L1 cache', C.l1 ? fmtMem(C.l1) : '—'], ['L2 cache', C.l2 ? fmtMem(C.l2) : '—'], ['L3 cache', C.l3 ? fmtMem(C.l3) : '—'], ['Power plan', C.power_plan || '—']
        ].map(([k, v]) => `<div><dt>${k}</dt><dd>${esc(String(v))}</dd></div>`).join('')}</dl>`;
    } else if (cur.k === 'mem') {
      const mods = SI.modules || [];
      sub = `${(P.mem_total / GB).toFixed(0)} GB${mods[0] ? ' ' + mods[0].kind : ''}`;
      head = `<div class="tm-ledhead mem">${leds(P.mem_used / P.mem_total, 40, { horizontal: true })}<b>${(P.mem_used / GB).toFixed(1)} GB</b></div>`;
      const used = P.mem_used / P.mem_total * 100, cache = clamp(P.cached / P.mem_total * 100, 0, 100 - used);
      body = graph([{ h: hist.mem, cls: 'mem' }], 100) + `<div class="tm-gcap"><small>Memory composition</small><small>In use · cached · free</small></div><div class="tm-memc"><i style="width:${used.toFixed(1)}%"></i><i class="c" style="width:${cache.toFixed(1)}%"></i></div>`
        + K([['In use', (P.mem_used / GB).toFixed(1) + ' GB'], ['Available', ((P.mem_total - P.mem_used) / GB).toFixed(1) + ' GB'], ['Committed', `${(P.committed / GB).toFixed(1)} / ${(P.commit_limit / GB).toFixed(1)} GB`], ['Cached', (P.cached / GB).toFixed(1) + ' GB'], ['Paged pool', fmtMem(P.paged)], ['Non-paged pool', fmtMem(P.nonpaged)]])
        + `<dl class="tm-specs">${[
          mods[0] && ['Speed', `${mods[0].configured || mods[0].speed} MT/s`], SI.mem_slots && ['Slots used', `${mods.length} of ${SI.mem_slots}`], mods[0] && ['Type', mods[0].kind || '—'],
          SI.mem_installed && ['Installed', fmtMem(SI.mem_installed)], SI.mem_installed && SI.mem_usable && ['Hardware reserved', fmtMem(Math.max(0, SI.mem_installed - SI.mem_usable))],
          ...mods.map(m => [m.slot, `${fmtMem(m.size_mb * MB)} ${m.kind} ${m.configured || m.speed} MT/s · ${[m.maker, m.part].filter(Boolean).join(' ')}`, true])
        ].filter(Boolean).map(([k, v, wide]) => `<div${wide ? ' class="wide"' : ''}><dt>${esc(k)}</dt><dd>${esc(String(v))}</dd></div>`).join('')}</dl>`;
    } else if (cur.gpu) {
      const g = cur.gpu, info = (SI.gpus || []).find(x => g.name && x.name === g.name) || {};
      title = g.name || s.gpu_name || 'GPU'; sub = g.integrated ? 'Built into the CPU' : '';
      head = g.integrated ? '' : `<div class="tm-ledhead gpu">${leds(P.gpu / 100, 40, { horizontal: true })}<b>${Math.round(P.gpu)}%</b></div>`;
      body = (g.integrated ? `<p class="tm-note">Task Manager’s graphics counters are summed over every adapter, so this one’s share isn’t shown on its own.</p>` : graph([{ h: hist.gpu, cls: 'gpu' }], 100) + `<div class="tm-eng">${P.gpu_engines.map(([n, v]) => `<div><small>${esc(n)}</small><span><i style="width:${clamp(v, 0, 100).toFixed(1)}%"></i></span><b>${Math.round(v)}%</b></div>`).join('')}</div>`)
        + K([!g.integrated && ['Utilization', Math.round(P.gpu) + '%'], g.temp != null && ['Temperature', Math.round(g.temp) + '°C'], g.fan_rpm != null && ['Fan', g.fan_rpm ? g.fan_rpm.toLocaleString() + ' rpm' : 'Stopped'], g.power_pct != null && ['Power', Math.round(g.power_pct) + '% of limit'], g.core_mhz && ['Core clock', fmtGHz(g.core_mhz)], g.mem_mhz && ['Memory clock', Math.round(g.mem_mhz) + ' MHz'], g.voltage_mv && ['Voltage', (g.voltage_mv / 1000).toFixed(3) + ' V'], !g.integrated && s.vram_total && ['Dedicated memory', `${(s.vram_used || 0).toFixed(1)} / ${s.vram_total.toFixed(1)} GB`]])
        + `<dl class="tm-specs">${[info.driver && ['Driver version', info.driver], info.vram && ['Dedicated memory', fmtMem(info.vram)], info.shared && ['Shared memory', fmtMem(info.shared)], ...(SI.displays || []).filter(d => d.gpu === info.name).map(d => ['Display', `${d.name} · ${d.width}×${d.height} @ ${Math.round(d.hz)} Hz`])].filter(Boolean).map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(String(v))}</dd></div>`).join('')}</dl>`;
    } else if (cur.disk) {
      const d = cur.disk, letters = d.name.split(' ').slice(1), dr = cur.drive, info = (SI.disks || []).find(x => x.number === cur.num) || {};
      const vols = (SI.volumes || []).filter(x => letters.includes(x.letter));
      sub = info.model || (dr ? dr.name : '');
      head = `<div class="tm-ledhead disk">${leds(d.active / 100, 40, { horizontal: true })}<b>${Math.round(d.active)}%</b></div>`;
      body = graph([{ h: hist['disk:' + d.name], cls: 'disk' }], 100) + K([['Active time', Math.round(d.active) + '%'], ['Read speed', fmtRate(d.read)], ['Write speed', fmtRate(d.write)], dr && ['Temperature', Math.round(dr.temp) + '°C' + (dr.hottest && dr.hottest > dr.temp + 3 ? ` (hottest part ${Math.round(dr.hottest)}°C)` : '')]])
        + `<dl class="tm-specs">${[info.size && ['Capacity', fmtMem(info.size)], ['Type', [info.bus, info.ssd === true ? 'SSD' : info.ssd === false ? 'HDD' : ''].filter(Boolean).join(' ') || '—'], ...vols.map(v => [`${v.letter} ${v.label || ''}`.trim(), `${fmtMem(v.free)} free of ${fmtMem(v.total)} · ${v.fs}`])].filter(Boolean).map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(String(v))}</dd></div>`).join('')}</dl>`
        + `<p class="tm-note">Its health (wear, hours, errors) is under Power &amp; Freq.</p>`;
    } else if (cur.net) {
      const n = cur.net, ad = (SI.adapters || []).find(a => a.description && (n.name.replace(/[()#\/]/g, '_').toLowerCase().includes(a.description.replace(/[()#\/]/g, '_').toLowerCase().slice(0, 18)) || a.description.toLowerCase().replace(/[^a-z0-9]/g, '') === n.name.toLowerCase().replace(/[^a-z0-9]/g, ''))) || {};
      sub = ad.name ? `${ad.name} · ${n.name}` : n.name; scale = fmtBits(cur.max);
      body = graph([{ h: hist['rx:' + n.name], cls: 'net' }, { h: hist['tx:' + n.name], dash: true, cls: 'net2', fill: false }], cur.max) + `<div class="tm-legend"><span><i class="net"></i>Receive</span><span><i class="dash"></i>Send</span></div>`
        + K([['Receive', fmtBits(n.rx)], ['Send', fmtBits(n.tx)], ['Link speed', fmtLink(n.bandwidth)]])
        + `<dl class="tm-specs">${[['Adapter', n.name], ad.ipv4 && ad.ipv4.length && ['IPv4 address', ad.ipv4.join(', ')], ad.ipv6 && ad.ipv6.length && ['IPv6 address', ad.ipv6[0]], ad.gateway && ad.gateway.length && ['Gateway', ad.gateway.join(', ')], ad.dns && ad.dns.length && ['DNS servers', ad.dns.join(', ')], ad.mac && ['Hardware address', ad.mac]].filter(Boolean).map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(String(v))}</dd></div>`).join('')}</dl>`;
    }
    const listScroll = $('.tm-plist') ? $('.tm-plist').scrollTop : 0, mainScroll = $('.tm-pmain') ? $('.tm-pmain').scrollTop : 0;
    built = 'perf';
    main.innerHTML = top('perf') + `<div class="tm-perf"><div class="tm-plist" role="listbox" aria-label="Devices">${items.map(x => `<button class="tm-pi ${x.cls || ''}" type="button" role="option" data-perf="${esc(x.k)}" aria-selected="${x.k === cur.k}">${x.h ? spark(x.h, x.max, x.h2) : `<span class="tm-spark none">${ic(x.ic)}</span>`}<span><b>${esc(x.n)}</b><small>${esc(x.sub)}</small></span></button>`).join('')}</div>
      <div class="tm-pmain ${cur.cls || ''}"><h5><span>${esc(title)}</span><small>${esc(sub)}</small></h5>${head}${cur.k === 'cpu' ? '' : `<div class="tm-gcap"><small>Last 60 seconds</small><small>${esc(scale)}</small></div>`}${body}</div></div>`;
    $('.tm-plist').scrollTop = listScroll; $('.tm-pmain').scrollTop = mainScroll;
  }

  /* ---------- Startup apps ---------- */
  async function loadStartup() {
    try { startup = await invoke('tm_startup'); } catch (e) { startup = []; flash(String(e)); }
    if (isOpen && page === 'startup') renderStartup();
  }
  function renderStartup() {
    if (startup == null) {
      if (built !== 'startup-wait') { built = 'startup-wait'; main.innerHTML = top('startup') + `<p class="tm-empty">Reading…</p>`; loadStartup(); }
      return;
    }
    if (built === 'startup') return; // nothing live here; redrawn when something changes
    built = 'startup';
    const on = startup.filter(s => s.on).length;
    const row = s => `<div class="tm-su${s.on ? '' : ' off'}"><span class="tm-su-ic">${s.icon ? `<img src="${esc(assetUrl(s.icon))}" alt="">` : `<span class="tm-glyph">${esc(s.name.charAt(0).toUpperCase())}</span>`}</span>
      <span class="tm-su-t"><b>${esc(s.name)}</b><small title="${esc(s.command)}">${esc([s.publisher, s.source, s.user ? '' : 'All users'].filter(Boolean).join(' · '))}</small></span>
      <small class="tm-su-st">${s.on ? 'Starts with Windows' : 'Off'}</small>
      <label class="tm-sw"${s.user ? '' : ' title="Set for every account on this PC; changing it needs administrator rights"'}><input type="checkbox" data-su="${esc(s.id)}"${s.on ? ' checked' : ''}${s.user ? '' : ' disabled'} aria-label="Start ${esc(s.name)} with Windows"><i></i></label></div>`;
    main.innerHTML = top('startup', `<small>${on} of ${startup.length} start with Windows</small><span class="grow"></span><button class="cc-btn" type="button" data-tm="su-refresh">${ic('restart')}Refresh</button>`)
      + `<div class="tm-sus">${startup.length ? startup.map(row).join('') : '<p class="tm-empty">Nothing is set to start with Windows.</p>'}</div>
      <div class="tm-foot"><span class="grow">Switching one off only stops it starting with Windows; nothing is uninstalled. Apps marked “All users” need administrator rights to change.</span></div>`;
  }

  /* ---------- input ---------- */
  el.addEventListener('click', e => {
    const t = e.target;
    const b = t.closest('[data-tm]');
    if (b) {
      const k = b.dataset.tm;
      if (k === 'close') close();
      else if (k === 'max') toggleMax();
      else if (k === 'end') { confirm = true; renderFoot(); }
      else if (k === 'cancel') { confirm = false; renderFoot(); }
      else if (k === 'kill' || k === 'loc' || k === 'eco' || k === 'copycmd') act(k);
      else if (k === 'su-refresh') { startup = null; built = ''; renderStartup(); }
      else if (k === 'cols') { colsOpen = !colsOpen; renderColMenu(); }
      else if (k === 'cols-reset') { pcols = DEF_COLS.slice(); store.set('tmCols', pcols); renderColMenu(); renderProcs(); }
      else if (k === 'run') runBox();
      else if (k === 'run-cancel') { const r = el.querySelector('.tm-run'); if (r) r.remove(); }
      else if (pages[page] && pages[page].click) pages[page].click(e, b);
      return;
    }
    if (colsOpen && !t.closest('.tm-colmenu')) { colsOpen = false; renderColMenu(); }
    const pg = t.closest('[data-page]');
    if (pg) { go(pg.dataset.page); return; }
    const fl = t.closest('[data-flat]');
    if (fl) { flat = fl.dataset.flat === '1'; store.set('tmFlat', flat); order = []; sel = null; built = ''; renderProcs(); return; }
    const cv = t.closest('[data-cpuview]');
    if (cv) { cpuView = cv.dataset.cpuview; store.set('tmCpuView', cpuView); renderPerf(); return; }
    if (pages[page] && pages[page].click) pages[page].click(e, null);
  });
  el.addEventListener('pointerdown', e => {
    const pi = e.button === 0 && e.target.closest('[data-perf]');
    if (pi) { perfSel = pi.dataset.perf; renderPerf(); }
  });
  el.addEventListener('change', async e => {
    if (e.target.closest('.tm-speed')) { speed = +e.target.value; store.set('tmSpeed', speed); render(); tick(); return; }
    const col = e.target.closest('[data-col]');
    if (col) { const k = col.dataset.col; pcols = col.checked ? [...pcols, k] : pcols.filter(x => x !== k); store.set('tmCols', pcols); renderProcs(); return; }
    const cb = e.target.closest('[data-su]');
    if (cb) {
      const s = startup && startup.find(x => x.id === cb.dataset.su); cb.disabled = true;
      // Rows stay where they were (Windows' list puts the ones that are on first).
      const pos = new Map((startup || []).map((x, i) => [x.id, i]));
      try { startup = (await invoke('tm_startup_set', { id: cb.dataset.su, on: cb.checked })).sort((x, y) => (pos.has(x.id) ? pos.get(x.id) : 1e9) - (pos.has(y.id) ? pos.get(y.id) : 1e9)); flash(`${s ? s.name : 'It'} ${cb.checked ? 'starts with Windows' : 'won’t start with Windows'}`); }
      catch (err) { flash(String(err)); cb.checked = !cb.checked; }
      built = ''; renderStartup();
      return;
    }
    if (pages[page] && pages[page].change) pages[page].change(e);
  });

  /** Keys while the window is open: 'used' (handled), 'own' (the window's, let it type) or false. */
  function key(e) {
    if (!isOpen) return false;
    const typing = !!(e.target.matches && e.target.matches('input[type="text"]'));
    const run = el.querySelector('.tm-run');
    if (run) { if (e.key === 'Escape') { run.remove(); return 'used'; } return 'own'; }
    if (e.key === 'Escape') {
      if (colsOpen) { colsOpen = false; renderColMenu(); }
      else if (confirm) { confirm = false; renderFoot(); }
      else if (pages[page] && pages[page].key && pages[page].key(e) === 'used') { /* the page used it */ }
      else if (typing && e.target.value) { e.target.value = ''; e.target.dispatchEvent(new Event('input', { bubbles: true })); }
      else close();
      return 'used';
    }
    if (page === 'procs' && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { moveSel(e.key === 'ArrowDown' ? 1 : -1); return 'used'; }
      if (!typing && (e.key === 'ArrowRight' || e.key === 'ArrowLeft') && sel && sel.pid == null) {
        const g = groupOf(sel.g); if (g && g.procs.length > 1) { e.key === 'ArrowRight' ? expanded.add(g.id) : expanded.delete(g.id); renderProcs(); }
        return 'used';
      }
      if (e.key === 'Delete' && sel && !(typing && e.target.value)) { const g = groupOf(sel.g); if (g && !g.locked) { confirm = true; renderFoot(); } return 'used'; }
      if (e.key === 'Enter' && confirm) { act('kill'); return 'used'; }
      // Typing anywhere goes to the search box.
      if (!typing && e.key.length === 1 && !e.target.closest?.('button, input, select')) { const qi = $('.tm-q'); if (qi) qi.focus(); }
    } else if (pages[page] && pages[page].key) {
      const r = pages[page].key(e); if (r) return r;
    }
    return 'own';
  }

  return { open, close, key, isOpen: () => isOpen, owns: n => el.contains(n) };
}
