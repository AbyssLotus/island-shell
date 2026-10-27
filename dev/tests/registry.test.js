// The registries' rules (src/registry.js): ordering, cube slots, activities, the resting pill, hotkeys.
import test from 'node:test';
import assert from 'node:assert/strict';
import { List, createRegistry, layoutCubes, rankCubes, farness, moveInOrder, swapInOrder, FIXED, NEAR, CSIZE, WIDE, MORE, sortActivities, topActivity, weatherIcon, restWeather, restDate, hotkeyTable, keywordHit } from '../../src/registry.js';

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

const base = ['apps', 'audio', 'today', 'network', 'clipboard', 'stats', 'modules'].map(id => ({ id }));
const seatOf = (l, id) => (l.placed.find(p => p.cube.id === id) || {}).seat;
const near = i => ({ x: NEAR[i][0], y: NEAR[i][1], size: NEAR[i][2] });

test('six cubes stay put; the rest take the seats nearest the eclipse in rank order', () => {
  const mods = [{ id: 'music', module: 'music', slot: 'apps', size: 'L' }, { id: 'games', module: 'games' }, { id: 'discord', module: 'discord' }, { id: 'canvas', module: 'canvas' }, { id: 'destiny', module: 'destiny' }];
  const l = layoutCubes([...base, ...mods]);
  // Fixed places, whatever a manifest asks for (Music wanted Apps's slot and a big size).
  for (const [id, x, y, size] of FIXED) assert.deepEqual(seatOf(l, id), { x, y, size }, id);
  // With no ranking of the user's: the island's arrangement from before.
  assert.deepEqual(l.ranked.map(c => c.id), ['discord', 'network', 'canvas', 'destiny', 'today', 'stats']);
  l.ranked.forEach((c, i) => assert.deepEqual(seatOf(l, c.id), near(i), c.id));
  // The user's ranking wins; cubes it doesn't name follow in the default order.
  const mine = layoutCubes([...base, ...mods], { order: ['stats', 'canvas'] });
  assert.deepEqual(mine.ranked.map(c => c.id), ['stats', 'canvas', 'discord', 'network', 'destiny', 'today']);
  assert.deepEqual(seatOf(mine, 'stats'), near(0));
  // Hidden cubes aren't placed; a fixed one can't be hidden.
  const hid = layoutCubes([...base, ...mods], { hidden: ['today', 'apps'] });
  assert.equal(seatOf(hid, 'today'), undefined);
  assert.deepEqual(hid.hidden.map(c => c.id), ['today']);
  assert.ok(seatOf(hid, 'apps'));
  // Keyboard order: the fixed ones, the ranking, then Modules.
  assert.deepEqual(l.keys, ['apps', 'audio', 'games', 'clipboard', 'music', 'discord', 'network', 'canvas', 'destiny', 'today', 'stats', 'modules']);
});

test('35 modules all get a seat on a wide screen, none overlapping, all on screen', () => {
  const many = Array.from({ length: 35 }, (_, i) => ({ id: 'm' + i, module: 'm' + i }));
  const l = layoutCubes([...base, ...many]);
  assert.equal(l.overflow.length, 0);
  assert.equal(l.placed.length, base.length + 35);
  const s = l.placed.map(p => ({ ...p.seat, id: p.cube.id }));
  for (const a of s) {
    const [w, h] = CSIZE[a.size];
    assert.ok(a.x - w / 2 >= WIDE.left && a.x + w / 2 <= WIDE.right && a.y - h / 2 >= WIDE.top && a.y + h / 2 <= WIDE.bottom, `${a.id} off screen`);
    for (const b of s) if (a !== b) assert.ok(!(Math.abs(a.x - b.x) < (w + CSIZE[b.size][0]) / 2 && Math.abs(a.y - b.y) < (h + CSIZE[b.size][1]) / 2), `${a.id} overlaps ${b.id}`);
  }
  // Better ranked is never further out (for the screen's shape).
  const d = c => farness(seatOf(l, c.id), WIDE);
  const ranks = l.ranked.slice(NEAR.length).map(d);
  ranks.forEach((r, i) => i && assert.ok(r >= ranks[i - 1] - 1e-9));
});

test('a small screen shows what fits and puts the rest in "+N more"; moving and swapping', () => {
  const small = { left: -3.9, right: 3.9, top: -2.2, bottom: 2.4 };
  const many = Array.from({ length: 20 }, (_, i) => ({ id: 'm' + i, module: 'm' + i }));
  const l = layoutCubes([...base, ...many], { bounds: small });
  const more = l.placed.find(p => p.cube.id === MORE);
  assert.ok(more && l.overflow.length > 0);
  assert.equal(more.cube.title, `${l.overflow.length} more`);
  assert.deepEqual(more.cube.more, l.overflow.map(c => c.id));
  assert.equal(l.placed.length - 1 + l.overflow.length, base.length + 20);
  // A missing fixed cube (no Music) leaves its place to a ranked one.
  assert.ok(l.placed.some(p => !p.fixed && p.seat.x === 0 && p.seat.y === 1.62));
  // Moving and swapping work on the ranking as it stands.
  const r = [{ id: 'a' }, { id: 'b' }, { id: 'c' }];
  assert.deepEqual(moveInOrder(r, 'c', -1), ['a', 'c', 'b']);
  assert.deepEqual(moveInOrder(r, 'a', -5), ['a', 'b', 'c']);
  assert.deepEqual(swapInOrder(r, 'a', 'c'), ['c', 'b', 'a']);
  assert.deepEqual(rankCubes([{ id: 'x' }, { id: 'today' }, { id: 'discord' }]).map(c => c.id), ['discord', 'today', 'x']);
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
