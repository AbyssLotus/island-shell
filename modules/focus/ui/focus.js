// The Focus timer: focus stretches and breaks (logic.js), in place of Today's own timer card, on
// the pill (a `timer` activity), as a Controls tile and as Command Center commands. While a focus
// stretch runs, module notifications wait (ctx.island.quiet); with the eye tracker, stepping away
// pauses it and coming back carries on. There's no backend: the state lives in the module's store,
// so it survives the island restarting.

import * as F from './logic.js';

let ctx, root = null, shown = false, T = 0;
let cfg = F.settings(null), st = null, log = {};
const $ = s => root && root.querySelector(s);

const NAMES = { focus: 'Focus', short: 'Short break', long: 'Long break' };
const clock = ms => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
const mmss = s => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;

function save() {
  ctx.store.set('state', st);
  ctx.store.set('log', log);
}

/* ---------- what the rest of the island sees ---------- */

/** The pill, the Controls tile, the commands and holding notifications, from the state. */
function publish() {
  const running = st.run === 'running', focusing = running && st.phase === 'focus';
  if (st.run === 'idle') ctx.activity.clear('timer');
  else ctx.activity.set({ id: 'timer', kind: 'timer', text: NAMES[st.phase], total: st.total, tone: st.phase === 'focus' ? '' : 'break', action: 'tab:today', ...(running ? { ends_at: st.ends_at } : { paused: true, left: st.left }) });
  ctx.island.quiet(focusing && cfg.hold, 'Focus');
  const sub = running ? `${st.phase === 'focus' ? 'Until' : 'Break until'} ${clock(st.ends_at)}` : st.run === 'paused' ? (st.away ? 'Paused while you’re away' : 'Paused') : `${cfg.focus} min`;
  ctx.tiles.set({ id: 'focus', icon: 'timer', title: 'Focus', sub, pressed: running, order: 30, run: () => toggle() });
  const cmds = [];
  if (running) cmds.push({ id: 'pause', title: `Pause ${st.phase === 'focus' ? 'focus' : 'the break'}`, icon: 'pause', keys: 'focus pomodoro timer pause', run: () => toggle() });
  else cmds.push({ id: 'start', title: st.run === 'paused' ? 'Resume focus' : st.phase === 'focus' ? 'Start focus' : `Start the ${NAMES[st.phase].toLowerCase()}`, icon: 'timer', keys: 'focus pomodoro timer start study', run: () => toggle() });
  if (st.run !== 'idle' || st.phase !== 'focus') cmds.push({ id: 'skip', title: st.phase === 'focus' ? 'Skip to the break' : 'End the break', icon: 'next', keys: 'focus pomodoro break skip', run: () => skip() });
  if (st.run !== 'idle' || st.done) cmds.push({ id: 'reset', title: 'Reset focus', icon: 'restart', keys: 'focus pomodoro timer stop reset', run: () => resetAll() });
  ctx.commands.set(cmds);
}

function set(next) {
  st = next;
  save();
  publish();
  loop();
  render();
}

/* ---------- actions ---------- */

function toggle() {
  const now = Date.now();
  set(st.run === 'running' ? F.pause(st, now) : F.start(st, now));
}
function skip() {
  const r = F.finish(st, Date.now(), cfg, false);
  set(r.st);
}
function resetAll() { set(F.reset(cfg)); }

/** A stretch ran out: log it, say so (after letting notifications through), and go on. */
function finished(r) {
  if (r.minutes) log = F.record(log, F.dayKey(new Date()), r.minutes);
  set(r.st);
  if (r.late) return;
  const next = r.st, auto = next.run === 'running';
  if (r.finished === 'focus') {
    const today = (log[F.dayKey(new Date())] || {}).n || 0;
    const goal = today === cfg.goal ? ` That’s your ${cfg.goal} for today.` : '';
    ctx.notify({ title: 'Focus done', body: `${auto ? 'Your' : 'Time for a'} ${Math.round(next.total / 60)}-minute ${next.phase === 'long' ? 'long ' : ''}break${auto ? ' has started' : ''}.${goal}`, icon: 'timer', sound: 'timer', action: 'tab:today' });
  } else {
    ctx.notify({ title: 'Break’s over', body: auto ? 'The next focus stretch has started.' : 'Start the next focus stretch when you’re ready.', icon: 'timer', sound: 'timer', action: 'tab:today' });
  }
}

/** Once a second while something runs: the end of a stretch, and the card's ring. */
function loop() {
  clearInterval(T); T = 0;
  if (st.run !== 'running') return;
  T = setInterval(() => {
    const r = F.tick(st, Date.now(), cfg);
    if (r) finished(r);
    else if (shown) paintTime();
  }, 1000);
}

/* ---------- stepping away (the eye tracker's presence) ---------- */

function onPresence(p) {
  if (!cfg.away) return;
  if (p === 'away' && st.run === 'running' && st.phase === 'focus') set(F.pause(st, Date.now(), true));
  else if ((p === 'present' || p === 'looking') && st.run === 'paused' && st.away) set(F.start(st, Date.now()));
}

/* ---------- the card in Today ---------- */

function view() {
  const { icon } = ctx.ui;
  return `
  <div class="card fo" id="card">
    <div class="fo-ring"><svg viewBox="0 0 80 80" aria-hidden="true"><circle class="trk" cx="40" cy="40" r="34"/><circle class="val" id="ring" cx="40" cy="40" r="34"/></svg><span><b id="time">25:00</b><small id="phase">Focus</small></span></div>
    <div class="fo-ctl">
      <div class="fo-head"><b>Focus</b><small id="state"></small><button class="ib sm" type="button" id="gear" title="Focus settings" aria-label="Focus settings">${icon('gear')}</button></div>
      <div class="fo-dots" id="dots" aria-hidden="true"></div>
      <div class="fo-btns"><button class="btn primary" type="button" id="go">Start</button><button class="ib" type="button" id="skip">${icon('next')}</button><button class="ib" type="button" id="reset" title="Reset" aria-label="Reset">${icon('restart')}</button></div>
      <small class="fo-today" id="today"></small>
    </div>
  </div>
  <div class="card fo-set" id="set" hidden></div>`;
}

/** The settings, in the card (with Done) or on the module's page in the Modules sheet. */
function settingsHtml(inCard) {
  const { esc } = ctx.ui;
  const num = (k, label, max) => `<label class="fo-num"><span>${esc(label)}</span><input class="field" type="number" min="1" max="${max}" data-k="${k}" value="${cfg[k]}"></label>`;
  const sw = (k, label, sub) => `<label class="switch fo-sw"><input type="checkbox" data-k="${k}"${cfg[k] ? ' checked' : ''}><span>${esc(label)}${sub ? `<small>${esc(sub)}</small>` : ''}</span></label>`;
  const tracker = ctx.island.presence();
  return `
    ${inCard ? '<header><b>Focus settings</b><button class="link-btn" type="button" data-done>Done</button></header>' : ''}
    <div class="fo-nums">${num('focus', 'Focus (min)', 180)}${num('short', 'Short break', 60)}${num('long', 'Long break', 90)}${num('every', 'Long break every', 12)}${num('goal', 'Daily goal', 24)}</div>
    <div class="fo-sws">
      ${sw('autoBreak', 'Start breaks by themselves')}
      ${sw('autoFocus', 'Start the next focus after a break')}
      ${sw('hold', 'Hold notifications while focusing', 'Urgent ones still come through; the rest show when the stretch ends')}
      ${sw('away', 'Pause when I step away', tracker && tracker !== 'off' ? 'With the eye tracker' : 'Needs the Tobii eye tracker module')}
    </div>`;
}

function bindSettings(box, after) {
  box.addEventListener('change', e => {
    const k = e.target.dataset && e.target.dataset.k; if (!k) return;
    const was = cfg;
    cfg = F.settings({ ...cfg, [k]: e.target.type === 'checkbox' ? e.target.checked : +e.target.value });
    ctx.store.set('settings', cfg);
    if (e.target.type !== 'checkbox') e.target.value = cfg[k];
    // A new length applies straight away to a stretch that hasn't started.
    if (st.run === 'idle' && F.lengthOf(st.phase, cfg) !== F.lengthOf(st.phase, was)) set(F.fresh(cfg, st.phase, st.done));
    else { publish(); render(); }
    after && after();
  });
}

function paintTime() {
  const left = F.leftOf(st, Date.now());
  $('#time').textContent = mmss(left);
  $('#ring').style.strokeDashoffset = (213.63 * (1 - left / Math.max(st.total, 1))).toFixed(2);
}

function render() {
  if (!root) return;
  const { setHtml, txt } = ctx.ui;
  paintTime();
  txt($('#phase'), NAMES[st.phase]);
  $('#card').classList.toggle('break', st.phase !== 'focus');
  $('#card').classList.toggle('paused', st.run === 'paused');
  txt($('#state'), st.run === 'running' ? `until ${clock(st.ends_at)}` : st.run === 'paused' ? (st.away ? 'paused, you stepped away' : 'paused') : 'ready');
  $('#go').textContent = st.run === 'running' ? 'Pause' : st.run === 'paused' ? 'Resume' : 'Start';
  const sk = $('#skip'), skipLabel = st.phase === 'focus' ? 'Skip to the break' : 'End the break';
  sk.title = skipLabel; sk.setAttribute('aria-label', skipLabel);
  sk.disabled = st.run === 'idle' && st.phase === 'focus';
  $('#reset').disabled = st.run === 'idle' && st.phase === 'focus' && !st.done;
  // One dot per focus stretch in the cycle: done, the current one, still to come.
  const inCycle = st.done % cfg.every || (st.phase === 'long' ? cfg.every : 0);
  setHtml($('#dots'), Array.from({ length: cfg.every }, (_, i) => `<i class="${i < inCycle ? 'on' : i === inCycle && st.phase === 'focus' ? 'now' : ''}"></i>`).join(''));
  const day = log[F.dayKey(new Date())] || { n: 0, mins: 0 }, run = F.streak(log, new Date(), cfg.goal);
  txt($('#today'), `${day.n}/${cfg.goal} today${day.mins ? ` · ${F.duration(day.mins)}` : ''}${run > 1 ? ` · ${run}-day streak` : ''}`);
  ctx.island.layout();
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#go').addEventListener('click', toggle);
  $('#skip').addEventListener('click', skip);
  $('#reset').addEventListener('click', resetAll);
  $('#gear').addEventListener('click', () => {
    const box = $('#set'), open = box.hidden;
    if (open) ctx.ui.setHtml(box, settingsHtml(true));
    box.hidden = !open; $('#card').hidden = open;
    ctx.island.layout();
  });
  $('#set').addEventListener('click', e => { if (e.target.closest('[data-done]')) { $('#set').hidden = true; $('#card').hidden = false; ctx.island.layout(); } });
  bindSettings($('#set'));
  render();
}

export function activate(c) {
  ctx = c;
  cfg = F.settings(ctx.store.get('settings', null));
  log = ctx.store.get('log', {}) || {};
  st = F.load(ctx.store.get('state', null), cfg);
  // A stretch that ran out while the island was closed.
  const r = F.tick(st, Date.now(), cfg);
  if (r) { if (r.minutes) log = F.record(log, F.dayKey(new Date()), r.minutes); st = { ...r.st, run: 'idle', ends_at: 0, left: r.st.total }; save(); }
  ctx.sections.register({ id: 'timer', mount, show() { shown = true; render(); }, hide() { shown = false; } });
  ctx.island.on('presence', onPresence);
  ctx.settings.mount(box => {
    ctx.ui.setHtml(box, `<div class="fo-set">${settingsHtml(false)}</div>`);
    bindSettings(box);
  });
  publish();
  loop();
}

export function deactivate() {
  clearInterval(T);
  ctx.island.quiet(false);
}
