// Screenshots of the harness in headless Chromium, and a check that the page logs no errors.
//   node dev/harness/shoot.mjs                 every shot in SHOTS
//   node dev/harness/shoot.mjs power cc        only those
// Images go to dev/harness/out/ (not committed). Exits 1 if any page error or console error happened.
import { createRequire } from 'node:module';
import path from 'node:path';
import fs from 'node:fs';
import url from 'node:url';
import { start } from './serve.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const here = path.dirname(url.fileURLToPath(import.meta.url));
const out = path.join(here, 'out');

// name → query string. The island window is 820×660 (tauri.conf.json); the Command Center is full screen.
export const SHOTS = {
  pill: '',
  controls: 'tab=controls', music: 'tab=music', audio: 'tab=audio', today: 'tab=today', canvas: 'tab=canvas',
  network: 'tab=network', discord: 'tab=discord', games: 'tab=games', destiny: 'tab=destiny', stats: 'tab=stats',
  clipboard: 'tab=clipboard', apps: 'tab=apps', wallpaper: 'tab=wallpaper', power: 'tab=power',
  'power-update': 'tab=power&scenario=update',
  bar: 'style=bar',
  cc: 'cc=1',
};
const BIG = new Set(['cc']);

const only = process.argv.slice(2);
const names = only.length ? only : Object.keys(SHOTS);
fs.mkdirSync(out, { recursive: true });
const server = await start(0);
const port = server.address().port;
const browser = await chromium.launch();
let failed = false;
for (const name of names) {
  const q = SHOTS[name] ?? name;
  const big = BIG.has(name) || /(^|&)(cc|sheet)=/.test(q);
  const page = await browser.newPage({ viewport: big ? { width: 1600, height: 900 } : { width: 820, height: 660 } });
  const problems = [];
  page.on('pageerror', e => problems.push(`pageerror: ${e.message}`));
  // "Failed to load resource" is reported again, with its URL, by requestfailed below.
  page.on('console', m => { if (m.type() === 'error' && !m.text().startsWith('Failed to load resource')) problems.push(`console: ${m.text()}`); });
  page.on('requestfailed', r => { if (new URL(r.url()).hostname === '127.0.0.1') problems.push(`failed: ${r.url()}`); });
  // Nothing leaves the machine: fonts fall back, weather and artwork stay empty.
  await page.route(u => u.hostname !== '127.0.0.1', r => r.abort());
  // A fixed date and time (Friday the 24th, 9:41), so clocks and calendars read the same every run.
  if (page.clock) await page.clock.setFixedTime(new Date(2026, 9, 23, 9, 41));
  await page.goto(`http://127.0.0.1:${port}/?${q}`);
  await page.waitForTimeout(big ? 3000 : 2600);
  const file = path.join(out, `${name.replace(/[^a-z0-9-]+/gi, '_')}.png`);
  await page.screenshot({ path: file });
  const missing = await page.evaluate(() => [...window.harness.missing]);
  console.log(`${problems.length ? 'FAIL' : 'ok  '} ${name.padEnd(14)} ${path.relative(process.cwd(), file)}${missing.length ? `  (no fixture: ${missing.join(', ')})` : ''}`);
  for (const p of problems) console.log(`       ${p}`);
  if (problems.length) failed = true;
  await page.close();
}
await browser.close();
server.close();
process.exit(failed ? 1 : 0);
