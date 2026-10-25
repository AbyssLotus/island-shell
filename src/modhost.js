// The module host on the page (docs/design/modules-handoff.md §4.10). Each enabled module's manifest
// becomes registry entries straight away (its tabs, cubes, sites and sections show up at once), and
// its UI is imported the first time one of them is used, or right after start for `ui.eager`
// modules (the ones with something on the pill, a Controls tile or a bar chip).
//
// Module UI runs in the island's page but mounts in shadow roots: each adopts the UI kit (kit.css)
// and links the module's own CSS, so neither side's classes reach the other. The page's theme
// arrives through inherited custom properties. This is isolation, not security: module code is
// trusted because it's signed (or loaded in developer mode).
//
// A module's entry exports `activate(ctx)` and, optionally, `deactivate()`. ctx (API 1):
//   id, version                     the module
//   call(method, params)            its backend; rejects with { code, message }
//   on(name, fn) → off              its backend's host.emit events
//   store.get(k, d) / .set(k, v)    localStorage, as "mod.<id>.<k>"
//   url(path), cacheUrl(path)       a file in the module, an image in its cache folder
//   ui                              the island's helpers: icon(), esc(), setHtml(), txt(), clip(), grad(), glyph(), ago(), note()
//   tabs.register({ id, mount(root), show(), hide() })        an island tab from its manifest
//   sections.register({ id, mount(root), show(), hide() })    a section of a base tab
//   cubes.register({ id, stat(), sheet?: { mount(el), unmount(), key(e) } })
//   tiles.set({ id, icon, title, sub, pressed, hidden, run }), tiles.remove(id)    Controls tiles
//   chips.set({ id, icon, text, title, hidden, tone, avatars, run }), chips.remove(id)  top-bar chips
//   search.register({ id, kind, label, icon, items() })        Command Center results
//   commands.set([{ id, title, icon, sub, keys, run, stay }])  quick commands (replaces its own)
//   activity.set({ id, kind, … }), activity.clear(id)         live things on the pill
//   notify({ title, body, icon, img, round, urgent, action, sound, ms })
//   open('tab:<id>' | 'cube:<id>')
//   island                          layout(), showing(tab), closePanel(keep), releaseFocus(), menu(x, y, items), state(), on(event, fn)
//   settings.mount(fn(root))        its page in the Modules sheet

const BASE = () => window.__ISLAND_MOD_BASE__ || 'http://mod.localhost/';
export const modUrl = (id, path) => `${BASE()}${id}/${path.split('/').map(encodeURIComponent).join('/')}`;

let kitSheet = null;
/** The kit's rules as one constructable stylesheet, built from the page's own kit.css (same origin). */
function kit() {
  if (kitSheet) return kitSheet;
  kitSheet = new CSSStyleSheet();
  const src = [...document.styleSheets].find(s => s.href && /\/kit\.css(\?|$)/.test(s.href));
  let text = '';
  try { text = src ? [...src.cssRules].map(r => r.cssText).join('\n') : ''; } catch (e) {}
  kitSheet.replaceSync(`:host { display: block; color: var(--fg); font: inherit; }\n${text}`);
  return kitSheet;
}

/** A shadow root on `host` with the kit and the module's CSS. */
export function shadowFor(host, id, css) {
  const root = host.shadowRoot || host.attachShadow({ mode: 'open' });
  root.adoptedStyleSheets = [kit()];
  for (const c of css || []) {
    const l = document.createElement('link');
    l.rel = 'stylesheet'; l.href = modUrl(id, c);
    root.append(l);
  }
  return root;
}

/**
 * env, from app.js: { R, invoke, listen, log, ui, island, addView(tab), removeView(tab), notify(module, n),
 * open(target), setTiles(), setChips(), activitiesChanged() }
 */
export function createModuleHost(env) {
  const { R, invoke, listen, log } = env;
  const mods = new Map(); // id → { info, man, offs, loading, api, ctx, tabs, sections, cubes, settings }
  let seq = 1;

  const iconFor = (id, ic) => (/\.svg$/.test(ic || '') ? modUrl(id, ic) : ic || 'grid');

  function register(info) {
    const man = info.manifest, id = man.id, c = man.contributes || {};
    const m = { info, man, offs: [], loading: null, api: null, ctx: null, tabs: new Map(), sections: new Map(), cubes: new Map(), settings: null, shown: new Set() };
    mods.set(id, m);
    for (const t of c.tabs || []) {
      if (R.tabs.has(t.id)) { log(`modules: ${id} wants tab ${t.id}, which is taken`); continue; }
      R.tabs.add({ id: t.id, title: t.title, icon: iconFor(id, t.icon), order: t.order, width: t.width || 760, module: id });
    }
    for (const s of c.sections || []) R.sections.add({ id: `${id}:${s.id}`, sid: s.id, tab: s.tab, order: s.order, module: id, title: s.title || man.name });
    for (const s of c.sites || []) R.sites.add({ id: `${id}:${s.id}`, sid: s.id, site: `${id}/${s.id}`, label: s.label, default: !!s.default, module: id });
    for (const cube of c.cubes || []) {
      if (R.cubes.has(cube.id)) { log(`modules: ${id} wants cube ${cube.id}, which is taken`); continue; }
      R.cubes.add({ id: cube.id, title: cube.title, icon: iconFor(id, cube.icon), size: cube.size, slot: cube.slot, module: id, site: cube.site ? `${id}:${cube.site}` : null, stat: () => (m.cubes.get(cube.id) || {}).stat?.() || '' });
    }
    for (const h of c.hotkeys || []) if (!R.hotkeys.has(h.key)) R.hotkeys.add({ id: h.key, opens: h.opens, module: id });
    if (man.ui && man.ui.eager) activate(id);
  }

  function unregister(id) {
    const m = mods.get(id); if (!m) return;
    try { m.api && m.api.deactivate && m.api.deactivate(); } catch (e) { log(`modules: ${id} deactivate: ${e && e.stack || e}`); }
    for (const off of m.offs) { try { off(); } catch (e) {} }
    for (const t of m.tabs.keys()) env.removeView(t);
    for (const list of Object.values(R)) list.removeModule(id);
    env.setTiles(); env.setChips(); env.activitiesChanged();
    mods.delete(id);
  }

  /** Imports the module's UI once; resolves when its activate(ctx) is done. */
  function activate(id) {
    const m = mods.get(id);
    if (!m) return Promise.reject(new Error('That module isn\'t installed'));
    if (!m.man.ui) return Promise.resolve();
    if (!m.loading) {
      m.ctx = makeCtx(m);
      m.loading = import(modUrl(id, m.man.ui.entry))
        .then(async api => { m.api = api; if (typeof api.activate === 'function') await api.activate(m.ctx); })
        .catch(e => { log(`modules: ${id} UI failed: ${e && e.stack || e}`); m.failed = String(e && e.message || e); throw e; });
    }
    return m.loading;
  }

  /** A tab or section host: its shadow root, mounted once the module has registered it. */
  async function mountIn(m, kind, key, host) {
    await activate(m.man.id);
    const def = m[kind].get(key);
    if (!def) throw new Error(`${m.man.name} didn't set up ${key}`);
    if (!def.root) { def.root = shadowFor(host, m.man.id, m.man.ui.css); def.mount && def.mount(def.root); }
    return def;
  }

  function makeCtx(m) {
    const id = m.man.id;
    const own = fn => { m.offs.push(fn); return fn; };
    return {
      id,
      version: m.man.version,
      call: (method, params) => invoke('mod_call', { id, method, params: params === undefined ? null : params }),
      on(name, fn) {
        let off = null, dead = false;
        listen(`mod:${id}:${name}`, e => fn(e.payload)).then(u => { if (dead) u(); else off = u; });
        return own(() => { dead = true; if (off) off(); });
      },
      store: {
        get(k, d) { try { const v = localStorage.getItem(`mod.${id}.${k}`); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
        set(k, v) { try { localStorage.setItem(`mod.${id}.${k}`, JSON.stringify(v)); } catch (e) {} }
      },
      url: path => modUrl(id, path),
      cacheUrl: path => modUrl(id, `~cache/${path}`),
      ui: env.ui,
      tabs: {
        register(def) {
          if (!R.tabs.get(def.id) || R.tabs.get(def.id).module !== id) throw new Error(`Tab ${def.id} isn't in ${id}'s manifest`);
          m.tabs.set(def.id, { ...def });
          env.addView(def.id);
        }
      },
      sections: {
        register(def) {
          const key = `${id}:${def.id}`;
          if (!R.sections.has(key)) throw new Error(`Section ${def.id} isn't in ${id}'s manifest`);
          m.sections.set(key, { ...def });
          env.sectionsChanged();
        }
      },
      cubes: {
        register(def) {
          if (!R.cubes.get(def.id) || R.cubes.get(def.id).module !== id) throw new Error(`Cube ${def.id} isn't in ${id}'s manifest`);
          m.cubes.set(def.id, def);
          if (def.sheet) R.sheets.set({ id: def.id, module: id, ...def.sheet });
          // A docked site that needs setting up first says so (and why) through siteReady().
          const site = R.cubes.get(def.id).site && R.sites.get(R.cubes.get(def.id).site);
          if (site && def.siteReady) { site.ready = def.siteReady; site.why = def.siteWhy || ''; }
        }
      },
      tiles: {
        set(t) { R.tiles.set({ order: 100, ...t, id: `${id}:${t.id}`, tid: t.id, module: id }); env.setTiles(); },
        remove(tid) { R.tiles.remove(`${id}:${tid}`); env.setTiles(); }
      },
      chips: {
        set(c) { R.chips.set({ order: 100, ...c, id: `${id}:${c.id}`, cid: c.id, module: id }); env.setChips(); },
        remove(cid) { R.chips.remove(`${id}:${cid}`); env.setChips(); }
      },
      search: { register(src) { R.search.set({ ...src, id: `${id}:${src.id || src.kind}`, module: id }); } },
      commands: {
        set(list) {
          for (const c of R.commands.all()) if (c.module === id) R.commands.remove(c.id);
          for (const c of list || []) R.commands.set({ ...c, id: `${id}:${c.id || c.title}`, module: id });
        }
      },
      activity: {
        set(a) { R.activities.set({ ...a, id: `${id}:${a.id}`, aid: a.id, module: id, seq: seq++ }); env.activitiesChanged(); },
        clear(aid) { R.activities.remove(`${id}:${aid}`); env.activitiesChanged(); }
      },
      notify: n => env.notify(id, n),
      open: target => env.open(target),
      island: env.island,
      settings: { mount(fn) { m.settings = fn; } },
      setup: { card(fn) { R.setup.set({ id, module: id, order: 100, render: fn }); } }
    };
  }

  // ---------- what the page asks of modules ----------

  /** A module tab is about to show: mounts it on first use, then calls its show(). */
  async function showTab(tab, host) {
    const t = R.tabs.get(tab); if (!t || !t.module) return;
    const m = mods.get(t.module); if (!m) return;
    try {
      const def = await mountIn(m, 'tabs', tab, host);
      if (!m.shown.has(tab)) { m.shown.add(tab); def.show && def.show(); }
    } catch (e) { env.tabFailed(tab, m.man.name, e); }
  }
  function hideTab(tab) {
    const t = R.tabs.get(tab); if (!t || !t.module) return;
    const m = mods.get(t.module); if (!m || !m.shown.delete(tab)) return;
    const def = m.tabs.get(tab); try { def && def.hide && def.hide(); } catch (e) {}
  }
  /** A section's host is on screen (or off it). */
  async function showSection(key, host, on) {
    const s = R.sections.get(key); if (!s) return;
    const m = mods.get(s.module); if (!m) return;
    if (!on) { if (m.shown.delete(key)) { const d = m.sections.get(key); try { d && d.hide && d.hide(); } catch (e) {} } return; }
    try {
      const def = await mountIn(m, 'sections', key, host);
      if (!m.shown.has(key)) { m.shown.add(key); def.show && def.show(); }
    } catch (e) { log(`modules: section ${key}: ${e}`); }
  }
  /** A module cube's own app in a sheet, once the module is loaded. */
  async function sheetFor(cube) {
    const c = R.cubes.get(cube); if (!c || !c.module) return null;
    await activate(c.module).catch(() => {});
    return R.sheets.get(cube) || null;
  }
  /** Whether a module's docked site may open yet (a module can say it needs setting up first). */
  async function siteReady(cube) {
    const c = R.cubes.get(cube); if (!c || !c.module) return true;
    await activate(c.module).catch(() => {});
    const def = mods.get(c.module)?.cubes.get(cube);
    return def && def.siteReady ? def.siteReady() : true;
  }

  async function start() {
    let list = { modules: [], broken: [] };
    try { list = await invoke('mod_list'); } catch (e) { log(`modules: mod_list failed: ${e}`); }
    for (const info of (list && list.modules) || []) if (info.enabled && info.manifest && info.manifest.ui) register(info);
    // A module turned on or off, installed or removed, shows up or goes away without a restart.
    listen('mod:changed', e => {
      const info = e.payload, had = mods.has(info.id);
      if (info.enabled && !had && info.manifest.ui) register(info);
      else if (!info.enabled && had) unregister(info.id);
      else if (had) mods.get(info.id).info = info;
    });
    listen('mod:installed', async () => {
      const l = await invoke('mod_list').catch(() => null);
      for (const info of (l && l.modules) || []) if (info.enabled && info.manifest.ui && !mods.has(info.id)) register(info);
    });
    listen('mod:removed', e => unregister(e.payload));
  }

  return { start, activate, showTab, hideTab, showSection, sheetFor, siteReady, mods, settingsOf: id => mods.get(id)?.settings || null };
}
