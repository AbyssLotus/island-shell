// A stand-in for window.__TAURI__ (Tauri's global API), answering `invoke` from fixtures/*.json.
// Loaded by serve.mjs before app.js. Everything in the fixtures is made up.
//
// Query options: ?scenario=<name> layers fixtures/<name>.json over default.json; ?tab=<id> opens the
// panel on that tab; ?cc=1 opens the Command Center; ?sheet=<id> opens a Command Center sheet;
// ?style=bar shows the top bar; ?fresh=1 starts with an empty profile (the setup guide); ?mods=0
// leaves out the repo's modules (modules/*, otherwise all on). window.harness.emit(name, payload) sends any backend event.
//
// A module's backend answers mod_call from the fixture "mod:<id>.<method>"; without one the call fails
// the way a missing method does.
(() => {
  const params = new URLSearchParams(location.search);
  // Module UI comes straight from the repo's module folders.
  window.__ISLAND_MOD_BASE__ = '/__repo/modules/';
  // A fresh profile gets the setup guide; ?fresh=1 shows it, otherwise the page starts as set up.
  try {
    localStorage.clear();
    if (!params.get('fresh')) localStorage.setItem('island.setupDone', 'true');
  } catch (e) {}
  const get = p => {
    const x = new XMLHttpRequest();
    x.open('GET', p, false);
    x.send();
    return x.status === 200 ? JSON.parse(x.responseText) : null;
  };
  const load = name => get(`/__harness/fixtures/${name}.json`) || {};
  const fixtures = load('default');
  for (const s of (params.get('scenario') || '').split(',').filter(Boolean)) Object.assign(fixtures, load(s));
  if (params.get('style')) fixtures.get_style = params.get('style');
  const missing = new Set();
  const calls = [];
  const day = 86400000;

  // Answers that depend on the arguments or the clock.
  const dynamic = {
    calendar_events: ({ from, to }) => {
      const out = [];
      const start = new Date(); start.setHours(0, 0, 0, 0);
      const make = (d, h, m, len, title, source, color, extra = {}) => {
        const s = new Date(start.getTime() + d * day); s.setHours(h, m);
        return { id: `${source}-${d}-${h}`, title, start: s.getTime(), end: s.getTime() + len * 60000, all_day: false, source, color, ...extra };
      };
      for (let d = -3; d < 10; d++) {
        if (d % 7 === 1 || d % 7 === 3) out.push(make(d, 9, 30, 75, 'Lecture: Signals', 'local', '#8ab4ff'));
        if (d % 7 === 2) out.push(make(d, 23, 59, 0, 'Problem set 4', 'canvas', '#f5c26b', { kind: 'due' }));
        if (d === 0) out.push(make(d, 14, 0, 60, 'Coffee with Sam', 'local', '#b9f5c9'));
      }
      return out.filter(e => e.start >= from && e.start <= to);
    },
    tm_snapshot: () => fixtures.tm_snapshot ?? null,
    mod_list: () => {
      const l = fixtures.mod_list ?? (params.get('mods') === '0' ? { modules: [], broken: [] } : get('/__harness/modules.json'));
      // A scenario can say whose backends are running ("mod_states": { "<id>": "running" }).
      for (const m of (l && l.modules) || []) if (fixtures.mod_states && fixtures.mod_states[m.id]) m.state = fixtures.mod_states[m.id];
      return l;
    },
    mod_call: ({ id, method }) => {
      const k = `mod:${id}.${method}`;
      if (k in fixtures) return structuredClone(fixtures[k]);
      throw { code: -32601, message: `${id} has no ${method} in the harness fixtures` };
    },
    log: ({ msg }) => { console.debug('[page log]', msg); return null; },
  };

  async function invoke(cmd, args = {}) {
    calls.push([cmd, args]);
    if (cmd in dynamic) return dynamic[cmd](args); // a throw rejects, like a command's Err
    if (cmd in fixtures) return structuredClone(fixtures[cmd]);
    if (!missing.has(cmd)) { missing.add(cmd); console.debug('[harness] no fixture for', cmd); }
    return null;
  }

  const listeners = {};
  function listen(name, fn) {
    (listeners[name] ||= []).push(fn);
    return Promise.resolve(() => { listeners[name] = listeners[name].filter(f => f !== fn); });
  }
  function emit(name, payload) {
    for (const fn of listeners[name] || []) fn({ event: name, payload, id: 0 });
  }

  const win = {
    label: 'island',
    setFocus: async () => {},
    onFocusChanged: async () => () => {},
    listen: async (name, fn) => listen(name, fn),
  };
  window.__TAURI__ = {
    core: { invoke, convertFileSrc: p => p, transformCallback: f => f },
    event: { listen, emit: async (n, p) => emit(n, p) },
    window: { getCurrentWindow: () => win },
    webviewWindow: { getCurrentWebviewWindow: () => win },
  };
  window.harness = { emit, calls, missing, fixtures };

  addEventListener('load', () => setTimeout(() => {
    if (params.get('tab')) emit('island:toggle', params.get('tab'));
    if (params.get('cc') || params.get('sheet')) emit('island:toggle', 'cc');
    if (params.get('sheet')) setTimeout(() => emit('island:toggle', params.get('sheet')), 600);
  }, 700));
})();
