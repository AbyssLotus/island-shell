// The first-run setup guide: a few full-screen steps over the Command Center, the first time Island
// Shell starts (and again from "Setup guide" in search). It explains the island, the Windows key and
// the Command Center, picks a layout and a skin, starts with Windows, and picks the modules to add
// (from the signed release catalog; they install in the background, Music ticked to start with).
// Nothing in it is required; every step can be skipped.

const STEPS = ['welcome', 'island', 'cc', 'look', 'start', 'modules', 'done'];
/** The island's own icon for each first-party module (the catalog carries no pictures). */
const MOD_ICON = { music: 'note', discord: 'chat', canvas: 'cap', destiny: 'orbit', games: 'gamepad', gaze: 'eye', voicemeeter: 'speaker', 'wallpaper-engine': 'image', vpn: 'shield', focus: 'timer', notion: 'check', transit: 'bus' };
const size = b => (b >= 1048576 ? (b / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1024)) + ' KB');

export function createSetup({ root, invoke, store, ic, esc, S, setDark, openTab, moduleCards }) {
  const el = document.createElement('div');
  el.className = 'su-wrap'; el.hidden = true;
  el.setAttribute('role', 'dialog'); el.setAttribute('aria-modal', 'true'); el.setAttribute('aria-label', 'Island Shell setup');
  root.append(el);
  let step = 0, layout = 'island', dark = true, auto = true, isOpen = false, onClose = null;
  // The module step: the catalog, what's installed, what's ticked, and what was asked for.
  let cat = [], inst = new Set(), picked = new Set(), touched = false, upd = {}, loaded = false, asked = [];

  const keycap = k => `<kbd>${k}</kbd>`;
  /** One tip: the key on the left, what it does on the right (one grid cell each). */
  const tip = (key, text) => `<li><span class="su-k">${keycap(key)}</span><span class="su-tipd">${text}</span></li>`;
  const win = `<svg viewBox="0 0 24 24" class="su-win" aria-hidden="true"><path d="M3 5.5 10.5 4.4v7.1H3zM11.5 4.3 21 3v8.5h-9.5zM3 12.5h7.5v7.1L3 18.5zM11.5 12.5H21V21l-9.5-1.3z"/></svg>`;
  const art = {
    welcome: `<div class="su-eclipse"><i></i><b></b></div>`,
    island: `<div class="su-desk"><div class="su-pill"><span class="su-pd"></span><span class="su-pt">9:41</span><span class="su-peq"><i></i><i></i><i></i><i></i></span></div><div class="su-panel"><div class="su-ptabs"><i></i><i class="su-on"></i><i></i><i></i><i></i></div><div class="su-pcover"></div><div class="su-plines"><i></i><i></i><i></i></div></div><div class="su-cursor"></div></div>`,
    cc: `<div class="su-mini"><div class="su-core">9:41<small>Search anything</small></div>${['', '', '', '', '', ''].map((_, i) => `<span class="su-cube su-c${i}"></span>`).join('')}</div>`,
    look: `<div class="su-looks"><div class="su-lk su-lk-island${layout === 'island' ? ' su-on' : ''}"><span class="su-lk-pillshape"></span></div><div class="su-lk su-lk-bar${layout === 'bar' ? ' su-on' : ''}"><span class="su-lk-strip"></span></div></div>`,
    start: `<div class="su-power">${win}<span class="su-ring"></span></div>`,
    modules: `<div class="su-orbit"><span class="su-me"></span>${['note', 'gamepad', 'chat', 'cap', 'star', 'calendar'].map((n, i) => `<span class="su-o su-o${i}">${ic(n)}</span>`).join('')}</div>`,
    done: `<div class="su-eclipse su-done"><i></i><b></b></div>`
  };

  /** The catalog, what's installed and the updater's state, read when the guide opens. */
  async function loadModules() {
    loaded = false;
    try { upd = (await invoke('update_status')) || {}; } catch (e) {}
    try { cat = (await invoke('mod_catalog')) || []; } catch (e) {}
    if (!cat.length && upd.state && upd.state !== 'off') {
      try { upd = await invoke('update_check'); cat = (await invoke('mod_catalog')) || []; } catch (e) { upd = { ...upd, state: 'error', error: String((e && e.message) || e) }; }
    }
    try { const l = await invoke('mod_list'); inst = new Set(((l && l.modules) || []).map(m => m.id)); } catch (e) {}
    if (!touched) picked = new Set(cat.filter(c => c.preselect && !inst.has(c.id)).map(c => c.id));
    loaded = true;
    if (isOpen && STEPS[step] === 'modules') render();
  }

  function moduleList() {
    const card = (icn, title, sub, right, extra = '') => `<div class="su-card${extra}"><span class="su-cic">${ic(icn)}</span><span class="su-ctx"><b>${title}</b><small>${sub}</small></span>${right}</div>`;
    const setUp = tab => `<button class="su-btn su-ghost" type="button" data-su-tab="${esc(tab)}">Set up</button>`;
    const own = moduleCards(), cards = [];
    for (const c of cat) {
      const talks = (c.talksTo || []).length ? `Talks to ${c.talksTo.join('; ')}` : 'Nothing leaves this PC';
      const sub = `${esc(c.description)}<span class="su-mt">${esc(talks)} · ${size(c.size)}</span>`;
      if (inst.has(c.id)) {
        // Installed already: its tab can set it up, unless it says it's ready.
        const mc = own.find(m => m.id === c.id);
        cards.push(card(MOD_ICON[c.id] || 'layers', esc(c.name), sub, mc && !mc.state ? setUp(mc.tab) : `<span class="su-cst">${esc((mc && mc.state) || 'Installed')}</span>`, ' su-ok'));
      } else {
        const on = picked.has(c.id);
        cards.push(card(MOD_ICON[c.id] || 'layers', esc(c.name), sub, `<button class="su-btn su-pick ${on ? 'su-primary' : 'su-ghost'}" type="button" data-su-pick="${esc(c.id)}" aria-pressed="${on}">${on ? 'Added' : 'Add'}</button>`, on ? ' su-picked' : ''));
      }
    }
    // Installed modules the catalog doesn't list (developer mode), with their own setup.
    for (const m of own) if (!cat.some(c => c.id === m.id)) cards.push(card(MOD_ICON[m.id] || m.icon, esc(m.title), esc(m.sub), m.state ? `<span class="su-cst">${esc(m.state)}</span>` : setUp(m.tab), m.state ? ' su-ok' : ''));
    cards.push(card('calendar', 'Calendars', 'Built in: add any calendar’s iCal link (Google, Outlook, school) in the Today cube.', setUp('today')));
    return cards.join('');
  }
  function modulesNote() {
    if (!loaded) return '<p class="su-note">Looking for modules…</p>';
    if (cat.length) return '';
    const why = upd.state === 'off' ? 'This build of Island Shell can’t download modules (it has no release key).'
      : upd.state === 'error' ? `Island Shell couldn’t reach its releases right now${upd.error ? ` (${esc(upd.error)})` : ''}.` : 'There are no modules to add right now.';
    return `<p class="su-note">${why} You can add modules any time from the Modules cube in the Command Center.</p>`;
  }
  /** Leaving the module step: what's ticked starts installing in the background. */
  function commitModules() {
    const ids = [...picked].filter(id => !inst.has(id) && !asked.includes(id));
    if (!ids.length) return;
    asked.push(...ids);
    invoke('mod_queue', { ids }).catch(() => {});
  }
  /** To step n (the module step's picks go when it's left). */
  function go(n) {
    if (STEPS[step] === 'modules' && n !== step) commitModules();
    step = n; render();
  }

  function body() {
    switch (STEPS[step]) {
      case 'welcome': return `<h2>Welcome to Island Shell</h2><p class="su-lead">Your taskbar becomes an island at the top of the screen: music, calls, sound, your day and your PC, one tap away.</p><p>This takes about a minute. Everything here can be changed later.</p>`;
      case 'island': return `<h2>The island</h2><p class="su-lead">It sits at the top of your screen and grows into a panel when you need it.</p>
        <ul class="su-tips">${tip(win + ' Win', 'Tap the Windows key to open it; tap again (or Esc) to close.')}${tip('Hover', 'Point at it for a peek; click a tab to go straight there.')}${tip(win + ' + A', 'Win with A, G or V jumps to Audio, Games and Clipboard; N or W to Today.')}</ul>
        <p class="su-note">The Windows taskbar is hidden while Island Shell runs, and comes back when you quit (the island’s Power tab, or its tray icon).</p>`;
      case 'cc': return `<h2>The Command Center</h2><p class="su-lead">${keycap(win + ' + Space')} opens it: one search for apps, windows, files, games, settings and commands, with every section as a cube.</p>
        <ul class="su-tips">${tip('Type', `Search right away; Enter opens, Tab switches kinds, ${keycap('&gt;')} for commands, ${keycap('=')} for math.`)}${tip('Cubes', 'Each opens full size: your Steam library, Apps, Music with lyrics, a calendar, Discord…')}${tip('Stats', 'A full Task Manager: processes, performance, sensors, disk space, benchmarks.')}</ul>`;
      case 'look': return `<h2>Pick your look</h2><p class="su-lead">The island floats at the top center. The bar is a full-width strip with the clock, your apps and the tray.</p>
        <div class="su-choice" role="group" aria-label="Layout"><button type="button" data-su-layout="island" aria-pressed="${layout === 'island'}"><b>Island</b><small>A pill that grows</small></button><button type="button" data-su-layout="bar" aria-pressed="${layout === 'bar'}"><b>Bar</b><small>A strip across the top</small></button></div>
        <div class="su-choice" role="group" aria-label="Skin"><button type="button" data-su-dark="1" aria-pressed="${dark}"><b>Dark</b><small>Glowing on black</small></button><button type="button" data-su-dark="0" aria-pressed="${!dark}"><b>Light</b><small>Ink on frosted glass</small></button></div>`;
      case 'start': return `<h2>Start with Windows</h2><p class="su-lead">So the island is there every time you sign in, and your taskbar stays tucked away.</p>
        <label class="su-toggle"><input type="checkbox" data-su-auto${auto ? ' checked' : ''}><i></i><span>Start Island Shell when I sign in</span></label>
        <p class="su-note">It doesn’t need administrator rights, and it doesn’t slow sign-in: it waits until Windows is ready.</p>`;
      case 'modules': return `<h2>Pick your modules</h2><p class="su-lead">The island, the Command Center, the Task Manager and your calendar are built in. The rest are modules: add the ones you want and leave out the others. They install in the background.</p>${modulesNote()}<div class="su-cards">${moduleList()}</div>`;
      case 'done': {
        const names = asked.map(id => (cat.find(c => c.id === id) || {}).name || id);
        return `<h2>You’re all set</h2><p class="su-lead">Tap ${keycap(win + ' Win')} for the island and ${keycap(win + ' + Space')} for the Command Center.</p><p>This guide is in the Command Center’s search as “Setup guide” if you want it again.</p>${names.length ? `<p class="su-note">Installing ${esc(names.join(', '))} in the background; each shows up as soon as it’s ready. The Modules cube has the rest.</p>` : ''}${layout === 'bar' ? '<p class="su-note">Island Shell restarts in the bar layout when you finish (about a second).</p>' : ''}`;
      }
    }
    return '';
  }

  function render() {
    const id = STEPS[step], last = step === STEPS.length - 1;
    el.innerHTML = `<div class="su-card-main" data-step="${id}">
      <div class="su-art">${art[id]}</div>
      <div class="su-body"><div class="su-dots" aria-hidden="true">${STEPS.map((_, i) => `<i class="${i === step ? 'su-on' : i < step ? 'su-past' : ''}"></i>`).join('')}</div>
        <div class="su-text">${body()}</div>
        <div class="su-foot">${step > 0 && !last ? '<button class="su-btn su-ghost" type="button" data-su="back">Back</button>' : '<span></span>'}<span class="su-grow"></span>${!last ? '<button class="su-btn su-link" type="button" data-su="skip">Skip setup</button>' : ''}<button class="su-btn su-primary" type="button" data-su="next">${step === 0 ? 'Let’s go' : last ? 'Finish' : 'Next'}</button></div></div></div>`;
    const b = el.querySelector('[data-su="next"]'); if (b) setTimeout(() => b.focus({ preventScroll: true }), 30);
  }

  async function finish(thenTab) {
    if (STEPS[step] === 'modules') commitModules();
    store.set('setupDone', true);
    // A guide that's been through picks its own modules: there's no upgrade to migrate from.
    store.set('modulesMigrated', true);
    isOpen = false; el.classList.add('su-out');
    setTimeout(() => { el.hidden = true; el.classList.remove('su-out'); }, 260);
    try { await invoke('autostart_set', { on: auto }); } catch (e) {}
    if (onClose) onClose(thenTab);
    if (layout === 'bar' && S.style !== 'bar') { try { await invoke('set_style', { style: 'bar' }); } catch (e) {} }
  }

  el.addEventListener('click', e => {
    const b = e.target.closest('[data-su]');
    if (b) {
      if (b.dataset.su === 'next') { if (step === STEPS.length - 1) finish(); else go(step + 1); }
      else if (b.dataset.su === 'back') go(Math.max(0, step - 1));
      else if (b.dataset.su === 'skip') finish();
      return;
    }
    const l = e.target.closest('[data-su-layout]'); if (l) { layout = l.dataset.suLayout; render(); return; }
    const d = e.target.closest('[data-su-dark]'); if (d) { dark = d.dataset.suDark === '1'; setDark(dark); render(); return; }
    const p = e.target.closest('[data-su-pick]'); if (p) { touched = true; const id = p.dataset.suPick; if (picked.has(id)) picked.delete(id); else picked.add(id); render(); return; }
    const t = e.target.closest('[data-su-tab]'); if (t) { finish(t.dataset.suTab); }
  });
  el.addEventListener('change', e => { if (e.target.matches('[data-su-auto]')) auto = e.target.checked; });

  return {
    isOpen: () => isOpen,
    async open(done) {
      onClose = done || null;
      step = 0; layout = S.style === 'bar' || S.style === 'tall' ? 'bar' : 'island'; dark = S.dark !== false;
      try { auto = await invoke('autostart_get'); if (!store.get('setupDone')) auto = true; } catch (e) {}
      touched = false; asked = [];
      isOpen = true; el.hidden = false; render();
      loadModules();
    },
    /** Keys while it's open: true when used (the Command Center leaves them alone). */
    key(e) {
      if (!isOpen) return false;
      const typing = !!(e.target && e.target.matches && e.target.matches('input'));
      if (e.key === 'Escape') { finish(); return true; }
      if (e.key === 'ArrowRight' && !typing) { if (step < STEPS.length - 1) go(step + 1); return true; }
      if (e.key === 'ArrowLeft' && !typing) { if (step > 0) go(step - 1); return true; }
      return e.key !== 'Tab' && e.key !== 'Enter' && e.key !== ' ';
    }
  };
}
