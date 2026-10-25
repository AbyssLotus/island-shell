// Tailscale and Radmin VPN: a section of the Network tab (who's online, your addresses, the exit
// node), a tile for each in Controls, and a shield on the top bar's network chip while either is
// connected. Status comes from the backend (src/main.rs): every 4 s while the Network tab is open,
// every 20 s otherwise, and every 5 minutes when neither is installed.

let ctx, root = null, T = 0, busy = '', v = null;
const $ = s => root.querySelector(s);
const showing = () => ctx.island.showing('network');

const tsOn = () => !!(v && v.tailscale && v.tailscale.state === 'Running');
const rvOn = () => { const r = v && v.radmin; return !!(r && r.running && r.on && r.server); };
const tsName = p => p.name && p.name !== 'localhost' ? p.name : (p.dns || '').split('.')[0] || 'Device';
const TS_STATE = { Running: 'Connected', Stopped: 'Disconnected', NeedsLogin: 'Signed out', Starting: 'Connecting…', NoState: 'Tailscale app isn’t running', NeedsMachineAuth: 'Waiting for admin approval' };
const errText = e => (e && e.message) || String(e);

function view() {
  const { icon } = ctx.ui;
  return `
  <div class="vpn-grid">
    <div class="card k-svc" id="ts" hidden>
      <div class="k-head">
        <span class="k-logo">${icon('dots9')}</span>
        <div class="k-title"><b>Tailscale</b><small id="tsState">Checking…</small></div>
        <button class="btn" type="button" id="tsToggle" hidden>Connect</button>
      </div>
      <button class="vpn-me" type="button" id="tsMe" hidden></button>
      <label class="vpn-exit" id="tsExitRow" hidden><span>Exit node</span><select class="field" id="tsExit" aria-label="Exit node"></select></label>
      <div class="vpn-list" id="tsPeers"></div>
    </div>
    <div class="card k-svc" id="rv" hidden>
      <div class="k-head">
        <span class="k-logo">${icon('nodes')}</span>
        <div class="k-title"><b>Radmin VPN</b><small id="rvState">Checking…</small></div>
        <button class="btn" type="button" id="rvOpen" hidden>Open</button>
      </div>
      <button class="vpn-me" type="button" id="rvMe" hidden></button>
      <div class="vpn-list" id="rvPeers"></div>
    </div>
  </div>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => { if (root) ctx.ui.note($('#note'), msg); };

async function load() {
  try { v = await ctx.call('status'); } catch (e) {}
  render(); renderTiles();
}
function loop() {
  clearTimeout(T);
  load().finally(() => {
    const none = v && !v.tailscale && !v.radmin;
    T = setTimeout(loop, none ? 300000 : showing() ? 4000 : 20000);
  });
}

/** The Controls tiles and the bar's shield. */
function renderTiles() {
  const t = v && v.tailscale, r = v && v.radmin;
  if (t) ctx.tiles.set({ id: 'tailscale', order: 60, icon: 'dots9', title: 'Tailscale', pressed: tsOn(), sub: busy || (tsOn() ? t.ip || 'Connected' : TS_STATE[t.state] || t.state), run: toggle });
  else ctx.tiles.remove('tailscale');
  if (r) {
    const n = r.peers.filter(p => p.online).length;
    ctx.tiles.set({ id: 'radmin', order: 61, icon: 'nodes', title: 'Radmin VPN', pressed: rvOn(), sub: !r.running ? 'Service stopped' : !r.on ? 'Off' : !r.server ? 'Connecting…' : `${n} online`, run: () => ctx.open('tab:network') });
  } else ctx.tiles.remove('radmin');
  ctx.chips.set({ id: 'shield', in: 'net', icon: 'shield', title: 'VPN connected', hidden: !(tsOn() || rvOn()) });
}

function render() {
  if (!root) return;
  const { esc, setHtml, icon, ago } = ctx.ui;
  const t = v && v.tailscale, r = v && v.radmin;
  $('#ts').hidden = !t; $('#rv').hidden = !r;
  if (t) {
    const on = tsOn();
    $('#ts').classList.toggle('on', on);
    $('#tsState').textContent = busy || (on ? `Connected · ${t.tailnet || 'your tailnet'}` : TS_STATE[t.state] || t.state);
    const btn = $('#tsToggle');
    btn.hidden = false; btn.disabled = !!busy;
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
    $('#rv').classList.toggle('on', on);
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
  if (showing()) ctx.island.layout();
}

/** Connects or disconnects Tailscale (from the card or its Controls tile). */
async function toggle() {
  const t = v && v.tailscale; if (!t || busy) return;
  const up = !tsOn();
  busy = up ? 'Connecting…' : 'Disconnecting…';
  render(); renderTiles();
  try { await ctx.call('tailscale', { action: up ? 'up' : 'down' }); }
  finally { busy = ''; await load(); }
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#tsToggle').addEventListener('click', () => { note(''); toggle().catch(err => note(errText(err))); });
  $('#tsExit').addEventListener('change', e => {
    note('');
    ctx.call('tailscale', { action: 'exit', arg: e.target.value }).catch(err => note(errText(err))).finally(load);
  });
  $('#rvOpen').addEventListener('click', () => { ctx.call('radmin_open').then(() => ctx.island.closePanel(true)).catch(err => note(errText(err))); });
  // Click an address (yours or a device's) to copy it.
  root.addEventListener('click', e => {
    const el = e.target.closest('[data-copy]'); if (!el || !el.dataset.copy) return;
    navigator.clipboard.writeText(el.dataset.copy).then(() => {
      el.classList.remove('k-copied'); void el.offsetWidth; el.classList.add('k-copied');
      clearTimeout(el._t); el._t = setTimeout(() => el.classList.remove('k-copied'), 1300);
    }).catch(err => note(errText(err)));
  });
  render();
}

export function activate(c) {
  ctx = c;
  ctx.sections.register({ id: 'vpn', mount, show: loop });
  loop();
}

export function deactivate() {
  clearTimeout(T);
}
