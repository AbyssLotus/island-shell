// The Focus timer's rules, with no DOM, so they run in Node too (dev/tests/focus.test.js).
//
// A session goes focus → short break → focus …, with a long break after every `every` focus
// stretches. State is { phase: 'focus' | 'short' | 'long', run: 'idle' | 'running' | 'paused',
// ends_at (Unix ms, while running), left (s, while idle or paused), total (s), done (focus
// stretches finished in this cycle), away (paused because the user stepped away) }.

export const DEFAULTS = { focus: 25, short: 5, long: 15, every: 4, autoBreak: true, autoFocus: false, hold: true, away: true, goal: 4 };

/** Settings with anything missing or out of range put back to the defaults. */
export function settings(saved) {
  const c = { ...DEFAULTS, ...(saved || {}) };
  const mins = (v, lo, hi, d) => (Number.isFinite(+v) && +v >= lo && +v <= hi ? Math.round(+v) : d);
  c.focus = mins(c.focus, 1, 180, DEFAULTS.focus);
  c.short = mins(c.short, 1, 60, DEFAULTS.short);
  c.long = mins(c.long, 1, 90, DEFAULTS.long);
  c.every = mins(c.every, 1, 12, DEFAULTS.every);
  c.goal = mins(c.goal, 1, 24, DEFAULTS.goal);
  for (const k of ['autoBreak', 'autoFocus', 'hold', 'away']) c[k] = !!c[k];
  return c;
}

export const lengthOf = (phase, cfg) => 60 * (phase === 'short' ? cfg.short : phase === 'long' ? cfg.long : cfg.focus);

export function fresh(cfg, phase = 'focus', done = 0) {
  const total = lengthOf(phase, cfg);
  return { phase, run: 'idle', ends_at: 0, left: total, total, done, away: false };
}

/** A saved state, checked, or a new one. */
export function load(saved, cfg) {
  const s = saved && typeof saved === 'object' ? saved : null;
  if (!s || !['focus', 'short', 'long'].includes(s.phase) || !['idle', 'running', 'paused'].includes(s.run)) return fresh(cfg);
  const total = Math.max(1, Math.round(+s.total || lengthOf(s.phase, cfg)));
  return { phase: s.phase, run: s.run, ends_at: +s.ends_at || 0, left: Math.min(total, Math.max(0, Math.round(+s.left || 0))), total, done: Math.max(0, Math.round(+s.done || 0)), away: !!s.away };
}

/** Seconds left at `now`. */
export const leftOf = (st, now) => (st.run === 'running' ? Math.max(0, Math.ceil((st.ends_at - now) / 1000)) : st.left);

export function start(st, now) {
  if (st.run === 'running') return st;
  const left = st.left > 0 ? st.left : st.total;
  return { ...st, run: 'running', ends_at: now + left * 1000, left, away: false };
}

export function pause(st, now, away = false) {
  if (st.run !== 'running') return st;
  return { ...st, run: 'paused', left: leftOf(st, now), ends_at: 0, away };
}

/** Back to a full focus stretch, not running; the cycle starts over. */
export const reset = cfg => fresh(cfg);

/** What comes after `phase`, with `done` focus stretches finished in the cycle. */
export function nextPhase(phase, done, cfg) {
  if (phase !== 'focus') return 'focus';
  return done > 0 && done % cfg.every === 0 ? 'long' : 'short';
}

/**
 * The current stretch is over (`counted`: it ran to the end, rather than being skipped). Returns
 * { st, finished: phase, minutes } where minutes is the focus time to log.
 */
export function finish(st, now, cfg, counted = true, autoStart = true) {
  const wasFocus = st.phase === 'focus';
  const done = wasFocus && counted ? st.done + 1 : st.done;
  const phase = nextPhase(st.phase, wasFocus ? done : st.done, cfg);
  // After a long break the cycle starts over.
  let next = fresh(cfg, phase, st.phase === 'long' ? 0 : done);
  const auto = autoStart && (wasFocus ? cfg.autoBreak : cfg.autoFocus);
  if (auto) next = start(next, now);
  const minutes = wasFocus && counted ? Math.round(st.total / 60) : 0;
  return { st: next, finished: st.phase, minutes };
}

/**
 * Called every second or so: the stretch that ran out, if one did. A stretch that ended long ago
 * (the PC was asleep) finishes without starting the next one or telling anyone.
 */
export function tick(st, now, cfg) {
  if (st.run !== 'running' || now < st.ends_at) return null;
  const late = now - st.ends_at > 10 * 60e3;
  const r = finish(st, now, cfg, true, !late);
  return { ...r, late };
}

/* ---------- the log: focus minutes per day ---------- */

export const dayKey = d => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

/** Adds a finished focus stretch to the log ({ 'YYYY-MM-DD': { n, mins } }), keeping 90 days. */
export function record(log, day, minutes) {
  const out = { ...(log || {}) };
  const d = out[day] || { n: 0, mins: 0 };
  out[day] = { n: d.n + 1, mins: d.mins + minutes };
  const keys = Object.keys(out).sort();
  for (const k of keys.slice(0, Math.max(0, keys.length - 90))) delete out[k];
  return out;
}

/** Days in a row, up to `today`, that reached the goal (today counts once it's reached). */
export function streak(log, today, goal) {
  let n = 0;
  const d = new Date(today);
  if (!((log[dayKey(d)] || {}).n >= goal)) d.setDate(d.getDate() - 1);
  while ((log[dayKey(d)] || {}).n >= goal) { n++; d.setDate(d.getDate() - 1); }
  return n;
}

/** "1h 15m", "40m". */
export function duration(mins) {
  const h = Math.floor(mins / 60), m = mins % 60;
  return h ? `${h}h${m ? ` ${m}m` : ''}` : `${m}m`;
}
