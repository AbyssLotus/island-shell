// The island's Music tab: the cover (glowing with the music), the song, the player and its other
// sessions, a progress bar you can drag, the player's buttons (Apple Music adds seek, shuffle,
// repeat and favorite), its own volume, and the synced lyrics (click a line to go there). Moved
// from the island's page; everything comes from `core` (music.js).

const fmt = s => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;

export function createTab(core) {
  let root = null, shown = false, raf = 0, lastSec = -1, lyIdx = -2, followPause = 0, scrubbing = false, scrubVal = 0, volAt = 0, volT = 0, noteT = 0, lyKey = '';
  const ctx = () => core.ctx;
  const $ = s => root.querySelector(s);
  const $$ = s => [...root.querySelectorAll(s)];

  function view() {
    const { icon } = ctx().ui;
    return `<div class="mu">
      <div class="mu-hero">
        <div class="mu-backdrop" aria-hidden="true"></div>
        <div class="mu-art art"></div>
        <div class="mu-side">
          <div class="mu-src">
            <span class="mu-app" id="app"></span>
            <span class="mu-badge" id="badge" hidden></span>
            <span class="grow"></span>
            <div class="mu-sources" id="sources" role="group" aria-label="Player"></div>
          </div>
          <b class="mu-title" id="title"></b>
          <span class="mu-sub"><span id="artist"></span><span class="mu-album" id="album"></span></span>
          <div class="mu-prog">
            <span id="pos">0:00</span>
            <input class="scrub" id="seek" type="range" min="0" max="1000" value="0" step="1" aria-label="Song position">
            <span id="left">-0:00</span>
          </div>
          <div class="mu-ctl">
            <button class="ib tg" type="button" data-act="shuffle" aria-label="Shuffle" aria-pressed="false">${icon('shuffle')}</button>
            <button class="ib" type="button" data-act="prev" aria-label="Previous track">${icon('prev')}</button>
            <button class="ib play" type="button" data-act="toggle" aria-label="Play"></button>
            <button class="ib" type="button" data-act="next" aria-label="Next track">${icon('next')}</button>
            <button class="ib tg" type="button" data-act="repeat" aria-label="Repeat" aria-pressed="false">${icon('repeat')}</button>
          </div>
          <div class="mu-row">
            <button class="ib tg fav" type="button" data-act="favorite" aria-label="Favorite" aria-pressed="false">${icon('star')}</button>
            <label class="mu-vol" id="volWrap">${icon('speaker')}<input id="vol" type="range" min="0" max="100" value="100" aria-label="Player volume"></label>
            <span class="grow"></span>
            <button class="chip" type="button" id="away" hidden title="After 10 seconds away from the screen, pause what's playing; play it again when you're back">Pause when I step away</button>
            <button class="chip" type="button" id="lyricsBtn" aria-pressed="true">Lyrics</button>
            <button class="chip" type="button" data-act="open" id="open">Open</button>
          </div>
        </div>
      </div>
      <div class="lyrics" id="lyrics">
        <div class="ly-lines" id="lines"></div>
        <p class="ly-note" id="lyNote"></p>
      </div>
      <p class="note" id="note" hidden></p>
    </div>`;
  }

  const flashNote = msg => { ctx().ui.note($('#note'), msg); clearTimeout(noteT); noteT = setTimeout(() => ctx().ui.note($('#note'), ''), 4500); };
  const errText = e => (e && e.message) || String(e);

  function render(what) {
    if (!root) return;
    const m = core.media(), { txt, setHtml, icon, esc, grad } = ctx().ui;
    $('.mu').classList.toggle('idle', !m);
    $('.mu').classList.toggle('playing', !!(m && m.playing));
    const art = core.art(), bg = art ? `url("${art}") center / cover no-repeat` : m ? grad(m.title + m.artist) : 'var(--ink-3)';
    for (const el of [$('.mu-art'), $('.mu-backdrop')]) { el.style.setProperty('--art', bg); el.classList.toggle('has-thumb', !!art); }
    const t = core.tint();
    if (t) { $('.mu').style.setProperty('--tint', t.tint); $('.mu').style.setProperty('--on-tint', t.on); }
    else { $('.mu').style.removeProperty('--tint'); $('.mu').style.removeProperty('--on-tint'); }
    txt($('#app'), m ? m.app : 'Nothing playing');
    const badge = $('#badge');
    badge.hidden = !(m && m.badge);
    if (m && m.badge) txt(badge, m.badge);
    txt($('#title'), m ? m.title || 'Unknown title' : 'Nothing playing');
    txt($('#artist'), m ? m.artist || m.app : 'Play something in Apple Music, Spotify, Amazon Music or YouTube Music');
    txt($('#album'), m && m.album ? m.album : '');
    if (what === 'song') { const el = $('#title'); el.classList.remove('new'); void el.offsetWidth; el.classList.add('new'); }
    // Shuffle and repeat keep their place (hidden but still taking space) so play stays centered.
    const tg = (act, show, on, label, keepSpace) => $$(`[data-act="${act}"]`).forEach(b => {
      if (keepSpace) b.style.visibility = show ? '' : 'hidden'; else b.hidden = !show;
      b.setAttribute('aria-pressed', !!on); b.setAttribute('aria-label', label); b.title = label;
    });
    tg('shuffle', !!m && m.shuffle != null, m && m.shuffle, m && m.shuffle ? 'Shuffle is on' : 'Shuffle', true);
    const rep = m ? m.repeat : null;
    tg('repeat', rep != null, rep && rep !== 'off', rep === 'one' ? 'Repeating this song' : rep === 'all' ? 'Repeating all' : 'Repeat', true);
    setHtml($('[data-act="repeat"]'), icon('repeat') + (rep === 'one' ? '<b class="rep1">1</b>' : ''));
    tg('favorite', !!m && m.favorite != null, m && m.favorite, m && m.favorite ? 'Favorited' : 'Favorite', false);
    const p = !!(m && m.playing), play = $('[data-act="toggle"]');
    setHtml(play, icon(p ? 'pause' : 'play')); play.setAttribute('aria-label', p ? 'Pause' : 'Play'); play.disabled = !m;
    $('[data-act="next"]').disabled = !(m && m.can_next); $('[data-act="prev"]').disabled = !(m && m.can_prev);
    const v = core.volume(), vw = $('#volWrap'), vol = $('#vol');
    vw.hidden = v == null;
    if (v != null && performance.now() - volAt > 1500) { vol.value = Math.round(v * 100); vol.style.setProperty('--p', vol.value + '%'); }
    vw.title = m ? `${m.app} volume (just this app)` : '';
    const open = $('#open'); open.hidden = !m; if (m) txt(open, `Open ${m.app}`);
    $('#seek').disabled = !(m && m.can_seek && m.duration > 0);
    const ss = (m && m.sessions) || [];
    setHtml($('#sources'), ss.length > 1 || (m && m.pinned)
      ? `<button type="button" class="chip" data-src="" aria-pressed="${!(m && m.pinned)}" title="Show whatever is playing">Auto</button>` +
        ss.map(x => `<button type="button" class="chip${x.playing ? ' live' : ''}" data-src="${esc(x.id)}" aria-pressed="${!!(m && m.pinned && m.id === x.id)}" title="${esc(x.title || x.app)}">${esc(x.app)}</button>`).join('')
      : '');
    $('#lines').classList.toggle('no-seek', !(m && m.can_seek));
    const s = core.settings(), away = $('#away');
    away.hidden = !s.tracker; away.setAttribute('aria-pressed', !!s.pause_when_away);
    renderLyrics();
    lastSec = -1; paint();
    if (shown) ctx().island.layout();
  }

  function renderLyrics() {
    const ly = core.lyrics(), m = core.media(), on = core.lyricsOn();
    const key = [ly.key, ly.state, !!ly.data, on, !!m].join('|');
    if (key === lyKey) return;
    lyKey = key;
    const box = $('#lyrics'), lines = $('#lines'), d = ly.data, st = ly.state, { esc } = ctx().ui;
    box.hidden = !on || !m;
    $('#lyricsBtn').setAttribute('aria-pressed', on);
    let html = '', msg = '', foot = '';
    if (st === 'loading') msg = 'Looking for lyrics…';
    else if (st === 'error') msg = 'Couldn’t reach the lyrics service. Trying again in a bit.';
    else if (st === 'none' || !d) msg = st === 'idle' ? '' : 'No lyrics found for this song.';
    else if (d.lines.length) {
      html = d.lines.map((l, i) => `<button type="button" class="ly-line${l.text ? '' : ' gap'}" data-i="${i}">${l.text ? esc(l.text) : '♪'}</button>`).join('');
      foot = 'Lyrics from LRCLIB';
    } else if (d.plain) {
      html = d.plain.split('\n').map(t => `<p class="ly-line">${esc(t) || '&nbsp;'}</p>`).join('');
      foot = 'Not time-synced · LRCLIB';
    } else msg = 'Instrumental ♪';
    box.classList.toggle('msg', !html);
    lines.classList.toggle('plain', !!(d && !d.lines.length && d.plain));
    lines.innerHTML = html || esc(msg);
    $('#lyNote').textContent = foot;
    lines.scrollTop = 0; lyIdx = -2;
    syncLyric(core.curPos(), true);
  }

  /** Highlights the line being sung and keeps it in the middle of the box. */
  function syncLyric(pos, instant) {
    const d = core.lyrics().data, timed = !!(d && d.lines.length);
    const idx = timed ? core.lineAt(pos) : -1;
    if (idx === lyIdx) return;
    const first = lyIdx === -2;
    lyIdx = idx;
    if (!timed) return;
    const lines = $('#lines'), els = lines.children;
    for (let i = 0; i < els.length; i++) { els[i].classList.toggle('on', i === idx); els[i].classList.toggle('past', i < idx); }
    const el = els[Math.max(idx, 0)];
    if (el && shown && performance.now() > followPause) lines.scrollTo({ top: el.offsetTop - lines.clientHeight / 2 + el.offsetHeight / 2, behavior: instant || first || reduced ? 'auto' : 'smooth' });
  }

  function paint() {
    const m = core.media(), dur = m ? m.duration : 0;
    const pos = scrubbing ? scrubVal : core.curPos();
    const pct = dur > 0 ? clamp(pos / dur * 100, 0, 100) : 0;
    const seek = $('#seek');
    if (!scrubbing) seek.value = Math.round(pct * 10);
    seek.style.setProperty('--p', pct.toFixed(2) + '%');
    const sec = Math.floor(pos);
    if (sec !== lastSec) {
      lastSec = sec;
      $('#pos').textContent = fmt(pos);
      $('#left').textContent = dur > 0 ? '-' + fmt(dur - Math.floor(pos)) : '';
    }
    if (!scrubbing) syncLyric(pos);
  }

  /** Every frame while the tab shows and the song moves. */
  function tick() {
    raf = 0;
    if (!shown || !root) return;
    paint();
    const m = core.media();
    if (m && (m.playing || scrubbing)) raf = requestAnimationFrame(tick);
  }
  const wake = () => { if (!raf && shown) raf = requestAnimationFrame(tick); };

  function mount(r) {
    root = r;
    root.innerHTML = view();
    root.addEventListener('click', e => {
      const b = e.target.closest('[data-act]');
      if (b && !b.disabled) {
        const a = b.dataset.act;
        core.act(a).then(() => { if (a === 'open') ctx().island.closePanel(true); }).catch(err => flashNote(errText(err)));
        wake();
        return;
      }
      const src = e.target.closest('[data-src]');
      if (src) { core.setSource(src.dataset.src).catch(err => flashNote(errText(err))); return; }
      const line = e.target.closest('.ly-line[data-i]');
      if (line) { const d = core.lyrics().data, l = d && d.lines[+line.dataset.i]; if (l) { lyIdx = -2; followPause = 0; core.seekTo(l.t).catch(err => flashNote(errText(err))); wake(); } return; }
      if (e.target.closest('#lyricsBtn')) { core.toggleLyrics(); return; }
      if (e.target.closest('#away')) { core.setPause(!core.settings().pause_when_away).catch(err => flashNote(errText(err))); }
    });
    const seek = $('#seek');
    seek.addEventListener('input', () => {
      const m = core.media(); if (!m || !(m.duration > 0)) return;
      scrubbing = true; scrubVal = seek.value / 1000 * m.duration; lastSec = -1;
      paint(); wake();
    });
    seek.addEventListener('change', () => {
      const m = core.media(); scrubbing = false;
      if (m && m.duration > 0) { lyIdx = -2; followPause = 0; core.seekTo(seek.value / 1000 * m.duration).catch(err => flashNote(errText(err))); wake(); }
    });
    ['pointerup', 'blur'].forEach(ev => seek.addEventListener(ev, () => { setTimeout(() => { scrubbing = false; }, 0); }));
    $('#vol').addEventListener('input', e => {
      const el = e.target; el.style.setProperty('--p', el.value + '%'); volAt = performance.now();
      clearTimeout(volT); volT = setTimeout(() => core.setVolume(el.value / 100).catch(err => flashNote(errText(err))), 40);
    });
    // Scrolling the lyrics yourself pauses the auto-follow for a few seconds.
    ['wheel', 'pointerdown'].forEach(ev => $('#lines').addEventListener(ev, () => { followPause = performance.now() + 4000; }, { passive: true }));
    core.on(what => { render(what); wake(); });
    core.onLevel(v => { if (shown && root) root.querySelector('.mu-art').style.setProperty('--lvl', v.toFixed(3)); });
    render('song');
  }

  return {
    mount,
    show() {
      shown = true; core.refreshVolume(); render(); wake();
      // Once the panel has laid the tab out, bring the sung line to the middle.
      setTimeout(() => { if (shown) { lyIdx = -2; syncLyric(core.curPos(), true); } }, 80);
    },
    hide() { shown = false; cancelAnimationFrame(raf); raf = 0; }
  };
}
