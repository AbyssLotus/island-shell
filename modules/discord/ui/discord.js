// Discord: the island's Discord tab (setting up your Discord application, then your call, mute,
// deafen, leave, and the status and notification switches), the call on the pill and the top bar
// (who's in it, a green ring on whoever's talking), quick commands, the cube's line, and the setup
// guide's card. The backend (src/main.rs) runs while Discord does; it sends the whole `status` when
// it changes and `speaking` when someone starts or stops talking. @mentions it shows by itself.

let ctx, root = null, dc = null, edit = false, autoAuth = false, order = [];
const $ = s => root.querySelector(s);
const $$ = s => [...root.querySelectorAll(s)];
const errText = e => (e && e.message) || String(e);
const DC_CDN = /^https:\/\/(cdn\.discordapp\.com|media\.discordapp\.net)\//;
const avatarOf = m => (DC_CDN.test(m.avatar || '') ? m.avatar : '');

function view() {
  return `
  <div id="setup" hidden>
    <div class="card k-setup">
      <div class="k-head">
        <span class="k-logo">${ctx.ui.icon('chat')}</span>
        <div class="k-title"><b>Connect Discord</b><small>Calls, notifications and “Listening to Apple Music”</small></div>
        <button class="btn" type="button" id="back" hidden>Cancel</button>
      </div>
      <ol class="k-steps">
        <li>Open the <button class="k-link" type="button" id="portal">Discord Developer Portal</button> and click <b>New Application</b>. Name it <b>Apple Music</b>: your status will read “Listening to Apple Music”.</li>
        <li>In the app’s <b>OAuth2</b> page, add the redirect <code>http://localhost</code> and save.</li>
        <li>Copy the <b>Client ID</b> and <b>Client Secret</b> from that page into here.</li>
      </ol>
      <form class="k-form" id="form" autocomplete="off">
        <input class="field" id="cid" placeholder="Client ID" inputmode="numeric" spellcheck="false" aria-label="Client ID">
        <input class="field" id="secret" type="password" placeholder="Client Secret" spellcheck="false" aria-label="Client Secret">
        <button class="btn primary" type="submit">Connect</button>
      </form>
      <div class="k-fine"><span>The secret and Discord’s sign-in are stored encrypted for your Windows account only.</span><span class="grow"></span><button class="k-link" type="button" id="forget" hidden>Forget this app</button></div>
    </div>
  </div>
  <div class="dc-grid" id="main" hidden>
    <div class="card dc-voice" id="voice"></div>
    <div class="card dc-side">
      <div class="k-head" id="me"></div>
      <div class="dc-ctl" id="ctl">
        <button class="dc-b" type="button" id="mute"></button>
        <button class="dc-b" type="button" id="deaf"></button>
        <button class="dc-b bad" type="button" id="leave">${ctx.ui.icon('phoneoff')}<span>Leave</span></button>
      </div>
      <label class="switch"><input type="checkbox" id="presence"><span>Show what I’m listening to</span></label>
      <small class="dc-now" id="now"></small>
      <label class="switch"><input type="checkbox" id="notify"><span>Show when someone @mentions me</span></label>
      <div class="dc-foot"><button class="k-link" type="button" id="openApp">Open Discord</button><span class="grow"></span><button class="k-link" type="button" id="editApp">App settings</button></div>
    </div>
  </div>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => { if (root) ctx.ui.note($('#note'), msg); };
const showing = () => ctx.island.showing('discord');

function stateLine(d) {
  if (!d.connected) return 'Discord isn’t open';
  if (!d.authorized) return d.has_secret ? 'Connected · not authorized yet' : 'Connected · status only';
  return 'Signed in';
}

function render() {
  if (!root) return;
  const { esc, setHtml, icon, txt } = ctx.ui;
  const d = dc || {}, setup = !d.configured || edit;
  const img = (m, cls) => `<img class="${cls}" src="${esc(avatarOf(m))}" alt="" data-id="${esc(m.id)}" referrerpolicy="no-referrer">`;
  $('#setup').hidden = !setup; $('#main').hidden = setup;
  ctx.ui.note($('#note'), d.error || '');
  if (setup) {
    const id = $('#cid');
    if (!id.value && d.client_id) id.value = d.client_id;
    $('#secret').placeholder = d.has_secret ? 'Client Secret (saved; blank keeps it)' : 'Client Secret';
    $('#back').hidden = !d.configured; $('#forget').hidden = !d.configured;
  } else {
    const me = d.user, v = d.voice;
    setHtml($('#me'), `${me ? img(me, 'dc-av') : `<span class="k-logo">${icon('chat')}</span>`}<div class="k-title${d.authorized ? ' on' : ''}"><b>${esc(me ? me.name : 'Discord')}</b><small>${esc(stateLine(d))}</small></div>`);
    let html;
    if (!d.connected) html = `<div class="empty">${icon('chat')}<span>Open Discord to see your calls here</span><button class="btn" type="button" data-dc="open">Open Discord</button></div>`;
    else if (!d.authorized) html = d.has_secret
      ? `<div class="empty">${icon('headphones')}<span>Let the island see your calls and notifications.<br>Discord will ask you to confirm.</span><button class="btn primary" type="button" data-dc="authorize">Authorize</button></div>`
      : `<div class="empty">${icon('headphones')}<span>Add your app’s Client Secret to see calls and notifications</span><button class="btn" type="button" data-dc="edit">Add the secret</button></div>`;
    else if (!v) html = `<div class="empty">${icon('headphones')}<span>You’re not in a voice channel</span></div>`;
    else html = `<div class="k-head"><span class="k-logo on">${icon('headphones')}</span><div class="k-title"><b>${esc(v.name)}</b><small>${esc(v.guild || 'Voice')} · ${v.members.length} ${v.members.length === 1 ? 'person' : 'people'}</small></div></div>`
      + `<div class="dc-members">${v.members.map(m => `<div class="dc-m">${img(m, 'dc-av')}<b>${esc(m.name)}</b>${m.deaf ? icon('headoff') : m.mute ? icon('micoff') : ''}</div>`).join('')}</div>`;
    setHtml($('#voice'), html);
    $('#ctl').hidden = !d.authorized;
    const mute = $('#mute'), deaf = $('#deaf');
    mute.setAttribute('aria-pressed', !!d.mute); deaf.setAttribute('aria-pressed', !!d.deaf);
    setHtml(mute, `${icon(d.mute ? 'micoff' : 'mic')}<span>${d.mute ? 'Unmute' : 'Mute'}</span>`);
    setHtml(deaf, `${icon(d.deaf ? 'headoff' : 'headphones')}<span>${d.deaf ? 'Undeafen' : 'Deafen'}</span>`);
    $('#leave').disabled = !v;
    $('#presence').checked = !!d.presence; $('#notify').checked = !!d.notify;
    txt($('#now'), !d.presence || !d.connected ? '' : d.presence_now ? `Showing ${d.presence_now}` : 'Shows while Apple Music plays');
    talk();
  }
  if (showing()) ctx.island.layout();
}

/** Up to three people for the pill and the bar: the order sticks, except someone out of sight who starts talking moves up. */
function visible(v) {
  const ids = v.members.map(m => m.id);
  order = order.filter(id => ids.includes(id));
  for (const id of ids) if (!order.includes(id)) order.push(id);
  return order.slice(0, 3).map(id => v.members.find(m => m.id === id));
}

/** The call on the pill and the top bar, while you're in one with somebody there. */
function renderCall() {
  const d = dc, v = d && d.voice;
  if (!(v && v.members.length)) { ctx.activity.clear('call'); ctx.chips.remove('call'); return; }
  const avatars = visible(v).map(m => ({ id: m.id, src: avatarOf(m), talk: !!m.speaking }));
  const self = d.deaf ? 'headoff' : d.mute ? 'micoff' : '';
  const where = `${v.name}${v.guild ? ' · ' + v.guild : ''}`;
  ctx.activity.set({ id: 'call', kind: 'call', text: v.name, sub: v.guild || 'Discord', avatars, icon: self || undefined, action: 'tab:discord' });
  ctx.chips.set({ id: 'call', side: 'right', order: 5, tone: 'call-chip', avatars, icon: self || '', title: `${where} (Discord)`, opens: 'tab:discord' });
}

/** Green rings on whoever is talking, in the tab (the pill and the bar follow renderCall). */
function talk() {
  const v = dc && dc.voice; if (!v || !root) return;
  const talking = new Set(v.members.filter(m => m.speaking).map(m => m.id));
  for (const img of $$('#voice .dc-av')) img.classList.toggle('talk', talking.has(img.dataset.id));
}

function renderCommands() {
  const d = dc || {}, list = [];
  if (d.authorized) {
    list.push({ id: 'mute', title: d.mute ? 'Unmute in Discord' : 'Mute in Discord', icon: d.mute ? 'mic' : 'micoff', sub: 'Discord · your mic', keys: 'discord mic voice', run: () => action(d.mute ? 'unmute' : 'mute') });
    list.push({ id: 'deaf', title: d.deaf ? 'Undeafen in Discord' : 'Deafen in Discord', icon: 'headphones', sub: 'Discord · your sound', keys: 'discord deafen', run: () => action(d.deaf ? 'undeafen' : 'deafen') });
  }
  if (d.voice) list.push({ id: 'leave', title: 'Leave the Discord call', icon: 'phoneoff', sub: `Discord · ${d.voice.name}`, keys: 'discord hang up disconnect voice', run: () => action('leave') });
  ctx.commands.set(list);
}

function setStatus(s) {
  const prev = dc, was = prev && prev.voice;
  dc = s || null;
  const now = dc && dc.voice, { clip } = ctx.ui;
  if (prev && now && (!was || was.channel_id !== now.channel_id)) ctx.notify({ title: clip(now.name, 40), body: 'Joined voice', icon: 'headphones', sound: 'voice' });
  else if (prev && was && !now && dc && dc.connected) ctx.notify({ title: clip(was.name, 40), body: 'Left voice', icon: 'phoneoff', sound: 'voice' });
  if (autoAuth && dc && dc.connected && !dc.authorized && dc.has_secret && !dc.error) { autoAuth = false; action('authorize'); }
  render(); renderCall(); renderCommands();
}

function load() {
  return ctx.call('status').then(setStatus).catch(() => {});
}

function action(a) {
  note('');
  return ctx.call('action', { action: a }).catch(err => note(errText(err)));
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#portal').addEventListener('click', () => ctx.openWeb('https://discord.com/developers/applications').catch(err => note(errText(err))));
  $('#form').addEventListener('submit', e => {
    e.preventDefault();
    const client_id = $('#cid').value.trim(), secret = $('#secret').value.trim();
    if (!client_id) { note('Paste the Client ID first.'); return; }
    note('');
    ctx.call('configure', { client_id, secret }).then(() => {
      $('#secret').value = '';
      edit = false; autoAuth = !!secret || !!(dc && dc.has_secret);
      load();
    }).catch(err => note(errText(err)));
  });
  $('#back').addEventListener('click', () => { edit = false; $('#secret').value = ''; render(); });
  $('#editApp').addEventListener('click', () => { edit = true; render(); });
  $('#openApp').addEventListener('click', () => ctx.call('open_app').then(() => ctx.island.closePanel(true)).catch(err => note(errText(err))));
  $('#forget').addEventListener('click', () => {
    // A second click within 3 s confirms.
    const b = $('#forget');
    if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget this app'; }, 3000); return; }
    delete b.dataset.sure; b.textContent = 'Forget this app';
    $('#cid').value = ''; $('#secret').value = ''; edit = false;
    action('forget').then(load);
  });
  $('#voice').addEventListener('click', e => {
    const b = e.target.closest('[data-dc]'); if (!b) return;
    if (b.dataset.dc === 'open') $('#openApp').click();
    else if (b.dataset.dc === 'edit') { edit = true; render(); }
    else action(b.dataset.dc);
  });
  $('#mute').addEventListener('click', () => { if (dc) { dc.mute = !dc.mute; render(); renderCall(); action(dc.mute ? 'mute' : 'unmute'); } });
  $('#deaf').addEventListener('click', () => { if (dc) { dc.deaf = !dc.deaf; render(); renderCall(); action(dc.deaf ? 'deafen' : 'undeafen'); } });
  $('#leave').addEventListener('click', () => action('leave'));
  $('#presence').addEventListener('change', e => ctx.call('options', { presence: e.target.checked }).catch(err => note(errText(err))));
  $('#notify').addEventListener('change', e => ctx.call('options', { notify: e.target.checked }).catch(err => note(errText(err))));
  render();
}

export function activate(c) {
  ctx = c;
  // The tab asks the backend for the status (starting it if Discord isn't open, to set it up).
  ctx.tabs.register({ id: 'discord', mount, show: load });
  ctx.cubes.register({ id: 'discord', stat: () => (dc && dc.voice ? `${dc.voice.members.length} in ${dc.voice.name}` : dc && dc.connected ? 'Connected' : 'Not connected') });
  ctx.setup.card(() => ({ state: dc && dc.authorized ? 'Connected' : '' }));
  ctx.on('status', setStatus);
  ctx.on('speaking', ({ id, speaking }) => {
    const v = dc && dc.voice; if (!v) return;
    const m = v.members.find(x => x.id === id); if (!m) return;
    m.speaking = speaking;
    const i = order.indexOf(id);
    if (speaking && i >= 3) { order.splice(i, 1); order.unshift(id); }
    renderCall(); talk();
  });
  // The backend comes and goes with Discord.
  ctx.backend.on(st => {
    if (st === 'running') load();
    else if (st !== 'starting' && dc && (dc.connected || dc.voice)) setStatus({ ...dc, connected: false, authorized: false, voice: null });
  });
  if (ctx.backend.running()) load();
}

export function deactivate() {
  ctx.activity.clear('call');
  ctx.chips.remove('call');
  ctx.commands.set([]);
}
