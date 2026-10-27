// The Command Center's Music sheet: what's playing, full size. A big cover that glows with the
// music, the song's year and genre, a progress bar you can drag, the player's controls, and the
// synced lyrics (click a line to jump there). It reads `core` (music.js: the song, where it is,
// the lyrics already looked up) and asks the backend for the full-size cover from Apple's catalog.
// Moved from the island's cc-music.js.

const fmt = s => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

export function createSheet(core) {
  let el = null, raf = 0, offLevel = null;
  let metaKey = '', lyKey = '', lyIdx = -2, followPause = 0, lastSec = -1, lvl = 0;
  let info = null, infoFor = '', cover = '', scrub = null, volAt = 0, volT = 0;
  const ctx = () => core.ctx;
  const ic = n => ctx().ui.icon(n), esc = s => ctx().ui.esc(s);
  const flash = msg => ctx().island.flash(msg);
  const $ = s => el && el.querySelector(s);

  function build() {
    el.innerHTML = `<div class="mus-bg" aria-hidden="true"></div>
      <div class="mus-left">
        <div class="mus-art"><div class="mus-glow" aria-hidden="true"></div><div class="mus-ph">${ic('note')}</div><img class="mus-cover" alt="" draggable="false" hidden></div>
        <div class="mus-meta"><div class="mus-src"></div><h2 class="mus-title"></h2><p class="mus-artist"></p><p class="mus-sub"></p></div>
        <div class="mus-seek"><span class="mus-pos">0:00</span><div class="mus-bar" role="slider" tabindex="0" aria-label="Position in the song" aria-valuemin="0"><i class="mus-fill"></i><i class="mus-knob"></i></div><span class="mus-rem"></span></div>
        <div class="mus-ctl">
          <button class="mus-b" type="button" data-act="shuffle" aria-label="Shuffle">${ic('shuffle')}</button>
          <button class="mus-b big" type="button" data-act="prev" aria-label="Previous">${ic('prev')}</button>
          <button class="mus-b play" type="button" data-act="toggle" aria-label="Play">${ic('play')}</button>
          <button class="mus-b big" type="button" data-act="next" aria-label="Next">${ic('next')}</button>
          <button class="mus-b" type="button" data-act="repeat" aria-label="Repeat">${ic('repeat')}</button>
        </div>
        <div class="mus-row">
          <button class="mus-b sm" type="button" data-act="favorite" aria-label="Favorite" hidden>${ic('star')}</button>
          <button class="mus-b sm" type="button" data-mu="link" title="Copy the song's Apple Music link" aria-label="Copy the song's Apple Music link" hidden>${ic('link')}</button>
          <label class="mus-vol" hidden title="This player's own volume">${ic('speaker')}<input type="range" min="0" max="100" aria-label="Player volume"></label>
          <span class="grow"></span>
          <button class="cc-btn" type="button" data-mu="lyrics"></button>
          <button class="cc-btn" type="button" data-mu="open"></button>
        </div>
        <div class="mus-sources"></div>
      </div>
      <div class="mus-right"><div class="mus-ly" aria-label="Lyrics"></div><small class="mus-lyfoot"></small></div>
      <div class="mus-empty" hidden><div class="mus-ph">${ic('note')}</div><b>Nothing is playing</b><span>Play something in Apple Music, Spotify, Amazon Music, YouTube Music or any player and it shows up here.</span></div>`;
    metaKey = lyKey = ''; lyIdx = -2; lastSec = -1;
  }

  /* ---------- the song's details ---------- */
  function wantInfo(m) {
    const k = m ? `${m.title}\n${m.artist}\n${m.album || ''}` : '';
    if (k === infoFor) return;
    infoFor = k; info = null; cover = '';
    if (!m || !m.title) return;
    ctx().call('info', { title: m.title, artist: m.artist || '', album: m.album || '' }).then(r => {
      if (infoFor !== k) return;
      info = r; metaKey = '';
      // The full-size cover replaces the player's small one once it has loaded.
      if (r && r.cover) { const img = new Image(); img.onload = () => { if (infoFor === k) { cover = r.cover; metaKey = ''; } }; img.src = r.cover; }
    }).catch(() => {});
  }
  function renderMeta(m) {
    el.classList.toggle('idle', !m);
    $('.mus-empty').hidden = !!m;
    if (!m) return;
    const art = cover || core.art() || '';
    const img = $('.mus-cover');
    if (art && img.getAttribute('src') !== art) img.src = art;
    img.hidden = !art; $('.mus-art .mus-ph').hidden = !!art;
    $('.mus-bg').style.backgroundImage = $('.mus-glow').style.backgroundImage = art ? `url("${art}")` : 'none';
    const rgb = core.rgb();
    el.style.setProperty('--mt', rgb ? rgb.map(v => Math.round(v)).join(',') : '190,230,255');
    $('.mus-src').innerHTML = `<span class="mus-dot${m.playing ? ' on' : ''}"></span>${esc(m.app)}${m.badge ? `<b class="mus-badge">${esc(m.badge)}</b>` : ''}`;
    $('.mus-title').textContent = m.title || 'Unknown title';
    $('.mus-artist').textContent = [m.artist, m.album || (info && info.album)].filter(Boolean).join(' — ');
    $('.mus-sub').textContent = info ? [info.year, info.genre, info.track && info.tracks ? `Track ${info.track} of ${info.tracks}` : ''].filter(Boolean).join(' · ') : '';
    const set = (act, show, on, label) => { const b = $(`[data-act="${act}"]`); b.style.visibility = show ? '' : 'hidden'; b.setAttribute('aria-pressed', !!on); b.title = label; b.setAttribute('aria-label', label); };
    set('shuffle', m.shuffle != null, m.shuffle, m.shuffle ? 'Shuffle is on' : 'Shuffle');
    set('repeat', m.repeat != null, m.repeat && m.repeat !== 'off', m.repeat === 'one' ? 'Repeating this song' : m.repeat === 'all' ? 'Repeating all' : 'Repeat');
    $('[data-act="repeat"]').innerHTML = ic('repeat') + (m.repeat === 'one' ? '<b class="rep1">1</b>' : '');
    const play = $('[data-act="toggle"]'); play.innerHTML = ic(m.playing ? 'pause' : 'play'); play.setAttribute('aria-label', m.playing ? 'Pause' : 'Play');
    $('[data-act="next"]').disabled = !m.can_next; $('[data-act="prev"]').disabled = !m.can_prev;
    const fav = $('[data-act="favorite"]'); fav.hidden = m.favorite == null; fav.setAttribute('aria-pressed', !!m.favorite); fav.title = m.favorite ? 'Favorited' : 'Favorite';
    const v = core.volume(), vw = $('.mus-vol'), vol = vw.querySelector('input');
    vw.hidden = v == null;
    if (v != null && performance.now() - volAt > 1500) { vol.value = Math.round(v * 100); vol.style.setProperty('--p', vol.value + '%'); }
    const on = core.lyricsOn();
    $('[data-mu="lyrics"]').innerHTML = `${ic('text')}${on ? 'Lyrics on' : 'Lyrics off'}`;
    $('[data-mu="lyrics"]').setAttribute('aria-pressed', !!on);
    $('[data-mu="link"]').hidden = !(info && info.link);
    $('[data-mu="open"]').innerHTML = `${ic('ext')}Open ${esc(m.app)}`;
    const ss = m.sessions || [];
    $('.mus-sources').innerHTML = ss.length > 1 || m.pinned
      ? `<button type="button" class="chip" data-src="" aria-pressed="${!m.pinned}" title="Show whatever is playing">Auto</button>` + ss.map(x => `<button type="button" class="chip${x.playing ? ' live' : ''}" data-src="${esc(x.id)}" aria-pressed="${!!(m.pinned && m.id === x.id)}" title="${esc(x.title || x.app)}">${esc(x.app)}</button>`).join('')
      : '';
    $('.mus-bar').classList.toggle('off', !(m.can_seek && m.duration > 0));
  }

  /* ---------- lyrics ---------- */
  function renderLyrics() {
    const box = $('.mus-ly'), ly = core.lyrics(), d = ly.data, st = ly.state;
    let html = '', msg = '', foot = '';
    if (!core.lyricsOn()) msg = 'Lyrics are off.';
    else if (st === 'loading') msg = 'Looking for lyrics…';
    else if (st === 'error') msg = 'Couldn’t reach the lyrics service. Trying again in a bit.';
    else if (!d) msg = st === 'none' ? 'No lyrics found for this song.' : '';
    else if (d.lines.length) { html = d.lines.map((l, i) => `<button type="button" class="mus-line${l.text ? '' : ' gap'}" data-i="${i}">${l.text ? esc(l.text) : '♪'}</button>`).join(''); foot = 'Lyrics from LRCLIB'; }
    else if (d.plain) { html = d.plain.split('\n').map(t => `<p class="mus-line">${esc(t) || '&nbsp;'}</p>`).join(''); foot = 'Not time-synced · LRCLIB'; }
    else msg = 'Instrumental ♪';
    box.classList.toggle('msg', !html); box.classList.toggle('plain', !!(d && !d.lines.length && d.plain));
    box.innerHTML = html ? `<div class="mus-pad"></div>${html}<div class="mus-pad"></div>` : `<p class="mus-msg">${esc(msg)}</p>`;
    $('.mus-lyfoot').textContent = foot;
    box.scrollTop = 0; lyIdx = -2;
  }
  function syncLyrics(pos) {
    const d = core.lyrics().data; if (!d || !d.lines.length) return;
    const idx = core.lineAt(pos);
    if (idx === lyIdx) return;
    const first = lyIdx === -2; lyIdx = idx;
    const box = $('.mus-ly'), els = box.querySelectorAll('.mus-line');
    els.forEach((e, i) => { e.classList.toggle('on', i === idx); e.classList.toggle('past', i < idx); });
    const cur = els[Math.max(idx, 0)];
    if (cur && performance.now() > followPause) box.scrollTo({ top: cur.offsetTop - box.clientHeight / 2 + cur.offsetHeight / 2, behavior: first ? 'auto' : 'smooth' });
  }

  /* ---------- every frame while it shows ---------- */
  function frame() {
    raf = 0; if (!el) return;
    const m = core.media(), ly = core.lyrics();
    wantInfo(m);
    const k = m ? [m.key, m.playing, m.shuffle, m.repeat, m.favorite, m.badge, core.volume(), m.can_next, m.can_prev, m.can_seek, m.pinned, (m.sessions || []).map(x => x.id + x.playing).join(), core.art(), cover, info ? 1 : 0, core.lyricsOn(), (core.rgb() || []).join()].join('|') : 'none';
    if (k !== metaKey) { metaKey = k; renderMeta(m); }
    const lk = [ly.key, ly.state, ly.data ? 1 : 0, core.lyricsOn(), !!m].join('|');
    if (lk !== lyKey) { lyKey = lk; renderLyrics(); }
    if (m) {
      const dur = m.duration || 0, pos = scrub != null ? scrub * dur : core.curPos();
      const pct = dur > 0 ? clamp(pos / dur, 0, 1) : 0;
      el.style.setProperty('--pp', (pct * 100).toFixed(3) + '%');
      const sec = Math.floor(pos);
      if (sec !== lastSec) { lastSec = sec; $('.mus-pos').textContent = fmt(pos); $('.mus-rem').textContent = dur > 0 ? '-' + fmt(dur - sec) : ''; const bar = $('.mus-bar'); bar.setAttribute('aria-valuemax', Math.round(dur)); bar.setAttribute('aria-valuenow', sec); bar.setAttribute('aria-valuetext', `${fmt(pos)} of ${fmt(dur)}`); }
      if (scrub == null) syncLyrics(pos);
    }
    lvl *= 0.9;
    el.style.setProperty('--lvl', lvl.toFixed(3));
    raf = requestAnimationFrame(frame);
  }

  /* ---------- input ---------- */
  function onClick(e) {
    const t = e.target;
    const a = t.closest('[data-act]');
    if (a) { if (!a.disabled) core.act(a.dataset.act).catch(err => flash(String(err && err.message || err))); return; }
    const line = t.closest('.mus-line[data-i]');
    const m = core.media(), d = core.lyrics().data;
    if (line && d && m && m.can_seek) { const l = d.lines[+line.dataset.i]; if (l) { core.seekTo(l.t).catch(() => {}); lyIdx = -2; followPause = 0; } return; }
    const b = t.closest('[data-mu]');
    if (b) {
      const k = b.dataset.mu;
      if (k === 'lyrics') core.toggleLyrics();
      else if (k === 'link' && info && info.link) navigator.clipboard.writeText(info.link).then(() => flash('Song link copied')).catch(err => flash(String(err)));
      else if (k === 'open') ctx().island.closeCommandCenter(() => core.act('open').catch(() => {}));
      return;
    }
    const src = t.closest('[data-src]');
    if (src) core.setSource(src.dataset.src).catch(err => flash(String(err && err.message || err)));
  }
  function fracAt(x) { const r = $('.mus-bar').getBoundingClientRect(); return clamp((x - r.left) / r.width, 0, 1); }
  function onPointerDown(e) {
    const bar = e.target.closest('.mus-bar'); const m = core.media();
    if (!bar || e.button !== 0 || !m || !m.can_seek || !(m.duration > 0)) return;
    bar.setPointerCapture(e.pointerId); el.classList.add('scrubbing'); scrub = fracAt(e.clientX);
    const move = ev => { scrub = fracAt(ev.clientX); };
    const up = ev => { bar.removeEventListener('pointermove', move); bar.removeEventListener('pointerup', up); bar.removeEventListener('pointercancel', up); el.classList.remove('scrubbing'); const f = fracAt(ev.clientX); scrub = null; core.seekTo(f * m.duration).catch(() => {}); lyIdx = -2; };
    bar.addEventListener('pointermove', move); bar.addEventListener('pointerup', up); bar.addEventListener('pointercancel', up);
  }
  function onInput(e) {
    const r = e.target.closest('.mus-vol input'); if (!r) return;
    r.style.setProperty('--p', r.value + '%'); volAt = performance.now();
    clearTimeout(volT); volT = setTimeout(() => core.setVolume(r.value / 100).catch(err => flash(String(err && err.message || err))), 40);
  }
  /** Keys while the sheet shows; true when it used the key. */
  function key(e) {
    if (!el || (e.target.matches && e.target.matches('input, textarea, select'))) return false;
    const m = core.media();
    if (e.key === ' ' && m) { el.querySelector('[data-act="toggle"]').click(); return true; }
    if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && m && m.can_seek) { core.seekTo(core.curPos() + (e.key === 'ArrowRight' ? 10 : -10)).catch(() => {}); lyIdx = -2; return true; }
    if ((e.key === 'n' || e.key === 'N') && m && m.can_next) { el.querySelector('[data-act="next"]').click(); return true; }
    if ((e.key === 'p' || e.key === 'P') && m && m.can_prev) { el.querySelector('[data-act="prev"]').click(); return true; }
    if (e.key === 'l' || e.key === 'L') { core.toggleLyrics(); return true; }
    return false;
  }

  return {
    mount(host) {
      el = document.createElement('div'); el.className = 'mus';
      build(); host.append(el);
      el.addEventListener('click', onClick); el.addEventListener('pointerdown', onPointerDown); el.addEventListener('input', onInput);
      // Scrolling the lyrics yourself pauses the auto-follow for a few seconds.
      const box = $('.mus-ly');
      ['wheel', 'pointerdown'].forEach(ev => box.addEventListener(ev, () => { followPause = performance.now() + 4000; }, { passive: true }));
      core.refreshVolume();
      offLevel = core.onLevel(v => { lvl = Math.max(v, lvl); });
      cancelAnimationFrame(raf); frame(); // draws now, then once a frame
    },
    unmount() { cancelAnimationFrame(raf); raf = 0; if (offLevel) { offLevel(); offLevel = null; } if (el) el.remove(); el = null; scrub = null; },
    key
  };
}
