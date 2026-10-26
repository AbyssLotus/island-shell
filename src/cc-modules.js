// The Command Center's Modules sheet: the parts of Island Shell you install or leave out (Music,
// Discord, Canvas, Destiny 2…). Installed ones show how they're doing (running, stopped, crashed),
// what their process uses, what they talk to, their settings page and their recent log, with Turn
// off, Restart, Update and Remove (keeping or forgetting their keys and data). Available ones come
// from the signed release catalog, ready to install; Updates lists newer versions. Developer mode
// loads an unpacked module folder (unsigned, and badged so). Backend: modules.rs and updater.rs.

import { modUrl, shadowFor } from './modhost.js';

const VIEWS = [['installed', 'Installed'], ['available', 'Available'], ['updates', 'Updates']];
const STATE = { running: 'Running', starting: 'Starting…', stopped: 'Not running', backoff: 'Restarting after a crash', failed: 'Stopped after crashing', disabled: 'Turned off' };
const RUNS = { always: 'all the time', 'while-process': 'while its app is open', scheduled: 'now and then, on a schedule', 'on-demand': 'when you use it' };
const size = b => (b >= 1048576 ? (b / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1024)) + ' KB');

export function createModules({ invoke, listen, esc, ic, flash, settingsOf }) {
  let el = null, view = 'installed', sel = '', list = [], broken = [], catalog = [], upd = {}, usage = {}, logs = '', logsFor = '', timer = 0, offs = [], confirmRemove = false, settingsFor = '';
  const $ = s => el && el.querySelector(s);
  const installed = id => list.find(m => m.id === id);
  const available = () => catalog.filter(c => !installed(c.id));
  const updates = () => (upd && upd.modules) || [];
  const updateOf = id => updates().find(u => u.id === id);

  function build() {
    el.innerHTML = `<section class="mds-main">
        <div class="sh-bar"><h3>Modules <small></small></h3>
          <div class="sh-chips" role="group" aria-label="Show">${VIEWS.map(([k, n]) => `<button type="button" data-v="${k}">${n}</button>`).join('')}</div>
          <span class="grow"></span>
          <button class="cc-btn" type="button" data-a="updateall" hidden>${ic('download')}Update all</button>
          <button class="cc-btn" type="button" data-a="check">${ic('restart')}Check for updates</button>
          <button class="cc-btn" type="button" data-a="folder" title="Developer mode: load an unpacked module (unsigned)">${ic('folder')}Load a folder…</button>
        </div>
        <div class="mds-list" role="listbox" aria-label="Modules"></div>
      </section>
      <aside class="mds-side" aria-live="polite"></aside>`;
  }

  /* ---------- reading ---------- */
  async function load() {
    try { const l = await invoke('mod_list'); list = (l && l.modules) || []; broken = (l && l.broken) || []; } catch (e) {}
    try { upd = (await invoke('update_status')) || {}; } catch (e) {}
    try { catalog = (await invoke('mod_catalog')) || []; } catch (e) {}
    render();
  }
  async function poll() {
    clearTimeout(timer);
    if (!el) return;
    try { usage = Object.fromEntries(((await invoke('mod_usage')) || []).map(u => [u.id, u])); } catch (e) {}
    if (sel && installed(sel) && logsFor === sel) { try { logs = ((await invoke('mod_logs', { id: sel })) || []).slice(-40).join('\n'); } catch (e) {} }
    if (!el) return;
    // The rows' memory, in place (redrawing the list would lose the pointer's place).
    if (view === 'installed') for (const m of list) { const s = el.querySelector(`.mds-row[data-id="${CSS.escape(m.id)}"] small`); if (s) s.textContent = stateLine(m); }
    renderSide(false);
    timer = setTimeout(poll, 2000);
  }

  /* ---------- drawing ---------- */
  // A module's own icon is drawn in the theme's color (its SVG is a mask; stroke colors don't reach an <img>).
  const iconOf = (id, man) => (man && /\.svg$/.test(man.icon || '') ? `<span class="mds-svg" style="--src:url('${esc(modUrl(id, man.icon, man.version))}')"></span>` : ic((man && man.icon) || 'layers'));
  function stateLine(m) {
    const u = usage[m.id], st = STATE[m.state] || m.state;
    return [`Version ${m.manifest.version}`, st, u && u.mem_mb != null ? `${u.mem_mb} MB` : ''].filter(Boolean).join(' · ');
  }
  function rows() {
    if (view === 'installed') {
      const items = list.map(m => ({ id: m.id, name: m.manifest.name, man: m.manifest, sub: stateLine(m), tone: m.state, dev: m.dev }));
      for (const [id, err] of broken) items.push({ id, name: id, man: null, sub: `Couldn't load: ${err}`, tone: 'failed' });
      return items;
    }
    if (view === 'available') return available().map(c => ({ id: c.id, name: c.name, man: null, sub: `Version ${c.version} · ${size(c.size)}`, tone: '' }));
    return updates().map(u => ({ id: u.id, name: u.name, man: installed(u.id) && installed(u.id).manifest, sub: `${u.from} → ${u.to}`, tone: 'update' }));
  }
  function render() {
    if (!el) return;
    el.querySelectorAll('.sh-chips [data-v]').forEach(b => {
      b.setAttribute('aria-pressed', b.dataset.v === view);
      if (b.dataset.v === 'updates') b.innerHTML = `Updates${updates().length ? ` <b class="mds-n">${updates().length}</b>` : ''}`;
    });
    $('.sh-bar h3 small').textContent = `${list.length} installed`;
    $('[data-a="updateall"]').hidden = !(view === 'updates' && updates().length);
    const r = rows();
    if (!r.some(x => x.id === sel)) sel = r.length ? r[0].id : '';
    let empty = '';
    if (!r.length) {
      if (view === 'installed') empty = 'Nothing installed yet. See what’s available, or load a module folder in developer mode.';
      else if (view === 'updates') empty = upd.checked ? 'Everything is up to date.' : 'Check for updates to see newer versions.';
      else if (upd.state === 'off') empty = 'This build of Island Shell can’t install modules (it has no release key). Load a module folder in developer mode instead.';
      else empty = catalog.length ? 'Every module is installed.' : 'Check for updates to see what’s available.';
    }
    $('.mds-list').innerHTML = r.length ? r.map(x => `<button type="button" class="mds-row${x.id === sel ? ' sel' : ''}${x.tone ? ' t-' + esc(x.tone) : ''}" data-id="${esc(x.id)}" role="option" aria-selected="${x.id === sel}">
        <span class="mds-ic">${iconOf(x.id, x.man)}</span>
        <span class="mds-t"><b>${esc(x.name)}${x.dev ? '<i class="mds-dev">Developer</i>' : ''}</b><small>${esc(x.sub)}</small></span>
        <span class="mds-dot" aria-hidden="true"></span></button>`).join('') : `<p class="mds-none">${esc(empty)}</p>`;
    renderSide(true);
  }
  function renderSide(full) {
    const side = $('.mds-side'); if (!side) return;
    const m = installed(sel), c = catalog.find(x => x.id === sel), b = broken.find(x => x[0] === sel);
    if (!m && !c && !b) { side.innerHTML = ''; settingsFor = ''; return; }
    if (!full && side.dataset.for === sel) { fillLive(m); return; }
    side.dataset.for = sel; settingsFor = '';
    const man = m ? m.manifest : null, u = updateOf(sel), busy = upd.module_busy;
    const name = man ? man.name : c ? c.name : sel;
    const talks = (man ? man.talksTo : c ? c.talksTo : []) || [];
    const head = `<div class="mds-head"><span class="mds-ic big">${iconOf(sel, man)}</span><div><h2>${esc(name)}</h2><small>${man ? `Version ${esc(man.version)}${m.dev ? ' · Developer mode (unsigned)' : ''}` : c ? `Version ${esc(c.version)} · ${size(c.size)}` : 'Couldn’t load'}</small></div></div>`;
    const desc = `<p class="mds-desc">${esc((man || c || {}).description || (b ? b[1] : ''))}</p>`;
    let body = '';
    if (m) {
      body = `<div class="mds-facts"><div><small>State</small><b class="mds-state"></b></div><div><small>Memory</small><b class="mds-mem">–</b></div><div><small>CPU</small><b class="mds-cpu">–</b></div></div>
        <p class="mds-runs">${man.backend ? `Runs ${esc(RUNS[man.backend.run] || man.backend.run)}.` : 'No background process: it’s all in the island’s page.'}</p>
        <div class="mds-acts">
          <button class="cc-btn" type="button" data-a="toggle">${m.enabled ? 'Turn off' : 'Turn on'}</button>
          ${man.backend ? `<button class="cc-btn" type="button" data-a="restart"${m.enabled ? '' : ' disabled'}>${ic('restart')}Restart</button>` : ''}
          ${u ? `<button class="cc-btn primary" type="button" data-a="update"${busy ? ' disabled' : ''}>${ic('download')}Update to ${esc(u.to)}</button>` : ''}
          <button class="cc-btn danger" type="button" data-a="remove">${ic('trash')}Remove</button>
        </div>
        <div class="mds-confirm" ${confirmRemove ? '' : 'hidden'}>
          <label class="mds-forget"><input type="checkbox" data-forget> Also forget its keys and data</label>
          <span class="grow"></span>
          <button class="cc-btn" type="button" data-a="keep">Keep it</button>
          <button class="cc-btn danger" type="button" data-a="remove-yes">Remove</button>
        </div>
        <p class="mds-err" hidden></p>`;
    } else if (c) {
      const doing = busy && busy === c.name;
      body = `<div class="mds-acts"><button class="cc-btn primary" type="button" data-a="install"${busy ? ' disabled' : ''}>${ic('download')}${doing ? 'Installing…' : 'Install'}</button></div>`;
    }
    const talk = `<h4>Talks to</h4>${talks.length ? `<ul class="mds-talks">${talks.map(t => `<li>${esc(t)}</li>`).join('')}</ul>` : '<p class="mds-quiet">Nothing outside this PC.</p>'}`;
    const settings = m && settingsOf(sel) ? '<h4>Settings</h4><div class="mds-settings"></div>' : '';
    const log = m && man.backend ? `<details class="mds-logs"${logsFor === sel ? ' open' : ''}><summary>Recent log</summary><pre></pre></details>` : '';
    side.innerHTML = `<div class="mds-card">${head}${desc}${body}${talk}${settings}${log}</div>`;
    if (settings) mountSettings(m);
    fillLive(m);
  }
  /** A module's own settings page, in a shadow root with its CSS. */
  function mountSettings(m) {
    const host = $('.mds-settings'), fn = settingsOf(m.id); if (!host || !fn) return;
    settingsFor = m.id;
    const root = shadowFor(host, m.id, (m.manifest.ui && m.manifest.ui.css) || [], m.manifest.version);
    try { fn(root); } catch (e) { root.textContent = `Couldn't show its settings: ${e && e.message || e}`; }
  }
  /** What changes every couple of seconds: the state, memory, CPU and log. */
  function fillLive(m) {
    if (!m) return;
    const u = usage[m.id], st = $('.mds-state');
    if (st) { st.textContent = STATE[m.state] || m.state; st.className = `mds-state t-${m.state}`; }
    const mem = $('.mds-mem'), cpu = $('.mds-cpu');
    if (mem) mem.textContent = u && u.mem_mb != null ? `${u.mem_mb} MB` : '–';
    if (cpu) cpu.textContent = u && u.cpu != null ? `${u.cpu.toFixed(1)}%` : '–';
    const pre = $('.mds-logs pre');
    if (pre && logsFor === m.id) pre.textContent = logs || 'Nothing yet.';
    const err = $('.mds-err');
    if (err) { err.hidden = !m.error; err.textContent = m.error || ''; }
  }

  /* ---------- doing ---------- */
  const fail = e => flash(String((e && e.message) || e));
  async function act(a) {
    const m = installed(sel), c = catalog.find(x => x.id === sel);
    if (a === 'check') { flash('Checking for updates…'); try { upd = await invoke('update_check'); catalog = (await invoke('mod_catalog')) || []; render(); flash(upd.state === 'error' ? `Couldn't check: ${upd.error}` : 'Checked'); } catch (e) { fail(e); } return; }
    if (a === 'folder') { try { const info = await invoke('mod_load_folder', { path: null }); if (info) { view = 'installed'; sel = info.id; await load(); flash(`${info.manifest.name} loaded (developer mode)`); } } catch (e) { fail(e); } return; }
    if (a === 'updateall') { try { const done = await invoke('update_modules'); flash(done && done.length ? `Updated ${done.length}` : 'Nothing to update'); await load(); } catch (e) { fail(e); } return; }
    if (a === 'install' && c) { flash(`Installing ${c.name}…`); try { await invoke('mod_install', { id: c.id }); view = 'installed'; await load(); flash(`${c.name} is installed`); } catch (e) { fail(e); load(); } return; }
    if (!m) return;
    if (a === 'toggle') { try { await invoke('mod_set_enabled', { id: m.id, on: !m.enabled }); await load(); } catch (e) { fail(e); } return; }
    if (a === 'restart') { try { await invoke('mod_restart', { id: m.id }); flash(`Restarting ${m.manifest.name}`); load(); } catch (e) { fail(e); } return; }
    if (a === 'update') { try { await invoke('update_modules'); await load(); } catch (e) { fail(e); } return; }
    if (a === 'remove') { confirmRemove = true; renderSide(true); return; }
    if (a === 'keep') { confirmRemove = false; renderSide(true); return; }
    if (a === 'remove-yes') {
      const forget = !!(el.querySelector('[data-forget]') || {}).checked;
      confirmRemove = false;
      try { await invoke('mod_remove', { id: m.id, forget }); flash(`${m.manifest.name} removed${forget ? ', with its data' : ''}`); await load(); } catch (e) { fail(e); }
    }
  }
  function onClick(e) {
    const v = e.target.closest('[data-v]');
    if (v) { view = v.dataset.v; confirmRemove = false; render(); return; }
    const row = e.target.closest('.mds-row');
    if (row) { if (sel !== row.dataset.id) { sel = row.dataset.id; confirmRemove = false; logs = ''; } render(); return; }
    const a = e.target.closest('[data-a]');
    if (a && !a.disabled) act(a.dataset.a);
  }
  function onToggle(e) {
    const d = e.target.closest('.mds-logs'); if (!d) return;
    logsFor = d.open ? sel : '';
    if (d.open) poll();
  }
  /** Keys while the sheet shows: up and down through the list. */
  function key(e) {
    if (!el || (e.target.matches && e.target.matches('input, textarea, select'))) return false;
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return false;
    const r = rows(), i = r.findIndex(x => x.id === sel);
    const n = r[Math.max(0, Math.min(r.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))];
    if (n) { sel = n.id; confirmRemove = false; render(); }
    return true;
  }

  return {
    stat: () => `${list.length} installed${updates().length ? ` · ${updates().length} update${updates().length === 1 ? '' : 's'}` : ''}`,
    /** Shows the sheet on one module or list (from search: "install canvas"). */
    show(id, v) { if (v) view = v; if (id) sel = id; render(); },
    async mount(host) {
      el = document.createElement('div'); el.className = 'mds';
      build(); host.append(el);
      el.addEventListener('click', onClick);
      el.addEventListener('toggle', onToggle, true);
      for (const [ev, fn] of [['mod:changed', () => load()], ['mod:installed', () => load()], ['mod:removed', () => load()], ['update:status', e => { upd = e.payload || upd; render(); }]]) {
        try { const off = await listen(ev, fn); if (el) offs.push(off); else off(); } catch (e) {}
      }
      await load(); poll();
    },
    unmount() { clearTimeout(timer); for (const off of offs) { try { off(); } catch (e) {} } offs = []; if (el) el.remove(); el = null; settingsFor = ''; },
    key,
    /** For the cube's line before the sheet was ever opened. */
    refresh: load
  };
}
