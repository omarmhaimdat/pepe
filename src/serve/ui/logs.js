'use strict';
/* pepe logs --serve: the page. It draws what the stream says and nothing
   else. Each event is one JSON object; a part missing from it is
   unchanged, so a quiet second costs a few bytes and no drawing. Drawing
   happens at most once a frame, never while the tab is hidden, and only
   for the tab in view. Text goes in as text, never as markup. */

const $ = (s, root = document) => root.querySelector(s);
const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined && text !== null) e.textContent = text; return e; };

/* ----- the token: from the link's #token=, kept for the tab ------------- */
const token = (() => {
  const m = location.hash.match(/token=([0-9a-f]+)/);
  try {
    if (m) { sessionStorage.setItem('pepe-logs-token', m[1]); history.replaceState(null, '', location.pathname); return m[1]; }
    return sessionStorage.getItem('pepe-logs-token');
  } catch { return m && m[1]; }
})();
const headers = { Authorization: `Bearer ${token || ''}` };

/* ----- what the page knows ---------------------------------------------- */
const RING = 300;
const S = {
  secs: new Float64Array(RING * 4), head: 0, count: 0,
  h: null, title: '', findings: [], limits: [], statuses: {}, top: [],
  causes: { faults: 0, rows: [] }, allCauses: null, paths: null, slots: {}, held: null,
  log: [], logMax: 2000, passed: 0, lastLine: 0, viewers: 0,
  wire: { bytes: 0, events: 0, since: performance.now(), rate: 0 },
  online: false,
};
const UI = { tab: 'dashboard', grain: 'minute', sort: 'n', heldPath: null, pathq: '', logq: '', logk: 'all', dirty: new Set(['all']) };

/* ----- numbers as pepe writes them --------------------------------------- */
const fmt = {
  n(v) { v = +v || 0; return v >= 1e9 ? (v / 1e9).toFixed(1) + 'B' : v >= 1e6 ? (v / 1e6).toFixed(1) + 'M' : v >= 1e4 ? (v / 1e3).toFixed(1) + 'k' : Math.round(v).toLocaleString('en'); },
  rate(v) { v = +v || 0; return v >= 1000 ? (v / 1000).toFixed(1) + 'k' : v >= 10 ? Math.round(v).toString() : v >= 1 ? v.toFixed(1) : v.toFixed(2); },
  pct(v) { v = +v || 0; return v === 0 ? '0%' : v < .001 ? '<0.1%' : (v * 100).toFixed(v < .1 ? 1 : 0) + '%'; },
  ms(v) { return v == null ? '—' : v >= 1000 ? (v / 1000).toFixed(2) + ' s' : v >= 10 ? Math.round(v) + ' ms' : v >= 1 ? v.toFixed(1) + ' ms' : Math.round(v * 1000) + ' µs'; },
  bytes(v) { v = +v || 0; return v >= 2 ** 30 ? (v / 2 ** 30).toFixed(1) + ' GB' : v >= 2 ** 20 ? (v / 2 ** 20).toFixed(1) + ' MB' : v >= 1024 ? (v / 1024).toFixed(1) + ' KB' : Math.round(v) + ' B'; },
  // Times arrive on the log's own clock: read as they are, never moved to the browser's zone
  clock(t) { const d = new Date(t * 1000); return d.toISOString().slice(11, 19); },
  hm(t) { return new Date(t * 1000).toISOString().slice(11, 16); },
  day(t) { const d = new Date(t * 1000); return ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][d.getUTCDay()] + ' ' + d.toISOString().slice(8, 10); },
  // As the screen says it: "1.8× a usual minute", "12% under a usual minute"
  versus(now, then, what) {
    if (!then) return 'nothing to hold it to yet';
    const r = now / then;
    return r >= 2 ? `${r.toFixed(1)}× ${what}` : r > 1.005 ? `${Math.round((r - 1) * 100)}% over ${what}` : r < .995 ? `${Math.round((1 - r) * 100)}% under ${what}` : `as in ${what}`;
  },
};

/* ----- the stream -------------------------------------------------------- */
async function connect() {
  let wait = 500;
  for (;;) {
    try {
      const res = await fetch('/api/stream', { headers, cache: 'no-store' });
      if (res.status === 401) { gate('This link has no token, or not the one pepe made. Open the link pepe logs --serve printed, with its #token=.'); return; }
      if (!res.ok || !res.body) throw new Error(res.status);
      online(true); wait = 500;
      const reader = res.body.getReader(), dec = new TextDecoder();
      let buf = '';
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        S.wire.bytes += value.byteLength;
        buf += dec.decode(value, { stream: true });
        let cut;
        while ((cut = buf.indexOf('\n\n')) >= 0) {
          const event = buf.slice(0, cut); buf = buf.slice(cut + 2);
          for (const line of event.split('\n')) if (line.startsWith('data: ')) take(JSON.parse(line.slice(6)));
        }
      }
    } catch { /* gone: try again */ }
    online(false);
    await new Promise(r => setTimeout(r, wait)); wait = Math.min(wait * 2, 8000);
  }
}
function online(on) { S.online = on; UI.dirty.add('hero'); schedule(); }
function gate(text) {
  document.body.replaceChildren();
  const g = el('div', 'gate'); const img = el('img'); img.src = '/ui/img/pepe-worried.png'; img.width = 72; img.style.imageRendering = 'pixelated';
  g.append(img, el('p', '', text)); document.body.append(g);
}

function take(f) {
  S.wire.events++;
  if (f.title !== undefined) { S.title = f.title; UI.dirty.add('title'); }
  if (f.head) { S.h = f.head; UI.dirty.add('hero').add('cards').add('causes'); }
  if (f.sec) { for (const s of f.sec) push(s); UI.dirty.add('seconds').add('cards'); }
  if (f.findings) { S.findings = f.findings; UI.dirty.add('findings'); }
  if (f.limits) { S.limits = f.limits; UI.dirty.add('findings'); }
  if (f.statuses) { S.statuses = f.statuses; UI.dirty.add('codes'); }
  if (f.top) { S.top = f.top; UI.dirty.add('top'); }
  if (f.causes) { S.causes = f.causes; UI.dirty.add('causes'); }
  if (f.viewers !== undefined) S.viewers = f.viewers;
  UI.dirty.add('foot');
  schedule();
}
// A second said again (it was still filling) is put right where it is
function push([t, n, c4, c5]) {
  if (S.count) {
    const lastAt = ((S.head + RING - 1) % RING) * 4, last = S.secs[lastAt];
    if (t <= last) {
      const back = last - t; if (back >= S.count) return;
      const i = ((S.head + RING - 1 - back) % RING) * 4;
      if (S.secs[i] === t) { S.secs[i + 1] = n; S.secs[i + 2] = c4; S.secs[i + 3] = c5; }
      return;
    }
    // a gap is seconds with nothing in them
    for (let gap = last + 1; gap < t && gap > t - RING; gap++) put(gap, 0, 0, 0);
  }
  put(t, n, c4, c5);
}
function put(t, n, c4, c5) { const i = S.head * 4; S.secs[i] = t; S.secs[i + 1] = n; S.secs[i + 2] = c4; S.secs[i + 3] = c5; S.head = (S.head + 1) % RING; S.count = Math.min(RING, S.count + 1); }
function seconds() { const out = []; for (let k = 0; k < S.count; k++) { const i = ((S.head - S.count + k + RING) % RING) * 4; out.push([S.secs[i], S.secs[i + 1], S.secs[i + 2], S.secs[i + 3]]); } return out; }

/* ----- what only some tabs show, asked for while they are in view -------- */
const asking = {};
async function ask(key, url, every, into) {
  if (asking[key] && performance.now() - asking[key] < every) return;
  asking[key] = performance.now();
  try {
    const res = await fetch(url, { headers, cache: 'no-store' });
    if (!res.ok) return;
    const text = await res.text(); S.wire.bytes += text.length;
    into(JSON.parse(text)); schedule();
  } catch { /* the stream says when the server is gone */ }
}
function wants() {
  if (document.hidden || !S.online) return;
  if (UI.tab === 'paths' || UI.tab === 'errors') ask('paths', '/api/paths', 950, v => { S.paths = v; UI.dirty.add('paths'); });
  if (UI.tab === 'errors') ask('causes', '/api/causes', 1950, v => { S.allCauses = v; UI.dirty.add('causes'); });
  if (UI.tab === 'traffic') ask('slots:' + UI.grain, '/api/slots?grain=' + UI.grain, UI.grain === 'minute' ? 4950 : 29950, v => { S.slots[UI.grain] = v; UI.dirty.add('slots'); });
  // The lines, only while they are in view: the server reads them from the logs only then
  if (UI.tab === 'log') ask('lines', '/api/lines?' + new URLSearchParams({ after: S.lastLine, kind: UI.logk, q: UI.logq, path: UI.heldPath || '' }), 950, takeLines);
  if (UI.heldPath) ask('held', '/api/path?path=' + encodeURIComponent(UI.heldPath), 950, v => { if (v.path === UI.heldPath) { S.held = v; UI.dirty.add('held'); } });
}
setInterval(wants, 250);

const filterless = () => UI.logk === 'all' && !UI.logq && !UI.heldPath;
// The filters are the server's too: a new one asks again from the start
function refilter() { S.log = []; S.lastLine = 0; S.passed = 0; delete asking.lines; drawLog(true); wants(); }
// The lines after the last one held; a gap in their numbers is lines that went by
function takeLines(v) {
  if (v.last < S.lastLine) { S.log = []; S.lastLine = 0; S.passed = 0; } // a reader started again
  let prev = S.lastLine;
  for (const l of v.log) {
    if (l[0] <= S.lastLine) continue;
    // with no filter, a gap in the numbers is lines that went by; with one, lines it left out
    if (prev && l[0] > prev + 1 && filterless()) S.passed += l[0] - prev - 1;
    S.log.push(l); prev = S.lastLine = l[0];
  }
  if (S.log.length > S.logMax) S.log.splice(0, S.log.length - S.logMax);
  UI.dirty.add('log');
}

/* ----- drawing ----------------------------------------------------------- */
let pending = false;
function schedule() { if (!pending && !document.hidden) { pending = true; requestAnimationFrame(draw); } }
document.addEventListener('visibilitychange', () => { if (!document.hidden) { UI.dirty.add('all'); schedule(); } });
const BY_TAB = { dashboard: ['seconds', 'codes', 'top', 'causes'], traffic: ['slots', 'grain', 'held'], paths: ['paths'], errors: ['causes', 'paths'], log: ['log'] };
const ALWAYS = ['title', 'hero', 'cards', 'findings', 'foot', 'held'];
function draw() {
  pending = false;
  const all = UI.dirty.has('all'), d = all ? new Set([...ALWAYS, ...Object.values(BY_TAB).flat()]) : UI.dirty, v = UI.tab;
  if (d.has('title')) { $('[data-title]').textContent = S.title; document.title = `pepe logs · ${S.title}`; }
  if (d.has('hero')) drawHero();
  if (d.has('cards')) drawCards();
  if (d.has('findings')) drawFindings();
  if (d.has('held')) drawHeld();
  if (v === 'dashboard') { if (d.has('seconds')) drawSeconds(); if (d.has('codes')) drawCodes(); if (d.has('top')) drawTop(); if (d.has('causes')) drawCauses($('[data-causes-short]'), S.causes); }
  if (v === 'traffic' && (d.has('slots') || d.has('grain') || d.has('held'))) drawSlots();
  if (v === 'paths' && d.has('paths')) drawPathTable();
  if (v === 'errors') { if (d.has('causes')) drawCauses($('[data-causes]'), S.allCauses || S.causes); if (d.has('paths')) drawBadPaths(); }
  if (v === 'log' && d.has('log')) drawLog(all);
  if (d.has('foot')) drawFoot();
  // What this tab didn't draw stays owed to the tab that shows it
  const owed = new Set();
  for (const k of d) if (!ALWAYS.includes(k) && !BY_TAB[v].includes(k)) owed.add(k);
  UI.dirty = owed;
}

const FACES = { quiet: 'sleeping', steady: 'happy', busy: 'proud', degraded: 'worried', failing: 'on-fire', ended: 'sleeping', reading: 'waiting', waiting: 'waiting' };
const WORDS = { quiet: 'quiet', steady: 'steady', busy: 'busy', degraded: 'degraded', failing: 'failing', ended: 'done', reading: 'reading', waiting: 'waiting' };
function drawHero() {
  const h = S.h, b = $('[data-badge]');
  b.textContent = !S.online ? 'OFFLINE' : h && h.live ? 'LIVE' : 'READ';
  b.classList.toggle('off', !S.online || !(h && h.live));
  if (!h) return;
  const st = h.state, sel = $('[data-state]');
  sel.textContent = WORDS[st] || st; sel.dataset.state = st;
  $('[data-headline]').classList.toggle('live', !!h.live && S.online);
  const face = $('[data-face]'), src = `/ui/img/pepe-${FACES[st] || 'proud'}.png`;
  if (face.getAttribute('src') !== src) face.setAttribute('src', src);
  $('[data-tag]').textContent = h.live ? `pepe logs · now against a usual minute` : `pepe logs · read to the end`;
  const lede = $('[data-lede]'); lede.replaceChildren();
  const add = (t, bold) => lede.append(bold ? el('b', '', t) : document.createTextNode(t));
  if (h.trouble) { add(h.trouble, true); return; }
  if (st === 'reading') { const [r, all] = h.read; add(`Reading the logs: ${fmt.bytes(r)} of ${fmt.bytes(all)}`, true); add(all ? ` (${Math.round(r / all * 100)}%), ${fmt.n(h.total)} requests so far.` : '.'); return; }
  if (st === 'waiting') { add('Nothing in the logs yet: the page follows them as they are written.'); return; }
  add(`${fmt.rate(h.rate)} req/s`, true); add(h.live ? ' over the last minute, ' : ' at the end, ');
  add(fmt.versus(h.rate, h.usual, 'a usual minute')); add('. ');
  add(`5xx ${fmt.pct(h.s5xx)}`, h.s5xx >= .01);
  if (h.p99 != null) { add(', p99 '); add(fmt.ms(h.p99), true); }
  add(h.echo != null ? `; ${h.echo_label} ${fmt.rate(h.echo)} req/s.` : '.');
}

function card(label, big, sub, bad, spark) {
  const c = el('div', 'card' + (bad ? ' bad' : ''));
  c.append(el('small', '', label), el('b', 'num', big), el('span', 'num', sub));
  if (spark) c.append(spark);
  return c;
}
function sparkline(values, color) {
  const w = 160, h = 22, cv = document.createElement('canvas'); cv.className = 'spark';
  const dpr = devicePixelRatio || 1; cv.width = w * dpr; cv.height = h * dpr;
  const g = cv.getContext('2d'); g.scale(dpr, dpr);
  let max = 1; for (const v of values) if (v > max) max = v;
  const bw = w / Math.max(1, values.length); g.fillStyle = color;
  values.forEach((v, i) => { const bh = v ? Math.max(1, Math.round(v / max * h)) : 0; g.fillRect(Math.floor(i * bw), h - bh, Math.max(1, Math.ceil(bw) - 1), bh); });
  return cv;
}
const css = k => getComputedStyle(document.documentElement).getPropertyValue(k).trim();
function drawCards() {
  const h = S.h; if (!h) return;
  const last = seconds().slice(-60), live = h.live;
  const cards = [card(live ? 'Now' : 'At the end', `${fmt.rate(h.rate)}/s`, fmt.versus(h.rate, h.usual, 'a usual minute'), false, sparkline(last.map(s => s[1]), css('--bar')))];
  if (h.usual != null) cards.push(card('A usual minute', `${fmt.rate(h.usual)}/s`, 'the minutes the logs go back'));
  if (h.busiest != null) cards.push(card('Busiest minute', `${fmt.rate(h.busiest)}/s`, h.busiest_at || ''));
  cards.push(card(live ? 'Failing now' : 'Failing then', `5xx ${fmt.pct(h.s5xx)}`, `4xx ${fmt.pct(h.s4xx)}`, h.s5xx >= .01, sparkline(last.map(s => s[3]), css('--c5'))));
  cards.push(h.p50 != null
    ? card('Request time', `p50 ${fmt.ms(h.p50)}`, `p95 ${fmt.ms(h.p95)} · p99 ${fmt.ms(h.p99)}`, h.p99 >= 1000)
    : card('Request time', '—', 'the log has no $request_time'));
  cards.push(card('In all', fmt.n(h.total), `requests · ${fmt.bytes(h.bytes)}`));
  $('[data-cards]').replaceChildren(...cards);
}

function drawFindings() {
  const box = $('[data-findings]'); box.replaceChildren();
  for (const f of S.findings) {
    const d = el('div', 'finding ' + f.level);
    d.append(el('span', 'lv', f.level), el('span', '', f.text));
    if (f.since) d.append(el('span', 'since', 'since ' + f.since));
    if (f.path) { d.dataset.path = f.path; d.title = 'hold the page to ' + f.path; }
    box.append(d);
  }
  const lim = $('[data-limits]'); lim.replaceChildren();
  for (const l of S.limits) lim.append(el('span', 'limit ' + (l.held ? 'held' : 'crossed'), `${l.condition} · ${l.measured}`));
}

/* ----- the charts: canvas, drawn whole, no DOM per bar -------------------- */
function sizeCanvas(cv) {
  const dpr = devicePixelRatio || 1, w = cv.clientWidth, h = +cv.getAttribute('height');
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); cv.style.height = h + 'px'; }
  const g = cv.getContext('2d'); g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
  return [g, w, h];
}
function niceMax(v) { const p = 10 ** Math.floor(Math.log10(v)); for (const m of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (m * p >= v) return m * p; return 10 * p; }
function bars(cv, rows, o) {
  if (!cv.clientWidth) return;
  const [g, w, h] = sizeCanvas(cv);
  const C = { bar: css('--bar'), c4: css('--c4'), c5: css('--c5'), line: css('--line'), t2: css('--text-2'), red: css('--red-deep'), mono: css('--mono') };
  const L = 46, B = 22, T = 8, pw = w - L - 4, ph = h - B - T;
  let top = o.usual || 0; for (const r of rows) if (r[1] > top) top = r[1];
  const max = niceMax(Math.max(top, .001));
  g.font = '11px ' + C.mono; g.fillStyle = C.t2; g.strokeStyle = C.line; g.lineWidth = 1;
  for (const f of [0, .5, 1]) { const y = Math.round(T + ph - f * ph) + .5; g.setLineDash(f ? [2, 4] : []); g.beginPath(); g.moveTo(L, y); g.lineTo(w - 4, y); g.stroke(); g.fillText(fmt.rate(max * f), 0, y + 4); }
  g.setLineDash([]);
  const n = Math.max(o.slots || rows.length, 1), bw = pw / n, gap = bw > 4 ? 1 : 0, off = n - rows.length;
  rows.forEach((r, i) => {
    const x = L + (off + i) * bw, ww = Math.max(1, bw - gap);
    let y = T + ph;
    for (const [v, col] of [[r[1] - r[2] - r[3], C.bar], [r[2], C.c4], [r[3], C.c5]]) { if (v <= 0) continue; const hh = v / max * ph; g.globalAlpha = r[4] ? .55 : 1; g.fillStyle = col; g.fillRect(x, y - hh, ww, hh); y -= hh; }
  });
  g.globalAlpha = 1;
  if (o.usual) { const y = Math.round(T + ph - o.usual / max * ph) + .5; g.strokeStyle = C.red; g.setLineDash([5, 3]); g.lineWidth = 1.5; g.beginPath(); g.moveTo(L, y); g.lineTo(w - 4, y); g.stroke(); g.setLineDash([]); g.fillStyle = C.red; g.textAlign = 'right'; g.fillText('usual', w - 6, y - 5); g.textAlign = 'left'; }
  g.fillStyle = C.t2;
  if (rows.length) {
    g.fillText(o.label(rows[0][0]), L, h - 6);
    if (rows.length > 6) { const mid = rows[rows.length >> 1]; g.textAlign = 'center'; g.fillText(o.label(mid[0]), L + (off + (rows.length >> 1)) * bw, h - 6); }
    g.textAlign = 'right'; g.fillText(o.nowLabel || o.label(rows[rows.length - 1][0]), w - 4, h - 6); g.textAlign = 'left';
  }
  cv._hit = { L, bw, off, rows, tip: o.tip };
}
function hover(cv, ev) {
  const hit = cv._hit, tip = $('[data-tipbox]'); if (!hit) return;
  const r = cv.getBoundingClientRect(), i = Math.floor((ev.clientX - r.left - hit.L) / hit.bw) - hit.off, row = hit.rows[i];
  if (!row) { tip.style.display = 'none'; return; }
  tip.textContent = hit.tip(row);
  tip.style.display = 'block';
  tip.style.left = Math.min(ev.clientX + 14, innerWidth - tip.offsetWidth - 8) + 'px'; tip.style.top = ev.clientY + 14 + 'px';
}
for (const cv of document.querySelectorAll('canvas[data-chart]')) { cv.addEventListener('mousemove', e => hover(cv, e)); cv.addEventListener('mouseleave', () => { $('[data-tipbox]').style.display = 'none'; }); }
addEventListener('resize', () => { UI.dirty.add('seconds').add('slots').add('held'); schedule(); });

function drawSeconds() {
  bars($('[data-chart="seconds"]'), seconds(), {
    slots: RING, label: fmt.clock, nowLabel: S.h && S.h.live ? 'now' : null, usual: S.h && S.h.usual,
    tip: r => `${fmt.clock(r[0])}  ${fmt.n(r[1])} req · 4xx ${fmt.n(r[2])} · 5xx ${fmt.n(r[3])}`,
  });
}
const GRAIN_LABEL = { minute: fmt.hm, hour: t => fmt.day(t) + ' ' + fmt.hm(t), day: fmt.day };
function drawSlots() {
  const g = UI.grain, s = S.slots[g], rows = s ? s.rows : [];
  $('[data-traffic-sub]').textContent = s ? `${rows.length} ${g}s · requests a second over each, the dashed line a usual ${g}` : 'asking…';
  const rate = v => `${fmt.rate(v)}/s`;
  bars($('[data-chart="slots"]'), rows, { label: GRAIN_LABEL[g], usual: s && s.usual, tip: r => `${GRAIN_LABEL[g](r[0])}${r[4] ? ' (so far)' : ''}  ${rate(r[1])} · 4xx ${rate(r[2])} · 5xx ${rate(r[3])}` });
  const box = $('[data-heldbox]'), held = S.held && UI.heldPath && S.held.known;
  box.hidden = !held;
  if (held) {
    $('[data-heldtitle]').textContent = S.held.path;
    bars($('[data-chart="held"]'), S.held.minutes, { label: fmt.hm, tip: r => `${fmt.hm(r[0])}  ${rate(r[1])} · 4xx ${rate(r[2])} · 5xx ${rate(r[3])}` });
    const leg = $('[data-heldcodes]'); leg.replaceChildren();
    for (const [code, n] of S.held.statuses) { const l = el('div'); const i = el('i'); i.style.background = CODE(code); l.append(i, document.createTextNode(code), el('span', 'num', fmt.n(n))); leg.append(l); }
  }
}

const CODE = c => c >= 500 ? 'var(--c5)' : c >= 400 ? 'var(--c4)' : c >= 300 ? '#7aa7d9' : 'var(--bar)';
function drawCodes() {
  const all = Object.entries(S.statuses).map(([c, n]) => [+c, n]).sort((a, b) => b[1] - a[1]);
  const total = all.reduce((a, [, n]) => a + n, 0) || 1;
  const bar = $('[data-codes]'), leg = $('[data-codes-legend]'); bar.replaceChildren(); leg.replaceChildren();
  for (const [c, n] of all.slice(0, 12)) {
    const d = el('div'); d.style.flexGrow = n; d.style.background = CODE(c); d.title = `${c}: ${fmt.n(n)}`; bar.append(d);
    const l = el('div'); const i = el('i'); i.style.background = CODE(c); l.append(i, document.createTextNode(c), el('span', 'num', `${fmt.n(n)} · ${fmt.pct(n / total)}`)); leg.append(l);
  }
  $('[data-codes-total]').textContent = `${fmt.n(total)} in all`;
}
function pathRow(path, frac, a, b, bad) {
  const r = el('div', 'row'); r.dataset.path = path;
  const name = el('span', 'p', path), rail = el('span', 'rail'), fill = el('span', 'fill' + (bad ? ' bad' : ''));
  fill.style.width = (Math.min(1, frac) * 100) + '%'; name.append(rail, fill);
  r.append(name, el('b', 'num', a), el('em', 'num' + (bad ? ' bad' : ''), b));
  return r;
}
function drawTop() {
  const box = $('[data-toppaths]'); box.replaceChildren();
  const max = Math.max(1e-9, ...S.top.map(p => p[1]));
  if (!S.top.length) box.append(el('div', 'row', 'no request in the last minute'));
  for (const [path, rate, s5] of S.top) box.append(pathRow(path, rate / max, `${fmt.rate(rate)}/s`, fmt.pct(s5), s5 >= .02));
}
// rows: [path, requests, 4xx, 5xx, req/s now, p50, p99, upstream share]
function drawBadPaths() {
  const box = $('[data-badpaths]'); box.replaceChildren();
  if (!S.paths) { box.append(el('div', 'row', 'asking…')); return; }
  const bad = S.paths.rows.filter(p => p[3] > 0).sort((a, b) => b[3] - a[3]).slice(0, 25), max = Math.max(1, ...bad.map(p => p[3]));
  if (!bad.length) box.append(el('div', 'row', 'no path answers 5xx'));
  for (const p of bad) box.append(pathRow(p[0], p[3] / max, fmt.n(p[3]), fmt.pct(p[3] / p[1]), true));
}
const SORT = { path: 0, n: 1, s4xx: r => r[2] / r[1], s5xx: r => r[3] / r[1], rate: 4, p50: 5, p99: 6, up: 7 };
function drawPathTable() {
  const body = $('[data-pathrows]');
  if (!S.paths) { body.replaceChildren(); return; }
  const q = UI.pathq.toLowerCase(), key = SORT[UI.sort], val = typeof key === 'function' ? key : r => r[key];
  const rows = S.paths.rows.filter(p => !q || p[0].toLowerCase().includes(q))
    .sort((a, b) => UI.sort === 'path' ? a[0].localeCompare(b[0]) : (val(b) ?? -1) - (val(a) ?? -1));
  const frag = document.createDocumentFragment();
  for (const p of rows) {
    const tr = el('tr'); tr.dataset.path = p[0];
    const s5 = p[3] / p[1];
    for (const [v, bad] of [[p[0]], [fmt.n(p[1])], [fmt.rate(p[4])], [fmt.pct(s5), s5 >= .02], [fmt.pct(p[2] / p[1])], [fmt.ms(p[5])], [fmt.ms(p[6]), p[6] >= 1000], [p[7] == null ? '—' : fmt.pct(p[7])]]) tr.append(el('td', bad ? 'bad num' : 'num', v));
    frag.append(tr);
  }
  body.replaceChildren(frag);
}
// As the screen says it: 45s, 12m, 3h, 2d
const ago = s => (s = Math.max(0, s)) < 120 ? `${s}s` : s < 7200 ? `${Math.floor(s / 60)}m` : s < 172800 ? `${Math.floor(s / 3600)}h` : `${Math.floor(s / 86400)}d`;
function drawCauses(box, c) {
  box.replaceChildren();
  const sub = `${fmt.n(c.faults)} lines · most said first`;
  $('[data-faults-sub]').textContent = sub; $('[data-faults-sub2]').textContent = sub;
  if (!c.rows.length) box.append(el('div', 'cause', c.faults ? '' : 'nothing in the error log'));
  for (const r of c.rows) {
    const d = el('div', 'cause'), t = el('span');
    t.append(el('b', '', r.text), el('small', '', r.last && S.h ? `last ${ago(S.h.now - r.last)} ago` : ''));
    d.append(el('span', 'lv ' + r.level, r.level), t, el('b', 'num', `${fmt.n(r.n)}×`));
    box.append(d);
  }
}
function drawHeld() {
  const on = !!UI.heldPath, h = S.held;
  $('[data-held]').classList.toggle('on', on);
  $('[data-heldpath]').textContent = UI.heldPath || '';
  $('[data-heldnums]').textContent = on && h && h.known && h.path === UI.heldPath
    ? ` · ${fmt.rate(h.rate)}/s · 5xx ${fmt.pct(h.s5xx)}${h.p99 != null ? ' · p99 ' + fmt.ms(h.p99) : ''}` : '';
}

/* ----- the log: appended, never redrawn whole while it follows ----------- */
// a line: [number, time, code, path, text]; code is a status, -1-severity for the error log, 0 unread
let shown = 0, stick = true;
const tail = $('[data-tail]');
tail.addEventListener('scroll', () => { stick = tail.scrollTop + tail.clientHeight >= tail.scrollHeight - 24; });
function keep(l) {
  const c = l[2];
  if (UI.heldPath && l[3] !== UI.heldPath) return false;
  if (UI.logk === '4xx' && !(c >= 400 && c < 500)) return false;
  if (UI.logk === '5xx' && !(c >= 500)) return false;
  if (UI.logk === 'error' && !(c < 0)) return false;
  return !UI.logq || l[4].toLowerCase().includes(UI.logq);
}
const SEVERITY = ['notice', 'warn', 'error', 'crit'];
function lineEl(l) {
  const d = el('div'), c = l[2];
  d.append(el('span', 'n', l[0]), el('span', 't', l[1]));
  if (c < 0) d.append(el('span', 'e', SEVERITY[Math.min(3, -1 - c)] || 'error'));
  else if (c > 0) d.append(el('span', 's' + Math.floor(c / 100), String(c)));
  d.append(el('span', c < 0 ? 'm' : '', l[4]));
  return d;
}
function drawLog(full) {
  if (full) { tail.replaceChildren(); shown = 0; }
  const start = S.log.findIndex(l => l[0] > shown);
  if (start >= 0) {
    const frag = document.createDocumentFragment();
    for (let i = start; i < S.log.length; i++) if (keep(S.log[i])) frag.append(lineEl(S.log[i]));
    tail.append(frag); shown = S.log[S.log.length - 1][0];
    let extra = tail.childElementCount - S.logMax; while (extra-- > 0) tail.firstChild.remove();
  }
  if (stick) tail.scrollTop = tail.scrollHeight;
  const held = UI.heldPath ? ` · held to ${UI.heldPath}, among them` : '';
  $('[data-lognote]').textContent = `the last ${fmt.n(S.log.length)} lines${held}` +
    (S.passed ? ` · ${fmt.n(S.passed)} more went by faster than a page needs to show them` : '');
}

function drawFoot() {
  const now = performance.now(), secs = (now - S.wire.since) / 1000;
  if (secs >= 5) { S.wire.rate = S.wire.bytes / secs; S.wire.bytes = 0; S.wire.since = now; }
  const f = $('[data-foot]'); f.replaceChildren();
  const item = (k, v) => { const s = el('span', '', k + ' '); s.append(el('b', 'num', v)); f.append(s); };
  item('on the wire', `${fmt.bytes(S.wire.rate)}/s`); item('events', fmt.n(S.wire.events));
  item('viewers', String(S.viewers || 1)); if (S.h) item('error log', fmt.n(S.h.faults));
  f.append(el('span', '', 'pepe logs --serve · it reads the logs, and sends nginx nothing'));
}

/* ----- keys and clicks, as on the screen --------------------------------- */
function pick(tab) {
  UI.tab = tab;
  for (const b of document.querySelectorAll('[data-tab]')) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
  for (const s of document.querySelectorAll('[data-view]')) s.hidden = s.dataset.view !== tab;
  for (const k of BY_TAB[tab]) UI.dirty.add(k);
  if (tab === 'log') { stick = true; drawLog(true); }
  wants(); schedule();
}
function hold(path) {
  UI.heldPath = path; S.held = null; delete asking.held;
  UI.dirty.add('held');
  if (UI.tab === 'log') refilter(); else { S.log = []; S.lastLine = 0; S.passed = 0; wants(); }
  schedule();
}
$('[data-tabs]').addEventListener('click', e => { const b = e.target.closest('[data-tab]'); if (b) pick(b.dataset.tab); });
$('[data-unhold]').addEventListener('click', () => hold(null));
document.addEventListener('click', e => {
  const r = e.target.closest('[data-path]');
  if (!r || e.target.closest('[data-unhold]')) return;
  hold(r.dataset.path);
  if (UI.tab !== 'traffic') pick('traffic');
});
document.addEventListener('keydown', e => {
  if (e.target.matches('input')) { if (e.key === 'Escape') e.target.blur(); return; }
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const i = '12345'.indexOf(e.key); if (i >= 0) pick(['dashboard', 'traffic', 'paths', 'errors', 'log'][i]);
  if (e.key === 'Escape') hold(null);
  if (e.key === '/') { e.preventDefault(); if (UI.tab !== 'log') pick('paths'); (UI.tab === 'log' ? $('[data-logfilter]') : $('[data-pathfilter]')).focus(); }
});
$('[data-grain]').addEventListener('click', e => { const b = e.target.closest('[data-g]'); if (!b) return; UI.grain = b.dataset.g; for (const x of $('[data-grain]').children) x.setAttribute('aria-pressed', String(x === b)); UI.dirty.add('grain'); wants(); schedule(); });
$('thead').addEventListener('click', e => { const th = e.target.closest('[data-sort]'); if (!th) return; UI.sort = th.dataset.sort; for (const x of document.querySelectorAll('th')) x.toggleAttribute('aria-sort', x === th); UI.dirty.add('paths'); schedule(); });
$('[data-pathfilter]').addEventListener('input', e => { UI.pathq = e.target.value; UI.dirty.add('paths'); schedule(); });
let typing;
$('[data-logfilter]').addEventListener('input', e => { UI.logq = e.target.value.toLowerCase(); drawLog(true); clearTimeout(typing); typing = setTimeout(refilter, 250); });
$('[data-logchips]').addEventListener('click', e => { const b = e.target.closest('[data-k]'); if (!b) return; UI.logk = b.dataset.k; for (const x of $('[data-logchips]').children) x.setAttribute('aria-pressed', String(x === b)); refilter(); });
$('[data-themebtn]').addEventListener('click', () => {
  const r = document.documentElement, dark = r.dataset.theme ? r.dataset.theme === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
  r.dataset.theme = dark ? 'light' : 'dark';
  try { localStorage.setItem('pepe-theme', r.dataset.theme); } catch { /* kept for this page only */ }
  UI.dirty.add('all'); schedule();
});
try { const t = localStorage.getItem('pepe-theme'); if (t) document.documentElement.dataset.theme = t; } catch { /* the system's */ }

if (!token) gate('This page opens with the link pepe logs --serve printed: it carries the token after #token=.');
else connect();
