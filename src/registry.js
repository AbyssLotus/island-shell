// The island's registries (docs/design/modules-handoff.md, M0.3): every tab, tab section, Controls
// tile, bar chip, Command Center cube, docked site, native sheet, search source, command, hotkey,
// setup card and live activity comes from here. The base registers its own first, modules after
// (src/modhost.js), and the page renders whatever is registered. Nothing in this file touches the
// DOM, so its rules run in Node too (dev/tests/registry.test.js).

/** One kind of contribution: items by id, in order, with change listeners. */
export class List {
  constructor(sort) {
    this.items = new Map();
    this.sort = sort || null;
    this.subs = new Set();
  }
  /** Adds an item (it must have a unique `id`); returns a function that removes it again. */
  add(item) {
    if (!item || typeof item.id !== 'string' || !item.id) throw new Error('A registered item needs an id');
    if (this.items.has(item.id)) throw new Error(`${item.id} is already registered`);
    this.items.set(item.id, item);
    this.changed();
    return () => { if (this.items.get(item.id) === item) this.remove(item.id); };
  }
  /** Replaces an item with the same id, or adds it. */
  set(item) {
    this.items.set(item.id, item);
    this.changed();
  }
  remove(id) { if (this.items.delete(id)) this.changed(); }
  /** Removes everything a module registered. */
  removeModule(module) {
    let any = false;
    for (const [id, it] of this.items) if (it.module === module) { this.items.delete(id); any = true; }
    if (any) this.changed();
  }
  get(id) { return this.items.get(id); }
  has(id) { return this.items.has(id); }
  all() { const v = [...this.items.values()]; return this.sort ? v.sort(this.sort) : v; }
  ids() { return this.all().map(x => x.id); }
  get size() { return this.items.size; }
  on(fn) { this.subs.add(fn); return () => this.subs.delete(fn); }
  changed() { for (const fn of this.subs) fn(this); }
}

export const byOrder = (a, b) => (a.order ?? 0) - (b.order ?? 0) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

export function createRegistry() {
  return {
    /** Island tabs: { id, title, icon, order, width, module? }. Base tabs use multiples of 10. */
    tabs: new List(byOrder),
    /** Parts of a base tab that a module adds: { id, tab, order, module }. */
    sections: new List(byOrder),
    /** Controls tiles: { id, order, icon, title, sub, pressed, hidden, run }. */
    tiles: new List(byOrder),
    /** Top-bar chips: { id, side: 'left' | 'right', order, ... }. */
    chips: new List(byOrder),
    /** Command Center cubes: { id, title, icon, size, slot?, module?, stat?() }. */
    cubes: new List(),
    /** Real websites a cube can dock: { id (the cube), site, label, default, ready?() }. */
    sites: new List(),
    /** Cubes that open their own app in the sheet: { id (the cube), mount(el), unmount(), key(e) }. */
    sheets: new List(),
    /** Command Center search sources: { id, kind, label, icon, items() }. */
    search: new List(),
    /** Quick commands for search: { id, title, icon, sub, keys, run, stay? }. */
    commands: new List(),
    /** Win + letter: { id (the letter), opens, module? }. */
    hotkeys: new List(),
    /** Cards in the setup guide's module step: { id, order, ... }. */
    setup: new List(byOrder),
    /** Live things on the pill: { id, module, kind, priority?, seq, ... }. */
    activities: new List()
  };
}

/* ---------- Command Center cube layout ----------
   Six cubes keep a place of their own: Modules, Apps, Games, Audio, Clipboard and Music. Every
   other cube (Today, Network, Stats and every other module's) is ranked by the user: the first ones
   sit nearest the eclipse and the rest fill outward, smaller the further out, as many as the screen
   has room for. When there are more, the last seat holds "+N more". The island decides all of
   this; a module's manifest has no say in where its cube goes. Positions and sizes are in eclipse
   radii from its center. */

/** The cubes that stay put: [id, x, y, size]. */
export const FIXED = [
  ['apps', -1.8, -1.12, 'L'], ['audio', -2, 1.1, 'L'], ['games', 1.72, -1.1, 'M'],
  ['clipboard', 1.92, 1.14, 'M'], ['music', 0, 1.62, 'W'], ['modules', 0, -1.62, 'S']
];
export const isFixed = id => FIXED.some(f => f[0] === id);
/** The seats nearest the eclipse, best first: the island's arrangement from before. */
export const NEAR = [[-2.52, -0.02, 'M'], [2.48, 0.04, 'M'], [-3.3, -0.84, 'S'], [3.3, -0.84, 'S'], [-3.42, 0.95, 'S'], [3.42, 1.02, 'S']];
/** Cube sizes, [width, height]. */
export const CSIZE = { L: [.9, .9], M: [.74, .74], S: [.52, .52], XS: [.42, .42], W: [1.14, .72] };
/** The ranking before the user picks one, best first; other cubes follow in the order they came. */
export const DEFAULT_ORDER = ['discord', 'network', 'canvas', 'destiny', 'today', 'stats'];
/** The id of the "+N more" cube. */
export const MORE = '__more';
/** A 2560 × 1440 screen, for when there's no screen to measure (tests). */
export const WIDE = { left: -5.9, right: 5.9, top: -3.1, bottom: 3.2 };

const GAP = .12; // the near seats from before sit this close
function overlaps(a, b) {
  const [aw, ah] = CSIZE[a.size], [bw, bh] = CSIZE[b.size];
  return Math.abs(a.x - b.x) < (aw + bw) / 2 + GAP && Math.abs(a.y - b.y) < (ah + bh) / 2 + GAP;
}

/**
 * How far out a seat is, for ranking: measured against the screen's own shape (an ellipse), so a
 * wide screen fills its sides before its top and bottom edges. 1 is the edge of the screen.
 */
export function farness(s, bounds) {
  return Math.hypot(s.x / Math.max(bounds.right, -bounds.left), s.y / Math.max(bounds.bottom, -bounds.top));
}

/**
 * The seats for ranked cubes, best first, inside `bounds` ({ left, right, top, bottom }) and clear
 * of `taken` (the fixed cubes there are): the near ones, then a staggered grid further out, nearest
 * first (farness) and smaller further out, plus the place of a fixed cube that isn't there.
 */
export function seats(bounds, taken, spare = []) {
  const inside = s => { const [w, h] = CSIZE[s.size]; return s.x - w / 2 >= bounds.left && s.x + w / 2 <= bounds.right && s.y - h / 2 >= bounds.top && s.y + h / 2 <= bounds.bottom; };
  const out = [];
  const add = s => { if (inside(s) && !taken.some(t => overlaps(s, t)) && !out.some(t => overlaps(s, t))) out.push(s); };
  for (const [x, y, size] of NEAR) add({ x, y, size });
  const far = spare.map(s => ({ ...s, d: farness(s, bounds) }));
  const DX = .86, DY = .82;
  for (let row = Math.ceil(bounds.top / DY); row * DY <= bounds.bottom; row++) {
    const off = row % 2 ? DX / 2 : 0;
    for (let col = Math.ceil((bounds.left - off) / DX); col * DX + off <= bounds.right; col++) {
      const x = +(col * DX + off).toFixed(3), y = +(row * DY).toFixed(3), d = farness({ x, y }, bounds);
      if (Math.hypot(x, y) >= 2.3) far.push({ x, y, d, size: d < .78 ? 'S' : 'XS' }); // nearer is the eclipse's corona
    }
  }
  far.sort((a, b) => a.d - b.d || Math.abs(a.y) - Math.abs(b.y) || a.x - b.x);
  for (const { x, y, size } of far) add({ x, y, size });
  return out;
}

/** Ranked cubes in the user's order (`order`, ids), then DEFAULT_ORDER's, then the order they came. */
export function rankCubes(cubes, order = []) {
  const at = c => {
    const i = order.indexOf(c.id);
    if (i >= 0) return i;
    const d = DEFAULT_ORDER.indexOf(c.id);
    return order.length + (d >= 0 ? d : DEFAULT_ORDER.length + cubes.indexOf(c));
  };
  return [...cubes].sort((a, b) => at(a) - at(b));
}

/**
 * Where every cube goes: { placed: [{ cube, seat: { x, y, size }, fixed }], ranked (every ranked
 * cube, best first), overflow (the ones in "+N more"), hidden, keys (ids in keyboard order) }.
 * `order` is the user's ranking and `hidden` the cubes they took off the screen (fixed ones can't
 * be). A "+N more" cube is { id: MORE, title, icon, more: [ids] }.
 */
export function layoutCubes(cubes, { order = [], hidden = [], bounds = WIDE } = {}) {
  const fixedAt = new Map(FIXED.map(([id, x, y, size]) => [id, { x, y, size }]));
  const placed = [], movable = [], off = [];
  for (const c of cubes) {
    if (fixedAt.has(c.id)) placed.push({ cube: c, seat: fixedAt.get(c.id), fixed: true });
    else (hidden.includes(c.id) ? off : movable).push(c);
  }
  const ranked = rankCubes(movable, order);
  // A fixed cube that isn't there (no Music module) leaves its place to the ranked ones.
  const spare = FIXED.filter(([id]) => !cubes.some(c => c.id === id)).map(([, x, y]) => ({ x, y, size: 'M' }));
  const room = seats(bounds, placed.map(p => p.seat), spare);
  let shown = ranked, overflow = [];
  if (ranked.length > room.length) { shown = ranked.slice(0, Math.max(0, room.length - 1)); overflow = ranked.slice(shown.length); }
  shown.forEach((c, i) => placed.push({ cube: c, seat: room[i], fixed: false }));
  if (overflow.length && room[shown.length]) placed.push({ cube: { id: MORE, title: `${overflow.length} more`, icon: 'grid', more: overflow.map(c => c.id) }, seat: room[shown.length], fixed: false });
  const present = id => cubes.some(c => c.id === id);
  const keys = [...['apps', 'audio', 'games', 'clipboard', 'music'].filter(present), ...ranked.map(c => c.id), ...(present('modules') ? ['modules'] : [])];
  return { placed, ranked, overflow, hidden: off, keys };
}

/** `order` with `id` moved `by` places (negative: nearer the eclipse), from the ranking as it stands. */
export function moveInOrder(ranked, id, by) {
  const ids = ranked.map(c => c.id), i = ids.indexOf(id);
  if (i < 0) return ids;
  const j = Math.max(0, Math.min(ids.length - 1, i + by));
  ids.splice(j, 0, ids.splice(i, 1)[0]);
  return ids;
}

/** `order` with two cubes trading places (dragging one onto the other). */
export function swapInOrder(ranked, a, b) {
  const ids = ranked.map(c => c.id), i = ids.indexOf(a), j = ids.indexOf(b);
  if (i >= 0 && j >= 0) [ids[i], ids[j]] = [ids[j], ids[i]];
  return ids;
}

/* ---------- activities (§4.8) ---------- */

export const PRIORITY = { call: 30, timer: 20, media: 10, custom: 5 };
export const priorityOf = a => (typeof a.priority === 'number' ? a.priority : PRIORITY[a.kind] ?? PRIORITY.custom);

/** Highest priority first; on a tie, the most recent (higher seq). */
export function sortActivities(list) {
  return [...list].sort((a, b) => priorityOf(b) - priorityOf(a) || (b.seq || 0) - (a.seq || 0));
}
export const topActivity = list => sortActivities(list)[0] || null;

/* ---------- the pill at rest (§4.11) ---------- */

/** Four weather icons only (the owner's choice), from WMO weather codes. */
export function weatherIcon(code) {
  const c = Number(code);
  if (c === 0 || c === 1) return 'sun';
  if ([2, 3, 45, 48].includes(c)) return 'cloud';
  if ((c >= 51 && c <= 57) || (c >= 61 && c <= 67) || (c >= 80 && c <= 82) || (c >= 95 && c <= 99)) return 'rain';
  if ((c >= 71 && c <= 77) || c === 85 || c === 86) return 'snow';
  return null;
}

/**
 * What the resting pill shows of the weather: { icon, temp } from the Today card's cached reading
 * (Open-Meteo, Celsius), or null with no city, no reading, or one older than 3 hours.
 */
export function restWeather(cache, city, unit, now = Date.now()) {
  if (!cache || !city || cache.city !== city || typeof cache.temp !== 'number' || !(now - cache.t < 3 * 3600e3)) return null;
  const icon = weatherIcon(cache.code);
  if (!icon) return null;
  return { icon, temp: Math.round(unit === 'C' ? cache.temp : cache.temp * 9 / 5 + 32) };
}

/** "Fri 24": the short weekday and the day of the month, in the user's locale. */
export function restDate(d, locale) {
  return `${d.toLocaleDateString(locale, { weekday: 'short' })} ${d.getDate()}`;
}

/* ---------- hotkeys (§4.13) ---------- */

/** Letters the base keeps. */
export const BASE_KEYS = { A: 'tab:audio', V: 'tab:clipboard', N: 'tab:today', W: 'tab:today' };

/** The Win + letter table: the base's, then each module's in install order (first one keeps a letter). */
export function hotkeyTable(modules) {
  const table = { ...BASE_KEYS };
  for (const m of modules) for (const h of m.hotkeys || []) if (!(h.key in table)) table[h.key] = h.opens;
  return table;
}

/* ---------- search helpers ---------- */

/** A module's keywords that a query matches ("lyrics" finds the Music module before it's installed). */
export function keywordHit(q, keywords) {
  q = q.trim().toLowerCase();
  return q.length >= 3 && (keywords || []).some(k => k.toLowerCase().startsWith(q) || q.startsWith(k.toLowerCase()));
}
