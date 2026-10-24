// Task Manager pages: Disk Space (scan a drive, then a bubble map of what's taking the room, with
// the kinds of file and the biggest folders and files) and Benchmarks (CPU, memory, disk, graphics
// and internet, with dials, a history and an overall score).

import { clamp, fmtSize, fmtDate, leds, dial, fmtMs } from './tm-util.js';

const CATS = ['Media', 'Documents', 'Code', 'Archives', 'Apps & games', 'System', 'Other'];
const CAT_CLS = ['media', 'docs', 'code', 'arch', 'apps', 'sys', 'other'];

/* ---------- circle packing (the front-chain method d3's packSiblings uses) ---------- */
function place(b, a, c) {
  const dx = b.x - a.x, dy = b.y - a.y, d2 = dx * dx + dy * dy;
  if (d2) {
    let a2 = a.r + c.r; a2 *= a2;
    let b2 = b.r + c.r; b2 *= b2;
    if (a2 > b2) { const x = (d2 + b2 - a2) / (2 * d2), y = Math.sqrt(Math.max(0, b2 / d2 - x * x)); c.x = b.x - x * dx - y * dy; c.y = b.y - x * dy + y * dx; }
    else { const x = (d2 + a2 - b2) / (2 * d2), y = Math.sqrt(Math.max(0, a2 / d2 - x * x)); c.x = a.x + x * dx - y * dy; c.y = a.y + x * dy + y * dx; }
  } else { c.x = a.x + c.r; c.y = a.y; }
}
const intersects = (a, b) => { const dr = a.r + b.r - 1e-6, dx = b.x - a.x, dy = b.y - a.y; return dr > 0 && dr * dr > dx * dx + dy * dy; };
const score = n => { const a = n.c, b = n.next.c, ab = a.r + b.r, dx = (a.x * b.r + b.x * a.r) / ab, dy = (a.y * b.r + b.y * a.r) / ab; return dx * dx + dy * dy; };
export function pack(circles) {
  const n = circles.length; if (!n) return;
  let a = circles[0]; a.x = 0; a.y = 0; if (n < 2) return;
  let b = circles[1]; a.x = -b.r; b.x = a.r; b.y = 0; if (n < 3) return;
  let c = circles[2]; place(b, a, c);
  a = { c: a }; b = { c: b }; c = { c };
  a.next = c.prev = b; b.next = a.prev = c; c.next = b.prev = a;
  outer: for (let i = 3; i < n; ++i) {
    c = circles[i]; place(a.c, b.c, c); c = { c };
    let j = b.next, k = a.prev, sj = b.c.r, sk = a.c.r;
    do {
      if (sj <= sk) { if (intersects(j.c, c.c)) { b = j; a.next = b; b.prev = a; --i; continue outer; } sj += j.c.r; j = j.next; }
      else { if (intersects(k.c, c.c)) { a = k; a.next = b; b.prev = a; --i; continue outer; } sk += k.c.r; k = k.prev; }
    } while (j !== k.next);
    c.prev = a; c.next = b; a.next = b.prev = b = c;
    let aa = score(a);
    while ((c = c.next) !== b) { const ca = score(c); if (ca < aa) { a = c; aa = ca; } }
    b = a.next;
  }
}

export function createTools(ctx) {
  const { esc, ic, invoke, flash, store } = ctx;
  const mainEl = () => document.querySelector('.tmw .tm-main');

  /* ---------- Disk Space ---------- */
  let drive = store.get('tmDiskDrive', 'C:'), dStatus = null, dView = null, dPath = '', dT = 0, dGroup = 'folders', dSel = null, volumes = null, lastViewAt = 0;
  async function pollDisk() {
    clearTimeout(dT);
    const main = mainEl(); if (!ctx.open || !main || !main.querySelector('.tm-disk')) return;
    try { dStatus = await invoke('disk_scan_status'); } catch (e) { return; }
    // While scanning, the map refreshes every 2.5 s; afterwards once.
    const want = dStatus.root && (dStatus.running ? Date.now() - lastViewAt > 2500 : !dView || dView._done !== dStatus.done);
    if (want) {
      lastViewAt = Date.now();
      try { const v = await invoke('disk_scan_view', { path: dPath }); if (v) { v._done = dStatus.done; dView = v; } else if (dPath) { dPath = ''; } } catch (e) {}
    }
    drawDisk(main);
    if (dStatus.running) dT = setTimeout(pollDisk, 500);
  }
  async function openPath(p) {
    dPath = p; dSel = null;
    try { const v = await invoke('disk_scan_view', { path: p }); if (v) { v._done = dStatus && dStatus.done; dView = v; } } catch (e) { flash(String(e)); }
    const m = mainEl(); if (m) drawDisk(m);
  }
  function drawDisk(main) {
    const s = dStatus || {}, v = dView;
    const scanning = s.running;
    main.querySelector('[data-tm="disk-scan"]').innerHTML = scanning ? `${ic('stop')}Stop` : `${ic('play')}${s.root ? 'Scan again' : 'Scan'}`;
    // Progress / summary line.
    const used = s.total ? s.total - s.free : 0;
    const prog = main.querySelector('.tm-dprog');
    if (scanning) prog.innerHTML = `${leds(used ? s.bytes / used : 0, 48, { horizontal: true, cls: 'disk' })}<span><b>${s.files.toLocaleString()}</b> files · <b>${s.folders.toLocaleString()}</b> folders · ${fmtSize(s.bytes)} · ${(s.ms / 1000).toFixed(0)} s</span><small title="${esc(s.current)}">${esc(s.current)}</small>`;
    else if (s.root) prog.innerHTML = `<span>${s.done ? 'Scanned' : 'Stopped'} ${esc(s.root)} in ${fmtMs(s.ms)}: <b>${s.files.toLocaleString()}</b> files in <b>${s.folders.toLocaleString()}</b> folders, ${fmtSize(s.bytes)}${s.denied ? ` · <span class="faint">${s.denied} folders couldn’t be read without administrator rights</span>` : ''}</span>`;
    else prog.innerHTML = `<span class="faint">Pick a drive and press Scan. Nothing is changed: it only reads folder listings.</span>`;
    // Crumbs.
    main.querySelector('.tm-dcrumbs').innerHTML = v ? v.crumbs.map(([n, p], i) => `<button type="button" data-dpath="${esc(p)}"${i === v.crumbs.length - 1 ? ' aria-current="true"' : ''}>${esc(n)}</button>`).join('<span>›</span>') + `<small>${fmtSize(v.size)}</small>` : '';
    // The map.
    const map = main.querySelector('.tm-dmap');
    if (!v) { map.innerHTML = `<div class="tm-dempty">${ic('disk')}<p>${scanning ? 'Scanning…' : 'No scan yet'}</p></div>`; }
    else {
      const items = dGroup === 'types' ? v.cats.map((b, i) => ({ name: CATS[i], size: b, cat: i, kind: 'type', path: '' })).filter(x => x.size > 0) : v.items.filter(x => x.size > 0);
      const W = map.clientWidth || 800, H = map.clientHeight || 520;
      const total = items.reduce((a, x) => a + x.size, 0) || 1;
      const circles = items.map((x, i) => ({ r: Math.sqrt(x.size / total) * 100, i })).sort((a, b) => b.r - a.r);
      pack(circles);
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const c of circles) { minX = Math.min(minX, c.x - c.r); minY = Math.min(minY, c.y - c.r); maxX = Math.max(maxX, c.x + c.r); maxY = Math.max(maxY, c.y + c.r); }
      const pad = 6, k = Math.min((W - pad * 2) / (maxX - minX || 1), (H - pad * 2) / (maxY - minY || 1));
      const ox = (W - (maxX - minX) * k) / 2 - minX * k, oy = (H - (maxY - minY) * k) / 2 - minY * k;
      // Each big folder shows its own biggest folders inside it, packed the same way.
      const inner = (x, cx, cy, r) => {
        if (!x.kids || x.kids.length < 2 || r < 38) return '';
        const sum = x.kids.reduce((a, q) => a + q.size, 0) || 1;
        const cs = x.kids.map((q, j) => ({ r: Math.sqrt(q.size / sum), j })).sort((a, b) => b.r - a.r);
        pack(cs);
        let ext = 0; for (const c of cs) ext = Math.max(ext, Math.hypot(c.x, c.y) + c.r);
        const s = (r * .86) / (ext || 1);
        return cs.map(c => { const q = x.kids[c.j], rr = c.r * s - .8; if (rr < 1.2) return ''; return `<circle class="kid ${CAT_CLS[q.cat]}" cx="${(cx + c.x * s).toFixed(1)}" cy="${(cy + c.y * s).toFixed(1)}" r="${rr.toFixed(1)}"><title>${esc(q.name)} · ${fmtSize(q.size)}</title></circle>`; }).join('');
      };
      map.innerHTML = `<svg viewBox="0 0 ${W} ${H}" width="${W}" height="${H}">${circles.map(c => {
        const x = items[c.i], cx = c.x * k + ox, cy = c.y * k + oy, r = Math.max(1.5, c.r * k - 1.2);
        const nested = inner(x, cx, cy, r);
        const label = r > 26 ? `<text x="${cx.toFixed(1)}" y="${(cy - (r > 44 ? 4 : 0)).toFixed(1)}" class="t">${esc(x.name.length > r / 4.2 ? x.name.slice(0, Math.max(3, Math.floor(r / 4.2))) + '…' : x.name)}</text>${r > 44 ? `<text x="${cx.toFixed(1)}" y="${(cy + 13).toFixed(1)}" class="s">${fmtSize(x.size)}</text>` : ''}` : '';
        const id = x.kind === 'type' ? 'type:' + x.cat : x.kind + ':' + x.path + ':' + x.name;
        return `<g class="b ${CAT_CLS[x.cat]}${x.kind === 'dir' && x.deeper ? ' deep' : ''}${dSel === id ? ' sel' : ''}${x.kind !== 'dir' ? ' flat' : ''}${nested ? ' nest' : ''}" data-b="${c.i}"><circle cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="${r.toFixed(1)}"><title>${esc(x.name)} · ${fmtSize(x.size)}${x.kind === 'dir' ? ' · ' + CATS[x.cat] + ' mostly' : ''}</title></circle>${nested}${label}</g>`;
      }).join('')}</svg>`;
      map._items = items;
    }
    // Side: the drive, kinds of file, biggest folders and files.
    const side = main.querySelector('.tm-dside');
    const vol = (volumes || []).find(x => x.letter === drive);
    const tot = vol ? vol.total : s.total, free = vol ? vol.free : s.free;
    const catTotal = v ? v.cats.reduce((a, b) => a + b, 0) || 1 : 1;
    side.innerHTML = `<section><header><h5>${esc(drive)}\\</h5><small>${tot ? fmtSize(tot) : ''}</small></header>${tot ? `<span class="tm-dbar"><i style="width:${((tot - free) / tot * 100).toFixed(1)}%"></i></span><div class="tm-dbarl"><small>${fmtSize(tot - free)} used</small><small>${fmtSize(free)} free</small></div>` : ''}</section>
      <section><header><h5>File types</h5><small>${v ? 'in ' + esc(v.crumbs[v.crumbs.length - 1][0]) : ''}</small></header>${CATS.map((n, i) => `<div class="tm-dcat ${CAT_CLS[i]}"><i></i><span>${n}</span><b>${v ? fmtSize(v.cats[i]) : '—'}</b><span class="bar"><em style="width:${v ? (v.cats[i] / catTotal * 100).toFixed(1) : 0}%"></em></span></div>`).join('')}</section>
      <section><header><h5>Largest folders</h5></header>${v && v.folders.length ? v.folders.map(f => `<button type="button" class="tm-dli" data-dpath="${esc(f.path)}" title="${esc(f.path)}"><span class="c ${CAT_CLS[f.cat]}"></span><span class="n">${esc(f.path.split('\\').slice(-2).join('\\'))}</span><b>${fmtSize(f.size)}</b></button>`).join('') : `<p class="faint">${scanning ? 'After the scan finishes.' : 'Nothing measured yet.'}</p>`}</section>
      <section><header><h5>Largest files</h5></header>${v && v.largest.length ? v.largest.map(f => `<button type="button" class="tm-dli" data-dfile="${esc(f.path)}" title="Show in File Explorer: ${esc(f.path)}"><span class="c ${CAT_CLS[f.cat]}"></span><span class="n">${esc(f.name)}</span><b>${fmtSize(f.size)}</b></button>`).join('') : `<p class="faint">${scanning ? 'Collecting…' : 'None over 32 MB here.'}</p>`}</section>`;
    // Selection bar.
    const items = map._items || [];
    const selItem = items.find(x => (x.kind === 'type' ? 'type:' + x.cat : x.kind + ':' + x.path + ':' + x.name) === dSel);
    main.querySelector('.tm-dfoot').innerHTML = selItem
      ? `<span class="grow"><b>${esc(selItem.name)}</b> · ${fmtSize(selItem.size)}${selItem.kind === 'dir' ? ` · ${selItem.files.toLocaleString()} files · mostly ${CATS[selItem.cat]}` : selItem.kind === 'type' ? ` · ${(selItem.size / catTotal * 100).toFixed(1)}% of this folder` : ''}<span class="tm-path">${esc(selItem.path)}</span></span>${selItem.kind === 'dir' && selItem.deeper ? `<button class="cc-btn" type="button" data-tm="disk-in">${ic('chev')}Open</button>` : ''}${selItem.path ? `<button class="cc-btn" type="button" data-tm="disk-reveal">${ic('folder')}Show in File Explorer</button>` : ''}`
      : `<span class="grow faint">Click a bubble to choose it; double-click a folder to go inside. Sizes are file sizes (what the files hold), as File Explorer shows them.</span>`;
  }
  const disk = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg tm-diskpg">${ctx.top('disk', `<span class="grow"></span>
          <label class="tm-sel">Drive<select class="tm-ddrive" aria-label="Drive"><option>${esc(drive)}</option></select></label>
          <div class="tm-seg" role="group" aria-label="Group by"><button type="button" data-dg="folders" aria-pressed="${dGroup === 'folders'}">Folders</button><button type="button" data-dg="types" aria-pressed="${dGroup === 'types'}">File types</button></div>
          <button class="cc-btn primary" type="button" data-tm="disk-scan">${ic('play')}Scan</button>`)}
        <div class="tm-dprog"></div>
        <div class="tm-dbody tm-disk"><div class="tm-dleft"><nav class="tm-dcrumbs"></nav><div class="tm-dmap"></div><div class="tm-foot tm-dfoot"></div></div><aside class="tm-dside"></aside></div></div>`;
      const mapEl = main.querySelector('.tm-dmap');
      mapEl.addEventListener('click', e => {
        const g = e.target.closest('g[data-b]'); if (!g) { dSel = null; drawDisk(main); return; }
        const x = mapEl._items[+g.dataset.b]; dSel = x.kind === 'type' ? 'type:' + x.cat : x.kind + ':' + x.path + ':' + x.name; drawDisk(main);
      });
      mapEl.addEventListener('dblclick', e => { const g = e.target.closest('g[data-b]'); if (!g) return; const x = mapEl._items[+g.dataset.b]; if (x.kind === 'dir' && x.deeper) openPath(x.path); });
      new ResizeObserver(() => { if (dView && main.isConnected) drawDisk(main); }).observe(mapEl);
      ctx.system().then(s => {
        volumes = s.volumes.filter(x => x.kind === 'Fixed' || x.kind === 'Removable');
        const sel = main.querySelector('.tm-ddrive'); if (!sel) return;
        if (!volumes.some(x => x.letter === drive) && volumes[0]) drive = volumes[0].letter;
        sel.innerHTML = volumes.map(x => `<option value="${esc(x.letter)}"${x.letter === drive ? ' selected' : ''}>${esc(x.letter)} ${esc(x.label || x.kind)} · ${fmtSize(x.total - x.free)} used</option>`).join('');
        drawDisk(main);
      }).catch(() => {});
      pollDisk();
    },
    click(e, b) {
      const main = mainEl();
      const g = e.target.closest('[data-dg]'); if (g) { dGroup = g.dataset.dg; main.querySelectorAll('[data-dg]').forEach(x => x.setAttribute('aria-pressed', x === g)); dSel = null; drawDisk(main); return; }
      const p = e.target.closest('[data-dpath]'); if (p) { openPath(p.dataset.dpath); return; }
      const f = e.target.closest('[data-dfile]'); if (f) { invoke('cc_open', { path: f.dataset.dfile, reveal: true }).catch(err => flash(String(err))); return; }
      if (!b) return;
      if (b.dataset.tm === 'disk-scan') {
        if (dStatus && dStatus.running) { invoke('disk_scan_stop', { clear: false }); return; }
        dView = null; dPath = ''; dSel = null;
        invoke('disk_scan_start', { root: drive + '\\' }).then(() => { lastViewAt = 0; setTimeout(pollDisk, 150); }).catch(err => flash(String(err)));
      } else if (b.dataset.tm === 'disk-in') {
        const x = (main.querySelector('.tm-dmap')._items || []).find(i => i.kind + ':' + i.path + ':' + i.name === dSel); if (x) openPath(x.path);
      } else if (b.dataset.tm === 'disk-reveal') {
        const items = main.querySelector('.tm-dmap')._items || [];
        const x = items.find(i => (i.kind === 'type' ? 'type:' + i.cat : i.kind + ':' + i.path + ':' + i.name) === dSel); if (x && x.path) invoke('cc_open', { path: x.path, reveal: x.kind === 'dir' }).catch(err => flash(String(err)));
      }
    },
    change(e) { if (e.target.closest('.tm-ddrive')) { drive = e.target.value; store.set('tmDiskDrive', drive); const m = mainEl(); if (m) drawDisk(m); } },
    key(e) {
      if (e.key === 'Backspace' && dView && dView.crumbs.length > 1 && !(e.target.matches && e.target.matches('input'))) { openPath(dView.crumbs[dView.crumbs.length - 2][1]); return 'used'; }
      return false;
    },
    leave() { clearTimeout(dT); },
    // The scan's tree can be tens of MB; it goes when the window does.
    closed() { clearTimeout(dT); invoke('disk_scan_stop', { clear: true }).catch(() => {}); dView = null; dStatus = null; dPath = ''; volumes = null; }
  };

  /* ---------- Benchmarks ---------- */
  const KINDS = [
    ['cpu', 'CPU', 'cpu'], ['memory', 'Memory', 'mem'], ['disk', 'Disk', 'disk'], ['gpu', 'Graphics', 'monitor'], ['net', 'Internet', 'net'], ['score', 'Island score', 'trophy']
  ];
  // Reference values (score 1000 each): roughly a 2024 mid-range desktop.
  const REF = { cpu_single: 400, cpu_multi: 5000, mem_copy: 25, mem_latency: 100, disk_read: 3500, disk_write: 3000, disk_iops: 15000, gpu_gflops: 4000, gpu_fill: 20, net_down: 200, net_up: 50 };
  const LOWER_BETTER = new Set(['mem_latency']);
  let bKind = store.get('tmBenchKind', 'cpu'), bStatus = null, bT = 0, bDrive = store.get('tmBenchDrive', 'C:'), runAll = null, gpuRun = null;
  let best = store.get('tmBench', {}); // the latest result of each measure, with when
  let log = store.get('tmBenchLog', []); // past runs
  function remember(kind, r) {
    const at = Date.now();
    for (const [k, v] of Object.entries(r)) if (typeof v === 'number' && v > 0 && k in REF) best[k] = { v, at };
    store.set('tmBench', best);
    log.unshift({ kind, at, r }); log = log.slice(0, 40); store.set('tmBenchLog', log);
  }
  function scoreOf(keys) {
    const parts = keys.filter(k => best[k]).map(k => LOWER_BETTER.has(k) ? REF[k] / best[k].v : best[k].v / REF[k]);
    if (!parts.length) return 0;
    return Math.round(1000 * Math.exp(parts.reduce((a, x) => a + Math.log(x), 0) / parts.length));
  }
  const GROUPS = { cpu: ['cpu_single', 'cpu_multi'], memory: ['mem_copy', 'mem_latency'], disk: ['disk_read', 'disk_write', 'disk_iops'], gpu: ['gpu_gflops', 'gpu_fill'], net: ['net_down', 'net_up'] };
  async function pollBench() {
    clearTimeout(bT);
    try { bStatus = await invoke('bench_status'); } catch (e) { return; }
    const main = mainEl();
    if (main && main.querySelector('.tm-bench')) drawBench(main);
    if (bStatus.running) bT = setTimeout(pollBench, 250);
    else if (bStatus._was) {
      bStatus._was = false;
    }
  }
  async function start(kind) {
    if (kind === 'gpu') return runGpu();
    try { await invoke('bench_start', { kind, drive: kind === 'disk' ? bDrive : null }); } catch (e) { flash(String(e)); return false; }
    // Wait for it to finish, then keep its results.
    await new Promise(res => {
      const wait = async () => {
        try { bStatus = await invoke('bench_status'); } catch (e) { res(); return; }
        const main = mainEl(); if (main && main.querySelector('.tm-bench')) drawBench(main);
        if (bStatus.running) setTimeout(wait, 250);
        else { if (bStatus.error) flash(bStatus.error); else if (bStatus.stage === 'Done') { const r = {}; for (const k of GROUPS[kind]) r[k] = bStatus.results[k]; if (kind === 'cpu') r.threads = bStatus.results.threads; if (kind === 'disk') r.drive = bStatus.results.disk_drive; if (kind === 'net') r.ping = bStatus.results.net_ping; remember(kind, r); } res(); }
      };
      setTimeout(wait, 150);
    });
    const main = mainEl(); if (main && main.querySelector('.tm-bench')) drawBench(main);
    return true;
  }
  async function runAllKinds() {
    if (runAll) return;
    const includeNet = !!(mainEl() && mainEl().querySelector('.tm-bnet input') && mainEl().querySelector('.tm-bnet input').checked);
    runAll = ['cpu', 'memory', 'disk', 'gpu', ...(includeNet ? ['net'] : [])];
    for (const k of runAll.slice()) {
      if (!runAll) break;
      bKind = k; const m = mainEl(); if (m) drawBench(m, true);
      await start(k);
    }
    runAll = null; bKind = 'score'; const m = mainEl(); if (m) drawBench(m, true);
  }

  /** The graphics test, in the page: WebGL2 on the card that draws the island. */
  async function runGpu() {
    if (gpuRun) return;
    gpuRun = { stage: 'Starting', progress: 0, live: 0, unit: 'GFLOPS' };
    const draw = () => { const m = mainEl(); if (m && m.querySelector('.tm-bench')) drawBench(m); };
    let gl, lose;
    try {
      const cv = document.createElement('canvas'); cv.width = cv.height = 8;
      gl = cv.getContext('webgl2', { antialias: false, depth: false, powerPreference: 'high-performance', preserveDrawingBuffer: false });
      if (!gl) throw new Error('WebGL 2 isn’t available here');
      lose = gl.getExtension('WEBGL_lose_context');
      const S = 2048;
      const tex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, tex); gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, S, S);
      const fb = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      gl.viewport(0, 0, S, S);
      const prog = (fs) => {
        const vs = `#version 300 es\nvoid main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(p*2.0-1.0,0.0,1.0);}`;
        const p = gl.createProgram();
        for (const [t, src] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) { const s = gl.createShader(t); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); gl.attachShader(p, s); }
        gl.linkProgram(p); if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
        return p;
      };
      const ITER = 256;
      // 4 lanes × (multiply + add) × 2 statements per round.
      const alu = prog(`#version 300 es\nprecision highp float;uniform float k;out vec4 o;void main(){vec4 v=vec4(gl_FragCoord.xy*1e-4,k,1.0);vec4 a=vec4(1.0001,0.9999,1.0002,0.9998);vec4 b=vec4(1e-4,2e-4,3e-4,4e-4);for(int i=0;i<${ITER};i++){v=v*a+b;v=v*a-b;}o=v;}`);
      const fill = prog(`#version 300 es\nprecision mediump float;uniform float k;out vec4 o;void main(){o=vec4(k,0.5,0.25,0.1);}`);
      const pix = new Uint8Array(4);
      const sync = () => gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pix);
      const measure = async (p, perFrame, secs, stage, unit, toValue, p0, p1) => {
        gpuRun.stage = stage; gpuRun.unit = unit;
        gl.useProgram(p); const ku = gl.getUniformLocation(p, 'k');
        for (let i = 0; i < 3; i++) { gl.uniform1f(ku, i * .01); gl.drawArrays(gl.TRIANGLES, 0, 3); } sync(); // warm up
        const t0 = performance.now(); let frames = 0;
        while (performance.now() - t0 < secs * 1000) {
          for (let i = 0; i < perFrame; i++) { gl.uniform1f(ku, (frames + i) * 1e-3); gl.drawArrays(gl.TRIANGLES, 0, 3); }
          sync(); frames += perFrame;
          const dt = (performance.now() - t0) / 1000;
          gpuRun.live = toValue(frames, dt); gpuRun.progress = p0 + (p1 - p0) * Math.min(1, dt / secs);
          draw();
          await new Promise(r => setTimeout(r, 0));
          if (gpuRun.cancel) return gpuRun.live;
        }
        return toValue(frames, (performance.now() - t0) / 1000);
      };
      const gflops = await measure(alu, 2, 4, 'Shader math', 'GFLOPS', (f, dt) => f * S * S * ITER * 16 / dt / 1e9, 0, .6);
      gl.enable(gl.BLEND); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      const gpix = gpuRun.cancel ? 0 : await measure(fill, 24, 3, 'Fill rate', 'Gpixels/s', (f, dt) => f * S * S / dt / 1e9, .6, 1);
      if (!gpuRun.cancel) remember('gpu', { gpu_gflops: gflops, gpu_fill: gpix });
    } catch (e) { flash(`Graphics test: ${e.message || e}`); }
    if (lose) lose.loseContext();
    gpuRun = null; draw();
  }

  function drawBench(main, rebuild) {
    const nav = main.querySelector('.tm-bnav');
    if (rebuild || !nav.innerHTML) nav.innerHTML = KINDS.map(([k, n, icn]) => `<button type="button" data-bk="${k}" aria-selected="${k === bKind}">${ic(icn)}<span>${n}</span><small>${k === 'score' ? (scoreOf(Object.keys(REF)) || '—') : scoreOf(GROUPS[k]) || ''}</small></button>`).join('');
    else nav.querySelectorAll('[data-bk]').forEach(b => { b.setAttribute('aria-selected', b.dataset.bk === bKind); const k = b.dataset.bk; b.querySelector('small').textContent = k === 'score' ? (scoreOf(Object.keys(REF)) || '—') : scoreOf(GROUPS[k]) || ''; });
    const body = main.querySelector('.tm-bbody');
    const S = bStatus || {}, live = S.running && S.kind === bKind ? S : gpuRun && bKind === 'gpu' ? gpuRun : null;
    const running = (S.running) || !!gpuRun;
    // A reading from this run (finished stages) before the last kept one.
    const val = k => (live && S.results && S.results[k] > 0 && S.kind === bKind ? S.results[k] : best[k] ? best[k].v : 0);
    const startBtn = `<button class="cc-btn primary big" type="button" data-tm="bench-${running ? 'stop' : 'start'}"${running && !live && !runAll ? ' disabled' : ''}>${running ? ic('stop') + 'Stop' : ic('play') + 'Start'}</button>`;
    const progress = `<div class="tm-bprog">${leds(live ? live.progress : 0, 40, { horizontal: true, cls: 'gpu' })}<small>${live ? esc(live.stage) : runAll ? 'Running everything…' : ''}</small></div>`;
    const when = keys => { const t = Math.max(0, ...keys.map(k => (best[k] ? best[k].at : 0))); return t ? `Last run ${fmtDate(t, true)}` : 'Not run yet'; };
    let html = '';
    if (bKind === 'cpu') {
      const sv = live && S.stage === 'Single core' ? S.live : val('cpu_single'), mv = live && S.stage && S.stage.startsWith('All') ? S.live : val('cpu_multi');
      html = `<h5>CPU benchmark<small>${when(GROUPS.cpu)}</small></h5><div class="tm-dials">${dial(sv, 1000, { label: 'Single core', unit: 'Mrounds/s' })}${dial(mv, 10000, { label: 'All threads', unit: 'Mrounds/s', ticks: 5 })}</div>
        ${progress}<div class="tm-brow">${startBtn}<p>One thread, then every logical processor, each for 4 seconds of the same integer and floating-point work. ${val('cpu_single') && val('cpu_multi') ? `All threads ran <b>${(val('cpu_multi') / val('cpu_single')).toFixed(1)}×</b> one.` : ''}</p></div>`;
    } else if (bKind === 'memory') {
      const cv = live && S.stage === 'Copy' ? S.live : val('mem_copy'), lv = live && S.stage === 'Latency' ? S.live : val('mem_latency');
      html = `<h5>Memory benchmark<small>${when(GROUPS.memory)}</small></h5><div class="tm-dials">${dial(cv, 60, { label: 'Copy', unit: 'GB/s', digits: 1 })}${dial(lv, 200, { label: 'Latency', unit: 'ns (lower is better)', digits: 1 })}</div>
        ${progress}<div class="tm-brow">${startBtn}<p>Copies a 256 MB block for 3 seconds, then follows a random chain through 128 MB, one step at a time.</p></div>`;
    } else if (bKind === 'disk') {
      const rv = live && S.stage === 'Sequential read' ? S.live : val('disk_read'), wv = live && S.stage === 'Sequential write' ? S.live : val('disk_write');
      const iops = live && S.stage === 'Random 4 KB reads' ? S.live : val('disk_iops');
      const vols = (ctx.sysInfo && ctx.sysInfo.volumes || []).filter(v => v.kind === 'Fixed');
      html = `<h5>Disk benchmark<small>${when(GROUPS.disk)}</small></h5><div class="tm-dials">${dial(rv, 8000, { label: 'Read', unit: 'MB/s' })}${dial(wv, 8000, { label: 'Write', unit: 'MB/s' })}</div>
        <div class="tm-bstat"><small>Random 4 KB reads</small><b>${Math.round(iops).toLocaleString()} IOPS</b><small>${(iops * 4096 / 1048576).toFixed(0)} MB/s</small></div>
        ${progress}<div class="tm-brow">${startBtn}<label class="tm-sel">Drive<select class="tm-bdrive">${(vols.length ? vols : [{ letter: bDrive, label: '' }]).map(v => `<option value="${esc(v.letter)}"${v.letter === bDrive ? ' selected' : ''}>${esc(v.letter)} ${esc(v.label || '')}</option>`).join('')}</select></label>
        <p>Writes a 1 GB test file straight to the drive and reads it back (no cache), then 4 KB reads at random places for 4 seconds. The file is deleted afterwards; the drive needs 2 GB free.</p></div>`;
    } else if (bKind === 'gpu') {
      const g = gpuRun;
      const fv = g && g.stage === 'Shader math' ? g.live : val('gpu_gflops'), pv = g && g.stage === 'Fill rate' ? g.live : val('gpu_fill');
      html = `<h5>Graphics benchmark<small>${when(GROUPS.gpu)}</small></h5><div class="tm-dials">${dial(fv, 20000, { label: 'Shader math', unit: 'GFLOPS' })}${dial(pv, 100, { label: 'Fill rate', unit: 'Gpixels/s', digits: 1 })}</div>
        ${progress}<div class="tm-brow">${startBtn}<p>Runs in this window through WebGL, on the card that draws it: a heavy math shader over 4 million pixels, then blended fills. Browser graphics run below what games get, so compare it with itself.</p></div>`;
    } else if (bKind === 'net') {
      const dv = live && S.stage === 'Download' ? S.live : val('net_down'), uv = live && S.stage === 'Upload' ? S.live : val('net_up');
      const ping = live && S.stage === 'Latency' ? S.live : (S.results && S.results.net_ping) || (log.find(x => x.kind === 'net') || { r: {} }).r.ping || 0;
      html = `<h5>Internet speed<small>${when(GROUPS.net)}</small></h5><div class="tm-dials">${dial(dv, Math.max(1000, Math.ceil(dv / 500) * 500), { label: 'Download', unit: 'Mbps' })}${dial(uv, Math.max(500, Math.ceil(uv / 250) * 250), { label: 'Upload', unit: 'Mbps' })}</div>
        <div class="tm-bstat"><small>Latency</small><b>${ping ? ping.toFixed(0) + ' ms' : '—'}</b><small>HTTPS round trip</small></div>
        ${progress}<div class="tm-brow">${startBtn}<p>Uses Cloudflare’s public speed test (speed.cloudflare.com): eight small requests for latency, then four connections downloading and four uploading for several seconds. Nothing else is sent.</p></div>`;
    } else {
      const total = scoreOf(Object.keys(REF));
      html = `<h5>Island score<small>1,000 is about a 2024 mid-range desktop</small></h5>
        <div class="tm-score"><b>${total || '—'}</b><small>${total ? 'overall' : 'Run the benchmarks to get a score'}</small></div>
        <div class="tm-scores">${Object.entries(GROUPS).map(([k, keys]) => { const sc = scoreOf(keys); return `<div><span>${KINDS.find(x => x[0] === k)[1]}</span><span class="bar"><i style="width:${clamp(sc / 2000, 0, 1) * 100}%"></i></span><b>${sc || '—'}</b></div>`; }).join('')}</div>
        <div class="tm-brow"><button class="cc-btn primary big" type="button" data-tm="bench-all"${running ? ' disabled' : ''}>${ic('play')}${runAll ? 'Running…' : 'Run all'}</button><label class="tm-chk tm-bnet"><input type="checkbox"${store.get('tmBenchNet', false) ? ' checked' : ''}><span>Include the internet test</span></label></div>
        <table class="tm-grid tm-blog"><thead><tr><th>When</th><th>Test</th><th>Result</th></tr></thead><tbody>${log.slice(0, 14).map(x => `<tr><td>${fmtDate(x.at, true)}</td><td>${esc((KINDS.find(k => k[0] === x.kind) || [, x.kind])[1])}</td><td class="mono">${esc(describe(x))}</td></tr>`).join('') || '<tr class="tm-wait"><td colspan="3">No runs yet.</td></tr>'}</tbody></table>`;
    }
    body.innerHTML = html;
  }
  function describe(x) {
    const r = x.r;
    switch (x.kind) {
      case 'cpu': return `${Math.round(r.cpu_single)} / ${Math.round(r.cpu_multi).toLocaleString()} Mrounds/s (${r.threads} threads)`;
      case 'memory': return `${r.mem_copy.toFixed(1)} GB/s copy · ${r.mem_latency.toFixed(0)} ns`;
      case 'disk': return `${r.drive} read ${Math.round(r.disk_read).toLocaleString()} · write ${Math.round(r.disk_write).toLocaleString()} MB/s · ${Math.round(r.disk_iops).toLocaleString()} IOPS`;
      case 'gpu': return `${Math.round(r.gpu_gflops).toLocaleString()} GFLOPS · ${r.gpu_fill.toFixed(1)} Gpix/s`;
      case 'net': return `↓ ${r.net_down.toFixed(0)} · ↑ ${r.net_up.toFixed(0)} Mbps · ${r.ping ? r.ping.toFixed(0) + ' ms' : ''}`;
      default: return '';
    }
  }
  const bench = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg tm-benchpg">${ctx.top('bench', '<small>Nothing runs until you press Start</small>')}<div class="tm-bench"><nav class="tm-bnav"></nav><div class="tm-bbody"></div></div></div>`;
      if (!ctx.sysInfo) ctx.system().then(() => { const m = mainEl(); if (m && m.querySelector('.tm-bench') && bKind === 'disk') drawBench(m); }).catch(() => {});
      drawBench(main, true);
      pollBench();
    },
    click(e, b) {
      const main = mainEl();
      const k = e.target.closest('[data-bk]'); if (k) { bKind = k.dataset.bk; store.set('tmBenchKind', bKind); drawBench(main); return; }
      if (!b) return;
      if (b.dataset.tm === 'bench-start') { if (bKind !== 'score') start(bKind); setTimeout(() => drawBench(main), 60); }
      else if (b.dataset.tm === 'bench-stop') { runAll = null; if (gpuRun) gpuRun.cancel = true; invoke('bench_cancel').catch(() => {}); }
      else if (b.dataset.tm === 'bench-all') runAllKinds();
    },
    change(e) {
      if (e.target.closest('.tm-bdrive')) { bDrive = e.target.value; store.set('tmBenchDrive', bDrive); }
      if (e.target.closest('.tm-bnet')) store.set('tmBenchNet', e.target.checked);
    },
    leave() { clearTimeout(bT); },
    closed() { runAll = null; if (gpuRun) gpuRun.cancel = true; invoke('bench_cancel').catch(() => {}); }
  };

  return { disk, bench };
}
