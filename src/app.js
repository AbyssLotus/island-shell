import { WALLS, drawWall } from './engine.js';
import { createCommandCenter } from './cc.js';
import { createSounds } from './sounds.js';
import { createRegistry } from './registry.js';
import { createModuleHost } from './modhost.js';

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const assetUrl = p => TAURI.core.convertFileSrc(p);
const listen = TAURI.event.listen;
const appWin = TAURI.window.getCurrentWindow();

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const store = {
  get(k, d) { try { const v = localStorage.getItem('island.' + k); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
  set(k, v) { try { localStorage.setItem('island.' + k, JSON.stringify(v)); } catch (e) {} }
};
// The Taken theme (2026-10): Taken light is the accent unless you choose to follow the wallpaper.
// Earlier versions followed the wallpaper by default, so that's switched off once.
// A brand-new install has nothing stored yet: it gets the setup guide (an existing one is marked done).
const FIRST_RUN = store.get('setupDone', null) === null && store.get('themeV', 0) === 0;
if (!FIRST_RUN && store.get('setupDone', null) === null) store.set('setupDone', true);
if (store.get('themeV', 0) < 1) { store.set('themeV', 1); store.set('follow', false); store.set('accent', '#C9E9FF'); }
const TAKEN = '#C9E9FF';
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
const quiet = p => p.catch(() => {});
const log = msg => quiet(invoke('log', { msg: String(msg).slice(0, 2000) }));
addEventListener('error', e => log(`error: ${e.message} at ${e.filename}:${e.lineno}`));
addEventListener('unhandledrejection', e => log(`unhandled: ${e.reason && (e.reason.stack || e.reason)}`));

/* ---------- icons ---------- */
const IC = {
  wifi: '<path d="M5 12.55a11 11 0 0 1 14.08 0"/><path d="M1.42 9a16 16 0 0 1 21.16 0"/><path d="M8.53 16.11a6 6 0 0 1 6.95 0"/><path d="M12 20h.01"/>',
  wifioff: '<path d="M12 20h.01"/><path d="M8.5 16.43a5 5 0 0 1 7 0"/><path d="M2 8.82a15 15 0 0 1 4.18-2.65"/><path d="M19 12.86a10 10 0 0 0-2.48-1.63"/><path d="M22 8.82a15 15 0 0 0-11.29-3.76"/><path d="M5 12.86a10 10 0 0 1 5.17-2.7"/><path d="m2 2 20 20"/>',
  ethernet: '<rect x="4" y="4" width="16" height="12" rx="2"/><path d="M8 20h8M12 16v4M8 8v3M12 8v3M16 8v3"/>',
  bt: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  speaker: '<path d="M11 5 6 9H2v6h4l5 4V5z"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/>',
  mute: '<path d="M11 5 6 9H2v6h4l5 4V5z"/><path d="m22 9-6 6M16 9l6 6"/>',
  headphones: '<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>',
  monitor: '<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>',
  moon: '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>',
  contrast: '<circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 1 0 18z" fill="currentColor" stroke="none"/>',
  taskbar: '<rect x="2" y="4" width="20" height="16" rx="2.5"/><path d="M2 15.5h20M7 18h.01M11 18h2"/>',
  play: '<path d="M8 5.5v13a1 1 0 0 0 1.52.85l10.4-6.5a1 1 0 0 0 0-1.7L9.52 4.65A1 1 0 0 0 8 5.5z" fill="currentColor" stroke="none"/>',
  pause: '<rect x="6" y="5" width="4" height="14" rx="1.3" fill="currentColor" stroke="none"/><rect x="14" y="5" width="4" height="14" rx="1.3" fill="currentColor" stroke="none"/>',
  prev: '<path d="M18 6.2v11.6a1 1 0 0 1-1.55.83L8.6 13.2a1.4 1.4 0 0 1 0-2.4l7.85-5.43A1 1 0 0 1 18 6.2z" fill="currentColor" stroke="none"/><rect x="5" y="5" width="2.6" height="14" rx="1.1" fill="currentColor" stroke="none"/>',
  next: '<path d="M6 6.2v11.6a1 1 0 0 0 1.55.83l7.85-5.43a1.4 1.4 0 0 0 0-2.4L7.55 5.37A1 1 0 0 0 6 6.2z" fill="currentColor" stroke="none"/><rect x="16.4" y="5" width="2.6" height="14" rx="1.1" fill="currentColor" stroke="none"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  pin: '<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  copy: '<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21"/>',
  text: '<path d="M4 6h16M4 12h11M4 18h14"/>',
  drop: '<path d="M12 2.7s6 6.3 6 11.3a6 6 0 0 1-12 0c0-5 6-11.3 6-11.3z"/>',
  lock: '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
  sleep: '<path d="M4 5h6l-6 7h6"/><path d="M13 12h7l-7 8h7"/>',
  restart: '<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>',
  power: '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  download: '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  cloudsun: '<path d="M12 2v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="M20 12h2"/><path d="m19.07 4.93-1.41 1.41"/><path d="M15.95 12.65a4 4 0 0 0-5.93-4.13"/><path d="M13 22H7a5 5 0 1 1 4.9-6H13a3 3 0 0 1 0 6Z"/>',
  cloud: '<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"/>',
  rain: '<path d="M4 14.9A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.24"/><path d="M16 14v6M8 14v6M12 16v6"/>',
  snow: '<path d="M4 14.9A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.24"/><path d="M8 15h.01M8 19h.01M12 17h.01M12 21h.01M16 15h.01M16 19h.01"/>',
  storm: '<path d="M6 16.33A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 .5 8.97"/><path d="m13 12-3 5h4l-3 5"/>',
  fog: '<path d="M4 14.9A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.24"/><path d="M16 17H7M17 21H9"/>',
  down: '<path d="M12 5v14M19 12l-7 7-7-7"/>',
  drive: '<path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M6 16h.01M10 16h.01"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  timer: '<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2 2M9 2h6"/>',
  film: '<rect x="2" y="3" width="20" height="18" rx="2"/><path d="M7 3v18M17 3v18M2 9h5M2 15h5M17 9h5M17 15h5"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  layers: '<path d="m12 2 9 5-9 5-9-5 9-5z"/><path d="m3 12 9 5 9-5M3 17l9 5 9-5"/>',
  mic: '<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M19 10v1a7 7 0 0 1-14 0v-1M12 18v4"/>',
  micoff: '<path d="m2 2 20 20"/><path d="M18.89 13.23A7 7 0 0 0 19 12v-2M5 10v2a7 7 0 0 0 12 5"/><path d="M15 9.34V5a3 3 0 0 0-5.68-1.33M9 9v3a3 3 0 0 0 5.12 2.12M12 19v3"/>',
  gamepad: '<path d="M6 11h4M8 9v4"/><path d="M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.98 3.59l-.9 8.12A2.5 2.5 0 0 0 4.28 19.5c.9 0 1.73-.48 2.17-1.27L7.5 16.5h9l1.05 1.73a2.5 2.5 0 0 0 4.65-1.52l-.9-8.12A4 4 0 0 0 17.32 5z"/>',
  eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  dots9: '<g fill="currentColor" stroke="none"><circle cx="5" cy="5" r="2.2" opacity=".35"/><circle cx="12" cy="5" r="2.2" opacity=".35"/><circle cx="19" cy="5" r="2.2" opacity=".35"/><circle cx="5" cy="12" r="2.2"/><circle cx="12" cy="12" r="2.2"/><circle cx="19" cy="12" r="2.2"/><circle cx="5" cy="19" r="2.2" opacity=".35"/><circle cx="12" cy="19" r="2.2"/><circle cx="19" cy="19" r="2.2" opacity=".35"/></g>',
  nodes: '<circle cx="12" cy="5" r="2.6"/><circle cx="5" cy="18" r="2.6"/><circle cx="19" cy="18" r="2.6"/><path d="m10.7 7.3-4.4 8.4M13.3 7.3l4.4 8.4M7.6 18h8.8"/>',
  shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/>',
  repeat: '<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/>',
  star: '<path d="M11.52 2.3a.53.53 0 0 1 .96 0l2.31 4.68a2.12 2.12 0 0 0 1.6 1.16l5.16.76a.53.53 0 0 1 .3.9l-3.74 3.64a2.12 2.12 0 0 0-.61 1.88l.88 5.14a.53.53 0 0 1-.77.56l-4.62-2.43a2.12 2.12 0 0 0-1.97 0L6.4 21.01a.53.53 0 0 1-.77-.56l.88-5.14a2.12 2.12 0 0 0-.61-1.88L2.16 9.8a.53.53 0 0 1 .3-.9l5.16-.76a2.12 2.12 0 0 0 1.6-1.16z"/>',
  shuffle: '<path d="M2 18h1.4c1.3 0 2.5-.6 3.3-1.7l6.1-8.6c.7-1.1 2-1.7 3.3-1.7H22"/><path d="m18 2 4 4-4 4"/><path d="M2 6h1.9c1.5 0 2.9.9 3.6 2.2"/><path d="M22 18h-5.9c-1.3 0-2.6-.7-3.3-1.8l-.5-.8"/><path d="m18 14 4 4-4 4"/>',
  clip: '<rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
  folder: '<path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z"/>',
  gear: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  bellon: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  cpu: '<rect x="5" y="5" width="14" height="14" rx="2"/><path d="M9 9h6v6H9zM9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>',
  back: '<path d="m15 18-6-6 6-6"/>',
  orbit: '<circle cx="12" cy="12" r="4.5"/><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(-24 12 12)"/>',
  cap: '<path d="M21.42 10.92a1 1 0 0 0-.02-1.84L12.83 5.18a2 2 0 0 0-1.66 0L2.6 9.08a1 1 0 0 0 0 1.83l8.57 3.91a2 2 0 0 0 1.66 0z"/><path d="M22 10v6"/><path d="M6 12.5V16a6 3 0 0 0 12 0v-3.5"/>',
  pulse: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
  note: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  calendar: '<rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
  grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/>',
  chat: '<path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/><path d="M8 12h.01M12 12h.01M16 12h.01"/>',
  phoneoff: '<path d="M10.68 13.31a16 16 0 0 0 3.41 2.6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7 2 2 0 0 1 1.72 2v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.42 19.42 0 0 1-3.33-2.67m-2.67-3.34a19.79 19.79 0 0 1-3.07-8.63A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91"/><path d="m22 2-20 20"/>',
  headoff: '<path d="M21 14h-1.34"/><path d="M9.13 3.47A9 9 0 0 1 21 12v3.34"/><path d="m2 2 20 20"/><path d="M20.41 20.41A2 2 0 0 1 19 21h-1a2 2 0 0 1-2-2v-3"/><path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 2.64-6.36"/>',
  rocket: '<path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/>'
};
const icon = n => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${IC[n] || ''}</svg>`;
$$('i[data-ic]').forEach(el => { el.outerHTML = icon(el.dataset.ic); });

/* ---------- tabs (src/registry.js) ---------- */
const R = createRegistry();
// The base's own tabs, [id, title, icon, panel width], in strip order (order 10, 20, …). Modules add
// theirs from their manifests (src/modhost.js).
[['controls', 'Controls', 'gear', 600], ['music', 'Music', 'note', 700], ['audio', 'Audio', 'speaker', 680], ['today', 'Today', 'calendar', 760],
  ['canvas', 'Canvas', 'cap', 760], ['network', 'Network', 'wifi', 680], ['discord', 'Discord', 'chat', 700], ['games', 'Games', 'gamepad', 760],
  ['destiny', 'Destiny', 'orbit', 760], ['stats', 'Stats', 'pulse', 720], ['clipboard', 'Clipboard', 'clip', 760], ['apps', 'Apps', 'grid', 640],
  ['wallpaper', 'Wallpaper', 'image', 660], ['power', 'Power', 'power', 600]]
  .forEach(([id, title, ic, width], i) => R.tabs.add({ id, title, icon: ic, width, order: (i + 1) * 10 }));
/** Tab ids in strip order, kept current as modules come and go. */
let TABS = R.tabs.ids();
const tabW = t => (R.tabs.get(t) || {}).width || 700;

/* ---------- state ---------- */
const TOP = 8;
let BAR_H = 40; // 48 for the tall bar
/** Either top-bar layout: the regular bar or the taller one. */
function isBar() { return S.style === 'bar' || S.style === 'tall'; }
const GRADS = [['#F2A65A', '#7A3A6A'], ['#8CC8FF', '#2D4058'], ['#FF9DB4', '#5E2638'], ['#7FE0C2', '#0B1F2E'], ['#B9A4FF', '#28204F'], ['#FFD27A', '#8A4B12']];
const hashStr = s => { let h = 2166136261; for (const c of s) { h ^= c.codePointAt(0); h = Math.imul(h, 16777619); } return h >>> 0; };
const gradFor = s => { const g = GRADS[hashStr(s) % GRADS.length]; return `linear-gradient(135deg,${g[0]},${g[1]})`; };

const S = {
  mode: 'boot', tab: TABS.includes(store.get('tab', '')) ? store.get('tab', '') : 'controls', hover: false,
  status: null, media: null, mediaKey: '', thumb: null, thumbTry: 0, mediaHold: null, seek: null, scrubbing: false, scrubVal: 0, appVolAt: 0, tickRaf: 0,
  lyricsOn: store.get('lyrics', true), ly: { key: '', state: 'idle', data: null, idx: -1 }, lyWant: '', lyWantAt: 0, lyFollowPause: 0,
  audio: null, volSetAt: -1e9, // when the user last moved a volume slider (nothing yet)
  dark: store.get('dark', true),
  wall: store.get('wall', -1), wallSel: 0, follow: store.get('follow', false), applying: false,
  timer: store.get('timer', { min: 25, left: 1500, running: false, end: 0 }),
  todos: store.get('todos', [
    { id: 1, t: 'Try every tab on the island', d: false },
    { id: 2, t: 'Pick a wallpaper', d: false }
  ]),
  clips: [], clipFilter: 'all', clipQ: '', clipSel: null,
  apps: [], stats: null,
  city: store.get('city', 'Philadelphia'), unit: store.get('unit', 'F')
};

const sounds = createSounds({ store, isFullscreen: () => !!S.fullscreen });
S.wallSel = S.wall >= 0 && S.wall < WALLS.length ? S.wall : 0;
if (S.timer.running && S.timer.end <= Date.now()) { S.timer.running = false; S.timer.left = S.timer.min * 60; }

function setAccent(hex) { document.documentElement.style.setProperty('--accent', hex); store.set('accent', hex); }
/** Taken light, unless the accent follows the wallpaper (the light skin then uses black, Taken's inverse). */
function syncAccentMode() { document.documentElement.classList.toggle('accent-taken', !S.follow); if (!S.follow) setAccent(TAKEN); }

/* ---------- island ---------- */
const island = $('#island');
const L = { pill: $('#L-pill'), peek: $('#L-peek'), toast: $('#L-toast'), panel: $('#L-panel') };
const views = Object.fromEntries(TABS.map(t => [t, $(`.view[data-tab="${t}"]`)]));
/** The tab strip, built from the registry (an icon name, or a module's own .svg). */
function tabIcon(t) { return /\.svg$/.test(t.icon) ? `<img class="i" src="${esc(t.icon)}" alt="">` : icon(t.icon); }
function renderTabs() {
  const nav = $('.tabs'), ink = $('.tab-ink');
  nav.replaceChildren(ink);
  nav.insertAdjacentHTML('beforeend', R.tabs.all().map(t => `<button class="tab" type="button" role="tab" id="tab-${esc(t.id)}" data-tab="${esc(t.id)}" aria-controls="v-${esc(t.id)}">${tabIcon(t)}<span>${esc(t.title)}</span></button>`).join(''));
  TABS = R.tabs.ids(); tabLabelW = null;
}
const pillW = () => (S.timer.running ? 236 : 200) + callW();
const maxW = () => Math.max(240, innerWidth - 24);

/** Drops heavy, image-filled views a moment after they stop showing, so their decoded images
 * (the Games covers, some animated) don't sit in memory while the island rests. */
function releaseHidden() {
  clearTimeout(S.releaseT);
  S.releaseT = setTimeout(() => {
    if (!(S.mode === 'panel' && S.tab === 'games')) setHtml($('#gShelf'), '');
    if (!(S.mode === 'panel' && S.tab === 'wallpaper')) clearWeGrid();
  }, 900);
}
function setMode(m) {
  S.mode = m; island.dataset.mode = m;
  if (m !== 'toast') S.toastTop = false;
  if (m !== 'toast' && S.urgent) { S.urgent = false; island.classList.toggle('fs-hidden', !!S.fullscreen && m !== 'panel'); }
  if (m !== 'panel') releaseHidden();
  syncVmLevels();
  for (const [k, el] of Object.entries(L)) el.classList.toggle('on', k === m);
  L.pill.setAttribute('aria-expanded', m === 'panel');
  layout(); mediaTick(); syncModViews();
}
let lastRect = '';
function layout() {
  const mw = maxW(); let w = 36, h = 36, r = 18;
  L.pill.style.width = pillW() + 'px';
  L.peek.style.width = Math.min(400, mw) + 'px';
  for (const t of TABS) if (views[t]) views[t].style.width = Math.min(tabW(t), mw) + 'px';
  if (S.mode === 'pill') { w = pillW(); h = 38; r = 19; }
  else if (S.mode === 'peek') { w = Math.min(400, mw); h = 84; r = 30; }
  else if (S.mode === 'toast') { w = Math.min($('#toast').offsetWidth, mw); h = 52; r = 26; }
  else if (S.mode === 'panel') { w = Math.min(tabW(S.tab), mw); h = 54 + views[S.tab].offsetHeight; r = 30; }
  island.style.width = w + 'px'; island.style.height = h + 'px'; island.style.borderRadius = r + 'px';
  // Bar style: the island is only the dropdown under the chip that opened it (or a notification).
  const bar = isBar();
  let cx = innerWidth / 2;
  if (bar) { cx = clamp(S.anchor || innerWidth / 2, w / 2 + 8, innerWidth - w / 2 - 8); island.style.left = cx + 'px'; }
  else island.style.left = '';
  const visible = !bar || S.mode === 'panel' || S.mode === 'toast';
  island.classList.toggle('bar-hidden', !visible);
  // Raised: the panel or a notification shows above apps. Otherwise the island rests on the desktop.
  const expanded = S.mode === 'panel', raised = expanded || S.mode === 'toast';
  // An important notification (Discord @mention) shows above everything, fullscreen games included.
  // pinned above all windows: urgent ones, and those asking to be on top (opts.top)
  const urgent = !!S.toastTop && S.mode === 'toast';
  const key = `${Math.round(cx)},${w},${h},${expanded},${raised},${bar},${visible},${urgent}`;
  if (key !== lastRect && !S.ccOpen) {
    lastRect = key;
    quiet(invoke('island_rect', { x: cx - w / 2, y: bar ? BAR_H + 8 : TOP, w: visible ? w : 0, h: visible ? h : 0, expanded, raised, bar: bar ? BAR_H : 0, urgent }));
  }
}
/* Tab strip. The panel takes each tab's own width, so there isn't always room for every label:
   the open tab always shows its name, its nearest neighbours keep theirs while they fit, and the
   rest shrink to icons (named on hover). Widths are worked out here rather than measured after the
   fact, so the strip, the highlight and the island all move together on the same spring. */
const TAB_MIN = 9.5 + 15 + 9.5, TAB_LABEL_GAP = 6, TABS_GAP = 2, TABS_PAD = 3;
let tabLabelW = null; // each label's natural width (re-measured once the fonts are in)
renderTabs();
function tabWidths(active) {
  if (!tabLabelW) tabLabelW = Object.fromEntries(TABS.map(t => [t, $(`#tab-${t} span`).offsetWidth]));
  const room = Math.min(tabW(active), maxW()) - 20 - TABS_PAD * 2 - TABS_GAP * (TABS.length - 1);
  const w = Object.fromEntries(TABS.map(t => [t, TAB_MIN]));
  let used = TAB_MIN * TABS.length;
  const i0 = TABS.indexOf(active), order = [i0];
  for (let d = 1; d < TABS.length; d++) { if (i0 + d < TABS.length) order.push(i0 + d); if (i0 - d >= 0) order.push(i0 - d); }
  for (const i of order) {
    const extra = TAB_LABEL_GAP + tabLabelW[TABS[i]];
    if (i !== i0 && used + extra > room) break; // stop at the first that doesn't fit, so the labels stay together
    w[TABS[i]] += extra; used += extra;
  }
  return w;
}
function moveInk() {
  const w = tabWidths(S.tab);
  let x = TABS_PAD, at = 0;
  for (const t of TABS) {
    const b = $(`#tab-${t}`);
    b.style.width = w[t] + 'px';
    b.classList.toggle('short', w[t] === TAB_MIN);
    if (t === S.tab) at = x;
    x += w[t] + TABS_GAP;
  }
  const ink = $('.tab-ink'), nav = $('.tabs'), total = x - TABS_GAP + TABS_PAD, fits = total <= Math.min(tabW(S.tab), maxW()) - 20;
  ink.style.width = w[S.tab] + 'px'; ink.style.transform = `translateX(${at}px)`;
  nav.classList.toggle('over', !fits);
  if (!fits) nav.scrollTo({ left: at - nav.clientWidth / 2 + w[S.tab] / 2, behavior: reduced ? 'auto' : 'smooth' });
  tabTip.classList.remove('on');
}
const tabTip = Object.assign(document.createElement('div'), { className: 'tab-tip', ariaHidden: 'true' });
$('#L-panel').append(tabTip);
$('.tabs').addEventListener('pointerover', e => {
  const b = e.target.closest('.tab');
  if (!b || !b.classList.contains('short')) { tabTip.classList.remove('on'); return; }
  txt(tabTip, b.textContent.trim());
  const r = b.getBoundingClientRect(), p = $('#L-panel').getBoundingClientRect();
  tabTip.style.left = (r.left + r.width / 2 - p.left) + 'px';
  tabTip.classList.add('on');
});
$('.tabs').addEventListener('pointerleave', () => tabTip.classList.remove('on'));
function syncTabs() { $$('.tab').forEach(b => { const on = b.dataset.tab === S.tab; b.setAttribute('aria-selected', on); b.tabIndex = on ? 0 : -1; }); }
function switchView(t, instant) {
  const from = S.tab; if (t === from) return;
  const dir = Math.sign(TABS.indexOf(t) - TABS.indexOf(from)) || 1, ov = views[from], nv = views[t];
  ov.style.setProperty('--dx', instant ? '0px' : `${-28 * dir}px`); ov.classList.remove('on');
  if (!instant) { nv.style.transition = 'none'; nv.style.setProperty('--dx', `${28 * dir}px`); void nv.offsetWidth; nv.style.transition = ''; }
  nv.style.setProperty('--dx', '0px'); nv.classList.add('on');
  S.tab = t; store.set('tab', t); syncTabs();
  if (from === 'games' || from === 'wallpaper') releaseHidden();
  syncVmLevels(); syncModViews();
}
function setTab(t) {
  if (S.mode !== 'panel') { openPanel(t); return; }
  if (t === S.tab) return;
  switchView(t, false); layout(); moveInk(); onTabShown(t);
}
function onTabShown(t) {
  if (t === 'music' || t === 'controls') { S.ly.idx = -2; syncLyric(curPos(), true); mediaTick(); }
  if (t === 'stats') statsLoop();
  if (t === 'network') { renderVpn(); vpnLoop(); }
  if (t === 'discord') loadDc();
  if (t === 'destiny') d2Loop();
  if (t === 'canvas') cvLoop();
  if (t === 'games') gamesLoop();
  if (t === 'wallpaper') showWallpapers();
  if (t === 'audio') { renderAudioHead(); if (audioShowing()) { audioDevLoop(); audioAppsLoop(); } syncVmLevels(); }
  if (t === 'clipboard') renderClip();
  if (t === 'today') { renderWeek(); tickClock(); }
  if (t === 'apps') {
    winsLoop(); loadTray();
    setTimeout(() => { if (S.tab === 'apps' && S.mode === 'panel' && !S.openFolder) $('#appSearch').focus({ preventScroll: true }); }, 260);
  }
}
/** In bar style, where a panel opened by hotkey drops down from. */
function anchorFor(tab) {
  const sel = { apps: '#barApps', controls: '#barGear', audio: '#barVol', music: '#barMedia', network: '#barNet', discord: '#barDc', games: '#barGames', today: '#barClock', stats: '#barCpu', power: '#barPower' }[tab];
  const el = sel && $(sel);
  if (!el || el.hidden) return innerWidth / 2;
  const r = el.getBoundingClientRect();
  return r.left + r.width / 2;
}
function openPanel(tab, viaKey, fromBar) {
  clearTimeout(toastT);
  island.classList.remove('fs-hidden');
  if (isBar() && !fromBar) S.anchor = anchorFor(tab || S.tab);
  if (tab && tab !== S.tab) switchView(tab, true);
  setMode('panel'); moveInk(); onTabShown(S.tab);
  quiet(appWin.setFocus());
  if (viaKey) setTimeout(() => $(`#tab-${S.tab}`).focus({ preventScroll: true }), 40);
}
/** keepFocus: the panel is closing because the user already went somewhere else. */
function closePanel(keepFocus) {
  if (S.mode !== 'panel') return;
  hideCtx();
  if (document.activeElement && island.contains(document.activeElement)) document.activeElement.blur();
  setMode(S.hover ? 'peek' : 'pill');
  if (S.fullscreen) island.classList.add('fs-hidden');
  if (!keepFocus && document.hasFocus()) quiet(invoke('release_focus'));
}
/** Open, close, or (when a shortcut asks for another tab while it's open) switch tabs. */
function togglePanel(viaKey, tab) {
  if (S.mode === 'panel' && tab && tab !== S.tab) { if (isBar()) S.anchor = anchorFor(tab); setTab(tab); layout(); return; }
  if (S.mode === 'panel') closePanel(); else openPanel(tab || null, viaKey && tab !== 'apps');
}

/* hover peek, clicks, wheel */
let hoverT;
function onEnter() {
  if (isBar()) return;
  S.hover = true; clearTimeout(hoverT);
  if (S.mode === 'pill') hoverT = setTimeout(() => { if (S.mode === 'pill' && S.hover) setMode('peek'); }, 140);
}
function onLeave() {
  S.hover = false; clearTimeout(hoverT);
  if (S.mode === 'peek') hoverT = setTimeout(() => { if (S.mode === 'peek' && !S.hover && !S.gaze) setMode('pill'); }, 220);
}
island.addEventListener('pointerenter', e => { if (e.pointerType === 'mouse') onEnter(); });
island.addEventListener('pointerleave', onLeave);
L.pill.addEventListener('click', () => openPanel());
L.pill.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openPanel(null, true); } });
L.peek.addEventListener('click', e => { if (!e.target.closest('[data-act]')) openPanel(S.media ? 'music' : null); });
L.toast.addEventListener('click', () => openPanel(S.toastTab || undefined));

let wheelAcc = 0, wheelX = 0, wheelT, wheelLock = 0;
function scrollableAncestor(el) {
  while (el && el !== island) {
    if (el.scrollHeight > el.clientHeight + 2) { const oy = getComputedStyle(el).overflowY; if (oy === 'auto' || oy === 'scroll') return el; }
    el = el.parentElement;
  }
  return null;
}
island.addEventListener('wheel', e => {
  const now = performance.now();
  if (S.mode !== 'panel') {
    e.preventDefault();
    if (e.deltaY > 4 && now > wheelLock) { openPanel(); wheelLock = now + 500; }
    return;
  }
  if (now < wheelLock) return;
  clearTimeout(wheelT); wheelT = setTimeout(() => { wheelAcc = 0; wheelX = 0; }, 260);
  if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
    wheelX += e.deltaX;
    if (Math.abs(wheelX) > 70) { const i = TABS.indexOf(S.tab) + Math.sign(wheelX); if (i >= 0 && i < TABS.length) setTab(TABS[i]); wheelX = 0; wheelLock = now + 380; }
    return;
  }
  if (e.deltaY < 0) {
    const sc = scrollableAncestor(e.target);
    if (sc && sc.scrollTop > 0) return;
    wheelAcc += e.deltaY;
    if (wheelAcc < -90) { wheelAcc = 0; closePanel(); wheelLock = now + 500; }
  } else wheelAcc = 0;
}, { passive: false });

document.addEventListener('pointerdown', e => {
  const inMenu = e.target.closest && e.target.closest('#ctx');
  if (!inMenu) hideCtx();
  const inBar = e.target.closest && e.target.closest('#bar');
  const inCc = e.target.closest && e.target.closest('#cc');
  if (S.mode === 'panel' && !island.contains(e.target) && !inMenu && !inBar && !inCc) closePanel();
});
$('.tabs').addEventListener('click', e => { const b = e.target.closest('.tab'); if (b) setTab(b.dataset.tab); });

/* toast */
let toastT;
function toast(title, sub, ic, opts = {}) {
  // opts.sound: which notification sound (sounds.js); it plays even while the panel hides the toast.
  if (opts.sound) sounds.play(opts.sound);
  if (S.mode === 'panel') return;
  // opts.urgent: must be seen, so it also shows over a fullscreen app (otherwise the island stays faded).
  S.urgent = !!opts.urgent; S.toastTop = !!(opts.urgent || opts.top); S.toastTab = opts.tab || '';
  island.classList.toggle('fs-hidden', !!S.fullscreen && !S.urgent);
  const t = $('#tIco');
  t.className = 't-ico' + (opts.img || opts.glyph ? ' app' : '') + (opts.round ? ' av' : '');
  t.style.background = opts.bg || '';
  t.innerHTML = opts.img ? `<img src="${esc(opts.img)}" alt="">` : opts.glyph ? esc(opts.glyph) : icon(ic || 'check');
  $('#tTitle').textContent = title; $('#tBody').textContent = sub || '';
  if (isBar()) S.anchor = innerWidth - 200; // notifications drop in at the top right
  setMode('toast'); clearTimeout(toastT);
  const ms = opts.ms || 2300;
  const hide = () => { S.toastWait = null; if (S.mode === 'toast') setMode(!isBar() && (S.hover || S.gaze) ? 'peek' : 'pill'); };
  // With the eye tracker on and someone at the screen, a notification stays until it's been looked at.
  if (gazeLive() && !S.gaze) {
    S.toastWait = () => { S.toastWait = null; clearTimeout(toastT); toastT = setTimeout(hide, ms); };
    toastT = setTimeout(hide, 20000);
  } else toastT = setTimeout(hide, ms);
}

/* ---------- status: media, audio, radios ---------- */
const fmtT = s => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
async function pollStatus() {
  try { applyStatus(await invoke('get_status')); } catch (e) {}
  setTimeout(pollStatus, 1000);
}
/** What the user just changed wins over the next status polls (players report changes late). */
function holdMedia(fields, ms = 1800) {
  S.mediaHold = { ...fields, until: performance.now() + ms };
  if (S.media) Object.assign(S.media, fields);
}
function applyStatus(st) {
  S.status = st;
  const m = st.media, hold = S.mediaHold;
  if (m && hold && performance.now() < hold.until) for (const k in hold) if (k !== 'until') m[k] = hold[k];
  const changed = !!m !== !!S.media || !!(m && m.key !== S.mediaKey);
  S.media = m;
  if (!m) { S.mediaKey = ''; setThumb(null); setTint(null); }
  else if (m.key !== S.mediaKey || (!S.thumb && Date.now() - S.thumbTry > 5000)) {
    if (m.key !== S.mediaKey) { setThumb(null); S.seek = null; }
    S.mediaKey = m.key; S.thumbTry = Date.now();
    const key = m.key;
    // The artwork arrives as raw bytes and lives as one blob URL (no base64 copies in the page).
    invoke('media_thumb').then(buf => {
      if (S.mediaKey !== key || !buf || !buf.byteLength) return;
      setThumb(URL.createObjectURL(new Blob([buf]))); renderMedia(); tintFrom(S.thumb);
    }).catch(() => {});
  }
  if (st.audio && performance.now() - S.volSetAt > 1500) S.audio = st.audio;
  maybeLyrics(m);
  renderMedia(changed); renderControls(); renderBar(); mediaTick();
}
function setThumb(url) { if (S.thumb) URL.revokeObjectURL(S.thumb); S.thumb = url; }
const artEls = $$('[data-m="art"]'), titleEls = $$('[data-m="title"]'), artistEls = $$('[data-m="artist"]'), sourceEls = $$('[data-m="source"]'), durEls = $$('[data-m="dur"]'), toggleBtns = $$('[data-act="toggle"]');
function renderMedia(changed) {
  const m = S.media;
  island.classList.toggle('no-media', !m);
  island.classList.toggle('playing', !!(m && m.playing));
  const art = S.thumb ? `url("${S.thumb}") center / cover no-repeat` : m ? gradFor(m.title + m.artist) : 'var(--ink-3)';
  if (art !== S.artNow) { S.artNow = art; artEls.forEach(e => { e.style.setProperty('--art', art); e.classList.toggle('has-thumb', !!S.thumb); }); }
  const now = new Date();
  titleEls.forEach(e => txt(e, m ? m.title || 'Unknown title' : 'Nothing playing'));
  artistEls.forEach(e => txt(e, m ? m.artist || m.app : now.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' })));
  sourceEls.forEach(e => txt(e, m ? `${m.app}${S.audio ? ' · ' + S.audio.device : ''}` : (S.audio ? S.audio.device : 'Sound')));
  durEls.forEach(e => txt(e, fmtT(m && m.duration)));
  toggleBtns.forEach(b => { const p = !!(m && m.playing); setHtml(b, icon(p ? 'pause' : 'play')); b.setAttribute('aria-label', p ? 'Pause' : 'Play'); b.disabled = !m; });
  $$('[data-act="next"]').forEach(b => { b.disabled = !(m && m.can_next); });
  $$('[data-act="prev"]').forEach(b => { b.disabled = !(m && m.can_prev); });
  renderMusic(changed);
  S.lastSec = -1; paintProgress();
}

/* ---------- music tab ---------- */
/** Where the song is right now, carried forward from the last measurement. */
function curPos() {
  const m = S.media; if (!m) return 0;
  const now = performance.now();
  if (S.seek && now < S.seek.until) return S.seek.pos + (m.playing ? (now - S.seek.at) / 1000 : 0);
  const p = m.position + (m.playing ? Math.max(0, Date.now() - m.sampled_at) / 1000 : 0);
  return m.duration > 0 ? Math.min(p, m.duration) : p;
}
const mediaShowing = () => S.mode === 'peek' || (S.mode === 'panel' && (S.tab === 'music' || S.tab === 'controls'));
/** Runs every frame while a progress bar or the lyrics are on screen and the song is moving. */
function mediaTick() {
  if (S.tickRaf) return;
  const step = () => {
    S.tickRaf = 0;
    if (!S.media || !mediaShowing()) return;
    paintProgress();
    if (S.media.playing || S.scrubbing) S.tickRaf = requestAnimationFrame(step);
  };
  S.tickRaf = requestAnimationFrame(step);
}
const progBars = $$('[data-m="bar"]'), posTexts = $$('[data-m="pos"]'), seekEl = $('#muSeek');
function paintProgress() {
  const m = S.media, dur = m ? m.duration : 0;
  const pos = S.scrubbing ? S.scrubVal : curPos();
  const pct = dur > 0 ? clamp(pos / dur * 100, 0, 100) : 0;
  for (const e of progBars) e.style.width = pct.toFixed(2) + '%';
  if (!S.scrubbing) seekEl.value = Math.round(pct * 10);
  seekEl.style.setProperty('--p', pct.toFixed(2) + '%');
  const sec = Math.floor(pos);
  if (sec !== S.lastSec) {
    S.lastSec = sec;
    for (const e of posTexts) e.textContent = fmtT(pos);
    $('#muLeft').textContent = dur > 0 ? '-' + fmtT(dur - Math.floor(pos)) : '';
  }
  if (!S.scrubbing) syncLyric(pos);
}
function seekTo(sec) {
  const m = S.media; if (!m || !m.can_seek) return;
  sec = clamp(sec, 0, m.duration > 0 ? m.duration - 0.5 : sec);
  S.seek = { pos: sec, at: performance.now(), until: performance.now() + 2200 };
  S.ly.idx = -2; S.lyFollowPause = 0; S.lastSec = -1;
  paintProgress(); mediaTick();
  invoke('media_cmd', { action: 'seek', value: sec }).catch(err => { S.seek = null; flashNote(String(err)); });
}
function flashNote(msg) { note('#muNote', msg); clearTimeout(S.noteT); S.noteT = setTimeout(() => note('#muNote', ''), 4500); }
seekEl.addEventListener('input', () => {
  const m = S.media; if (!m || !(m.duration > 0)) return;
  S.scrubbing = true; S.scrubVal = seekEl.value / 1000 * m.duration; S.lastSec = -1;
  paintProgress(); mediaTick();
});
seekEl.addEventListener('change', () => {
  const m = S.media; S.scrubbing = false;
  if (m && m.duration > 0) seekTo(seekEl.value / 1000 * m.duration);
});
['pointerup', 'blur'].forEach(ev => seekEl.addEventListener(ev, () => { setTimeout(() => { S.scrubbing = false; }, 0); }));

function renderMusic(changed) {
  const m = S.media;
  txt($('#muApp'), m ? m.app : 'Nothing playing');
  const badge = $('#muBadge');
  badge.hidden = !(m && m.badge);
  if (m && m.badge) badge.textContent = m.badge;
  txt($('#muAlbum'), m && m.album ? m.album : '');
  if (!m) txt($('#v-music [data-m="artist"]'), 'Play something in Apple Music or Spotify');
  if (changed) { const t = $('.mu-title'); t.classList.remove('new'); void t.offsetWidth; t.classList.add('new'); }
  // Shuffle and repeat keep their place (hidden but still taking space) so play stays centered.
  const tg = (act, show, on, label, keepSpace) => $$(`#v-music [data-act="${act}"]`).forEach(b => {
    if (keepSpace) b.style.visibility = show ? '' : 'hidden'; else b.hidden = !show;
    b.setAttribute('aria-pressed', !!on); b.setAttribute('aria-label', label); b.title = label;
  });
  tg('shuffle', !!m && m.shuffle != null, m && m.shuffle, m && m.shuffle ? 'Shuffle is on' : 'Shuffle', true);
  const rep = m ? m.repeat : null;
  tg('repeat', rep != null, rep && rep !== 'off', rep === 'one' ? 'Repeating this song' : rep === 'all' ? 'Repeating all' : 'Repeat', true);
  setHtml($('#v-music [data-act="repeat"]'), icon('repeat') + (rep === 'one' ? '<b class="rep1">1</b>' : ''));
  tg('favorite', !!m && m.favorite != null, m && m.favorite, m && m.favorite ? 'Favorited' : 'Favorite', false);
  const vw = $('#muVolWrap'), vol = $('#muVol');
  vw.hidden = !(m && m.app_volume != null);
  if (m && m.app_volume != null && performance.now() - S.appVolAt > 1500) { const v = Math.round(m.app_volume * 100); vol.value = v; vol.style.setProperty('--p', v + '%'); }
  vw.title = m ? `${m.app} volume (just this app)` : '';
  const open = $('#muOpen'); open.hidden = !m; if (m) txt(open, `Open ${m.app}`);
  seekEl.disabled = !(m && m.can_seek && m.duration > 0);
  const ss = (m && m.sessions) || [];
  setHtml($('#muSources'), ss.length > 1 || (m && m.pinned)
    ? `<button type="button" class="chip" data-src="" aria-pressed="${!(m && m.pinned)}" title="Show whatever is playing">Auto</button>` +
      ss.map(x => `<button type="button" class="chip${x.playing ? ' live' : ''}" data-src="${esc(x.id)}" aria-pressed="${!!(m && m.pinned && m.id === x.id)}" title="${esc(x.title || x.app)}">${esc(x.app)}</button>`).join('')
    : '');
  $('#lyLines').classList.toggle('no-seek', !(m && m.can_seek));
}
$('#muSources').addEventListener('click', e => {
  const b = e.target.closest('[data-src]'); if (!b) return;
  quiet(invoke('media_source', { id: b.dataset.src }).then(() => setTimeout(async () => { try { applyStatus(await invoke('get_status')); } catch (err) {} }, 1100)));
});
$('#muVol').addEventListener('input', e => {
  const el = e.target; el.style.setProperty('--p', el.value + '%'); S.appVolAt = performance.now();
  clearTimeout(S.volT); S.volT = setTimeout(() => invoke('media_app_volume', { level: el.value / 100 }).catch(err => flashNote(String(err))), 40);
});
$('#v-controls .card.media').addEventListener('click', e => { if (!e.target.closest('[data-act]') && S.media) setTab('music'); });

document.addEventListener('click', e => {
  const b = e.target.closest('[data-act]'); if (!b || b.disabled) return;
  const action = b.dataset.act, m = S.media;
  if (m) {
    if (action === 'toggle') holdMedia({ playing: !m.playing }, 1500);
    if (action === 'shuffle' && m.shuffle != null) holdMedia({ shuffle: !m.shuffle });
    if (action === 'repeat' && m.repeat != null) holdMedia({ repeat: { off: 'all', all: 'one', one: 'off' }[m.repeat] || 'off' });
    if (action === 'favorite' && m.favorite != null) holdMedia({ favorite: !m.favorite }, 2500);
    if (action === 'next' || action === 'prev') S.seek = null;
    renderMedia();
  }
  invoke('media_cmd', { action }).catch(err => { S.mediaHold = null; flashNote(String(err)); });
  if (action === 'open') closePanel(true);
  else if (S.mode !== 'panel' && !S.ccOpen) quiet(invoke('release_focus'));
});

/* album-art tint for the music tab */
function tintFrom(url) {
  const img = new Image();
  img.onload = () => {
    const c = document.createElement('canvas'); c.width = c.height = 24;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(img, 0, 0, 24, 24);
    const px = g.getImageData(0, 0, 24, 24).data;
    let r = 0, gr = 0, b = 0, sum = 0;
    for (let i = 0; i < px.length; i += 4) {
      const R = px[i], G = px[i + 1], B = px[i + 2], mx = Math.max(R, G, B), mn = Math.min(R, G, B);
      const w = (mx ? (mx - mn) / mx : 0) ** 2 * (mx / 255) + 0.015; // favor vivid, bright pixels
      r += R * w; gr += G * w; b += B * w; sum += w;
    }
    setTint([r / sum, gr / sum, b / sum]);
  };
  img.src = url;
}
function setTint(rgb) {
  if (!rgb) { island.style.removeProperty('--tint'); island.style.removeProperty('--on-tint'); S.tint = null; return; }
  S.tint = rgb;
  let [r, g, b] = rgb.map(v => v / 255);
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
  let h = 0;
  if (d) h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h = Math.round(h * 60 + 360) % 360;
  const l0 = (mx + mn) / 2, s0 = d ? d / (1 - Math.abs(2 * l0 - 1)) : 0;
  // Keep it readable on the island: lighter on the dark skin, deeper on the light one.
  const sat = Math.round(clamp(s0 * 100, d ? 38 : 0, 85));
  const lig = Math.round(S.dark ? clamp(l0 * 100, 58, 72) : clamp(l0 * 100, 36, 48));
  island.style.setProperty('--tint', `hsl(${h} ${sat}% ${lig}%)`);
  island.style.setProperty('--on-tint', lig > 55 ? '#141218' : '#ffffff');
}

/* live levels from the player's audio session drive the equalizer bars */
// Only the bars themselves (and the cover art while the Music tab shows) are restyled, 30 times a
// second. Variables on <body> restyled every element on the page each time, which piled up
// hundreds of MB of garbage between collections.
let lvl = 0, lvlOffT = 0;
const muArt = $('.mu-art');
function eqBars() { if (!S.eqBars || !S.eqBars.every(b => b.isConnected)) S.eqBars = $$('.eq i'); return S.eqBars; }
listen('media:level', e => {
  lvl = Math.max(Math.pow(clamp(+e.payload || 0, 0, 1), 0.7), lvl * 0.8); // quick attack, smooth release
  const t = performance.now() / 1000, bars = eqBars();
  for (let i = 0; i < bars.length; i++) {
    const k = i % 4, h = lvl * (0.5 + 0.5 * Math.abs(Math.sin(t * (2.3 + k * 1.4) + k * 1.9)));
    bars[i].style.transform = `scaleY(${(0.16 + 0.84 * h).toFixed(3)})`;
  }
  if (S.mode === 'panel' && S.tab === 'music') muArt.style.setProperty('--lvl', lvl.toFixed(3));
  if (!S.eqLive) { S.eqLive = true; document.body.classList.add('live-eq'); }
  if (!S.eqLogged && e.payload > 0.02) { S.eqLogged = true; log(`equalizer is following the music (peak ${(+e.payload).toFixed(2)})`); }
  clearTimeout(lvlOffT);
  lvlOffT = setTimeout(() => {
    S.eqLive = false; document.body.classList.remove('live-eq');
    for (const b of eqBars()) b.style.transform = '';
    muArt.style.setProperty('--lvl', '0'); lvl = 0;
  }, 500);
});

/* lyrics (LRCLIB, looked up by the backend) */
function maybeLyrics(m) {
  const key = m ? m.key : '';
  if (key !== S.lyWant) { S.lyWant = key; S.lyWantAt = Date.now(); }
  if (S.ly.key && S.ly.key !== key) { S.ly = { key: '', state: 'idle', data: null, idx: -1 }; renderLyrics(); }
  if (!m || !S.lyricsOn || S.ly.key === key) return;
  // Wait a moment for the song length: it makes the match exact.
  if (!(m.duration > 0) && Date.now() - S.lyWantAt < 2500) return;
  S.ly = { key, state: 'loading', data: null, idx: -1 };
  renderLyrics();
  invoke('media_lyrics', { title: m.title, artist: m.artist, album: m.album || '', duration: m.duration || 0 })
    .then(d => { if (S.ly.key === key) { S.ly.state = d ? 'ok' : 'none'; S.ly.data = d; renderLyrics(); } })
    .catch(err => {
      if (S.ly.key !== key) return;
      S.ly.state = 'error'; renderLyrics(); log(`lyrics: ${err}`);
      setTimeout(() => { if (S.ly.key === key && S.ly.state === 'error') S.ly.key = ''; }, 30000); // try again later
    });
}
function renderLyrics() {
  const box = $('#lyrics'), lines = $('#lyLines'), d = S.ly.data, st = S.ly.state;
  box.hidden = !S.lyricsOn || !S.media;
  $('#muLyricsBtn').setAttribute('aria-pressed', S.lyricsOn);
  let html = '', msg = '', foot = '';
  if (st === 'loading') msg = 'Looking for lyrics…';
  else if (st === 'error') msg = 'Couldn’t reach the lyrics service. Trying again in a bit.';
  else if (st === 'none' || !d) msg = st === 'idle' ? '' : 'No lyrics found for this song.';
  else if (d.lines.length) {
    html = d.lines.map((l, i) => `<button type="button" class="ly-line${l.text ? '' : ' gap'}" data-i="${i}">${l.text ? esc(l.text) : '♪'}</button>`).join('');
    foot = 'Lyrics from LRCLIB';
  } else if (d.plain) {
    html = d.plain.split('\n').map(t => `<p class="ly-line">${esc(t) || '&nbsp;'}</p>`).join('');
    foot = 'Not time-synced · LRCLIB';
  } else msg = 'Instrumental ♪';
  box.classList.toggle('msg', !html);
  lines.classList.toggle('plain', !!(d && !d.lines.length && d.plain));
  lines.innerHTML = html || esc(msg);
  $('#lyNote').textContent = foot;
  lines.scrollTop = 0; S.ly.idx = -2;
  syncLyric(curPos(), true);
  if (S.mode === 'panel') layout();
}
/** Highlights the line being sung and keeps it in the middle of the box. */
function syncLyric(pos, instant) {
  const d = S.ly.data, timed = !!(d && d.lines.length);
  let idx = -1;
  if (timed) {
    const t = pos + 0.3; // a beat early, like Apple Music
    let lo = 0, hi = d.lines.length - 1;
    while (lo <= hi) { const mid = (lo + hi) >> 1; if (d.lines[mid].t <= t) { idx = mid; lo = mid + 1; } else hi = mid - 1; }
  }
  if (idx === S.ly.idx) return;
  const first = S.ly.idx === -2;
  S.ly.idx = idx;
  const lines = $('#lyLines');
  if (timed) {
    const els = lines.children;
    for (let i = 0; i < els.length; i++) { els[i].classList.toggle('on', i === idx); els[i].classList.toggle('past', i < idx); }
    const el = els[Math.max(idx, 0)];
    if (el && performance.now() > S.lyFollowPause && S.mode === 'panel' && S.tab === 'music') {
      lines.scrollTo({ top: el.offsetTop - lines.clientHeight / 2 + el.offsetHeight / 2, behavior: instant || first || reduced ? 'auto' : 'smooth' });
    }
  }
  const text = timed && idx >= 0 ? d.lines[idx].text : '';
  $$('[data-m="lyric"]').forEach(e => { if (e.textContent !== text) { e.textContent = text; e.style.animation = 'none'; void e.offsetWidth; e.style.animation = ''; } });
}
$('#lyLines').addEventListener('click', e => {
  const b = e.target.closest('.ly-line[data-i]'); if (!b || !S.ly.data) return;
  const l = S.ly.data.lines[+b.dataset.i]; if (l) seekTo(l.t);
});
// Scrolling the lyrics yourself pauses the auto-follow for a few seconds.
['wheel', 'pointerdown'].forEach(ev => $('#lyLines').addEventListener(ev, () => { S.lyFollowPause = performance.now() + 4000; }, { passive: true }));
$('#muLyricsBtn').addEventListener('click', () => {
  S.lyricsOn = !S.lyricsOn; store.set('lyrics', S.lyricsOn);
  S.ly = { key: '', state: 'idle', data: null, idx: -1 };
  renderLyrics(); maybeLyrics(S.media);
});

/* ---------- games: Steam covers + other launchers ---------- */
S.games = null; S.gamesT = 0;
// Other launchers, shown as app icons under the covers (matched against Start menu names).
const LAUNCHERS = [/^steam$/i, /^epic games launcher$/i, /^ea( app)?$/i, /^ubisoft connect$/i, /^riot client$/i, /^rockstar games launcher$/i, /^rsi launcher$/i,
  /^hytale launcher$/i, /^battle\.net$/i, /^gog galaxy/i, /^xbox$/i, /^minecraft launcher$/i, /^prism launcher$/i, /^modrinth app$/i, /^kyber launcher$/i, /^roblox player$/i];
/** The other game launchers that are installed (Start menu apps), in LAUNCHERS order. */
function launcherApps() {
  const seen = new Set();
  return LAUNCHERS.map(re => (S.apps || []).find(a => re.test(a.name.trim()) && !seen.has(a.id) && seen.add(a.id))).filter(Boolean);
}
async function loadGames() {
  try { S.games = await invoke('games_list'); } catch (e) { note('#gNote', String(e)); }
  renderGames();
}
/** Refreshes every 6 s while the tab is open, so a game you start shows as playing. */
function gamesLoop() {
  clearTimeout(S.gamesT);
  loadGames().finally(() => { if (S.mode === 'panel' && S.tab === 'games') S.gamesT = setTimeout(gamesLoop, 6000); });
}
function coverHtml(g, i, running) {
  const img = g.cover || g.header;
  const art = img ? `<img src="${esc(assetUrl(img))}" alt="" loading="${i < 8 ? 'eager' : 'lazy'}" draggable="false">` : `<span class="cv-name">${esc(g.name)}</span>`;
  const tag = running ? '<span class="cv-tag">Playing</span>' : g.needs_update ? '<span class="cv-tag up">Update</span>' : '';
  return `<button type="button" role="listitem" class="cover${i === 0 ? ' big' : ''}${running ? ' playing' : ''}${!g.cover && g.header ? ' wide' : ''}" data-appid="${g.appid}" title="${esc(g.name)}" aria-label="Play ${esc(g.name)}"${img ? '' : ` style="background:${gradFor(g.name)}"`}>${art}${tag}<span class="cv-play">${icon('play')}</span></button>`;
}
function renderGames() {
  const lib = S.games;
  if (!lib) return;
  const shelf = $('#gShelf');
  // Covers (some animated) are only in the page while the tab shows; see releaseHidden().
  if (!(S.mode === 'panel' && S.tab === 'games')) setHtml(shelf, '');
  else if (!lib.installed) setHtml(shelf, '<p class="empty">Steam isn’t installed.</p>');
  else if (!lib.games.length) setHtml(shelf, '<p class="empty">No Steam games installed yet.</p>');
  else {
    // A running game goes first, then most recently played.
    const games = [...lib.games].sort((a, b) => (b.appid === lib.running) - (a.appid === lib.running));
    setHtml(shelf, games.map((g, i) => coverHtml(g, i, g.appid === lib.running)).join(''));
  }
  setHtml($('#gLaunchers'), launcherApps().map(appTile).join(''));
  if (S.mode === 'panel' && S.tab === 'games') layout();
}
function playGame(g) {
  closePanel(true);
  invoke('game_action', { appid: g.appid, what: 'play' })
    .then(() => setTimeout(() => toast(g.name, 'Starting through Steam…', null, g.cover ? { img: assetUrl(g.cover), ms: 2600, sound: 'small' } : { glyph: glyphOf(g.name), bg: gradFor(g.name), ms: 2600, sound: 'small' }), 260))
    .catch(e => setTimeout(() => toast(`Couldn't start ${g.name}`, String(e), 'x', { sound: 'problem' }), 260));
}
const gameOf = el => S.games && S.games.games.find(g => String(g.appid) === el.dataset.appid);
$('#v-games').addEventListener('click', e => {
  const c = e.target.closest('.cover[data-appid]');
  if (c) { const g = gameOf(c); if (g) playGame(g); return; }
  const tile = e.target.closest('.app[data-id]');
  if (tile) { const a = appOf(tile.dataset.id); if (a) openPinned(a); }
});
$('#v-games').addEventListener('contextmenu', e => {
  const c = e.target.closest('.cover[data-appid]'), tile = e.target.closest('.app[data-id]');
  if (!c && !tile) return;
  e.preventDefault();
  if (tile) { const a = appOf(tile.dataset.id); if (a) appMenu(a, e.clientX, e.clientY); return; }
  const g = gameOf(c); if (!g) return;
  const act = what => invoke('game_action', { appid: g.appid, what }).then(() => closePanel(true)).catch(err => note('#gNote', String(err)));
  showCtx(e.clientX, e.clientY, [
    { heading: g.name },
    { label: 'Play', icon: 'play', action: () => playGame(g) },
    { label: 'Show in Steam', icon: 'gamepad', action: () => act('library') },
    { label: 'Open install folder', icon: 'folder', action: () => act('folder') }
  ]);
});
// The mouse wheel scrolls the covers and launchers sideways (and doesn't switch tabs or close the panel).
// A cover that won't load falls back to the wide header art, then to a name card.
$('#gShelf').addEventListener('error', e => {
  const img = e.target; if (img.tagName !== 'IMG') return;
  const c = img.closest('.cover'), g = c && gameOf(c); if (!g) return;
  if (g.header && !img.dataset.fallback) { img.dataset.fallback = '1'; c.classList.add('wide'); img.src = assetUrl(g.header); }
  else { c.style.background = gradFor(g.name); img.replaceWith(Object.assign(document.createElement('span'), { className: 'cv-name', textContent: g.name })); }
}, true);
// Load the library shortly after start so the tab opens instantly, and note once whether covers load.
setTimeout(async () => {
  await loadGames();
  const first = S.games && S.games.games.find(g => g.cover);
  if (!first) return;
  const img = new Image();
  img.onload = () => log(`games: ${S.games.games.length} Steam games; covers load (${img.naturalWidth}x${img.naturalHeight})`);
  img.onerror = () => log(`games: a cover didn't load (${first.cover})`);
  img.src = assetUrl(first.cover);
}, 7000);
['#gShelf', '#gLaunchers'].forEach(sel => $(sel).addEventListener('wheel', e => {
  e.preventDefault(); e.stopPropagation();
  e.currentTarget.scrollLeft += (Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX) * 1.1;
}, { passive: false }));

/* ---------- audio: master volume, devices, microphone, per-app mixer ---------- */
S.audioDev = null; S.audioApps = []; S.auDevT = 0; S.auAppsT = 0;
const audioShowing = () => S.mode === 'panel' && S.tab === 'audio' && S.auMode !== 'sounds' && !(S.auMode === 'vm' && S.vm);
/** Devices every 3 s, the mixer (with its live meters) 4 times a second, only while the tab shows. */
function audioDevLoop() {
  clearTimeout(S.auDevT);
  invoke('audio_devices').then(d => { S.audioDev = d; renderAudioDevices(); }).catch(err => note('#auNote', String(err)))
    .finally(() => { if (audioShowing()) S.auDevT = setTimeout(audioDevLoop, 3000); });
}
function audioAppsLoop() {
  clearTimeout(S.auAppsT);
  invoke('audio_apps').then(list => { S.audioApps = list; renderAudioApps(); }).catch(() => {})
    .finally(() => { if (audioShowing()) S.auAppsT = setTimeout(audioAppsLoop, 250); });
}
function renderAudioMaster() {
  const a = S.audio; if (!a) return;
  const input = $('#auVol'), v = Math.round(a.level * 100);
  if (document.activeElement !== input || performance.now() - S.volSetAt > 1500) input.value = v;
  input.style.setProperty('--p', (a.muted ? 0 : +input.value) + '%');
  txt($('#auVolOut'), a.muted ? 'Muted' : input.value + '%');
  const mb = $('#auMute'); setHtml(mb, icon(a.muted || a.level === 0 ? 'mute' : deviceIcon(a))); mb.classList.toggle('muted', a.muted);
  mb.setAttribute('aria-label', a.muted ? 'Unmute' : 'Mute');
}
$('#auVol').addEventListener('input', e => {
  const v = +e.target.value;
  S.audio = { ...(S.audio || { device: '' }), level: v / 100, muted: false }; S.volSetAt = performance.now();
  renderControls(); renderAudioMaster();
  clearTimeout(S.auVolT); S.auVolT = setTimeout(() => quiet(invoke('set_volume', { level: v / 100 })), 40);
});
$('#auMute').addEventListener('click', () => {
  if (!S.audio) return;
  S.audio.muted = !S.audio.muted; S.volSetAt = performance.now(); renderControls(); renderAudioMaster();
  quiet(invoke('set_mute', { muted: S.audio.muted }));
});
const devHtml = d => `<button type="button" class="au-dv" role="radio" aria-checked="${d.default}" data-dev="${esc(d.id)}" title="${esc(d.name)}${d.detail ? ' (' + esc(d.detail) + ')' : ''}"><i class="dot"></i><span><b>${esc(d.name)}</b><small>${esc(d.detail || ' ')}</small></span></button>`;
function renderAudioDevices() {
  const d = S.audioDev; if (!d) return;
  setHtml($('#auOut'), d.outputs.map(devHtml).join('') || '<p class="empty">No output devices</p>');
  setHtml($('#auIn'), d.inputs.map(devHtml).join('') || '<p class="empty">No microphones</p>');
  txt($('#auOutCount'), `${d.outputs.length} device${d.outputs.length === 1 ? '' : 's'}`);
  txt($('#auInCount'), `${d.inputs.length} device${d.inputs.length === 1 ? '' : 's'}`);
  $('#auMicRow').hidden = !d.mic;
  if (d.mic) {
    const [level, muted] = d.mic, mic = $('#auMic');
    if (document.activeElement !== mic || !(performance.now() - (S.micSetAt || -1e9) < 1500)) mic.value = Math.round(level * 100);
    mic.style.setProperty('--p', (muted ? 0 : +mic.value) + '%');
    txt($('#auMicOut'), muted ? 'Muted' : mic.value + '%');
    const mb = $('#auMicMute'); setHtml(mb, icon(muted ? 'micoff' : 'mic')); mb.classList.toggle('muted', muted);
    mb.setAttribute('aria-label', muted ? 'Unmute microphone' : 'Mute microphone');
  }
  if (audioShowing()) layout();
}
$('#v-audio').addEventListener('click', e => {
  const b = e.target.closest('.au-dv[data-dev]'); if (!b || b.getAttribute('aria-checked') === 'true') return;
  note('#auNote', '');
  invoke('audio_default', { id: b.dataset.dev })
    .then(() => { audioDevLoop(); quiet(invoke('get_status').then(applyStatus)); })
    .catch(err => note('#auNote', `Couldn't switch: ${err}`));
});
$('#auMic').addEventListener('input', e => {
  const v = +e.target.value; S.micSetAt = performance.now();
  e.target.style.setProperty('--p', v + '%'); txt($('#auMicOut'), v + '%');
  if (S.audioDev && S.audioDev.mic) S.audioDev.mic = [v / 100, false];
  clearTimeout(S.micT); S.micT = setTimeout(() => quiet(invoke('audio_mic', { level: v / 100, muted: false })), 40);
});
$('#auMicMute').addEventListener('click', () => {
  const m = S.audioDev && S.audioDev.mic; if (!m) return;
  S.audioDev.mic = [m[0], !m[1]]; renderAudioDevices();
  quiet(invoke('audio_mic', { muted: S.audioDev.mic[1] }));
});
/** A friendly name for a program in the mixer. */
function audioAppName(a) {
  if (!a.exe) return 'System sounds';
  const stem = exeStem(a.exe).toLowerCase();
  if (stem === 'amplibraryagent') return 'Apple Music';
  if (stem === 'applemusic') return 'Apple Music window';
  const known = S.apps.find(x => x.exe && x.exe.toLowerCase() === a.exe);
  return known ? known.name : prettyStem(exeStem(a.exe));
}
/** Rows are built once per program and then only updated, so a slider being dragged isn't replaced. */
function renderAudioApps() {
  const box = $('#auApps'), list = S.audioApps;
  if (!list.length) { setHtml(box, '<p class="empty">Nothing is making sound.</p>'); return; }
  if (box.querySelector('.empty')) setHtml(box, '');
  const keep = new Set(list.map(a => String(a.pid)));
  box.querySelectorAll('.au-app').forEach(r => { if (!keep.has(r.dataset.pid)) r.remove(); });
  list.forEach((a, i) => {
    let row = box.querySelector(`.au-app[data-pid="${a.pid}"]`);
    if (!row) {
      const name = audioAppName(a);
      row = document.createElement('div');
      row.className = 'au-app'; row.dataset.pid = a.pid;
      const ico = a.icon ? `<span class="ico"><img src="${esc(assetUrl(a.icon))}" alt=""></span>` : `<span class="ico g" style="background:${a.exe ? gradFor(name) : 'var(--ink-3)'}">${a.exe ? esc(glyphOf(name)) : icon('speaker')}</span>`;
      row.innerHTML = `${ico}<span class="nm"><b title="${esc(a.exe || name)}">${esc(name)}</b><span class="meter"><i></i></span></span><input class="au-range" type="range" min="0" max="100" aria-label="${esc(name)} volume"><span class="pct"></span><button class="ib" type="button" aria-label="Mute ${esc(name)}"></button>`;
    }
    if (box.children[i] !== row) box.insertBefore(row, box.children[i] || null);
    row.classList.toggle('on', a.active);
    const range = row.querySelector('input'), pct = Math.round(a.volume * 100);
    if (document.activeElement !== range && !(performance.now() - (+row.dataset.setAt || -1e9) < 1200)) range.value = pct;
    range.style.setProperty('--p', (a.muted ? 0 : +range.value) + '%');
    txt(row.querySelector('.pct'), a.muted ? 'off' : range.value + '%');
    row.querySelector('.meter i').style.transform = `scaleX(${Math.min(1, Math.sqrt(a.peak)).toFixed(3)})`;
    const mb = row.querySelector('button'); setHtml(mb, icon(a.muted ? 'mute' : 'speaker')); mb.classList.toggle('muted', a.muted);
  });
}
$('#auApps').addEventListener('input', e => {
  const range = e.target.closest('.au-app input'); if (!range) return;
  const row = range.closest('.au-app'), pid = +row.dataset.pid, v = +range.value;
  row.dataset.setAt = performance.now(); range.style.setProperty('--p', v + '%'); txt(row.querySelector('.pct'), v + '%');
  clearTimeout(row._t); row._t = setTimeout(() => invoke('audio_app_set', { pid, level: v / 100, muted: false }).catch(err => note('#auNote', String(err))), 40);
});
$('#auApps').addEventListener('click', e => {
  const b = e.target.closest('.au-app button'); if (!b) return;
  const row = b.closest('.au-app'), a = S.audioApps.find(x => String(x.pid) === row.dataset.pid); if (!a) return;
  a.muted = !a.muted; row.dataset.setAt = performance.now(); renderAudioApps();
  invoke('audio_app_set', { pid: a.pid, muted: a.muted }).catch(err => note('#auNote', String(err)));
});

/* ---------- Voicemeeter, through VoiceDeck's engine ---------- */
S.vm = null; S.vdInstalled = false; S.vmInstalled = false; S.vmLevelsOn = false;
S.auMode = store.get('auMode', 'vm'); S.vmSide = store.get('vmSide', 'in');
const VIRTUAL_NAMES = ['Voicemeeter VAIO', 'Voicemeeter AUX', 'Voicemeeter VAIO3'];
function stripName(s) {
  if (s.label) return s.label;
  if (s.physical) return s.device || `Input ${s.index + 1}`;
  const v = S.vm.strips.filter(x => !x.physical).indexOf(s);
  return VIRTUAL_NAMES[v] || `Virtual ${v + 1}`;
}
/** Meters (and the engine's level reads) only while the Voicemeeter view is on screen. */
function syncVmLevels() {
  const want = !!(S.vm && S.auMode === 'vm' && S.mode === 'panel' && S.tab === 'audio');
  if (want !== S.vmLevelsOn) { S.vmLevelsOn = want; quiet(invoke('vm_levels', { on: want })); }
}
function renderAudioHead() {
  const installed = S.vmInstalled || !!S.vm;
  $('#auHead').hidden = false; // Sounds is always there
  $('#auMode [data-mode="vm"]').hidden = !installed;
  const vmView = installed && S.auMode === 'vm', sndView = S.auMode === 'sounds';
  $$('#auMode button').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === (vmView ? 'vm' : sndView ? 'sounds' : 'win')));
  $('#auVm').hidden = !vmView; $('#auWin').hidden = vmView || sndView; $('#auSnd').hidden = !sndView;
  if (sndView) renderSounds();
  $('#vmRestart').hidden = !(vmView && S.vm);
  $('#vdOpen').hidden = !S.vdInstalled;
  $$('#vmSide button').forEach(b => b.setAttribute('aria-pressed', b.dataset.side === S.vmSide));
  $('#vmStrips').hidden = S.vmSide !== 'in' || !S.vm; $('#vmBuses').hidden = S.vmSide !== 'out' || !S.vm;
  $('#vmSide').hidden = !S.vm; $('#vmOff').hidden = !!S.vm;
  if (S.mode === 'panel' && S.tab === 'audio') layout();
}
const gainPct = g => clamp((g + 60) / 72 * 100, 0, 100);
const levelPct = v => clamp((20 * Math.log10(Math.max(v, 1e-6)) + 60) / 72, 0, 1);
function vmRowHtml(kind, i) {
  const route = kind === 'strip'
    ? `<div class="vm-route">${S.vm.busNames.map((n, b) => `<button type="button" class="${n.startsWith('B') ? 'b' : ''}" data-route="${b}" title="Send to ${esc(n)}">${esc(n)}</button>`).join('')}</div>` : '';
  return `<div class="vm-row${kind === 'bus' ? ' bus' : ''}" data-kind="${kind}" data-i="${i}">
    <div class="vm-name"><b></b><small></small></div>
    <div class="vm-fader"><span class="vm-meter"><i></i></span><input type="range" min="-60" max="12" step="0.1" aria-label="Gain"></div>
    <span class="vm-db"></span>
    <button type="button" class="vm-btn" data-mute aria-pressed="false" title="Mute">M</button>${route}</div>`;
}
/** Builds the rows when the mixer's shape changes, then only updates values (a dragged fader is left alone). */
function renderVm(full) {
  renderAudioHead();
  if (!S.vm) { S.vmShape = ''; return; }
  const shape = `${S.vm.strips.length}/${S.vm.buses.length}/${S.vm.busNames.join()}`;
  if (full || shape !== S.vmShape) {
    S.vmShape = shape;
    $('#vmStrips').innerHTML = S.vm.strips.map((_, i) => vmRowHtml('strip', i)).join('');
    $('#vmBuses').innerHTML = S.vm.buses.map((_, i) => vmRowHtml('bus', i)).join('');
    S.vmMeters = null;
  }
  const now = performance.now();
  const paint = (row, item, name, sub, route) => {
    txt(row.querySelector('.vm-name b'), name); txt(row.querySelector('.vm-name small'), sub);
    const input = row.querySelector('input');
    if (document.activeElement !== input && !(now - (+row.dataset.setAt || -1e9) < 1200)) input.value = item.gain;
    input.style.setProperty('--p', gainPct(+input.value) + '%');
    txt(row.querySelector('.vm-db'), (+input.value > 0 ? '+' : '') + (+input.value).toFixed(1));
    row.classList.toggle('muted', item.mute);
    row.querySelector('[data-mute]').setAttribute('aria-pressed', item.mute);
    if (route) row.querySelectorAll('[data-route]').forEach(b => b.setAttribute('aria-pressed', !!route[+b.dataset.route]));
  };
  $$('#vmStrips .vm-row').forEach((row, i) => { const s = S.vm.strips[i]; if (s) paint(row, s, stripName(s), s.physical ? (s.device || 'Hardware input') : 'Virtual input', s.routing); });
  $$('#vmBuses .vm-row').forEach((row, i) => { const b = S.vm.buses[i]; if (b) paint(row, b, b.name, b.device || (b.name.startsWith('B') ? 'Virtual output' : 'No device'), null); });
  if (full && S.mode === 'panel' && S.tab === 'audio') layout();
}
function applyVmDelta(d) {
  if (!S.vm || !d) return;
  for (const x of d.strips || []) { const s = S.vm.strips[x.index]; if (s) for (const k of ['label', 'device', 'gain', 'mute', 'solo', 'routing', 'fx']) if (x[k] !== undefined) s[k] = x[k]; }
  for (const x of d.buses || []) { const b = S.vm.buses[x.index]; if (b) for (const k of ['name', 'device', 'gain', 'mute']) if (x[k] !== undefined) b[k] = x[k]; }
}
function paintVmLevels(l) {
  if (!S.vmMeters) S.vmMeters = { strips: $$('#vmStrips .vm-meter i'), buses: $$('#vmBuses .vm-meter i') };
  // Reveal the full-width green→red gradient up to the level (scaling it would squeeze the colors).
  const set = (els, vals) => { if (vals) vals.forEach((lr, i) => { const el = els[i]; if (el) el.style.clipPath = `inset(0 ${((1 - levelPct(Math.max(lr[0], lr[1]))) * 100).toFixed(1)}% 0 0)`; }); };
  if (S.vmSide === 'in') set(S.vmMeters.strips, l.strips); else set(S.vmMeters.buses, l.buses);
}
function renderVmChip() {
  const chip = $('#barMic'), s = S.vm && S.vm.strips[0];
  chip.hidden = !s;
  if (!s) return;
  setHtml(chip, icon(s.mute ? 'micoff' : 'mic'));
  chip.classList.toggle('mic-off', s.mute);
  chip.title = `${stripName(s)}: ${s.mute ? 'muted (click to unmute)' : 'live (click to mute)'}`;
}
listen('vm:state', e => { S.vm = e.payload; renderVm(true); renderVmChip(); syncVmLevels(); });
listen('vm:delta', e => { applyVmDelta(e.payload); renderVm(false); renderVmChip(); });
listen('vm:levels', e => paintVmLevels(e.payload));
invoke('vm_state').then(s => { S.vm = s; renderVm(true); renderVmChip(); }).catch(() => {});
invoke('mixer_apps').then(a => { S.vdInstalled = a.voicedeck; S.vmInstalled = a.voicemeeter; renderAudioHead(); }).catch(() => {});

function vmParam(row, field) { return `${row.dataset.kind === 'strip' ? 'Strip' : 'Bus'}[${row.dataset.i}].${field}`; }
function vmItem(row) { return row.dataset.kind === 'strip' ? S.vm.strips[+row.dataset.i] : S.vm.buses[+row.dataset.i]; }
function vmGain(row, g) {
  const item = vmItem(row); if (!item) return;
  g = Math.round(clamp(g, -60, 12) * 10) / 10;
  item.gain = g; row.dataset.setAt = performance.now();
  const input = row.querySelector('input'); input.value = g;
  input.style.setProperty('--p', gainPct(g) + '%'); txt(row.querySelector('.vm-db'), (g > 0 ? '+' : '') + g.toFixed(1));
  clearTimeout(row._t); row._t = setTimeout(() => invoke('vm_set', { name: vmParam(row, 'Gain'), value: g }).catch(err => note('#auNote', String(err))), 30);
}
$('#auVm').addEventListener('input', e => { const row = e.target.closest('.vm-row'); if (row && e.target.matches('input')) vmGain(row, +e.target.value); });
$('#auVm').addEventListener('dblclick', e => { const row = e.target.closest('.vm-fader') && e.target.closest('.vm-row'); if (row) vmGain(row, 0); });
$('#auVm').addEventListener('wheel', e => {
  const row = e.target.closest('.vm-fader') && e.target.closest('.vm-row'); if (!row) return;
  e.preventDefault(); e.stopPropagation();
  vmGain(row, vmItem(row).gain + (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.1 : 1));
}, { passive: false });
$('#auVm').addEventListener('click', e => {
  const row = e.target.closest('.vm-row'); if (!row || !S.vm) return;
  const item = vmItem(row); if (!item) return;
  if (e.target.closest('[data-mute]')) {
    item.mute = !item.mute; renderVm(false); renderVmChip();
    invoke('vm_set', { name: vmParam(row, 'Mute'), value: item.mute ? 1 : 0 }).catch(err => note('#auNote', String(err)));
  }
  const r = e.target.closest('[data-route]');
  if (r && item.routing) {
    const b = +r.dataset.route; item.routing[b] = !item.routing[b]; renderVm(false);
    invoke('vm_set', { name: vmParam(row, S.vm.busNames[b]), value: item.routing[b] ? 1 : 0 }).catch(err => note('#auNote', String(err)));
  }
});
$('#auMode').addEventListener('click', e => {
  const b = e.target.closest('[data-mode]'); if (!b) return;
  S.auMode = b.dataset.mode; store.set('auMode', S.auMode);
  renderAudioHead(); syncVmLevels();
  if (audioShowing()) { audioDevLoop(); audioAppsLoop(); }
});
/* ---------- notification sounds (Audio → Sounds) ---------- */
function renderSounds() {
  $('#sndOn').setAttribute('aria-pressed', sounds.on); $('#sndOn').textContent = sounds.on ? 'Notification sounds on' : 'Notification sounds off';
  $('#sndFs').setAttribute('aria-pressed', sounds.fsMute);
  if (document.activeElement !== $('#sndVol')) { $('#sndVol').value = Math.round(sounds.vol * 100); }
  $('#sndVolOut').textContent = Math.round(sounds.vol * 100) + '%';
  $('#sndList').classList.toggle('off', !sounds.on);
  const opts = kind => {
    const cur = sounds.choice(kind), file = sounds.fileName(kind);
    return sounds.PRESETS.filter(([v]) => v !== 'file').map(([v, n]) => `<option value="${v}"${v === cur ? ' selected' : ''}>${n}</option>`).join('')
      + (file ? `<option value="file"${cur === 'file' ? ' selected' : ''}>${esc(clipTxt(file, 28))}</option><option value="file-new">Choose another file…</option>` : '<option value="file-new">Your own file…</option>');
  };
  setHtml($('#sndList'), sounds.KINDS.map(([k, n, sub]) => `<div class="snd-row" data-kind="${k}"><span class="snd-t"><b>${esc(n)}</b><small>${esc(sub)}</small></span>
    <select class="snd-sel" aria-label="Sound for ${esc(n)}">${opts(k)}</select>
    <button class="ib snd-play" type="button" aria-label="Play the sound for ${esc(n)}" title="Play it"${sounds.choice(k) === 'none' ? ' disabled' : ''}>${icon('play')}</button></div>`).join(''));
  if (S.mode === 'panel' && S.tab === 'audio') layout();
}
let sndFor = '';
$('#sndList').addEventListener('change', e => {
  const sel = e.target.closest('.snd-sel'); if (!sel) return;
  const kind = sel.closest('.snd-row').dataset.kind;
  if (sel.value === 'file-new') { sndFor = kind; sel.value = sounds.choice(kind); $('#sndFile').value = ''; $('#sndFile').click(); return; }
  sounds.choose(kind, sel.value); renderSounds();
  if (sel.value !== 'none') sounds.sound(sel.value, kind);
});
$('#sndList').addEventListener('click', e => {
  const b = e.target.closest('.snd-play'); if (!b) return;
  const kind = b.closest('.snd-row').dataset.kind; sounds.sound(sounds.choice(kind), kind);
});
$('#sndFile').addEventListener('change', async e => {
  const f = e.target.files && e.target.files[0]; if (!f || !sndFor) return;
  note('#sndNote', '');
  try { await sounds.setFile(sndFor, f); renderSounds(); sounds.sound('file', sndFor); }
  catch (err) { note('#sndNote', err.message || String(err)); }
});
$('#sndOn').addEventListener('click', () => { sounds.set({ on: !sounds.on }); if (sounds.on) sounds.sound('glass'); renderSounds(); renderControls(); });
$('#sndFs').addEventListener('click', () => { sounds.set({ fsMute: !sounds.fsMute }); renderSounds(); });
$('#sndVol').addEventListener('input', e => { sounds.set({ vol: +e.target.value / 100 }); $('#sndVolOut').textContent = e.target.value + '%'; });
$('#sndVol').addEventListener('change', () => { sounds.sound(sounds.choice('mention') !== 'none' ? sounds.choice('mention') : 'glass', 'mention'); renderControls(); });
$('#vmSide').addEventListener('click', e => {
  const b = e.target.closest('[data-side]'); if (!b) return;
  S.vmSide = b.dataset.side; store.set('vmSide', S.vmSide); renderAudioHead();
});
$('#vmRestart').addEventListener('click', () => invoke('vm_restart').then(() => note('#auNote', 'Restarting Voicemeeter’s audio engine…')).catch(err => note('#auNote', String(err))));
$('#vdOpen').addEventListener('click', () => invoke('voicedeck_open').then(() => closePanel(true)).catch(err => note('#auNote', String(err))));
$('#barMic').addEventListener('click', e => {
  e.stopPropagation();
  const s = S.vm && S.vm.strips[0]; if (!s) return;
  s.mute = !s.mute; renderVmChip(); renderVm(false);
  invoke('vm_mic').catch(() => { s.mute = !s.mute; renderVmChip(); });
});

/* ---------- eye tracking (Tobii) ---------- */
// The backend only reports whether your eyes are on the island (gaze:island) and tracker status.
S.gaze = false; S.gazeSt = null; S.toastWait = null;
const gazeLive = () => !!(S.gazeSt && S.gazeSt.enabled && S.gazeSt.connected && S.gazeSt.present);
listen('gaze:island', e => {
  S.gaze = !!e.payload;
  if (S.gaze) {
    if (S.mode === 'toast' && S.toastWait) S.toastWait();
    else if (S.mode === 'pill' && !isBar()) setMode('peek');
  } else if (S.mode === 'peek' && !S.hover) {
    clearTimeout(hoverT);
    hoverT = setTimeout(() => { if (S.mode === 'peek' && !S.hover && !S.gaze) setMode('pill'); }, 120);
  }
});
listen('gaze:status', e => { S.gazeSt = e.payload; if (!gazeLive()) S.gaze = false; renderControls(); });
invoke('gaze_status').then(st => { S.gazeSt = st; renderControls(); }).catch(() => {});
function renderGaze() {
  const g = S.gazeSt, tile = $('.tile[data-tile="eye"]');
  tile.hidden = !(g && g.available);
  $('#gazeRow').hidden = !(g && g.available && g.enabled);
  if (!g || !g.available) return;
  tile.setAttribute('aria-pressed', g.enabled && g.connected);
  txt(tile.querySelector('small'), !g.enabled ? 'Off' : !g.connected ? 'Looking for the tracker…' : g.present ? 'On' : 'On · you’re away');
  $('#gazePause').setAttribute('aria-pressed', !!g.pause_when_away);
}
$('#gazePause').addEventListener('click', async () => {
  if (!S.gazeSt) return;
  S.gazeSt = await invoke('gaze_set', { pauseWhenAway: !S.gazeSt.pause_when_away });
  renderControls();
});

/* ---------- network: Tailscale + Radmin VPN ---------- */
S.vpn = null; S.vpnBusy = ''; S.vpnT = 0;
async function loadVpn() {
  try { S.vpn = await invoke('vpn_status'); } catch (e) {}
  if (S.vpn && !S.vpnLogged) {
    S.vpnLogged = true;
    const t = S.vpn.tailscale, r = S.vpn.radmin;
    log(`network: tailscale ${t ? `${t.state} ${t.ip} ${t.peers.length} devices` : 'not installed'}; radmin ${r ? `${r.running && r.on ? 'on' : 'off'} ${r.ip} ${r.peers.filter(p => p.online).length}/${r.peers.length} online` : 'not installed'}`);
  }
  renderVpn(); renderControls(); renderBar();
}
/** Every 4 s while the Network tab is open, every 20 s otherwise (for the tiles and the bar). */
function vpnLoop() {
  clearTimeout(S.vpnT);
  loadVpn().finally(() => { S.vpnT = setTimeout(vpnLoop, S.mode === 'panel' && S.tab === 'network' ? 4000 : 20000); });
}
const tsOn = () => !!(S.vpn && S.vpn.tailscale && S.vpn.tailscale.state === 'Running');
const rvOn = () => { const r = S.vpn && S.vpn.radmin; return !!(r && r.running && r.on && r.server); };
const vpnOn = () => tsOn() || rvOn();
function ago(iso) {
  const t = Date.parse(iso); if (!t || t < 946684800000) return '';
  const m = Math.round((Date.now() - t) / 60000);
  if (m < 2) return 'just now';
  if (m < 60) return `${m} min ago`;
  if (m < 60 * 24) return `${Math.round(m / 60)} h ago`;
  if (m < 60 * 48) return 'yesterday';
  return new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
const tsName = p => p.name && p.name !== 'localhost' ? p.name : (p.dns || '').split('.')[0] || 'Device';
const TS_STATE = { Running: 'Connected', Stopped: 'Disconnected', NeedsLogin: 'Signed out', Starting: 'Connecting…', NoState: 'Tailscale app isn’t running', NeedsMachineAuth: 'Waiting for admin approval' };
function renderVpnTiles() {
  const t = S.vpn && S.vpn.tailscale, r = S.vpn && S.vpn.radmin;
  const ts = $('.tile[data-tile="tailscale"]'), rv = $('.tile[data-tile="radmin"]');
  ts.hidden = !t; rv.hidden = !r;
  if (t) { ts.setAttribute('aria-pressed', tsOn()); txt(ts.querySelector('small'), S.vpnBusy || (tsOn() ? t.ip || 'Connected' : TS_STATE[t.state] || t.state)); }
  if (r) {
    const n = r.peers.filter(p => p.online).length;
    rv.setAttribute('aria-pressed', rvOn());
    txt(rv.querySelector('small'), !r.running ? 'Service stopped' : !r.on ? 'Off' : !r.server ? 'Connecting…' : `${n} online`);
  }
}
function renderVpn() {
  const st = S.status;
  if (st) {
    const n = st.net || {};
    $('#netNow').innerHTML = n.kind === 'none' || !n.kind ? `${icon('wifioff')}<span>Offline</span>`
      : `${icon(n.kind === 'ethernet' ? 'ethernet' : 'wifi')}<span><b>${esc(n.name || (n.kind === 'ethernet' ? 'Ethernet' : 'Wi-Fi'))}</b> · ${n.kind === 'ethernet' ? 'Ethernet' : 'Wi-Fi'}${n.bars != null && n.kind === 'wifi' ? ` · ${n.bars * 20}% signal` : ''}</span>`;
  }
  const v = S.vpn, t = v && v.tailscale, r = v && v.radmin;
  $('#tsCard').hidden = !t; $('#rvCard').hidden = !r;
  if (t) {
    const on = tsOn();
    $('#tsCard').classList.toggle('on', on);
    $('#tsState').textContent = S.vpnBusy || (on ? `Connected · ${t.tailnet || 'your tailnet'}` : TS_STATE[t.state] || t.state);
    const btn = $('#tsToggle');
    btn.hidden = false; btn.disabled = !!S.vpnBusy;
    btn.textContent = on ? 'Disconnect' : t.state === 'NeedsLogin' ? 'Sign in' : 'Connect';
    btn.classList.toggle('primary', !on);
    const me = $('#tsMe');
    me.hidden = !t.ip;
    if (t.ip) { me.dataset.copy = t.ip; setHtml(me, `${icon('copy')}<span>This PC · ${esc(t.name)}</span><b>${esc(t.ip)}</b>`); }
    const exits = t.peers.filter(p => p.exit_option);
    $('#tsExitRow').hidden = !on || !exits.length;
    setHtml($('#tsExit'), `<option value="">None</option>` + exits.map(p => `<option value="${esc(p.ip)}"${p.exit_node ? ' selected' : ''}${p.online ? '' : ' disabled'}>${esc(tsName(p))}${p.online ? '' : ' (offline)'}</option>`).join(''));
    setHtml($('#tsPeers'), !on ? '' : t.peers.length ? t.peers.map(p => {
      const sub = p.online ? [p.ip, p.os, p.link === 'direct' ? 'direct' : p.link ? `relay ${p.link}` : ''].filter(Boolean).join(' · ') : `Offline${ago(p.last_seen) ? ' · last seen ' + ago(p.last_seen) : ''}`;
      return `<button type="button" class="peer${p.online ? ' on' : ''}" data-copy="${esc(p.ip)}" title="Copy ${esc(p.ip)}"><i class="dot"></i><span><b>${esc(tsName(p))}</b><small>${esc(sub)}</small></span>${p.exit_node ? '<span class="tag exit">Exit node</span>' : ''}</button>`;
    }).join('') : '<p class="empty">No other devices on this tailnet.</p>');
  }
  if (r) {
    const on = rvOn(), online = r.peers.filter(p => p.online);
    $('#rvCard').classList.toggle('on', on);
    $('#rvState').textContent = !r.running ? 'Service stopped' : !r.on ? 'Switched off' : !r.server ? 'Connecting to Radmin…' : `${online.length} online${r.networks.length ? ' · ' + r.networks.join(', ') : ''}`;
    $('#rvOpen').hidden = false;
    const me = $('#rvMe');
    me.hidden = !r.ip;
    if (r.ip) { me.dataset.copy = r.ip; setHtml(me, `${icon('copy')}<span>This PC</span><b>${esc(r.ip)}</b>`); }
    setHtml($('#rvPeers'), r.peers.length ? r.peers.map(p => {
      const when = new Date(p.since);
      const t2 = isNaN(when) ? '' : (Date.now() - when < 864e5 ? when.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : ago(p.since));
      const how = p.link === 'Relay' ? 'Through Radmin’s relay' : p.link === 'Direct' ? 'Direct connection' : 'Connected';
      const sub = p.online ? `${how}${t2 ? ' · since ' + t2 : ''}` : `Offline${t2 ? ' · last seen ' + t2 : ''}`;
      return `<div class="peer${p.online ? ' on' : ''}"><i class="dot"></i><span><b>${esc(p.name)}</b><small>${esc(sub)}</small></span></div>`;
    }).join('') : '<p class="empty">Nobody has connected yet.</p>');
  }
  if (S.mode === 'panel' && S.tab === 'network') layout();
}
async function tsToggle() {
  const t = S.vpn && S.vpn.tailscale; if (!t || S.vpnBusy) return;
  const up = !tsOn();
  S.vpnBusy = up ? 'Connecting…' : 'Disconnecting…';
  renderVpn(); renderVpnTiles();
  try { await invoke('vpn_tailscale', { action: up ? 'up' : 'down' }); }
  finally { S.vpnBusy = ''; await loadVpn(); }
}
$('#tsToggle').addEventListener('click', () => { note('#vpnNote', ''); tsToggle().catch(err => note('#vpnNote', String(err))); });
$('#tsExit').addEventListener('change', e => {
  note('#vpnNote', '');
  invoke('vpn_tailscale', { action: 'exit', arg: e.target.value }).catch(err => note('#vpnNote', String(err))).finally(loadVpn);
});
$('#rvOpen').addEventListener('click', () => { invoke('vpn_radmin_open').then(() => closePanel(true)).catch(err => note('#vpnNote', String(err))); });
// Click an address (yours or a device's) to copy it.
$('#v-network').addEventListener('click', e => {
  const el = e.target.closest('[data-copy]'); if (!el || !el.dataset.copy) return;
  invoke('copy_text', { text: el.dataset.copy }).then(() => {
    el.classList.remove('copied'); void el.offsetWidth; el.classList.add('copied');
    clearTimeout(el._t); el._t = setTimeout(() => el.classList.remove('copied'), 1300);
  }).catch(err => note('#vpnNote', String(err)));
});

/* ---------- controls ---------- */
/* ---------- Discord (voice call, notifications, Rich Presence; see discord.rs) ---------- */
S.dc = null; S.dcEdit = false; S.dcAutoAuth = false; S.dcOrder = []; S.callW = 0;
const DC_CDN = /^https:\/\/(cdn\.discordapp\.com|media\.discordapp\.net)\//;
const dcImg = (m, cls) => `<img${cls ? ` class="${cls}"` : ''} src="${esc(DC_CDN.test(m.avatar) ? m.avatar : '')}" alt="" data-id="${esc(m.id)}" referrerpolicy="no-referrer">`;
const clipTxt = (s, n) => { s = String(s || '').replace(/\s+/g, ' ').trim(); return s.length > n ? s.slice(0, n - 1) + '…' : s; };
async function loadDc() {
  try { S.dc = await invoke('discord_status'); } catch (e) { return; }
  renderDc(); renderDcCall();
}
function dcState(d) {
  if (!d.connected) return 'Discord isn’t open';
  if (!d.authorized) return d.has_secret ? 'Connected · not authorized yet' : 'Connected · status only';
  return 'Signed in';
}
function renderDc() {
  const d = S.dc || {}, setup = !d.configured || S.dcEdit;
  $('#dcSetup').hidden = !setup; $('#dcMain').hidden = setup;
  const n = $('#dcNote'); n.textContent = d.error || ''; n.hidden = !d.error;
  if (setup) {
    const id = $('#dcId');
    if (!id.value && d.client_id) id.value = d.client_id;
    $('#dcSecret').placeholder = d.has_secret ? 'Client Secret (saved; blank keeps it)' : 'Client Secret';
    $('#dcBack').hidden = !d.configured; $('#dcForget').hidden = !d.configured;
  } else {
    const me = d.user, v = d.voice;
    setHtml($('#dcMe'), `${me ? dcImg(me, 'dc-av') : `<span class="vpn-logo">${icon('chat')}</span>`}<div class="vpn-title"><b>${esc(me ? me.name : 'Discord')}</b><small>${esc(dcState(d))}</small></div>`);
    $('.dc-side').classList.toggle('on', !!d.authorized);
    let html;
    if (!d.connected) html = `<div class="empty">${icon('chat')}<span>Open Discord to see your calls here</span><button class="btn" type="button" data-dc="open">Open Discord</button></div>`;
    else if (!d.authorized) html = d.has_secret
      ? `<div class="empty">${icon('headphones')}<span>Let the island see your calls and notifications.<br>Discord will ask you to confirm.</span><button class="btn primary" type="button" data-dc="authorize">Authorize</button></div>`
      : `<div class="empty">${icon('headphones')}<span>Add your app’s Client Secret to see calls and notifications</span><button class="btn" type="button" data-dc="edit">Add the secret</button></div>`;
    else if (!v) html = `<div class="empty">${icon('headphones')}<span>You’re not in a voice channel</span></div>`;
    else html = `<div class="dc-vh"><span class="vpn-logo">${icon('headphones')}</span><div class="vpn-title"><b>${esc(v.name)}</b><small>${esc(v.guild || 'Voice')} · ${v.members.length} ${v.members.length === 1 ? 'person' : 'people'}</small></div></div>`
      + `<div class="dc-members">${v.members.map(m => `<div class="dc-m">${dcImg(m, 'dc-av')}<b>${esc(m.name)}</b>${m.deaf ? icon('headoff') : m.mute ? icon('micoff') : ''}</div>`).join('')}</div>`;
    setHtml($('#dcVoice'), html);
    $('#dcCtl').hidden = !d.authorized;
    const mute = $('#dcMute'), deaf = $('#dcDeaf');
    mute.setAttribute('aria-pressed', !!d.mute); deaf.setAttribute('aria-pressed', !!d.deaf);
    setHtml(mute, `${icon(d.mute ? 'micoff' : 'mic')}<span>${d.mute ? 'Unmute' : 'Mute'}</span>`);
    setHtml(deaf, `${icon(d.deaf ? 'headoff' : 'headphones')}<span>${d.deaf ? 'Undeafen' : 'Deafen'}</span>`);
    $('#dcLeave').disabled = !v;
    $('#dcPresence').checked = !!d.presence; $('#dcNotify').checked = !!d.notify;
    txt($('#dcNow'), !d.presence || !d.connected ? '' : d.presence_now ? `Showing ${d.presence_now}` : 'Shows while Apple Music plays');
    dcTalk();
  }
  if (S.mode === 'panel' && S.tab === 'discord') layout();
}
/** Extra pill width for the call avatars (and the muted mic). */
function callW() { return S.callW || 0; }
/** Up to three people for the pill: the order sticks, except someone out of sight who starts talking moves up. */
function dcVisible(v) {
  const ids = v.members.map(m => m.id);
  S.dcOrder = S.dcOrder.filter(id => ids.includes(id));
  for (const id of ids) if (!S.dcOrder.includes(id)) S.dcOrder.push(id);
  return S.dcOrder.slice(0, 3).map(id => v.members.find(m => m.id === id));
}
function renderDcCall() {
  const d = S.dc, v = d && d.voice, on = !!(v && v.members.length);
  island.classList.toggle('in-call', on);
  const pill = $('#pillCall'), chip = $('#barDc');
  pill.hidden = !on; chip.hidden = !on;
  let w = 0;
  if (on) {
    const ms = dcVisible(v), self = d.deaf ? icon('headoff') : d.mute ? icon('micoff') : '';
    const html = `<span class="dc-stack">${ms.map(m => dcImg(m)).join('')}</span>${self}`;
    setHtml(pill, html); setHtml(chip, html);
    chip.title = `${v.name}${v.guild ? ' · ' + v.guild : ''} (Discord)`;
    w = 9 + 18 + (ms.length - 1) * 12 + (self ? 17 : 0);
    dcTalk();
  }
  if (w !== S.callW) { S.callW = w; layout(); }
}
/** Green rings on whoever is talking (changes many times a second, so only classes flip). */
function dcTalk() {
  const v = S.dc && S.dc.voice; if (!v) return;
  const talking = new Set(v.members.filter(m => m.speaking).map(m => m.id));
  for (const img of $$('#pillCall img, #barDc img, #dcVoice .dc-av')) img.classList.toggle('talk', talking.has(img.dataset.id));
}
listen('discord:status', e => {
  const prev = S.dc, was = prev && prev.voice;
  S.dc = e.payload;
  const now = S.dc.voice;
  if (prev && now && (!was || was.channel_id !== now.channel_id)) toast(clipTxt(now.name, 40), 'Joined voice', 'headphones', { sound: 'voice' });
  else if (prev && was && !now && S.dc.connected) toast(clipTxt(was.name, 40), 'Left voice', 'phoneoff', { sound: 'voice' });
  if (S.dcAutoAuth && S.dc.connected && !S.dc.authorized && S.dc.has_secret && !S.dc.error) { S.dcAutoAuth = false; dcAction('authorize'); }
  renderDc(); renderDcCall();
});
listen('discord:speaking', e => {
  const { id, speaking } = e.payload, v = S.dc && S.dc.voice; if (!v) return;
  const m = v.members.find(x => x.id === id); if (!m) return;
  m.speaking = speaking;
  const i = S.dcOrder.indexOf(id);
  if (speaking && i >= 3) { S.dcOrder.splice(i, 1); S.dcOrder.unshift(id); renderDcCall(); } else dcTalk();
});
listen('discord:notify', e => {
  const n = e.payload || {};
  if (!S.dc || !S.dc.notify) return;
  const img = DC_CDN.test(n.icon || '') ? n.icon : '';
  // Only @mentions of you get here (discord.rs filters), so they're shown on top of everything.
  toast(clipTxt(n.title || "Discord", 48), clipTxt(n.body, 72), 'chat', { img, round: !!img, ms: 6000, urgent: true, tab: 'discord', sound: 'mention' });
});
function dcAction(action) {
  note('#dcNote', '');
  return invoke('discord_action', { action }).catch(err => note('#dcNote', String(err)));
}
$('#dcPortal').addEventListener('click', () => invoke('discord_open', { what: 'portal' }).catch(err => note('#dcNote', String(err))));
$('#dcForm').addEventListener('submit', e => {
  e.preventDefault();
  const clientId = $('#dcId').value.trim(), secret = $('#dcSecret').value.trim();
  if (!clientId) { note('#dcNote', 'Paste the Client ID first.'); return; }
  note('#dcNote', '');
  invoke('discord_configure', { clientId, secret }).then(() => {
    $('#dcSecret').value = '';
    S.dcEdit = false; S.dcAutoAuth = !!secret || !!(S.dc && S.dc.has_secret);
    loadDc();
  }).catch(err => note('#dcNote', String(err)));
});
$('#dcBack').addEventListener('click', () => { S.dcEdit = false; $('#dcSecret').value = ''; renderDc(); });
$('#dcEdit').addEventListener('click', () => { S.dcEdit = true; renderDc(); });
$('#dcOpen').addEventListener('click', () => invoke('discord_open', { what: 'app' }).then(() => closePanel(true)).catch(err => note('#dcNote', String(err))));
$('#dcForget').addEventListener('click', () => {
  // Second click within 3 s confirms.
  const b = $('#dcForget');
  if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget this app'; }, 3000); return; }
  delete b.dataset.sure; b.textContent = 'Forget this app';
  $('#dcId').value = ''; $('#dcSecret').value = ''; S.dcEdit = false;
  dcAction('forget').then(loadDc);
});
$('#dcVoice').addEventListener('click', e => {
  const b = e.target.closest('[data-dc]'); if (!b) return;
  if (b.dataset.dc === 'open') $('#dcOpen').click();
  else if (b.dataset.dc === 'edit') { S.dcEdit = true; renderDc(); }
  else dcAction(b.dataset.dc);
});
$('#dcMute').addEventListener('click', () => { if (S.dc) { S.dc.mute = !S.dc.mute; renderDc(); renderDcCall(); dcAction(S.dc.mute ? 'mute' : 'unmute'); } });
$('#dcDeaf').addEventListener('click', () => { if (S.dc) { S.dc.deaf = !S.dc.deaf; renderDc(); renderDcCall(); dcAction(S.dc.deaf ? 'deafen' : 'undeafen'); } });
$('#dcLeave').addEventListener('click', () => dcAction('leave'));
$('#dcPresence').addEventListener('change', e => invoke('discord_options', { presence: e.target.checked }).catch(err => note('#dcNote', String(err))));
$('#dcNotify').addEventListener('change', e => invoke('discord_options', { notify: e.target.checked }).catch(err => note('#dcNote', String(err))));

/* ---------- Destiny 2 (Bungie API with the user's own key; see destiny.rs) ---------- */
S.d2 = null; S.d2Profile = null; S.d2Xur = null; S.d2Sel = store.get('d2Sel', ''); S.d2Edit = false; S.d2T = 0;
/** Daily reset is 17:00 UTC; the weekly one is Tuesday's; Xûr arrives at Friday's. */
function nextReset(day) {
  const now = new Date(), d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 17));
  if (d <= now) d.setUTCDate(d.getUTCDate() + 1);
  if (day != null) while (d.getUTCDay() !== day) d.setUTCDate(d.getUTCDate() + 1);
  return d;
}
const until = d => { const m = Math.max(0, Math.round((d - Date.now()) / 60000)); const days = Math.floor(m / 1440), h = Math.floor(m % 1440 / 60); return days ? `${days}d ${h}h` : h ? `${h}h ${m % 60}m` : `${m}m`; };
/** Playing right now: an activity, started in the last 12 hours. */
const d2Live = c => !!c.activity && Date.now() - Date.parse(c.activity_since) < 12 * 3600e3;
async function d2Loop() {
  clearTimeout(S.d2T);
  if (S.mode !== 'panel' || S.tab !== 'destiny') return;
  try { S.d2 = await invoke('destiny_status'); } catch (e) { return; }
  if (S.d2.configured && !S.d2Edit) {
    if (!S.d2Profile) { renderD2(); $('#d2Wait').hidden = false; layout(); }
    const [p, x] = await Promise.allSettled([invoke('destiny_profile'), invoke('destiny_xur')]);
    $('#d2Wait').hidden = true;
    if (p.status === 'fulfilled') S.d2Profile = p.value;
    if (x.status === 'fulfilled') S.d2Xur = x.value;
    const err = p.status === 'rejected' ? p.reason : x.status === 'rejected' ? x.reason : '';
    note('#d2Note', err ? String(err) : '');
  }
  renderD2();
  S.d2T = setTimeout(d2Loop, 60000);
}
function renderD2() {
  const st = S.d2 || {}, setup = !st.configured || S.d2Edit;
  $('#d2Setup').hidden = !setup; $('#d2Main').hidden = setup || !S.d2Profile;
  if (setup) {
    const who = $('#d2Who'); if (!who.value && st.name) who.value = st.name;
    $('#d2Key').placeholder = st.configured ? 'API key (saved; blank keeps it)' : 'API key';
    $('#d2Back').hidden = !st.configured; $('#d2Forget').hidden = !st.configured;
    if (S.mode === 'panel' && S.tab === 'destiny') layout();
    return;
  }
  const p = S.d2Profile; if (!p) return;
  txt($('#d2Title'), p.name.replace(/#\d+$/, ''));
  txt($('#d2Rank'), p.guardian_rank ? `Guardian Rank ${p.guardian_rank}` : '');
  setHtml($('#d2Daily'), `Daily reset <b>${until(nextReset())}</b>`);
  setHtml($('#d2Weekly'), `Weekly <b>${until(nextReset(2))}</b>`);
  if (!p.characters.some(c => c.id === S.d2Sel)) S.d2Sel = (p.characters.find(d2Live) || p.characters[0] || {}).id || '';
  setHtml($('#d2Chars'), p.characters.map(c => `<button class="d2-char" type="button" data-id="${esc(c.id)}" aria-pressed="${c.id === S.d2Sel}" style="background-image:url(${esc(JSON.stringify(c.emblem_bg))})"><span><b>${esc(c.class)}</b><small class="${d2Live(c) ? 'd2-now' : ''}">${esc(d2Live(c) ? '● ' + c.activity : [c.race, ago(c.last_played)].filter(Boolean).join(' · '))}</small></span><span class="d2-light">✦${c.light}</span></button>`).join(''));
  const c = p.characters.find(x => x.id === S.d2Sel);
  if (c) {
    const sub = c.items.find(i => i.slot === 'Subclass'), weapons = c.items.filter(i => ['Kinetic', 'Energy', 'Power'].includes(i.slot)), armor = c.items.filter(i => ['Helmet', 'Arms', 'Chest', 'Legs', 'Class item'].includes(i.slot));
    const ic = i => `<img class="d2-ic${i.exotic ? ' exotic' : ''}" src="${esc(i.icon)}" alt="" loading="lazy">`;
    setHtml($('#d2Load'), `<header><b>${esc(c.class)} loadout</b><small>${Math.round(c.minutes_played / 60).toLocaleString()} hours played</small></header>`
      + (sub ? `<div class="d2-sub"><img src="${esc(sub.icon)}" alt=""><div><b>${esc(sub.name)}</b><small>Subclass</small></div></div>` : '')
      + `<div class="d2-label">Weapons</div><div class="d2-items">${weapons.map(i => `<div class="d2-item" title="${esc(i.name)}">${ic(i)}<div><b>${esc(i.name)}</b><small>${esc(i.slot)} · ${esc(i.kind)}</small></div><em>${i.power || ''}</em></div>`).join('')}</div>`
      + `<div class="d2-label">Armor</div><div class="d2-armor">${armor.map(i => `<div title="${esc(i.name)} (${esc(i.slot)})">${ic(i)}<em>${i.power || ''}</em></div>`).join('')}</div>`);
  }
  const x = S.d2Xur;
  if (!x) setHtml($('#d2Xur'), '');
  else if (x.here) {
    // Bungie's own refresh date when it gives one (Xûr now sells all week); otherwise the weekly reset.
    const refresh = Date.parse(x.leaves) > Date.now() ? new Date(x.leaves) : nextReset(2);
    setHtml($('#d2Xur'), `<header><b>Xûr${x.location ? ' · ' + esc(x.location) : ''}</b><small>new stock in ${until(refresh)}</small></header>`
      + (x.items.length ? `<div class="d2-armor">${x.items.map(i => `<div title="${esc(i.name)} (${esc(i.kind)})">${ic2(i)}</div>`).join('')}</div>` : '<small class="d2-label">Exotics not listed</small>'));
  }
  else setHtml($('#d2Xur'), `<header><b>Xûr</b><small>arrives in ${until(nextReset(5))}</small></header>`);
  if (S.mode === 'panel' && S.tab === 'destiny') layout();
}
const ic2 = i => `<img class="d2-ic exotic" src="${esc(i.icon)}" alt="" loading="lazy">`;
$('#d2Chars').addEventListener('click', e => {
  const b = e.target.closest('.d2-char'); if (!b) return;
  S.d2Sel = b.dataset.id; store.set('d2Sel', S.d2Sel); renderD2();
});
$('#d2Portal').addEventListener('click', () => invoke('destiny_open').catch(err => note('#d2Note', String(err))));
$('#d2Form').addEventListener('submit', e => {
  e.preventDefault();
  const key = $('#d2Key').value.trim(), name = $('#d2Who').value.trim();
  if (!name) { note('#d2Note', 'Add your Bungie Name, like Guardian#1234.'); return; }
  note('#d2Note', ''); $('#d2Save').disabled = true;
  invoke('destiny_configure', { key, name }).then(st => {
    $('#d2Key').value = ''; S.d2 = st; S.d2Edit = false; S.d2Profile = null; d2Loop();
  }).catch(err => note('#d2Note', String(err))).finally(() => { $('#d2Save').disabled = false; });
});
$('#d2Edit').addEventListener('click', () => { S.d2Edit = true; renderD2(); });
$('#d2Back').addEventListener('click', () => { S.d2Edit = false; $('#d2Key').value = ''; renderD2(); });
$('#d2Forget').addEventListener('click', () => {
  const b = $('#d2Forget');
  if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget'; }, 3000); return; }
  delete b.dataset.sure; b.textContent = 'Forget';
  invoke('destiny_forget').then(() => { S.d2Edit = false; S.d2Profile = null; S.d2Xur = null; $('#d2Who').value = ''; d2Loop(); });
});

/* ---------- Canvas (school LMS, with the user's own token; see canvas.rs) ---------- */
S.cv = null; S.cvData = null; S.cvEdit = false; S.cvT = 0; S.cvSetup = store.get('cvSetup', 'feed');
// Courses without a Canvas color (the calendar feed has none) get one from this set.
const CV_COLORS = ['#4f8cf7', '#e8704a', '#7fd99a', '#c07fe0', '#f5b45b', '#5bc8d8', '#f07aa0', '#a3b86c'];
const cvCourse = id => (S.cvData && S.cvData.courses.find(c => c.id === id)) || {};
const cvColor = id => cvCourse(id).color || (id ? CV_COLORS[id % CV_COLORS.length] : 'var(--faint)');
const KIND = { assignment: 'Assignment', quiz: 'Quiz', discussion_topic: 'Discussion', wiki_page: 'Page', planner_note: 'To-do', calendar_event: 'Event', assessment_request: 'Peer review' };
async function cvLoop() {
  clearTimeout(S.cvT);
  if (S.mode !== 'panel' || S.tab !== 'canvas') return;
  try { S.cv = await invoke('canvas_status'); } catch (e) { return; }
  if (S.cv.configured && !S.cvEdit) {
    if (!S.cvData) { renderCv(); $('#cvWait').hidden = false; layout(); }
    try { S.cvData = await invoke('canvas_overview'); note('#cvNote', ''); } catch (e) { note('#cvNote', String(e)); }
    $('#cvWait').hidden = true;
  }
  renderCv();
  S.cvT = setTimeout(cvLoop, 120000);
}
function cvStatus(t) {
  if (t.graded) return ['Graded', 'ok'];
  if (t.submitted) return [t.late ? 'Submitted late' : 'Submitted', 'ok'];
  if (t.missing) return ['Missing', 'bad'];
  const left = Date.parse(t.due) - Date.now();
  if (left > 0 && left < 864e5 && !t.done) return [left < 36e5 ? `${Math.max(1, Math.round(left / 6e4))} min left` : `${Math.round(left / 36e5)}h left`, 'warn'];
  return null;
}
function cvRow(t, i, list) {
  const d = new Date(t.due), c = cvCourse(t.course_id), st = cvStatus(t);
  const when = isNaN(d) ? '' : t.all_day ? 'All day' : d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const sub = [KIND[t.kind] || '', c.code || c.name || '', list === 'missing' && !isNaN(d) ? d.toLocaleDateString([], { month: 'short', day: 'numeric' }) : when, t.points ? `${t.points} pts` : ''].filter(Boolean).join(' · ');
  return `<div class="cv-row${t.done ? ' done' : ''}" data-list="${list}" data-i="${i}" title="${esc(t.title)}"><button class="cv-check" type="button" aria-pressed="${!!t.done}" title="${t.done ? 'Mark not done' : 'Mark done'}${S.cvData && S.cvData.limited ? ' (on this PC)' : ''}" aria-label="Done">${icon('check')}</button><span class="cv-bar" style="background:${esc(cvColor(t.course_id))}"></span><div><b>${esc(t.title)}</b><small>${esc(sub)}</small></div>${st ? `<span class="cv-chip ${st[1]}">${esc(st[0])}</span>` : '<span></span>'}</div>`;
}
const dayLabel = d => { const a = new Date(); a.setHours(0, 0, 0, 0); const b = new Date(d); b.setHours(0, 0, 0, 0); const n = Math.round((b - a) / 864e5); return n === 0 ? 'Today' : n === 1 ? 'Tomorrow' : n === -1 ? 'Yesterday' : b.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' }); };
function renderCv() {
  const st = S.cv || {}, setup = !st.configured || S.cvEdit;
  $('#cvSetup').hidden = !setup; $('#cvMain').hidden = setup || !S.cvData;
  if (setup) {
    if (st.configured && st.mode) S.cvSetup = st.mode === 'feed' && !S.cvSetupPicked ? 'feed' : S.cvSetup;
    $$('#cvMode button').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === S.cvSetup));
    $$('#cvSetup [data-cv]').forEach(el => { el.hidden = el.dataset.cv !== S.cvSetup; });
    const dom = $('#cvDomain'); if (!dom.value && st.domain) dom.value = st.domain;
    $('#cvFeed').placeholder = st.configured && st.mode === 'feed' ? 'Feed link (saved; paste a new one to replace it)' : 'https://school.instructure.com/feeds/calendars/user_….ics';
    $('#cvToken').placeholder = st.configured ? 'Access token (saved; blank keeps it)' : 'Access token';
    $('#cvBack').hidden = !st.configured; $('#cvForget').hidden = !st.configured;
    if (S.mode === 'panel' && S.tab === 'canvas') layout();
    return;
  }
  const o = S.cvData; if (!o) return;
  txt($('#cvWho'), [o.name, o.domain, o.limited ? 'calendar feed: due dates only' : ''].filter(Boolean).join(' · '));
  $('#cvGrades').hidden = $('#cvAnn').hidden = o.limited;
  $('#cvCourses').hidden = !o.limited;
  if (o.limited) {
    // The feed has no grades: show each course's load for the coming week instead.
    const week = Date.now() + 7 * 864e5;
    setHtml($('#cvCourses'), `<header><b>Courses</b><small>next 7 days</small></header>` + (o.courses.length ? o.courses.map(c => {
      const due = o.upcoming.filter(t => t.course_id === c.id && !t.done && Date.parse(t.due) < week), next = due[0];
      return `<div class="cv-course" data-url="${esc(c.url)}" title="${esc(c.name)}"><span class="cv-bar" style="background:${esc(cvColor(c.id))}"></span><div><b>${esc(c.name)}</b><small>${next ? `next: ${esc(next.title)} · ${esc(dayLabel(next.due))}` : 'nothing due this week'}</small></div><em>${due.length || ''}</em></div>`;
    }).join('') : '<p class="empty">No courses in the feed.</p>'));
  }
  const miss = $('#cvMissing');
  miss.hidden = !o.missing.length;
  setHtml(miss, o.missing.length ? `<header><b>Missing</b><small>${o.missing.length}</small></header><div class="cv-scroll">${o.missing.map((t, i) => cvRow(t, i, 'missing')).join('')}</div>` : '');
  let last = '', rows = '';
  o.upcoming.forEach((t, i) => { const l = dayLabel(t.due); if (l !== last) { rows += `<div class="cv-day">${esc(l)}</div>`; last = l; } rows += cvRow(t, i, 'upcoming'); });
  const open = o.upcoming.filter(t => !t.done && !t.submitted && !t.graded).length;
  setHtml($('#cvUp'), `<header><b>Coming up</b><small>${open ? `${open} to do` : 'All caught up'}</small></header>` + (rows ? `<div class="cv-scroll">${rows}</div>` : '<p class="empty">Nothing due in the next three weeks.</p>'));
  setHtml($('#cvGrades'), `<header><b>Grades</b><small>current</small></header>` + (o.courses.length ? o.courses.map(c => `<div class="cv-course" data-url="${esc(c.url + '/grades')}" title="${esc(c.name)}"><span class="cv-bar" style="background:${esc(c.color || 'var(--accent)')}"></span><div><b>${esc(c.code || c.name)}</b><small>${esc(c.code ? c.name : '')}</small></div><em>${c.score != null ? `${c.score.toFixed(1)}%` : '–'}${c.grade ? `<small> ${esc(c.grade)}</small>` : ''}</em></div>`).join('') : '<p class="empty">No active courses.</p>'));
  setHtml($('#cvAnn'), `<header><b>Announcements</b><small>last 3 weeks</small></header>` + (o.announcements.length ? o.announcements.slice(0, 5).map(a => { const c = cvCourse(a.course_id); return `<div class="cv-an" data-url="${esc(a.url)}"><div><i style="background:${esc(c.color || 'var(--accent)')}"></i><b>${esc(a.title)}</b></div><small>${esc(c.code || c.name || '')} · ${esc(ago(a.posted))}</small>${a.preview ? `<p>${esc(a.preview)}</p>` : ''}</div>`; }).join('') : '<p class="empty">No recent announcements.</p>'));
  if (S.mode === 'panel' && S.tab === 'canvas') layout();
}
$('#cvMain').addEventListener('click', e => {
  const check = e.target.closest('.cv-check'), row = e.target.closest('.cv-row');
  if (check && row) {
    const t = S.cvData[row.dataset.list][+row.dataset.i]; if (!t) return;
    t.done = !t.done; renderCv();
    invoke('canvas_done', { key: t.key, kind: t.kind, id: t.id, overrideId: t.override_id, done: t.done })
      .then(id => { if (id) t.override_id = id; }).catch(err => { t.done = !t.done; renderCv(); note('#cvNote', String(err)); });
    return;
  }
  const url = row ? (S.cvData[row.dataset.list][+row.dataset.i] || {}).url : (e.target.closest('[data-url]') || {}).dataset?.url;
  if (url) invoke('canvas_open', { url }).then(() => closePanel(true)).catch(err => note('#cvNote', String(err)));
});
$('#cvForm').addEventListener('submit', e => {
  e.preventDefault();
  const domain = $('#cvDomain').value.trim(), token = $('#cvToken').value.trim();
  if (!domain) { note('#cvNote', 'Add your school’s Canvas address first.'); return; }
  note('#cvNote', ''); $('#cvSave').disabled = true;
  invoke('canvas_configure', { domain, token }).then(st => {
    $('#cvToken').value = ''; S.cv = st; S.cvEdit = false; S.cvData = null; cvLoop();
  }).catch(err => note('#cvNote', String(err))).finally(() => { $('#cvSave').disabled = false; });
});
$('#cvFeedForm').addEventListener('submit', e => {
  e.preventDefault();
  const url = $('#cvFeed').value.trim();
  if (!url) { note('#cvNote', 'Paste the calendar feed link first.'); return; }
  note('#cvNote', ''); $('#cvFeedSave').disabled = true;
  invoke('canvas_configure_feed', { url }).then(st => {
    $('#cvFeed').value = ''; S.cv = st; S.cvEdit = false; S.cvData = null; cvLoop();
  }).catch(err => note('#cvNote', String(err))).finally(() => { $('#cvFeedSave').disabled = false; });
});
$('#cvMode').addEventListener('click', e => {
  const b = e.target.closest('[data-mode]'); if (!b) return;
  S.cvSetup = b.dataset.mode; S.cvSetupPicked = true; store.set('cvSetup', S.cvSetup); note('#cvNote', ''); renderCv();
});
$('#cvEdit').addEventListener('click', () => { S.cvEdit = true; renderCv(); });
$('#cvBack').addEventListener('click', () => { S.cvEdit = false; $('#cvToken').value = ''; renderCv(); });
$('#cvForget').addEventListener('click', () => {
  const b = $('#cvForget');
  if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget'; }, 3000); return; }
  delete b.dataset.sure; b.textContent = 'Forget';
  invoke('canvas_forget').then(() => { S.cvEdit = false; S.cvData = null; $('#cvDomain').value = ''; $('#cvFeed').value = ''; cvLoop(); });
});
// Due-soon reminders show on top of everything (fullscreen games too); grades and announcements above windows.
listen('canvas:notify', e => {
  const n = e.payload || {};
  toast(clipTxt(n.title, 48), clipTxt(n.body, 72), 'cap', { urgent: !!n.urgent, top: true, ms: n.urgent ? 8000 : 5500, tab: 'canvas', sound: n.urgent ? 'urgent' : 'canvas' });
});

function note(id, msg) { const n = $(id); n.textContent = msg || ''; n.hidden = !msg; layout(); }
function renderControls() {
  const st = S.status || { wifi: {}, bluetooth: {}, net: {} };
  const set = (k, pressed, sub) => { const b = $(`.tile[data-tile="${k}"]`); b.setAttribute('aria-pressed', !!pressed); txt(b.querySelector('small'), sub); };
  if (st.wifi.present) {
    txt($('#netTitle'), 'Wi-Fi');
    setHtml($('#netIco'), icon(st.wifi.on ? 'wifi' : 'wifioff'));
    set('wifi', st.wifi.on, !st.wifi.on ? 'Off' : st.net.kind === 'wifi' ? st.net.name || 'Connected' : 'Not connected');
  } else {
    txt($('#netTitle'), st.net.kind === 'ethernet' ? 'Ethernet' : 'Network');
    setHtml($('#netIco'), icon(st.net.kind === 'ethernet' ? 'ethernet' : 'wifioff'));
    set('wifi', st.net.kind !== 'none', st.net.kind === 'none' ? 'Offline' : st.net.name || 'Connected');
  }
  set('bluetooth', st.bluetooth.on, st.bluetooth.present ? (st.bluetooth.on ? 'On' : 'Off') : 'Not available');
  const a = S.audio;
  set('sound', a && !a.muted && a.level > 0, a ? a.device || 'Default device' : 'No output');
  const dev = (a && a.device || '').toLowerCase();
  setHtml($('#soundIco'), icon(dev.includes('headphone') || dev.includes('headset') ? 'headphones' : dev.includes('monitor') || dev.includes('display') ? 'monitor' : 'speaker'));
  set('dark', S.dark, S.dark ? 'On' : 'Off');
  set('taskbar', st.taskbar_hidden, st.taskbar_hidden ? 'Hidden' : 'Visible');
  set('night', false, 'Opens Settings');
  set('sounds', sounds.on, sounds.on ? `On · ${Math.round(sounds.vol * 100)}%` : 'Off');
  renderVpnTiles(); renderGaze(); renderAudioMaster();
  if (a) {
    const v = Math.round(a.level * 100), input = $('#vol');
    if (document.activeElement !== input || performance.now() - S.volSetAt > 1500) input.value = v;
    input.style.setProperty('--p', (a.muted ? 0 : +input.value) + '%');
    txt($('#volOut'), a.muted ? 'Muted' : input.value + '%');
    const mb = $('#muteBtn'); setHtml(mb, icon(a.muted || a.level === 0 ? 'mute' : 'speaker')); mb.classList.toggle('muted', a.muted);
    mb.setAttribute('aria-label', a.muted ? 'Unmute' : 'Mute');
  }
}
function openSettings(uri) { closePanel(true); quiet(invoke('open_settings', { uri })); }
$$('.tile').forEach(b => b.addEventListener('click', async () => {
  const st = S.status; const k = b.dataset.tile;
  note('#ctlNote', '');
  try {
    if (k === 'wifi') {
      if (st && st.wifi.present) { st.wifi.on = !st.wifi.on; renderControls(); await invoke('set_radio', { kind: 'wifi', on: st.wifi.on }); }
      else openSettings('ms-settings:network-status');
    } else if (k === 'bluetooth') {
      if (st && st.bluetooth.present) { st.bluetooth.on = !st.bluetooth.on; renderControls(); await invoke('set_radio', { kind: 'bluetooth', on: st.bluetooth.on }); }
      else openSettings('ms-settings:bluetooth');
    } else if (k === 'sound') setTab('audio');
    else if (k === 'night') openSettings('ms-settings:nightlight');
    else if (k === 'sounds') { sounds.set({ on: !sounds.on }); if (sounds.on) sounds.sound('glass'); renderControls(); renderSounds(); }
    else if (k === 'dark') { S.dark = !S.dark; store.set('dark', S.dark); applySkin(); renderControls(); }
    else if (k === 'tailscale') await tsToggle();
    else if (k === 'eye' && S.gazeSt) { S.gazeSt = await invoke('gaze_set', { enabled: !S.gazeSt.enabled }); renderControls(); }
    else if (k === 'radmin') setTab('network');
    else if (k === 'taskbar' && st) { st.taskbar_hidden = !st.taskbar_hidden; renderControls(); await invoke('set_taskbar', { hidden: st.taskbar_hidden }); }
  } catch (err) { note('#ctlNote', String(err)); }
}));
let volTimer = 0, volPending = null;
$('#vol').addEventListener('input', e => {
  const v = +e.target.value;
  S.audio = { ...(S.audio || { device: '' }), level: v / 100, muted: false }; S.volSetAt = performance.now();
  renderControls();
  volPending = v / 100;
  if (!volTimer) volTimer = setTimeout(() => { volTimer = 0; if (volPending != null) { quiet(invoke('set_volume', { level: volPending })); volPending = null; } }, 40);
});
$('#muteBtn').addEventListener('click', () => {
  if (!S.audio) return;
  S.audio.muted = !S.audio.muted; S.volSetAt = performance.now(); renderControls();
  quiet(invoke('set_mute', { muted: S.audio.muted }));
});

/* ---------- today ---------- */
function isoWeek(d) { const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate())); const day = t.getUTCDay() || 7; t.setUTCDate(t.getUTCDate() + 4 - day); const y0 = new Date(Date.UTC(t.getUTCFullYear(), 0, 1)); return Math.ceil(((t - y0) / 864e5 + 1) / 7); }
let weekDay = -1;
function renderWeek() {
  const d = new Date(), dow = (d.getDay() + 6) % 7, mon = new Date(d); mon.setDate(d.getDate() - dow);
  const sun = new Date(mon); sun.setDate(mon.getDate() + 6);
  const md = x => x.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  $('#weekRange').textContent = `${md(mon)} – ${md(sun)}`;
  let html = '';
  for (let i = 0; i < 7; i++) {
    const day = new Date(mon); day.setDate(mon.getDate() + i);
    const cls = ['day', i === dow ? 'is-today' : '', i < dow ? 'past' : ''].join(' ');
    html += `<div class="${cls}"><small>${day.toLocaleDateString(undefined, { weekday: 'short' }).slice(0, 3)}</small><b>${day.getDate()}</b></div>`;
  }
  $('#days').innerHTML = html;
  const end = new Date(d.getFullYear(), 11, 31), left = Math.round((end - new Date(d.getFullYear(), d.getMonth(), d.getDate())) / 864e5);
  $('#weekFoot').textContent = `Week ${isoWeek(d)} · ${left} day${left === 1 ? '' : 's'} left in ${d.getFullYear()}`;
  weekDay = d.getDate();
}

const WMO = [[[0], 'Clear', 'sun'], [[1, 2], 'Partly cloudy', 'cloudsun'], [[3], 'Overcast', 'cloud'], [[45, 48], 'Fog', 'fog'], [[51, 53, 55, 56, 57], 'Drizzle', 'rain'], [[61, 63, 65, 66, 67], 'Rain', 'rain'], [[71, 73, 75, 77], 'Snow', 'snow'], [[80, 81, 82], 'Showers', 'rain'], [[85, 86], 'Snow showers', 'snow'], [[95, 96, 99], 'Thunderstorm', 'storm']];
const wmo = c => { const f = WMO.find(w => w[0].includes(c)); return f ? [f[1], f[2]] : ['Weather', 'cloud']; };
function renderWeather(d) {
  const conv = c => Math.round(S.unit === 'F' ? c * 9 / 5 + 32 : c);
  const [label, ic] = wmo(d.code);
  $('#wIco').innerHTML = icon(ic);
  $('#wTemp').textContent = `${conv(d.temp)}°${S.unit}`;
  $('#wDesc').textContent = label;
  $('#wRange').textContent = `H ${conv(d.hi)}° · L ${conv(d.lo)}°`;
  $('#wCity').textContent = d.name;
}
async function loadWeather(force) {
  const cache = store.get('weather', null);
  if (!force && cache && cache.city === S.city && Date.now() - cache.t < 20 * 60e3) { renderWeather(cache); return; }
  $('#wCity').textContent = S.city;
  try {
    let geo = store.get('geo', null);
    if (!geo || geo.q !== S.city) {
      const r = await fetch(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(S.city)}&count=1&language=en&format=json`);
      const g = ((await r.json()).results || [])[0];
      if (!g) throw new Error('nocity');
      geo = { q: S.city, name: g.name, lat: g.latitude, lon: g.longitude };
      store.set('geo', geo);
    }
    const r = await fetch(`https://api.open-meteo.com/v1/forecast?latitude=${geo.lat}&longitude=${geo.lon}&current=temperature_2m,weather_code&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1`);
    const j = await r.json();
    const data = { city: S.city, name: geo.name, t: Date.now(), temp: j.current.temperature_2m, code: j.current.weather_code, hi: j.daily.temperature_2m_max[0], lo: j.daily.temperature_2m_min[0] };
    store.set('weather', data); renderWeather(data);
  } catch (e) {
    $('#wDesc').textContent = e.message === 'nocity' ? 'City not found' : 'Weather unavailable';
    if (cache && cache.city === S.city) renderWeather(cache);
  }
}
$('#wTemp').addEventListener('click', () => { S.unit = S.unit === 'F' ? 'C' : 'F'; store.set('unit', S.unit); const c = store.get('weather', null); if (c) renderWeather(c); });
$('#wCity').addEventListener('click', () => { const inp = $('#wInput'); $('#wCity').hidden = true; inp.hidden = false; inp.value = S.city; inp.focus(); inp.select(); layout(); });
$('#wInput').addEventListener('keydown', e => {
  if (e.key === 'Enter') { const v = e.target.value.trim(); if (v) { S.city = v; store.set('city', v); loadWeather(true); } e.target.blur(); }
  else if (e.key === 'Escape') { e.stopPropagation(); e.target.blur(); }
});
$('#wInput').addEventListener('blur', () => { $('#wInput').hidden = true; $('#wCity').hidden = false; layout(); });

function renderTimer() {
  const T = S.timer, tot = T.min * 60, frac = T.left / tot;
  const mm = String(Math.floor(T.left / 60)).padStart(2, '0') + ':' + String(T.left % 60).padStart(2, '0');
  $('#focusTxt').textContent = mm; $('#focusRing').style.strokeDashoffset = (213.63 * (1 - frac)).toFixed(2);
  $('#pillTimerTxt').textContent = mm; $('#pillRing').style.strokeDashoffset = (50.27 * (1 - frac)).toFixed(2);
  $('#focusGo').textContent = T.running ? 'Pause' : (T.left < tot ? 'Resume' : 'Start');
  $('#focusState').textContent = T.running ? 'Running' : (T.left < tot ? 'Paused' : 'Ready');
  $$('#presets .chip').forEach(b => b.setAttribute('aria-pressed', +b.dataset.min === T.min));
  $('#pillTimer').hidden = !T.running;
  island.classList.toggle('timing', T.running);
}
const saveTimer = () => store.set('timer', S.timer);
$('#presets').addEventListener('click', e => {
  const b = e.target.closest('.chip'); if (!b || S.timer.running) return;
  S.timer.min = +b.dataset.min; S.timer.left = S.timer.min * 60; saveTimer(); renderTimer();
});
$('#focusGo').addEventListener('click', () => {
  const T = S.timer;
  if (T.running) { T.left = Math.max(0, Math.ceil((T.end - Date.now()) / 1000)); T.running = false; }
  else { T.running = true; T.end = Date.now() + T.left * 1000; }
  saveTimer(); renderTimer(); layout();
});
$('#focusReset').addEventListener('click', () => { S.timer.running = false; S.timer.left = S.timer.min * 60; saveTimer(); renderTimer(); layout(); });
setInterval(() => {
  const T = S.timer; if (!T.running) return;
  T.left = Math.max(0, Math.ceil((T.end - Date.now()) / 1000));
  if (T.left <= 0) {
    T.running = false; T.left = T.min * 60; saveTimer(); renderTimer(); layout();
    toast('Focus session done', `${T.min} minutes. Take a short break.`, 'timer', { ms: 4000, sound: 'timer' });
    return;
  }
  renderTimer();
}, 250);

function renderTodos() {
  $('#todoList').innerHTML = S.todos.map(t => `<li class="${t.d ? 'done' : ''}" data-id="${t.id}"><button class="chk" type="button" aria-label="${t.d ? 'Mark not done' : 'Mark done'}">${icon('check')}</button><span class="lbl">${esc(t.t)}</span><button class="ib del" type="button" aria-label="Delete">${icon('x')}</button></li>`).join('')
    || '<li class="empty">Nothing left. Add something below.</li>';
  $('#todoCount').textContent = `${S.todos.filter(t => t.d).length}/${S.todos.length}`;
  store.set('todos', S.todos);
}
$('#todoList').addEventListener('click', e => {
  const li = e.target.closest('li[data-id]'); if (!li) return;
  const id = +li.dataset.id, t = S.todos.find(x => x.id === id);
  if (e.target.closest('.chk')) { t.d = !t.d; li.classList.toggle('done', t.d); $('#todoCount').textContent = `${S.todos.filter(x => x.d).length}/${S.todos.length}`; store.set('todos', S.todos); }
  else if (e.target.closest('.del')) { S.todos = S.todos.filter(x => x.id !== id); renderTodos(); }
});
$('#todoForm').addEventListener('submit', e => {
  e.preventDefault(); const inp = $('#todoInput'), v = inp.value.trim(); if (!v) return;
  S.todos.push({ id: Date.now(), t: v.slice(0, 120), d: false }); inp.value = ''; renderTodos();
});

/* ---------- stats ---------- */
let statsT;
function statsLoop() {
  clearTimeout(statsT);
  if (S.mode !== 'panel' || S.tab !== 'stats') return;
  invoke('get_stats').then(s => { S.stats = s; renderStats(); }).catch(() => {}).finally(() => { statsT = setTimeout(statsLoop, 1500); });
}
const fmtRate = b => b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB/s` : `${Math.round(b / 1e3)} KB/s`;
const fmtUp = s => { const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60); return d ? `${d}d ${h}h ${m}m` : `${h}h ${m}m`; };
function renderStats() {
  const s = S.stats; if (!s) return;
  const set = (g, v, sub) => {
    const el = $(`.gauge[data-g="${g}"]`);
    el.querySelector('.val').style.strokeDashoffset = v == null ? 201.06 : (201.06 * (1 - clamp(v, 0, 100) / 100)).toFixed(1);
    el.querySelector('.ring span').textContent = v == null ? '–' : Math.round(v) + '%';
    el.querySelector('small').textContent = sub; el.querySelector('small').title = sub;
  };
  set('cpu', s.cpu, s.cpu_name);
  set('gpu', s.gpu, s.vram_total > 0 ? `${s.vram_used.toFixed(1)} / ${Math.round(s.vram_total)} GB VRAM` : s.gpu_name || 'No data');
  set('ram', s.ram_total ? s.ram_used / s.ram_total * 100 : null, s.ram_total ? `${s.ram_used.toFixed(1)} / ${Math.round(s.ram_total)} GB` : '');
  $('#sysLine').textContent = [`${s.cores} threads`, s.gpu_name].filter(Boolean).join(' · ');
  const sv = $('#spark'), W = Math.max(120, Math.round(sv.getBoundingClientRect().width) || 300), H = 64, hist = s.cpu_hist.length ? s.cpu_hist : [0];
  const stepX = W / 49;
  sv.setAttribute('viewBox', `0 0 ${W} ${H}`);
  const pts = hist.map((v, i) => [W - (hist.length - 1 - i) * stepX, H - 4 - v / 100 * (H - 10)]);
  const line = pts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join('');
  const last = pts[pts.length - 1];
  sv.innerHTML = `<g class="grid">${[.25, .5, .75].map(f => `<line x1="0" x2="${W}" y1="${(H * f).toFixed(1)}" y2="${(H * f).toFixed(1)}"/>`).join('')}</g><path class="area" d="${line}L${last[0].toFixed(1)} ${H}L${pts[0][0].toFixed(1)} ${H}Z"/><path class="line" d="${line}"/><circle class="dot" cx="${last[0].toFixed(1)}" cy="${last[1].toFixed(1)}" r="3.5"/>`;
  $('#sparkNow').textContent = Math.round(s.cpu) + '% now';
  const list = $('#procs');
  while (list.children.length < 5) list.insertAdjacentHTML('beforeend', '<li><span class="ap"></span><span></span><em></em><span class="meter"><i></i></span></li>');
  [...list.children].forEach((li, i) => {
    const p = s.top[i]; li.hidden = !p; if (!p) return;
    const ap = li.querySelector('.ap'); ap.textContent = p.name.replace(/[^A-Za-z0-9]/g, '').slice(0, 2) || '?'; ap.style.background = gradFor(p.name);
    li.children[1].textContent = p.name; li.querySelector('em').textContent = p.cpu.toFixed(1) + '%';
    li.querySelector('.meter i').style.width = Math.min(100, p.cpu * 3) + '%';
  });
  const disks = [...s.disks].sort((a, b) => (a.letter === 'C:' ? -1 : b.letter === 'C:' ? 1 : b.total - a.total)).slice(0, 2);
  const fmtGB = g => g >= 1000 ? `${(g / 1024).toFixed(2)} TB` : `${Math.round(g)} GB`;
  $('#minis').innerHTML = disks.map(d => `<div class="card mini"><small>${icon('drive')}${esc(d.letter)} ${esc(d.label || 'Local Disk')}</small><b>${fmtGB(d.used)} / ${fmtGB(d.total)}</b><div class="bar"><i style="width:${(d.used / d.total * 100).toFixed(1)}%"></i></div></div>`).join('')
    + `<div class="card mini"><small>${icon('down')}Network</small><b>↓ ${fmtRate(s.net_down)}<br>↑ ${fmtRate(s.net_up)}</b></div>`
    + `<div class="card mini"><small>${icon('clock')}Uptime</small><b>${fmtUp(s.uptime)}</b></div>`;
  $$('[data-uptime]').forEach(e => { e.textContent = fmtUp(s.uptime); });
  renderSensors(s);
}
/* Temperatures, fans and clocks that Windows reports itself (sensors.rs; no HWiNFO). */
const heat = (t, warn) => t >= warn ? 'hot' : t >= warn - 12 ? 'warm' : '';
const CPU_WARN = 90, GPU_WARN = 90;
const shortCpu = n => (n || 'CPU').replace(/^(AMD|Intel)\s+/, '').replace(/\s+(w\/|with)\s.*$/, '').replace(/\s+\d+-Core Processor$/, '').replace(/^\d+th Gen\s+/, '');
function renderSensors(s) {
  const z = s.sensors || {}, gpus = z.gpus || [], drives = z.drives || [];
  const gpu = gpus.find(g => !g.integrated) || null;
  const tag = (g, t, warn) => { const el = $(`.gauge[data-g="${g}"] .g-temp`); el.hidden = t == null; if (t != null) { txt(el, Math.round(t) + '°'); el.className = 'g-temp ' + heat(t, warn); } };
  tag('cpu', z.cpu_temp, CPU_WARN);
  tag('gpu', gpu && gpu.temp, GPU_WARN);
  const rows = [];
  const ghz = z.cpu_mhz ? ` · ${(z.cpu_mhz / 1000).toFixed(2)} GHz` : '';
  if (z.cpu_temp != null) rows.push({ ic: 'cpu', name: shortCpu(s.cpu_name), sub: `CPU${ghz}`, t: z.cpu_temp, warn: CPU_WARN, tip: `Measured by the ${z.cpu_temp_from}` });
  for (const g of gpus.filter(g => !g.integrated && g.temp != null)) rows.push({ ic: 'monitor', name: g.name.replace(/^AMD |^NVIDIA /, ''), sub: 'GPU', t: g.temp, warn: GPU_WARN, tip: '' });
  for (const d of drives) rows.push({ ic: 'drive', name: d.name, sub: d.letters.join(' ') || 'SSD', t: d.temp, warn: d.warning, tip: d.hottest > d.temp ? `Hottest sensor ${d.hottest}° (its controller); warning at ${d.warning}°` : `Warning at ${d.warning}°` });
  $('#thermCard').hidden = !rows.length;
  setHtml($('#therms'), rows.map(r => {
    const pct = clamp((r.t - 20) / (r.warn - 20) * 100, 2, 100), h = heat(r.t, r.warn);
    return `<li title="${esc(r.tip)}"><span class="th-ic">${icon(r.ic)}</span><b>${esc(r.name)}<small>${esc(r.sub)}</small></b><em class="${h}">${Math.round(r.t)}°</em><span class="meter"><i class="${h}" style="width:${pct.toFixed(0)}%"></i></span></li>`;
  }).join(''));
  $('#gpuCard').hidden = !gpu;
  if (gpu) {
    txt($('#gpuCardName'), gpu.name.replace(/^AMD |^NVIDIA /, ''));
    txt($('#gpuCardTemp'), gpu.temp != null ? Math.round(gpu.temp) + '°C' : '');
    const fan = gpu.fan_rpm == null ? null : gpu.fan_rpm === 0 ? 'Stopped <small>(quiet at low load)</small>' : `${gpu.fan_rpm.toLocaleString()} rpm <small>${Math.round(gpu.fan_rpm / gpu.fan_max * 100)}%</small>`;
    const mhz = m => m >= 1000 ? `${(m / 1000).toFixed(2)} GHz` : `${Math.round(m)} MHz`;
    const items = [
      ['Fans', fan],
      ['Power', gpu.power_pct != null ? `${Math.round(gpu.power_pct)}% <small>of its limit</small>` : null],
      ['Core', gpu.core_mhz ? `${mhz(gpu.core_mhz)}${gpu.core_max_mhz && gpu.core_max_mhz >= gpu.core_mhz ? ` <small>/ ${mhz(gpu.core_max_mhz)}</small>` : ''}` : null],
      ['Memory', gpu.mem_mhz ? mhz(gpu.mem_mhz) : null],
      ['Voltage', gpu.voltage_mv ? `${(gpu.voltage_mv / 1000).toFixed(3)} V` : null],
      ['VRAM', s.vram_total > 0 ? `${s.vram_used.toFixed(1)} <small>/ ${Math.round(s.vram_total)} GB</small>` : null]
    ].filter(x => x[1] != null);
    setHtml($('#gpuStats'), items.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join(''));
  }
}

/* ---------- clipboard ---------- */
const KIND_IC = { text: 'text', link: 'link', image: 'image', color: 'drop' };
async function loadClips() {
  try { S.clips = await invoke('clip_list'); } catch (e) { log(`clip_list failed: ${e}`); return; }
  try { renderClip(); } catch (e) { log(`renderClip failed: ${e && e.stack || e}`); }
}
const agoLabel = ts => { const m = Math.floor((Date.now() - ts) / 60000); return m < 1 ? 'now' : m < 60 ? m + 'm' : m < 1440 ? Math.floor(m / 60) + 'h' : Math.floor(m / 1440) + 'd'; };
const isCommand = t => !/\n/.test(t) && t.length < 200 && (/\s--?\w/.test(t) || /^(npm|npx|pnpm|yarn|cargo|git|cd|ls|python|pip|node|winget|choco|scoop|docker|kubectl|ssh)\b/.test(t));
function clipGroups() {
  const q = S.clipQ.trim().toLowerCase();
  const items = S.clips.filter(it => (S.clipFilter === 'all' || it.kind === S.clipFilter) && (!q || it.text.toLowerCase().includes(q)));
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const t0 = today.getTime(), y0 = t0 - 864e5;
  return [
    ['Pinned', items.filter(i => i.pinned)],
    ['Today', items.filter(i => !i.pinned && i.ts >= t0)],
    ['Yesterday', items.filter(i => !i.pinned && i.ts < t0 && i.ts >= y0)],
    ['Earlier', items.filter(i => !i.pinned && i.ts < y0)]
  ].filter(g => g[1].length);
}
const clipOrdered = () => clipGroups().flatMap(g => g[1]);
function renderClip() {
  const groups = clipGroups(), ord = groups.flatMap(g => g[1]);
  if (!ord.some(i => i.id === S.clipSel)) S.clipSel = ord[0] ? ord[0].id : null;
  let html = '';
  for (const [g, arr] of groups) {
    html += `<li class="cb-group" role="presentation">${g}</li>`;
    for (const it of arr) {
      const k = it.kind === 'color' ? `<span class="k sw" style="background:${esc(it.text)}"></span>`
        : it.kind === 'image' && it.image ? `<span class="k"><img src="${esc(assetUrl(it.thumb || it.image))}" alt="" loading="lazy" decoding="async"></span>`
        : `<span class="k">${icon(KIND_IC[it.kind] || 'text')}</span>`;
      html += `<li class="cb-item" role="option" id="cb-${it.id}" data-id="${it.id}" aria-selected="${it.id === S.clipSel}">${k}<span class="tx${isCommand(it.text) ? ' mono' : ''}">${esc(it.text.slice(0, 200))}</span><time>${agoLabel(it.ts)}</time></li>`;
    }
  }
  const list = $('#cbList');
  list.innerHTML = html || `<li class="empty">${S.clips.length ? 'Nothing matches. Try another filter.' : 'Copy something and it shows up here.'}</li>`;
  $('#cbSearch').placeholder = `Search ${S.clips.length} item${S.clips.length === 1 ? '' : 's'}`;
  $$('#cbChips .chip').forEach(b => b.setAttribute('aria-pressed', b.dataset.f === S.clipFilter));
  const sel = S.clipSel != null && $('#cb-' + S.clipSel);
  if (sel) { const t = sel.offsetTop, b = t + sel.offsetHeight; if (t < list.scrollTop + 24) list.scrollTop = Math.max(0, t - 28); else if (b > list.scrollTop + list.clientHeight) list.scrollTop = b - list.clientHeight + 4; }
  renderPreview();
}
function hexToRgb(h) { h = h.replace('#', ''); if (h.length === 3) h = h.split('').map(c => c + c).join(''); const n = parseInt(h, 16); return [n >> 16 & 255, n >> 8 & 255, n & 255]; }
function renderPreview() {
  const it = S.clips.find(i => i.id === S.clipSel), pv = $('#cbPrev');
  if (!it) { pv.innerHTML = '<p class="empty">Select an item to preview it.</p>'; return; }
  let body = '', type = '', size = '';
  if (it.kind === 'image') {
    body = `<img class="pv-img" src="${esc(assetUrl(it.image || ''))}" alt="">`; type = 'Image'; size = `${it.w} × ${it.h}`;
  } else if (it.kind === 'link') {
    let host = ''; try { host = new URL(it.text).hostname; } catch (e) { host = it.text; }
    body = `<div class="pv-link"><span class="pv-fav">${esc(host.replace(/^www\./, '').slice(0, 1).toUpperCase())}</span><div><b>${esc(host)}</b><span>${esc(it.text)}</span></div></div>`;
    type = 'Link'; size = `${it.text.length} characters`;
  } else if (it.kind === 'color') {
    const rgb = hexToRgb(it.text.trim());
    body = `<div class="pv-swatch" style="background:${esc(it.text.trim())}"></div><div class="pv-body mono">${esc(it.text.trim().toUpperCase())}<br>rgb(${rgb.join(', ')})</div>`; type = 'Color'; size = 'Hex';
  } else {
    const words = it.text.trim().split(/\s+/).length, cmd = isCommand(it.text);
    body = `<div class="pv-body${cmd ? ' mono' : ''}">${esc(it.text)}</div>`; type = cmd ? 'Command' : 'Text';
    size = `${it.text.length} characters · ${words} word${words > 1 ? 's' : ''}`;
  }
  const when = new Date(it.ts).toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' });
  pv.innerHTML = `${body}<dl class="pv-meta"><dt>Type</dt><dd>${type}</dd><dt>Size</dt><dd>${size}</dd><dt>Copied</dt><dd>${when}</dd></dl><div class="pv-act"><button class="btn primary" type="button" data-cb="copy">${icon('copy')}<span>Copy</span></button><button class="btn" type="button" data-cb="pin">${icon('pin')}<span>${it.pinned ? 'Unpin' : 'Pin'}</span></button><span class="grow"></span><button class="ib" type="button" data-cb="del" aria-label="Remove from history">${icon('trash')}</button></div>`;
}
async function copySel() {
  const it = S.clips.find(i => i.id === S.clipSel); if (!it) return;
  try {
    await invoke('clip_copy', { id: it.id });
    closePanel();
    toast('Copied to clipboard', it.kind === 'image' ? `Image ${it.w} × ${it.h}` : it.text.slice(0, 42) + (it.text.length > 42 ? '…' : ''), 'clip', { ms: 1600, sound: 'small' });
  } catch (e) { const s = $('[data-cb="copy"] span'); if (s) s.textContent = "Couldn't copy"; }
}
async function togglePin() { const it = S.clips.find(i => i.id === S.clipSel); if (!it) return; await quiet(invoke('clip_pin', { id: it.id })); loadClips(); }
async function delSel() {
  const ord = clipOrdered(), idx = ord.findIndex(i => i.id === S.clipSel); if (idx < 0) return;
  const nxt = ord[idx + 1] || ord[idx - 1];
  await quiet(invoke('clip_delete', { id: S.clipSel }));
  S.clipSel = nxt ? nxt.id : null; loadClips();
}
$('#cbList').addEventListener('click', e => { const li = e.target.closest('.cb-item'); if (!li) return; S.clipSel = +li.dataset.id; renderClip(); });
$('#cbList').addEventListener('dblclick', e => { if (e.target.closest('.cb-item')) copySel(); });
$('#cbChips').addEventListener('click', e => { const b = e.target.closest('.chip'); if (!b) return; S.clipFilter = b.dataset.f; renderClip(); });
$('#cbSearch').addEventListener('input', e => { S.clipQ = e.target.value; renderClip(); });
$('#cbPrev').addEventListener('click', e => {
  const b = e.target.closest('[data-cb]'); if (!b) return;
  if (b.dataset.cb === 'copy') copySel(); else if (b.dataset.cb === 'pin') togglePin(); else delSel();
});
$('#cbClear').addEventListener('click', async () => { await quiet(invoke('clip_clear')); loadClips(); });

/* ---------- apps: running windows, tray, pinned apps, folders ---------- */
S.pins = store.get('pins', []);
S.folders = store.get('folders', []);
S.pinMeta = store.get('pinMeta', {}); // "exe:<path>" pins (programs not in the Start menu) → { name, icon }
S.openFolder = null; S.wins = []; S.winsSig = ''; S.tray = [];
S.appUse = store.get('appUse', {}); // id → { n: times opened from the island, t: last time }
let appById = new Map(), appByExe = new Map(), appNames = [], dragId = null, dragFrom = null;
const savePins = () => { store.set('pins', S.pins); store.set('pinMeta', S.pinMeta); };
const saveFolders = () => store.set('folders', S.folders);
const folderOf = id => S.folders.find(f => f.apps.includes(id));
const query = () => $('#appSearch').value.trim().toLowerCase();
const glyphOf = name => name.replace(/[^A-Za-z0-9]/g, '').slice(0, 2) || '?';
const norm = s => s.toLowerCase().replace(/[^a-z0-9]/g, '');
const exeStem = exe => (exe.split('\\').pop() || '').replace(/\.exe$/i, '');
const prettyStem = stem => stem ? stem[0].toUpperCase() + stem.slice(1) : 'Program';

/** An app tile by id: a Start menu entry, or a pinned program stored as "exe:<path>". */
function appOf(id) {
  if (appById.has(id)) return appById.get(id);
  if (!id.startsWith('exe:')) return null;
  const meta = S.pinMeta[id] || {};
  return { id, name: meta.name || prettyStem(exeStem(id.slice(4))), icon: meta.icon || null, exe: id.slice(4).toLowerCase() };
}
/** Which Start menu app a window belongs to: its AppUserModelID, its exe (or shortcut target),
 *  or, for launcher-style apps (Discord, Steam…), an app whose name its exe name starts with. */
function appForWin(w) {
  if (w.aumid && appById.has(w.aumid)) return appById.get(w.aumid);
  if (appByExe.has(w.exe)) return appByExe.get(w.exe);
  const stem = norm(exeStem(w.exe));
  if (stem.length < 3) return null;
  const hit = appNames.find(([n]) => n === stem) || appNames.find(([n]) => n.length >= 4 && stem.startsWith(n));
  return hit ? hit[1] : null;
}
const winsFor = a => S.wins.filter(w => (a.exe && w.exe === a.exe) || (appForWin(w) || {}).id === a.id);

async function loadApps() {
  try {
    S.apps = await invoke('apps_list');
    appById = new Map(S.apps.map(a => [a.id, a]));
    appByExe = new Map(S.apps.filter(a => a.exe).map(a => [a.exe, a]));
    appNames = S.apps.map(a => [norm(a.name), a]).filter(([n]) => n).sort((x, y) => y[0].length - x[0].length);
    renderApps();
  } catch (e) { log(`apps_list failed: ${e}`); }
}
async function loadTray() { try { S.tray = await invoke('tray_list'); if (!dragId) { renderTray(); renderBarTray(); } } catch (e) {} }
let winsT;
function winsLoop() {
  clearTimeout(winsT);
  // The Apps tab needs open windows; so does the bar's dock, all the time.
  if (!((S.mode === 'panel' && S.tab === 'apps') || isBar())) return;
  invoke('windows_list').then(ws => {
    const sig = JSON.stringify(ws);
    if (sig !== S.winsSig) {
      S.winsSig = sig; S.wins = ws;
      if (!dragId) { renderDock(); if (S.mode === 'panel' && S.tab === 'apps') { renderRunning(); renderPins(); renderGrid(); layout(); } }
    }
  }).catch(e => log(`windows_list failed: ${e}`)).finally(() => { winsT = setTimeout(winsLoop, 1200); });
}

function appIconHtml(a) {
  return a.icon ? `<span class="ico"><img src="${esc(assetUrl(a.icon))}" alt="" loading="lazy"></span>` : `<span class="glyph" style="background:${gradFor(a.name)}">${esc(glyphOf(a.name))}</span>`;
}
function appTile(a) {
  const dot = winsFor(a).length ? '<i class="dot" title="Running"></i>' : '';
  return `<button class="app" type="button" data-id="${esc(a.id)}" title="${esc(a.name)}">${appIconHtml(a)}<span class="nm">${esc(a.name)}</span>${dot}</button>`;
}
function folderTile(f) {
  const minis = f.apps.map(appOf).filter(Boolean).slice(0, 4)
    .map(a => a.icon ? `<img src="${esc(assetUrl(a.icon))}" alt="">` : `<span style="background:${gradFor(a.name)}"></span>`).join('');
  return `<button class="app folder" type="button" data-folder="${esc(f.id)}" title="${esc(f.name)}"><span class="fgrid">${minis}</span><span class="nm">${esc(f.name)}</span></button>`;
}
function matchingWins() { const q = query(); return S.wins.filter(w => !q || w.title.toLowerCase().includes(q) || w.exe.includes(q)); }
function renderRunning() {
  const wins = matchingWins();
  $('#secRunning').hidden = !wins.length;
  $('#runCount').textContent = `${wins.length} window${wins.length === 1 ? '' : 's'}`;
  $('#wins').innerHTML = wins.map((w, i) => {
    const a = appForWin(w), src = a && a.icon ? assetUrl(a.icon) : w.icon ? assetUrl(w.icon) : null;
    const name = a ? a.name : (w.exe.split('\\').pop() || '').replace(/\.exe$/, '');
    const ico = src ? `<img src="${esc(src)}" alt="">` : `<span class="glyph" style="background:${gradFor(name)}">${esc(glyphOf(name))}</span>`;
    const cls = ['win', w.active ? 'active' : '', w.minimized ? 'min' : '', query() && i === 0 ? 'first' : ''].join(' ');
    return `<div class="${cls}" role="button" tabindex="0" data-h="${w.hwnd}" title="${esc(w.title)}"><span class="w-ico">${ico}</span><span class="w-t"><b>${esc(w.title)}</b><small>${esc(name)}${w.minimized ? ' · minimized' : ''}</small></span><button class="w-x" type="button" data-close aria-label="Close ${esc(w.title)}">${icon('x')}</button></div>`;
  }).join('');
}
function renderTray() {
  $('#secTray').hidden = !!query() || !S.tray.length;
  $('#tray').innerHTML = S.tray.map(t => `<button type="button" data-key="${esc(t.key)}" title="${esc(t.tip || t.exe)}" aria-label="${esc(t.tip || t.exe)}"><img src="${esc(t.icon)}" alt=""></button>`).join('');
  if (S.mode === 'panel' && S.tab === 'apps') layout();
}
function renderPins() {
  $('#secPins').hidden = !!query();
  const pins = S.pins.map(appOf).filter(Boolean);
  $('#pins').innerHTML = pins.map(appTile).join('') || '<p class="pin-empty">Right-click any app or open window and choose Pin, or drag an app here.</p>';
}
function renderGrid() {
  const q = query();
  if (!S.apps.length) { $('#apps').innerHTML = '<p class="empty">Reading your Start menu…</p>'; return; }
  const inFolder = new Set(S.folders.flatMap(f => f.apps));
  const list = S.apps.filter(a => q ? a.name.toLowerCase().includes(q) : !inFolder.has(a.id));
  const folders = q ? '' : [...S.folders].sort((a, b) => a.name.localeCompare(b.name)).map(folderTile).join('');
  $('#apps').innerHTML = folders + list.map(appTile).join('');
  if (q && !matchingWins().length) { const f = $('#apps .app[data-id]'); if (f) f.classList.add('first'); }
  $('#allTitle').textContent = q ? 'Apps' : 'All apps';
  $('#newFolderBtn').hidden = !!q;
  $('#appsEmpty').hidden = !(q && !list.length && !matchingWins().length);
}
function renderApps() {
  renderRunning(); renderTray(); renderPins(); renderGrid();
  if (S.openFolder) renderFolder();
  if (S.mode === 'panel' && S.tab === 'apps') layout();
}

/** Counts an app opened from the island (the Command Center's Apps sheet lists them as Recent). */
function noteUse(id) {
  const u = S.appUse[id] || { n: 0, t: 0 };
  S.appUse[id] = { n: u.n + 1, t: Date.now() };
  const ids = Object.keys(S.appUse);
  if (ids.length > 200) ids.sort((x, y) => S.appUse[x].t - S.appUse[y].t).slice(0, ids.length - 200).forEach(x => delete S.appUse[x]);
  store.set('appUse', S.appUse);
}
function launch(a) {
  closePanel(true); $('#appSearch').value = ''; renderApps();
  noteUse(a.id);
  invoke('app_launch', { id: a.id })
    .then(() => setTimeout(() => toast(`Opening ${a.name}`, 'Launching…', null, a.icon ? { img: assetUrl(a.icon), ms: 1800, sound: 'small' } : { glyph: a.name.slice(0, 2), bg: gradFor(a.name), ms: 1800, sound: 'small' }), 260))
    .catch(e => setTimeout(() => toast(`Couldn't open ${a.name}`, String(e), 'x', { sound: 'problem' }), 260));
}
function activateWin(w) {
  closePanel(true); $('#appSearch').value = '';
  invoke('window_activate', { hwnd: w.hwnd }).catch(e => setTimeout(() => toast("Couldn't switch", String(e), 'x', { sound: 'problem' }), 260));
}
function closeWin(w) {
  quiet(invoke('window_close', { hwnd: w.hwnd }));
  S.wins = S.wins.filter(x => x.hwnd !== w.hwnd); S.winsSig = ''; renderRunning(); renderPins(); layout();
}
/** Pinned apps act like taskbar buttons: switch to an open window, otherwise launch. */
function openPinned(a) {
  const ws = winsFor(a);
  if (!ws.length) return launch(a);
  activateWin(ws.length > 1 && ws[0].active ? ws[1] : ws[0]);
}
function togglePinApp(id) {
  const i = S.pins.indexOf(id);
  if (i >= 0) S.pins.splice(i, 1); else S.pins.push(id);
  savePins(); renderApps();
}
function moveToFolder(id, folderId) {
  S.folders.forEach(f => { f.apps = f.apps.filter(x => x !== id); });
  const f = folderId && S.folders.find(x => x.id === folderId);
  if (f) f.apps.push(id);
  saveFolders(); renderApps();
}
function newFolder(withApp) {
  let name = 'New folder', n = 2;
  while (S.folders.some(f => f.name === name)) name = `New folder ${n++}`;
  const f = { id: 'f' + Date.now(), name, apps: [] };
  S.folders.push(f); saveFolders();
  if (withApp) moveToFolder(withApp, f.id);
  openFolder(f.id, true);
}
function openFolder(id, rename) {
  S.openFolder = id; $('#appScroll').hidden = true; $('#folderView').hidden = false;
  renderFolder(); layout();
  if (rename) setTimeout(() => { const n = $('#fvName'); n.focus(); n.select(); }, 60);
}
function closeFolder() { S.openFolder = null; $('#folderView').hidden = true; $('#appScroll').hidden = false; renderApps(); }
function renderFolder() {
  const f = S.folders.find(x => x.id === S.openFolder); if (!f) { closeFolder(); return; }
  if (document.activeElement !== $('#fvName')) $('#fvName').value = f.name;
  const apps = f.apps.map(appOf).filter(Boolean);
  $('#fvApps').innerHTML = apps.map(appTile).join('');
  $('#fvEmpty').hidden = apps.length > 0;
}
$('#fvBack').addEventListener('click', closeFolder);
$('#fvName').addEventListener('input', e => { const f = S.folders.find(x => x.id === S.openFolder); if (f) { f.name = e.target.value.trim() || 'Folder'; saveFolders(); } });
$('#fvName').addEventListener('keydown', e => { if (e.key === 'Enter') e.target.blur(); });
$('#fvDelete').addEventListener('click', () => { S.folders = S.folders.filter(x => x.id !== S.openFolder); saveFolders(); closeFolder(); });
$('#newFolderBtn').addEventListener('click', () => newFolder(null));

/* right-click menu */
function hideCtx() { $('#ctx').hidden = true; }
function showCtx(x, y, items) {
  const m = $('#ctx');
  m.dataset.skin = island.dataset.skin;
  m.innerHTML = items.map((it, i) => it.sep ? '<hr>' : it.heading ? `<div class="label">${esc(it.heading)}</div>` : `<button type="button" role="menuitem" data-i="${i}">${icon(it.icon || 'rocket')}<span>${esc(it.label)}</span></button>`).join('');
  m.hidden = false;
  const r = m.getBoundingClientRect();
  m.style.left = clamp(x, 6, innerWidth - r.width - 6) + 'px';
  m.style.top = clamp(y, 6, innerHeight - r.height - 6) + 'px';
  m.onclick = e => { const b = e.target.closest('button[data-i]'); if (!b) return; hideCtx(); items[+b.dataset.i].action(); };
}
function appMenu(a, x, y) {
  const pinned = S.pins.includes(a.id), inF = folderOf(a.id), ws = winsFor(a);
  const items = [{ label: 'Open', icon: 'rocket', action: () => launch(a) }];
  if (ws.length) items.push({ label: 'Switch to its window', icon: 'monitor', action: () => activateWin(ws[0]) });
  items.push({ sep: true }, { label: pinned ? 'Unpin' : 'Pin', icon: 'pin', action: () => togglePinApp(a.id) });
  items.push({ sep: true }, { heading: 'Folder' });
  S.folders.filter(f => f !== inF).forEach(f => items.push({ label: f.name, icon: 'folder', action: () => moveToFolder(a.id, f.id) }));
  items.push({ label: 'New folder…', icon: 'plus', action: () => newFolder(a.id) });
  if (inF) items.push({ label: `Take out of ${inF.name}`, icon: 'x', action: () => moveToFolder(a.id, null) });
  showCtx(x, y, items);
}
/** The pin id for a window: its Start menu app if we can tell, otherwise its program file. */
function pinIdForWin(w) {
  const a = appForWin(w);
  if (a) return a.id;
  if (!w.exe) return null;
  const id = 'exe:' + w.exe;
  if (!S.pinMeta[id]) S.pinMeta[id] = { name: prettyStem(exeStem(w.exe)), icon: w.icon || null };
  return id;
}
function winMenu(w, x, y) {
  const pid = pinIdForWin(w), a = pid && appOf(pid);
  const items = [
    { label: 'Switch to', icon: 'monitor', action: () => activateWin(w) },
    { label: 'Minimize', icon: 'down', action: () => { quiet(invoke('window_minimize', { hwnd: w.hwnd })); } },
    { label: 'Close window', icon: 'x', action: () => closeWin(w) }
  ];
  if (a) items.push({ sep: true }, { label: S.pins.includes(a.id) ? `Unpin ${a.name}` : `Pin ${a.name}`, icon: 'pin', action: () => togglePinApp(a.id) });
  showCtx(x, y, items);
}

/* clicks */
$('#v-apps').addEventListener('click', e => {
  if (justDragged) { justDragged = false; return; }
  const close = e.target.closest('[data-close]');
  const win = e.target.closest('.win');
  if (win) { const w = S.wins.find(x => String(x.hwnd) === win.dataset.h); if (w) close ? closeWin(w) : activateWin(w); return; }
  const folder = e.target.closest('.app.folder');
  if (folder) { openFolder(folder.dataset.folder); return; }
  const tile = e.target.closest('.app[data-id]');
  if (!tile) return;
  const a = appOf(tile.dataset.id); if (!a) return;
  tile.closest('#pins') ? openPinned(a) : launch(a);
});
$('#v-apps').addEventListener('auxclick', e => {
  if (e.button !== 1) return;
  const win = e.target.closest('.win'); if (!win) return;
  const w = S.wins.find(x => String(x.hwnd) === win.dataset.h); if (w) closeWin(w);
});
$('#v-apps').addEventListener('keydown', e => {
  if ((e.key === 'Enter' || e.key === ' ') && e.target.classList.contains('win')) { e.preventDefault(); e.target.click(); }
});
$('#v-apps').addEventListener('contextmenu', e => {
  const win = e.target.closest('.win'), tile = e.target.closest('.app[data-id]'), tray = e.target.closest('#tray button');
  if (tray) return;
  e.preventDefault();
  if (win) { const w = S.wins.find(x => String(x.hwnd) === win.dataset.h); if (w) winMenu(w, e.clientX, e.clientY); }
  else if (tile) { const a = appOf(tile.dataset.id); if (a) appMenu(a, e.clientX, e.clientY); }
});

/* tray icons (Apps tab and the bar): single click waits a moment so a double-click can win */
let trayClickT;
function trayClick(key, button) { closePanel(true); quiet(invoke('tray_click', { key, button })); }
$$('.tray-set').forEach(set => {
  set.addEventListener('click', e => {
    const b = e.target.closest('button[data-key]'); if (!b) return;
    clearTimeout(trayClickT); trayClickT = setTimeout(() => trayClick(b.dataset.key, 'left'), 230);
  });
  set.addEventListener('dblclick', e => {
    const b = e.target.closest('button[data-key]'); if (!b) return;
    clearTimeout(trayClickT); trayClick(b.dataset.key, 'double');
  });
  set.addEventListener('contextmenu', e => {
    const b = e.target.closest('button[data-key]'); if (!b) return;
    e.preventDefault(); clearTimeout(trayClickT); trayClick(b.dataset.key, 'right');
  });
});

/* Drag apps onto folders, into Pinned (to pin or reorder), or out of Pinned (to unpin).
 * Done with pointer events: HTML5 drag-and-drop is unreliable inside the transparent shell window. */
const DRAG_START_PX = 6;
let drag = null, justDragged = false, dragScrollRaf = 0;
function dropTargetAt(x, y) {
  const el = document.elementFromPoint(x, y);
  if (!el) return null;
  return el.closest('.app.folder, #pins') || (dragFrom === 'pins' && el.closest('#secAll, #fvApps')) || null;
}
function endDrag() {
  if (drag && drag.ghost) drag.ghost.remove();
  cancelAnimationFrame(dragScrollRaf);
  drag = null; dragId = null; dragFrom = null;
  $$('.dragging, .drop').forEach(x => x.classList.remove('dragging', 'drop'));
}
// Scroll the app list while the pointer hovers near its top or bottom edge.
function dragAutoScroll() {
  if (!drag || !drag.moved) return;
  const sc = $('#appScroll').hidden ? views.apps : $('#appScroll'), r = sc.getBoundingClientRect();
  if (drag.lastY < r.top + 36) sc.scrollTop -= 10;
  else if (drag.lastY > r.bottom - 36) sc.scrollTop += 10;
  dragScrollRaf = requestAnimationFrame(dragAutoScroll);
}
$('#v-apps').addEventListener('pointerdown', e => {
  if (e.button !== 0) return;
  const t = e.target.closest('.app[data-id]'); if (!t) return;
  drag = { id: t.dataset.id, from: t.closest('#pins') ? 'pins' : 'grid', el: t, x: e.clientX, y: e.clientY, lastY: e.clientY, moved: false, pointer: e.pointerId };
});
document.addEventListener('pointermove', e => {
  if (!drag || e.pointerId !== drag.pointer) return;
  drag.lastY = e.clientY;
  if (!drag.moved) {
    if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < DRAG_START_PX) return;
    drag.moved = true; dragId = drag.id; dragFrom = drag.from;
    hideCtx();
    try { drag.el.setPointerCapture(e.pointerId); } catch (err) {}
    const g = drag.el.cloneNode(true);
    g.classList.add('drag-ghost'); g.dataset.skin = island.dataset.skin;
    document.body.appendChild(g); drag.ghost = g;
    drag.el.classList.add('dragging');
    dragScrollRaf = requestAnimationFrame(dragAutoScroll);
  }
  drag.ghost.style.transform = `translate(${e.clientX - 42}px, ${e.clientY - 34}px)`;
  const t = dropTargetAt(e.clientX, e.clientY);
  $$('.drop').forEach(x => { if (x !== t) x.classList.remove('drop'); });
  if (t) t.classList.add('drop');
});
document.addEventListener('pointerup', e => {
  if (!drag || e.pointerId !== drag.pointer) return;
  if (!drag.moved) { drag = null; return; }
  justDragged = true; setTimeout(() => { justDragged = false; }, 50);
  const id = drag.id, t = dropTargetAt(e.clientX, e.clientY);
  if (t && t.id === 'pins') {
    const pins = S.pins.filter(x => x !== id);
    let idx = pins.length;
    const over = document.elementFromPoint(e.clientX, e.clientY);
    const tile = over && over.closest('#pins .app[data-id]');
    if (tile && tile.dataset.id !== id) {
      idx = pins.indexOf(tile.dataset.id);
      const r = tile.getBoundingClientRect();
      if (e.clientX > r.left + r.width / 2) idx++;
    }
    pins.splice(idx, 0, id); S.pins = pins; savePins();
  } else if (t && t.classList.contains('folder')) {
    moveToFolder(id, t.dataset.folder);
  } else if (t) {
    S.pins = S.pins.filter(x => x !== id); savePins();
  }
  endDrag();
  renderApps();
});
document.addEventListener('pointercancel', e => { if (drag && e.pointerId === drag.pointer) endDrag(); });

$('#appSearch').addEventListener('input', () => { renderApps(); });
$('#appSearch').addEventListener('keydown', e => {
  if (e.key !== 'Enter') return;
  const w = query() && matchingWins()[0];
  if (w) { activateWin(w); return; }
  const f = $('#apps .app[data-id]');
  if (f) { const a = appOf(f.dataset.id); if (a) launch(a); }
});
listen('tray:changed', loadTray);

/* ---------- Wallpaper Engine ---------- */
S.we = null; S.weAt = 0; S.weKind = ''; S.weQ = ''; S.weShown = 0; S.wePaused = false; S.weMuted = false;
S.wpMode = store.get('wpMode', 'we');
const WE_PAGE = 48;
/** Opens the tab: Wallpaper Engine's library (re-read at most once a minute) or the island's scenes. */
function showWallpapers() {
  renderWpMode();
  if (S.wpMode !== 'we') return;
  if (S.we && Date.now() - S.weAt < 60000) { renderWeGrid(); return; }
  invoke('we_library').then(lib => {
    S.we = lib; S.weAt = Date.now();
    if (!lib.installed) { S.wpMode = 'island'; renderWpMode(); return; }
    renderWpMode(); renderWeGrid();
  }).catch(err => note('#wpNote', String(err)));
}
function renderWpMode() {
  const has = !S.we || S.we.installed; // unknown until the first look: offer it
  const we = has && S.wpMode === 'we';
  $('#wpMode').hidden = !has;
  $$('#wpMode button').forEach(b => b.setAttribute('aria-pressed', b.dataset.mode === (we ? 'we' : 'island')));
  $('#weView').hidden = !we; $('#islandWalls').hidden = we;
  $('#wpTitle').hidden = has; $('#wpSub').hidden = has && !we;
  if (we && S.we) txt($('#wpSub'), `${S.we.wallpapers.length} wallpapers`);
  else if (!has) txt($('#wpSub'), `${WALLS.length} generated scenes`);
  setHtml($('#wePlay'), icon(S.wePaused ? 'play' : 'pause'));
  $('#wePlay').title = S.wePaused ? 'Resume the wallpaper' : 'Pause the wallpaper';
  setHtml($('#weMute'), icon(S.weMuted ? 'mute' : 'speaker'));
  $('#weMute').title = S.weMuted ? 'Unmute the wallpaper' : 'Mute the wallpaper';
  if (S.mode === 'panel' && S.tab === 'wallpaper') layout();
}
/** Current wallpaper first, then recently used, then newest; filtered by type and search. */
function weList() {
  const lib = S.we, q = S.weQ.trim().toLowerCase();
  const rank = w => w.dir === lib.current ? 0 : lib.recent.includes(w.dir) ? 1 + lib.recent.indexOf(w.dir) : 1000;
  return lib.wallpapers
    .filter(w => (!S.weKind || w.kind === S.weKind) && (!q || w.title.toLowerCase().includes(q) || w.tags.some(t => t.toLowerCase().includes(q))))
    .sort((a, b) => rank(a) - rank(b) || b.added - a.added);
}
const KIND_ICON = { scene: 'layers', video: 'film', web: 'globe' };
function weTile(w) {
  const cur = w.dir === S.we.current, mature = w.rating === 'Mature';
  const img = w.thumb ? `<img src="${esc(assetUrl(w.thumb))}" alt="" loading="lazy" decoding="async">` : '';
  return `<button type="button" role="listitem" class="we-tile${cur ? ' on' : ''}${mature ? ' mature' : ''}" data-dir="${esc(w.dir)}" title="${esc(w.title)}${w.tags.length ? ' · ' + esc(w.tags.join(', ')) : ''}"${w.thumb ? '' : ` style="background:${gradFor(w.title)}"`}>${img}${cur ? '<span class="we-badge">On</span>' : mature ? '<span class="we-badge r18">18+</span>' : ''}${KIND_ICON[w.kind] ? `<span class="we-kind">${icon(KIND_ICON[w.kind])}</span>` : ''}<span class="we-t">${esc(w.title)}</span></button>`;
}
/** Builds the grid a page at a time as you scroll, so a 600-wallpaper library stays light. */
function renderWeGrid(more) {
  if (!S.we || !S.we.installed) return;
  const list = weList(), grid = $('#weGrid');
  if (!more) { S.weShown = 0; grid.innerHTML = ''; grid.scrollTop = 0; }
  const next = list.slice(S.weShown, S.weShown + WE_PAGE);
  grid.querySelector('.we-more')?.remove();
  grid.insertAdjacentHTML('beforeend', next.map(weTile).join('') + (S.weShown + next.length < list.length ? '<i class="we-more"></i>' : ''));
  S.weShown += next.length;
  const sentinel = grid.querySelector('.we-more');
  if (sentinel) { S.weObs?.disconnect(); S.weObs = new IntersectionObserver(es => { if (es.some(e => e.isIntersecting)) renderWeGrid(true); }, { root: grid, rootMargin: '200px' }); S.weObs.observe(sentinel); }
  $('#weEmpty').hidden = list.length > 0; txt($('#weEmpty'), S.weQ ? `Nothing matches “${S.weQ}”.` : 'No wallpapers of that type.');
  if (!more && S.mode === 'panel' && S.tab === 'wallpaper') layout();
}
function clearWeGrid() { S.weObs?.disconnect(); S.weObs = null; S.weShown = 0; $('#weGrid').innerHTML = ''; }
const weOf = el => S.we && S.we.wallpapers.find(w => w.dir === el.dataset.dir);
// Thumbnails arrive one by one the first time the library is opened.
listen('we:thumb', e => {
  const w = S.we && S.we.wallpapers.find(x => x.dir === e.payload.dir); if (!w) return;
  w.thumb = e.payload.thumb;
  const tile = $(`#weGrid .we-tile[data-dir="${CSS.escape(w.dir)}"]`);
  if (tile && !tile.querySelector('img')) { tile.style.background = ''; tile.insertAdjacentHTML('afterbegin', `<img src="${esc(assetUrl(w.thumb))}" alt="" decoding="async">`); }
});
// Hovering plays that one wallpaper's real (often animated) preview; only one at a time.
$('#weGrid').addEventListener('pointerover', e => {
  const tile = e.target.closest('.we-tile'); if (!tile || tile === S.weHover) return;
  weUnhover(); const w = weOf(tile), img = tile.querySelector('img');
  if (w && w.preview && img && /\.gif$/i.test(w.preview)) { S.weHover = tile; img.src = assetUrl(w.preview); }
});
function weUnhover() {
  const t = S.weHover; S.weHover = null; if (!t) return;
  const w = weOf(t), img = t.querySelector('img'); if (w && img && w.thumb) img.src = assetUrl(w.thumb);
}
$('#weGrid').addEventListener('pointerleave', weUnhover);
$('#weGrid').addEventListener('click', e => {
  const tile = e.target.closest('.we-tile'); const w = tile && weOf(tile); if (!w || tile.classList.contains('busy')) return;
  note('#wpNote', ''); tile.classList.add('busy');
  invoke('we_apply', { dir: w.dir }).then(() => {
    S.we.current = w.dir; S.we.recent = [w.dir, ...S.we.recent.filter(d => d !== w.dir)].slice(0, 10);
    S.wePaused = false; renderWpMode();
    $$('#weGrid .we-tile.on').forEach(t => { t.classList.remove('on'); t.querySelector('.we-badge:not(.r18)')?.remove(); });
    tile.classList.add('on'); if (!tile.querySelector('.we-badge')) tile.insertAdjacentHTML('beforeend', '<span class="we-badge">On</span>');
    if (S.follow && w.thumb) accentFrom(assetUrl(w.thumb));
  }).catch(err => note('#wpNote', String(err))).finally(() => tile.classList.remove('busy'));
});
/** A lively accent from the wallpaper's thumbnail (its most vivid colors, kept readable). */
function accentFrom(url) {
  const img = new Image();
  img.onload = () => {
    const c = document.createElement('canvas'); c.width = c.height = 24;
    const g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(img, 0, 0, 24, 24);
    const px = g.getImageData(0, 0, 24, 24).data;
    let r = 0, gr = 0, b = 0, sum = 0;
    for (let i = 0; i < px.length; i += 4) {
      const R = px[i], G = px[i + 1], B = px[i + 2], mx = Math.max(R, G, B), mn = Math.min(R, G, B);
      const w = (mx ? (mx - mn) / mx : 0) ** 2 * (mx / 255) + 0.01; r += R * w; gr += G * w; b += B * w; sum += w;
    }
    const [R, G, B] = [r / sum / 255, gr / sum / 255, b / sum / 255];
    const mx = Math.max(R, G, B), mn = Math.min(R, G, B), d = mx - mn;
    let h = 0; if (d) h = mx === R ? ((G - B) / d) % 6 : mx === G ? (B - R) / d + 2 : (R - G) / d + 4;
    h = (h * 60 + 360) % 360;
    const l0 = (mx + mn) / 2, s0 = d ? d / (1 - Math.abs(2 * l0 - 1)) : 0;
    const s = clamp(s0, 0.45, 0.9), l = clamp(l0, 0.55, 0.66);
    // HSL → hex for setAccent
    const k = n => (n + h / 30) % 12, a = s * Math.min(l, 1 - l);
    const f = n => Math.round(255 * (l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1)))).toString(16).padStart(2, '0');
    setAccent(`#${f(0)}${f(8)}${f(4)}`);
  };
  img.src = url;
}
$('#wpMode').addEventListener('click', e => {
  const b = e.target.closest('[data-mode]'); if (!b) return;
  S.wpMode = b.dataset.mode; store.set('wpMode', S.wpMode);
  if (S.wpMode === 'we') showWallpapers(); else { clearWeGrid(); renderWpMode(); }
});
$('#weKinds').addEventListener('click', e => {
  const b = e.target.closest('[data-kind]'); if (!b) return;
  S.weKind = b.dataset.kind; $$('#weKinds .chip').forEach(c => c.setAttribute('aria-pressed', c === b)); renderWeGrid();
});
$('#weQ').addEventListener('input', e => { S.weQ = e.target.value; clearTimeout(S.weQT); S.weQT = setTimeout(() => renderWeGrid(), 120); });
$('#wePlay').addEventListener('click', () => {
  S.wePaused = !S.wePaused; renderWpMode();
  invoke('we_control', { cmd: S.wePaused ? 'pause' : 'play' }).catch(err => { S.wePaused = !S.wePaused; renderWpMode(); note('#wpNote', String(err)); });
});
$('#weMute').addEventListener('click', () => {
  S.weMuted = !S.weMuted; renderWpMode();
  invoke('we_control', { cmd: S.weMuted ? 'mute' : 'unmute' }).catch(err => { S.weMuted = !S.weMuted; renderWpMode(); note('#wpNote', String(err)); });
});

/* ---------- wallpaper ---------- */
WALLS.forEach((wl, i) => {
  const b = document.createElement('button'); b.type = 'button'; b.className = 'wcard'; b.dataset.i = i; b.setAttribute('aria-label', wl.name);
  const cv = document.createElement('canvas'); cv.width = 320; cv.height = 200; drawWall(cv, wl);
  b.appendChild(cv); b.insertAdjacentHTML('beforeend', '<span class="cur">Current</span>');
  $('#carousel').appendChild(b);
});
function renderWallUI() {
  $$('.wcard').forEach(b => { const i = +b.dataset.i; b.classList.toggle('sel', i === S.wallSel); b.classList.toggle('applied', i === S.wall); b.setAttribute('aria-pressed', i === S.wallSel); });
  const wl = WALLS[S.wallSel];
  $('#wpName').textContent = wl.name; $('#wpSw').style.background = wl.accent; $('#wpHex').textContent = `Accent ${wl.accent}`;
  const ap = $('#wpApply');
  ap.textContent = S.applying ? 'Applying…' : S.wallSel === S.wall ? 'Applied' : 'Apply';
  ap.disabled = S.applying || S.wallSel === S.wall;
  $('#wpShuffle').disabled = S.applying;
}
async function applyWall(i) {
  if (S.applying) return;
  S.applying = true; S.wallSel = i; renderWallUI(); note('#wpNote', '');
  await new Promise(r => setTimeout(r, 40));
  try {
    const dpr = window.devicePixelRatio || 1;
    const W = Math.min(7680, Math.round(screen.width * dpr)), H = Math.min(4320, Math.round(screen.height * dpr));
    const cv = document.createElement('canvas'); cv.width = W; cv.height = H;
    drawWall(cv, WALLS[i]);
    const blob = await new Promise((res, rej) => cv.toBlob(b => b ? res(b) : rej(new Error('Rendering failed')), 'image/png'));
    cv.width = cv.height = 0; // frees its ~15 MB of pixels now instead of at the next collection
    const bytes = new Uint8Array(await blob.arrayBuffer());
    const slug = `${WALLS[i].name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}-${W}x${H}`;
    await invoke('wallpaper_apply', bytes, { headers: { 'x-name': slug } });
    S.wall = i; store.set('wall', i);
    if (S.follow) setAccent(WALLS[i].accent);
  } catch (e) { note('#wpNote', `Couldn't set the wallpaper: ${e}`); }
  S.applying = false; renderWallUI();
}
$('#carousel').addEventListener('click', e => {
  const b = e.target.closest('.wcard'); if (!b) return; const i = +b.dataset.i;
  if (i === S.wallSel) { if (i !== S.wall) applyWall(i); } else { S.wallSel = i; renderWallUI(); }
});
let carouselT = 0;
$('#carousel').addEventListener('wheel', e => {
  e.preventDefault(); e.stopPropagation();
  const now = performance.now(); if (now < carouselT) return;
  const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY; if (Math.abs(d) < 4) return;
  S.wallSel = (S.wallSel + (d > 0 ? 1 : -1) + WALLS.length) % WALLS.length; renderWallUI(); carouselT = now + 170;
}, { passive: false });
$('#wpApply').addEventListener('click', () => applyWall(S.wallSel));
$('#wpShuffle').addEventListener('click', () => { let i; do { i = Math.floor(Math.random() * WALLS.length); } while (i === S.wall && WALLS.length > 1); applyWall(i); });
$('#follow').addEventListener('change', e => { S.follow = e.target.checked; store.set('follow', S.follow); syncAccentMode(); if (S.follow && S.wall >= 0) setAccent(WALLS[S.wall].accent); });

/* ---------- power ---------- */
$$('.pw').forEach(b => {
  const kind = b.dataset.pw;
  const run = () => { closePanel(true); setTimeout(() => quiet(invoke('power', { action: kind })), 150); };
  if (kind === 'lock') { b.addEventListener('click', run); return; }
  let t;
  const start = e => {
    if (e.type === 'keydown' && !(e.key === 'Enter' || e.key === ' ')) return;
    if (e.repeat) return; e.preventDefault();
    b.classList.add('holding'); clearTimeout(t);
    t = setTimeout(() => { b.classList.remove('holding'); run(); }, 800);
  };
  const stop = () => { clearTimeout(t); b.classList.remove('holding'); };
  b.addEventListener('pointerdown', start); b.addEventListener('pointerup', stop); b.addEventListener('pointerleave', stop); b.addEventListener('pointercancel', stop);
  b.addEventListener('keydown', start); b.addEventListener('keyup', stop);
});
invoke('autostart_get').then(on => { $('#autostart').checked = on; }).catch(() => {});
$('#autostart').addEventListener('change', e => quiet(invoke('autostart_set', { on: e.target.checked })));
$('#quitBtn').addEventListener('click', () => quiet(invoke('quit')));

/* ---------- updates (updater.rs) ---------- */
const fmtMB = b => `${(b / 1048576).toFixed(b < 10485760 ? 1 : 0)} MB`;
const updReady = u => !!u.latest && u.latest !== u.current && (u.state === 'available' || u.state === 'error');
function renderUpdate(u) {
  if (u) S.upd = u; u = S.upd; if (!u) return;
  $('#upd').hidden = false;
  const off = u.state === 'off';
  txt($('#updTitle'), `Island Shell ${u.current}`);
  const sub = {
    off: "Updates aren't set up in this build",
    checking: 'Checking for updates…',
    available: `Version ${u.latest} is ready to install${u.size ? ` · ${fmtMB(u.size)}` : ''}`,
    downloading: `Downloading ${u.latest}… ${Math.round((u.progress || 0) * 100)}%`,
    installing: 'Installing; the island will be right back',
    error: u.error || "Couldn't check for updates",
  }[u.state] || (u.checked ? `Up to date · checked ${ago(new Date(u.checked).toISOString())}` : 'Up to date');
  txt($('#updSub'), sub);
  $('#updSub').title = u.notes || sub;
  $('#updAuto').checked = !!u.auto; $('#updAuto').disabled = off;
  const b = $('#updBtn');
  b.hidden = off;
  b.disabled = ['checking', 'downloading', 'installing'].includes(u.state);
  setHtml(b, updReady(u) ? `${icon('download')}Update` : 'Check now');
  b.classList.toggle('primary', updReady(u));
}
function toastUpdate(u) {
  toast('Update available', `Island Shell ${u.latest} · open Power to install`, 'download', { tab: 'power', top: true, ms: 6000 });
}
$('#updBtn').addEventListener('click', () => {
  const u = S.upd || {};
  quiet(invoke(updReady(u) ? 'update_install' : 'update_check'));
});
$('#updAuto').addEventListener('change', e => invoke('update_auto', { on: e.target.checked }).then(renderUpdate).catch(() => {}));
invoke('update_status').then(renderUpdate).catch(() => {});
// A background check that finds a new version says so once per version.
listen('update:status', e => {
  const u = e.payload; renderUpdate(u);
  if (u.state === 'available' && store.get('updToast', '') !== u.latest) { store.set('updToast', u.latest); toastUpdate(u); }
});
// "Check for updates" in the tray menu always gets an answer.
listen('update:asked', e => {
  const u = e.payload; renderUpdate(u);
  if (updReady(u) && u.state === 'available') { store.set('updToast', u.latest); toastUpdate(u); }
  else if (u.state === 'error') toast("Couldn't check for updates", u.error, 'x', { top: true, ms: 5000 });
  else toast('Island Shell is up to date', `Version ${u.current}`, 'check', { top: true });
});

/* ---------- top-bar style ---------- */
S.style = 'island'; S.anchor = 0;
const barEl = $('#bar');
/** Sets innerHTML only when it changed, so animations inside (like the EQ bars) don't restart
 * and unchanged parts of the page aren't rebuilt every poll. Returns whether it changed. */
const htmlNow = new WeakMap();
function setHtml(el, html) { if (htmlNow.get(el) === html) return false; htmlNow.set(el, html); el.innerHTML = html; return true; }
/** Same for plain text. */
function txt(el, s) { s = String(s); if (el.textContent !== s) el.textContent = s; }
function applySkin() { island.dataset.skin = barEl.dataset.skin = S.dark ? 'dark' : 'light'; if (S.tint) setTint(S.tint); }
function applyStyle(style) {
  S.style = style === 'bar' || style === 'tall' ? style : 'island';
  const bar = isBar();
  BAR_H = S.style === 'tall' ? 48 : 40;
  document.body.classList.toggle('style-bar', bar);
  document.body.classList.toggle('bar-tall', S.style === 'tall');
  barEl.hidden = !bar;
  $$('#layoutSeg button').forEach(b => b.setAttribute('aria-pressed', b.dataset.style === S.style));
  if (bar && S.mode === 'peek') setMode('pill');
  lastRect = ''; layout(); renderBar(); renderDock(); renderBarTray();
  if (bar) {
    winsLoop(); loadTray();
    setTimeout(() => {
      const w = s => Math.round($(s).getBoundingClientRect().width);
      log(`bar layout: ${innerWidth}x${innerHeight} css px, scale ${devicePixelRatio}, left ${w('.bar-l')} center ${w('.bar-c')} right ${w('.bar-r')}`);
    }, 2500);
  }
}
$('#layoutSeg').addEventListener('click', async e => {
  const b = e.target.closest('button[data-style]'); if (!b || b.dataset.style === S.style) return;
  const style = b.dataset.style;
  // The top bar is drawn light like the reference; the island starts out dark. Dark mode still toggles both.
  // Going between the two bar heights keeps whatever skin is on.
  if (!(isBar() && style !== 'island')) { S.dark = style === 'island'; store.set('dark', S.dark); }
  b.textContent = 'Switching…';
  // The shell restarts in the new layout (about a second).
  try { await invoke('set_style', { style }); } catch (err) { note('#ctlNote', String(err)); }
});
/** Opens a panel dropping down from a bar chip; clicking the same chip again closes it. */
function openFromBar(tab, el) {
  const r = el.getBoundingClientRect();
  if (S.mode === 'panel' && S.tab === tab && Math.abs(S.anchor - (r.left + r.width / 2)) < 2) { closePanel(); return; }
  S.anchor = r.left + r.width / 2;
  if (S.mode === 'panel') { setTab(tab); layout(); } else openPanel(tab, false, true);
}
const deviceIcon = a => { const d = (a && a.device || '').toLowerCase(); return d.includes('headphone') || d.includes('headset') ? 'headphones' : d.includes('monitor') || d.includes('display') ? 'monitor' : 'speaker'; };
const batteryIcon = pct => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><rect x="2" y="7" width="17" height="10" rx="2.5"/><path d="M22 11v2"/><rect x="4" y="9" width="${(13 * pct / 100).toFixed(1)}" height="6" rx="1" fill="currentColor" stroke="none"/></svg>`;
function renderBar() {
  if (!isBar()) return;
  const st = S.status || { wifi: {}, bluetooth: {}, net: {}, battery: {}, desktops: null, cpu: 0 };
  const d = new Date();
  setHtml($('#barClock'), `<span>${d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })} ${d.toLocaleDateString(undefined, { weekday: 'short' })}</span>`);
  const a = S.audio;
  setHtml($('#barVol'), `${icon(a && (a.muted || a.level === 0) ? 'mute' : deviceIcon(a))}<span>${a ? (a.muted ? 'Muted' : Math.round(a.level * 100) + '%') : '–'}</span>`);
  const bt = $('#barBt'); bt.hidden = !st.bluetooth.present; bt.classList.toggle('on', !!st.bluetooth.on);
  setHtml(bt, `${icon('bt')}<span>${st.bluetooth.on ? 'on' : 'off'}</span>`);
  let net;
  if (st.wifi.present && st.wifi.on && st.net.kind === 'wifi') net = `${icon('wifi')}<span>${esc(st.net.name || 'Wi-Fi')}${st.net.bars != null ? ` (${st.net.bars * 20}%)` : ''}</span>`;
  else if (st.net.kind === 'ethernet') net = `${icon('ethernet')}<span>${esc(st.net.name || 'Ethernet')}</span>`;
  else net = `${icon('wifioff')}<span>${st.wifi.present && !st.wifi.on ? 'Wi-Fi off' : 'Offline'}</span>`;
  if (vpnOn()) net += `<span class="vpn-shield" title="VPN connected">${icon('shield')}</span>`;
  setHtml($('#barNet'), net);
  const bat = $('#barBat'), b = st.battery || {};
  bat.hidden = !b.present;
  if (b.present) { setHtml(bat, `${batteryIcon(b.percent)}<span>${b.percent}%${b.charging ? ' · charging' : ''}</span>`); bat.title = b.charging ? 'Charging' : 'On battery'; }
  const ct = st.cpu_temp;
  setHtml($('#barCpu'), `${icon('cpu')}<span>${Math.round(st.cpu || 0)}%${ct != null ? ` · <b class="${heat(ct, CPU_WARN)}">${Math.round(ct)}°</b>` : ''}</span>`);
  const m = S.media, mc = $('#barMedia');
  mc.hidden = !m;
  if (m) { setHtml(mc, `<span class="eq" aria-hidden="true"><i></i><i></i><i></i><i></i></span><span>${esc(m.title)}${m.artist ? ' · ' + esc(m.artist) : ''}</span>`); mc.title = `${m.title}${m.artist ? ' — ' + m.artist : ''} (${m.app})`; }
  barEl.classList.toggle('playing', !!(m && m.playing));
  renderWs(st.desktops);
}
function renderWs(ds) {
  ds = ds || { count: 1, current: 0, names: [''] };
  let html = '';
  for (let i = 0; i < ds.count; i++) html += `<button class="ws-b${i === ds.current ? ' on' : ''}" type="button" data-ws="${i}" title="${esc(ds.names[i] || 'Desktop ' + (i + 1))}">${i + 1}</button>`;
  html += '<button class="ws-b add" type="button" data-ws="new" title="New desktop" aria-label="New desktop">+</button>';
  setHtml($('#barWs'), html);
}
/** Pinned apps plus any other open windows, like taskbar buttons. */
function renderDock() {
  if (!isBar() || dragId) return;
  const items = [], seen = new Set();
  for (const id of S.pins) { const a = appOf(id); if (a && !seen.has(a.id)) { seen.add(a.id); items.push({ a, wins: winsFor(a) }); } }
  for (const w of S.wins) {
    const a = appForWin(w);
    if (a) { if (!seen.has(a.id)) { seen.add(a.id); items.push({ a, wins: winsFor(a) }); } }
    else if (!seen.has('h' + w.hwnd)) { seen.add('h' + w.hwnd); items.push({ w, wins: [w] }); }
  }
  setHtml($('#barDock'), items.map(it => {
    const name = it.a ? it.a.name : it.w.title, src = it.a && it.a.icon ? assetUrl(it.a.icon) : (it.w || it.wins[0] || {}).icon ? assetUrl((it.w || it.wins[0]).icon) : null;
    const ico = src ? `<img src="${esc(src)}" alt="">` : `<span class="g" style="background:${gradFor(name)}">${esc(glyphOf(name))}</span>`;
    const cls = [it.wins.length ? 'run' : '', it.wins.some(w => w.active) ? 'active' : ''].join(' ');
    const data = it.a ? `data-id="${esc(it.a.id)}"` : `data-h="${it.w.hwnd}"`;
    return `<button type="button" class="${cls}" ${data} title="${esc(name)}">${ico}</button>`;
  }).join(''));
}
function renderBarTray() {
  if (!isBar()) return;
  // Fewer inline tray icons on narrower (or more scaled) screens; the rest sit behind "+N".
  const max = innerWidth >= 2200 ? 14 : innerWidth >= 1700 ? 10 : innerWidth >= 1300 ? 6 : 3;
  const list = S.tray.slice(0, max);
  let html = list.map(t => `<button type="button" data-key="${esc(t.key)}" title="${esc(t.tip || t.exe)}" aria-label="${esc(t.tip || t.exe)}"><img src="${esc(t.icon)}" alt=""></button>`).join('');
  if (S.tray.length > max) html += `<button type="button" class="more" data-more title="More tray icons">+${S.tray.length - max}</button>`;
  setHtml($('#barTray'), html);
}
barEl.addEventListener('click', e => {
  const open = e.target.closest('[data-open]');
  if (open) { openFromBar(open.dataset.open, open); return; }
  if (e.target.closest('[data-more]')) { openFromBar('apps', $('#barApps')); return; }
  const dockBtn = e.target.closest('#barDock button');
  if (dockBtn) {
    if (dockBtn.dataset.id) { const a = appOf(dockBtn.dataset.id); if (a) openPinned(a); }
    else { const w = S.wins.find(x => String(x.hwnd) === dockBtn.dataset.h); if (w) activateWin(w); }
    return;
  }
  if (e.target.closest('#barFiles')) { launch({ id: 'exe:c:\\windows\\explorer.exe', name: 'File Explorer', icon: null }); return; }
  if (e.target.closest('#barBell')) { closePanel(true); quiet(invoke('open_settings', { uri: 'ms-actioncenter:' })); return; }
  if (e.target.closest('#barBt')) {
    const st = S.status; if (!st || !st.bluetooth.present) return;
    st.bluetooth.on = !st.bluetooth.on; renderBar(); renderControls();
    invoke('set_radio', { kind: 'bluetooth', on: st.bluetooth.on }).catch(err => toast("Couldn't change Bluetooth", String(err), 'bt', { sound: 'problem' }));
    if (S.mode !== 'panel') quiet(invoke('release_focus'));
    return;
  }
  const ws = e.target.closest('[data-ws]');
  if (ws) {
    if (ws.dataset.ws === 'new') quiet(invoke('desktop_new'));
    else {
      const st = S.status;
      if (st && st.desktops) { st.desktops.current = +ws.dataset.ws; renderWs(st.desktops); }
      quiet(invoke('desktop_switch', { index: +ws.dataset.ws }));
    }
  }
});
barEl.addEventListener('contextmenu', e => {
  if (e.target.closest('.tray-set')) return;
  e.preventDefault();
  const dockBtn = e.target.closest('#barDock button');
  if (!dockBtn) return;
  if (dockBtn.dataset.id) { const a = appOf(dockBtn.dataset.id); if (a) appMenu(a, e.clientX, e.clientY + 8); }
  else { const w = S.wins.find(x => String(x.hwnd) === dockBtn.dataset.h); if (w) winMenu(w, e.clientX, e.clientY + 8); }
});
// Scroll on the volume chip to change volume, on the desktops to switch desktop.
let barWheelT = 0;
barEl.addEventListener('wheel', e => {
  const vol = e.target.closest('#barVol'), ws = e.target.closest('#barWs');
  if (!vol && !ws) return;
  e.preventDefault();
  if (vol && S.audio) {
    const level = clamp(Math.round(S.audio.level * 100) + (e.deltaY < 0 ? 2 : -2), 0, 100) / 100;
    S.audio = { ...S.audio, level, muted: false }; S.volSetAt = performance.now();
    renderBar(); renderControls(); quiet(invoke('set_volume', { level }));
    return;
  }
  const now = performance.now(), ds = S.status && S.status.desktops;
  if (now < barWheelT || !ds || ds.count < 2) return;
  const next = clamp(ds.current + (e.deltaY > 0 ? 1 : -1), 0, ds.count - 1);
  if (next !== ds.current) { barWheelT = now + 450; ds.current = next; renderWs(ds); quiet(invoke('desktop_switch', { index: next })); }
}, { passive: false });
addEventListener('resize', () => { lastRect = ''; layout(); moveInk(); renderBarTray(); });

/* ---------- keyboard ---------- */
document.addEventListener('keydown', e => {
  const tgt = e.target, typing = !!(tgt.matches && tgt.matches('input,textarea') && !tgt.matches('[type="checkbox"]'));
  if (e.key === 'Escape') {
    if (!$('#ctx').hidden) { e.preventDefault(); hideCtx(); return; }
    if (S.mode === 'panel') { e.preventDefault(); closePanel(); }
    return;
  }
  if (S.mode !== 'panel') return;
  if (S.tab === 'clipboard') {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const ord = clipOrdered(); if (!ord.length) return; e.preventDefault();
      const idx = ord.findIndex(i => i.id === S.clipSel), n = clamp(idx + (e.key === 'ArrowDown' ? 1 : -1), 0, ord.length - 1);
      S.clipSel = ord[n].id; renderClip(); return;
    }
    if (e.key === 'Enter' && (tgt.id === 'cbSearch' || tgt === document.body || tgt.closest('.cb-list'))) { e.preventDefault(); copySel(); return; }
    if (!typing && (e.key === 'p' || e.key === 'P')) { togglePin(); return; }
    if (!typing && e.key === 'Delete') { delSel(); return; }
  }
  if (typing) return;
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
    e.preventDefault();
    const i = TABS.indexOf(S.tab) + (e.key === 'ArrowRight' ? 1 : -1);
    setTab(TABS[(i + TABS.length) % TABS.length]);
    if (document.activeElement && document.activeElement.classList.contains('tab')) $(`#tab-${S.tab}`).focus({ preventScroll: true });
    return;
  }
  if (/^[1-9]$/.test(e.key) && TABS[+e.key - 1]) setTab(TABS[+e.key - 1]);
});

/* ---------- clock ---------- */
function tickClock() {
  const d = new Date(), h12 = d.getHours() % 12 || 12, mm = String(d.getMinutes()).padStart(2, '0');
  $('#pillTime').textContent = `${h12}:${mm}`;
  $('#tHour').textContent = String(h12).padStart(2, '0'); $('#tMin').textContent = mm;
  $('#tSub').textContent = `${d.getHours() < 12 ? 'AM' : 'PM'} · ${d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric' })}`;
  if (d.getDate() !== weekDay) renderWeek();
}
setInterval(tickClock, 1000);

/* ---------- events from the shell ---------- */
// Win + Space (payload "cc") toggles the Command Center; another shortcut while it's open
// opens that section full size inside it.
listen('island:toggle', e => {
  if (e.payload === 'cc') { cmdCenter.toggle(); return; }
  if (cmdCenter.isOpen()) { if (!cmdCenter.openSheet(e.payload || 'apps')) cmdCenter.close(() => openPanel(e.payload || 'apps', true), true); return; }
  togglePanel(true, e.payload);
});
listen('island:style', e => { closePanel(true); applyStyle(e.payload); renderControls(); });
listen('island:fullscreen', e => {
  S.fullscreen = !!e.payload;
  island.classList.toggle('fs-hidden', S.fullscreen && S.mode !== 'panel' && !(S.urgent && S.mode === 'toast'));
  document.body.classList.toggle('fs', S.fullscreen);
});
listen('island:leave', onLeave);
listen('island:close', () => closePanel());
listen('clip:changed', loadClips);
listen('apps:ready', loadApps);
appWin.onFocusChanged(({ payload: focused }) => { if (!focused && S.mode === 'panel' && !S.ccOpen) closePanel(true); });
TABS.forEach(t => new ResizeObserver(() => { if (S.mode === 'panel' && S.tab === t) layout(); }).observe(views[t]));

/* ---------- modules (src/modhost.js) ---------- */
// Module tabs get a view as soon as they're registered (their UI mounts in it on first show);
// sections, Controls tiles and bar chips render from the registry; module notifications and
// activities arrive here from the backend (modules.rs) or the module's own UI.
const modHost = createModuleHost({
  R, invoke, listen, log,
  ui: {
    icon, esc, setHtml, txt, clip: clipTxt, grad: gradFor, glyph: glyphOf, ago,
    /** An error line inside a module's view: shown with text, hidden without. */
    note(el, msg) { if (!el) return; el.textContent = msg || ''; el.hidden = !msg; layout(); }
  },
  island: {
    layout: () => layout(),
    showing: tab => S.mode === 'panel' && S.tab === tab,
    closePanel: keep => closePanel(keep),
    releaseFocus: () => quiet(invoke('release_focus')),
    menu: (x, y, items) => showCtx(x, y, items),
    state: () => ({ mode: S.mode, tab: S.tab, dark: S.dark, style: S.style, cc: !!S.ccOpen, fullscreen: !!S.fullscreen }),
    on: (name, fn) => { const h = e => fn(e.detail); islandEvents.addEventListener(name, h); return () => islandEvents.removeEventListener(name, h); }
  },
  addView: () => syncViews(), removeView: () => syncViews(),
  notify: (module, n) => modToast(module, n),
  open: target => openTarget(target),
  setTiles: () => renderModTiles(), setChips: () => renderModChips(),
  activitiesChanged: () => islandEvents.dispatchEvent(new CustomEvent('activities')),
  sectionsChanged: () => syncSections(),
  tabFailed: (tab, name, e) => { const v = views[tab]; if (v) v.querySelector('.mod-fail').textContent = `${name} couldn't load: ${e && e.message || e}`; layout(); }
});
/** Island events modules can follow: 'panel' (open or closed), 'tab', 'skin', 'activities'. */
const islandEvents = new EventTarget();
/** Views for module tabs come and go with the registry. */
function syncViews() {
  const box = $('.views');
  for (const t of R.tabs.all()) {
    if (views[t.id]) continue;
    const v = document.createElement('section');
    v.className = 'view mod-view'; v.id = `v-${t.id}`; v.dataset.tab = t.id; v.setAttribute('role', 'tabpanel'); v.setAttribute('aria-labelledby', `tab-${t.id}`);
    v.innerHTML = '<div class="mod-host"></div><p class="note mod-fail"></p>';
    box.append(v); views[t.id] = v;
  }
  for (const id of Object.keys(views)) if (!R.tabs.has(id)) { views[id].remove(); delete views[id]; if (S.tab === id) { S.tab = 'controls'; views.controls.classList.add('on'); } }
}
R.tabs.on(() => { syncViews(); renderTabs(); syncTabs(); if (S.mode === 'panel') { moveInk(); layout(); } });
/** Which module tab (and sections) are on screen; the module hears show() and hide(). */
let modShown = null;
function syncModViews() {
  const now = S.mode === 'panel' ? S.tab : null;
  if (now !== modShown) {
    if (modShown) modHost.hideTab(modShown);
    modShown = now;
    if (now && R.tabs.get(now) && R.tabs.get(now).module) modHost.showTab(now, views[now].querySelector('.mod-host'));
    islandEvents.dispatchEvent(new CustomEvent('tab', { detail: now }));
  }
  syncSections();
}
/** Module sections inside base tabs: a host per section, in order, shown while their tab is. */
function syncSections() {
  for (const s of R.sections.all()) {
    const view = views[s.tab]; if (!view) continue;
    let box = view.querySelector(`.mod-sections[data-tab="${s.tab}"]`);
    if (!box) { box = Object.assign(document.createElement('div'), { className: 'mod-sections' }); box.dataset.tab = s.tab; view.append(box); }
    let host = box.querySelector(`[data-sec="${CSS.escape(s.id)}"]`);
    if (!host) { host = Object.assign(document.createElement('div'), { className: 'mod-section' }); host.dataset.sec = s.id; box.append(host); }
    modHost.showSection(s.id, host, S.mode === 'panel' && S.tab === s.tab);
  }
  $$('.mod-section').forEach(h => { if (!R.sections.has(h.dataset.sec)) h.remove(); });
}
function modToast(module, n) {
  const tab = n.action && n.action.startsWith('tab:') ? n.action.slice(4) : '';
  toast(clipTxt(n.title || module, 48), clipTxt(n.body || '', 72), n.icon && !/\./.test(n.icon) ? n.icon : 'bellon',
    { img: n.img || '', round: !!n.round, urgent: !!n.urgent, top: !!(n.urgent || n.top), ms: n.ms || (n.urgent ? 8000 : 5500), tab, sound: n.sound || '' });
}
listen('mod:notify', e => modToast(e.payload.module, e.payload));
/** Activities a module's backend sets (host.activity) sit in the same registry as the UI's own. */
listen('island:activities', e => {
  for (const a of R.activities.all()) if (a.backend) R.activities.remove(a.id);
  for (const a of e.payload || []) R.activities.set({ ...a, id: `${a.module}:${a.id}`, aid: a.id, backend: true });
  islandEvents.dispatchEvent(new CustomEvent('activities'));
});
function openTarget(target) {
  const [kind, id] = String(target || '').split(':');
  if (kind === 'tab' && R.tabs.has(id)) { if (cmdCenter.isOpen()) cmdCenter.close(() => openPanel(id, true), true); else openPanel(id); }
  else if (kind === 'cube') { if (!cmdCenter.isOpen()) cmdCenter.open().then(() => cmdCenter.openSheet(id)); else cmdCenter.openSheet(id); }
}
/** Controls tiles from modules, after the base's own. */
function renderModTiles() {
  const box = $('#modTiles'); if (!box) return;
  const list = R.tiles.all().filter(t => t.module && !t.hidden);
  setHtml(box, list.map(t => `<button class="tile" type="button" data-mtile="${esc(t.id)}" aria-pressed="${!!t.pressed}"><span class="ti">${icon(t.icon || 'grid')}</span><span class="tt"><b>${esc(t.title || '')}</b><small>${esc(t.sub || '')}</small></span></button>`).join(''));
  box.hidden = !list.length;
}
$('#modTiles').addEventListener('click', e => {
  const b = e.target.closest('[data-mtile]'); const t = b && R.tiles.get(b.dataset.mtile); if (!t || !t.run) return;
  note('#ctlNote', '');
  Promise.resolve().then(() => t.run()).catch(err => note('#ctlNote', String(err)));
});
/** Top-bar chips from modules (left: after the Games button; right: before the volume). */
function renderModChips() {
  for (const side of ['left', 'right']) {
    const box = $(side === 'left' ? '#barModsL' : '#barModsR'); if (!box) continue;
    const list = R.chips.all().filter(c => c.module && !c.hidden && (c.side || 'right') === side);
    setHtml(box, list.map(c => {
      const av = (c.avatars || []).map(a => `<img src="${esc(a.src || '')}" alt="" data-id="${esc(a.id || '')}"${a.talk ? ' class="talk"' : ''} referrerpolicy="no-referrer">`).join('');
      return `<button class="bchip${c.icon && !c.text && !av ? ' icon' : ''}${c.tone ? ' ' + esc(c.tone) : ''}" type="button" data-mchip="${esc(c.id)}" title="${esc(c.title || '')}" aria-label="${esc(c.title || c.text || '')}">${av ? `<span class="dc-stack">${av}</span>` : ''}${c.icon ? icon(c.icon) : ''}${c.text ? `<span>${esc(c.text)}</span>` : ''}</button>`;
    }).join(''));
  }
}
barEl.addEventListener('click', e => {
  const b = e.target.closest('[data-mchip]'); const c = b && R.chips.get(b.dataset.mchip); if (!c) return;
  e.stopPropagation();
  if (c.opens) { const [kind, id] = c.opens.split(':'); if (kind === 'tab') openFromBar(id, b); else openTarget(c.opens); }
  else if (c.run) Promise.resolve().then(() => c.run()).catch(err => toast(c.title || 'Couldn\'t do that', String(err), 'x', { sound: 'problem' }));
}, true);

/* ---------- boot ---------- */
setAccent(store.get('accent', TAKEN)); syncAccentMode();
applySkin();
invoke('get_style').then(applyStyle).catch(() => {});
$('#follow').checked = S.follow;
views[S.tab].classList.add('on'); syncTabs();
renderWallUI(); renderTodos(); renderTimer(); renderWeek(); tickClock(); renderMedia(); renderControls();
setMode('boot');
setTimeout(() => setMode('pill'), 450);
/* The Command Center shows a tab's own view full size by borrowing it from the island: the view
   moves into the sheet and the island acts as if its panel were open on that tab (so its refresh
   loops run), then the view goes back. */
let hosted = null;
function hostTab(t, box) {
  if (hosted && hosted.v === views[t]) return;
  unhostTab(true);
  if (S.tab !== t) switchView(t, true);
  setMode('panel');
  const v = views[t];
  hosted = { v, parent: v.parentNode, next: v.nextSibling };
  box.append(v); v.classList.add('cc-hosted', 'on');
  onTabShown(t);
}
function unhostTab(keepMode) {
  if (!hosted) return;
  const { v, parent, next } = hosted; hosted = null;
  v.classList.remove('cc-hosted');
  parent.insertBefore(v, next);
  if (!keepMode) setMode('pill');
}
const cmdCenter = createCommandCenter({
  S, invoke, assetUrl, esc, store, gradFor, glyphOf, icon, closePanel, R, activateModule: id => modHost.activate(id),
  openTab: t => openPanel(t, true),
  hostTab, unhost: () => unhostTab(),
  relayout: () => { lastRect = ''; layout(); },
  media: { curPos, seekTo, toggleLyrics: () => $('#muLyricsBtn').click() },
  games: { play: playGame, launchers: launcherApps, openApp: openPinned },
  setDark: on => { S.dark = on; store.set('dark', on); applySkin(); },
  apps: {
    appOf, appForWin, pinIdForWin, launch, openPinned, activate: activateWin,
    togglePin: togglePinApp, moveToFolder, saveFolders, changed: renderApps,
    toast: (title, body) => toast(title, body, 'x', { sound: 'problem' })
  },
  listen
});
$('#openTm').addEventListener('click', () => cmdCenter.openTaskManager());
if (FIRST_RUN) setTimeout(() => cmdCenter.openSetup(), 1800);
modHost.start();

pollStatus(); loadClips(); loadApps(); loadWeather(); vpnLoop(); loadDc();
setInterval(() => loadWeather(), 20 * 60e3);
if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => { tabLabelW = null; moveInk(); layout(); });
setTimeout(() => log(`ready: apps=${S.apps.length} clips=${S.clips.length} media=${S.media ? S.media.app + ' / ' + S.media.title : 'none'} audio=${S.audio ? S.audio.device + ' ' + Math.round(S.audio.level * 100) + '%' : 'none'} wifi=${JSON.stringify(S.status && S.status.wifi)} net=${JSON.stringify(S.status && S.status.net)}`), 6000);
