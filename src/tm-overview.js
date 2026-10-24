// Task Manager pages: Summary (the whole machine at a glance), System Info (what this PC is), App
// history (what each program has used since the island started counting) and Users (who's signed
// in and what their session is using).

import { GB, MB, clamp, fmtMem, fmtRate, fmtBits, fmtGHz, fmtMs, fmtDate, fmtLink, fmtSize, ago, heat, graph, leds, thead, sorter } from './tm-util.js';

export function createOverview(ctx) {
  const { esc, ic, invoke, flash, assetUrl, store } = ctx;
  const icon = (src, name, cls = 'tm-app') => (src ? `<img class="${cls}" src="${esc(assetUrl(src))}" alt="" draggable="false">` : `<span class="${cls} tm-glyph">${esc((name || '?').trim().charAt(0).toUpperCase())}</span>`);

  /* ---------- Summary ---------- */
  let maxMhz = 0;
  const summary = {
    render(main, first) {
      const S = ctx.snap, st = ctx.stats || {}, sen = st.sensors || {}, h = ctx.hist, C = ctx.cpu || {};
      if (first) {
        main.innerHTML = `<div class="tm-pg tm-sum">${ctx.top('summary', '<span class="grow"></span><small class="tm-sum-cpu"></small>')}
          <div class="tm-sgrid">
            <section class="tm-card tm-meters"></section>
            <section class="tm-card tm-ov"><header><h5>CPU overview</h5><b class="big"></b></header><div class="tm-ovleg"><span class="cpu">Utilization</span><span class="temp">Temperature</span><span class="kern">Kernel</span></div><div class="tm-ovg"><div class="ax l"><span>100%</span><span>50%</span><span>0%</span></div><div class="g"></div><div class="ax r"><span>110°</span><span>55°</span><span>0°</span></div></div><footer><small class="lp"></small><small class="sp"></small></footer></section>
            <section class="tm-card tm-top5"><header><h5>Top processes</h5><small></small></header><div class="tm-tl"><table><thead><tr><th>PID</th><th>Name</th><th>CPU</th><th>GPU</th><th>Memory</th></tr></thead><tbody></tbody></table></div></section>
            <section class="tm-card tm-memsum"><div class="mled"></div><div class="mg"><header><h5>Memory utilization</h5><small></small></header><div class="g"></div><footer></footer></div></section>
            <section class="tm-tiles"></section>
          </div></div>`;
        const tl = main.querySelector('.tm-tl');
        tl.addEventListener('pointerdown', e => { const r = e.button === 0 && e.target.closest('tr[data-pid]'); if (r) ctx.showProcess(+r.dataset.pid); });
      }
      if (!S) return;
      const P = S.perf;
      const mhz = P.mhz || sen.cpu_mhz || 0;
      maxMhz = Math.max(maxMhz, mhz, (C.base_mhz || 0) * 1.2);
      const temp = sen.cpu_temp;
      const meter = (cls, label, frac, value, tip) => `<div class="tm-meter ${cls}" title="${esc(tip || '')}"><small>${label}</small>${leds(frac, 28)}<b>${value}</b></div>`;
      main.querySelector('.tm-meters').innerHTML = meter('cpu', 'CPU', P.cpu / 100, P.cpu.toFixed(1) + '%')
        + meter('clock', 'Clock', mhz / (maxMhz || 1), mhz ? (mhz / 1000).toFixed(1) + 'G' : '—', 'Effective clock')
        + meter('temp', 'Temp', temp != null ? temp / 100 : 0, temp != null ? Math.round(temp) + '°' : '—', temp != null ? 'CPU chip, measured by its built-in graphics' : 'No CPU temperature on this PC')
        + meter('gpu', 'GPU', P.gpu / 100, P.gpu.toFixed(1) + '%');
      main.querySelector('.tm-ov .big').textContent = P.cpu.toFixed(1) + '%';
      main.querySelector('.tm-ov .g').innerHTML = graph([{ h: h.cpu, cls: 'cpu' }, { h: h.temp, max: 110, cls: 'temp', fill: false }, { h: h.kernel, cls: 'kern', fill: false }], 100, { H: 260 });
      main.querySelector('.tm-ov .lp').textContent = `${P.cores.length || C.logical || ''} logical processors`;
      main.querySelector('.tm-ov .sp').textContent = mhz ? `Speed ${fmtGHz(mhz)}` : '';
      main.querySelector('.tm-sum-cpu').textContent = C.name || st.cpu_name || '';
      // Every process, busiest first.
      const procs = [];
      for (const g of S.groups) for (const p of g.procs) procs.push([p, g]);
      procs.sort((a, b) => b[0].cpu - a[0].cpu || b[0].mem - a[0].mem);
      main.querySelector('.tm-top5 header small').textContent = `${procs.length} total`;
      main.querySelector('.tm-tl tbody').innerHTML = procs.slice(0, 60).map(([p, g]) => `<tr data-pid="${p.pid}" title="${esc(g.name)}"><td class="pid">${p.pid}</td><td class="nm">${icon(g.icon, g.name)}<span>${esc(p.image)}</span></td><td style="${heat(p.cpu, 20)}">${p.cpu.toFixed(1)}%</td><td>${p.gpu >= .1 ? p.gpu.toFixed(1) + '%' : '—'}</td><td style="${heat(p.mem, P.mem_total * .08)}">${fmtMem(p.mem)}</td></tr>`).join('');
      // Memory.
      const used = P.mem_used / (P.mem_total || 1);
      main.querySelector('.tm-memsum .mled').innerHTML = `<small>Memory</small>${leds(used, 18)}<b>${(used * 100).toFixed(1)}%</b>`;
      main.querySelector('.tm-memsum header small').textContent = `${(P.mem_used / GB).toFixed(1)} GB / ${(P.mem_total / GB).toFixed(1)} GB`;
      main.querySelector('.tm-memsum .g').innerHTML = graph([{ h: h.mem, cls: 'mem' }], 100, { H: 110 });
      main.querySelector('.tm-memsum footer').innerHTML = `<small>Available ${((P.mem_total - P.mem_used) / GB).toFixed(1)} GB</small><small>Cached ${(P.cached / GB).toFixed(1)} GB</small><small>Committed ${(P.committed / GB).toFixed(1)} / ${(P.commit_limit / GB).toFixed(1)} GB</small>`;
      // Tiles.
      const dr = P.disks.reduce((a, d) => a + d.read, 0), dw = P.disks.reduce((a, d) => a + d.write, 0), act = P.disks.length ? Math.max(...P.disks.map(d => d.active)) : 0;
      const rx = P.nets.reduce((a, n) => a + n.rx, 0), tx = P.nets.reduce((a, n) => a + n.tx, 0), link = Math.max(...P.nets.map(n => n.bandwidth), 1e8);
      const g = (sen.gpus || []).find(x => !x.integrated) || (sen.gpus || [])[0];
      const temps = [temp != null && ['CPU', temp], ...(sen.gpus || []).filter(x => x.temp != null && !x.integrated).map(x => ['GPU', x.temp]), ...(sen.drives || []).map(d => [d.letters && d.letters[0] ? 'Drive ' + d.letters[0] : 'Drive', d.temp])].filter(Boolean);
      const hot = temps.sort((a, b) => b[1] - a[1])[0];
      const tile = (cls, title, val, sub, frac, tip) => `<div class="tm-tile ${cls}" title="${esc(tip || '')}"><header><h6>${title}</h6><b>${val}</b></header><small>${sub}</small>${leds(frac, 30, { horizontal: true })}</div>`;
      main.querySelector('.tm-tiles').innerHTML = tile('disk', 'Disks', act.toFixed(0) + '%', `R: ${fmtRate(dr)}  W: ${fmtRate(dw)}`, act / 100, 'Busiest disk’s active time')
        + tile('net', 'Network', fmtBits(rx + tx), `R: ${fmtBits(rx)}  S: ${fmtBits(tx)}`, clamp(Math.log10(1 + (rx + tx) * 8) / Math.log10(1 + link), 0, 1), 'All adapters (the strip is on a log scale up to the link speed)')
        + (g && g.power_pct != null ? tile('pow', 'Graphics power', Math.round(g.power_pct) + '%', `of the card’s limit${g.fan_rpm != null ? ' · fan ' + (g.fan_rpm ? g.fan_rpm.toLocaleString() + ' rpm' : 'stopped') : ''}`, g.power_pct / 100, 'CPU power needs a hardware driver, so the graphics card’s is shown') : tile('pow off', 'Power', '—', 'Needs a hardware driver', 0))
        + (hot ? tile('therm', 'Thermals', Math.round(hot[1]) + '°C', `Hottest: ${hot[0]} · ${temps.length} sensors`, hot[1] / 100) : tile('therm off', 'Thermals', '—', 'No sensors Windows shares', 0));
    }
  };

  /* ---------- System Info ---------- */
  let sysText = '';
  const system = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg">${ctx.top('system', '<span class="grow"></span><button class="cc-btn" type="button" data-tm="sys-copy">' + ic('copy') + 'Copy as text</button>')}<div class="tm-sys"><p class="tm-empty">Reading…</p></div></div>`;
      ctx.system().then(s => { if (ctx.open && main.querySelector('.tm-sys')) draw(main, s); }).catch(e => { main.querySelector('.tm-sys').innerHTML = `<p class="tm-empty">${esc(String(e))}</p>`; });
    },
    click(e, b) {
      if (b && b.dataset.tm === 'sys-copy' && sysText) navigator.clipboard.writeText(sysText).then(() => flash('System info copied')).catch(() => {});
    }
  };
  function draw(main, s) {
    const C = s.cpu || {};
    const day = secs => (secs ? fmtDate(secs * 1000) : '—');
    const cards = [
      ['Windows', 'monitor', [['Edition', s.os_name], ['Version', s.os_version], ['OS build', s.os_build], ['Installed', day(s.installed)], ['Computer name', s.computer], ['Signed in as', s.user], ['Up since', fmtDate(s.booted * 1000, true)], ['Time zone', s.time_zone]]],
      ['Computer', 'cpu', [['Maker', s.maker || '—'], s.model && ['Model', s.model], ['Motherboard', s.board || '—'], ['BIOS', s.bios ? `${s.bios}${s.bios_date ? ' (' + s.bios_date + ')' : ''}` : '—'], ['Firmware', s.firmware || '—'], ['Secure Boot', s.secure_boot == null ? 'Not supported' : s.secure_boot ? 'On' : 'Off'], ['TPM', s.tpm ? 'TPM ' + s.tpm : 'None found']]],
      ['Processor', 'cpu', [['Name', C.name], ['Cores', `${C.cores} cores, ${C.logical} threads${C.sockets > 1 ? `, ${C.sockets} sockets` : ''}`], ['Base speed', C.base_mhz ? fmtGHz(C.base_mhz) : '—'], ['Cache', `L1 ${fmtMem(C.l1)} · L2 ${fmtMem(C.l2)} · L3 ${fmtMem(C.l3)}`], ['Virtualization', C.virtualization ? 'Enabled' : 'Off'], ['Power plan', C.power_plan || '—']]],
      ['Memory', 'mem', [['Installed', fmtMem(s.mem_installed)], ['Usable', fmtMem(s.mem_usable)], s.mem_slots && ['Slots', `${s.modules.length} of ${s.mem_slots} used`], ...s.modules.map(m => [m.slot, `${fmtMem(m.size_mb * MB)} ${m.kind} · ${m.configured || m.speed} MT/s${m.speed && m.configured && m.speed !== m.configured ? ` (rated ${m.speed})` : ''}`, [m.maker, m.part].filter(Boolean).join(' ')])]],
      ['Graphics', 'monitor', s.gpus.flatMap(g => [[g.name, `${fmtMem(g.vram)} dedicated`, `Driver ${g.driver}`]])],
      ['Displays', 'monitor', s.displays.map(d => [d.name, `${d.width} × ${d.height} at ${Math.round(d.hz)} Hz`, d.gpu])],
      ['Storage', 'disk', [...s.disks.map(d => [`Disk ${d.number}${d.letters.length ? ' (' + d.letters.join(' ') + ')' : ''}`, `${d.model} · ${fmtSize(d.size)}`, [d.bus, d.ssd === true ? 'SSD' : d.ssd === false ? 'HDD' : ''].filter(Boolean).join(' ')])]],
      ['Network', 'net', s.adapters.map(a => [`${a.name}`, [a.ipv4[0], a.speed ? fmtLink(a.speed) : ''].filter(Boolean).join(' · '), [a.kind, a.description, a.mac && 'MAC ' + a.mac, a.gateway[0] && 'gateway ' + a.gateway[0], a.dns.length && 'DNS ' + a.dns.join(', ')].filter(Boolean).join(' · ')])]
    ];
    const vol = v => { const used = v.total - v.free, p = v.total ? used / v.total : 0; return `<div class="tm-vol"><div><b>${esc(v.letter)}</b> <span>${esc(v.label || v.kind)}</span><small>${fmtSize(v.free)} free of ${fmtSize(v.total)} · ${esc(v.fs)}</small></div><span class="bar"><i style="width:${(p * 100).toFixed(1)}%" class="${p > .9 ? 'hot' : ''}"></i></span></div>`; };
    main.querySelector('.tm-sys').innerHTML = cards.map(([t, icn, rows]) => `<section class="tm-card tm-syscard"><header><h5>${ic(icn)}${t}</h5></header><dl>${rows.filter(Boolean).map(([k, v, sub]) => `<div><dt>${esc(String(k))}</dt><dd>${esc(String(v ?? '—'))}${sub ? `<small>${esc(String(sub))}</small>` : ''}</dd></div>`).join('') || '<div><dd class="faint">None found</dd></div>'}</dl>${t === 'Storage' ? s.volumes.map(vol).join('') : ''}</section>`).join('');
    sysText = cards.map(([t, , rows]) => `${t}\n` + rows.filter(Boolean).map(([k, v, sub]) => `  ${k}: ${v ?? '—'}${sub ? ` (${sub})` : ''}`).join('\n')).join('\n\n') + '\n\nVolumes\n' + s.volumes.map(v => `  ${v.letter} ${v.label}: ${fmtSize(v.free)} free of ${fmtSize(v.total)} (${v.fs})`).join('\n');
  }

  /* ---------- App history ---------- */
  let hist = null, hSort = store.get('tmHSort', { k: 'cpu_ms', d: -1 }), hAll = false, hq = '', hT = 0, hConfirm = false;
  const HCOLS = [['name', 'Name'], ['cpu_ms', 'CPU time', 'n'], ['read', 'Disk read', 'n'], ['write', 'Disk write', 'n'], ['launches', 'Launches', 'n'], ['last', 'Last used', 'n']];
  async function loadHistory(main) {
    try { hist = await invoke('tm_history'); } catch (e) { flash(String(e)); }
    if (ctx.open && main.querySelector('.tm-hist')) drawHistory(main);
    clearTimeout(hT); hT = setTimeout(() => { if (ctx.open && main.querySelector('.tm-hist')) loadHistory(main); }, 30000);
  }
  function drawHistory(main) {
    if (!hist) return;
    const ql = hq.trim().toLowerCase();
    const list = hist.apps.filter(a => (hAll || !a.system) && (!ql || a.name.toLowerCase().includes(ql) || a.exe.toLowerCase().includes(ql))).sort(sorter(hSort));
    const maxCpu = Math.max(1, ...list.map(a => a.cpu_ms));
    main.querySelector('.tm-hsince').textContent = `Counted by the island since ${fmtDate(hist.since)} · ${list.length} programs`;
    main.querySelector('.tm-hist thead').innerHTML = thead(esc, HCOLS, hSort);
    main.querySelector('.tm-hist tbody').innerHTML = list.map(a => `<tr title="${esc(a.exe.startsWith('?') ? a.name : a.exe)}"><td class="nm">${icon(a.icon, a.name)}<span>${esc(a.name)}</span></td><td style="${heat(a.cpu_ms, maxCpu)}">${fmtMs(a.cpu_ms)}</td><td>${fmtSize(a.read)}</td><td>${fmtSize(a.write)}</td><td>${a.launches.toLocaleString()}</td><td>${ago(a.last)}</td></tr>`).join('') || `<tr class="tm-wait"><td colspan="6">${hist.apps.length ? 'Nothing matches.' : 'Nothing counted yet: it fills in as you use the PC.'}</td></tr>`;
    main.querySelector('.tm-hfoot').innerHTML = hConfirm
      ? `<span class="grow tm-confirm">Clear the app history? Counting starts again from now.</span><button class="cc-btn" type="button" data-tm="h-cancel">Cancel</button><button class="cc-btn danger-fill" type="button" data-tm="h-clear-yes">Clear</button>`
      : `<span class="grow">Windows’ own app history needs administrator rights to read, so the island keeps this one: CPU time, reads and writes and launches, checked every 30 seconds while it runs.</span><button class="cc-btn" type="button" data-tm="h-clear">${ic('trash')}Clear history</button>`;
  }
  const history = {
    render(main, first) {
      if (!first) return;
      main.innerHTML = `<div class="tm-pg tm-histpg">${ctx.top('history', `<small class="tm-hsince"></small><span class="grow"></span>
          <div class="tm-seg" role="group" aria-label="Show"><button type="button" data-hall="0" aria-pressed="${!hAll}">Apps</button><button type="button" data-hall="1" aria-pressed="${hAll}">Including Windows</button></div>
          <label class="tm-search">${ic('search')}<input type="text" class="tm-hq" placeholder="Find a program" spellcheck="false" autocomplete="off" aria-label="Find a program"></label>`)}
        <div class="tm-tbl"><table class="tm-grid tm-hist"><thead></thead><tbody><tr class="tm-wait"><td colspan="6">Reading…</td></tr></tbody></table></div><div class="tm-foot tm-hfoot"></div></div>`;
      const qi = main.querySelector('.tm-hq'); qi.value = hq;
      qi.addEventListener('input', () => { hq = qi.value; drawHistory(main); });
      main.querySelector('.tm-hist').addEventListener('pointerdown', e => { const th = e.target.closest('th[data-sort]'); if (th) { const k = th.dataset.sort; hSort = hSort.k === k ? { k, d: -hSort.d } : { k, d: k === 'name' ? 1 : -1 }; store.set('tmHSort', hSort); drawHistory(main); } });
      hConfirm = false;
      loadHistory(main);
    },
    click(e, b) {
      const main = e.target.closest('.tm-main');
      const all = e.target.closest('[data-hall]');
      if (all) { hAll = all.dataset.hall === '1'; main.querySelectorAll('[data-hall]').forEach(x => x.setAttribute('aria-pressed', x === all)); drawHistory(main); return; }
      if (!b) return;
      if (b.dataset.tm === 'h-clear') { hConfirm = true; drawHistory(main); }
      else if (b.dataset.tm === 'h-cancel') { hConfirm = false; drawHistory(main); }
      else if (b.dataset.tm === 'h-clear-yes') { hConfirm = false; invoke('tm_history_clear').then(() => loadHistory(main)).catch(err => flash(String(err))); }
    },
    leave() { clearTimeout(hT); },
    key(e) { if (e.key === 'Escape' && hConfirm) { hConfirm = false; const m = document.querySelector('.tmw .tm-main'); if (m) drawHistory(m); return 'used'; } return false; }
  };

  /* ---------- Users ---------- */
  let sessions = null, sT = 0;
  async function loadSessions() { try { sessions = await invoke('tm_sessions'); } catch (e) { sessions = []; } }
  const users = {
    render(main, first) {
      if (first) {
        main.innerHTML = `<div class="tm-pg">${ctx.top('users', '<small class="tm-un"></small>')}<div class="tm-users"><p class="tm-empty">Reading…</p></div></div>`;
        loadSessions().then(() => users.render(main, false));
        clearInterval(sT); sT = setInterval(loadSessions, 15000);
      }
      const S = ctx.snap; if (!S || !sessions) return;
      const by = new Map();
      for (const g of S.groups) for (const p of g.procs) {
        const k = p.session; if (!by.has(k)) by.set(k, { cpu: 0, mem: 0, n: 0, io: 0, apps: new Map() });
        const u = by.get(k); u.cpu += p.cpu; u.mem += p.mem; u.n++; u.io += p.read + p.write;
        const a = u.apps.get(g.id) || { g, cpu: 0, mem: 0, n: 0 }; a.cpu += p.cpu; a.mem += p.mem; a.n++; u.apps.set(g.id, a);
      }
      const P = S.perf;
      const card = (title, sub, state, u, own, initial) => {
        const apps = [...u.apps.values()].sort((a, b) => b.cpu - a.cpu || b.mem - a.mem).slice(0, 8);
        return `<section class="tm-card tm-user${own ? ' own' : ''}"><header><span class="av">${esc(initial)}</span><span class="who"><b>${esc(title)}</b><small>${esc(sub)}</small></span>${state ? `<span class="chip ${state.toLowerCase()}">${esc(state)}</span>` : ''}</header>
          <div class="tm-ustats"><div><small>CPU</small><b>${Math.min(100, u.cpu).toFixed(1)}%</b>${leds(u.cpu / 100, 24, { horizontal: true, cls: 'cpu' })}</div><div><small>Memory</small><b>${fmtMem(u.mem)}</b>${leds(u.mem / (P.mem_total || 1), 24, { horizontal: true, cls: 'mem' })}</div><div><small>Processes</small><b>${u.n}</b></div><div><small>Disk I/O</small><b>${fmtRate(u.io)}</b></div></div>
          <table class="tm-uapps"><tbody>${apps.map(a => `<tr><td class="nm">${icon(a.g.icon, a.g.name)}<span>${esc(a.g.name)}</span>${a.n > 1 ? `<small>(${a.n})</small>` : ''}</td><td>${a.cpu.toFixed(1)}%</td><td>${fmtMem(a.mem)}</td></tr>`).join('')}</tbody></table></section>`;
      };
      let html = sessions.map(s => card(s.domain && !s.own ? `${s.domain}\\${s.user}` : s.user, `Signed in ${s.logon ? fmtDate(s.logon, true) + ' (' + ago(s.logon) + ')' : ''} · session ${s.id}${s.own ? ' · you' : ''}`, s.state, by.get(s.id) || { cpu: 0, mem: 0, n: 0, io: 0, apps: new Map() }, s.own, s.user.charAt(0).toUpperCase())).join('');
      const sys = by.get(0);
      if (sys) html += card('Windows services', 'Session 0: services and drivers’ helpers, shared by everyone', '', sys, false, 'W');
      main.querySelector('.tm-un').textContent = `${sessions.length} signed in`;
      main.querySelector('.tm-users').innerHTML = html;
    },
    leave() { clearInterval(sT); }
  };

  return { summary, system, history, users };
}
