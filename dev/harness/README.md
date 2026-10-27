# Dev harness

The island page and the Command Center in an ordinary browser, without Windows. `serve.mjs` serves
`src/` as it is and loads `mock-tauri.js` before `app.js`; the mock answers every `invoke` from
`fixtures/*.json` and lets you send backend events. Everything in the fixtures is made up.

```
node dev/harness/serve.mjs            # http://localhost:5174/
node dev/harness/shoot.mjs            # screenshots of every tab into dev/harness/out/
node dev/harness/shoot.mjs power cc   # only those
```

`shoot.mjs` uses the Playwright that's installed globally (cloud sessions have it), blocks every request
that would leave the machine, pins the clock to Friday the 24th at 9:41, and exits with 1 if the page
logged an error. It also lists commands that had no fixture.

| Query | Does |
| --- | --- |
| `?tab=power` | Opens the panel on that tab. |
| `?cc=1` | Opens the Command Center. |
| `?sheet=games` | Opens a Command Center sheet. |
| `?style=bar` | The top bar instead of the island. |
| `?scenario=update` | Layers `fixtures/update.json` over `fixtures/default.json` (several: `update,playing`). |
| `?cc=1&scenario=manycubes` | The Command Center with 30 made-up modules' cubes, for its layout (more than fit on a small screen go in "+N more"). |
| `?fresh=1` | An empty profile, so the setup guide opens. |

In the browser console, `harness.emit('update:status', {...})` sends a backend event,
`harness.calls` lists every `invoke` so far and `harness.missing` the ones without a fixture.

## Adding fixtures

Unknown commands answer `null`. Add the answer to `fixtures/default.json` (or a scenario file) in
the shape the Rust command returns; answers that depend on arguments or the clock go in `dynamic`
in `mock-tauri.js`. No real names, paths, keys or screenshots of anyone's PC.
