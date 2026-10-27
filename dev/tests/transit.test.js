// The Transit module's commute windows (modules/transit/ui/transit.js): when a stop's next
// departure goes on the pill.
import test from 'node:test';
import assert from 'node:assert/strict';
import { inWindow } from '../../modules/transit/ui/transit.js';

test('a stop is on the pill on its days, between its times', () => {
  const stops = [
    { key: 'a', window: { days: [0, 1, 2, 3, 4], from: '07:30', to: '09:00' } },
    { key: 'b', window: { days: [5], from: '22:00', to: '23:59' } },
    { key: 'c', window: null }
  ];
  const at = (y, m, d, h, mi) => new Date(y, m - 1, d, h, mi);
  // Tuesday 27 October 2026.
  assert.deepEqual(inWindow(stops, at(2026, 10, 27, 7, 30)).map(s => s.key), ['a']);
  assert.deepEqual(inWindow(stops, at(2026, 10, 27, 8, 59)).map(s => s.key), ['a']);
  assert.deepEqual(inWindow(stops, at(2026, 10, 27, 9, 0)).map(s => s.key), []);
  assert.deepEqual(inWindow(stops, at(2026, 10, 27, 7, 29)).map(s => s.key), []);
  // Saturday night; Sunday is 6.
  assert.deepEqual(inWindow(stops, at(2026, 10, 31, 22, 15)).map(s => s.key), ['b']);
  assert.deepEqual(inWindow(stops, at(2026, 11, 1, 8, 0)).map(s => s.key), []);
  assert.deepEqual(inWindow(null), []);
});
