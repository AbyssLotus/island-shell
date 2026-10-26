// Voicemeeter's mixer, through VoiceDeck's engine: a view of the Audio tab (every input strip and
// output bus with its gain, mute, routing and meter), a mic switch on the top bar (strip 1, the one
// VoiceDeck's mic key uses) and two quick commands. The backend (src/main.rs) runs while Voicemeeter
// does; it sends the whole mixer (`state`, null when Voicemeeter goes), then only what changed
// (`delta`), and meters (`levels`) only while the mixer is on screen.

let ctx, root = null, vm = null, apps = null, shown = false, levelsOn = false, shape = '', meters = null;
let side = 'in';
const $ = s => root.querySelector(s);
const $$ = s => [...root.querySelectorAll(s)];
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const errText = e => (e && e.message) || String(e);

const VIRTUAL_NAMES = ['Voicemeeter VAIO', 'Voicemeeter AUX', 'Voicemeeter VAIO3'];
function stripName(s) {
  if (s.label) return s.label;
  if (s.physical) return s.device || `Input ${s.index + 1}`;
  const v = vm.strips.filter(x => !x.physical).indexOf(s);
  return VIRTUAL_NAMES[v] || `Virtual ${v + 1}`;
}
const gainPct = g => clamp((g + 60) / 72 * 100, 0, 100);
const levelPct = v => clamp((20 * Math.log10(Math.max(v, 1e-6)) + 60) / 72, 0, 1);

function view() {
  return `
  <div class="vm-top">
    <div class="seg" id="side" role="group" aria-label="Show">
      <button type="button" data-side="in">Inputs</button>
      <button type="button" data-side="out">Outputs</button>
    </div>
    <span class="grow"></span>
    <button class="chip" type="button" id="restart" hidden title="Restart Voicemeeter's audio engine (fixes crackling)">Restart engine</button>
    <button class="chip" type="button" id="vdOpen" hidden>Open VoiceDeck</button>
  </div>
  <div class="vm-list" id="strips"></div>
  <div class="vm-list" id="buses" hidden></div>
  <p class="empty" id="off" hidden></p>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => { if (root) ctx.ui.note($('#note'), msg); };

function rowHtml(kind, i) {
  const { esc } = ctx.ui;
  const route = kind === 'strip'
    ? `<div class="vm-route">${vm.busNames.map((n, b) => `<button type="button" class="${n.startsWith('B') ? 'b' : ''}" data-route="${b}" title="Send to ${esc(n)}">${esc(n)}</button>`).join('')}</div>` : '';
  return `<div class="vm-row${kind === 'bus' ? ' bus' : ''}" data-kind="${kind}" data-i="${i}">
    <div class="vm-name"><b></b><small></small></div>
    <div class="vm-fader"><span class="vm-meter"><i></i></span><input type="range" min="-60" max="12" step="0.1" aria-label="Gain"></div>
    <span class="vm-db"></span>
    <button type="button" class="vm-btn" data-mute aria-pressed="false" title="Mute">M</button>${route}</div>`;
}

/** The header, and the rows: built when the mixer's shape changes, then only updated (a dragged fader is left alone). */
function render(full) {
  if (!root) return;
  const { txt } = ctx.ui;
  $$('#side button').forEach(b => b.setAttribute('aria-pressed', b.dataset.side === side));
  $('#side').hidden = !vm;
  $('#restart').hidden = !vm;
  $('#vdOpen').hidden = !(apps && apps.voicedeck);
  $('#strips').hidden = side !== 'in' || !vm; $('#buses').hidden = side !== 'out' || !vm;
  $('#off').hidden = !!vm;
  if (!vm) {
    shape = '';
    txt($('#off'), apps && !apps.voicemeeter ? 'Voicemeeter isn’t installed. Install it from vb-audio.com and its mixer shows up here.' : 'Voicemeeter isn’t running. Start it (or VoiceDeck) and its mixer shows up here.');
    if (shown) ctx.island.layout();
    return;
  }
  const s = `${vm.strips.length}/${vm.buses.length}/${vm.busNames.join()}`;
  if (full || s !== shape) {
    shape = s;
    $('#strips').innerHTML = vm.strips.map((_, i) => rowHtml('strip', i)).join('');
    $('#buses').innerHTML = vm.buses.map((_, i) => rowHtml('bus', i)).join('');
    meters = null;
  }
  const now = performance.now(), focused = root.getRootNode().activeElement;
  const paint = (row, item, name, sub, route) => {
    txt(row.querySelector('.vm-name b'), name); txt(row.querySelector('.vm-name small'), sub);
    const input = row.querySelector('input');
    if (focused !== input && !(now - (+row.dataset.setAt || -1e9) < 1200)) input.value = item.gain;
    input.style.setProperty('--p', gainPct(+input.value) + '%');
    txt(row.querySelector('.vm-db'), (+input.value > 0 ? '+' : '') + (+input.value).toFixed(1));
    row.classList.toggle('muted', item.mute);
    row.querySelector('[data-mute]').setAttribute('aria-pressed', item.mute);
    if (route) row.querySelectorAll('[data-route]').forEach(b => b.setAttribute('aria-pressed', !!route[+b.dataset.route]));
  };
  $$('#strips .vm-row').forEach((row, i) => { const x = vm.strips[i]; if (x) paint(row, x, stripName(x), x.physical ? (x.device || 'Hardware input') : 'Virtual input', x.routing); });
  $$('#buses .vm-row').forEach((row, i) => { const b = vm.buses[i]; if (b) paint(row, b, b.name, b.device || (b.name.startsWith('B') ? 'Virtual output' : 'No device'), null); });
  if (full && shown) ctx.island.layout();
}

function applyDelta(d) {
  if (!vm || !d) return;
  for (const x of d.strips || []) { const s = vm.strips[x.index]; if (s) for (const k of ['label', 'device', 'gain', 'mute', 'solo', 'routing', 'fx']) if (x[k] !== undefined) s[k] = x[k]; }
  for (const x of d.buses || []) { const b = vm.buses[x.index]; if (b) for (const k of ['name', 'device', 'gain', 'mute']) if (x[k] !== undefined) b[k] = x[k]; }
}

function paintLevels(l) {
  if (!root || !l) return;
  if (!meters) meters = { strips: $$('#strips .vm-meter i'), buses: $$('#buses .vm-meter i') };
  // Reveal the full-width green→red gradient up to the level (scaling it would squeeze the colors).
  const set = (els, vals) => { if (vals) vals.forEach((lr, i) => { const el = els[i]; if (el) el.style.clipPath = `inset(0 ${((1 - levelPct(Math.max(lr[0], lr[1]))) * 100).toFixed(1)}% 0 0)`; }); };
  if (side === 'in') set(meters.strips, l.strips); else set(meters.buses, l.buses);
}

/** Meters (and the engine's level reads) only while the mixer is on screen. */
function syncLevels() {
  const want = !!(vm && shown && ctx.backend.running());
  if (want === levelsOn) return;
  levelsOn = want;
  ctx.call('levels', { on: want }).catch(() => {});
}

/** The top bar's mic switch and the quick commands follow strip 1. */
function renderChip() {
  const s = vm && vm.strips[0];
  if (!s) { ctx.chips.remove('mic'); return; }
  ctx.chips.set({ id: 'mic', side: 'right', order: 10, icon: s.mute ? 'micoff' : 'mic', tone: s.mute ? 'mic-off' : '', title: `${stripName(s)}: ${s.mute ? 'muted (click to unmute)' : 'live (click to mute)'}`, run: toggleMic });
}
function renderCommands() {
  ctx.commands.set(vm ? [
    { id: 'mic', title: 'Mute or unmute my mic', icon: 'mic', sub: 'Voicemeeter · strip 1', keys: 'voicemeeter microphone', run: toggleMic },
    { id: 'restart', title: 'Restart the audio engine', icon: 'restart', sub: 'Voicemeeter', keys: 'voicemeeter audio engine fix sound crackle', run: () => ctx.call('restart') }
  ] : []);
}

function setState(s) {
  vm = s || null;
  render(true); renderChip(); renderCommands(); syncLevels();
}
function load() {
  return ctx.call('state').then(setState).catch(() => {});
}

function toggleMic() {
  const s = vm && vm.strips[0]; if (!s) return Promise.resolve();
  s.mute = !s.mute; renderChip(); render(false);
  return ctx.call('mic').catch(err => { s.mute = !s.mute; renderChip(); render(false); throw err; });
}

const param = (row, field) => `${row.dataset.kind === 'strip' ? 'Strip' : 'Bus'}[${row.dataset.i}].${field}`;
const item = row => (row.dataset.kind === 'strip' ? vm.strips[+row.dataset.i] : vm.buses[+row.dataset.i]);
function setGain(row, g) {
  const it = item(row); if (!it) return;
  g = Math.round(clamp(g, -60, 12) * 10) / 10;
  it.gain = g; row.dataset.setAt = performance.now();
  const input = row.querySelector('input'); input.value = g;
  input.style.setProperty('--p', gainPct(g) + '%'); ctx.ui.txt(row.querySelector('.vm-db'), (g > 0 ? '+' : '') + g.toFixed(1));
  clearTimeout(row._t); row._t = setTimeout(() => ctx.call('set', { name: param(row, 'Gain'), value: g }).catch(err => note(errText(err))), 30);
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  root.addEventListener('input', e => { const row = e.target.closest('.vm-row'); if (row && vm && e.target.matches('input')) setGain(row, +e.target.value); });
  root.addEventListener('dblclick', e => { const row = e.target.closest('.vm-fader') && e.target.closest('.vm-row'); if (row && vm) setGain(row, 0); });
  root.addEventListener('wheel', e => {
    const row = e.target.closest('.vm-fader') && e.target.closest('.vm-row'); if (!row || !vm) return;
    e.preventDefault(); e.stopPropagation();
    setGain(row, item(row).gain + (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.1 : 1));
  }, { passive: false });
  root.addEventListener('click', e => {
    const sb = e.target.closest('[data-side]');
    if (sb) { side = sb.dataset.side; ctx.store.set('side', side); meters = null; render(false); if (shown) ctx.island.layout(); return; }
    if (e.target.closest('#restart')) { note(''); ctx.call('restart').then(() => note('Restarting Voicemeeter’s audio engine…')).catch(err => note(errText(err))); return; }
    if (e.target.closest('#vdOpen')) { note(''); ctx.call('voicedeck_open').then(() => ctx.island.closePanel(true)).catch(err => note(errText(err))); return; }
    const row = e.target.closest('.vm-row'); if (!row || !vm) return;
    const it = item(row); if (!it) return;
    if (e.target.closest('[data-mute]')) {
      it.mute = !it.mute; render(false); renderChip();
      ctx.call('set', { name: param(row, 'Mute'), value: it.mute ? 1 : 0 }).catch(err => note(errText(err)));
    }
    const rb = e.target.closest('[data-route]');
    if (rb && it.routing) {
      const b = +rb.dataset.route; it.routing[b] = !it.routing[b]; render(false);
      ctx.call('set', { name: param(row, vm.busNames[b]), value: it.routing[b] ? 1 : 0 }).catch(err => note(errText(err)));
    }
  });
  render(true);
}

function show() {
  shown = true;
  // What's installed (VoiceDeck's button, the not-installed message): asked once, when first shown.
  if (!apps) ctx.call('apps').then(a => { apps = a; render(false); }).catch(() => {});
  syncLevels();
}
function hide() {
  shown = false;
  syncLevels();
}

export function activate(c) {
  ctx = c;
  // Inputs or outputs, kept from 0.1.x's island.vmSide the first time.
  try { const v = localStorage.getItem('island.vmSide'); if (v != null && localStorage.getItem('mod.voicemeeter.side') == null) localStorage.setItem('mod.voicemeeter.side', v); } catch (e) {}
  side = ctx.store.get('side', 'in');
  ctx.sections.register({ id: 'mixer', mount, show, hide });
  ctx.on('state', setState);
  ctx.on('delta', d => { applyDelta(d); render(false); renderChip(); });
  ctx.on('levels', paintLevels);
  // The backend comes and goes with Voicemeeter: a fresh one has its meters off and its mixer to send.
  ctx.backend.on(st => {
    if (st === 'running') { levelsOn = false; load(); }
    else if (st !== 'starting') { levelsOn = false; if (vm) setState(null); }
  });
  if (ctx.backend.running()) load();
}

export function deactivate() {
  ctx.chips.remove('mic');
  ctx.commands.set([]);
}
