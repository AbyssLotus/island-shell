// The registries' rules (src/registry.js): ordering, cube slots, activities, the resting pill, hotkeys.
import test from 'node:test';
import assert from 'node:assert/strict';
import { List, createRegistry, placeCubes, SLOTS, sortActivities, topActivity, weatherIcon, restWeather, restDate, hotkeyTable, keywordHit } from '../../src/registry.js';

test('lists keep order, refuse duplicates and tell listeners', () => {
  const r = createRegistry();
  let changes = 0;
  r.tabs.on(() => changes++);
  const off = r.tabs.add({ id: 'music', order: 20 });
  r.tabs.add({ id: 'controls', order: 10 });
  r.tabs.add({ id: 'destiny', order: 70, module: 'destiny' });
  assert.deepEqual(r.tabs.ids(), ['controls', 'music', 'destiny']);
  assert.throws(() => r.tabs.add({ id: 'music' }), /already/);
  assert.throws(() => r.tabs.add({}), /id/);
  off();
  assert.deepEqual(r.tabs.ids(), ['controls', 'destiny']);
  r.tabs.removeModule('destiny');
  assert.deepEqual(r.tabs.ids(), ['controls']);
  assert.equal(changes, 5);
  // Equal orders fall back to the id, so the order never depends on registration timing.
  const l = new List((a, b) => (a.order ?? 0) - (b.order ?? 0) || (a.id < b.id ? -1 : 1));
  l.add({ id: 'b', order: 1 }); l.add({ id: 'a', order: 1 });
  assert.deepEqual(l.ids(), ['a', 'b']);
});

test('base cubes keep their slots, module cubes take theirs or a spare', () => {
  const base = ['apps', 'audio', 'today', 'network', 'clipboard', 'stats', 'modules'].map(id => ({ id }));
  // No modules: only the base slots draw.
  assert.deepEqual(placeCubes(base).map(p => p.slot.name), ['apps', 'audio', 'today', 'network', 'clipboard', 'stats', 'modules']);
  const mods = [
    { id: 'destiny', module: 'destiny', slot: 'destiny' },
    { id: 'music', module: 'music', slot: 'music' },
    { id: 'pomodoro', module: 'pomodoro' }, // no slot of its own: the first spare
    { id: 'clock', module: 'clock', slot: 'apps' } // a base slot: refused, gets the next spare
  ];
  const placed = placeCubes([...base, ...mods]);
  const at = Object.fromEntries(placed.map(p => [p.cube.id, p.slot.name]));
  assert.equal(at.destiny, 'destiny');
  assert.equal(at.music, 'music');
  assert.equal(at.pomodoro, 'discord');
  assert.equal(at.clock, 'canvas');
  assert.equal(at.apps, 'apps');
  // Slots come back in slot order, with their position and size.
  assert.deepEqual(placed.map(p => p.slot.name), SLOTS.map(s => s[0]).filter(n => Object.values(at).includes(n)));
  const d = placed.find(p => p.cube.id === 'destiny').slot;
  assert.deepEqual([d.x, d.y, d.size], [3.3, -0.84, 'S']);
  // More module cubes than spare slots: the extra ones aren't shown.
  const many = Array.from({ length: 8 }, (_, i) => ({ id: 'm' + i, module: 'm' + i }));
  assert.equal(placeCubes([...base, ...many]).length, base.length + 5);
});

test('activities: priority, then the most recent', () => {
  const list = [
    { id: 'song', kind: 'media', seq: 3 },
    { id: 'focus', kind: 'timer', seq: 1 },
    { id: 'call', kind: 'call', seq: 2 },
    { id: 'x', kind: 'custom', seq: 9 },
    { id: 'loud', kind: 'custom', priority: 40, seq: 0 }
  ];
  assert.deepEqual(sortActivities(list).map(a => a.id), ['loud', 'call', 'focus', 'song', 'x']);
  assert.equal(topActivity([{ id: 'a', kind: 'media', seq: 1 }, { id: 'b', kind: 'media', seq: 2 }]).id, 'b');
  assert.equal(topActivity([]), null);
});

test('four weather icons from WMO codes', () => {
  const want = { 0: 'sun', 1: 'sun', 2: 'cloud', 3: 'cloud', 45: 'cloud', 48: 'cloud', 51: 'rain', 57: 'rain', 61: 'rain', 67: 'rain', 80: 'rain', 82: 'rain', 95: 'rain', 99: 'rain', 71: 'snow', 77: 'snow', 85: 'snow', 86: 'snow' };
  for (const [code, icon] of Object.entries(want)) assert.equal(weatherIcon(+code), icon, `code ${code}`);
  for (const code of [4, 50, 58, 70, 78, 83, 90, 100, -1, 'x']) assert.equal(weatherIcon(code), null, `code ${code}`);
});

test('the resting pill leaves out weather it can\'t trust', () => {
  const now = Date.UTC(2026, 9, 23, 13, 41);
  const cache = { city: 'Springfield', name: 'Springfield', t: now - 3600e3, temp: 22.2, code: 3 };
  assert.deepEqual(restWeather(cache, 'Springfield', 'F', now), { icon: 'cloud', temp: 72 });
  assert.deepEqual(restWeather(cache, 'Springfield', 'C', now), { icon: 'cloud', temp: 22 });
  assert.equal(restWeather({ ...cache, t: now - 3.1 * 3600e3 }, 'Springfield', 'F', now), null, 'older than 3 h');
  assert.equal(restWeather(cache, '', 'F', now), null, 'no city');
  assert.equal(restWeather(cache, 'Shelbyville', 'F', now), null, 'another city');
  assert.equal(restWeather(null, 'Springfield', 'F', now), null, 'no reading');
  assert.equal(restWeather({ ...cache, code: 4 }, 'Springfield', 'F', now), null, 'unknown code');
  assert.equal(restDate(new Date(2026, 9, 23), 'en-US'), 'Fri 23');
});

test('hotkeys: the base keeps its letters, the first module keeps a contested one', () => {
  const t = hotkeyTable([
    { id: 'games', hotkeys: [{ key: 'G', opens: 'tab:games' }, { key: 'A', opens: 'tab:games' }] },
    { id: 'other', hotkeys: [{ key: 'G', opens: 'tab:other' }, { key: 'O', opens: 'cube:other' }] }
  ]);
  assert.equal(t.A, 'tab:audio');
  assert.equal(t.G, 'tab:games');
  assert.equal(t.O, 'cube:other');
  assert.deepEqual(Object.keys(hotkeyTable([])).sort(), ['A', 'N', 'V', 'W']);
});

test('search hints by module keywords', () => {
  assert.ok(keywordHit('lyrics', ['lyrics', 'now playing']));
  assert.ok(keywordHit('lyr', ['lyrics']));
  assert.ok(!keywordHit('ly', ['lyrics']), 'too short');
  assert.ok(!keywordHit('calendar', ['lyrics']));
});
