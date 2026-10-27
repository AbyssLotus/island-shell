// Serves src/ with a mocked Tauri bridge (mock-tauri.js), so the island page and the Command Center
// run in any browser without Windows. `node dev/harness/serve.mjs [port]`, then open
// http://localhost:5174/?tab=power (see README.md for the query options).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import url from 'node:url';

const here = path.dirname(url.fileURLToPath(import.meta.url));
const src = path.resolve(here, '../../src');
const repo = path.resolve(here, '../..');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.woff2': 'font/woff2' };

function file(res, p) {
  fs.readFile(p, (err, data) => {
    if (err) { res.writeHead(404); res.end('not found'); return; }
    res.writeHead(200, { 'Content-Type': TYPES[path.extname(p)] || 'application/octet-stream', 'Cache-Control': 'no-store' });
    res.end(data);
  });
}

/** Only paths inside `root`: no `..` tricks. */
function inside(root, rel) {
  const p = path.resolve(root, '.' + decodeURIComponent(rel));
  return p === root || p.startsWith(root + path.sep) ? p : null;
}

export function start(port = 5174) {
  const server = http.createServer((req, res) => {
    const { pathname } = new URL(req.url, 'http://x');
    if (pathname === '/' || pathname === '/index.html') {
      // The real page, with the mock loaded before app.js.
      const html = fs.readFileSync(path.join(src, 'index.html'), 'utf8')
        .replace('<script type="module" src="app.js"></script>', '<script src="/__harness/mock-tauri.js"></script>\n<script type="module" src="app.js"></script>');
      res.writeHead(200, { 'Content-Type': 'text/html', 'Cache-Control': 'no-store' });
      res.end(html);
      return;
    }
    if (pathname === '/__harness/modules.json') {
      // The repo's modules as mod_list answers them: every modules/<id>/module.json, on, in developer mode.
      const dir = path.join(repo, 'modules'), modules = [];
      for (const id of fs.existsSync(dir) ? fs.readdirSync(dir).sort() : []) {
        const mf = path.join(dir, id, 'module.json');
        if (!fs.existsSync(mf)) continue;
        const manifest = JSON.parse(fs.readFileSync(mf, 'utf8'));
        modules.push({ id: manifest.id, manifest, dir: `modules/${id}`, dev: true, enabled: true, state: 'stopped', pid: null, error: null, crashes: 0 });
      }
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
      res.end(JSON.stringify({ modules, broken: [] }));
      return;
    }
    // /__harness/* is this folder, /__repo/* the repo (module folders: their UI loads from there), the rest src/.
    // Module files come under their version (/__repo/modules/<id>@<version>/…), like mod.localhost.
    const plain = pathname.replace(/^\/__repo\/modules\/([a-z0-9-]+)@[^/]+\//, '/__repo/modules/$1/');
    // Made-up modules in fixtures (demo-01…, scenario manycubes) share an empty UI.
    if (/^\/__repo\/modules\/demo-\d+\/ui\/none\.js$/.test(plain)) { res.writeHead(200, { 'Content-Type': 'text/javascript' }); res.end('export {};\n'); return; }
    const [root, rel] = plain.startsWith('/__harness/') ? [here, plain.slice(10)] : plain.startsWith('/__repo/') ? [repo, plain.slice(7)] : [src, plain];
    const p = inside(root, rel);
    if (!p) { res.writeHead(403); res.end(); return; }
    file(res, p);
  });
  return new Promise(ok => server.listen(port, '127.0.0.1', () => ok(server)));
}

if (process.argv[1] && import.meta.url === url.pathToFileURL(process.argv[1]).href) {
  const port = +process.argv[2] || 5174;
  start(port).then(() => console.log(`Island Shell harness: http://localhost:${port}/`));
}
