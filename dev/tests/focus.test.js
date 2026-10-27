// The Focus timer module's rules (modules/focus/ui/logic.js): stretches, breaks, the cycle, the log.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as F from '../../modules/focus/ui/logic.js';

const cfg = F.settings({ focus: 25, short: 5, long: 15, every: 2 });
const MIN = 60e3;

test('settings fall back to the defaults when they are missing or out of range', () => {
  assert.deepEqual(F.settings(null), F.DEFAULTS);
  const c = F.settings({ focus: 0, short: '7', long: 900, every: 'x', hold: 0 });
  assert.equal(c.focus, 25); assert.equal(c.short, 7); assert.equal(c.long, 15); assert.equal(c.every, 4); assert.equal(c.hold, false);
});

test('start, pause and resume keep the time left', () => {
  let st = F.fresh(cfg);
  assert.equal(st.left, 1500);
  st = F.start(st, 0);
  assert.equal(st.run, 'running'); assert.equal(st.ends_at, 25 * MIN);
  st = F.pause(st, 10 * MIN);
  assert.equal(st.run, 'paused'); assert.equal(st.left, 900);
  st = F.start(st, 20 * MIN);
  assert.equal(st.ends_at, 35 * MIN);
  assert.equal(F.leftOf(st, 34 * MIN), 60);
  // Stepping away is remembered, and starting again forgets it.
  assert.equal(F.pause(st, 21 * MIN, true).away, true);
  assert.equal(F.start(F.pause(st, 21 * MIN, true), 22 * MIN).away, false);
});

test('focus, short break, focus, long break, then the cycle starts over', () => {
  let st = F.start(F.fresh(cfg), 0), now = 0;
  const run = () => { now = st.ends_at; const r = F.tick(st, now, cfg); st = r.st; return r; };
  let r = run();
  assert.equal(r.finished, 'focus'); assert.equal(r.minutes, 25);
  assert.equal(st.phase, 'short'); assert.equal(st.run, 'running'); assert.equal(st.done, 1);
  r = run();
  assert.equal(r.finished, 'short'); assert.equal(r.minutes, 0);
  // autoFocus is off: the next focus waits.
  assert.equal(st.phase, 'focus'); assert.equal(st.run, 'idle');
  st = F.start(st, now);
  run();
  assert.equal(st.phase, 'long'); assert.equal(st.done, 2); assert.equal(st.total, 15 * 60);
  run();
  assert.equal(st.phase, 'focus'); assert.equal(st.done, 0);
});

test('a skipped focus stretch is not counted; nothing happens before the end', () => {
  const st = F.start(F.fresh(cfg), 0);
  assert.equal(F.tick(st, 24 * MIN, cfg), null);
  const r = F.finish(st, 3 * MIN, cfg, false);
  assert.equal(r.minutes, 0); assert.equal(r.st.done, 0); assert.equal(r.st.phase, 'short');
});

test('a stretch that ended while the PC slept finishes without starting the next', () => {
  const st = F.start(F.fresh(cfg), 0);
  const r = F.tick(st, 25 * MIN + 30 * MIN, cfg);
  assert.equal(r.late, true); assert.equal(r.minutes, 25); assert.equal(r.st.run, 'idle');
});

test('a broken saved state starts fresh', () => {
  assert.deepEqual(F.load({ phase: 'nap', run: 'running' }, cfg), F.fresh(cfg));
  assert.equal(F.load({ phase: 'short', run: 'paused', left: 9999, total: 300, done: 1 }, cfg).left, 300);
});

test('the log counts each day, keeps 90 days and finds streaks', () => {
  let log = {};
  const day = n => F.dayKey(new Date(2026, 9, n));
  for (const n of [20, 21, 22, 22]) log = F.record(log, day(n), 25);
  assert.deepEqual(log[day(22)], { n: 2, mins: 50 });
  assert.equal(F.streak(log, new Date(2026, 9, 22), 1), 3);
  assert.equal(F.streak(log, new Date(2026, 9, 22), 2), 1);
  // Today not reached yet: the streak up to yesterday still counts.
  assert.equal(F.streak(log, new Date(2026, 9, 23), 1), 3);
  for (let i = 0; i < 100; i++) log = F.record(log, F.dayKey(new Date(2026, 0, 1 + i)), 25);
  assert.equal(Object.keys(log).length, 90);
  assert.equal(F.duration(75), '1h 15m'); assert.equal(F.duration(40), '40m'); assert.equal(F.duration(120), '2h');
});
