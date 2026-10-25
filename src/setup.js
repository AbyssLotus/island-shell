// The first-run setup guide: a few full-screen steps over the Command Center, the first time Island
// Shell starts (and again from "Setup guide" in search). It explains the island, the Windows key and
// the Command Center, picks a layout and a skin, starts with Windows, and points at what can be
// connected. Nothing in it is required; every step can be skipped.

const STEPS = ['welcome', 'island', 'cc', 'look', 'start', 'connect', 'done'];

export function createSetup({ root, invoke, store, ic, esc, S, setDark, openTab, moduleCards }) {
  const el = document.createElement('div');
  el.className = 'su-wrap'; el.hidden = true;
  el.setAttribute('role', 'dialog'); el.setAttribute('aria-modal', 'true'); el.setAttribute('aria-label', 'Island Shell setup');
  root.append(el);
  let step = 0, layout = 'island', dark = true, auto = true, isOpen = false, onClose = null;

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
    connect: `<div class="su-orbit"><span class="su-me"></span>${['note', 'gamepad', 'chat', 'cap', 'star', 'calendar'].map((n, i) => `<span class="su-o su-o${i}">${ic(n)}</span>`).join('')}</div>`,
    done: `<div class="su-eclipse su-done"><i></i><b></b></div>`
  };

  function connectCards() {
    const dc = S.dc || {};
    const card = (icn, title, sub, state, tab) => `<div class="su-card${state ? ' su-ok' : ''}"><span class="su-cic">${ic(icn)}</span><span class="su-ctx"><b>${title}</b><small>${sub}</small></span>${state ? `<span class="su-cst">${state}</span>` : `<button class="su-btn su-ghost" type="button" data-su-tab="${tab}">Set up</button>`}</div>`;
    return card('note', 'Music', 'Apple Music, Spotify, your browser: anything Windows knows is playing. Lyrics come along.', S.media ? 'Playing now' : 'Automatic')
      + card('chat', 'Discord', 'Your call (mute, deafen, who’s talking) and notifications on the island. Needs the Discord app and a free Discord application, about two minutes.', dc.authorized ? 'Connected' : '', 'discord')
      + moduleCards().map(m => card(m.icon, esc(m.title), esc(m.sub), '', m.tab)).join('')
      + card('calendar', 'Calendars', 'Add any calendar’s iCal link (Google, Outlook, school) in the Today cube.', '', 'today');
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
      case 'connect': return `<h2>Connect your stuff</h2><p class="su-lead">These work on their own or after a quick setup. All optional; they live in their tabs whenever you want them.</p><div class="su-cards">${connectCards()}</div>`;
      case 'done': return `<h2>You’re all set</h2><p class="su-lead">Tap ${keycap(win + ' Win')} for the island and ${keycap(win + ' + Space')} for the Command Center.</p><p>This guide is in the Command Center’s search as “Setup guide” if you want it again.</p>${layout === 'bar' ? '<p class="su-note">Island Shell restarts in the bar layout when you finish (about a second).</p>' : ''}`;
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
    store.set('setupDone', true);
    isOpen = false; el.classList.add('su-out');
    setTimeout(() => { el.hidden = true; el.classList.remove('su-out'); }, 260);
    try { await invoke('autostart_set', { on: auto }); } catch (e) {}
    if (onClose) onClose(thenTab);
    if (layout === 'bar' && S.style !== 'bar') { try { await invoke('set_style', { style: 'bar' }); } catch (e) {} }
  }

  el.addEventListener('click', e => {
    const b = e.target.closest('[data-su]');
    if (b) {
      if (b.dataset.su === 'next') { if (step === STEPS.length - 1) finish(); else { step++; render(); } }
      else if (b.dataset.su === 'back') { step = Math.max(0, step - 1); render(); }
      else if (b.dataset.su === 'skip') finish();
      return;
    }
    const l = e.target.closest('[data-su-layout]'); if (l) { layout = l.dataset.suLayout; render(); return; }
    const d = e.target.closest('[data-su-dark]'); if (d) { dark = d.dataset.suDark === '1'; setDark(dark); render(); return; }
    const t = e.target.closest('[data-su-tab]'); if (t) { finish(t.dataset.suTab); }
  });
  el.addEventListener('change', e => { if (e.target.matches('[data-su-auto]')) auto = e.target.checked; });

  return {
    isOpen: () => isOpen,
    async open(done) {
      onClose = done || null;
      step = 0; layout = S.style === 'bar' || S.style === 'tall' ? 'bar' : 'island'; dark = S.dark !== false;
      try { auto = await invoke('autostart_get'); if (!store.get('setupDone')) auto = true; } catch (e) {}
      isOpen = true; el.hidden = false; render();
    },
    /** Keys while it's open: true when used (the Command Center leaves them alone). */
    key(e) {
      if (!isOpen) return false;
      const typing = !!(e.target && e.target.matches && e.target.matches('input'));
      if (e.key === 'Escape') { finish(); return true; }
      if (e.key === 'ArrowRight' && !typing) { if (step < STEPS.length - 1) { step++; render(); } return true; }
      if (e.key === 'ArrowLeft' && !typing) { if (step > 0) { step--; render(); } return true; }
      return e.key !== 'Tab' && e.key !== 'Enter' && e.key !== ' ';
    }
  };
}
