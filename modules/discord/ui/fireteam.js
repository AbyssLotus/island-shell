// What the Discord tab (tab.js) and sheet (sheet.js) both draw: the Taken sigil, what a card says
// under someone's name, times, a slider's lit part, and the menu a right-click on someone opens.

/** The Taken sigil (a tall diamond with a slit of light), in currentColor. */
export const sigil = (size = 22) => `<svg class="ft-sigil" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true"><path d="M12 2.5 18.5 12 12 21.5 5.5 12Z"/><path d="M12 7c1.5 2.7 1.5 7.3 0 10-1.5-2.7-1.5-7.3 0-10Z" fill="currentColor"/><path d="M1.5 12h3.6M18.9 12h3.6"/></svg>`;

/** Lit while they're talking (and you can hear them). */
export const talking = m => !!m.speaking && !m.muted;

/** A card's state: its class, what it says under the name and its icon. */
export function state(m, isMe) {
  if (m.muted) return ['muted', 'Muted for you', 'mute'];
  if (talking(m)) return ['talk', 'Speaking', ''];
  if (m.deaf) return ['deaf', 'Deafened', 'headoff'];
  if (m.mute) return ['off', 'Mic off', 'micoff'];
  return isMe ? ['live', 'Live', ''] : ['', 'Listening', ''];
}

export function stateHtml(ui, m, isMe) {
  const [, text, icon] = state(m, isMe);
  return `${icon ? ui.icon(icon) : ''}${ui.esc(text)}`;
}

/** "now", "4 min", "2 h", "yesterday". */
export function ago(ms) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86400) return `${Math.floor(s / 3600)} h`;
  return 'yesterday';
}

/** How long since `since` (Unix ms), as 1:24:07 or 4:07. */
export function clock(since) {
  const s = Math.max(0, Math.floor((Date.now() - since) / 1000));
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** A range input's lit part, as --p. */
export function fill(r) {
  const min = +r.min || 0, max = +r.max || 100;
  r.style.setProperty('--p', `${((+r.value - min) / (max - min)) * 100}%`);
}

/** Two letters for a server without an icon ("Signals class" → SC). */
export const initials = name => (String(name).trim().split(/\s+/).slice(0, 2).map(w => [...w][0] || '').join('') || '?').toUpperCase();

export function closeMenu(host) {
  const el = host && host.querySelector(':scope > .ft-menu');
  if (el) { el.off(); el.remove(); }
}

/** A small menu at the pointer, kept inside `host`; it closes on Escape or a click elsewhere. */
export function openMenu(host, x, y, html, wire) {
  closeMenu(host);
  const el = document.createElement('div');
  el.className = 'ft-menu';
  el.setAttribute('role', 'menu');
  el.innerHTML = html;
  host.append(el);
  const r = host.getBoundingClientRect();
  el.style.left = `${Math.max(6, Math.min(x - r.left, r.width - el.offsetWidth - 6))}px`;
  el.style.top = `${Math.max(6, Math.min(y - r.top, r.height - el.offsetHeight - 6))}px`;
  const away = e => { if (!e.composedPath().includes(el)) closeMenu(host); };
  const key = e => { if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); closeMenu(host); } };
  const t = setTimeout(() => document.addEventListener('pointerdown', away, true), 0);
  document.addEventListener('keydown', key, true);
  el.off = () => { clearTimeout(t); document.removeEventListener('pointerdown', away, true); document.removeEventListener('keydown', key, true); };
  wire(el, () => closeMenu(host));
  // The menu holds the focus (Escape closes it) without lighting up its first control.
  el.tabIndex = -1;
  el.focus({ preventScroll: true });
  return el;
}

/** The right-click menu on someone in the call: their volume and mute for you, their profile, their name. */
export function personMenu(core, host, e, m, after) {
  const { esc, icon } = core.ctx.ui;
  const d = core.status(), v = d && d.voice, isMe = m.id === core.me();
  const vol = Math.round(m.volume);
  e.preventDefault();
  openMenu(host, e.clientX, e.clientY, `
    <div class="ft-mh"><img class="ft-av" src="${esc(core.avatarOf(m))}" alt="" referrerpolicy="no-referrer" draggable="false"><span><b>${esc(m.name)}</b><small>${esc(state(m, isMe)[1])}${v ? ` · in ${esc(v.name)}` : ''}</small></span></div>
    ${isMe ? '' : `<label class="ft-mvol"><span>Volume for me<output>${vol}%</output></span><input class="ft-range" type="range" min="0" max="200" value="${vol}" aria-label="${esc(m.name)}'s volume for you"></label>
    <label class="ft-mi" role="menuitemcheckbox" aria-checked="${!!m.muted}"><span>Mute for me</span><span class="switch"><input type="checkbox"${m.muted ? ' checked' : ''}></span></label>`}
    <button class="ft-mi" type="button" role="menuitem" data-m="profile"><span>Open profile in Discord</span>${icon('link')}</button>
    <button class="ft-mi" type="button" role="menuitem" data-m="copy"><span>Copy name</span>${icon('copy')}</button>`, (el, close) => {
    const r = el.querySelector('input[type=range]');
    if (r) {
      fill(r);
      r.addEventListener('input', () => { el.querySelector('output').textContent = `${r.value}%`; fill(r); core.userVoice(m.id, { volume: +r.value }); });
    }
    const box = el.querySelector('input[type=checkbox]');
    if (box) box.addEventListener('change', () => { box.closest('[role]').setAttribute('aria-checked', box.checked); core.userVoice(m.id, { mute: box.checked }); });
    el.addEventListener('click', ev => {
      const b = ev.target.closest('[data-m]'); if (!b) return;
      if (b.dataset.m === 'profile') core.open({ user: m.id }).then(ok => ok && after && after());
      else navigator.clipboard.writeText(m.name).catch(() => {});
      close();
    });
  });
}
