// Destiny 2's island tab: your Guardians and the selected one's loadout, Xûr, and the daily and
// weekly resets, through the module's backend (src/main.rs) with your own Bungie API key.
// Its Command Center cube docks DIM (from the manifest's sites), which needs no key.

let ctx, root, T = 0;
const st = { status: null, profile: null, xur: null, sel: '', edit: false };
const $ = s => root.querySelector(s), $$ = s => [...root.querySelectorAll(s)];

/** Daily reset is 17:00 UTC; the weekly one is Tuesday's; Xûr arrives at Friday's. */
function nextReset(day) {
  const now = new Date(), d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 17));
  if (d <= now) d.setUTCDate(d.getUTCDate() + 1);
  if (day != null) while (d.getUTCDay() !== day) d.setUTCDate(d.getUTCDate() + 1);
  return d;
}
const until = d => { const m = Math.max(0, Math.round((d - Date.now()) / 60000)); const days = Math.floor(m / 1440), h = Math.floor(m % 1440 / 60); return days ? `${days}d ${h}h` : h ? `${h}h ${m % 60}m` : `${m}m`; };
/** Playing right now: an activity, started in the last 12 hours. */
const live = c => !!c.activity && Date.now() - Date.parse(c.activity_since) < 12 * 3600e3;

function view() {
  const { icon } = ctx.ui;
  return `
  <div id="setup" hidden>
    <div class="card k-setup">
      <div class="k-head">
        <span class="k-logo">${icon('orbit')}</span>
        <div class="k-title"><b>Connect Destiny 2</b><small>Your Guardians, their loadouts, Xûr and the resets</small></div>
        <button class="btn" type="button" id="back" hidden>Cancel</button>
      </div>
      <ol class="k-steps">
        <li>Your API key is on <button class="k-link" type="button" id="portal">bungie.net/en/Application</button>: open the app you made and copy its <b>API Key</b>.</li>
        <li>Your <b>Bungie Name</b> is the name with a number, like <code>Guardian#1234</code> (shown in Destiny's roster and on bungie.net).</li>
      </ol>
      <form class="k-form" id="form" autocomplete="off">
        <input class="field" id="key" type="password" placeholder="API key" spellcheck="false" aria-label="Bungie API key">
        <input class="field" id="who" placeholder="Bungie Name (Guardian#1234)" spellcheck="false" aria-label="Bungie Name">
        <button class="btn primary" type="submit" id="save">Connect</button>
      </form>
      <div class="k-fine"><span>The key is stored encrypted for your Windows account only. Everything shown is public, so there's no Bungie sign-in.</span><span class="grow"></span><button class="k-link" type="button" id="forget" hidden>Forget</button></div>
    </div>
  </div>
  <div id="main" hidden>
    <div class="v-head">
      <b id="title"></b><small id="rank"></small><span class="grow"></span>
      <span class="d2-reset" id="daily"></span><span class="d2-reset" id="weekly"></span>
      <button class="ib" type="button" id="editBtn" title="Destiny settings" aria-label="Destiny settings">${icon('gear')}</button>
    </div>
    <div class="d2-grid">
      <div class="d2-side">
        <div class="d2-chars" id="chars" role="group" aria-label="Characters"></div>
        <div class="card d2-xur" id="xur"></div>
      </div>
      <div class="card d2-load" id="load"></div>
    </div>
  </div>
  <p class="empty" id="wait" hidden>Loading your Guardians…</p>
  <p class="note" id="note" hidden></p>`;
}

const note = msg => ctx.ui.note($('#note'), msg);
const showing = () => ctx.island.showing('destiny');

async function loop() {
  clearTimeout(T);
  if (!showing()) return;
  try { st.status = await ctx.call('status'); } catch (e) { note(e.message || String(e)); return; }
  if (st.status.configured && !st.edit) {
    if (!st.profile) { render(); $('#wait').hidden = false; ctx.island.layout(); }
    const [p, x] = await Promise.allSettled([ctx.call('profile'), ctx.call('xur')]);
    $('#wait').hidden = true;
    if (p.status === 'fulfilled') st.profile = p.value;
    if (x.status === 'fulfilled') st.xur = x.value;
    const err = p.status === 'rejected' ? p.reason : x.status === 'rejected' ? x.reason : '';
    note(err ? err.message || String(err) : '');
  }
  render();
  T = setTimeout(loop, 60000);
}

function render() {
  const { esc, setHtml, txt, ago } = ctx.ui;
  const s = st.status || {}, setup = !s.configured || st.edit;
  $('#setup').hidden = !setup; $('#main').hidden = setup || !st.profile;
  if (setup) {
    const who = $('#who'); if (!who.value && s.name) who.value = s.name;
    $('#key').placeholder = s.configured ? 'API key (saved; blank keeps it)' : 'API key';
    $('#back').hidden = !s.configured; $('#forget').hidden = !s.configured;
    if (showing()) ctx.island.layout();
    return;
  }
  const p = st.profile; if (!p) return;
  txt($('#title'), p.name.replace(/#\d+$/, ''));
  txt($('#rank'), p.guardian_rank ? `Guardian Rank ${p.guardian_rank}` : '');
  setHtml($('#daily'), `Daily reset <b>${until(nextReset())}</b>`);
  setHtml($('#weekly'), `Weekly <b>${until(nextReset(2))}</b>`);
  if (!p.characters.some(c => c.id === st.sel)) st.sel = (p.characters.find(live) || p.characters[0] || {}).id || '';
  setHtml($('#chars'), p.characters.map(c => `<button class="d2-char" type="button" data-id="${esc(c.id)}" aria-pressed="${c.id === st.sel}" style="background-image:url(${esc(JSON.stringify(c.emblem_bg))})"><span><b>${esc(c.class)}</b><small class="${live(c) ? 'd2-now' : ''}">${esc(live(c) ? '● ' + c.activity : [c.race, ago(c.last_played)].filter(Boolean).join(' · '))}</small></span><span class="d2-light">✦${c.light}</span></button>`).join(''));
  const c = p.characters.find(x => x.id === st.sel);
  const ic = i => `<img class="d2-ic${i.exotic ? ' exotic' : ''}" src="${esc(i.icon)}" alt="" loading="lazy">`;
  if (c) {
    const sub = c.items.find(i => i.slot === 'Subclass'), weapons = c.items.filter(i => ['Kinetic', 'Energy', 'Power'].includes(i.slot)), armor = c.items.filter(i => ['Helmet', 'Arms', 'Chest', 'Legs', 'Class item'].includes(i.slot));
    setHtml($('#load'), `<header><b>${esc(c.class)} loadout</b><small>${Math.round(c.minutes_played / 60).toLocaleString()} hours played</small></header>`
      + (sub ? `<div class="d2-sub"><img src="${esc(sub.icon)}" alt=""><div><b>${esc(sub.name)}</b><small>Subclass</small></div></div>` : '')
      + `<div class="d2-label">Weapons</div><div class="d2-items">${weapons.map(i => `<div class="d2-item" title="${esc(i.name)}">${ic(i)}<div><b>${esc(i.name)}</b><small>${esc(i.slot)} · ${esc(i.kind)}</small></div><em>${i.power || ''}</em></div>`).join('')}</div>`
      + `<div class="d2-label">Armor</div><div class="d2-armor">${armor.map(i => `<div title="${esc(i.name)} (${esc(i.slot)})">${ic(i)}<em>${i.power || ''}</em></div>`).join('')}</div>`);
  }
  const x = st.xur;
  if (!x) setHtml($('#xur'), '');
  else if (x.here) {
    // Bungie's own refresh date when it gives one (Xûr now sells all week); otherwise the weekly reset.
    const refresh = Date.parse(x.leaves) > Date.now() ? new Date(x.leaves) : nextReset(2);
    setHtml($('#xur'), `<header><b>Xûr${x.location ? ' · ' + esc(x.location) : ''}</b><small>new stock in ${until(refresh)}</small></header>`
      + (x.items.length ? `<div class="d2-armor">${x.items.map(i => `<div title="${esc(i.name)} (${esc(i.kind)})"><img class="d2-ic exotic" src="${esc(i.icon)}" alt="" loading="lazy"></div>`).join('')}</div>` : '<small class="d2-label">Exotics not listed</small>'));
  } else setHtml($('#xur'), `<header><b>Xûr</b><small>arrives in ${until(nextReset(5))}</small></header>`);
  if (showing()) ctx.island.layout();
}

function mount(r) {
  root = r;
  root.innerHTML = view();
  $('#chars').addEventListener('click', e => {
    const b = e.target.closest('.d2-char'); if (!b) return;
    st.sel = b.dataset.id; ctx.store.set('sel', st.sel); render();
  });
  $('#portal').addEventListener('click', () => ctx.openWeb('https://www.bungie.net/en/Application').catch(err => note(String(err))));
  $('#form').addEventListener('submit', e => {
    e.preventDefault();
    const key = $('#key').value.trim(), name = $('#who').value.trim();
    if (!name) { note('Add your Bungie Name, like Guardian#1234.'); return; }
    note(''); $('#save').disabled = true;
    ctx.call('configure', { key, name }).then(s => {
      $('#key').value = ''; st.status = s; st.edit = false; st.profile = null; loop();
    }).catch(err => note(err.message || String(err))).finally(() => { $('#save').disabled = false; });
  });
  $('#editBtn').addEventListener('click', () => { st.edit = true; render(); });
  $('#back').addEventListener('click', () => { st.edit = false; $('#key').value = ''; render(); });
  $('#forget').addEventListener('click', () => {
    // A second click within 3 s confirms.
    const b = $('#forget');
    if (!b.dataset.sure) { b.dataset.sure = 1; b.textContent = 'Click again to forget'; setTimeout(() => { delete b.dataset.sure; b.textContent = 'Forget'; }, 3000); return; }
    delete b.dataset.sure; b.textContent = 'Forget';
    ctx.call('forget').then(() => { st.edit = false; st.profile = null; st.xur = null; $('#who').value = ''; loop(); });
  });
}

export function activate(c) {
  ctx = c;
  // The selected Guardian, kept from 0.1.x's island.d2Sel the first time.
  let legacy = '';
  try { legacy = JSON.parse(localStorage.getItem('island.d2Sel') || '""'); } catch (e) {}
  st.sel = ctx.store.get('sel', legacy);
  ctx.tabs.register({ id: 'destiny', mount, show: loop, hide: () => clearTimeout(T) });
  ctx.cubes.register({ id: 'destiny', stat: () => `Reset in ${until(nextReset(2))}` });
}

export function deactivate() {
  clearTimeout(T);
}
