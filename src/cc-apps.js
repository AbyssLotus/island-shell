// The Command Center's Apps sheet: everything in your Start menu, full size. Pinned apps, the ones
// you opened lately and your folders sit above every app from A to Z (with a letter rail to jump);
// the side shows the app under the pointer (its big icon, publisher, version and program file, with
// Open, Run as administrator, Open file location, Pin and Uninstall), every open window and the tray.
// The app list, pins and folders are the island's own (shared with its Apps tab); details come from
// apps.rs (app_info, app_action) and open windows from running.rs, polled while the sheet shows.

const VIEWS = [['all', 'All'], ['pinned', 'Pinned'], ['recent', 'Recent'], ['folders', 'Folders']];
const LETTERS = '#ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');
const KIND = { desktop: 'Desktop app', store: 'Store app', other: 'Shortcut' };
const letterOf = name => { const c = (name.trim()[0] || '#').toUpperCase(); return c >= 'A' && c <= 'Z' ? c : '#'; };
const exeName = exe => (exe.split('\\').pop() || '').replace(/\.exe$/i, '');
function ago(t) {
  const m = (Date.now() - t) / 6e4;
  if (m < 1) return 'just now';
  if (m < 60) return `${Math.round(m)} min ago`;
  if (m < 24 * 60) return `${Math.round(m / 60)} h ago`;
  const d = Math.round(m / 1440);
  if (d === 1) return 'yesterday';
  if (d < 14) return `${d} days ago`;
  return new Date(t).toLocaleDateString([], { month: 'short', day: 'numeric' });
}

export function createApps({ S, invoke, esc, ic, assetUrl, store, flash, close, gradFor, glyphOf, apps }) {
  let el = null, view = store.get('apView', 'all'), q = '', sel = null, wins = [], winsSig = '', traySig = '', appsN = -1, timer = 0, hoverT = 0, trayT = 0, scrollRaf = 0;
  const infos = new Map();
  const $ = s => el && el.querySelector(s), $$ = s => (el ? [...el.querySelectorAll(s)] : []);
  const all = () => S.apps || [];
  const use = () => S.appUse || {};
  const winsFor = a => wins.filter(w => (a.exe && w.exe === a.exe) || (apps.appForWin(w) || {}).id === a.id);
  const folderOf = id => S.folders.find(f => f.apps.includes(id));

  function build() {
    el.innerHTML = `<section class="aps-main">
        <div class="gms-bar aps-bar"><h3>Apps <small></small></h3>
          <div class="gms-chips" role="group" aria-label="Show">${VIEWS.map(([k, n]) => `<button type="button" data-v="${k}" aria-pressed="${k === view}">${n}</button>`).join('')}</div>
          <span class="grow"></span>
          <button class="aps-newf" type="button" data-a="newfolder" hidden>${ic('plus')}New folder</button>
          <label class="gms-search">${ic('search')}<input type="text" placeholder="Find an app or window" spellcheck="false" autocomplete="off" aria-label="Find an app or window"></label>
        </div>
        <div class="aps-body"><div class="aps-scroll"></div><nav class="aps-az" aria-label="Jump to a letter"></nav></div>
      </section>
      <aside class="aps-side">
        <div class="aps-card" aria-live="polite"></div>
        <div class="aps-open"><h4>Open now <small></small></h4><div class="aps-wins"></div></div>
        <div class="aps-trayw"><h4>In the tray</h4><div class="aps-tray"></div></div>
      </aside>
      <div class="aps-menu" role="menu" hidden></div>`;
    const qi = $('.gms-search input');
    qi.value = q;
    qi.addEventListener('input', () => { q = qi.value; renderMain(); renderWins(); });
    $('.aps-scroll').addEventListener('scroll', () => { cancelAnimationFrame(scrollRaf); scrollRaf = requestAnimationFrame(markLetter); });
  }

  /* ---------- the apps ---------- */
  const iconHtml = a => a.icon ? `<img src="${esc(assetUrl(a.icon))}" alt="" loading="lazy" draggable="false">` : `<span class="aps-gl" style="background:${gradFor(a.name)}">${esc(glyphOf(a.name))}</span>`;
  function tile(a, sub) {
    return `<button type="button" class="aps-app${a.id === sel ? ' sel' : ''}${winsFor(a).length ? ' run' : ''}" data-id="${esc(a.id)}" title="${esc(a.name)}">`
      + `<span class="aps-ic">${iconHtml(a)}</span><span class="aps-nm">${esc(a.name)}</span>${sub ? `<small class="aps-sub">${esc(sub)}</small>` : ''}</button>`;
  }
  function folderTile(f) {
    const minis = f.apps.map(apps.appOf).filter(Boolean).slice(0, 4).map(a => (a.icon ? `<img src="${esc(assetUrl(a.icon))}" alt="" draggable="false">` : `<span style="background:${gradFor(a.name)}"></span>`)).join('');
    return `<button type="button" class="aps-app aps-fold" data-folder="${esc(f.id)}" title="${esc(f.name)}"><span class="aps-ic">${minis}</span><span class="aps-nm">${esc(f.name)}</span><small class="aps-sub">${f.apps.length} app${f.apps.length === 1 ? '' : 's'}</small></button>`;
  }
  const grid = (list, cls = '', sub = null, pins = false) => `<div class="aps-grid ${cls}"${pins ? ' data-pins' : ''}>${list.map(a => tile(a, sub && sub(a))).join('')}</div>`;
  const sec = (title, n, body, extra = '') => `<section class="aps-sec"><h4 class="aps-h">${title}${n != null ? ` <small>${n}</small>` : ''}${extra}</h4>${body}</section>`;
  function recent(n) {
    const u = use();
    return Object.keys(u).sort((x, y) => u[y].t - u[x].t).map(apps.appOf).filter(Boolean).slice(0, n);
  }
  function rank(a, ql) { const n = a.name.toLowerCase(); return n.startsWith(ql) ? 0 : n.split(/[\s\-_.]+/).some(w => w.startsWith(ql)) ? 1 : 2; }
  /** The apps the main area shows right now, in order (search first, then the chosen view). */
  function renderMain() {
    const box = $('.aps-scroll'); if (!box) return;
    const list = all(), ql = q.trim().toLowerCase();
    let html = '', letters = false;
    if (!list.length) html = '<p class="aps-none">Reading your Start menu…</p>';
    else if (ql) {
      const hits = list.filter(a => a.name.toLowerCase().includes(ql)).sort((x, y) => rank(x, ql) - rank(y, ql) || x.name.localeCompare(y.name));
      html = hits.length ? sec('Apps', hits.length, grid(hits)) : `<p class="aps-none">No app called “${esc(q.trim())}”.${wins.some(winMatch) ? ' Matching windows are on the right.' : ''}</p>`;
      if (hits.length && !hits.some(a => a.id === sel)) sel = hits[0].id;
    } else if (view === 'pinned') {
      const pins = S.pins.map(apps.appOf).filter(Boolean);
      html = pins.length ? sec('Pinned', pins.length, grid(pins, 'big', null, true)) : '<p class="aps-none">Nothing pinned yet. Right-click any app and choose Pin.</p>';
    } else if (view === 'recent') {
      const u = use(), rs = recent(60);
      html = rs.length ? sec('Opened lately', rs.length, grid(rs, '', a => `${u[a.id].n}× · ${ago(u[a.id].t)}`)) : '<p class="aps-none">Apps you open from the island show up here.</p>';
    } else if (view === 'folders') {
      html = S.folders.length ? [...S.folders].sort((a, b) => a.name.localeCompare(b.name)).map(f => {
        const fa = f.apps.map(apps.appOf).filter(Boolean);
        return `<section class="aps-fsec" data-fid="${esc(f.id)}"><div class="aps-fh"><input type="text" value="${esc(f.name)}" data-fname="${esc(f.id)}" aria-label="Folder name" spellcheck="false"><small>${fa.length} app${fa.length === 1 ? '' : 's'}</small><button class="aps-wx on" type="button" data-a="delfolder" data-fid="${esc(f.id)}" title="Delete folder (the apps stay)" aria-label="Delete folder">${ic('trash')}</button></div>`
          + (fa.length ? grid(fa) : '<p class="aps-none sm">Right-click an app and choose this folder to put it here.</p>') + '</section>';
      }).join('') : '<p class="aps-none">No folders yet. Make one with New folder, then right-click apps to fill it.</p>';
    } else {
      const pins = S.pins.map(apps.appOf).filter(Boolean), rs = recent(8), u = use();
      if (pins.length) html += sec('Pinned', pins.length, grid(pins, 'big', null, true));
      if (rs.length) html += sec('Opened lately', null, grid(rs, '', a => ago(u[a.id].t)));
      if (S.folders.length) html += sec('Folders', S.folders.length, `<div class="aps-grid">${[...S.folders].sort((a, b) => a.name.localeCompare(b.name)).map(folderTile).join('')}</div>`);
      html += `<h4 class="aps-h">All apps <small>${list.length}</small></h4>`;
      for (const L of LETTERS) {
        const g = list.filter(a => letterOf(a.name) === L);
        if (g.length) html += `<div class="aps-l" data-l="${L}">${L}</div>${grid(g)}`;
      }
      letters = true;
    }
    box.innerHTML = html;
    renderAz(letters);
    $('.aps-newf').hidden = view !== 'folders' || !!ql;
    $$('.gms-chips [data-v]').forEach(b => b.setAttribute('aria-pressed', b.dataset.v === view));
    $('.aps-bar small').textContent = list.length ? `${list.length} apps · ${wins.length} window${wins.length === 1 ? '' : 's'}` : '';
    if (!sel || !apps.appOf(sel)) { const first = $('.aps-app[data-id]'); sel = first ? first.dataset.id : null; first && first.classList.add('sel'); }
    renderCard();
  }
  function renderAz(on) {
    const az = $('.aps-az');
    az.hidden = !on;
    if (!on) return;
    const have = new Set(all().map(a => letterOf(a.name)));
    az.innerHTML = LETTERS.map(L => `<button type="button" data-jump="${L}"${have.has(L) ? '' : ' disabled'}>${L}</button>`).join('');
    markLetter();
  }
  function markLetter() {
    const box = $('.aps-scroll'); if (!box || $('.aps-az').hidden) return;
    let cur = null;
    for (const h of $$('.aps-l')) { if (h.offsetTop - 30 <= box.scrollTop) cur = h.dataset.l; else break; }
    $$('.aps-az button').forEach(b => b.classList.toggle('on', b.dataset.jump === cur));
  }
  function jump(L) { const h = $(`.aps-l[data-l="${L}"]`); if (h) $('.aps-scroll').scrollTo({ top: h.offsetTop - 10, behavior: 'smooth' }); }
  function updateRun() { $$('.aps-app[data-id]').forEach(t => { const a = apps.appOf(t.dataset.id); t.classList.toggle('run', !!(a && winsFor(a).length)); }); }

  /* ---------- the chosen app ---------- */
  function select(id, tileEl, scroll) {
    if (!apps.appOf(id)) return;
    const changed = sel !== id;
    sel = id;
    $$('.aps-app.sel').forEach(t => t.classList.remove('sel'));
    const t = tileEl || $(`.aps-app[data-id="${CSS.escape(id)}"]`);
    if (t) { t.classList.add('sel'); if (scroll) t.scrollIntoView({ block: 'nearest' }); }
    if (changed) renderCard();
  }
  function fetchInfo(id) {
    if (infos.has(id)) return;
    infos.set(id, null);
    invoke('app_info', { id }).then(i => { infos.set(id, i); if (sel === id) renderCard(); }).catch(() => infos.delete(id));
  }
  let cardSig = '';
  function renderCard() {
    const box = $('.aps-card'); if (!box) return;
    const a = sel && apps.appOf(sel);
    if (!a) { cardSig = ''; box.innerHTML = '<div class="aps-plate"><span class="aps-sig"></span></div><p class="aps-none sm">Point at an app to see it here.</p>'; return; }
    fetchInfo(a.id);
    const i = infos.get(a.id), ws = winsFor(a), u = use()[a.id], pinned = S.pins.includes(a.id), f = folderOf(a.id);
    const sig = JSON.stringify([a.id, i, ws.map(w => [w.hwnd, w.title, w.minimized]), u, pinned, f && f.name]);
    if (sig === cardSig) return;
    const sameApp = cardSig.startsWith(JSON.stringify([a.id]).slice(0, -1));
    cardSig = sig;
    const big = i && i.icon ? assetUrl(i.icon) : a.icon ? assetUrl(a.icon) : null;
    const meta = i ? [i.publisher, i.version && `v${i.version}`].filter(Boolean).join(' · ') || KIND[i.kind] : 'Reading details…';
    const desc = i && i.description && i.description.toLowerCase() !== a.name.toLowerCase() ? `<p class="aps-desc">${esc(i.description)}</p>` : '';
    const dis = (ok, why) => (i && !ok ? ` disabled title="${why}"` : '');
    box.innerHTML = `<div class="aps-plate${sameApp ? ' still' : ''}">${big ? `<img class="aps-bg" src="${esc(big)}" alt="" draggable="false"><img class="aps-big" src="${esc(big)}" alt="" draggable="false">` : `<span class="aps-gl xl" style="background:${gradFor(a.name)}">${esc(glyphOf(a.name))}</span>`}</div>
      <div class="aps-meta"><h2>${esc(a.name)}</h2><p>${esc(meta)}</p>${desc}</div>
      <div class="aps-acts">
        <button class="aps-go" type="button" data-a="open">${ic('rocket')}Open</button>
        <button class="gms-ib aps-ib" type="button" data-a="admin" title="Run as administrator" aria-label="Run as administrator"${dis(i && i.admin, 'This app can’t run as administrator')}>${ic('shield')}</button>
        <button class="gms-ib aps-ib" type="button" data-a="location" title="Open file location" aria-label="Open file location"${dis(i && i.location, 'Nothing to show in File Explorer')}>${ic('folder')}</button>
        <button class="gms-ib aps-ib" type="button" data-a="pin" aria-pressed="${pinned}" title="${pinned ? 'Unpin' : 'Pin'}" aria-label="${pinned ? 'Unpin' : 'Pin'}">${ic('pin')}</button>
        <button class="gms-ib aps-ib" type="button" data-a="uninstall" title="Uninstall… (opens Settings)" aria-label="Uninstall"${dis(i && i.kind !== 'other', 'Not an installed app')}>${ic('trash')}</button>
      </div>
      <dl class="aps-facts">
        <div><dt>Kind</dt><dd>${i ? KIND[i.kind] : '…'}</dd></div>
        <div><dt>Running</dt><dd>${ws.length ? `${ws.length} window${ws.length === 1 ? '' : 's'}` : 'No'}</dd></div>
        <div><dt>Opened here</dt><dd>${u ? `${u.n}× · ${ago(u.t)}` : 'Not yet'}</dd></div>
        <div><dt>Folder</dt><dd>${f ? esc(f.name) : '—'}</dd></div>
        ${i && i.path ? `<div class="wide path"><dt>Program</dt><dd data-a="copypath" title="Copy the path">${esc(i.path)}</dd></div>` : ''}
      </dl>
      ${ws.length ? `<div class="aps-aw">${ws.map(winRow).join('')}</div>` : ''}`;
  }

  /* ---------- windows and the tray ---------- */
  const winMatch = w => { const ql = q.trim().toLowerCase(); return !ql || w.title.toLowerCase().includes(ql) || w.exe.includes(ql); };
  function winRow(w) {
    const a = apps.appForWin(w), src = a && a.icon ? a.icon : w.icon, name = a ? a.name : exeName(w.exe);
    const ico = src ? `<img src="${esc(assetUrl(src))}" alt="" draggable="false">` : `<span class="aps-gl" style="background:${gradFor(name)}">${esc(glyphOf(name))}</span>`;
    return `<div class="aps-w${w.active ? ' act' : ''}${w.minimized ? ' min' : ''}" role="button" tabindex="0" data-h="${w.hwnd}" title="${esc(w.title)}"><span class="aps-wi">${ico}</span><span class="aps-wt"><b>${esc(w.title)}</b><small>${esc(name)}${w.minimized ? ' · minimized' : w.active ? ' · last used' : ''}</small></span>`
      + `<button class="aps-wx" type="button" data-w="min" title="Minimize" aria-label="Minimize">${ic('down')}</button><button class="aps-wx" type="button" data-w="close" title="Close" aria-label="Close">${ic('x')}</button></div>`;
  }
  function renderWins() {
    const box = $('.aps-wins'); if (!box) return;
    const ws = wins.filter(winMatch);
    $('.aps-open small').textContent = ws.length;
    box.innerHTML = ws.length ? ws.map(winRow).join('') : `<p class="aps-none sm">${q.trim() ? 'No windows match.' : 'No windows open.'}</p>`;
  }
  function renderTray() {
    const t = S.tray || [], sig = t.map(x => x.key + x.icon.length).join();
    if (sig === traySig) return;
    traySig = sig;
    $('.aps-trayw').hidden = !t.length;
    $('.aps-tray').innerHTML = t.map(x => `<button type="button" data-key="${esc(x.key)}" title="${esc(x.tip || x.exe)}" aria-label="${esc(x.tip || x.exe)}"><img src="${esc(x.icon)}" alt="" draggable="false"></button>`).join('');
  }
  async function poll() {
    clearTimeout(timer);
    try {
      const ws = await invoke('windows_list'), sig = JSON.stringify(ws);
      if (el && sig !== winsSig) { winsSig = sig; wins = ws; renderWins(); updateRun(); renderCard(); $('.aps-bar small').textContent = all().length ? `${all().length} apps · ${wins.length} window${wins.length === 1 ? '' : 's'}` : ''; }
    } catch (e) {}
    if (!el) return;
    if (all().length !== appsN) { appsN = all().length; renderMain(); }
    renderTray();
    timer = setTimeout(poll, 1500);
  }
  const findWin = h => wins.find(x => String(x.hwnd) === h);
  function closeWin(w) { invoke('window_close', { hwnd: w.hwnd }).catch(() => {}); wins = wins.filter(x => x !== w); winsSig = ''; renderWins(); updateRun(); renderCard(); }
  function minWin(w) { invoke('window_minimize', { hwnd: w.hwnd }).catch(() => {}); w.minimized = true; winsSig = ''; renderWins(); renderCard(); }

  /* ---------- actions ---------- */
  function openApp(a, pinned) { close(() => (pinned ? apps.openPinned(a) : apps.launch(a))); }
  function appAction(a, what) {
    // Each of these brings up another window (Windows' question, File Explorer, Settings), so the
    // Command Center gets out of the way first.
    close(() => invoke('app_action', { id: a.id, what }).catch(e => apps.toast(`Couldn't do that for ${a.name}`, String(e))));
  }
  function togglePin(id) { apps.togglePin(id); renderMain(); }
  function moveTo(id, fid) { apps.moveToFolder(id, fid); cardSig = ''; renderMain(); }
  function newFolder(withApp) {
    let name = 'New folder', n = 2;
    while (S.folders.some(f => f.name === name)) name = `New folder ${n++}`;
    const f = { id: 'f' + Date.now(), name, apps: [] };
    S.folders.push(f); apps.saveFolders();
    if (withApp) apps.moveToFolder(withApp, f.id); else apps.changed();
    view = 'folders'; store.set('apView', view); q = ''; $('.gms-search input').value = '';
    renderMain();
    const inp = $(`[data-fname="${f.id}"]`); if (inp) { inp.scrollIntoView({ block: 'nearest' }); inp.focus(); inp.select(); }
  }
  function doAct(what, b) {
    const a = sel && apps.appOf(sel);
    if (what === 'newfolder') return newFolder(null);
    if (what === 'delfolder') { S.folders = S.folders.filter(f => f.id !== b.dataset.fid); apps.saveFolders(); apps.changed(); renderMain(); return; }
    if (!a) return;
    if (what === 'open') openApp(a, false);
    else if (what === 'pin') togglePin(a.id);
    else if (what === 'copypath') { const i = infos.get(a.id); if (i && i.path) navigator.clipboard.writeText(i.path).then(() => flash('Path copied')).catch(() => {}); }
    else appAction(a, what);
  }
  function setView(v) { view = v; store.set('apView', v); q = ''; $('.gms-search input').value = ''; renderMain(); renderWins(); $('.aps-scroll').scrollTop = 0; }

  /* ---------- the right-click menu ---------- */
  function hideMenu() { const m = $('.aps-menu'); if (m) m.hidden = true; }
  function showMenu(x, y, items) {
    const m = $('.aps-menu');
    m.innerHTML = items.map((it, n) => (it.sep ? '<hr>' : it.heading ? `<div class="aps-mh">${esc(it.heading)}</div>` : `<button type="button" role="menuitem" data-mi="${n}"${it.off ? ' disabled' : ''}${it.danger ? ' class="danger"' : ''}>${ic(it.icon)}<span>${esc(it.label)}</span></button>`)).join('');
    m.hidden = false;
    const r = m.getBoundingClientRect();
    m.style.left = Math.max(6, Math.min(x, innerWidth - r.width - 6)) + 'px';
    m.style.top = Math.max(6, Math.min(y, innerHeight - r.height - 6)) + 'px';
    m.onclick = e => { const b = e.target.closest('[data-mi]'); if (!b) return; hideMenu(); items[+b.dataset.mi].run(); };
    const first = m.querySelector('button:not([disabled])'); if (first) first.focus({ preventScroll: true });
  }
  function appMenu(a, x, y) {
    fetchInfo(a.id);
    const i = infos.get(a.id), pinned = S.pins.includes(a.id), inF = folderOf(a.id), ws = winsFor(a);
    const items = [{ label: 'Open', icon: 'rocket', run: () => openApp(a, false) }];
    if (ws.length) items.push({ label: 'Switch to its window', icon: 'monitor', run: () => close(() => apps.activate(ws[0])) });
    items.push({ label: 'Run as administrator', icon: 'shield', off: i && !i.admin, run: () => appAction(a, 'admin') });
    items.push({ label: 'Open file location', icon: 'folder', off: i && !i.location, run: () => appAction(a, 'location') });
    items.push({ sep: true }, { label: pinned ? 'Unpin' : 'Pin', icon: 'pin', run: () => togglePin(a.id) });
    items.push({ sep: true }, { heading: 'Folder' });
    S.folders.filter(f => f !== inF).forEach(f => items.push({ label: f.name, icon: 'folder', run: () => moveTo(a.id, f.id) }));
    items.push({ label: 'New folder…', icon: 'plus', run: () => newFolder(a.id) });
    if (inF) items.push({ label: `Take out of ${inF.name}`, icon: 'x', run: () => moveTo(a.id, null) });
    items.push({ sep: true }, { label: 'Uninstall…', icon: 'trash', danger: true, off: i && i.kind === 'other', run: () => appAction(a, 'uninstall') });
    showMenu(x, y, items);
  }
  function winMenu(w, x, y) {
    const pid = apps.pinIdForWin(w), a = pid && apps.appOf(pid);
    const items = [
      { label: 'Switch to', icon: 'monitor', run: () => close(() => apps.activate(w)) },
      { label: 'Minimize', icon: 'down', run: () => minWin(w) },
      { label: 'Close window', icon: 'x', danger: true, run: () => closeWin(w) }
    ];
    if (a) items.push({ sep: true }, { label: S.pins.includes(a.id) ? `Unpin ${a.name}` : `Pin ${a.name}`, icon: 'pin', run: () => { apps.togglePin(a.id); renderMain(); } });
    showMenu(x, y, items);
  }

  /* ---------- input ---------- */
  function onClick(e) {
    if (!e.target.closest('.aps-menu')) hideMenu();
    const b = e.target.closest('[data-a]'); if (b && !b.disabled) { doAct(b.dataset.a, b); return; }
    const v = e.target.closest('[data-v]'); if (v) { setView(v.dataset.v); return; }
    const L = e.target.closest('[data-jump]'); if (L) { jump(L.dataset.jump); return; }
    const fo = e.target.closest('[data-folder]');
    if (fo) { setView('folders'); const s = $(`.aps-fsec[data-fid="${CSS.escape(fo.dataset.folder)}"]`); if (s) s.scrollIntoView({ block: 'start' }); return; }
    const row = e.target.closest('.aps-w');
    if (row) { const w = findWin(row.dataset.h); if (!w) return; const x = e.target.closest('[data-w]'); if (x) (x.dataset.w === 'close' ? closeWin : minWin)(w); else close(() => apps.activate(w)); return; }
    const t = e.target.closest('.aps-app[data-id]');
    if (t) { const a = apps.appOf(t.dataset.id); if (a) openApp(a, !!t.closest('[data-pins]')); return; }
    const tr = e.target.closest('.aps-tray [data-key]');
    if (tr) { clearTimeout(trayT); trayT = setTimeout(() => close(() => invoke('tray_click', { key: tr.dataset.key, button: 'left' }).catch(() => {})), 230); }
  }
  function onDbl(e) {
    const tr = e.target.closest('.aps-tray [data-key]'); if (!tr) return;
    clearTimeout(trayT); close(() => invoke('tray_click', { key: tr.dataset.key, button: 'double' }).catch(() => {}));
  }
  function onContext(e) {
    const tr = e.target.closest('.aps-tray [data-key]');
    if (tr) { e.preventDefault(); clearTimeout(trayT); close(() => invoke('tray_click', { key: tr.dataset.key, button: 'right' }).catch(() => {})); return; }
    if (e.target.closest('input')) return;
    e.preventDefault();
    const row = e.target.closest('.aps-w'), t = e.target.closest('.aps-app[data-id]');
    if (row) { const w = findWin(row.dataset.h); if (w) winMenu(w, e.clientX, e.clientY); }
    else if (t) { select(t.dataset.id, t); const a = apps.appOf(t.dataset.id); if (a) appMenu(a, e.clientX, e.clientY); }
    else if (e.target.closest('.aps-card') && sel) { const a = apps.appOf(sel); if (a) appMenu(a, e.clientX, e.clientY); }
    else hideMenu();
  }
  // The side follows the pointer, after a moment so sweeping across to it doesn't change it.
  function onOver(e) {
    const t = e.target.closest('.aps-app[data-id]');
    clearTimeout(hoverT);
    if (t && $('.aps-menu').hidden) hoverT = setTimeout(() => select(t.dataset.id, t), 110);
  }
  function onInput(e) {
    const n = e.target.closest('[data-fname]'); if (!n) return;
    const f = S.folders.find(x => x.id === n.dataset.fname); if (!f) return;
    f.name = n.value.trim() || 'Folder'; apps.saveFolders(); apps.changed();
  }
  function move(k) {
    const tiles = $$('.aps-scroll .aps-app[data-id]'); if (!tiles.length) return;
    let i = tiles.findIndex(t => t.classList.contains('sel'));
    if (i < 0) i = Math.max(0, tiles.findIndex(t => t.dataset.id === sel));
    let j = i;
    if (k === 'ArrowLeft') j = Math.max(0, i - 1);
    else if (k === 'ArrowRight') j = Math.min(tiles.length - 1, i + 1);
    else {
      const r = tiles[i].getBoundingClientRect(), cx = r.left + r.width / 2, down = k === 'ArrowDown';
      const rows = tiles.map((t, n) => [n, t.getBoundingClientRect()]).filter(([, b]) => (down ? b.top > r.top + 4 : b.top < r.top - 4));
      if (rows.length) {
        const rowTop = down ? Math.min(...rows.map(([, b]) => b.top)) : Math.max(...rows.map(([, b]) => b.top));
        j = rows.filter(([, b]) => Math.abs(b.top - rowTop) < 4).sort((x, y) => Math.abs(x[1].left + x[1].width / 2 - cx) - Math.abs(y[1].left + y[1].width / 2 - cx))[0][0];
      }
    }
    select(tiles[j].dataset.id, tiles[j], true);
  }
  /** Keys while the sheet shows; true when it used the key. */
  function key(e) {
    if (!el) return false;
    const m = $('.aps-menu');
    if (!m.hidden) {
      if (e.key === 'Escape') { hideMenu(); return true; }
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        const bs = [...m.querySelectorAll('button:not([disabled])')], i = bs.indexOf(document.activeElement);
        const n = bs[(i + (e.key === 'ArrowDown' ? 1 : -1) + bs.length) % bs.length]; if (n) n.focus();
        return true;
      }
      return false;
    }
    const qi = $('.gms-search input'), inSearch = e.target === qi, inName = e.target.matches && e.target.matches('[data-fname]');
    if (inName) { if (e.key === 'Enter') { e.target.blur(); renderMain(); return true; } return false; }
    if (e.key === 'Escape' && inSearch && qi.value) { qi.value = ''; q = ''; renderMain(); renderWins(); return true; }
    if (['ArrowUp', 'ArrowDown'].includes(e.key) || (!inSearch && ['ArrowLeft', 'ArrowRight'].includes(e.key))) { move(e.key); return true; }
    if (e.key === 'Enter') {
      const a = sel && apps.appOf(sel); if (!a) return false;
      if (e.ctrlKey) appAction(a, 'admin'); else openApp(a, false);
      return true;
    }
    if (!inSearch && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey && !/^[1-9]$/.test(e.key)) { qi.focus(); return false; }
    return false;
  }

  return {
    mount(host) {
      el = document.createElement('div'); el.className = 'aps';
      build(); host.append(el);
      el.addEventListener('click', onClick); el.addEventListener('dblclick', onDbl); el.addEventListener('contextmenu', onContext);
      el.addEventListener('pointerover', onOver); el.addEventListener('input', onInput);
      if (!sel) { const r = recent(1)[0] || S.pins.map(apps.appOf).filter(Boolean)[0]; sel = r ? r.id : null; }
      winsSig = ''; traySig = ''; appsN = all().length; cardSig = '';
      renderMain(); renderWins(); renderTray();
      poll();
    },
    unmount() { clearTimeout(timer); clearTimeout(hoverT); clearTimeout(trayT); cancelAnimationFrame(scrollRaf); if (el) el.remove(); el = null; },
    key
  };
}
