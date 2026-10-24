// Procedural landscape wallpapers drawn on canvas. The same scene renders as a 320×200
// carousel thumbnail or at full monitor resolution for the real desktop.

const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

function rng(seed) {
  let a = seed >>> 0 || 1;
  return () => {
    a = a + 0x6D2B79F5 | 0;
    let t = Math.imul(a ^ a >>> 15, 1 | a);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}
function hash(i, s) {
  let x = Math.imul(i, 374761393) + Math.imul(s, 668265263) | 0;
  x = Math.imul(x ^ (x >>> 13), 1274126177);
  x ^= x >>> 16;
  return (x >>> 0) / 4294967296;
}
function vnoise(x, s) { const i = Math.floor(x), f = x - i, u = f * f * (3 - 2 * f); return hash(i, s) * (1 - u) + hash(i + 1, s) * u; }
function terrain(x, o) {
  let a = 1, fr = o.f || 2, sum = 0, norm = 0;
  const oct = o.oct || 5, rough = o.rough || .5;
  for (let k = 0; k < oct; k++) {
    let n = vnoise(x * fr + k * 7.31, (o.seed || 1) + k * 131);
    if (o.ridged) { n = 1 - Math.abs(n * 2 - 1); n *= n; }
    sum += n * a; norm += a; a *= rough; fr *= 2.07;
  }
  return sum / norm;
}
function grad(ctx, y0, y1, stops) {
  const g = ctx.createLinearGradient(0, y0, 0, y1);
  stops.forEach((s, i) => Array.isArray(s) ? g.addColorStop(s[0], s[1]) : g.addColorStop(stops.length === 1 ? 0 : i / (stops.length - 1), s));
  return g;
}

let grainCanvas = null;
const OPS = {
  sky(ctx, w, h, u, o) {
    ctx.fillStyle = o.stops[o.stops.length - 1][1]; ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = grad(ctx, 0, h * o.h, o.stops); ctx.fillRect(0, 0, w, h * o.h);
  },
  stars(ctx, w, h, u, o) {
    const r = rng(o.seed), n = Math.round(o.count * clamp((w / h) / (16 / 9), .5, 2));
    for (let i = 0; i < n; i++) {
      const x = r() * w, y = Math.pow(r(), 1.5) * h * o.maxY, big = r() > .93;
      const s = Math.max(.5, (big ? 1.5 + r() * 1.1 : .7 + r() * .8) * u);
      ctx.fillStyle = `rgba(255,255,255,${((.2 + r() * .75) * (o.a || 1)).toFixed(3)})`;
      if (big) { ctx.beginPath(); ctx.arc(x, y, s, 0, 7); ctx.fill(); } else ctx.fillRect(x, y, s, s);
    }
  },
  sun(ctx, w, h, u, o) {
    const cx = o.x * w, cy = o.y * h, m = Math.max(w, h);
    const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, o.glowR * m);
    g.addColorStop(0, `rgba(${o.glow},${o.a})`); g.addColorStop(.16, `rgba(${o.glow},${o.a * .42})`); g.addColorStop(1, `rgba(${o.glow},0)`);
    ctx.fillStyle = g; ctx.fillRect(0, 0, w, h);
    if (o.r) {
      const R = o.r * Math.min(w, h);
      ctx.save(); ctx.shadowColor = o.core; ctx.shadowBlur = R * 1.4; ctx.fillStyle = o.core;
      ctx.beginPath(); ctx.arc(cx, cy, R, 0, 7); ctx.fill(); ctx.restore();
    }
  },
  haze(ctx, w, h, u, o) {
    const y = o.y * h, d = o.hgt * h, g = ctx.createLinearGradient(0, y - d, 0, y + d);
    g.addColorStop(0, `rgba(${o.c},0)`); g.addColorStop(.5, `rgba(${o.c},${o.a})`); g.addColorStop(1, `rgba(${o.c},0)`);
    ctx.fillStyle = g; ctx.fillRect(0, y - d, w, 2 * d);
  },
  aurora(ctx, w, h, u, o) {
    const off = document.createElement('canvas'); off.width = w; off.height = h;
    const c = off.getContext('2d'); c.globalCompositeOperation = 'lighter';
    const step = Math.max(1.5, w / 900);
    for (let x = 0; x < w; x += step) {
      const nx = x / w;
      const base = o.y * h + Math.sin(nx * 5.5 + 1.2) * h * .05 + (terrain(nx, { f: 2.5, seed: o.seed, oct: 3 }) - .5) * h * .14;
      let k = terrain(nx * 1.3, { f: 4, seed: o.seed + 9, oct: 3 }); k = clamp((k - .32) * 2.4, 0, 1); k *= k;
      if (k < .02) continue;
      const top = base - o.hgt * h * (.45 + .7 * terrain(nx, { f: 3, seed: o.seed + 5, oct: 2 }));
      const g = c.createLinearGradient(0, top, 0, base + 4 * u);
      g.addColorStop(0, `rgba(${o.c2},0)`); g.addColorStop(.5, `rgba(${o.c2},${(o.a * k * .45).toFixed(3)})`);
      g.addColorStop(.88, `rgba(${o.c1},${(o.a * k).toFixed(3)})`); g.addColorStop(1, `rgba(${o.c1},0)`);
      c.fillStyle = g; c.fillRect(x, top, step + .8, base - top + 4 * u);
    }
    ctx.save(); ctx.globalCompositeOperation = 'lighter';
    ctx.filter = `blur(${Math.max(1, 3 * u)}px)`; ctx.drawImage(off, 0, 0);
    ctx.filter = 'none'; ctx.globalAlpha = .45; ctx.drawImage(off, 0, 0);
    ctx.restore();
  },
  ridge(ctx, w, h, u, o) {
    const step = Math.max(1, w / 800), pts = [];
    let minY = 1e9;
    for (let x = 0; x <= w + step; x += step) {
      const nx = x / w, t = terrain(nx, o), v = o.ridged ? t * 1.6 : t;
      const y = o.y * h - o.amp * h * v + (o.tilt || 0) * nx * h;
      pts.push(x, y); if (y < minY) minY = y;
    }
    ctx.beginPath(); ctx.moveTo(0, h + 2);
    for (let i = 0; i < pts.length; i += 2) ctx.lineTo(pts[i], pts[i + 1]);
    ctx.lineTo(w, h + 2); ctx.closePath();
    ctx.fillStyle = grad(ctx, minY, Math.max(minY + 1, o.y * h), o.fill); ctx.fill();
    if (o.rim) {
      ctx.save(); ctx.beginPath();
      for (let i = 0; i < pts.length; i += 2) i ? ctx.lineTo(pts[i], pts[i + 1]) : ctx.moveTo(pts[i], pts[i + 1]);
      ctx.strokeStyle = `rgba(${o.rim},${o.rimA})`; ctx.lineWidth = Math.max(.8, 1.6 * u);
      ctx.shadowColor = `rgba(${o.rim},${o.rimA})`; ctx.shadowBlur = 8 * u; ctx.stroke(); ctx.restore();
    }
    if (o.pines) pines(ctx, w, h, u, o, pts, step);
    if (o.fog) {
      const top = o.y * h - o.amp * h * .4, g = ctx.createLinearGradient(0, top, 0, o.y * h + h * .05);
      g.addColorStop(0, `rgba(${o.fog},0)`); g.addColorStop(1, `rgba(${o.fog},${o.fogA})`);
      ctx.fillStyle = g; ctx.fillRect(0, top, w, h - top);
    }
  },
  lake(ctx, w, h, u, o) {
    const ly = Math.round(o.y * h), H = h - ly, src = Math.max(0, ly - H), sh = ly - src;
    if (sh <= 0) return;
    const tmp = document.createElement('canvas'); tmp.width = w; tmp.height = sh;
    tmp.getContext('2d').drawImage(ctx.canvas, 0, src, w, sh, 0, 0, w, sh);
    ctx.save(); ctx.translate(0, ly + sh); ctx.scale(1, -1);
    ctx.filter = `blur(${Math.max(.5, 1.4 * u)}px)`; ctx.drawImage(tmp, 0, 0); ctx.restore();
    const g = ctx.createLinearGradient(0, ly, 0, h);
    g.addColorStop(0, `rgba(${o.tint},${o.a * .55})`); g.addColorStop(1, `rgba(${o.tint},${Math.min(1, o.a + .35)})`);
    ctx.fillStyle = g; ctx.fillRect(0, ly, w, H);
    ctx.fillStyle = 'rgba(0,0,0,.07)';
    for (let y = ly + 2 * u; y < h; y += (3 + (y - ly) / H * 6) * u) ctx.fillRect(0, y, w, Math.max(.5, .7 * u));
    const r = rng(o.seed || 9); ctx.save(); ctx.globalCompositeOperation = 'lighter';
    for (let k = 0; k < 240; k++) {
      const d = Math.pow(r(), 1.3), y = ly + d * H, spread = w * (.04 + d * .22);
      const x = o.sx * w + (r() - .5) * 2 * spread * r(), len = w * (.006 + r() * .03) * (1 - d * .5);
      ctx.fillStyle = `rgba(${o.shimmer},${((.05 + r() * .22) * (1 - d)).toFixed(3)})`;
      ctx.fillRect(x - len / 2, y, len, Math.max(.6, 1.1 * u));
    }
    ctx.restore();
    ctx.fillStyle = 'rgba(255,255,255,.08)'; ctx.fillRect(0, ly - u * .5, w, Math.max(1, u));
  },
  /** A black disc with a white-hot rim, its corona leaking off in fine bent rays (Taken light). */
  eclipse(ctx, w, h, u, o) {
    const m = Math.min(w, h), cx = o.x * w, cy = o.y * h, R = o.r * m, r = rng(o.seed || 7);
    ctx.save(); ctx.globalCompositeOperation = 'lighter';
    const g = ctx.createRadialGradient(cx, cy, R * .9, cx, cy, R * 4.4);
    g.addColorStop(0, `rgba(${o.glow},${o.a})`); g.addColorStop(.16, `rgba(${o.glow},${o.a * .34})`); g.addColorStop(1, `rgba(${o.glow},0)`);
    ctx.fillStyle = g; ctx.fillRect(0, 0, w, h);
    for (let i = 0; i < o.rays; i++) {
      const a = r() * Math.PI * 2, len = R * (.2 + Math.pow(r(), 2.4) * 2.6), bend = (r() - .5) * .4;
      const x0 = cx + Math.cos(a) * R * .98, y0 = cy + Math.sin(a) * R * .98, x1 = cx + Math.cos(a + bend) * (R + len), y1 = cy + Math.sin(a + bend) * (R + len);
      const lg = ctx.createLinearGradient(x0, y0, x1, y1);
      lg.addColorStop(0, `rgba(242,251,255,${(.3 + r() * .45).toFixed(3)})`); lg.addColorStop(.35, `rgba(${o.glow},${(.1 + r() * .2).toFixed(3)})`); lg.addColorStop(1, `rgba(${o.glow},0)`);
      ctx.strokeStyle = lg; ctx.lineWidth = Math.max(.6, (.5 + r() * 2.2) * u);
      ctx.beginPath(); ctx.moveTo(x0, y0);
      ctx.quadraticCurveTo(cx + Math.cos(a + bend / 2) * (R + len * .5), cy + Math.sin(a + bend / 2) * (R + len * .5), x1, y1); ctx.stroke();
    }
    ctx.restore();
    ctx.save(); ctx.shadowColor = `rgba(${o.glow},.9)`; ctx.shadowBlur = R * .3; ctx.fillStyle = '#000';
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, 7); ctx.fill(); ctx.restore();
    ctx.save(); ctx.strokeStyle = 'rgba(248,252,255,.88)'; ctx.lineWidth = Math.max(1, 1.8 * u); ctx.shadowColor = `rgba(${o.glow},1)`; ctx.shadowBlur = 16 * u;
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, 7); ctx.stroke(); ctx.restore();
  },
  /** Dark crystal shards hanging in the void, each edged with cold light. */
  shards(ctx, w, h, u, o) {
    const r = rng(o.seed || 3), m = Math.min(w, h);
    for (let i = 0; i < o.count; i++) {
      const x = (o.x0 + r() * (o.x1 - o.x0)) * w, y = (o.y0 + r() * (o.y1 - o.y0)) * h, s = (o.min + Math.pow(r(), 2) * (o.max - o.min)) * m;
      const pts = [[0, -1.6], [.55, -.2], [.3, 1], [-.35, .8], [-.6, -.3]].map(([a, b]) => [a * (.75 + r() * .5) * s, b * (.75 + r() * .5) * s]);
      ctx.save(); ctx.translate(x, y); ctx.rotate((r() - .5) * 1.2);
      ctx.beginPath(); pts.forEach(([a, b], k) => (k ? ctx.lineTo(a, b) : ctx.moveTo(a, b))); ctx.closePath();
      ctx.fillStyle = o.fill; ctx.fill();
      ctx.strokeStyle = `rgba(${o.rim},${(o.rimA * (.5 + r() * .5)).toFixed(3)})`; ctx.lineWidth = Math.max(.7, 1.2 * u);
      ctx.shadowColor = `rgba(${o.rim},.85)`; ctx.shadowBlur = 7 * u; ctx.stroke();
      ctx.restore();
    }
  },
  vignette(ctx, w, h, u, o) {
    const g = ctx.createRadialGradient(w / 2, h * .45, Math.min(w, h) * .3, w / 2, h * .45, Math.hypot(w, h) * .62);
    g.addColorStop(0, 'rgba(0,0,0,0)'); g.addColorStop(1, `rgba(0,0,0,${o.a})`);
    ctx.fillStyle = g; ctx.fillRect(0, 0, w, h);
  },
  grain(ctx, w, h, u, o) {
    if (!grainCanvas) {
      grainCanvas = document.createElement('canvas'); grainCanvas.width = grainCanvas.height = 160;
      const g = grainCanvas.getContext('2d'), id = g.createImageData(160, 160);
      for (let i = 0; i < id.data.length; i += 4) { const v = Math.random() * 255 | 0; id.data[i] = id.data[i + 1] = id.data[i + 2] = v; id.data[i + 3] = 255; }
      g.putImageData(id, 0, 0);
    }
    ctx.save(); ctx.globalAlpha = o.a; ctx.globalCompositeOperation = 'overlay';
    ctx.fillStyle = ctx.createPattern(grainCanvas, 'repeat'); ctx.fillRect(0, 0, w, h); ctx.restore();
  }
};

function pines(ctx, w, h, u, o, pts, step) {
  const r = rng((o.seed || 1) * 7 + 3), n = pts.length / 2;
  const top = Array.isArray(o.fill[0]) ? o.fill[0][1] : o.fill[0];
  ctx.fillStyle = o.pineColor || top;
  let x = r() * w * .01;
  while (x < w) {
    const i = Math.min(n - 1, Math.round(x / step)), y = pts[i * 2 + 1], th = h * (o.pineH || .05) * (.5 + r() * .9);
    if (y < h + th * .2) {
      const bw = th * .2;
      for (let k = 0; k < 4; k++) {
        const ty = y - th + k * th * .22, half = bw * (.5 + k * .32);
        ctx.beginPath(); ctx.moveTo(x, ty); ctx.lineTo(x - half, ty + th * .36); ctx.lineTo(x + half, ty + th * .36); ctx.closePath(); ctx.fill();
      }
      ctx.fillRect(x - th * .025, y - th * .12, th * .05, th * .2);
    }
    x += w * (.003 + r() * .011) * (o.pineGap || 1) + th * .15;
  }
}

export function drawWall(cv, wall) {
  const ctx = cv.getContext('2d'), w = cv.width, h = cv.height, u = h / 900;
  ctx.save();
  for (const o of wall.ops) OPS[o.t](ctx, w, h, u, o);
  ctx.restore();
}

export const WALLS = [
  { name: 'Golden Hour', accent: '#E8B04A', ops: [
    { t: 'sky', h: .76, stops: [[0, '#3F4C78'], [.38, '#8E7A97'], [.64, '#D9A07F'], [.85, '#F2C08A'], [1, '#FADFA8']] },
    { t: 'sun', x: .7, y: .5, r: .03, core: '#FFF7E2', glow: '255,206,140', glowR: .55, a: .9 },
    { t: 'haze', y: .6, hgt: .1, c: '255,214,160', a: .35 },
    { t: 'ridge', y: .63, amp: .16, f: 2.4, seed: 11, oct: 6, ridged: 1, fill: ['#A08AA0', '#C49A8E'], fog: '244,196,148', fogA: .55 },
    { t: 'ridge', y: .69, amp: .12, f: 3.2, seed: 23, ridged: 1, fill: ['#7C6886', '#9C7A86'], fog: '238,180,132', fogA: .5 },
    { t: 'ridge', y: .745, amp: .06, f: 4.5, seed: 37, fill: ['#4E4362', '#5A4A68'] },
    { t: 'lake', y: .745, tint: '30,24,46', a: .32, shimmer: '255,228,170', sx: .7, seed: 5 },
    { t: 'ridge', y: .95, amp: .1, f: 1.6, seed: 51, oct: 4, tilt: .16, fill: ['#1C1824', '#121016'], pines: 1, pineH: .07 },
    { t: 'vignette', a: .35 }, { t: 'grain', a: .05 }
  ] },
  { name: 'Ember Ridge', accent: '#FF9466', ops: [
    { t: 'sky', h: .7, stops: [[0, '#120C2A'], [.3, '#3A1A4E'], [.58, '#9A3A64'], [.82, '#E8705E'], [1, '#F7A86A']] },
    { t: 'stars', count: 160, maxY: .32, seed: 5, a: .6 },
    { t: 'sun', x: .36, y: .5, r: .055, core: '#FFD9A8', glow: '255,140,100', glowR: .6, a: .8 },
    { t: 'ridge', y: .6, amp: .18, f: 2.1, seed: 101, ridged: 1, fill: ['#8A3E70', '#A84A6C'], fog: '250,130,110', fogA: .45 },
    { t: 'ridge', y: .67, amp: .17, f: 2.8, seed: 113, ridged: 1, fill: ['#62305E', '#7A3866'], fog: '230,110,110', fogA: .4 },
    { t: 'ridge', y: .75, amp: .15, f: 3.3, seed: 127, ridged: 1, fill: ['#401E48', '#4E2552'], fog: '200,90,110', fogA: .35 },
    { t: 'ridge', y: .85, amp: .14, f: 2.6, seed: 139, ridged: 1, fill: ['#271434', '#2F183C'], fog: '150,60,100', fogA: .3 },
    { t: 'ridge', y: 1.0, amp: .16, f: 2.0, seed: 151, fill: ['#130A1C', '#0C0612'] },
    { t: 'vignette', a: .4 }, { t: 'grain', a: .05 }
  ] },
  { name: 'Glacier Lake', accent: '#8CC8FF', ops: [
    { t: 'sky', h: .62, stops: [[0, '#5D84B8'], [.45, '#9DB7D6'], [.8, '#D6E1EC'], [1, '#EEF2F5']] },
    { t: 'sun', x: .3, y: .25, r: 0, core: '#fff', glow: '255,255,255', glowR: .5, a: .35 },
    { t: 'ridge', y: .58, amp: .3, f: 2.0, seed: 201, oct: 6, ridged: 1, fill: [[0, '#FFFFFF'], [.3, '#E3EAF3'], [.55, '#9FB2CA'], [1, '#7C93B0']], fog: '226,234,243', fogA: .6 },
    { t: 'ridge', y: .63, amp: .16, f: 3.0, seed: 211, ridged: 1, fill: [[0, '#DCE5EF'], [.4, '#8EA3BE'], [1, '#5D7594']], fog: '210,222,236', fogA: .5 },
    { t: 'ridge', y: .655, amp: .04, f: 6, seed: 223, fill: ['#3E5672', '#2E435C'], pines: 1, pineH: .025, pineGap: .7 },
    { t: 'lake', y: .655, tint: '22,52,86', a: .3, shimmer: '255,255,255', sx: .3, seed: 7 },
    { t: 'ridge', y: 1.02, amp: .12, f: 1.4, seed: 231, oct: 4, tilt: -.2, fill: ['#1D2B3B', '#121B26'], pines: 1, pineH: .06 },
    { t: 'vignette', a: .25 }, { t: 'grain', a: .04 }
  ] },
  { name: 'Pine Night', accent: '#7FE0C2', ops: [
    { t: 'sky', h: .8, stops: [[0, '#03070F'], [.4, '#081A26'], [.75, '#10394A'], [1, '#1A5160']] },
    { t: 'stars', count: 700, maxY: .75, seed: 301 },
    { t: 'sun', x: .78, y: .2, r: .022, core: '#EAF7F2', glow: '150,230,210', glowR: .35, a: .35 },
    { t: 'haze', y: .66, hgt: .12, c: '90,190,170', a: .18 },
    { t: 'ridge', y: .66, amp: .2, f: 2.2, seed: 311, ridged: 1, fill: ['#12384A', '#0F2F3F'], fog: '70,160,150', fogA: .25 },
    { t: 'ridge', y: .75, amp: .12, f: 2.4, seed: 323, fill: ['#0B2533', '#09202C'], pines: 1, pineH: .04, fog: '60,140,140', fogA: .2 },
    { t: 'ridge', y: .9, amp: .12, f: 1.8, seed: 337, fill: ['#06141D', '#050F16'], pines: 1, pineH: .075, pineGap: 1.2 },
    { t: 'vignette', a: .45 }, { t: 'grain', a: .06 }
  ] },
  { name: 'Aurora', accent: '#B9A4FF', ops: [
    { t: 'sky', h: .78, stops: [[0, '#03040C'], [.45, '#0A0F2A'], [.8, '#18204A'], [1, '#253064']] },
    { t: 'stars', count: 600, maxY: .7, seed: 401 },
    { t: 'aurora', y: .46, hgt: .34, seed: 411, c1: '86,240,170', c2: '150,110,255', a: .55 },
    { t: 'ridge', y: .68, amp: .2, f: 2.6, seed: 421, ridged: 1, fill: [[0, '#5A6690'], [.35, '#2E3760'], [1, '#1A2142']], fog: '120,140,255', fogA: .2 },
    { t: 'ridge', y: .74, amp: .08, f: 3.5, seed: 433, fill: ['#141A36', '#10152C'], pines: 1, pineH: .03 },
    { t: 'lake', y: .74, tint: '8,10,30', a: .35, shimmer: '140,240,200', sx: .5, seed: 3 },
    { t: 'ridge', y: 1.0, amp: .1, f: 1.5, seed: 441, oct: 4, tilt: .2, fill: ['#080A18', '#05060F'], pines: 1, pineH: .06 },
    { t: 'vignette', a: .4 }, { t: 'grain', a: .05 }
  ] },
  { name: 'Rose Dunes', accent: '#FF9DB4', ops: [
    { t: 'sky', h: .62, stops: [[0, '#C98AA6'], [.4, '#EDB4B8'], [.75, '#F8D2BF'], [1, '#FCE6CB']] },
    { t: 'sun', x: .5, y: .44, r: .07, core: '#FFF6EA', glow: '255,236,214', glowR: .45, a: .7 },
    { t: 'haze', y: .6, hgt: .1, c: '255,230,210', a: .4 },
    { t: 'ridge', y: .62, amp: .07, f: 1.4, seed: 501, oct: 3, rough: .35, fill: ['#F0B6A6', '#E09A92'], rim: '255,236,224', rimA: .55 },
    { t: 'ridge', y: .72, amp: .1, f: 1.2, seed: 511, oct: 3, rough: .35, fill: ['#E7A08F', '#C27274'], rim: '255,226,210', rimA: .5 },
    { t: 'ridge', y: .84, amp: .12, f: 1.0, seed: 523, oct: 3, rough: .3, fill: ['#D88675', '#9C4F5E'], rim: '255,216,196', rimA: .45 },
    { t: 'ridge', y: .98, amp: .12, f: .9, seed: 537, oct: 3, rough: .3, fill: ['#B2615F', '#6A2B40'], rim: '255,200,180', rimA: .35 },
    { t: 'vignette', a: .3 }, { t: 'grain', a: .07 }
  ] },
  // Taken: a black eclipse leaking cold light over rim-lit peaks, shards hanging in the dark.
  { name: 'Ascendant', accent: '#C9E9FF', ops: [
    { t: 'sky', h: 1, stops: [[0, '#010204'], [.5, '#04060c'], [.8, '#070b16'], [1, '#0b1222']] },
    { t: 'stars', count: 420, maxY: .9, seed: 601, a: .55 },
    { t: 'eclipse', x: .62, y: .33, r: .11, glow: '190,230,255', a: .55, rays: 280, seed: 611 },
    { t: 'shards', count: 16, x0: .04, x1: .96, y0: .14, y1: .6, min: .005, max: .028, fill: '#020306', rim: '200,235,255', rimA: .75, seed: 621 },
    { t: 'haze', y: .7, hgt: .14, c: '150,200,255', a: .12 },
    { t: 'ridge', y: .74, amp: .2, f: 2.3, seed: 631, ridged: 1, fill: ['#05070c', '#020306'], rim: '200,235,255', rimA: .55 },
    { t: 'ridge', y: .86, amp: .12, f: 3.1, seed: 641, ridged: 1, fill: ['#030407', '#010203'], rim: '190,230,255', rimA: .35 },
    { t: 'ridge', y: 1.0, amp: .1, f: 1.7, seed: 651, oct: 4, fill: ['#010203', '#000000'] },
    { t: 'vignette', a: .5 }, { t: 'grain', a: .05 }
  ] }
];
