// Task Manager pages: Services (start, stop and restart them), Power & Freq (every sensor Windows
// shares, with the lowest and highest since the page opened), Connections (who's talking to whom)
// and Installed Apps (everything Settings > Apps lists, with uninstall).

import { GB, MB, clamp, fmtMem, fmtRate, fmtBits, fmtGHz, fmtDate, fmtSize, heat, thead, sorter } from './tm-util.js';

export function createSystemPages(ctx) {
  const { esc, ic, invoke, flash, assetUrl, store } = ctx;
  const icon = (src, name, cls = 'tm-app') => (src ? `<img class="${cls}" src="${esc(assetUrl(src))}" alt="" draggable="false">` : `<span class="${cls} tm-glyph">${esc((name || '?').trim().charAt(0).toUpperCase())}</span>`);
  const mainEl = () => document.querySelector('.tmw .tm-main');
  /** A table whose rows are chosen on press (it redraws while you look at it). */
  function pressRows(tbl, onSort, onPick) {
    tbl.addEventListener('pointerdown', e => {
      if (e.button !== 0) return;
      const th = e.target.closest('th[data-sort]'); if (th) { onSort(th.dataset.sort); return; }
      const tr = e.target.closest('tr[data-k]'); if (tr) onPick(tr.dataset.k);
    });
  }
  const nextSort = (s, k, textKeys) => (s.k === k ? { k, d: -s.d } : { k, d: textKeys.includes(k) ? 1 : -1 });

  /* ---------- Services ---------- */
  let svcs = null, sSort = store.get('tmSvcSort', { k: 'name', d: 1 }), sq = '', sFilter = 'all', sSel = null, sConfirm = null, sT = 0, sBusy = '';
  const SCOLS = [['state', '', 'dot'], ['name', 'Name'], ['pid', 'PID', 'n'], ['description', 'Description'], ['status', 'Status'], ['start', 'Startup type'], ['group', 'Group']];
  async function loadSvcs(fresh) {
    try { svcs = await invoke('tm_services', { fresh: !!fresh }); } catch (e) { flash(String(e)); svcs = svcs || []; }
    const m = mainEl(); if (ctx.open && m && m.querySelector('.tm-svc')) drawSvcs(m);
  }
  function drawSvcs(main) {
    if (!svcs) return;
    const ql = sq.trim().toLowerCase();
    const list = svcs.filter(s => (sFilter === 'all' || (sFilter === 'running' ? s.state === 'Running' : s.state !== 'Running')) && (!ql || s.name.toLowerCase().includes(ql) || s.display.toLowerCase().includes(ql) || s.description.toLowerCase().includes(ql) || String(s.pid) === ql || s.group.toLowerCase().includes(ql)))
      .sort(sorter(sSort, (x, k) => (k === 'status' ? x.state : k === 'state' ? (x.state === 'Running' ? 1 : 0) : x[k])));
    const running = svcs.filter(s => s.state === 'Running').length;
    main.querySelector('.tm-svcn').textContent = `${running} running of ${svcs.length}`;
    main.querySelector('.tm-svc thead').innerHTML = thead(esc, SCOLS, sSort);
    const scroll = main.querySelector('.tm-tbl').scrollTop;
    main.querySelector('.tm-svc tbody').innerHTML = list.map(s => `<tr data-k="${esc(s.name)}" class="${s.name === sSel ? 'sel' : ''}${s.state === 'Running' ? ' on' : ''}" title="${esc(s.display)}"><td class="dot"><i class="${s.state === 'Running' ? 'on' : s.state.endsWith('ing') ? 'mid' : ''}"></i></td><td class="nm"><b>${esc(s.display)}</b><small>${esc(s.name)}</small></td><td class="n">${s.pid || ''}</td><td class="desc">${esc(s.description)}</td><td>${esc(s.state)}</td><td>${esc(s.start)}</td><td>${esc(s.group)}</td></tr>`).join('') || `<tr class="tm-wait"><td colspan="7">No services match.</td></tr>`;
    main.querySelector('.tm-tbl').scrollTop = scroll;
    drawSvcFoot(main);
  }
  function drawSvcFoot(main) {
    const s = svcs && svcs.find(x => x.name === sSel), foot = main.querySelector('.tm-svcfoot');
    if (!s) { foot.className = 'tm-foot tm-svcfoot'; foot.innerHTML = `<span class="grow tm-none">Select a service for its details. Starting or stopping most services needs administrator rights: Windows asks first.</span>`; return; }
    foot.className = 'tm-foot tm-svcfoot tm-detail on';
    if (sConfirm) {
      foot.innerHTML = `<span class="grow tm-confirm">${sConfirm === 'stop' ? 'Stop' : 'Restart'} ${esc(s.display)}? Programs that rely on it may stop working until it runs again.</span><button class="cc-btn" type="button" data-tm="svc-cancel">Cancel</button><button class="cc-btn danger-fill" type="button" data-tm="svc-go">${sConfirm === 'stop' ? 'Stop' : 'Restart'}</button>`;
      return;
    }
    const running = s.state === 'Running';
    foot.innerHTML = `<div class="tm-dhead"><span class="tm-app tm-glyph">${ic('wrench')}</span><span><b>${esc(s.display)}</b><span class="tm-path" title="${esc(s.path)}">${esc(s.path || s.name)}</span></span></div>
      <div class="tm-dfacts"><div><small>Name</small><b>${esc(s.name)}</b></div><div><small>Status</small><b>${esc(s.state)}${s.pid ? ' · PID ' + s.pid : ''}</b></div><div><small>Startup type</small><b>${esc(s.start || '—')}</b></div><div><small>Log on as</small><b>${esc(s.account || '—')}</b></div><div><small>Group</small><b>${esc(s.group || '—')}</b></div></div>
      ${s.description ? `<div class="tm-dcmd"><small>Description</small><p>${esc(s.description)}</p></div>` : ''}
      <div class="tm-dacts">${sBusy ? `<span class="tm-busy">${esc(sBusy)}</span>` : ''}<button class="cc-btn" type="button" data-tm="svc-start"${running || s.start === 'Disabled' ? ' disabled' : ''}${s.start === 'Disabled' ? ' title="It’s disabled"' : ''}>${ic('play')}Start</button><button class="cc-btn" type="button" data-tm="svc-stop"${running && s.stoppable ? '' : ' disabled'}>${ic('stop')}Stop</button><button class="cc-btn" type="button" data-tm="svc-restart"${running && s.stoppable ? '' : ' disabled'}>${ic('restart')}Restart</button>${s.pid ? `<button class="cc-btn" type="button" data-tm="svc-proc">${ic('grid')}Its process</button>` : ''}</div>`;
  }
  async function svcAct(action) {
    const s = svcs && svcs.find(x => x.name === sSel); if (!s) return;
    sConfirm = null; sBusy = action === 'start' ? 'Starting…' : action === 'stop' ? 'Stopping…' : 'Restarting…';
    const m = mainEl(); if (m) drawSvcFoot(m);
    try {
      const r = await invoke('tm_service', { name: s.name, action });
      flash(r === 'asked' ? `Windows asked for permission to ${action} ${s.display}` : `${s.display}: ${action === 'start' ? 'started' : action === 'stop' ? 'stopped' : 'restarted'}`);
    } catch (e) { flash(String(e)); }
    sBusy = '';
    setTimeout(() => loadSvcs(false), 800); setTimeout(() => loadSvcs(false), 3000);
  }
  const services = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg tm-svcpg">${ctx.top('services', `<small class="tm-svcn"></small><span class="grow"></span>
          <div class="tm-seg" role="group" aria-label="Show">${[['all', 'All'], ['running', 'Running'], ['stopped', 'Stopped']].map(([k, n]) => `<button type="button" data-sf="${k}" aria-pressed="${k === sFilter}">${n}</button>`).join('')}</div>
          <button class="cc-btn" type="button" data-tm="run">${ic('plus')}Run new task</button><button class="cc-btn" type="button" data-tm="svc-msc">${ic('ext')}Services app</button><button class="cc-btn" type="button" data-tm="svc-refresh">${ic('restart')}Refresh</button>
          <label class="tm-search">${ic('search')}<input type="text" class="tm-sq" placeholder="Filter by name, PID or group" spellcheck="false" autocomplete="off" aria-label="Filter services"></label>`)}
        <div class="tm-tbl"><table class="tm-grid tm-svc"><colgroup><col class="dot"><col class="nm"><col class="n"><col class="desc"><col class="st"><col class="start"><col class="grp"></colgroup><thead></thead><tbody><tr class="tm-wait"><td colspan="7">Reading…</td></tr></tbody></table></div><div class="tm-foot tm-svcfoot"></div></div>`;
      const qi = main.querySelector('.tm-sq'); qi.value = sq;
      qi.addEventListener('input', () => { sq = qi.value; drawSvcs(main); });
      pressRows(main.querySelector('.tm-svc'), k => { sSort = nextSort(sSort, k, ['name', 'description', 'status', 'start', 'group']); store.set('tmSvcSort', sSort); drawSvcs(main); }, k => { sSel = k; sConfirm = null; drawSvcs(main); });
      loadSvcs(false);
      clearInterval(sT); sT = setInterval(() => loadSvcs(false), 3000);
    },
    click(e, b) {
      const main = mainEl();
      const f = e.target.closest('[data-sf]'); if (f) { sFilter = f.dataset.sf; main.querySelectorAll('[data-sf]').forEach(x => x.setAttribute('aria-pressed', x === f)); drawSvcs(main); return; }
      if (!b) return;
      const k = b.dataset.tm;
      if (k === 'svc-refresh') loadSvcs(true);
      else if (k === 'svc-msc') invoke('tm_run', { command: 'services.msc', admin: false }).catch(err => flash(String(err)));
      else if (k === 'svc-start') svcAct('start');
      else if (k === 'svc-stop' || k === 'svc-restart') { sConfirm = k === 'svc-stop' ? 'stop' : 'restart'; drawSvcFoot(main); }
      else if (k === 'svc-go') svcAct(sConfirm);
      else if (k === 'svc-cancel') { sConfirm = null; drawSvcFoot(main); }
      else if (k === 'svc-proc') { const s = svcs.find(x => x.name === sSel); if (s && s.pid) ctx.showProcess(s.pid); }
    },
    key(e) {
      if (e.key === 'Escape' && sConfirm) { sConfirm = null; const m = mainEl(); if (m) drawSvcFoot(m); return 'used'; }
      if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && svcs) {
        const rows = [...document.querySelectorAll('.tm-svc tbody tr[data-k]')]; if (!rows.length) return 'used';
        let i = rows.findIndex(r => r.dataset.k === sSel); i = clamp(i + (e.key === 'ArrowDown' ? 1 : -1), 0, rows.length - 1);
        sSel = rows[i].dataset.k; sConfirm = null; const m = mainEl(); drawSvcs(m); const r = m.querySelector('.tm-svc tr.sel'); if (r) r.scrollIntoView({ block: 'nearest' });
        return 'used';
      }
      return false;
    },
    leave() { clearInterval(sT); },
    closed() { svcs = null; sSel = null; }
  };

  /* ---------- Power & Freq ---------- */
  let nvme = null, nT = 0;
  const mm = new Map(); // sensor id → [min, max]
  const collapsed = new Set(store.get('tmSensClosed', ['cpu-u', 'cpu-c']));
  function track(id, v) { if (v == null || !Number.isFinite(v)) return; const m = mm.get(id); if (!m) mm.set(id, [v, v]); else { if (v < m[0]) m[0] = v; if (v > m[1]) m[1] = v; } }
  const F = {
    pct: v => v.toFixed(1) + '%', temp: v => v.toFixed(1) + ' °C', mhz: v => (v >= 1000 ? fmtGHz(v) : Math.round(v) + ' MHz'), rpm: v => Math.round(v).toLocaleString() + ' RPM',
    rate: v => fmtRate(v), v: v => v.toFixed(3) + ' V', gb: v => v.toFixed(1) + ' GB', n: v => Math.round(v).toLocaleString(), h: v => Math.round(v).toLocaleString() + ' h', min: v => Math.round(v).toLocaleString() + ' min', bytes: v => fmtSize(v)
  };
  function sensorTree() {
    const S = ctx.snap, st = ctx.stats || {}, sen = st.sensors || {}, C = ctx.cpu || {}, P = S && S.perf;
    const sys = ctx.sysInfo;
    const kids = [];
    if (P) {
      const cores = P.cores.map((v, i) => ({ id: 'cu' + i, n: `CPU ${i}`, v, f: 'pct', bar: v / 100 }));
      const clocks = (P.cores_mhz || []).map((v, i) => ({ id: 'cc' + i, n: `Core ${i}`, v, f: 'mhz', bar: v / Math.max(5000, C.base_mhz * 1.3 || 5000) }));
      kids.push({ id: 'cpu', n: C.name || st.cpu_name || 'Processor', icon: 'cpu', kids: [
        { id: 'cpu-t', n: 'Temperatures', icon: 'thermo', kids: sen.cpu_temp != null ? [{ id: 'ct', n: 'CPU chip', v: sen.cpu_temp, f: 'temp', bar: sen.cpu_temp / 100, tip: 'Read through the graphics built into the same chip' }] : [{ id: 'ct-na', n: 'Not shared by this CPU', note: true }] },
        { id: 'cpu-u', n: 'Utilization', icon: 'pulse', v: P.cpu, f: 'pct', kids: [{ id: 'cu', n: 'Total', v: P.cpu, f: 'pct', bar: P.cpu / 100 }, { id: 'ck', n: 'Kernel time', v: P.kernel || 0, f: 'pct', bar: (P.kernel || 0) / 100 }, ...cores] },
        { id: 'cpu-c', n: 'Clocks', icon: 'gauge', v: P.mhz, f: 'mhz', kids: [{ id: 'cca', n: 'Average (effective)', v: P.mhz, f: 'mhz' }, C.base_mhz && { id: 'ccb', n: 'Base', v: C.base_mhz, f: 'mhz', fixed: true }, ...clocks].filter(Boolean) },
        { id: 'cpu-p', n: 'Power', icon: 'bolt', kids: [{ id: 'cp-na', n: 'Package power needs a hardware driver', note: true }] }
      ] });
    }
    for (const [i, g] of (sen.gpus || []).slice().sort((a, b) => a.integrated - b.integrated).entries()) {
      const info = sys && sys.gpus.find(x => x.name === g.name);
      const rows = [
        g.temp != null && { id: `g${i}t`, n: 'Temperature', v: g.temp, f: 'temp', bar: g.temp / 100 },
        !g.integrated && P && { id: `g${i}u`, n: 'Utilization', v: P.gpu, f: 'pct', bar: P.gpu / 100 },
        g.core_mhz != null && { id: `g${i}c`, n: 'Core clock', v: g.core_mhz, f: 'mhz', bar: g.core_max_mhz ? g.core_mhz / g.core_max_mhz : null },
        g.mem_mhz != null && { id: `g${i}m`, n: 'Memory clock', v: g.mem_mhz, f: 'mhz' },
        g.fan_rpm != null && { id: `g${i}f`, n: 'Fan', v: g.fan_rpm, f: 'rpm', bar: g.fan_max ? g.fan_rpm / g.fan_max : null, tip: g.fan_rpm === 0 ? 'Stopped (it idles quietly)' : '' },
        g.power_pct != null && { id: `g${i}p`, n: 'Power (share of limit)', v: g.power_pct, f: 'pct', bar: g.power_pct / 100 },
        g.voltage_mv != null && { id: `g${i}v`, n: 'Core voltage', v: g.voltage_mv / 1000, f: 'v' },
        !g.integrated && st.vram_total && { id: `g${i}vr`, n: 'Dedicated memory used', v: st.vram_used || 0, f: 'gb', bar: (st.vram_used || 0) / st.vram_total },
        info && { id: `g${i}d`, n: 'Driver', text: info.driver }
      ].filter(Boolean);
      kids.push({ id: 'gpu' + i, n: g.name, icon: 'monitor', kids: rows });
    }
    // Drives: live numbers by disk number, temperatures by letter, health from the NVMe log.
    const disks = (sys && sys.disks) || [];
    const perfDisks = P ? P.disks : [];
    const seen = new Set();
    for (const d of disks) {
      seen.add(d.number);
      const pd = perfDisks.find(x => x.name.split(' ')[0] === String(d.number));
      const dr = (sen.drives || []).find(x => x.letters && d.letters.some(l => x.letters.includes(l)));
      const hlt = (nvme || []).find(x => x.number === d.number);
      const smart = hlt ? [
        { id: `d${d.number}h`, n: 'Health', text: hlt.critical_warning ? 'Warning (critical flag set)' : 'Healthy', good: !hlt.critical_warning },
        { id: `d${d.number}as`, n: 'Available spare', v: hlt.spare, f: 'pct', fixed: true },
        { id: `d${d.number}at`, n: 'Available spare threshold', v: hlt.spare_threshold, f: 'pct', fixed: true },
        { id: `d${d.number}pu`, n: 'Percentage used (wear)', v: hlt.used, f: 'pct', fixed: true, bar: hlt.used / 100 },
        { id: `d${d.number}dr`, n: 'Data read', v: hlt.read, f: 'bytes', fixed: true },
        { id: `d${d.number}dw`, n: 'Data written', v: hlt.written, f: 'bytes', fixed: true },
        { id: `d${d.number}hr`, n: 'Host read commands', v: hlt.host_reads, f: 'n', fixed: true },
        { id: `d${d.number}hw`, n: 'Host write commands', v: hlt.host_writes, f: 'n', fixed: true },
        { id: `d${d.number}cb`, n: 'Controller busy time', v: hlt.busy_minutes, f: 'min', fixed: true },
        { id: `d${d.number}pc`, n: 'Power cycles', v: hlt.power_cycles, f: 'n', fixed: true },
        { id: `d${d.number}ph`, n: 'Power-on hours', v: hlt.power_on_hours, f: 'h', fixed: true },
        { id: `d${d.number}us`, n: 'Unsafe shutdowns', v: hlt.unsafe_shutdowns, f: 'n', fixed: true },
        { id: `d${d.number}me`, n: 'Media and data integrity errors', v: hlt.media_errors, f: 'n', fixed: true, bad: hlt.media_errors > 0 },
        { id: `d${d.number}el`, n: 'Error log entries', v: hlt.error_log, f: 'n', fixed: true },
        { id: `d${d.number}wt`, n: 'Warning temperature time', v: hlt.warning_temp_minutes, f: 'min', fixed: true },
        { id: `d${d.number}ct`, n: 'Critical temperature time', v: hlt.critical_temp_minutes, f: 'min', fixed: true, bad: hlt.critical_temp_minutes > 0 }
      ] : [{ id: `d${d.number}na`, n: d.bus === 'NVMe' ? 'Reading…' : 'Health is shared by NVMe drives only', note: true }];
      kids.push({ id: 'disk' + d.number, n: `${d.model}${d.letters.length ? ' (' + d.letters.join(' ') + ')' : ''}`, icon: 'disk', kids: [
        { id: `d${d.number}-t`, n: 'Temperatures', icon: 'thermo', v: dr ? dr.temp : null, f: 'temp', kids: dr ? [{ id: `d${d.number}tc`, n: 'Composite', v: dr.temp, f: 'temp', bar: dr.temp / (dr.critical || 85), tip: `Warning at ${dr.warning} °C, critical at ${dr.critical} °C` }, dr.hottest > dr.temp && { id: `d${d.number}th`, n: 'Hottest sensor', v: dr.hottest, f: 'temp', bar: dr.hottest / (dr.critical || 85) }].filter(Boolean) : [{ id: `d${d.number}tna`, n: 'Not shared', note: true }] },
        pd && { id: `d${d.number}-u`, n: 'Utilization', icon: 'pulse', kids: [{ id: `d${d.number}a`, n: 'Active time', v: pd.active, f: 'pct', bar: pd.active / 100 }] },
        pd && { id: `d${d.number}-s`, n: 'Speed', icon: 'gauge', kids: [{ id: `d${d.number}r`, n: 'Read', v: pd.read, f: 'rate' }, { id: `d${d.number}w`, n: 'Write', v: pd.write, f: 'rate' }] },
        { id: `d${d.number}-h`, n: 'S.M.A.R.T.', icon: 'shield', text: hlt ? (hlt.critical_warning ? 'Warning' : 'Healthy') : '', kids: smart }
      ].filter(Boolean) });
    }
    if (sys) {
      kids.push({ id: 'board', n: sys.board || 'Motherboard', icon: 'cpu', kids: [{ id: 'b-na', n: 'Fans, voltages and board temperatures need a hardware driver, which isn’t installed (Windows doesn’t share them otherwise)', note: true }] });
      const mods = sys.modules || [];
      kids.push({ id: 'mem', n: `${fmtMem(sys.mem_installed)} ${mods[0] ? mods[0].kind : ''} memory`.trim(), icon: 'mem', kids: [
        P && { id: 'm-u', n: 'Used', v: P.mem_used / GB, f: 'gb', bar: P.mem_used / P.mem_total },
        P && { id: 'm-p', n: 'Load', v: P.mem_used / P.mem_total * 100, f: 'pct' },
        { id: 'm-c', n: 'Clocks', icon: 'gauge', kids: mods.map((m, i) => ({ id: 'mc' + i, n: m.slot, v: m.configured || m.speed, f: 'mhz', fixed: true, tip: `${m.configured || m.speed} MT/s (the clock is half: ${(m.configured || m.speed) / 2} MHz) · ${[m.maker, m.part].filter(Boolean).join(' ')}` })) },
        { id: 'm-t', n: 'Temperatures need a hardware driver', note: true }
      ].filter(Boolean) });
    }
    return { id: 'pc', n: sys ? sys.computer : 'This PC', icon: 'monitor', kids };
  }
  function trackAll(node) { if (node.v != null && !node.fixed) track(node.id, node.v); (node.kids || []).forEach(trackAll); }
  function rowsOf(node, depth, out) {
    const open = !collapsed.has(node.id), hasKids = node.kids && node.kids.length;
    out.push({ node, depth, open, hasKids });
    if (hasKids && open) node.kids.forEach(k => rowsOf(k, depth + 1, out));
    return out;
  }
  const fmt = n => (n.v == null ? '' : (F[n.f] || (x => x))(n.v));
  const sensors = {
    sample() { if (ctx.open) { const t = sensorTree(); trackAll(t); } },
    render(main, first) {
      if (first) {
        main.innerHTML = `<div class="tm-pg tm-senspg">${ctx.top('sensors', '<small>Everything Windows shares without a driver</small><span class="grow"></span><button class="cc-btn" type="button" data-tm="sens-reset">' + ic('restart') + 'Reset min / max</button><button class="cc-btn" type="button" data-tm="sens-all">Expand all</button><button class="cc-btn" type="button" data-tm="sens-none">Collapse all</button>')}
          <div class="tm-tbl"><table class="tm-grid tm-sens"><colgroup><col><col class="v"><col class="v"><col class="v"></colgroup><thead><tr><th>Sensor</th><th class="n">Value</th><th class="n">Min</th><th class="n">Max</th></tr></thead><tbody></tbody></table></div></div>`;
        main.querySelector('.tm-sens').addEventListener('pointerdown', e => {
          const tr = e.button === 0 && e.target.closest('tr[data-id].kids'); if (!tr) return;
          const id = tr.dataset.id; collapsed.has(id) ? collapsed.delete(id) : collapsed.add(id); store.set('tmSensClosed', [...collapsed]); sensors.render(main, false);
        });
        ctx.system().then(() => { if (ctx.open && main.querySelector('.tm-sens')) sensors.render(main, false); }).catch(() => {});
        const loadN = () => invoke('tm_nvme').then(n => { nvme = n; }).catch(() => { nvme = []; });
        loadN(); clearInterval(nT); nT = setInterval(loadN, 15000);
      }
      const tree = sensorTree();
      const rows = rowsOf(tree, 0, []);
      const tb = main.querySelector('.tm-sens tbody'), scroll = main.querySelector('.tm-tbl').scrollTop;
      tb.innerHTML = rows.map(({ node: n, depth, open, hasKids }) => {
        const m = mm.get(n.id);
        const bar = n.bar != null && Number.isFinite(n.bar) ? `<span class="sb"><i style="width:${(clamp(n.bar, 0, 1) * 100).toFixed(1)}%" class="${n.bar > .85 ? 'hot' : ''}"></i></span>` : '';
        const value = n.text != null ? `<span class="${n.good ? 'good' : n.bad ? 'bad' : ''}">${esc(n.text)}</span>` : n.v != null ? `${bar}<span class="${n.bad ? 'bad' : ''}">${esc(fmt(n))}</span>` : '';
        return `<tr data-id="${esc(n.id)}" class="d${Math.min(depth, 4)}${hasKids ? ' kids' : ''}${n.note ? ' note' : ''}"${n.tip ? ` title="${esc(n.tip)}"` : ''}><td><span class="tn" style="--d:${depth}">${hasKids ? `<i class="tw${open ? ' open' : ''}">${ic('chev')}</i>` : '<i class="tw sp"></i>'}${n.icon ? `<span class="si">${ic(n.icon)}</span>` : depth > 0 && !n.note ? '<span class="sdot"></span>' : ''}<span>${esc(n.n)}</span></span></td><td class="n">${value}</td><td class="n">${m && !n.fixed ? esc((F[n.f] || (x => x))(m[0])) : ''}</td><td class="n">${m && !n.fixed ? esc((F[n.f] || (x => x))(m[1])) : ''}</td></tr>`;
      }).join('');
      main.querySelector('.tm-tbl').scrollTop = scroll;
    },
    click(e, b) {
      if (!b) return;
      const main = mainEl();
      if (b.dataset.tm === 'sens-reset') { mm.clear(); sensors.render(main, false); }
      else if (b.dataset.tm === 'sens-all') { collapsed.clear(); store.set('tmSensClosed', []); sensors.render(main, false); }
      else if (b.dataset.tm === 'sens-none') { const t = sensorTree(); const walk = n => { if (n.kids && n.kids.length && n.id !== 'pc') collapsed.add(n.id); (n.kids || []).forEach(walk); }; walk(t); store.set('tmSensClosed', [...collapsed]); sensors.render(main, false); }
    },
    leave() { clearInterval(nT); },
    closed() { mm.clear(); nvme = null; }
  };

  /* ---------- Connections ---------- */
  let conns = null, cT = 0, cFilter = 'all', cq = '', cSort = store.get('tmConnSort', { k: 'proc', d: 1 }), cSel = null;
  const CCOLS = [['proc', 'Process'], ['proto', 'Protocol'], ['local', 'Local address'], ['remote', 'Remote address'], ['host', 'Remote host'], ['state', 'State'], ['service', 'Service']];
  const keyOf = c => `${c.proto}|${c.local}|${c.lport}|${c.remote}|${c.rport}|${c.pid}`;
  const addr = (a, p) => (a ? (a.includes(':') ? `[${a}]` : a) + (p ? ':' + p : '') : '');
  async function loadConns() {
    try { conns = await invoke('tm_conns'); } catch (e) { conns = conns || []; }
    const m = mainEl(); if (ctx.open && m && m.querySelector('.tm-conn')) drawConns(m);
  }
  function procOf(pid) { const S = ctx.snap; if (!S) return null; for (const g of S.groups) for (const p of g.procs) if (p.pid === pid) return { g, p }; return null; }
  function drawConns(main) {
    if (!conns) return;
    const S = ctx.snap, P = S && S.perf;
    const connected = conns.filter(c => c.state === 'Connected'), listening = conns.filter(c => c.state === 'Listening');
    const pub = conns.filter(c => c.scope === 'public'), procs = new Set(conns.map(c => c.pid));
    const rx = P ? P.nets.reduce((a, n) => a + n.rx, 0) : 0, tx = P ? P.nets.reduce((a, n) => a + n.tx, 0) : 0;
    main.querySelector('.tm-ctiles').innerHTML = [['Connections', connected.length, 'TCP connections open now'], ['Listening', listening.length, 'Ports waiting for connections'], ['Public', pub.length, 'To or from the internet'], ['Processes', procs.size, 'Programs with any network endpoint'], ['System traffic ↓ / ↑', `${fmtRate(rx)} / ${fmtRate(tx)}`, 'All adapters']].map(([k, v, t]) => `<div class="tm-ctile" title="${t}"><small>${k}</small><b>${v}</b></div>`).join('');
    const ql = cq.trim().toLowerCase();
    const named = c => { const o = procOf(c.pid); return o ? o.g.name : c.image || `PID ${c.pid}`; };
    const pass = c => (cFilter === 'all' || (cFilter === 'connected' && c.state === 'Connected') || (cFilter === 'listening' && c.state === 'Listening') || (cFilter === 'public' && c.scope === 'public') || (cFilter === 'udp' && c.proto === 'UDP') || (cFilter === 'local' && c.scope === 'loopback'))
      && (!ql || named(c).toLowerCase().includes(ql) || c.image.toLowerCase().includes(ql) || c.remote.includes(ql) || c.local.includes(ql) || c.host.toLowerCase().includes(ql) || String(c.rport) === ql || String(c.lport) === ql || String(c.pid) === ql || c.service.includes(ql));
    const list = conns.filter(pass).sort(sorter(cSort, (x, k) => (k === 'proc' ? named(x) : k === 'local' ? x.local + ':' + String(x.lport).padStart(5, '0') : k === 'remote' ? x.remote + ':' + String(x.rport).padStart(5, '0') : x[k])));
    main.querySelectorAll('[data-cf]').forEach(b => { const k = b.dataset.cf; const n = k === 'all' ? conns.length : k === 'connected' ? connected.length : k === 'listening' ? listening.length : k === 'public' ? pub.length : k === 'udp' ? conns.filter(c => c.proto === 'UDP').length : conns.filter(c => c.scope === 'loopback').length; b.querySelector('small').textContent = n; });
    main.querySelector('.tm-conn thead').innerHTML = thead(esc, CCOLS, cSort);
    const tbl = main.querySelector('.tm-conn').closest('.tm-tbl'), scroll = tbl.scrollTop;
    main.querySelector('.tm-conn tbody').innerHTML = list.slice(0, 800).map(c => {
      const o = procOf(c.pid), k = keyOf(c);
      return `<tr data-k="${esc(k)}" class="${k === cSel ? 'sel' : ''} sc-${c.scope}"><td class="nm">${o ? icon(o.g.icon, o.g.name) : '<span class="tm-app tm-glyph">?</span>'}<span>${esc(named(c))}</span><small>${c.pid}</small></td><td>${c.proto}</td><td class="mono">${esc(addr(c.local, c.lport))}</td><td class="mono">${esc(addr(c.remote, c.rport))}</td><td class="host" title="${esc(c.host)}">${esc(c.host || (c.scope === 'loopback' ? 'this PC' : c.scope === 'lan' ? 'local network' : ''))}</td><td><span class="cst ${c.state.replace(/\s/g, '').toLowerCase()}">${esc(c.state)}</span></td><td>${esc(c.service)}</td></tr>`;
    }).join('') || `<tr class="tm-wait"><td colspan="7">No connections match.</td></tr>`;
    tbl.scrollTop = scroll;
    drawMap(main, list);
  }
  function drawMap(main, list) {
    const sel = conns.find(c => keyOf(c) === cSel);
    // Remote ends, one node per remote address (busiest first), on rings by how far away they are.
    const ends = new Map();
    for (const c of list) {
      if (!c.remote) continue;
      const k = c.remote; const e = ends.get(k) || { k, scope: c.scope, host: c.host, n: 0, procs: new Set(), sel: false }; e.n++; e.procs.add(c.pid); if (sel && sel.remote === k) e.sel = true; ends.set(k, e);
    }
    const nodes = [...ends.values()].sort((a, b) => b.sel - a.sel || b.n - a.n).slice(0, 42);
    const R = { loopback: 42, lan: 78, public: 112 }, by = { loopback: [], lan: [], public: [] };
    nodes.forEach(n => (by[n.scope] || by.public).push(n));
    let svg = `<circle class="ring" r="42"/><circle class="ring" r="78"/><circle class="ring" r="112"/>`;
    let dots = '';
    for (const [scope, arr] of Object.entries(by)) arr.forEach((n, i) => {
      // A stable angle from the address, nudged apart within its ring.
      let h = 0; for (const ch of n.k) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
      const a = ((h % 360) / 360 + i / Math.max(arr.length, 1) * 0.5) * Math.PI * 2, r = R[scope] + (h % 9) - 4;
      const x = (Math.cos(a) * r).toFixed(1), y = (Math.sin(a) * r).toFixed(1);
      svg += `<line class="ln ${scope}${n.sel ? ' sel' : ''}" x1="0" y1="0" x2="${x}" y2="${y}"/>`;
      dots += `<circle class="nd ${scope}${n.sel ? ' sel' : ''}" cx="${x}" cy="${y}" r="${(2.2 + Math.min(4, Math.sqrt(n.n))).toFixed(1)}"><title>${esc(n.host || n.k)} · ${n.n} connection${n.n === 1 ? '' : 's'}</title></circle>`;
    });
    main.querySelector('.tm-cmap svg').innerHTML = svg + dots + `<circle class="me" r="7"/><text class="me" y="24">This PC</text>`;
    const info = main.querySelector('.tm-cinfo');
    if (!sel) { info.innerHTML = `<h5>${ic('globe')}Connection map</h5><p>Rings out from this PC: programs talking to each other on it, the local network, then the internet. Select a row to see who owns it and where it goes.</p><div class="tm-cleg"><span class="loopback">This PC</span><span class="lan">Local network</span><span class="public">Internet</span></div>`; return; }
    const o = procOf(sel.pid), same = conns.filter(c => c.pid === sel.pid);
    info.innerHTML = `<h5>${o ? icon(o.g.icon, o.g.name) : ''}${esc(o ? o.g.name : sel.image)}</h5>
      <dl><div><dt>Program</dt><dd>${esc(o && o.g.exe ? o.g.exe : sel.image || '—')}</dd></div>
      <div><dt>${sel.remote ? 'Talking to' : 'Open on'}</dt><dd class="mono">${esc(sel.remote ? addr(sel.remote, sel.rport) : addr(sel.local, sel.lport))}</dd></div>
      ${sel.host ? `<div><dt>Remote host</dt><dd>${esc(sel.host)}</dd></div>` : ''}
      <div><dt>Where</dt><dd>${{ loopback: 'Another program on this PC', lan: 'Your local network', public: 'The internet', local: 'Waiting on this PC' }[sel.scope]}</dd></div>
      <div><dt>State</dt><dd>${esc(sel.proto)} · ${esc(sel.state)}${sel.service ? ' · ' + esc(sel.service) : ''}</dd></div>
      <div><dt>Local end</dt><dd class="mono">${esc(addr(sel.local, sel.lport))}</dd></div>
      <div><dt>This program</dt><dd>${same.length} endpoint${same.length === 1 ? '' : 's'} (${same.filter(c => c.state === 'Connected').length} connected, ${same.filter(c => c.state === 'Listening').length} listening)</dd></div></dl>
      <div class="tm-dacts"><button class="cc-btn" type="button" data-tm="conn-proc">${ic('grid')}Show process</button>${sel.remote ? `<button class="cc-btn" type="button" data-tm="conn-copy">${ic('copy')}Copy address</button>` : ''}</div>`;
  }
  const connections = {
    render(main, first) {
      if (first) {
        main.innerHTML = `<div class="tm-pg tm-connpg">${ctx.top('conns', '<small>No per-connection traffic without administrator rights; the system’s traffic is shown instead</small>')}
          <div class="tm-ctiles"></div>
          <div class="tm-cbar"><div class="tm-seg" role="group" aria-label="Show">${[['all', 'All'], ['connected', 'Connected'], ['listening', 'Listening'], ['public', 'Public'], ['udp', 'UDP'], ['local', 'This PC']].map(([k, n]) => `<button type="button" data-cf="${k}" aria-pressed="${k === cFilter}">${n} <small></small></button>`).join('')}</div><span class="grow"></span>
            <label class="tm-search">${ic('search')}<input type="text" class="tm-cq" placeholder="Program, address, host or port" spellcheck="false" autocomplete="off" aria-label="Filter connections"></label></div>
          <div class="tm-cbody"><div class="tm-tbl"><table class="tm-grid tm-conn"><colgroup><col class="nm"><col class="pr"><col class="ad"><col class="ad"><col class="host"><col class="st"><col class="sv"></colgroup><thead></thead><tbody><tr class="tm-wait"><td colspan="7">Reading…</td></tr></tbody></table></div>
            <aside class="tm-cside"><div class="tm-cmap"><svg viewBox="-130 -130 260 260" aria-hidden="true"></svg></div><div class="tm-cinfo"></div></aside></div></div>`;
        const qi = main.querySelector('.tm-cq'); qi.value = cq;
        qi.addEventListener('input', () => { cq = qi.value; drawConns(main); });
        pressRows(main.querySelector('.tm-conn'), k => { cSort = nextSort(cSort, k, ['proc', 'proto', 'local', 'remote', 'host', 'state', 'service']); store.set('tmConnSort', cSort); drawConns(main); }, k => { cSel = k; drawConns(main); });
        loadConns(); clearInterval(cT); cT = setInterval(loadConns, 2000);
      }
    },
    click(e, b) {
      const main = mainEl();
      const f = e.target.closest('[data-cf]'); if (f) { cFilter = f.dataset.cf; main.querySelectorAll('[data-cf]').forEach(x => x.setAttribute('aria-pressed', x === f)); drawConns(main); return; }
      if (!b) return;
      const sel = conns && conns.find(c => keyOf(c) === cSel); if (!sel) return;
      if (b.dataset.tm === 'conn-proc') ctx.showProcess(sel.pid);
      else if (b.dataset.tm === 'conn-copy') navigator.clipboard.writeText(addr(sel.remote, sel.rport)).then(() => flash('Address copied')).catch(() => {});
    },
    leave() { clearInterval(cT); },
    closed() { conns = null; cSel = null; }
  };

  /* ---------- Installed Apps ---------- */
  let apps = null, aSort = store.get('tmAppSort', { k: 'name', d: 1 }), aq = '', aScope = 'all', aSel = null, aConfirm = false, aT = 0;
  const ACOLS = [['name', 'Application'], ['publisher', 'Publisher'], ['version', 'Version'], ['installed', 'Installed', 'n'], ['size', 'Size', 'n'], ['scope', 'Scope']];
  async function loadApps() {
    try { apps = await invoke('tm_installed'); } catch (e) { flash(String(e)); apps = apps || []; }
    const m = mainEl(); if (ctx.open && m && m.querySelector('.tm-apps')) drawApps(m);
  }
  function drawApps(main) {
    if (!apps) return;
    const ql = aq.trim().toLowerCase();
    const list = apps.filter(a => (aScope === 'all' || (aScope === 'store' ? a.scope === 'Microsoft Store' : aScope === 'user' ? a.scope === 'Current user' : a.scope.startsWith('Machine'))) && (!ql || a.name.toLowerCase().includes(ql) || a.publisher.toLowerCase().includes(ql))).sort(sorter(aSort));
    const total = apps.reduce((s, a) => s + a.size, 0);
    main.querySelector('.tm-appn').textContent = `${apps.length} apps · ${fmtSize(total)}${apps.some(a => !a.size) ? ' measured so far' : ''}`;
    main.querySelector('.tm-apps thead').innerHTML = thead(esc, ACOLS, aSort);
    const maxSize = Math.max(1, ...list.map(a => a.size));
    const tbl = main.querySelector('.tm-apps').closest('.tm-tbl'), scroll = tbl.scrollTop;
    main.querySelector('.tm-apps tbody').innerHTML = list.map(a => `<tr data-k="${esc(a.id)}" class="${a.id === aSel ? 'sel' : ''}"><td class="nm">${icon(a.icon, a.name)}<span>${esc(a.name)}</span></td><td>${esc(a.publisher)}</td><td class="mono">${esc(a.version)}</td><td>${a.installed ? fmtDate(a.installed * 1000) : '—'}</td><td class="n" style="${heat(a.size, maxSize)}">${fmtSize(a.size)}</td><td>${esc(a.scope)}</td></tr>`).join('') || `<tr class="tm-wait"><td colspan="6">No apps match.</td></tr>`;
    tbl.scrollTop = scroll;
    drawAppFoot(main);
  }
  function drawAppFoot(main) {
    const a = apps && apps.find(x => x.id === aSel), foot = main.querySelector('.tm-appfoot');
    if (!a) { foot.className = 'tm-foot tm-appfoot'; foot.innerHTML = '<span class="grow tm-none">Select an app to uninstall it, change it or open where it’s installed. Sizes the installer didn’t record are measured in the background.</span>'; return; }
    foot.className = 'tm-foot tm-appfoot tm-detail on';
    if (aConfirm) { foot.innerHTML = `<span class="grow tm-confirm">${a.scope === 'Microsoft Store' ? `Open ${esc(a.name)}’s page in Settings to uninstall it?` : `Start ${esc(a.name)}’s uninstaller? It may ask for administrator rights and then removes the app.`}</span><button class="cc-btn" type="button" data-tm="app-cancel">Cancel</button><button class="cc-btn danger-fill" type="button" data-tm="app-uninstall-yes">${a.scope === 'Microsoft Store' ? 'Open Settings' : 'Uninstall'}</button>`; return; }
    foot.innerHTML = `<div class="tm-dhead">${icon(a.icon, a.name)}<span><b>${esc(a.name)}</b><span class="tm-path" title="${esc(a.location)}">${esc(a.location || 'The installer didn’t record where it went')}</span></span></div>
      <div class="tm-dfacts"><div><small>Publisher</small><b>${esc(a.publisher || '—')}</b></div><div><small>Version</small><b>${esc(a.version || '—')}</b></div><div><small>Installed</small><b>${a.installed ? fmtDate(a.installed * 1000) : '—'}</b></div><div><small>Size</small><b>${fmtSize(a.size)}</b></div><div><small>Scope</small><b>${esc(a.scope)}</b></div></div>
      <div class="tm-dacts"><button class="cc-btn" type="button" data-tm="app-location"${a.location ? '' : ' disabled'}>${ic('folder')}Open location</button>${a.help ? `<button class="cc-btn" type="button" data-tm="app-help">${ic('ext')}Web page</button>` : ''}${a.can_modify ? `<button class="cc-btn" type="button" data-tm="app-modify">${ic('wrench')}Modify</button>` : ''}<button class="cc-btn danger" type="button" data-tm="app-uninstall"${a.can_uninstall ? '' : ' disabled title="It can’t be uninstalled from here"'}>${ic('trash')}Uninstall</button></div>`;
  }
  const installed = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg tm-apppg">${ctx.top('installed', `<small class="tm-appn"></small><span class="grow"></span>
          <div class="tm-seg" role="group" aria-label="Show">${[['all', 'All'], ['machine', 'Programs'], ['user', 'Just you'], ['store', 'Store apps']].map(([k, n]) => `<button type="button" data-as="${k}" aria-pressed="${k === aScope}">${n}</button>`).join('')}</div>
          <button class="cc-btn" type="button" data-tm="app-refresh">${ic('restart')}Refresh</button>
          <label class="tm-search">${ic('search')}<input type="text" class="tm-aq" placeholder="Find an app or publisher" spellcheck="false" autocomplete="off" aria-label="Find an app"></label>`)}
        <div class="tm-tbl"><table class="tm-grid tm-apps"><colgroup><col class="nm"><col class="pub"><col class="ver"><col class="dt"><col class="sz"><col class="sc"></colgroup><thead></thead><tbody><tr class="tm-wait"><td colspan="6">Reading…</td></tr></tbody></table></div><div class="tm-foot tm-appfoot"></div></div>`;
      const qi = main.querySelector('.tm-aq'); qi.value = aq;
      qi.addEventListener('input', () => { aq = qi.value; drawApps(main); });
      pressRows(main.querySelector('.tm-apps'), k => { aSort = nextSort(aSort, k, ['name', 'publisher', 'version', 'scope']); store.set('tmAppSort', aSort); drawApps(main); }, k => { aSel = k; aConfirm = false; drawApps(main); });
      aConfirm = false;
      loadApps();
      // Sizes measured in the background show up on a later read.
      clearTimeout(aT); aT = setTimeout(() => { if (ctx.open && mainEl() && mainEl().querySelector('.tm-apps')) loadApps(); }, 12000);
    },
    click(e, b) {
      const main = mainEl();
      const s = e.target.closest('[data-as]'); if (s) { aScope = s.dataset.as; main.querySelectorAll('[data-as]').forEach(x => x.setAttribute('aria-pressed', x === s)); drawApps(main); return; }
      if (!b) return;
      const k = b.dataset.tm, a = apps && apps.find(x => x.id === aSel);
      if (k === 'app-refresh') { loadApps(); return; }
      if (!a) return;
      const run = what => invoke('tm_installed_action', { id: a.id, what }).catch(err => flash(String(err)));
      if (k === 'app-uninstall') { aConfirm = true; drawAppFoot(main); }
      else if (k === 'app-cancel') { aConfirm = false; drawAppFoot(main); }
      else if (k === 'app-uninstall-yes') { aConfirm = false; drawAppFoot(main); run('uninstall').then(() => flash(a.scope === 'Microsoft Store' ? 'Opened in Settings' : `Started ${a.name}’s uninstaller`)); setTimeout(loadApps, 15000); }
      else if (k === 'app-modify') run('modify');
      else if (k === 'app-location') run('location');
      else if (k === 'app-help') run('help');
    },
    key(e) {
      if (e.key === 'Escape' && aConfirm) { aConfirm = false; const m = mainEl(); if (m) drawAppFoot(m); return 'used'; }
      if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && apps) {
        const rows = [...document.querySelectorAll('.tm-apps tbody tr[data-k]')]; if (!rows.length) return 'used';
        let i = rows.findIndex(r => r.dataset.k === aSel); i = clamp(i + (e.key === 'ArrowDown' ? 1 : -1), 0, rows.length - 1);
        aSel = rows[i].dataset.k; aConfirm = false; const m = mainEl(); drawApps(m); const r = m.querySelector('.tm-apps tr.sel'); if (r) r.scrollIntoView({ block: 'nearest' });
        return 'used';
      }
      return false;
    },
    leave() { clearTimeout(aT); },
    closed() { apps = null; aSel = null; }
  };

  return { services, sensors, conns: connections, installed };
}
