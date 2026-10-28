// The island's Discord tab, as a fireteam: everyone in your call as a card (whoever's talking lit,
// in Destiny's brackets; hover someone for their volume, right-click for the rest), your Mic, Sound
// and Leave, and the status and @mention switches. Not in a call: five empty slots with Rejoin and
// your latest ping. Discord not open, not authorized, and connecting your Discord application (three
// steps that tick as you go) each get one card. Everything comes from `core` (discord.js).

import { sigil, state, stateHtml, talking, ago, clock, fill, personMenu, closeMenu } from './fireteam.js';

const REDIRECT = 'http://localhost';

export function createTab(core) {
  let root = null, shown = false, edit = false, all = false, dragging = false, stale = false, timer = 0, pingShown = false;
  const ctx = () => core.ctx;
  const $ = s => root.querySelector(s);
  const $$ = s => [...root.querySelectorAll(s)];

  function view() {
    const { icon } = ctx().ui;
    return `<div class="ft">
      <div class="ft-setup" id="setup" hidden>
        <header class="ft-head">${sigil(22)}<div class="ft-ht"><h2>Connect Discord</h2><p>Calls, pings and “Listening to Apple Music” on the island</p></div><span class="ft-step" id="stepN"></span><button class="btn" type="button" id="back" hidden>Cancel</button></header>
        <ol class="ft-steps">
          <li data-step="0"><span class="ft-obj" aria-hidden="true"></span><span class="ft-sx">Make an app in the Discord Developer Portal and name it <b>Apple Music</b>, so your status reads “Listening to Apple Music”.</span><button class="ft-sb" type="button" id="portal">Open the portal${icon('link')}</button></li>
          <li data-step="1"><span class="ft-obj" aria-hidden="true"></span><span class="ft-sx">In the app’s <b>OAuth2</b> page, add this redirect and save.</span><code>${REDIRECT}</code><button class="ft-sb" type="button" id="copy">Copy</button></li>
          <li data-step="2"><span class="ft-obj" aria-hidden="true"></span><span class="ft-sx">Paste the app’s <b>Client ID</b> and <b>Client Secret</b> from the same page.</span></li>
        </ol>
        <form class="ft-form" id="form" autocomplete="off">
          <input class="field" id="cid" placeholder="Client ID" inputmode="numeric" spellcheck="false" aria-label="Client ID">
          <input class="field" id="secret" type="password" placeholder="Client Secret" spellcheck="false" aria-label="Client Secret">
          <button class="btn primary" type="submit" id="connect" disabled>Connect</button>
        </form>
        <div class="k-fine ft-fine"><span>The secret and Discord’s sign-in are kept encrypted for your Windows account only.</span><span class="grow"></span><button class="k-link" type="button" id="forget" hidden>Forget this app</button></div>
      </div>
      <div class="ft-main" id="main" hidden></div>
      <p class="note" id="note" hidden></p>
    </div>`;
  }

  /* ---------- connecting your Discord application ---------- */
  /** How many of the three steps are done (opening the portal and copying the redirect tick them). */
  function stepsDone() {
    const d = core.status() || {};
    return d.configured || /\d/.test($('#cid').value) ? 2 : Math.min(2, ctx().store.get('setupDone', 0));
  }
  function renderSetup() {
    const d = core.status() || {}, id = $('#cid');
    if (!id.value && d.client_id) id.value = d.client_id;
    $('#secret').placeholder = d.has_secret ? 'Client Secret (saved; blank keeps it)' : 'Client Secret';
    $('#back').hidden = !d.configured; $('#forget').hidden = !d.configured;
    const now = stepsDone();
    for (const li of $$('[data-step]')) {
      const i = +li.dataset.step;
      li.className = i < now ? 'done' : i === now ? 'now' : '';
    }
    $('#stepN').textContent = `Step ${now + 1} of 3`;
    $('#connect').disabled = !/^\d+$/.test(id.value.trim());
  }
  function stepDone(n) {
    ctx().store.set('setupDone', Math.max(n, ctx().store.get('setupDone', 0)));
    renderSetup();
  }

  /* ---------- the fireteam ---------- */
  function gate(title, text, button, foot = '') {
    return `<div class="ft-gate">${sigil(30)}<div><b>${title}</b><span>${text}</span></div>${button}${foot}</div>`;
  }

  function head(d) {
    const { esc, icon } = ctx().ui, v = d.voice, last = d.last;
    const title = v ? esc(v.name) : 'Not in voice';
    const sub = v
      ? `${esc(v.guild || 'Voice')} · ${v.members.length} in voice · <span class="ft-time" data-since="${v.since || Date.now()}">${clock(v.since || Date.now())}</span>`
      : last ? `Your last call: ${esc(last.name)}${last.guild ? `, ${esc(last.guild)}` : ''} · left ${last.left_at && ago(last.left_at) !== 'now' ? `${ago(last.left_at)} ago` : 'just now'}` : 'Join a voice channel in Discord, or from the Command Center';
    return `<header class="ft-head${v ? '' : ' idle'}">${sigil(22)}<div class="ft-ht"><h2>${title}</h2><p>${sub}</p></div>
      <button class="ft-link" type="button" data-a="open-app">Open Discord${icon('link')}</button>
      <button class="ib ft-gear" type="button" data-a="edit" aria-label="App settings" title="App settings">${icon('gear')}</button></header>`;
  }

  function card(m, isMe) {
    const { esc } = ctx().ui, ui = ctx().ui, vol = Math.round(m.volume);
    const [cls] = state(m, isMe);
    return `<li class="ft-card ${cls}${isMe ? ' me' : ''}" data-id="${esc(m.id)}">
      ${isMe ? '<span class="ft-you">You</span>' : ''}
      <img class="ft-av" src="${esc(core.avatarOf(m))}" alt="" referrerpolicy="no-referrer" draggable="false">
      <b>${esc(m.name)}</b>
      <span class="ft-st">${stateHtml(ui, m, isMe)}</span>
      ${isMe ? '' : `<label class="ft-vol"><span>Vol<output>${vol}%</output></span><input class="ft-range" type="range" min="0" max="200" value="${vol}" style="--p:${vol / 2}%" aria-label="${esc(m.name)}'s volume for you"></label>`}
    </li>`;
  }

  function roster(d) {
    const { esc } = ctx().ui, v = d.voice, me = core.me(), n = v.members.length;
    if (all && n > 5) {
      return `<div class="ft-allhead"><span>Everyone in ${esc(v.name)}</span><button class="k-link" type="button" data-a="all">Show less</button></div>
        <ul class="ft-roster all" aria-label="In voice">${core.roster(v, n).map(m => card(m, m.id === me)).join('')}</ul>`;
    }
    const list = core.roster(v, n > 5 ? 4 : 5), cards = list.map(m => card(m, m.id === me));
    if (n > 5) {
      const rest = v.members.filter(m => !list.includes(m));
      cards.push(`<li><button class="ft-more" type="button" data-a="all" aria-label="Show all ${n} people"><span class="ft-faces">${rest.slice(0, 6).map(m => `<img src="${esc(core.avatarOf(m))}" alt="" referrerpolicy="no-referrer" draggable="false">`).join('')}</span><b>+${rest.length}</b><small>Show all</small></button></li>`);
    }
    return `<ul class="ft-roster" aria-label="In voice">${cards.join('')}</ul>`;
  }

  function empty(d) {
    const { esc } = ctx().ui, last = d.last, n = d.last_people.length;
    const line = !last ? 'Join a voice channel and your fireteam shows up here'
      : n ? `${n} ${n === 1 ? 'person is' : 'people are'} still in ${esc(last.name)}` : `Nobody’s in ${esc(last.name)} right now`;
    return `<div class="ft-empty"><ul class="ft-slots" aria-hidden="true"><li></li><li></li><li></li><li></li><li></li></ul>
      <div class="ft-callout"><div><b>Your fireteam is empty</b><span>${line}</span></div>${last ? '<button class="btn primary" type="button" data-a="rejoin">Rejoin</button>' : ''}</div></div>`;
  }

  function ping(p) {
    const { esc } = ctx().ui;
    return `<button class="ft-ping${p.new ? ' new' : ''}" type="button" data-ping="${esc(p.id)}"><span class="ft-tag">Ping</span><img src="${esc(p.avatar)}" alt="" referrerpolicy="no-referrer" draggable="false"><span class="ft-pt"><b>${esc(p.author)}</b> <i>${esc(p.channel)}</i> ${esc(p.text)}</span><small>${ago(p.at)}</small></button>`;
  }

  function controls(d) {
    const { esc, icon } = ctx().ui, v = d.voice;
    const status = !d.presence ? 'Status off' : d.presence_now ? `Status: ${d.presence_now}` : 'Status: shows Apple Music';
    return `<div class="ft-ctl">
      <button class="ft-tile" type="button" data-a="mute" aria-pressed="${!!d.mute}"><span class="ft-ti">${icon(d.mute ? 'micoff' : 'mic')}</span><span><b>Mic</b><small>${d.mute ? 'Muted' : 'Live'}</small></span></button>
      <button class="ft-tile" type="button" data-a="deaf" aria-pressed="${!!d.deaf}"><span class="ft-ti">${icon(d.deaf ? 'headoff' : 'headphones')}</span><span><b>Sound</b><small>${d.deaf ? 'Deafened' : 'On'}</small></span></button>
      <button class="ft-tile leave" type="button" data-a="leave"${v ? '' : ' disabled'}><span class="ft-ti">${icon('phoneoff')}</span><span><b>Leave</b><small>${v ? esc(v.name) : 'No call'}</small></span></button>
      <div class="ft-chips">
        <button class="ft-chip" type="button" data-a="presence" aria-pressed="${!!d.presence}" title="Show what you’re listening to on your Discord status">${icon('note')}<span>${esc(status)}</span></button>
        <button class="ft-chip" type="button" data-a="notify" aria-pressed="${!!d.notify}" title="Show it on the island when someone @mentions you">${icon('bellon')}<span>@mention alerts ${d.notify ? 'on' : 'off'}</span></button>
      </div>
    </div>`;
  }

  function main(d) {
    const settings = '<button class="k-link" type="button" data-a="edit">App settings</button>';
    if (!d.connected) return gate('Discord isn’t open', 'Open it and your calls, pings and status show up here.', '<button class="btn primary" type="button" data-a="open-app">Open Discord</button>', settings);
    if (!d.authorized) {
      return d.has_secret
        ? gate('Join your fireteam', 'Let the island see your calls and pings. Discord asks you to confirm.', '<button class="btn primary" type="button" data-a="authorize">Authorize</button>', settings)
        : gate('One more step', 'Add your app’s Client Secret to see your calls and pings.', '<button class="btn primary" type="button" data-a="edit">Add the secret</button>');
    }
    pingShown = !d.voice && d.pings.length > 0;
    return head(d) + (d.voice ? roster(d) : empty(d) + (pingShown ? ping(d.pings[0]) : '')) + controls(d);
  }

  function render() {
    if (!root) return;
    const d = core.status() || {}, setup = !d.configured || edit;
    $('#setup').hidden = !setup; $('#main').hidden = setup;
    ctx().ui.note($('#note'), core.error() || d.error || '');
    if (setup) renderSetup();
    else if (dragging) stale = true;
    else ctx().ui.setHtml($('#main'), main(d)); // the menu on someone isn't in #main: it stays while their volume changes
    if (shown) ctx().island.layout();
  }

  /** Who's talking, without redrawing the cards. */
  function talk() {
    const v = core.status() && core.status().voice; if (!v || !root) return;
    const me = core.me(), ui = ctx().ui;
    for (const li of $$('.ft-card[data-id]')) {
      const m = v.members.find(x => x.id === li.dataset.id); if (!m) continue;
      const on = talking(m);
      if (li.classList.contains('talk') === on) continue;
      li.classList.toggle('talk', on);
      li.querySelector('.ft-st').innerHTML = stateHtml(ui, m, m.id === me);
    }
  }

  function tick() {
    for (const t of $$('.ft-time')) t.textContent = clock(+t.dataset.since);
  }

  function act(a) {
    const d = core.status();
    if (a === 'open-app') core.openApp().then(ok => ok && ctx().island.closePanel(true));
    else if (a === 'edit') { edit = true; render(); }
    else if (a === 'authorize') core.action('authorize');
    else if (a === 'mute') core.mute();
    else if (a === 'deaf') core.deafen();
    else if (a === 'leave') core.leave();
    else if (a === 'rejoin') core.rejoin();
    else if (a === 'all') { all = !all; render(); }
    else if (a === 'presence' && d) core.options({ presence: !d.presence });
    else if (a === 'notify' && d) core.options({ notify: !d.notify });
  }

  function onClick(e) {
    const a = e.target.closest('[data-a]');
    if (a) { act(a.dataset.a); return; }
    const p = e.target.closest('[data-ping]');
    if (p) {
      const ping = (core.status().pings || []).find(x => x.id === p.dataset.ping);
      if (ping) core.open({ guild: ping.guild_id, channel: ping.channel_id, message: ping.id }).then(ok => ok && ctx().island.closePanel(true));
    }
  }

  function mount(r) {
    root = r;
    root.innerHTML = view();
    const box = $('.ft');
    $('#portal').addEventListener('click', () => { core.openWeb('https://discord.com/developers/applications'); stepDone(1); });
    $('#copy').addEventListener('click', () => {
      navigator.clipboard.writeText(REDIRECT).then(() => { const b = $('#copy'); b.textContent = 'Copied'; setTimeout(() => { b.textContent = 'Copy'; }, 1500); }).catch(() => {});
      stepDone(2);
    });
    $('#cid').addEventListener('input', renderSetup);
    $('#form').addEventListener('submit', e => {
      e.preventDefault();
      const id = $('#cid').value.trim(), secret = $('#secret').value.trim();
      if (!/^\d+$/.test(id)) return;
      core.configure(id, secret).then(ok => {
        if (ok === false) return;
        $('#secret').value = '';
        edit = false; core.autoAuth = !!secret || !!(core.status() && core.status().has_secret);
        core.load();
      });
    });
    $('#back').addEventListener('click', () => { edit = false; $('#secret').value = ''; render(); });
    $('#forget').addEventListener('click', () => {
      // A second click within 3 s confirms.
      const b = $('#forget');
      if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget this app'; }, 3000); return; }
      delete b.dataset.sure; b.textContent = 'Forget this app';
      $('#cid').value = ''; $('#secret').value = ''; edit = false;
      ctx().store.set('setupDone', 0);
      core.action('forget').then(core.load);
    });
    const main = $('#main');
    main.addEventListener('click', onClick);
    main.addEventListener('contextmenu', e => {
      const li = e.target.closest('.ft-card[data-id]'); if (!li) return;
      const v = core.status().voice, m = v && v.members.find(x => x.id === li.dataset.id);
      if (m) personMenu(core, box, e, m, () => ctx().island.closePanel(true));
    });
    // Someone's volume, from the slider on their card; the cards aren't redrawn while it's held.
    main.addEventListener('pointerdown', e => { if (e.target.matches('.ft-vol input')) dragging = true; });
    addEventListener('pointerup', () => { if (dragging) { dragging = false; if (stale) { stale = false; render(); } } });
    main.addEventListener('input', e => {
      const r = e.target.closest('.ft-vol input'); if (!r) return;
      fill(r); r.closest('.ft-vol').querySelector('output').textContent = `${r.value}%`;
      core.userVoice(r.closest('[data-id]').dataset.id, { volume: +r.value });
    });
    core.on(what => { if (what === 'speaking') talk(); else render(); });
    render();
  }

  return {
    mount,
    show() {
      shown = true;
      // The status (starting the backend if Discord isn't open, to set it up), and who's still in your last call.
      core.load().then(() => { const d = core.status(); if (d && d.authorized && !d.voice) core.browse(); });
      clearInterval(timer); timer = setInterval(tick, 1000);
      render();
    },
    hide() {
      shown = false; all = false;
      clearInterval(timer);
      if (root) closeMenu($('.ft'));
      if (pingShown) core.pingsSeen();
    }
  };
}
