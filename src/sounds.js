// Notification sounds: the island's own, made on the spot with Web Audio (glassy bells, a soft
// eclipse swell, a Taken shimmer…), never Windows' default ding. Each kind of notification gets a
// sound of its own, "None", or a file of your choosing (kept in this page's IndexedDB, so nothing is
// copied around the disk). Settings live in local storage under island.sounds.

export const KINDS = [
  ['mention', 'Discord @mentions', 'Someone pings you'],
  ['urgent', 'Due soon', 'Canvas work due within 3 hours'],
  ['canvas', 'Canvas updates', 'New assignments, grades, announcements'],
  ['timer', 'Focus timer done', 'The end of a focus session'],
  ['attention', 'Apps asking for you', 'An app flashes for your attention'],
  ['voice', 'Calls', 'You join or leave a voice channel'],
  ['small', 'Small confirmations', 'Copied, opening an app, starting a game'],
  ['problem', 'Something didn’t work', 'An action that failed']
];
export const PRESETS = [['glass', 'Glass'], ['chime', 'Chime'], ['pulse', 'Pulse'], ['drop', 'Drop'], ['eclipse', 'Eclipse'], ['taken', 'Taken'], ['ember', 'Ember'], ['tick', 'Tick'], ['low', 'Low'], ['none', 'None'], ['file', 'Your own file…']];
const DEFAULTS = { on: true, vol: .7, fsMute: false, map: { mention: 'glass', urgent: 'chime', canvas: 'pulse', timer: 'eclipse', attention: 'drop', voice: 'none', small: 'none', problem: 'none' }, files: {} };
const MAX_FILE = 5 * 1024 * 1024, MAX_SECONDS = 8;

/* ---------- your own files, in IndexedDB ---------- */
function db() {
  return new Promise((res, rej) => {
    const r = indexedDB.open('island-sounds', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('files');
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
}
async function idb(mode, fn) {
  const d = await db();
  return new Promise((res, rej) => { const tx = d.transaction('files', mode), req = fn(tx.objectStore('files')); tx.oncomplete = () => res(req && req.result); tx.onerror = () => rej(tx.error); });
}

export function createSounds({ store, isFullscreen }) {
  const saved = store.get('sounds', {});
  const cfg = { ...DEFAULTS, ...saved, map: { ...DEFAULTS.map, ...(saved.map || {}) }, files: { ...(saved.files || {}) } };
  const save = () => store.set('sounds', cfg);
  let ctx = null, out = null, verb = null;
  const last = {};
  const decoded = {};

  function audio() {
    if (!ctx) {
      ctx = new AudioContext({ latencyHint: 'interactive' });
      const comp = ctx.createDynamicsCompressor();
      comp.threshold.value = -16; comp.ratio.value = 3; comp.attack.value = .003; comp.release.value = .2;
      out = ctx.createGain(); out.connect(comp).connect(ctx.destination);
      // A small generated room: decaying noise, a little brighter on the right.
      const secs = 2.2, n = Math.floor(ctx.sampleRate * secs), ir = ctx.createBuffer(2, n, ctx.sampleRate);
      for (let ch = 0; ch < 2; ch++) { const d = ir.getChannelData(ch); for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / n, 2.8) * (ch ? 1 : .9); }
      verb = ctx.createConvolver(); verb.buffer = ir;
      const wet = ctx.createGain(); wet.gain.value = .28; verb.connect(wet).connect(out);
    }
    if (ctx.state === 'suspended') ctx.resume();
    out.gain.value = cfg.vol * .9;
    return ctx;
  }

  /* ---------- the building blocks ---------- */
  /** One voice: an oscillator through an envelope, dry plus a share into the room. */
  function tone(t, f, { type = 'sine', a = .006, d = 1, g = .25, wet = .5, to = null, glide = 0, lp = 0 } = {}) {
    const o = ctx.createOscillator(), env = ctx.createGain();
    o.type = type; o.frequency.setValueAtTime(f, t);
    if (to) o.frequency.exponentialRampToValueAtTime(to, t + (glide || d * .6));
    env.gain.setValueAtTime(0, t); env.gain.linearRampToValueAtTime(g, t + a); env.gain.exponentialRampToValueAtTime(.0001, t + a + d);
    let node = o.connect(env);
    if (lp) { const f2 = ctx.createBiquadFilter(); f2.type = 'lowpass'; f2.frequency.value = lp; node = env.connect(f2); }
    node.connect(out);
    if (wet) { const s = ctx.createGain(); s.gain.value = wet; node.connect(s).connect(verb); }
    o.start(t); o.stop(t + a + d + .05);
  }
  /** A bell: a few out-of-tune partials, the high ones dying first. */
  function bell(t, f, g = .2, d = 1.3) {
    [[1, 1], [2.76, .45], [5.4, .22], [8.93, .1]].forEach(([r, k], i) => tone(t, f * r, { g: g * k, d: d / (1 + i * .9), a: .002, wet: .55 }));
  }
  function noise(t, d, { g = .1, bp = 2000, q = 1, to = 0, wet = .4 } = {}) {
    const n = Math.floor(ctx.sampleRate * d), b = ctx.createBuffer(1, n, ctx.sampleRate), x = b.getChannelData(0);
    for (let i = 0; i < n; i++) x[i] = Math.random() * 2 - 1;
    const s = ctx.createBufferSource(); s.buffer = b;
    const f = ctx.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = q; f.frequency.setValueAtTime(bp, t); if (to) f.frequency.exponentialRampToValueAtTime(to, t + d);
    const env = ctx.createGain(); env.gain.setValueAtTime(0, t); env.gain.linearRampToValueAtTime(g, t + d * .3); env.gain.exponentialRampToValueAtTime(.0001, t + d);
    s.connect(f).connect(env).connect(out);
    if (wet) { const w = ctx.createGain(); w.gain.value = wet; env.connect(w).connect(verb); }
    s.start(t); s.stop(t + d + .05);
  }

  /* ---------- the sounds ---------- */
  const SYNTH = {
    glass: t => { bell(t, 1318.5, .2); bell(t + .11, 1975.5, .16); },
    chime: t => { bell(t, 1046.5, .17, 1.1); bell(t + .12, 1318.5, .16, 1.1); bell(t + .24, 1568, .16, 1.3); bell(t + .38, 2093, .08, 1.6); },
    pulse: t => { tone(t, 880, { to: 960, glide: .08, d: .12, g: .38, wet: .25 }); tone(t + .14, 1320, { to: 1420, glide: .08, d: .16, g: .34, wet: .3 }); },
    drop: t => { tone(t, 1500, { to: 420, glide: .12, d: .22, a: .002, g: .62, wet: .45 }); tone(t + .17, 1900, { to: 700, glide: .1, d: .18, a: .002, g: .26, wet: .5 }); },
    eclipse: t => {
      // A slow, dark swell that opens up, with a high glint on top.
      const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.Q.value = 4;
      lp.frequency.setValueAtTime(300, t); lp.frequency.exponentialRampToValueAtTime(2600, t + .55); lp.frequency.exponentialRampToValueAtTime(700, t + 1.9);
      const env = ctx.createGain(); env.gain.setValueAtTime(0, t); env.gain.linearRampToValueAtTime(.075, t + .3); env.gain.exponentialRampToValueAtTime(.0001, t + 2.1);
      lp.connect(env); env.connect(out); const w = ctx.createGain(); w.gain.value = .9; env.connect(w).connect(verb);
      for (const [f, det] of [[220, -7], [220, 7], [329.6, -5], [554.4, 6], [440, 0]]) { const o = ctx.createOscillator(); o.type = 'sawtooth'; o.frequency.value = f; o.detune.value = det; o.connect(lp); o.start(t); o.stop(t + 2.2); }
      bell(t + .22, 1760, .07, 1.6);
    },
    taken: t => {
      // Two voices a semitone apart, beating and sinking, over a falling whisper.
      for (const [f, to] of [[659.25, 587.3], [698.46, 622.3]]) {
        // The wobble works on its own gain stage, so the fade-out still takes it down to nothing.
        const o = ctx.createOscillator(), wob = ctx.createGain(), env = ctx.createGain(), trem = ctx.createOscillator(), tg = ctx.createGain();
        o.type = 'sine'; o.frequency.setValueAtTime(f, t); o.frequency.exponentialRampToValueAtTime(to, t + 1.3);
        wob.gain.value = .7; trem.frequency.value = 7; tg.gain.value = .3; trem.connect(tg).connect(wob.gain);
        env.gain.setValueAtTime(0, t); env.gain.linearRampToValueAtTime(.2, t + .18); env.gain.exponentialRampToValueAtTime(.0001, t + 1.5);
        o.connect(wob).connect(env); env.connect(out); const w = ctx.createGain(); w.gain.value = 1; env.connect(w).connect(verb);
        o.start(t); trem.start(t); o.stop(t + 1.6); trem.stop(t + 1.6);
      }
      noise(t, 1.1, { g: .05, bp: 4200, to: 500, q: 2, wet: .9 });
    },
    ember: t => { tone(t, 392, { type: 'triangle', d: .7, g: .3, lp: 2200, wet: .35 }); tone(t, 784, { d: .45, g: .09, wet: .3 }); tone(t + .1, 523.25, { type: 'triangle', d: .8, g: .28, lp: 2400, wet: .4 }); },
    tick: t => { noise(t, .03, { g: 1.1, bp: 3500, q: 1.5, wet: .05 }); tone(t, 2400, { d: .035, a: .001, g: .2, wet: 0 }); },
    low: t => { tone(t, 392, { type: 'triangle', d: .28, g: .32, lp: 1600, wet: .25 }); tone(t + .16, 311.1, { type: 'triangle', d: .45, g: .32, lp: 1400, wet: .3 }); }
  };

  async function playFile(kind) {
    try {
      let buf = decoded[kind];
      if (!buf) {
        const blob = await idb('readonly', s => s.get(kind));
        if (!blob) return false;
        buf = decoded[kind] = await ctx.decodeAudioData(await blob.arrayBuffer());
      }
      const src = ctx.createBufferSource(), g = ctx.createGain();
      src.buffer = buf; g.gain.value = .9;
      const t = ctx.currentTime, end = t + Math.min(buf.duration, MAX_SECONDS);
      if (buf.duration > MAX_SECONDS) { g.gain.setValueAtTime(.9, end - .4); g.gain.linearRampToValueAtTime(0, end); }
      src.connect(g).connect(out); src.start(t); src.stop(end);
      return true;
    } catch (e) { return false; }
  }

  /** The sound for a kind of notification (respecting the switch, the volume and fullscreen). */
  function play(kind) {
    if (!cfg.on || !cfg.map[kind] || cfg.map[kind] === 'none') return;
    if (cfg.fsMute && isFullscreen && isFullscreen()) return;
    const now = performance.now();
    if (now - (last[kind] || 0) < 350 || now - (last.any || 0) < 120) return; // no pile-ups
    last[kind] = last.any = now;
    sound(cfg.map[kind], kind);
  }
  /** One sound by name, now (the preview button). */
  function sound(name, kind) {
    audio();
    if (name === 'file') { playFile(kind); return; }
    const f = SYNTH[name]; if (f) f(ctx.currentTime + .02);
  }

  async function setFile(kind, file) {
    if (!file) return false;
    if (file.size > MAX_FILE) throw new Error('That file is over 5 MB; pick something shorter');
    audio();
    const buf = await ctx.decodeAudioData(await file.arrayBuffer()).catch(() => null);
    if (!buf) throw new Error('That file isn’t a sound this can play');
    await idb('readwrite', s => s.put(file, kind));
    decoded[kind] = buf;
    cfg.map[kind] = 'file'; cfg.files[kind] = file.name; save();
    return true;
  }

  return {
    play, sound, setFile, KINDS, PRESETS,
    get on() { return cfg.on; },
    get vol() { return cfg.vol; },
    get fsMute() { return cfg.fsMute; },
    choice: kind => cfg.map[kind] || 'none',
    fileName: kind => cfg.files[kind] || '',
    set(patch) { Object.assign(cfg, patch); save(); if (out) out.gain.value = cfg.vol * .9; },
    choose(kind, name) { cfg.map[kind] = name; save(); }
  };
}
