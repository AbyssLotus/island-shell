// Shared pieces of the Task Manager (cc-taskmgr.js and its pages): number formats, and the SVG
// it draws with (line graphs, sparklines, LED meters, dials).

export const N = 60; // seconds of history in the graphs
export const GB = 1073741824, MB = 1048576, KB = 1024;
export const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

export const fmtMem = b => (b >= GB ? (b / GB).toFixed(1) + ' GB' : b >= MB ? (b / MB).toFixed(b >= 100 * MB ? 0 : 1) + ' MB' : Math.round(b / KB) + ' KB');
export const fmtBytes = b => (b >= 1e12 ? (b / 1e12).toFixed(2) + ' TB' : b >= 1e9 ? (b / 1e9).toFixed(b >= 1e11 ? 0 : 1) + ' GB' : b >= 1e6 ? (b / 1e6).toFixed(b >= 1e8 ? 0 : 1) + ' MB' : b >= 1e3 ? Math.round(b / 1e3) + ' KB' : Math.round(b) + ' B');
export const fmtSize = b => (b >= 1099511627776 ? (b / 1099511627776).toFixed(2) + ' TB' : b >= GB ? (b / GB).toFixed(b >= 100 * GB ? 0 : 1) + ' GB' : b >= MB ? (b / MB).toFixed(b >= 100 * MB ? 0 : 1) + ' MB' : b > 0 ? Math.max(1, Math.round(b / KB)) + ' KB' : '—');
export const fmtRate = b => (b < 1024 ? Math.round(b) + ' B/s' : b < MB ? (b / KB).toFixed(b < 10 * KB ? 1 : 0) + ' KB/s' : b < GB ? (b / MB).toFixed(1) + ' MB/s' : (b / GB).toFixed(2) + ' GB/s');
export const fmtIO = b => (b < .05 * MB ? '0 MB/s' : (b / MB).toFixed(1) + ' MB/s');
export const fmtBits = bytes => { const b = bytes * 8; return b >= 1e9 ? (b / 1e9).toFixed(1) + ' Gbps' : b >= 1e6 ? (b / 1e6).toFixed(1) + ' Mbps' : Math.round(b / 1e3) + ' Kbps'; };
export const fmtLink = bps => (bps >= 1e9 ? (bps / 1e9).toFixed(bps % 1e9 ? 1 : 0) + ' Gbps' : bps > 0 ? Math.round(bps / 1e6) + ' Mbps' : '—');
export const fmtUp = s => { const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60), x = Math.floor(s % 60); return `${d}:${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(x).padStart(2, '0')}`; };
export const fmtMs = ms => { const s = ms / 1000; if (s < 60) return s.toFixed(s < 10 ? 1 : 0) + ' s'; const m = s / 60; if (m < 60) return Math.round(m) + ' min'; const h = m / 60; return h < 100 ? h.toFixed(1) + ' h' : Math.round(h).toLocaleString() + ' h'; };
export const fmtGHz = mhz => (mhz / 1000).toFixed(2) + ' GHz';
export const fmtDate = (ms, withTime) => { if (!ms) return '—'; const d = new Date(ms); return d.toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' }) + (withTime ? ' ' + d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : ''); };
export function ago(ms) {
  if (!ms) return 'never';
  const s = (Date.now() - ms) / 1000;
  if (s < 90) return 'just now';
  if (s < 3600) return Math.round(s / 60) + ' min ago';
  if (s < 86400) return Math.round(s / 3600) + ' h ago';
  const d = Math.round(s / 86400);
  return d === 1 ? 'yesterday' : d < 30 ? d + ' days ago' : fmtDate(ms);
}
export const netKind = n => { const l = n.toLowerCase(); return /wireless|wi-?fi|802\.11|wlan/.test(l) ? 'Wi-Fi' : /bluetooth/.test(l) ? 'Bluetooth' : /tailscale|wintun|tap-|tunnel|radmin|vpn|wireguard|zerotier/.test(l) ? 'VPN' : 'Ethernet'; };

/** Cell tint: Taken blue, turning amber when it's a big share. */
export function heat(v, max) {
  const p = clamp(v / max, 0, 1);
  if (p < .02) return '';
  return p > .6 ? `background:rgba(245,181,107,${(.12 + p * .26).toFixed(3)})` : `background:rgba(190,230,255,${(.04 + p * .3).toFixed(3)})`;
}

/** History buffers: hist[key] is the last N readings. */
export function makeHist() {
  const h = {};
  return {
    h,
    push(k, v) { if (v == null || Number.isNaN(v)) return; const a = h[k] || (h[k] = []); a.push(v); if (a.length > N) a.shift(); },
    clear() { for (const k of Object.keys(h)) delete h[k]; }
  };
}

/* ---------- SVG ---------- */

export function line(arr, max, W, H, pad = 3, min = 0) {
  if (!arr || !arr.length) return '';
  const off = N - arr.length;
  return arr.map((v, i) => `${i ? 'L' : 'M'}${((off + i) / (N - 1) * W).toFixed(1)} ${(H - pad - clamp((v - min) / (max - min), 0, 1) * (H - pad * 2)).toFixed(1)}`).join('');
}
/** A history graph: series = [{ h, max?, min?, cls?, fill? }]; the first one gets an area unless fill: false. */
export function graph(series, max = 100, { W = 600, H = 190, grid = true, cls = '' } = {}) {
  const g = grid ? [.25, .5, .75].map(f => `<line class="gl" x1="0" x2="${W}" y1="${H * f}" y2="${H * f}"/>`).join('') + Array.from({ length: 5 }, (_, i) => `<line class="gl v" y1="0" y2="${H}" x1="${(i + 1) * W / 6}" x2="${(i + 1) * W / 6}"/>`).join('') : '';
  const paths = series.map((s, i) => {
    const d = line(s.h, s.max ?? max, W, H, 3, s.min || 0); if (!d) return '';
    const x0 = ((N - s.h.length) / (N - 1) * W).toFixed(1);
    const area = (s.fill ?? i === 0) ? `<path class="ar ${s.cls || ''}" d="${d}L${W} ${H}L${x0} ${H}Z"/>` : '';
    return `${area}<path class="ln ${s.cls || ''}${s.dash ? ' dash' : ''}" d="${d}" vector-effect="non-scaling-stroke"/>`;
  }).join('');
  return `<svg class="tm-graph ${cls}" viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" aria-hidden="true">${g}${paths}</svg>`;
}
export const spark = (h, max, h2, cls = '') => `<svg class="tm-spark ${cls}" viewBox="0 0 64 34" preserveAspectRatio="none" aria-hidden="true">${h && h.length ? `<path class="ar" d="${line(h, max, 64, 34, 2)}L64 34L${((N - h.length) / (N - 1) * 64).toFixed(1)} 34Z"/><path class="ln" d="${line(h, max, 64, 34, 2)}" vector-effect="non-scaling-stroke"/>` : ''}${h2 && h2.length ? `<path class="ln dash" d="${line(h2, max, 64, 34, 2)}" vector-effect="non-scaling-stroke"/>` : ''}</svg>`;

/** An LED meter: `n` segments, lit up to `frac` (0..1). Vertical by default. */
export function leds(frac, n = 28, { horizontal = false, cls = '' } = {}) {
  const lit = Math.round(clamp(frac, 0, 1) * n);
  let s = '';
  for (let i = 0; i < n; i++) s += `<i${i < lit ? ' class="on"' : ''}></i>`;
  return `<span class="tm-leds ${horizontal ? 'h' : 'v'} ${cls}" style="--n:${n}" aria-hidden="true">${s}</span>`;
}

/** A dial with a needle: value on a 0..max scale over 260°, with ticks and labels. */
export function dial(value, max, { label = '', unit = '', digits = 0, ticks = 5 } = {}) {
  const A0 = -220, A1 = 40, R = 100, cx = 120, cy = 120;
  const ang = v => (A0 + (A1 - A0) * clamp(v / max, 0, 1)) * Math.PI / 180;
  const pt = (a, r) => `${(cx + Math.cos(a) * r).toFixed(1)} ${(cy + Math.sin(a) * r).toFixed(1)}`;
  let t = '';
  const minor = ticks * 5;
  for (let i = 0; i <= minor; i++) {
    const a = ang(max * i / minor), major = i % 5 === 0;
    t += `<line class="${major ? 'mj' : 'mn'}" x1="${pt(a, major ? 82 : 88).split(' ')[0]}" y1="${pt(a, major ? 82 : 88).split(' ')[1]}" x2="${pt(a, 96).split(' ')[0]}" y2="${pt(a, 96).split(' ')[1]}"/>`;
    if (major) { const [x, y] = pt(a, 68).split(' '); const v = max * i / minor; t += `<text x="${x}" y="${(+y + 4).toFixed(1)}">${v >= 1000 ? (v / 1000).toFixed(v % 1000 ? 1 : 0) + 'k' : +v.toFixed(v < 10 && v % 1 ? 1 : 0)}</text>`; }
  }
  const a = ang(value);
  const arcEnd = pt(a, R), arcStart = pt(ang(0), R);
  const large = (a - ang(0)) * 180 / Math.PI > 180 ? 1 : 0;
  const shown = value >= 10000 ? Math.round(value).toLocaleString() : value.toFixed(digits);
  return `<svg class="tm-dial" viewBox="0 0 240 240" aria-label="${label} ${shown} ${unit}">
    <circle class="bezel" cx="${cx}" cy="${cy}" r="114"/><circle class="face" cx="${cx}" cy="${cy}" r="106"/>
    <path class="track" d="M${arcStart} A${R} ${R} 0 1 1 ${pt(ang(max), R)}"/>
    ${value > 0 ? `<path class="fill" d="M${arcStart} A${R} ${R} 0 ${large} 1 ${arcEnd}"/>` : ''}
    <g class="ticks">${t}</g>
    <text class="lbl" x="${cx}" y="${cy - 26}">${label}</text>
    <line class="needle" x1="${pt(a + Math.PI, 14).split(' ')[0]}" y1="${pt(a + Math.PI, 14).split(' ')[1]}" x2="${pt(a, 90).split(' ')[0]}" y2="${pt(a, 90).split(' ')[1]}"/>
    <circle class="hub" cx="${cx}" cy="${cy}" r="9"/>
    <rect class="lcd" x="${cx - 44}" y="${cy + 34}" width="88" height="28" rx="5"/><text class="val" x="${cx}" y="${cy + 55}">${shown}</text>
    <text class="unit" x="${cx}" y="${cy + 82}">${unit}</text></svg>`;
}

/** Key-value pairs; falsy entries are skipped. */
export const kv = (esc, pairs, cls = '') => `<div class="tm-kv ${cls}">${pairs.filter(Boolean).map(([k, v, t]) => `<div${t ? ` title="${esc(t)}"` : ''}><small>${esc(k)}</small><b>${esc(String(v))}</b></div>`).join('')}</div>`;

/** A sortable table header. cols = [[key, label, cls?, tip?]]. */
export const thead = (esc, cols, sort) => `<tr>${cols.map(([k, n, c, tip]) => `<th data-sort="${k}"${c ? ` class="${c}"` : ''}${sort.k === k ? ` aria-sort="${sort.d > 0 ? 'ascending' : 'descending'}"` : ''}${tip ? ` title="${esc(tip)}"` : ''}><span>${esc(n)}</span></th>`).join('')}</tr>`;
export function sorter(sort, get = (x, k) => x[k]) {
  return (a, b) => {
    const x = get(a, sort.k), y = get(b, sort.k);
    if (typeof x === 'string' || typeof y === 'string') return String(x || '').localeCompare(String(y || ''), undefined, { sensitivity: 'base', numeric: true }) * sort.d;
    return ((x || 0) - (y || 0)) * sort.d;
  };
}
