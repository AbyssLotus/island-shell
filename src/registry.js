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

/* ---------- Command Center cube slots (§4.12) ---------- */

/** The 12 named slots: [name, x, y (in eclipse radii from its center), size]. Their order is the
 * cubes' order for the keyboard (arrows and number keys in a sheet). */
export const SLOTS = [
  ['apps', -1.8, -1.12, 'L'], ['discord', -2.52, -0.02, 'M'], ['audio', -2, 1.1, 'L'],
  ['today', -3.4, 0.82, 'S'], ['canvas', -3.3, -0.84, 'S'],
  ['games', 1.72, -1.1, 'M'], ['network', 2.48, 0.04, 'W'], ['clipboard', 1.92, 1.14, 'M'],
  ['destiny', 3.3, -0.84, 'S'], ['stats', 3.42, 1.02, 'S'],
  ['music', 0, 1.62, 'W'], ['modules', 0, -1.62, 'S']
];
/** Slots a module cube may take when its own is taken, in this order. */
export const SPARE = ['discord', 'canvas', 'games', 'destiny', 'music'];

/**
 * Places cubes in slots. A base cube (no `module`) owns the slot named like it. A module cube takes
 * its `slot` if that's free, otherwise the first free spare slot; with none left it isn't shown.
 * Returns [{ cube, slot: { name, x, y, size } }] in slot order. Earlier cubes win a contested slot.
 */
export function placeCubes(cubes) {
  const free = new Map(SLOTS.map(s => [s[0], s]));
  const placed = new Map();
  const take = (cube, name) => { const s = free.get(name); if (!s) return false; free.delete(name); placed.set(name, cube); return true; };
  for (const c of cubes) if (!c.module) take(c, c.slot || c.id);
  for (const c of cubes) if (c.module && !take(c, c.slot || c.id)) SPARE.some(n => take(c, n));
  return SLOTS.filter(s => placed.has(s[0])).map(([name, x, y, size]) => ({ cube: placed.get(name), slot: { name, x, y, size } }));
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
