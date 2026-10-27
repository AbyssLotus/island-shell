import { WALLS, drawWall } from './engine.js';
import { createCommandCenter } from './cc.js';
import { createSounds } from './sounds.js';
import { createRegistry, topActivity, restWeather, restDate } from './registry.js';
import { createModuleHost } from './modhost.js';

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
/** A file on disk as a URL the page can load (URLs, like a module's images, pass through). */
const assetUrl = p => (/^(https?|data|blob):/.test(p) ? p : TAURI.core.convertFileSrc(p));
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
// The base's own tabs, [id, title, icon, panel width, order]. Modules add theirs from their manifests
// (src/modhost.js), slotting in by order: Music's 20, Canvas's 50, Discord's 70, Games' 80 and Destiny's 90 keep their old places.
[['controls', 'Controls', 'gear', 600, 10], ['audio', 'Audio', 'speaker', 680, 30], ['today', 'Today', 'calendar', 760, 40],
  ['network', 'Network', 'wifi', 680, 60],
  ['stats', 'Stats', 'pulse', 720, 100], ['clipboard', 'Clipboard', 'clip', 760, 110], ['apps', 'Apps', 'grid', 640, 120],
  ['wallpaper', 'Wallpaper', 'image', 660, 130], ['power', 'Power', 'power', 600, 140]]
  .forEach(([id, title, ic, width, order]) => R.tabs.add({ id, title, icon: ic, width, order }));
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
  status: null, media: null, mediaKey: '', meterFor: '', tickRaf: 0,
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

function setMode(m) {
  S.mode = m; island.dataset.mode = m;
  if (m !== 'toast') S.toastTop = false;
  if (m !== 'toast' && S.urgent) { S.urgent = false; island.classList.toggle('fs-hidden', !!S.fullscreen && m !== 'panel'); }
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
  syncModViews();
}
function setTab(t) {
  if (S.mode !== 'panel') { openPanel(t); return; }
  if (t === S.tab) return;
  switchView(t, false); layout(); moveInk(); onTabShown(t);
}
function onTabShown(t) {
  if (t === 'controls') mediaTick();
  if (t === 'stats') statsLoop();
  if (t === 'network') { renderNetNow(); layout(); }
  if (t === 'wallpaper') showWallpapers();
  if (t === 'audio') { renderAudioHead(); if (audioShowing()) { audioDevLoop(); audioAppsLoop(); } }
  if (t === 'clipboard') renderClip();
  if (t === 'today') { renderWeek(); tickClock(); }
  if (t === 'apps') {
    winsLoop(); loadTray();
    setTimeout(() => { if (S.tab === 'apps' && S.mode === 'panel' && !S.openFolder) $('#appSearch').focus({ preventScroll: true }); }, 260);
  }
}
/** In bar style, where a panel opened by hotkey drops down from. */
function anchorFor(tab) {
  const sel = { apps: '#barApps', controls: '#barGear', audio: '#barVol', music: '#barMedia', network: '#barNet', games: '#barGames', today: '#barClock', stats: '#barCpu', power: '#barPower' }[tab];
  const el = sel ? $(sel) : $(`[data-mchip][data-opens="tab:${CSS.escape(tab)}"]`);
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
L.peek.addEventListener('click', e => { if (e.target.closest('[data-act]')) return; if (S.media && S.media.action) openTarget(S.media.action); else openPanel(null); });
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

/* ---------- status: audio, radios ---------- */
const fmtT = s => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
async function pollStatus() {
  try { applyStatus(await invoke('get_status')); } catch (e) {}
  setTimeout(pollStatus, 1000);
}
function applyStatus(st) {
  S.status = st;
  if (st.audio && performance.now() - S.volSetAt > 1500) S.audio = st.audio;
  renderMediaSource(); renderControls(); renderBar();
  if (S.mode === 'panel' && S.tab === 'network') renderNetNow();
}

/* ---------- now playing (a module's `media` activity: the Music module's) ----------
   What's playing shows on the pill (cover and equalizer), the peek, the Controls card and the top
   bar, all drawn from the highest `media` activity; their buttons go back to its run(action). The
   equalizer follows the player's own sound: the activity's image names (`exe`), metered here.
   With nothing playing, the pill rests on the weather, the date and the clock (renderRest). */
function mediaOf(a) {
  if (!a) return null;
  return {
    title: a.text || '', artist: a.sub || '', app: a.app || '', playing: !!a.playing, position: +a.position || 0, duration: +a.duration || 0,
    sampled_at: +a.sampled_at || Date.now(), can_next: !!a.can_next, can_prev: !!a.can_prev, key: a.key || a.text || '',
    art: a.art || '', exe: a.exe || '', lyric: a.lyric || '', action: a.action || '', run: typeof a.run === 'function' ? a.run : null
  };
}
/** The activities changed: what's playing now, if anything. */
function syncMedia() {
  const m = mediaOf(topActivity(R.activities.all().filter(x => x.kind === 'media')));
  const changed = !!m !== !!S.media || !!(m && m.key !== S.mediaKey);
  S.media = m; S.mediaKey = m ? m.key : '';
  // The equalizer meters the player itself while it plays.
  const meter = m ? `${m.exe}|${m.playing}` : '';
  if (meter !== S.meterFor) { S.meterFor = meter; quiet(invoke('meter_follow', { exe: m ? m.exe : '', playing: !!(m && m.playing) })); }
  renderMedia(changed); renderControls(); renderBar(); mediaTick();
}
/** A now-playing button (prev, toggle, next), for the activity's module. */
function mediaAct(action) {
  const m = S.media; if (!m || !m.run) return;
  Promise.resolve().then(() => m.run(action)).catch(err => toast(clipTxt(m.app || 'Music', 48), clipTxt(String((err && err.message) || err), 72), 'x', { sound: 'problem' }));
}
const artEls = $$('[data-m="art"]'), titleEls = $$('[data-m="title"]'), artistEls = $$('[data-m="artist"]'), sourceEls = $$('[data-m="source"]'), durEls = $$('[data-m="dur"]'), toggleBtns = $$('[data-act="toggle"]');
function renderMediaSource() {
  const m = S.media;
  sourceEls.forEach(e => txt(e, m ? `${m.app}${S.audio ? ' · ' + S.audio.device : ''}` : (S.audio ? S.audio.device : 'Sound')));
}
/** The peek's second line with nothing playing: the weather, when there's a reading. */
function restLine() {
  const c = store.get('weather', null), w = restWeather(c, S.city, S.unit);
  return w ? `${w.temp}°${S.unit} · ${wmo(c.code)[0]}${c.name ? ' · ' + c.name : ''}` : '';
}
function renderMedia() {
  const m = S.media;
  island.classList.toggle('no-media', !m);
  island.classList.toggle('playing', !!(m && m.playing));
  const art = m && m.art ? `url("${m.art}") center / cover no-repeat` : m ? gradFor(m.title + m.artist) : 'var(--ink-3)';
  if (art !== S.artNow) { S.artNow = art; artEls.forEach(e => { e.style.setProperty('--art', art); e.classList.toggle('has-thumb', !!(m && m.art)); }); }
  const now = new Date();
  titleEls.forEach(e => txt(e, m ? m.title || 'Unknown title' : now.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' })));
  artistEls.forEach(e => txt(e, m ? m.artist || m.app : restLine()));
  renderMediaSource();
  durEls.forEach(e => txt(e, fmtT(m && m.duration)));
  toggleBtns.forEach(b => { const p = !!(m && m.playing); setHtml(b, icon(p ? 'pause' : 'play')); b.setAttribute('aria-label', p ? 'Pause' : 'Play'); b.disabled = !m; });
  $$('[data-act="next"]').forEach(b => { b.disabled = !(m && m.can_next); });
  $$('[data-act="prev"]').forEach(b => { b.disabled = !(m && m.can_prev); });
  // The line being sung, which the module follows in its lyrics.
  const line = m ? m.lyric : '';
  $$('[data-m="lyric"]').forEach(e => { if (e.textContent !== line) { e.textContent = line; e.style.animation = 'none'; void e.offsetWidth; e.style.animation = ''; } });
  S.lastSec = -1; paintProgress();
  renderRest();
}
/** Where the song is right now, carried forward from the last reading. */
function curPos() {
  const m = S.media; if (!m) return 0;
  const p = m.position + (m.playing ? Math.max(0, Date.now() - m.sampled_at) / 1000 : 0);
  return m.duration > 0 ? Math.min(p, m.duration) : p;
}
const mediaShowing = () => S.mode === 'peek' || (S.mode === 'panel' && S.tab === 'controls');
/** Runs every frame while a progress bar is on screen and the song is moving. */
function mediaTick() {
  if (S.tickRaf) return;
  const step = () => {
    S.tickRaf = 0;
    if (!S.media || !mediaShowing()) return;
    paintProgress();
    if (S.media.playing) S.tickRaf = requestAnimationFrame(step);
  };
  S.tickRaf = requestAnimationFrame(step);
}
const progBars = $$('[data-m="bar"]'), posTexts = $$('[data-m="pos"]');
function paintProgress() {
  const m = S.media, dur = m ? m.duration : 0, pos = curPos();
  const pct = dur > 0 ? clamp(pos / dur * 100, 0, 100) : 0;
  for (const e of progBars) e.style.width = pct.toFixed(2) + '%';
  const sec = Math.floor(pos);
  if (sec !== S.lastSec) { S.lastSec = sec; for (const e of posTexts) e.textContent = fmtT(pos); }
}
$('#v-controls .card.media').addEventListener('click', e => { if (!e.target.closest('[data-act]') && S.media && S.media.action) openTarget(S.media.action); });
// The island's own now-playing buttons (a module's views handle theirs inside their shadow roots).
document.addEventListener('click', e => {
  const b = e.target.closest('[data-act]'); if (!b || b.disabled) return;
  mediaAct(b.dataset.act);
  if (S.mode !== 'panel' && !S.ccOpen) quiet(invoke('release_focus'));
});

/* ---------- the pill at rest (§4.11) ----------
   Nothing playing, no call and no timer: the weather's icon and temperature, the date and the
   clock. With no city (or no reading in 3 hours), just the date and the clock. */
function renderRest() {
  const rest = !S.media && !island.classList.contains('in-call') && !S.timer.running;
  const w = rest ? restWeather(store.get('weather', null), S.city, S.unit) : null;
  island.classList.toggle('resting', rest);
  island.classList.toggle('rest-wx', !!w);
  const pr = $('#pillRest'), pd = $('#pillDate');
  pr.hidden = !w;
  if (w) setHtml(pr, `${icon(w.icon)}<span>${w.temp}°</span>`);
  pd.hidden = !rest;
  if (rest) txt(pd, restDate(new Date()));
  // The peek's square shows the weather too.
  const peekArt = $('.peek .art'), wx = !S.media && !!w;
  setHtml(peekArt, wx ? icon(w.icon) : '');
  peekArt.classList.toggle('wx', wx);
}

/* live levels from the player's audio session drive the equalizer bars */
// Only the bars themselves are restyled, 30 times a second. Variables on <body> restyled every
// element on the page each time, which piled up hundreds of MB of garbage between collections.
// Module views that glow with the music (the Music tab) hear the island's `level` event.
let lvl = 0, lvlOffT = 0;
function eqBars() { if (!S.eqBars || !S.eqBars.every(b => b.isConnected)) S.eqBars = $$('.eq i'); return S.eqBars; }
listen('media:level', e => {
  lvl = Math.max(Math.pow(clamp(+e.payload || 0, 0, 1), 0.7), lvl * 0.8); // quick attack, smooth release
  const t = performance.now() / 1000, bars = eqBars();
  for (let i = 0; i < bars.length; i++) {
    const k = i % 4, h = lvl * (0.5 + 0.5 * Math.abs(Math.sin(t * (2.3 + k * 1.4) + k * 1.9)));
    bars[i].style.transform = `scaleY(${(0.16 + 0.84 * h).toFixed(3)})`;
  }
  islandEvents.dispatchEvent(new CustomEvent('level', { detail: lvl }));
  if (!S.eqLive) { S.eqLive = true; document.body.classList.add('live-eq'); }
  if (!S.eqLogged && e.payload > 0.02) { S.eqLogged = true; log(`equalizer is following the music (peak ${(+e.payload).toFixed(2)})`); }
  clearTimeout(lvlOffT);
  lvlOffT = setTimeout(() => {
    S.eqLive = false; document.body.classList.remove('live-eq');
    for (const b of eqBars()) b.style.transform = '';
    lvl = 0; islandEvents.dispatchEvent(new CustomEvent('level', { detail: 0 }));
  }, 500);
});



/* ---------- audio: master volume, devices, microphone, per-app mixer ---------- */
S.audioDev = null; S.audioApps = []; S.auDevT = 0; S.auAppsT = 0;
const audioShowing = () => S.mode === 'panel' && S.tab === 'audio' && auView() === 'win';
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

/* ---------- audio views ----------
   Module sections with `view` (Voicemeeter's mixer) sit between the tab's own Windows mixer and its
   notification sounds, picked in the tab's header. Until one is picked, the first module view shows
   (the mixer it was installed for), else the Windows mixer. */
S.auMode = store.get('auMode', '');
if (S.auMode === 'vm') S.auMode = ''; // 0.1.x, when Voicemeeter was built in
const auViews = () => R.sections.all().filter(s => s.tab === 'audio' && s.view).sort((a, b) => a.order - b.order);
/** 'win', 'sounds', or a module view's section id. */
function auView() {
  if (S.auMode === 'win' || S.auMode === 'sounds') return S.auMode;
  const v = auViews();
  return v.some(x => x.id === S.auMode) ? S.auMode : v.length ? v[0].id : 'win';
}
function renderAudioHead() {
  const cur = auView();
  setHtml($('#auMode'), [['win', 'Windows'], ...auViews().map(x => [x.id, x.title]), ['sounds', 'Sounds']].map(([id, t]) => `<button type="button" data-mode="${esc(id)}" aria-pressed="${id === cur}">${esc(t)}</button>`).join(''));
  $('#auWin').hidden = cur !== 'win'; $('#auSnd').hidden = cur !== 'sounds';
  if (cur === 'sounds') renderSounds();
  syncSections();
  if (S.mode === 'panel' && S.tab === 'audio') layout();
}
$('#auMode').addEventListener('click', e => {
  const b = e.target.closest('[data-mode]'); if (!b) return;
  S.auMode = b.dataset.mode; store.set('auMode', S.auMode);
  renderAudioHead();
  if (audioShowing()) { audioDevLoop(); audioAppsLoop(); }
});
R.sections.on(() => renderAudioHead());
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

/* ---------- eye tracking: a module's `presence` (the eye tracking module's) ----------
   Eyes on the island count like hovering it (the pill opens its peek), and while someone is in
   front of the tracker a notification waits until it has been seen. */
S.gaze = false; S.presence = ''; S.toastWait = null;
const gazeLive = () => S.presence === 'present' || S.presence === 'looking';
function onPresence(p) {
  S.presence = (p && p.state) || '';
  const looking = S.presence === 'looking';
  if (looking === S.gaze) return;
  S.gaze = looking;
  if (S.gaze) {
    if (S.mode === 'toast' && S.toastWait) S.toastWait();
    else if (S.mode === 'pill' && !isBar()) setMode('peek');
  } else if (S.mode === 'peek' && !S.hover) {
    clearTimeout(hoverT);
    hoverT = setTimeout(() => { if (S.mode === 'peek' && !S.hover && !S.gaze) setMode('pill'); }, 120);
  }
}
listen('topic:presence', e => onPresence(e.payload));
invoke('topic_last', { topic: 'presence' }).then(onPresence).catch(() => {});

/* ---------- network ---------- */
function ago(iso) {
  const t = Date.parse(iso); if (!t || t < 946684800000) return '';
  const m = Math.round((Date.now() - t) / 60000);
  if (m < 2) return 'just now';
  if (m < 60) return `${m} min ago`;
  if (m < 60 * 24) return `${Math.round(m / 60)} h ago`;
  if (m < 60 * 48) return 'yesterday';
  return new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
/** What the PC is connected to, at the top of the Network tab (VPNs come from their module). */
function renderNetNow() {
  const st = S.status; if (!st) return;
  const n = st.net || {};
  setHtml($('#netNow'), n.kind === 'none' || !n.kind ? `${icon('wifioff')}<span>Offline</span>`
    : `${icon(n.kind === 'ethernet' ? 'ethernet' : 'wifi')}<span><b>${esc(n.name || (n.kind === 'ethernet' ? 'Ethernet' : 'Wi-Fi'))}</b> · ${n.kind === 'ethernet' ? 'Ethernet' : 'Wi-Fi'}${n.bars != null && n.kind === 'wifi' ? ` · ${n.bars * 20}% signal` : ''}</span>`);
}

/* ---------- controls ---------- */
/** A line of text at most n characters long (for notifications and the like). */
const clipTxt = (s, n) => { s = String(s || '').replace(/\s+/g, ' ').trim(); return s.length > n ? s.slice(0, n - 1) + '…' : s; };

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
  renderAudioMaster();
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
  renderRest(); renderMedia(); renderBar();
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
  renderRest();
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

/* ---------- wallpaper views ----------
   Module sections with `view` (Wallpaper Engine's library) are alternatives to the island's own
   scenes, picked in the tab's header; the first one shows unless the island's scenes were picked. */
S.wpMode = store.get('wpMode', 'we');
const wpViews = () => R.sections.all().filter(s => s.tab === 'wallpaper' && s.view).sort((a, b) => a.order - b.order);
function wpView() {
  const v = wpViews();
  if (S.wpMode === 'island' || !v.length) return 'island';
  return v.some(x => x.id === S.wpMode) ? S.wpMode : v[0].id;
}
/** A tab's current view: a module section's id, or '' for the tab's own content (Wallpaper and Audio have views). */
function viewOf(tab) {
  if (tab === 'wallpaper') return wpView() !== 'island' ? wpView() : '';
  if (tab === 'audio') { const v = auView(); return v === 'win' || v === 'sounds' ? '' : v; }
  return '';
}
function showWallpapers() { renderWpMode(); }
function renderWpMode() {
  const v = wpViews(), cur = wpView(), has = v.length > 0;
  $('#wpMode').hidden = !has;
  if (has) setHtml($('#wpMode'), [...v.map(x => [x.id, x.title]), ['island', 'Island scenes']].map(([id, t]) => `<button type="button" data-mode="${esc(id)}" aria-pressed="${id === cur}">${esc(t)}</button>`).join(''));
  $('#islandWalls').hidden = cur !== 'island';
  $('#wpTitle').hidden = has; $('#wpSub').hidden = has;
  syncSections();
  if (S.mode === 'panel' && S.tab === 'wallpaper') layout();
}
$('#wpMode').addEventListener('click', e => {
  const b = e.target.closest('[data-mode]'); if (!b) return;
  S.wpMode = b.dataset.mode; store.set('wpMode', S.wpMode); renderWpMode();
});
R.sections.on(() => renderWpMode());
/** A lively accent from the wallpaper's thumbnail (its most vivid colors, kept readable). */
function accentFrom(url) {
  const img = new Image();
  if (url.startsWith('http://mod.localhost/')) img.crossOrigin = 'anonymous'; // a module's image: its pixels are readable with CORS
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
  // Modules: the one updating now, or the updates waiting (when they don't update by themselves).
  const mods = u.modules || [];
  const modLine = u.module_busy ? `Updating ${u.module_busy}…` : mods.length ? `Module updates: ${mods.map(m => `${m.name} ${m.to}`).join(', ')}` : '';
  txt($('#updSub'), [sub, modLine].filter(Boolean).join(' · '));
  $('#updSub').title = [u.notes || sub, modLine].filter(Boolean).join('\n');
  $('#updAuto').checked = !!u.auto; $('#updAuto').disabled = off;
  const b = $('#updBtn'), modsOnly = !updReady(u) && mods.length > 0;
  b.hidden = off;
  b.disabled = ['checking', 'downloading', 'installing'].includes(u.state) || !!u.module_busy;
  setHtml(b, updReady(u) ? `${icon('download')}Update` : modsOnly ? `${icon('download')}Update modules` : 'Check now');
  b.classList.toggle('primary', updReady(u) || modsOnly);
}
function toastUpdate(u) {
  toast('Update available', `Island Shell ${u.latest} · open Power to install`, 'download', { tab: 'power', top: true, ms: 6000 });
}
$('#updBtn').addEventListener('click', () => {
  const u = S.upd || {};
  quiet(invoke(updReady(u) ? 'update_install' : (u.modules || []).length ? 'update_modules' : 'update_check'));
});
// A module updated (by itself, or from the button), or installed from the queue, says so once.
listen('update:module', e => {
  const m = e.payload || {};
  if (m.ok) toast(m.installed ? `${m.name} is installed` : `${m.name} updated`, `Version ${m.version}`, 'download', { ms: 4000, sound: 'small' });
  else toast(m.installed ? `${m.name} didn't install` : `${m.name} didn't update`, m.installed ? `It'll try again later · ${m.error || ''}` : m.error || '', 'x', { ms: 6000, sound: 'problem' });
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
function applySkin() { island.dataset.skin = barEl.dataset.skin = S.dark ? 'dark' : 'light'; }
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
  // The weather's icon and temperature just left of the clock (§4.11).
  const wx = restWeather(store.get('weather', null), S.city, S.unit);
  setHtml($('#barClock'), `${wx ? `<span class="bar-wx">${icon(wx.icon)}${wx.temp}°</span>` : ''}<span>${d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })} ${d.toLocaleDateString(undefined, { weekday: 'short' })}</span>`);
  const a = S.audio;
  setHtml($('#barVol'), `${icon(a && (a.muted || a.level === 0) ? 'mute' : deviceIcon(a))}<span>${a ? (a.muted ? 'Muted' : Math.round(a.level * 100) + '%') : '–'}</span>`);
  const bt = $('#barBt'); bt.hidden = !st.bluetooth.present; bt.classList.toggle('on', !!st.bluetooth.on);
  setHtml(bt, `${icon('bt')}<span>${st.bluetooth.on ? 'on' : 'off'}</span>`);
  let net;
  if (st.wifi.present && st.wifi.on && st.net.kind === 'wifi') net = `${icon('wifi')}<span>${esc(st.net.name || 'Wi-Fi')}${st.net.bars != null ? ` (${st.net.bars * 20}%)` : ''}</span>`;
  else if (st.net.kind === 'ethernet') net = `${icon('ethernet')}<span>${esc(st.net.name || 'Ethernet')}</span>`;
  else net = `${icon('wifioff')}<span>${st.wifi.present && !st.wifi.on ? 'Wi-Fi off' : 'Offline'}</span>`;
  // Badges modules put on the network chip (the VPN module's shield while one is connected).
  for (const c of R.chips.all()) if (c.module && !c.hidden && c.in === 'net') net += `<span class="net-badge" title="${esc(c.title || '')}">${icon(c.icon || 'dot')}</span>`;
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
  if (d.getMinutes() !== S.restMin) { S.restMin = d.getMinutes(); renderRest(); }
}
setInterval(tickClock, 1000);

/* ---------- events from the shell ---------- */
// Win + Space (payload "cc") toggles the Command Center; another shortcut while it's open
// opens that section full size inside it.
listen('island:toggle', e => {
  if (e.payload === 'cc') { cmdCenter.toggle(); return; }
  if (String(e.payload).startsWith('cube:')) { openTarget(e.payload); return; }
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
    /** The accent from an image (a module's wallpaper), when the accent follows the wallpaper. */
    accentFrom: url => { if (S.follow) accentFrom(url); },
    /** For a module's Command Center sheet: a short message in it, and closing it before `then`. */
    flash: msg => cmdCenter.flash(msg),
    closeCommandCenter: then => cmdCenter.close(then, true),
    /** The Start menu's apps (icons as URLs, `running` when one has a window), to open or offer a menu for. */
    apps: {
      list: () => (S.apps || []).map(a => ({ id: a.id, name: a.name, icon: a.icon ? assetUrl(a.icon) : null, running: winsFor(a).length > 0 })),
      open: id => { const a = appOf(id); if (a) openPinned(a); },
      menu: (id, x, y) => { const a = appOf(id); if (a) appMenu(a, x, y); }
    },
    showing: tab => S.mode === 'panel' && S.tab === tab,
    closePanel: keep => closePanel(keep),
    releaseFocus: () => quiet(invoke('release_focus')),
    /** Brings a module's app forward ({ id, exe, hwnd }): the island does it, being the one in front. */
    focusApp: app => invoke('focus_app', { id: app.id || '', exe: app.exe || '', hwnd: app.hwnd || null }),
    /** A program's own volume by its image names (`|`-separated), like the Volume mixer's. */
    audio: { appVolume: exe => invoke('app_volume_get', { exe }), setAppVolume: (exe, level) => invoke('app_volume_set', { exe, level }) },
    menu: (x, y, items) => showCtx(x, y, items),
    state: () => ({ mode: S.mode, tab: S.tab, dark: S.dark, style: S.style, cc: !!S.ccOpen, fullscreen: !!S.fullscreen }),
    on: (name, fn) => { const h = e => fn(e.detail); islandEvents.addEventListener(name, h); return () => islandEvents.removeEventListener(name, h); }
  },
  addView: () => syncViews(), removeView: () => syncViews(),
  notify: (module, n) => modToast(module, n),
  open: target => openTarget(target),
  setTiles: () => renderModTiles(), setChips: () => { renderModChips(); renderBar(); },
  activitiesChanged: () => islandEvents.dispatchEvent(new CustomEvent('activities')),
  sectionsChanged: () => syncSections(),
  tabFailed: (tab, name, e) => { const v = views[tab]; if (v) v.querySelector('.mod-fail').textContent = `${name} couldn't load: ${e && e.message || e}`; layout(); }
});
/** Island events modules can follow: 'panel' (open or closed), 'tab', 'skin', 'activities', 'level' (the equalizer's, 0–1). */
const islandEvents = new EventTarget();
/* ---------- a call on the pill and the top bar (a module's `call` activity, like Discord's) ----------
   Who's in it (up to three faces, a green ring on whoever's talking) and your own mute or deafen,
   beside the clock. Talking flips many times a second, so while the faces stay the same only their
   rings change, here and on the bar's chips. */
S.callW = 0;
/** Extra pill width for the faces (and the muted mic). */
function callW() { return S.callW || 0; }
const faces = list => list.map(a => `<img src="${esc(/^https:\/\//.test(a.src || '') ? a.src : '')}" alt="" data-id="${esc(a.id || '')}" referrerpolicy="no-referrer">`).join('');
function talkRings(box, list) {
  const t = new Set(list.filter(a => a.talk).map(a => a.id));
  for (const img of box.querySelectorAll('img[data-id]')) img.classList.toggle('talk', t.has(img.dataset.id));
}
function renderPillCall() {
  const a = topActivity(R.activities.all().filter(x => x.kind === 'call')), av = ((a && a.avatars) || []).slice(0, 3);
  const pill = $('#pillCall'), on = av.length > 0;
  island.classList.toggle('in-call', on);
  pill.hidden = !on;
  let w = 0;
  if (on) {
    setHtml(pill, `<span class="call-stack">${faces(av)}</span>${a.icon ? icon(a.icon) : ''}`);
    talkRings(pill, av);
    w = 9 + 18 + (av.length - 1) * 12 + (a.icon ? 17 : 0);
  }
  if (w !== S.callW) { S.callW = w; layout(); }
  renderRest();
}
islandEvents.addEventListener('activities', renderPillCall);
islandEvents.addEventListener('activities', syncMedia);
/** Views for module tabs come and go with the registry. */
function syncViews() {
  const box = $('.views');
  for (const t of R.tabs.all()) {
    if (views[t.id]) continue;
    const v = document.createElement('section');
    v.className = 'view mod-view'; v.id = `v-${t.id}`; v.dataset.tab = t.id; v.setAttribute('role', 'tabpanel'); v.setAttribute('aria-labelledby', `tab-${t.id}`);
    v.innerHTML = '<div class="mod-host"></div><p class="note mod-fail"></p>';
    box.append(v); views[t.id] = v;
    // Its size settles as its stylesheet and pictures arrive: follow it, like the base's views.
    new ResizeObserver(() => { if (S.mode === 'panel' && S.tab === t.id) layout(); }).observe(v);
  }
  for (const id of Object.keys(views)) if (!R.tabs.has(id)) { views[id].remove(); delete views[id]; if (S.tab === id) { S.tab = 'controls'; views.controls.classList.add('on'); } }
}
R.tabs.on(() => { syncViews(); renderTabs(); syncTabs(); syncBarTabs(); if (S.mode === 'panel') { moveInk(); layout(); } });
/** The bar's Games button opens a module's tab: it shows only while that tab exists. */
function syncBarTabs() { $('#barGames').hidden = !R.tabs.has('games'); }
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
    // A view goes in the tab's own .mod-views box and shows only while it's the one picked.
    let box = s.view ? view.querySelector('.mod-views') : view.querySelector(`.mod-sections[data-tab="${s.tab}"]`);
    if (!box && s.view) continue;
    if (!box) { box = Object.assign(document.createElement('div'), { className: 'mod-sections' }); box.dataset.tab = s.tab; view.append(box); }
    let host = box.querySelector(`[data-sec="${CSS.escape(s.id)}"]`);
    if (!host) { host = Object.assign(document.createElement('div'), { className: 'mod-section' }); host.dataset.sec = s.id; box.append(host); }
    const on = S.mode === 'panel' && S.tab === s.tab && (!s.view || viewOf(s.tab) === s.id);
    host.hidden = !!s.view && viewOf(s.tab) !== s.id;
    modHost.showSection(s.id, host, on);
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
/** 'tab:<id>', 'cube:<id>', or 'cube:<id>:web' for a cube's docked site (signing in to Canvas). */
function openTarget(target) {
  const [kind, id, mode] = String(target || '').split(':');
  const how = mode === 'web' || mode === 'island' ? mode : undefined;
  if (kind === 'tab' && R.tabs.has(id)) { if (cmdCenter.isOpen()) cmdCenter.close(() => openPanel(id, true), true); else openPanel(id); }
  else if (kind === 'cube') { if (!cmdCenter.isOpen()) cmdCenter.open().then(() => cmdCenter.openSheet(id, null, how)); else cmdCenter.openSheet(id, null, how); }
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
    const list = R.chips.all().filter(c => c.module && !c.hidden && !c.in && (c.side || 'right') === side);
    setHtml(box, list.map(c => {
      const av = faces(c.avatars || []);
      return `<button class="bchip${c.icon && !c.text && !av ? ' icon' : ''}${c.tone ? ' ' + esc(c.tone) : ''}" type="button" data-mchip="${esc(c.id)}"${c.opens ? ` data-opens="${esc(c.opens)}"` : ''} title="${esc(c.title || '')}" aria-label="${esc(c.title || c.text || '')}">${av ? `<span class="call-stack">${av}</span>` : ''}${c.icon ? icon(c.icon) : ''}${c.text ? `<span>${esc(c.text)}</span>` : ''}</button>`;
    }).join(''));
    for (const c of list) if (c.avatars && c.avatars.length) { const b = box.querySelector(`[data-mchip="${CSS.escape(c.id)}"]`); if (b) talkRings(b, c.avatars); }
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
  settingsOf: id => modHost.settingsOf(id),
  // Modules with a setup card (their manifest's contributes.setup) join the guide's Connect step.
  moduleCards: () => [...modHost.mods.values()].filter(m => m.man.contributes?.setup?.card && m.man.contributes.tabs?.length).map(m => {
    const t = m.man.contributes.tabs[0], card = R.setup.get(m.man.id);
    let state = '';
    try { state = String(((card && card.render && card.render()) || {}).state || ''); } catch (e) {}
    return { id: m.man.id, icon: /\.svg$/.test(t.icon) ? 'grid' : t.icon, title: m.man.name, sub: m.man.description, tab: t.id, state };
  }),
  openTab: t => openPanel(t, true),
  hostTab, unhost: () => unhostTab(),
  relayout: () => { lastRect = ''; layout(); },
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
// 0.1.x had every integration built in: the first start after the upgrade asks for the modules it
// was using (modules.rs, mod_migrate); they install in the background.
else if (!store.get('modulesMigrated', false)) invoke('mod_migrate').then(() => store.set('modulesMigrated', true)).catch(() => {});
modHost.start();

pollStatus(); loadClips(); loadApps(); loadWeather();
setInterval(() => loadWeather(), 20 * 60e3);
if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => { tabLabelW = null; moveInk(); layout(); });
setTimeout(() => log(`ready: apps=${S.apps.length} clips=${S.clips.length} media=${S.media ? S.media.app + ' / ' + S.media.title : 'none'} audio=${S.audio ? S.audio.device + ' ' + Math.round(S.audio.level * 100) + '%' : 'none'} wifi=${JSON.stringify(S.status && S.status.wifi)} net=${JSON.stringify(S.status && S.status.net)}`), 6000);
