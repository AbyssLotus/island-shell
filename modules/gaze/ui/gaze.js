// Eye tracking: a tile in Controls (on or off, and how the tracker is doing) and the same switch on
// the module's settings page. The backend (src/main.rs) does the tracking and publishes `presence`,
// which the island follows by itself (look at it to peek) as does the Music module.

let ctx, st = null;

function renderTile() {
  if (!st || !st.available) { ctx.tiles.remove('eye'); return; }
  ctx.tiles.set({
    id: 'eye', order: 50, icon: 'eye', title: 'Eye tracking', pressed: st.enabled && st.connected,
    sub: !st.enabled ? 'Off' : !st.connected ? 'Looking for the tracker…' : st.present ? 'On' : 'On · you’re away',
    run: () => ctx.call('set', { enabled: !st.enabled }).then(setStatus)
  });
}
function setStatus(s) { st = s || null; renderTile(); }

export function activate(c) {
  ctx = c;
  ctx.on('status', setStatus);
  ctx.settings.mount(root => {
    root.innerHTML = '<label class="switch"><input type="checkbox"><span>Eye tracking (look at the island to open it)</span></label>';
    const box = root.querySelector('input');
    box.checked = !!(st && st.enabled);
    box.addEventListener('change', () => ctx.call('set', { enabled: box.checked }).then(setStatus).catch(() => { box.checked = !box.checked; }));
  });
  ctx.call('status').then(setStatus).catch(() => {});
}

export function deactivate() {
  ctx.tiles.remove('eye');
}
