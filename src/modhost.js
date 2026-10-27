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
//   backend.running(), .state(), .on(fn(state)) → off
//                                   whether its backend runs now ('running', 'stopped', 'backoff', …)
//                                   and when that changes, without starting it the way call() can
//   store.get(k, d) / .set(k, v)    localStorage, as "mod.<id>.<k>"
//   url(path), cacheUrl(path)       a file in the module, an image in its cache folder
//   imageUrl(file)                  an image in a folder its backend shared (host.images)
//   ui                              the island's helpers: icon(), esc(), setHtml(), txt(), clip(), grad(), glyph(), ago(), note()
//   tabs.register({ id, mount(root), show(), hide() })        an island tab from its manifest
//   sections.register({ id, mount(root), show(), hide() })    a section of a base tab (in place of
//                                   one of its cards with the manifest's `replaces`)
//   cubes.register({ id, stat(), sheet?: { mount(root), unmount(), key(e) } })
//     (each root is an element in a shadow root of the module's own, with the kit and its CSS)
//   tiles.set({ id, icon, title, sub, pressed, hidden, run }), tiles.remove(id)    Controls tiles
//   chips.set({ id, icon, text, title, hidden, tone, avatars, side, run }), chips.remove(id)  top-bar chips
//     (with in: 'net', an icon badge on the bar's network chip instead)
//   search.register({ id, kind, label, icon, items() })        Command Center results
//   commands.set([{ id, title, icon, sub, keys, run, stay }])  quick commands (replaces its own)
//   activity.set({ id, kind, … }), activity.clear(id)         live things on the pill: `media` and
//                                   `call` (see app.js), `timer` { text, ends_at | left, total, tone }
//                                   and `custom` { icon, text, sub } (a short tag beside the clock)
//   notify({ title, body, icon, img, round, urgent, action, sound, ms })
//   open('tab:<id>' | 'cube:<id>')
//   openWeb(url)                    a link in the default browser (never Edge)
//   island                          layout(), showing(tab), closePanel(keep), releaseFocus(), menu(x, y, items), state(), on(event, fn),
//                                   focusApp({ id, exe, hwnd }), audio.appVolume(exe) / .setAppVolume(exe, level),
//                                   accentFrom(imageUrl), flash(msg) and closeCommandCenter(then) for a sheet,
//                                   apps.list() / .open(id) / .menu(id, x, y): the Start menu's apps,
//                                   presence(): 'looking' | 'present' | 'away' | 'off' | '' (no eye
//                                   tracker), with on('presence'), and quiet(on, label): hold
//                                   notifications (all but urgent ones) until it's let go
//   settings.mount(fn(root))        its page in the Modules sheet
//   setup.card(fn() → { state })    its card in the setup guide, and how it's doing ('Connected')

const BASE = () => window.__ISLAND_MOD_BASE__ || 'http://mod.localhost/';
/** A file of a module, under its version (`<id>@<version>`), so an update's files are fetched fresh
 * rather than taken from what the page already loaded (relative imports inside keep the version). */
export const modUrl = (id, path, version) => `${BASE()}${id}${version ? '@' + encodeURIComponent(version) : ''}/${path.split('/').map(encodeURIComponent).join('/')}`;

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

/**
 * A shadow root on `host` with the kit and the module's CSS, and the element in it the module
 * mounts into (so a module setting its root's innerHTML keeps its stylesheets).
 */
export function shadowFor(host, id, css, version) {
  const sr = host.shadowRoot || host.attachShadow({ mode: 'open' });
  sr.adoptedStyleSheets = [kit()];
  const links = (css || []).map(c => {
    const l = document.createElement('link');
    l.rel = 'stylesheet'; l.href = modUrl(id, c, version);
    return l;
  });
  const root = document.createElement('div');
  sr.replaceChildren(...links, root);
  return root;
}

/**
 * env, from app.js: { R, invoke, listen, log, ui, island, addView(tab), removeView(tab), notify(module, n),
 * quiet(module, on, label), open(target), setTiles(), setChips(), activitiesChanged() }
 */
export function createModuleHost(env) {
  const { R, invoke, listen, log } = env;
  const mods = new Map(); // id → { info, man, offs, loading, api, ctx, tabs, sections, cubes, settings }
  let seq = 1;

  const iconFor = (id, ic, version) => (/\.svg$/.test(ic || '') ? modUrl(id, ic, version) : ic || 'grid');

  function register(info) {
    const man = info.manifest, id = man.id, c = man.contributes || {};
    const m = { info, man, offs: [], loading: null, api: null, ctx: null, tabs: new Map(), sections: new Map(), cubes: new Map(), settings: null, shown: new Set(), stateFns: new Set() };
    mods.set(id, m);
    for (const t of c.tabs || []) {
      if (R.tabs.has(t.id)) { log(`modules: ${id} wants tab ${t.id}, which is taken`); continue; }
      R.tabs.add({ id: t.id, title: t.title, icon: iconFor(id, t.icon, man.version), order: t.order, width: t.width || 760, module: id });
    }
    for (const s of c.sections || []) R.sections.add({ id: `${id}:${s.id}`, sid: s.id, tab: s.tab, order: s.order, module: id, title: s.title || man.name, view: !!s.view, replaces: s.replaces || '' });
    for (const s of c.sites || []) R.sites.add({ id: `${id}:${s.id}`, sid: s.id, site: `${id}/${s.id}`, label: s.label, default: !!s.default, split: !!s.split, module: id });
    for (const cube of c.cubes || []) {
      if (R.cubes.has(cube.id)) { log(`modules: ${id} wants cube ${cube.id}, which is taken`); continue; }
      R.cubes.add({ id: cube.id, title: cube.title, icon: iconFor(id, cube.icon, man.version), size: cube.size, slot: cube.slot, module: id, site: cube.site ? `${id}:${cube.site}` : null, stat: () => (m.cubes.get(cube.id) || {}).stat?.() || '' });
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
      m.loading = import(modUrl(id, m.man.ui.entry, m.man.version))
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
    if (!def.root) { def.root = shadowFor(host, m.man.id, m.man.ui.css, m.man.version); def.mount && def.mount(def.root); }
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
      backend: {
        state: () => m.info.state,
        running: () => m.info.state === 'running',
        on(fn) { m.stateFns.add(fn); return own(() => m.stateFns.delete(fn)); }
      },
      store: {
        get(k, d) { try { const v = localStorage.getItem(`mod.${id}.${k}`); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
        set(k, v) { try { localStorage.setItem(`mod.${id}.${k}`, JSON.stringify(v)); } catch (e) {} }
      },
      url: path => modUrl(id, path, m.man.version),
      cacheUrl: path => modUrl(id, `~cache/${path}`),
      imageUrl: file => `${BASE()}${id}/~img/${encodeURIComponent(file)}`,
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
          // A sheet mounts in a box of its own inside the Command Center's host (which base sheets
          // share), filling it.
          if (def.sheet) {
            let box = null;
            R.sheets.set({
              id: def.id, module: id, ...def.sheet,
              mount(host) {
                box = Object.assign(document.createElement('div'), { className: 'mod-sheet' });
                host.append(box);
                const root = shadowFor(box, id, m.man.ui.css, m.man.version);
                root.style.height = '100%';
                def.sheet.mount(root);
              },
              unmount() { try { def.sheet.unmount && def.sheet.unmount(); } finally { box?.remove(); box = null; } }
            });
          }
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
      openWeb: url => invoke('open_link', { url }),
      // The island's services, plus holding notifications, which is the module's own until it lets go.
      island: { ...env.island, quiet: (on, label) => env.quiet(id, !!on, label || m.man.name) },
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
      else if (had) {
        const m = mods.get(info.id), was = m.info.state;
        m.info = info;
        if (info.state !== was) for (const fn of m.stateFns) { try { fn(info.state); } catch (err) { log(`modules: ${info.id} backend.on: ${err}`); } }
      }
    });
    // Installed, or updated: an updated module's old UI goes and the new version's comes in.
    listen('mod:installed', async () => {
      const l = await invoke('mod_list').catch(() => null);
      for (const info of (l && l.modules) || []) {
        const had = mods.get(info.id);
        if (had && had.man.version !== info.manifest.version) unregister(info.id);
        if (info.enabled && info.manifest.ui && !mods.has(info.id)) register(info);
      }
    });
    listen('mod:removed', e => unregister(e.payload));
  }

  return { start, activate, showTab, hideTab, showSection, sheetFor, siteReady, mods, settingsOf: id => mods.get(id)?.settings || null };
}
