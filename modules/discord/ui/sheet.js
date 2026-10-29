// The Command Center's Discord sheet: the island's own Discord, built from what Discord's local
// connection gives (no docked website). On the left, your servers as diamonds (a lit corner for a
// ping), then the chosen one's voice channels with who's in each (Join) and its text channels, which
// open in Discord (the connection can read but not send). On the right, your call as tall banners
// with a column of light behind whoever's talking, or, out of a call, Rejoin and where your people
// are; under it Discord's voice settings, what's on your status and your pings. Everything comes
// from `core` (discord.js).

import { sigil, state, stateHtml, talking, ago, clock, fill, initials, personMenu, closeMenu } from './fireteam.js';

/** The signal under a banner: a flat line, or while they talk a wave running along it. */
const WAVE = 'M0 9H10L14 4 18 14 22 2 26 16 30 6 34 12 38 3 42 15 46 7 50 11 54 4 58 13 62 9H72L76 5 80 13 84 6 88 12 92 9H100L104 3 108 15 112 6 116 12 120 9H140';
const signal = `<svg class="fs-sig" viewBox="0 0 140 18" preserveAspectRatio="none" aria-hidden="true"><path class="flat" d="M0 9H140"/><g class="wave"><path d="${WAVE}"/><path d="${WAVE}" transform="translate(140)"/></g></svg>`;
const mmss = s => { s = Math.max(0, Math.floor(s)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };

export function createSheet(core) {
  let el = null, off = null, timer = 0, poll = 0, gsel = '', all = false, dragging = false, stale = false;
  const parts = {};
  const ctx = () => core.ctx;
  const ui = () => ctx().ui;
  const $ = s => el && el.querySelector(s);
  const $$ = s => (el ? [...el.querySelectorAll(s)] : []);
  const close = then => ctx().island.closeCommandCenter(then);
  const h3 = (title, extra = '', after = '') => `<h3 class="fs-h">${title}${extra}<i aria-hidden="true"></i>${after}</h3>`;
  const face = m => `<img src="${ui().esc(core.avatarOf(m))}" alt="" data-id="${ui().esc(m.id)}" referrerpolicy="no-referrer" draggable="false"${talking(m) ? ' class="talk"' : ''}>`;

  /** Puts html in a part of the sheet only when it changed (an open dropdown or a held slider stays). */
  function put(sel, html) {
    const box = $(sel);
    if (!box || parts[sel] === html) return;
    parts[sel] = html;
    ui().setHtml(box, html);
  }

  function gate(d) {
    const g = (title, text, button) => `<div class="ft-gate fs-gate">${sigil(34)}<div><b>${title}</b><span>${text}</span></div>${button}</div>`;
    if (!d.configured) return g('Connect Discord', 'Connect your Discord application in the island’s Discord tab, and your fireteam shows up here.', '<button class="btn primary" type="button" data-a="tab">Open the Discord tab</button>');
    if (!d.connected) return g('Discord isn’t open', 'Open it and your servers, calls, pings and status show up here.', '<button class="btn primary" type="button" data-a="open-app">Open Discord</button>');
    return d.has_secret
      ? g('Join your fireteam', 'Let the island see your servers, calls and pings. Discord asks you to confirm.', '<button class="btn primary" type="button" data-a="authorize">Authorize</button>')
      : g('One more step', 'Add your app’s Client Secret in the island’s Discord tab.', '<button class="btn primary" type="button" data-a="tab">Open the Discord tab</button>');
  }

  /* ---------- the rail: servers and channels ---------- */
  function guildsHtml(d, b) {
    const { esc } = ui();
    const pinged = new Set(d.pings.filter(p => p.new).map(p => p.guild_id));
    return (b ? b.guilds : []).map(g => `<li><button class="fs-g" type="button" data-g="${esc(g.id)}" aria-pressed="${g.id === gsel}" aria-label="${esc(g.name)}${pinged.has(g.id) ? ', pinged' : ''}" title="${esc(g.name)}"><span class="fs-gi">${g.icon ? `<img src="${esc(g.icon)}" alt="" referrerpolicy="no-referrer" draggable="false">` : `<span>${esc(initials(g.name))}</span>`}</span>${pinged.has(g.id) ? '<i class="fs-dot" aria-hidden="true"></i>' : ''}</button></li>`).join('');
  }

  function chansHtml(d, b) {
    const { esc, icon } = ui();
    const g = b && b.guilds.find(x => x.id === gsel);
    if (!g) return `<p class="fs-wait">${b ? 'No servers yet' : 'Looking for your servers…'}</p>`;
    const list = b.channels[gsel], here = d.voice && d.voice.channel_id;
    if (!list) return `<h2>${esc(g.name)}</h2><p class="fs-wait">Looking…</p>`;
    const pinged = new Set(d.pings.filter(p => p.new).map(p => p.channel_id));
    const voice = list.filter(c => c.voice).map(c => {
      const people = c.id === here ? d.voice.members : c.people, n = people.length;
      return `<div class="fs-v${c.id === here ? ' here' : n ? ' busy' : ''}">
        <span class="fs-vn">${icon('speaker')}<span class="grow">${esc(c.name)}</span>${c.id === here ? '<em>You’re here</em>' : `<button class="fs-join" type="button" data-join="${esc(c.id)}">Join</button>`}</span>
        ${n ? `<span class="fs-faces">${people.slice(0, 8).map(face).join('')}${n > 8 ? `<b>+${n - 8}</b>` : ''}</span>` : ''}
      </div>`;
    }).join('');
    const text = list.filter(c => !c.voice).map(c => `<button class="fs-t${pinged.has(c.id) ? ' ping' : ''}" type="button" data-text="${esc(c.id)}"><span class="fs-hash">#</span><span class="grow">${esc(c.name)}</span>${pinged.has(c.id) ? '<i class="fs-dot" aria-label="Pinged"></i>' : ''}</button>`).join('');
    return `<h2>${esc(g.name)}</h2>${h3('Voice')}${voice || '<p class="fs-wait">No voice channels</p>'}${text ? h3('Text · opens in Discord') + text : ''}`;
  }

  /* ---------- the fireteam ---------- */
  function topHtml(d) {
    const { esc, icon } = ui(), v = d.voice, last = d.last;
    const sub = v
      ? `${esc(v.guild || 'Voice')} · ${v.members.length} in voice · <span class="ft-time" data-since="${v.since || Date.now()}">${clock(v.since || Date.now())}</span>`
      : last ? `Your last call: ${esc(last.name)}${last.guild ? `, ${esc(last.guild)}` : ''} · left ${ago(last.left_at) === 'now' ? 'just now' : `${ago(last.left_at)} ago`}` : 'Pick a voice channel on the left to join it';
    return `${sigil(28)}<div class="fs-ht"><h2>${v ? esc(v.name) : 'Not in voice'}</h2><p>${sub}</p></div>
      <button class="ft-tile" type="button" data-a="mute" aria-pressed="${!!d.mute}"><span class="ft-ti">${icon(d.mute ? 'micoff' : 'mic')}</span><span><b>Mic</b><small>${d.mute ? 'Muted' : 'Live'}</small></span></button>
      <button class="ft-tile" type="button" data-a="deaf" aria-pressed="${!!d.deaf}"><span class="ft-ti">${icon(d.deaf ? 'headoff' : 'headphones')}</span><span><b>Sound</b><small>${d.deaf ? 'Deafened' : 'On'}</small></span></button>
      <button class="ft-tile leave" type="button" data-a="leave"${v ? '' : ' disabled'}><span class="ft-ti">${icon('phoneoff')}</span><span><b>Leave</b><small>${v ? esc(v.name) : 'No call'}</small></span></button>`;
  }

  function banner(m, isMe) {
    const { esc } = ui(), vol = Math.round(m.volume);
    return `<li class="fs-b ${state(m, isMe)[0]}${isMe ? ' me' : ''}" data-id="${esc(m.id)}">
      ${isMe ? '<span class="ft-you">You</span>' : ''}
      <img class="ft-av" src="${esc(core.avatarOf(m))}" alt="" referrerpolicy="no-referrer" draggable="false">
      <b>${esc(m.name)}</b><span class="ft-st">${stateHtml(ui(), m, isMe)}</span><span class="grow"></span>${signal}
      ${isMe ? '' : `<label class="ft-vol"><span>Volume<output>${vol}%</output></span><input class="ft-range" type="range" min="0" max="200" value="${vol}" style="--p:${vol / 2}%" aria-label="${esc(m.name)}'s volume for you"></label>`}
    </li>`;
  }

  function teamHtml(d, b) {
    const { esc } = ui(), v = d.voice, me = core.me();
    if (v) {
      const n = v.members.length;
      if (all && n > 5) return `<div class="fs-allhead"><span>Everyone in ${esc(v.name)}</span><button class="k-link" type="button" data-a="all">Show less</button></div><ul class="fs-team all" aria-label="In voice">${core.roster(v, n).map(m => banner(m, m.id === me)).join('')}</ul>`;
      const list = core.roster(v, n > 5 ? 4 : 5), cards = list.map(m => banner(m, m.id === me));
      if (n > 5) {
        const rest = v.members.filter(m => !list.includes(m));
        cards.push(`<li><button class="ft-more fs-more" type="button" data-a="all" aria-label="Show all ${n} people"><span class="ft-faces">${rest.slice(0, 9).map(face).join('')}</span><b>+${rest.length}</b><small>Show all</small></button></li>`);
      }
      return `<ul class="fs-team" aria-label="In voice">${cards.join('')}</ul>`;
    }
    // Not in a call: an open fireteam, Rejoin, and the voice channels with people in them.
    const last = d.last, ln = d.last_people.length;
    const line = !last ? 'Pick a voice channel on the left to join it' : ln ? `${ln} ${ln === 1 ? 'person is' : 'people are'} still in ${esc(last.name)}` : `Nobody’s in ${esc(last.name)} right now`;
    const where = [];
    for (const [gid, list] of Object.entries((b && b.channels) || {})) {
      const g = b.guilds.find(x => x.id === gid);
      for (const c of list) if (c.voice && c.people.length) where.push({ g, c, last: !!last && c.id === last.channel_id });
    }
    where.sort((a, z) => z.last - a.last || z.c.people.length - a.c.people.length);
    const cards = where.slice(0, 3).map(({ g, c, last: mine }) => `<div class="fs-where${mine ? ' last' : ''}"><b>${esc(c.name)}</b><small>${esc(g ? g.name : 'Server')} · ${c.people.length}</small><span class="grow"></span>
      <span class="fs-wf"><span class="fs-faces big">${c.people.slice(0, 5).map(face).join('')}</span><span class="grow"></span><button class="btn${mine ? ' primary' : ''}" type="button" data-join="${esc(c.id)}">${mine ? 'Rejoin' : 'Join'}</button></span></div>`).join('');
    return `<div class="ft-empty fs-empty"><ul class="ft-slots" aria-hidden="true"><li></li><li></li><li></li><li></li><li></li></ul>
        <div class="ft-callout"><div><b>Your fireteam is empty</b><span>${line}</span></div>${last ? `<button class="btn primary" type="button" data-a="rejoin">Rejoin ${esc(last.name)}</button>` : ''}</div></div>
      <section class="fs-people">${h3('Where your people are')}<div class="fs-wheres">${cards || `<p class="fs-wait">${b ? 'Nobody’s in a voice channel right now' : 'Looking…'}</p>`}</div></section>`;
  }

  /* ---------- under the call ---------- */
  function voiceHtml(d) {
    const { esc } = ui(), x = d.devices;
    if (!x) return `${h3('Voice')}<p class="fs-wait">Waiting for Discord’s voice settings…</p>`;
    const opts = (list, sel) => (list.length ? list : [{ id: sel, name: sel === 'default' ? 'Default' : sel }]).map(o => `<option value="${esc(o.id)}"${o.id === sel ? ' selected' : ''}>${esc(o.name || o.id)}</option>`).join('');
    const iv = Math.round(x.input_volume), ov = Math.round(x.output_volume);
    return `${h3('Voice')}
      <label class="fs-row"><span>Input</span><select class="field" data-dev="input">${opts(x.inputs, x.input)}</select></label>
      <label class="fs-row"><span>Output</span><select class="field" data-dev="output">${opts(x.outputs, x.output)}</select></label>
      <div class="fs-row"><span>Mode</span><div class="seg fs-seg" role="group" aria-label="Input mode"><button type="button" data-ptt="0" aria-pressed="${!x.ptt}">Voice</button><button type="button" data-ptt="1" aria-pressed="${!!x.ptt}">Push to talk</button></div></div>
      <label class="fs-row"><span>Mic</span><input class="ft-range" type="range" min="0" max="100" value="${iv}" style="--p:${iv}%" data-vol="input_volume" aria-label="Input volume"><output>${iv}</output></label>
      <label class="fs-row"><span>Sound</span><input class="ft-range" type="range" min="0" max="200" value="${ov}" style="--p:${ov / 2}%" data-vol="output_volume" aria-label="Output volume"><output>${ov}</output></label>`;
  }

  function statusHtml(d) {
    const { esc, icon } = ui(), p = d.playing;
    const now = p
      ? `<div class="fs-now"><span class="fs-art">${p.art ? `<img src="${esc(p.art)}" alt="" referrerpolicy="no-referrer" draggable="false">` : icon('note')}</span><div><small>Apple Music</small><b>${esc(p.title)}</b><span>${esc(p.artist)}</span></div></div>
        ${p.end > p.start ? `<div class="fs-prog" data-start="${p.start}" data-end="${p.end}"><span>0:00</span><i><i></i></i><span>${mmss((p.end - p.start) / 1000)}</span></div>` : ''}`
      : `<p class="fs-wait">${d.presence ? 'Nothing on your status. Play something in Apple Music and it shows here.' : 'Your status shows nothing from the island.'}</p>`;
    return `${h3('On your status')}${now}<span class="grow"></span><label class="switch fs-sw"><input type="checkbox" data-opt="presence"${d.presence ? ' checked' : ''}><span>Show it on my Discord status</span></label>`;
  }

  function pingsHtml(d) {
    const { esc } = ui(), n = d.pings.filter(p => p.new).length;
    const list = d.pings.map(p => `<button class="fs-ping${p.new ? ' new' : ''}" type="button" data-ping="${esc(p.id)}"><img src="${esc(p.avatar)}" alt="" referrerpolicy="no-referrer" draggable="false"><span class="fs-pt"><span><b>${esc(p.author)}</b> <i>${esc(p.channel)}</i></span><span>${esc(p.text)}</span></span><small>${ago(p.at)}</small></button>`).join('');
    return `${h3('Pings', n ? `<small>${n} new</small>` : '', d.pings.length ? '<button class="fs-clear" type="button" data-a="clear">Clear all</button>' : '')}<div class="fs-pings">${list || '<p class="fs-wait">No pings in the last day</p>'}</div>`;
  }

  function render() {
    if (!el) return;
    const d = core.status() || {}, b = core.servers();
    ui().note($('.fs-note'), core.error() || d.error || '');
    const ready = !!(d.connected && d.authorized);
    el.classList.toggle('gated', !ready);
    if (!ready) { put('.fs-gatebox', gate(d)); return; }
    // The server shown: yours now, your last call's, the one you picked, or the first.
    const ids = b ? b.guilds.map(g => g.id) : [];
    if (!ids.includes(gsel)) {
      const want = [d.voice && d.voice.guild_id, d.last && d.last.guild_id, ctx().store.get('guild', ''), ids[0]].find(id => id && ids.includes(id));
      if (want) { gsel = want; core.browse({ guild: gsel }); }
    }
    put('.fs-guilds', guildsHtml(d, b));
    put('.fs-chans', chansHtml(d, b));
    if (dragging) { stale = true; return; }
    put('.fs-top', topHtml(d));
    put('.fs-teambox', teamHtml(d, b));
    put('#fsVoice', voiceHtml(d));
    put('#fsStatus', statusHtml(d));
    put('#fsPings', pingsHtml(d));
    tick();
  }

  /** Who's talking, without redrawing: the banners, their signal, the faces in the rail. */
  function talk() {
    const v = core.status() && core.status().voice; if (!v || !el) return;
    const me = core.me();
    for (const li of $$('.fs-b[data-id]')) {
      const m = v.members.find(x => x.id === li.dataset.id); if (!m) continue;
      const on = talking(m);
      if (li.classList.contains('talk') === on) continue;
      li.classList.toggle('talk', on);
      li.querySelector('.ft-st').innerHTML = stateHtml(ui(), m, m.id === me);
    }
    for (const img of $$('.fs-faces img[data-id]')) {
      const m = v.members.find(x => x.id === img.dataset.id);
      if (m) img.classList.toggle('talk', talking(m));
    }
  }

  function tick() {
    for (const t of $$('.ft-time')) t.textContent = clock(+t.dataset.since);
    const p = $('.fs-prog');
    if (p) {
      const start = +p.dataset.start, end = +p.dataset.end, at = Math.min(end, Math.max(start, Date.now()));
      p.firstElementChild.textContent = mmss((at - start) / 1000);
      p.querySelector('i > i').style.width = `${((at - start) / (end - start)) * 100}%`;
    }
  }

  /** Asks for your servers again: the one shown, and (out of a call) everyone's, for "where your people are". */
  function refresh() {
    const d = core.status();
    if (d && d.authorized) core.browse({ guild: gsel || undefined, all: !d.voice });
  }

  function onClick(e) {
    const t = e.target, d = core.status() || {};
    const g = t.closest('[data-g]');
    if (g) { gsel = g.dataset.g; ctx().store.set('guild', gsel); core.browse({ guild: gsel }); render(); return; }
    const j = t.closest('[data-join]');
    if (j) { core.join(j.dataset.join); return; }
    const tx = t.closest('[data-text]');
    if (tx) { core.open({ guild: gsel, channel: tx.dataset.text }).then(ok => ok && close()); return; }
    const p = t.closest('[data-ping]');
    if (p) {
      const ping = d.pings.find(x => x.id === p.dataset.ping);
      if (ping) core.open({ guild: ping.guild_id, channel: ping.channel_id, message: ping.id }).then(ok => ok && close());
      return;
    }
    const ptt = t.closest('[data-ptt]');
    if (ptt && d.devices) { d.devices.ptt = ptt.dataset.ptt === '1'; core.voice({ ptt: d.devices.ptt }); render(); return; }
    const a = t.closest('[data-a]'); if (!a) return;
    const k = a.dataset.a;
    if (k === 'mute') core.mute();
    else if (k === 'deaf') core.deafen();
    else if (k === 'leave') core.leave();
    else if (k === 'rejoin') core.rejoin();
    else if (k === 'all') { all = !all; render(); }
    else if (k === 'clear') core.clearPings();
    else if (k === 'authorize') core.action('authorize');
    else if (k === 'open-app') core.openApp().then(ok => ok && close());
    else if (k === 'tab') close(() => ctx().open('tab:discord'));
  }

  function onInput(e) {
    const r = e.target;
    if (!r.matches('input[type=range]')) return;
    fill(r);
    const out = r.closest('label').querySelector('output');
    if (r.dataset.vol) {
      if (out) out.textContent = r.value;
      const d = core.status();
      if (d && d.devices) d.devices[r.dataset.vol] = +r.value;
      core.voice({ [r.dataset.vol]: +r.value });
    } else if (r.closest('.ft-vol')) {
      if (out) out.textContent = `${r.value}%`;
      core.userVoice(r.closest('[data-id]').dataset.id, { volume: +r.value });
    }
  }

  function onChange(e) {
    const t = e.target;
    if (t.matches('select[data-dev]')) {
      const d = core.status();
      if (d && d.devices) d.devices[t.dataset.dev] = t.value;
      core.voice({ [t.dataset.dev]: t.value });
    } else if (t.matches('[data-opt="presence"]')) core.options({ presence: t.checked });
  }

  const onUp = () => { if (dragging) { dragging = false; if (stale) { stale = false; render(); } } };

  return {
    mount(host) {
      el = document.createElement('div');
      el.className = 'fs';
      el.innerHTML = `<div class="fs-gatebox"></div>
        <aside class="fs-rail" aria-label="Your servers"><ul class="fs-guilds" aria-label="Servers"></ul><div class="fs-chans"></div></aside>
        <div class="fs-main"><div class="fs-top"></div><div class="fs-teambox"></div>
          <div class="fs-panels"><section class="fs-panel" id="fsVoice"></section><section class="fs-panel" id="fsStatus"></section><section class="fs-panel" id="fsPings"></section></div></div>
        <p class="note fs-note" hidden></p>`;
      for (const k of Object.keys(parts)) delete parts[k];
      host.append(el);
      gsel = '';
      el.addEventListener('click', onClick);
      el.addEventListener('input', onInput);
      el.addEventListener('change', onChange);
      el.addEventListener('pointerdown', e => { if (e.target.matches('input[type=range]')) dragging = true; });
      addEventListener('pointerup', onUp);
      el.addEventListener('contextmenu', e => {
        const li = e.target.closest('.fs-b[data-id]'); if (!li) return;
        const v = core.status().voice, m = v && v.members.find(x => x.id === li.dataset.id);
        if (m) personMenu(core, el, e, m, () => close());
      });
      off = core.on(what => { if (what === 'speaking') talk(); else render(); });
      core.load();
      render();
      refresh();
      timer = setInterval(tick, 1000);
      poll = setInterval(refresh, 15000);
    },
    unmount() {
      clearInterval(timer); clearInterval(poll);
      removeEventListener('pointerup', onUp);
      if (off) { off(); off = null; }
      core.pingsSeen();
      if (el) { closeMenu(el); el.remove(); }
      el = null; all = false; dragging = false;
    },
    /** M mutes you, D deafens; true when it used the key. */
    key(e) {
      if (!el || (e.target.matches && e.target.matches('input, textarea, select'))) return false;
      const d = core.status();
      if (!(d && d.authorized) || e.ctrlKey || e.altKey || e.metaKey) return false;
      if (e.key === 'm' || e.key === 'M') { core.mute(); return true; }
      if (e.key === 'd' || e.key === 'D') { core.deafen(); return true; }
      return false;
    }
  };
}
