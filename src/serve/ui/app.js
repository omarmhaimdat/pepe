// pepe serve's page: the API (docs/serve.md) in a browser. No framework and
// no build step: the DOM is made with h(), which sets text, never markup, so
// nothing a target answers can become part of the page.
'use strict';

const view = document.getElementById('view');
const KEY = 'pepe-serve-token';
let token = null;
// What the view on screen has to stop (timers, streams) when it is left
let leaving = [];

// ─── Small things ────────────────────────────────────────────────────────

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'class') el.className = v;
    else if (k === 'value') el.value = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

const SVG = 'http://www.w3.org/2000/svg';
function s(tag, attrs, ...kids) {
  const el = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs || {})) el.setAttribute(k, v);
  for (const kid of kids.flat()) if (kid) el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  return el;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function ms(v) {
  if (v === null || v === undefined || Number.isNaN(+v)) return '—';
  v = +v;
  if (v < 1) return `${Math.round(v * 1000)}µs`;
  if (v < 10) return `${v.toFixed(2)}ms`;
  if (v < 1000) return `${v.toFixed(1)}ms`;
  return `${(v / 1000).toFixed(2)}s`;
}
const count = (n) => (n === null || n === undefined ? '—' : Math.round(+n).toLocaleString());
function rate(r) {
  if (r === null || r === undefined) return '—';
  r = +r;
  return r >= 10000 ? `${(r / 1000).toFixed(1)}k/s` : r >= 100 ? `${Math.round(r)}/s` : `${r.toFixed(1)}/s`;
}
function pct(p) {
  return p === null || p === undefined ? '—' : `${(+p).toFixed(p < 1 ? 2 : 1)}%`;
}
// `2026-10-25T23-00-48Z` (or `-2` after it) as the reader's local time
function when(stamp) {
  const m = /^(\d{4}-\d\d-\d\d)T(\d\d)-(\d\d)-(\d\d)Z/.exec(stamp || '');
  if (!m) return stamp || '—';
  const d = new Date(`${m[1]}T${m[2]}:${m[3]}:${m[4]}Z`);
  return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

const LEVEL = { healthy: 'good', degraded: 'warn', failing: 'bad', good: 'good', warn: 'warn', bad: 'bad', note: 'note' };
const STATE = { running: 'going', done: 'good', stopped: 'warn', failed: 'bad', lost: 'bad' };
const FACE = { running: 'waiting', healthy: 'proud', degraded: 'worried', failing: 'on-fire', stopped: 'sleeping', failed: 'on-fire', lost: 'sleeping', none: 'sleeping' };
const face = (name, cls = 'face') => h('img', { class: cls, src: `/ui/img/pepe-${name}.png`, alt: '' });
const pill = (text, cls) => h('span', { class: `pill ${cls || ''}` }, text);

function error(e) {
  return h('div', { class: 'error', role: 'alert' }, e && e.message ? e.message : String(e));
}

// ─── The API ─────────────────────────────────────────────────────────────

async function api(method, path, body) {
  const headers = { Authorization: `Bearer ${token}` };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await fetch(`/api${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  if (res.status === 401) {
    forget();
    gate('That token was refused: paste the one pepe serve printed.');
    throw new Error('the token was refused');
  }
  const type = res.headers.get('content-type') || '';
  const data = type.includes('json') ? await res.json() : await res.text();
  if (!res.ok) throw Object.assign(new Error((data && data.error) || `${res.status} ${res.statusText}`), { status: res.status });
  return data;
}

// A run's events, from the first (or after `after`), as Server-Sent Events
// read through fetch: EventSource can't send the token in a header, and a
// token in a URL would end up in logs. Picks up where it was if the
// connection drops, until the run's `end`.
async function follow(id, onEvent, signal) {
  let last = 0;
  while (!signal.aborted) {
    try {
      const headers = { Authorization: `Bearer ${token}` };
      if (last) headers['Last-Event-ID'] = String(last);
      const res = await fetch(`/api/runs/${encodeURIComponent(id)}/events`, { headers, signal });
      if (!res.ok) throw new Error(String(res.status));
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let at;
        while ((at = buf.indexOf('\n\n')) >= 0) {
          const block = buf.slice(0, at);
          buf = buf.slice(at + 2);
          const ev = { id: 0, event: 'message', data: '' };
          for (const line of block.split('\n')) {
            if (!line || line.startsWith(':')) continue;
            const c = line.indexOf(':');
            const k = line.slice(0, c);
            const v = line.slice(c + 1).replace(/^ /, '');
            if (k === 'id') ev.id = Number(v);
            else if (k === 'event') ev.event = v;
            else if (k === 'data') ev.data += v;
          }
          if (ev.id) last = ev.id;
          let data = null;
          try { data = JSON.parse(ev.data); } catch { continue; }
          onEvent(ev.event, data);
          if (ev.event === 'end') return;
        }
      }
    } catch (e) {
      if (signal.aborted) return;
    }
    await sleep(1000);
  }
}

// ─── The look's small parts ──────────────────────────────────────────────

// A page's head, as the front page writes one: a tag, a headline with a
// word selected as in a terminal, a line under it, and what to do here
function pagehead({ tag, before = '', sel, after = '', lede, right }) {
  return h('div', { class: 'pagehead' },
    tag ? h('span', { class: 'tag' }, tag) : null,
    h('div', { class: 'row' },
      h('h1', {}, before, sel ? selected(sel) : null, after),
      right ? h('div', { class: 'actions' }, right) : null),
    lede ? h('p', { class: 'lede' }, lede) : null);
}
const section = (text) => h('div', { class: 'section' }, text);
// Words selected as in a terminal, the cursor at their end
const selected = (text, blink) => h('span', { class: `sel${blink ? ' blink' : ''}` }, text, h('i', { class: 'cur', 'aria-hidden': 'true' }));
const card = (title, ...kids) => h('div', { class: 'card' }, title ? h('h2', {}, title) : null, kids);
const nums = (items) => h('div', { class: 'nums' }, items.map(([label, value, small]) => h('div', {}, h('small', {}, label), h('b', { class: small ? 'small' : '' }, value))));
const field = (label, input, hint) => h('div', { class: 'field' }, h('label', {}, label, input), hint ? h('div', { class: 'hint' }, hint) : null);
const command = (text) => h('pre', { class: 'cmd' }, h('span', { class: 'p' }, '$ '), text);
function emptyState(faceName, title, text, action) {
  return h('div', { class: 'card empty' }, face(faceName, ''), h('h3', {}, title), text ? h('p', {}, text) : null, action);
}

// Light or dark, as the reader picks; the page follows the system until then
const THEME = 'pepe-serve-theme';
function setTheme(theme) {
  if (theme) document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}
function themeToggle() {
  try { setTheme(localStorage.getItem(THEME)); } catch {}
  document.querySelector('.theme').addEventListener('click', () => {
    const dark = document.documentElement.dataset.theme
      ? document.documentElement.dataset.theme === 'dark'
      : matchMedia('(prefers-color-scheme: dark)').matches;
    const next = dark ? 'light' : 'dark';
    setTheme(next);
    try { localStorage.setItem(THEME, next); } catch {}
  });
}

// ─── The token ───────────────────────────────────────────────────────────

function takeToken() {
  const m = /(?:^#|[?&])token=([^&]+)/.exec(location.hash);
  if (m) {
    token = decodeURIComponent(m[1]);
    try { sessionStorage.setItem(KEY, token); } catch {}
    // Out of the address bar, the history and anything shared from it
    history.replaceState(null, '', `${location.pathname}#/runs`);
    return;
  }
  try { token = sessionStorage.getItem(KEY); } catch {}
}

function forget() {
  token = null;
  try { sessionStorage.removeItem(KEY); } catch {}
}

function chrome(on) {
  document.querySelector('.top').hidden = !on;
  document.querySelector('.top-strip').hidden = !on;
}

function gate(message) {
  chrome(false);
  const input = h('input', { type: 'password', placeholder: 'paste the token', autocomplete: 'off', 'aria-label': 'Token', required: true });
  const said = h('div', { class: 'said', role: 'alert' }, message || '');
  const form = h('form', {
    onsubmit: async (e) => {
      e.preventDefault();
      token = input.value.trim();
      try {
        await api('GET', '/runs?limit=1');
        try { sessionStorage.setItem(KEY, token); } catch {}
        location.hash = '#/runs';
        route();
      } catch (err) {
        said.textContent = err.message;
      }
    },
  }, h('span', { class: 'p' }, '$'), input, h('button', { type: 'submit' }, 'Open'));
  view.replaceChildren(h('div', { class: 'gate' },
    face('waiting', 'mascot'),
    h('span', { class: 'tag' }, 'pepe serve'),
    h('h1', {}, 'Load-test it, ', selected('from here', true)),
    h('p', { class: 'lede' }, 'Paste the token pepe serve printed when it started, or open the link it printed: it signs you in.'),
    form, said));
  input.focus();
}

// ─── Routing ─────────────────────────────────────────────────────────────

function route() {
  for (const stop of leaving) stop();
  leaving = [];
  // A link with a token, opened in a page that was open already
  if (/[#?&]token=/.test(location.hash)) takeToken();
  if (!token) return gate();
  chrome(true);
  document.querySelector('.who').replaceChildren(
    h('span', { class: 'who' }, `token ••••${token.slice(-4)} · `),
    h('a', { href: '#', onclick: (e) => { e.preventDefault(); forget(); gate(); } }, 'sign out'));
  const [path, q] = location.hash.replace(/^#\/?/, '').split('?');
  const [name, ...rest] = path.split('/').map(decodeURIComponent);
  const query = Object.fromEntries(new URLSearchParams(q || ''));
  const here = name === 'run' ? 'runs' : name === 'test' ? 'tests' : name || 'runs';
  for (const a of document.querySelectorAll('nav a')) a.classList.toggle('on', a.dataset.nav === here);
  const views = { runs: viewRuns, run: viewRun, new: viewNew, tests: viewTests, test: viewTest, compare: viewCompare, workers: viewWorkers };
  view.replaceChildren();
  window.scrollTo(0, 0);
  (views[name] || viewRuns)(rest, query).catch((e) => view.append(error(e)));
}

function every(ms, fn) {
  const t = setInterval(fn, ms);
  leaving.push(() => clearInterval(t));
}

const go = (id) => (location.hash = `#/run/${encodeURIComponent(id)}`);

// ─── Numbers, as the page writes them ────────────────────────────────────

function bytes(b) {
  if (b === null || b === undefined || Number.isNaN(+b)) return '—';
  b = +b;
  if (b < 1024) return `${Math.round(b)} B`;
  if (b < 1048576) return `${(b / 1024).toFixed(1)} KB`;
  if (b < 1073741824) return `${(b / 1048576).toFixed(1)} MB`;
  return `${(b / 1073741824).toFixed(2)} GB`;
}
function secs(t) {
  t = Math.max(0, +t || 0);
  if (t < 60) return `${t < 10 && t % 1 ? t.toFixed(1) : Math.round(t)}s`;
  if (t < 3600) return `${Math.floor(t / 60)}m${String(Math.round(t % 60)).padStart(2, '0')}s`;
  return `${Math.floor(t / 3600)}h${String(Math.floor((t % 3600) / 60)).padStart(2, '0')}m`;
}
function compact(n) {
  if (n === null || n === undefined) return '—';
  n = +n;
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M`;
  if (n >= 1e4) return `${(n / 1e3).toFixed(n >= 1e5 ? 0 : 1)}k`;
  return Math.round(n).toLocaleString();
}
const share = (part, whole) => (whole ? (100 * part) / whole : 0);

// Round numbers for an axis from 0 to at least `top`
function ticks(top, n = 4) {
  if (!(top > 0)) return [0, 1];
  const raw = top / n;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((x) => x >= raw * 0.999);
  const out = [];
  for (let v = 0; v < top + step * 0.5 && out.length < 12; v += step) out.push(+v.toPrecision(12));
  if (out.at(-1) < top) out.push(+(out.at(-1) + step).toPrecision(12));
  return out;
}

// ─── Charts ──────────────────────────────────────────────────────────────

// A tooltip that follows the pointer over a chart
function tipFor(wrap) {
  const tip = h('div', { class: 'tip', hidden: true });
  wrap.append(tip);
  return {
    show(x, W, kids) {
      tip.replaceChildren(...kids);
      tip.hidden = false;
      const box = wrap.getBoundingClientRect();
      const left = (x / W) * box.width;
      tip.style.left = `${left > box.width * 0.58 ? left - tip.offsetWidth - 14 : left + 14}px`;
    },
    hide() { tip.hidden = true; },
  };
}

// A chart over time, drawn again as the seconds come, with a crosshair that
// reads each series at the moment under the pointer. A series is
// { key, label, cls, kind: line | area | bars | step, dash, fmt }.
function timeChart({ series, fmt, xfmt = secs, height = 200, wide = false, empty = 'waiting for the first second…' }) {
  const W = wide ? 1140 : 560, H = height, L = 66, R = 12, T = 12, B = 24;
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  const body = s('g', {});
  const cross = s('line', { class: 'cross', y1: T, y2: H - B, x1: -9, x2: -9 });
  const dots = s('g', {});
  svg.append(body, cross, dots);
  const wrap = h('div', { class: 'chartwrap' }, svg);
  const tip = tipFor(wrap);
  let rows = [], marks = [], xs = null, ys = null, t0 = 0, t1 = 1;
  const valueOf = (r, x) => (r[x.key] === null || r[x.key] === undefined || !Number.isFinite(+r[x.key]) ? null : +r[x.key]);
  const draw = () => {
    body.replaceChildren();
    if (!rows.length) {
      xs = null;
      body.append(s('text', { class: 'axis', x: W / 2, y: H / 2, 'text-anchor': 'middle' }, empty));
      return;
    }
    t0 = rows[0].t;
    t1 = Math.max(t0 + 1, rows.at(-1).t);
    let top = 0;
    for (const r of rows) for (const x of series) { const v = valueOf(r, x); if (v !== null) top = Math.max(top, v); }
    const yt = ticks(top || 1, 4);
    const ymax = yt.at(-1);
    xs = (t) => L + ((t - t0) / (t1 - t0)) * (W - L - R);
    ys = (v) => T + (1 - v / ymax) * (H - T - B);
    for (const v of yt) {
      body.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: ys(v), y2: ys(v) }),
        s('text', { class: 'axis', x: L - 8, y: ys(v) + 3.5, 'text-anchor': 'end' }, fmt(v)));
    }
    for (const dt of ticks(t1 - t0, 5)) {
      const x = xs(t0 + dt);
      if (x > W - R + 0.5) break;
      body.append(s('text', { class: 'axis', x, y: H - 6, 'text-anchor': dt === 0 ? 'start' : 'middle' }, xfmt(t0 + dt)));
    }
    for (const m of marks) {
      if (m.t < t0 || m.t > t1) continue;
      body.append(s('line', { class: 'mark', x1: xs(m.t), x2: xs(m.t), y1: T, y2: H - B }),
        s('rect', { class: 'markhead', x: xs(m.t) - 3, y: T - 3, width: 6, height: 6 }, s('title', {}, m.text)));
    }
    const bw = Math.max(1.5, ((W - L - R) / Math.max(rows.length, 1)) * 0.62);
    for (const x of series) {
      const pts = rows.filter((r) => valueOf(r, x) !== null);
      if (!pts.length) continue;
      if (x.kind === 'bars') {
        for (const r of pts) {
          const v = valueOf(r, x);
          if (v > 0) body.append(s('rect', { class: `bar ${x.cls}`, x: xs(r.t) - bw / 2, y: ys(v), width: bw, height: Math.max(1, ys(0) - ys(v)) }));
        }
        continue;
      }
      let d = '';
      pts.forEach((r, i) => {
        const X = xs(r.t).toFixed(1), Y = ys(valueOf(r, x)).toFixed(1);
        d += i ? (x.kind === 'step' ? `H${X}V${Y}` : `L${X},${Y}`) : `M${X},${Y}`;
      });
      if (x.kind === 'area' || x.kind === 'step') {
        body.append(s('path', { class: `fill ${x.cls}`, d: `${d}L${xs(pts.at(-1).t).toFixed(1)},${ys(0)}L${xs(pts[0].t).toFixed(1)},${ys(0)}Z` }));
      }
      body.append(s('path', { class: `line ${x.cls}${x.dash ? ' dash' : ''}`, d }));
    }
  };
  svg.addEventListener('pointermove', (e) => {
    if (!xs) return;
    const box = svg.getBoundingClientRect();
    const t = t0 + ((((e.clientX - box.left) / box.width) * W - L) / (W - L - R)) * (t1 - t0);
    let r = rows[0];
    for (const q of rows) if (Math.abs(q.t - t) < Math.abs(r.t - t)) r = q;
    cross.setAttribute('x1', xs(r.t));
    cross.setAttribute('x2', xs(r.t));
    dots.replaceChildren(...series.filter((x) => x.kind !== 'bars' && valueOf(r, x) !== null)
      .map((x) => s('rect', { class: `dot ${x.cls}`, x: xs(r.t) - 3.5, y: ys(valueOf(r, x)) - 3.5, width: 7, height: 7 })));
    const here = marks.filter((m) => Math.abs(m.t - r.t) <= 0.6);
    tip.show(xs(r.t), W, [h('b', {}, xfmt(r.t)),
      ...series.filter((x) => valueOf(r, x) !== null).map((x) => h('div', {}, h('span', { class: `sw ${x.cls}${x.dash ? ' dash' : ''}` }), h('span', {}, x.label), h('i', {}, (x.fmt || fmt)(valueOf(r, x))))),
      ...here.map((m) => h('div', { class: 'tipnote' }, m.text))]);
  });
  svg.addEventListener('pointerleave', () => {
    tip.hide();
    cross.setAttribute('x1', -9);
    cross.setAttribute('x2', -9);
    dots.replaceChildren();
  });
  draw();
  return { el: wrap, update(r, m = []) { rows = r; marks = m; draw(); } };
}

const legend = (series) => h('div', { class: 'legend' }, series.map((x) => h('span', {}, h('span', { class: `sw ${x.cls}${x.dash ? ' dash' : ''}` }), x.label)));

// A chart in a card of its own, titled, its legend on the right
function chartCard(title, chart, series, note) {
  return h('div', { class: 'chartcard' },
    h('header', {}, h('h3', {}, title), series ? legend(series) : null),
    chart.el || chart, note ? h('p', { class: 'note' }, note) : null);
}

// A line of the last values, inside a tile
function spark(values, cls = '') {
  const W = 120, H = 30;
  const v = values.filter((x) => Number.isFinite(x));
  const svg = s('svg', { class: `spark ${cls}`, viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: 'none', 'aria-hidden': 'true' });
  if (v.length < 2) return svg;
  const top = Math.max(...v), lo = Math.min(0, ...v);
  const span = top - lo || 1;
  const d = v.map((x, i) => `${i ? 'L' : 'M'}${((i / (v.length - 1)) * W).toFixed(1)},${(H - 2 - ((x - lo) / span) * (H - 5)).toFixed(1)}`).join('');
  svg.append(s('path', { class: 'fill', d: `${d}L${W},${H}L0,${H}Z` }), s('path', { class: 'line', d }));
  return svg;
}

// The run's numbers in tiles: [label, value, under it, spark values, cls]
function tiles(items) {
  return h('div', { class: 'tiles' }, items.filter(Boolean).map(([label, value, sub, sv, cls]) => h('div', { class: `tile ${cls || ''}` },
    h('small', {}, label), h('b', {}, value), h('span', { class: 'sub' }, sub || ' '), sv ? spark(sv, cls) : null)));
}

// The latency's distribution, from the report's whole histogram, on a
// scale of time that widens as it goes (the tail is where it is long)
function histogramChart(buckets, marks) {
  const real = (buckets || []).filter((b) => b[1] > 0);
  if (!real.length) return null;
  const lo = Math.max(1, real[0][0]);
  const hi = Math.max(lo * 2, real.at(-1)[0]);
  const N = 54, llo = Math.log(lo), lhi = Math.log(hi) * 1.000001;
  const bin = (us) => Math.min(N - 1, Math.max(0, Math.floor(((Math.log(Math.max(us, lo)) - llo) / (lhi - llo)) * N)));
  const bins = Array(N).fill(0);
  let total = 0;
  for (const [us, c] of real) { bins[bin(us)] += c; total += c; }
  const W = 560, H = 220, L = 60, R = 12, T = 22, B = 24;
  const yt = ticks(Math.max(...bins), 4), ymax = yt.at(-1);
  const x = (us) => L + ((Math.log(us) - llo) / (lhi - llo)) * (W - L - R);
  const y = (c) => T + (1 - c / ymax) * (H - T - B);
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  for (const v of yt) svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: y(v), y2: y(v) }), s('text', { class: 'axis', x: L - 8, y: y(v) + 3.5, 'text-anchor': 'end' }, compact(v)));
  // A tick a power of ten, and at 2 and 5 when the range is short
  const decades = Math.log10(hi / lo);
  const steps = decades < 1.5 ? [1, 2, 5] : [1];
  for (let p = Math.floor(Math.log10(lo)); p <= Math.ceil(Math.log10(hi)); p++) {
    for (const m of steps) {
      const us = m * 10 ** p;
      if (us < lo || us > hi) continue;
      svg.append(s('line', { class: 'grid faint', x1: x(us), x2: x(us), y1: T, y2: H - B }), s('text', { class: 'axis', x: x(us), y: H - 6, 'text-anchor': 'middle' }, ms(us / 1000)));
    }
  }
  const step = (W - L - R) / N;
  const rects = bins.map((c, i) => {
    const r = s('rect', { class: 'bar hist', x: L + i * step + 0.5, y: y(c), width: Math.max(1, step - 1), height: Math.max(c ? 1 : 0, y(0) - y(c)) });
    svg.append(r);
    return r;
  });
  for (const [label, msv, cls] of marks) {
    if (!msv) continue;
    const X = x(Math.min(hi, Math.max(lo, msv * 1000)));
    svg.append(s('line', { class: `pmark ${cls}`, x1: X, x2: X, y1: T - 4, y2: H - B }), s('text', { class: `plabel ${cls}`, x: X, y: T - 8, 'text-anchor': 'middle' }, label));
  }
  const hover = s('rect', { class: 'hoverband', x: -99, y: T, width: step, height: H - T - B });
  svg.append(hover);
  const wrap = h('div', { class: 'chartwrap' }, svg);
  const tip = tipFor(wrap);
  svg.addEventListener('pointermove', (e) => {
    const box = svg.getBoundingClientRect();
    const i = Math.floor((((e.clientX - box.left) / box.width) * W - L) / step);
    if (i < 0 || i >= N) return tip.hide();
    const from = Math.exp(llo + (i / N) * (lhi - llo)), to = Math.exp(llo + ((i + 1) / N) * (lhi - llo));
    const below = bins.slice(0, i + 1).reduce((a, b) => a + b, 0);
    hover.setAttribute('x', L + i * step);
    rects.forEach((r, j) => r.classList.toggle('on', j === i));
    tip.show(L + (i + 0.5) * step, W, [h('b', {}, `${ms(from / 1000)} – ${ms(to / 1000)}`),
      h('div', {}, h('span', {}, 'requests'), h('i', {}, count(bins[i]))),
      h('div', {}, h('span', {}, 'of all'), h('i', {}, pct(share(bins[i], total)))),
      h('div', {}, h('span', {}, 'this fast or faster'), h('i', {}, pct(share(below, total))))]);
  });
  svg.addEventListener('pointerleave', () => { tip.hide(); hover.setAttribute('x', -99); rects.forEach((r) => r.classList.remove('on')); });
  return wrap;
}

// The latency at each percentile, the tail spread out: each nine (90,
// 99, 99.9) as wide as the one before. Curves: [{ label, cls, points: [[q, ms]] }]
function percentileChart(curves, wide = false) {
  const nines = (q) => -Math.log10(1 - Math.min(q, 99.95) / 100);
  const W = wide ? 1140 : 560, H = 220, L = 60, R = 24, T = 12, B = 24;
  const xmax = nines(99.9);
  const all = curves.flatMap((c) => c.points);
  if (!all.length) return null;
  const yt = ticks(Math.max(...all.map((p) => p[1])), 4), ymax = yt.at(-1);
  const x = (q) => L + (nines(q) / xmax) * (W - L - R);
  const y = (v) => T + (1 - v / ymax) * (H - T - B);
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  for (const v of yt) svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: y(v), y2: y(v) }), s('text', { class: 'axis', x: L - 8, y: y(v) + 3.5, 'text-anchor': 'end' }, ms(v)));
  for (const q of [0, 50, 90, 99, 99.9]) {
    svg.append(s('line', { class: 'grid faint', x1: x(q), x2: x(q), y1: T, y2: H - B }), s('text', { class: 'axis', x: x(q), y: H - 6, 'text-anchor': q === 99.9 ? 'end' : q ? 'middle' : 'start' }, q ? `p${q}` : 'p0'));
  }
  const dots = s('g', {});
  for (const c of curves) {
    const pts = c.points.filter((p) => p[0] <= 99.9);
    const d = pts.map((p, i) => `${i ? 'L' : 'M'}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join('');
    if (c.area) svg.append(s('path', { class: `fill ${c.cls}`, d: `${d}L${x(pts.at(-1)[0])},${y(0)}L${x(pts[0][0])},${y(0)}Z` }));
    svg.append(s('path', { class: `line ${c.cls}${c.dash ? ' dash' : ''}`, d }));
  }
  const cross = s('line', { class: 'cross', y1: T, y2: H - B, x1: -9, x2: -9 });
  svg.append(cross, dots);
  const wrap = h('div', { class: 'chartwrap' }, svg);
  const tip = tipFor(wrap);
  const qs = [...new Set(all.map((p) => p[0]).filter((q) => q <= 99.9))].sort((a, b) => a - b);
  svg.addEventListener('pointermove', (e) => {
    const box = svg.getBoundingClientRect();
    const X = ((e.clientX - box.left) / box.width) * W;
    let q = qs[0];
    for (const v of qs) if (Math.abs(x(v) - X) < Math.abs(x(q) - X)) q = v;
    cross.setAttribute('x1', x(q));
    cross.setAttribute('x2', x(q));
    const at = curves.map((c) => [c, (c.points.find((p) => p[0] === q) || [])[1]]).filter(([, v]) => v !== undefined);
    dots.replaceChildren(...at.map(([c, v]) => s('rect', { class: `dot ${c.cls}`, x: x(q) - 3.5, y: y(v) - 3.5, width: 7, height: 7 })));
    tip.show(x(q), W, [h('b', {}, q ? `p${q}` : 'fastest'), ...at.map(([c, v]) => h('div', {}, h('span', { class: `sw ${c.cls}${c.dash ? ' dash' : ''}` }), h('span', {}, c.label), h('i', {}, ms(v))))]);
  });
  svg.addEventListener('pointerleave', () => { tip.hide(); cross.setAttribute('x1', -9); cross.setAttribute('x2', -9); dots.replaceChildren(); });
  return wrap;
}

// A ramp's curve: what each step did (bars), and its p99 (a line on the
// right's scale), the step that held marked
function capacityChart(steps, held) {
  if (!steps.length) return null;
  const W = 1140, H = 260, L = 54, R = 54, T = 16, B = 26;
  const rt = ticks(Math.max(...steps.map((x) => x.requests_per_second || 0)), 4), rmax = rt.at(-1);
  const lt = ticks(Math.max(...steps.map((x) => x.p99_ms || 0)), 4), lmax = lt.at(-1);
  const slot = (W - L - R) / steps.length;
  const cx = (i) => L + (i + 0.5) * slot;
  const yr = (v) => T + (1 - v / rmax) * (H - T - B);
  const yl = (v) => T + (1 - v / lmax) * (H - T - B);
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  rt.forEach((v, i) => {
    svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: yr(v), y2: yr(v) }), s('text', { class: 'axis', x: L - 8, y: yr(v) + 3.5, 'text-anchor': 'end' }, compact(v)));
    if (lt[i] !== undefined) svg.append(s('text', { class: 'axis red', x: W - R + 8, y: yl(lt[i]) + 3.5 }, ms(lt[i])));
  });
  steps.forEach((x, i) => {
    if (x.concurrency === held) svg.append(s('rect', { class: 'heldband', x: cx(i) - slot / 2, y: T, width: slot, height: H - T - B }));
    const bw = Math.min(46, slot * 0.6);
    svg.append(s('rect', { class: `bar ${x.error_rate_percent > 1 ? 'c-red' : 'c-ember'}`, x: cx(i) - bw / 2, y: yr(x.requests_per_second || 0), width: bw, height: yr(0) - yr(x.requests_per_second || 0) }),
      s('text', { class: 'axis', x: cx(i), y: H - 8, 'text-anchor': 'middle' }, x.concurrency));
  });
  const d = steps.map((x, i) => `${i ? 'L' : 'M'}${cx(i).toFixed(1)},${yl(x.p99_ms || 0).toFixed(1)}`).join('');
  svg.append(s('path', { class: 'line c-red', d }));
  steps.forEach((x, i) => svg.append(s('rect', { class: 'dot c-red', x: cx(i) - 3.5, y: yl(x.p99_ms || 0) - 3.5, width: 7, height: 7 })));
  const wrap = h('div', { class: 'chartwrap' }, svg);
  const tip = tipFor(wrap);
  svg.addEventListener('pointermove', (e) => {
    const box = svg.getBoundingClientRect();
    const i = Math.floor((((e.clientX - box.left) / box.width) * W - L) / slot);
    const x = steps[i];
    if (!x) return tip.hide();
    tip.show(cx(i), W, [h('b', {}, `${x.concurrency} at once${x.concurrency === held ? ' · held' : ''}`),
      h('div', {}, h('span', { class: 'sw c-ember' }), h('span', {}, 'req/s'), h('i', {}, rate(x.requests_per_second))),
      h('div', {}, h('span', { class: 'sw c-red' }), h('span', {}, 'p99'), h('i', {}, ms(x.p99_ms))),
      h('div', {}, h('span', {}, 'median'), h('i', {}, ms(x.median_ms))),
      h('div', {}, h('span', {}, 'errors'), h('i', {}, pct(x.error_rate_percent)))]);
  });
  svg.addEventListener('pointerleave', () => tip.hide());
  return wrap;
}

// Where the time went, a bar a phase: the median filled, a tick at the
// p99, a dot at the slowest; all on the one scale
function phaseBars(rows) {
  rows = rows.filter((r) => r && r.median !== null && r.median !== undefined);
  if (!rows.length) return null;
  const top = Math.max(...rows.map((r) => Math.max(r.p99 || 0, r.median || 0))) || 1;
  const at = (v) => `${Math.min(100, (100 * v) / top)}%`;
  return h('div', { class: 'phases' },
    h('div', { class: 'phase head' }, h('span', {}, 'Phase'), h('span', {}, ''), h('span', { class: 'num' }, 'median'), h('span', { class: 'num' }, 'p99'), h('span', { class: 'num hide-narrow' }, 'max')),
    rows.map((r) => h('div', { class: 'phase' },
      h('span', { class: 'what' }, r.name, r.note ? h('small', {}, r.note) : null),
      h('span', { class: 'track' }, h('i', { class: `fillbar ${r.cls || ''}`, style: `width:${at(r.median)}` }),
        r.p99 ? h('i', { class: 'p99tick', style: `left:${at(r.p99)}`, title: `p99 ${ms(r.p99)}` }) : null),
      h('span', { class: 'num' }, ms(r.median)), h('span', { class: 'num' }, ms(r.p99)), h('span', { class: 'num hide-narrow faint' }, r.max === undefined ? '—' : ms(r.max)))));
}

// Status codes as one bar, a colour a class of them, then each
function statusBar(codes) {
  const list = Object.entries(codes || {}).map(([c, n]) => [c, +n]).sort((a, b) => b[1] - a[1]);
  if (!list.length) return null;
  const total = list.reduce((a, [, n]) => a + n, 0);
  const cls = (c) => `s${String(c)[0]}`;
  return h('div', {},
    h('div', { class: 'stack' }, list.map(([c, n]) => h('i', { class: cls(c), style: `width:${share(n, total)}%`, title: `${c}: ${count(n)}` }))),
    h('div', { class: 'codes' }, list.map(([c, n]) => h('div', {}, h('span', { class: `sq ${cls(c)}` }), h('b', { class: 'mono' }, c), h('span', { class: 'mono' }, count(n)), h('span', { class: 'faint mono' }, pct(share(n, total)))))));
}

// A cell with a bar behind its number, the bar its share of the most
const barCell = (value, most, text, cls = '') => h('span', { class: `barcell ${cls}` }, h('i', { style: `width:${most ? Math.min(100, (100 * value) / most) : 0}%` }), h('span', {}, text));

// ─── Thresholds ──────────────────────────────────────────────────────────

// The run's --fail-if limits, read from its command, as k6 shows thresholds
function limitsOf(command) {
  const out = [];
  // Quoted, or as the server writes it: up to the next flag or the URL
  const re = /--fail-if(?:=|\s+)(?:'([^']*)'|"([^"]*)"|(.+?)(?=\s+--?[a-zA-Z]|\s+\S+:\/\/|\s*$))/g;
  let m;
  while ((m = re.exec(command || ''))) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

// The latency at percentile `p` in a report's summary: exact where it was
// kept, read between the two nearest where it wasn't
function latencyAt(latency, p) {
  const named = { 50: latency.median_ms, 90: latency.p90_ms, 95: latency.p95_ms, 99: latency.p99_ms, 100: latency.max_ms, 0: latency.min_ms };
  if (named[p] !== undefined) return named[p];
  const qs = [[0, latency.min_ms], ...(latency.quantiles_ms || []).map((x) => [x.q, x.ms]), [100, latency.max_ms]].sort((a, b) => a[0] - b[0]);
  const exact = qs.find((x) => x[0] === p);
  if (exact) return exact[1];
  const hi = qs.findIndex((x) => x[0] > p);
  if (hi <= 0) return latency.max_ms;
  const [q0, v0] = qs[hi - 1], [q1, v1] = qs[hi];
  return v0 + ((v1 - v0) * (p - q0)) / (q1 - q0);
}

function checkLimit(text, summary) {
  const m = /^\s*(p[\d.]+|errors?|error rate)\s*>\s*([\d.]+)\s*(ms|us|µs|s|%)?\s*$/i.exec(text);
  if (!m || !summary) return { text, held: null, measured: '—' };
  if (/^p/i.test(m[1])) {
    const p = +m[1].slice(1);
    const limit = +m[2] * ({ ms: 1, us: 0.001, µs: 0.001, s: 1000 }[(m[3] || 'ms').toLowerCase()] || 1);
    const v = latencyAt(summary.latency, p);
    return { text, held: !(v > limit), measured: ms(v), limit: ms(limit) };
  }
  const v = share(summary.failed_requests, summary.total_requests);
  return { text, held: !(v > +m[2]), measured: pct(v), limit: pct(+m[2]) };
}

// ─── Runs ────────────────────────────────────────────────────────────────

// What a run is, in a few words: its test, or its mode and what it sends to
function titleOf(meta) {
  if (meta.test) return meta.test;
  const last = meta.command.replace(/^pepe /, '').split(' ').at(-1);
  const file = last.split('/').at(-1);
  const urls = (meta.command.match(/\S+:\/\/\S+/g) || []).length;
  return { api: `OpenAPI · ${file}`, flow: `Flow · ${file}`, replay: `Replay · ${file}`, ping: urls > 1 ? `Ping · ${urls} targets` : `Ping · ${last}` }[meta.mode] || last;
}

const MODE_NAMES = { run: 'Load test', api: 'OpenAPI', flow: 'Flow', replay: 'Replay', ping: 'Ping', ramp: 'Ramp' };
// A run in a word: how it went once it has a verdict, how it stands before
function standing(r) {
  if (r.state === 'running') return ['live', 'going'];
  const level = r.glance && r.glance.level;
  if (r.state === 'done' && level) return [level, LEVEL[level]];
  return [r.state, STATE[r.state]];
}

let runFilter = { mode: '', text: '' };

async function viewRuns() {
  const lede = h('p', { class: 'lede' }, '');
  const top = h('div', {});
  const strip = h('div', {});
  const body = h('div', {});
  const search = h('input', { type: 'search', placeholder: 'filter: a URL, a test, a word of the command', value: runFilter.text, 'aria-label': 'Filter runs' });
  const chips = h('div', { class: 'chips' });
  view.append(h('div', { class: 'pagehead' },
    h('span', { class: 'tag' }, 'Runs'),
    h('div', { class: 'row' },
      h('h1', {}, 'What ran, and ', selected('how it went')),
      h('div', { class: 'actions' }, h('a', { class: 'button go', href: '#/new' }, '+ New run'))),
    lede), top, strip, h('div', { class: 'filters' }, chips, search), body);
  let data = null;
  const table = () => {
    const shown = data.runs.filter((r) => (!runFilter.mode || r.mode === runFilter.mode)
      && (!runFilter.text || `${r.command} ${r.test || ''} ${r.by}`.toLowerCase().includes(runFilter.text.toLowerCase())));
    chips.replaceChildren(...[['', 'All'], ...Object.entries(MODE_NAMES)].map(([mode, name]) => {
      const n = mode ? data.runs.filter((r) => r.mode === mode).length : data.runs.length;
      if (mode && !n) return null;
      return h('button', { type: 'button', class: `chip${runFilter.mode === mode ? ' on' : ''}`, onclick: () => { runFilter.mode = mode; table(); } }, name, h('small', {}, n));
    }).filter(Boolean));
    if (!data.runs.length) {
      body.replaceChildren(emptyState('sleeping', 'Nothing has run yet', 'A run started here, from a script or from a schedule shows up here, live.',
        h('a', { class: 'button go', href: '#/new' }, 'Start the first one')));
      return;
    }
    if (!shown.length) return body.replaceChildren(h('div', { class: 'card flat empty' }, h('p', {}, 'No run matches.')));
    const most = Math.max(...shown.map((r) => (r.glance && r.glance.p99_ms) || 0));
    body.replaceChildren(h('div', { class: 'card tablecard' }, h('div', { class: 'table' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Status'), h('th', { style: 'width:40%' }, 'Run'), h('th', { class: 'num hide-narrow' }, 'Requests'), h('th', { class: 'num hide-narrow' }, 'req/s'),
        h('th', { class: 'num hide-narrow' }, 'p50'), h('th', { class: 'hide-narrow', style: 'min-width:120px' }, 'p99'), h('th', { class: 'num' }, 'Failed'), h('th', { class: 'num hide-narrow' }, 'Took'), h('th', { class: 'hide-narrow' }, 'Started'))),
      h('tbody', {}, shown.map((r) => {
        const g = r.glance || {};
        const [word, cls] = standing(r);
        return h('tr', { class: 'link', tabindex: 0, onclick: () => go(r.id), onkeydown: (e) => e.key === 'Enter' && go(r.id) },
          h('td', {}, pill(word, cls)),
          h('td', {}, h('span', { class: 'what' }, titleOf(r)),
            h('span', { class: 'sub clip hide-narrow', title: r.command }, h('span', { class: 'modechip' }, MODE_NAMES[r.mode] || r.mode), r.command.replace(/^pepe /, '')),
            r.workers && r.workers.length ? h('span', { class: 'sub' }, `shared out to ${r.workers.join(', ')}`) : null,
            h('span', { class: 'sub only-narrow' }, when(r.started))),
          h('td', { class: 'num hide-narrow' }, g.requests === undefined ? '—' : compact(g.requests)),
          h('td', { class: 'num hide-narrow' }, g.requests_per_second ? rate(g.requests_per_second) : '—'),
          h('td', { class: 'num hide-narrow' }, ms(g.p50_ms)),
          h('td', { class: 'hide-narrow' }, g.p99_ms === undefined || g.p99_ms === null ? h('span', { class: 'faint mono' }, '—') : barCell(g.p99_ms, most, ms(g.p99_ms))),
          h('td', { class: `num ${g.failed_percent > 1 ? 'bad' : g.failed_percent > 0 ? 'warn' : ''}` }, g.failed_percent === undefined ? '—' : pct(g.failed_percent)),
          h('td', { class: 'num hide-narrow faint' }, g.seconds ? secs(g.seconds) : '—'),
          h('td', { class: 'nowrap hide-narrow faint' }, when(r.started), h('span', { class: 'sub' }, r.by)));
      }))))));
  };
  const draw = async () => {
    data = await api('GET', '/runs?limit=200');
    lede.textContent = data.going
      ? `${data.going} going now, ${data.max_runs} at most at once. Newest first.`
      : `Nothing going. ${data.max_runs} at most at once. Newest first.`;
    const ended = data.runs.filter((r) => r.state !== 'running');
    const by = (level) => ended.filter((r) => r.glance && r.glance.level === level).length;
    const sent = data.runs.reduce((a, r) => a + ((r.glance && r.glance.requests) || 0), 0);
    if (data.runs.length) {
      top.replaceChildren(tiles([
        ['Runs', count(data.runs.length), `${data.going} going now`],
        ['Healthy', count(by('healthy')), `of ${ended.length} ended`, null, 'good'],
        ['Degraded', count(by('degraded')), 'look at the findings', null, by('degraded') ? 'warn' : ''],
        ['Failing', count(by('failing')) , `${ended.filter((r) => r.state === 'failed' || r.state === 'lost').length} didn't finish`, null, by('failing') ? 'bad' : ''],
        ['Requests sent', compact(sent), 'by every run listed'],
      ]));
      // The last runs a square each, oldest left, coloured as they went
      const last = data.runs.slice(0, 60).reverse();
      strip.replaceChildren(h('div', { class: 'history' }, h('small', {}, `The last ${last.length}`),
        h('div', { class: 'squares' }, last.map((r) => {
          const [word, cls] = standing(r);
          return h('a', { class: `square ${cls || ''}`, href: `#/run/${encodeURIComponent(r.id)}`, title: `${titleOf(r)} · ${word} · ${when(r.started)}` });
        }))));
    }
    table();
  };
  search.addEventListener('input', () => { runFilter.text = search.value; table(); });
  await draw();
  every(2500, () => document.activeElement !== search && draw().catch(() => {}));
}

// ─── A run ───────────────────────────────────────────────────────────────

let tabNow = 'Overview';

// A report's anomalies and live anomaly lines, as marks on the charts
function anomalyMarks(report) {
  return ((report && report.summary && report.summary.anomalies) || []).map((a) => ({ t: a.at_s, text: a.text }));
}

async function viewRun([id]) {
  const meta = await api('GET', `/runs/${encodeURIComponent(id)}`);
  const going = meta.state === 'running';
  const isPing = meta.mode === 'ping';
  const isRamp = meta.mode === 'ramp';
  const facePic = face(FACE[meta.state] || 'sleeping');
  const badge = h('span', { class: `badge ${going ? 'going' : STATE[meta.state] || ''}` }, going ? 'live' : meta.state);
  const steer = h('div', { class: 'steer' });
  const progressBar = h('div', { class: 'progress', hidden: true }, h('i', {}), h('span', {}));
  const limits = limitsOf(meta.command);

  // The tabs, as k6's dashboard has them
  const modeTab = { api: 'Endpoints', flow: 'Steps', replay: 'URLs', ramp: 'Ramp' }[meta.mode];
  const names = isPing ? ['Overview', 'Targets', 'Events'] : ['Overview', 'Timings', 'Summary', modeTab, 'Events'].filter(Boolean);
  const panes = Object.fromEntries(names.map((n) => [n, h('div', { class: 'pane', role: 'tabpanel' })]));
  const tabButtons = names.map((n) => h('button', { type: 'button', role: 'tab', class: 'tabbtn', onclick: () => pick(n) }, n));
  const pick = (n) => {
    if (!panes[n]) n = names[0];
    tabNow = n;
    names.forEach((m, i) => { panes[m].hidden = m !== n; tabButtons[i].classList.toggle('on', m === n); tabButtons[i].setAttribute('aria-selected', m === n); });
  };

  view.append(
    h('div', { class: 'runhead' }, facePic, h('div', { style: 'min-width:0;flex:1' },
      h('span', { class: 'tag' }, `${MODE_NAMES[meta.mode] || meta.mode} · ${when(meta.started)} · by ${meta.by}`),
      h('div', { class: 'titlerow' }, h('h1', {}, titleOf(meta)), badge))),
    command(meta.command),
    progressBar,
    steer,
    h('div', { class: 'tabs', role: 'tablist' }, tabButtons),
    ...names.map((n) => panes[n]));
  pick(tabNow);

  // ── What every tab holds, filled as the events and the report come
  const tilesBox = h('div', {});
  const limitsBox = h('div', {});
  const verdictBox = h('div', {});
  const feed = h('ul', { class: 'feed' });
  const workersBox = h('div', {});
  const rows = [];
  const marks = [];
  let report = null;

  const fmtRate = (v) => rate(v).replace('/s', '');
  const S = {
    rps: { key: 'rps', label: 'req/s', cls: 'c-ink warm', kind: 'area' },
    fps: { key: 'fps', label: 'errors/s', cls: 'c-red', kind: 'bars' },
    p50: { key: 'p50', label: 'p50', cls: 'c-leaf' },
    p90: { key: 'p90', label: 'p90', cls: 'c-ember' },
    p95: { key: 'p95', label: 'p95', cls: 'c-ember' },
    p99: { key: 'p99', label: 'p99', cls: 'c-red', dash: true },
    bps: { key: 'bps', label: 'received/s', cls: 'c-gold', kind: 'area' },
    conc: { key: 'conc', label: 'at once', cls: 'c-leaf', kind: 'step' },
    ttfb: { key: 'ttfb', label: 'first byte', cls: 'c-ember', kind: 'area' },
    read: { key: 'read', label: 'body', cls: 'c-ink' },
  };
  let charts = [];
  if (isPing) {
    const targets = (meta.command.match(/https?:\/\/\S+/g) || []);
    const cls = ['c-ink', 'c-ember', 'c-leaf', 'c-red', 'c-gold', 'c-blush'];
    const series = (targets.length ? targets : ['target']).slice(0, 6).map((t, i) => ({ key: `t${i}`, label: t.replace(/^https?:\/\//, '').replace(/\/$/, ''), cls: cls[i] }));
    const rt = timeChart({ series, fmt: ms, height: 240, wide: true });
    const loss = timeChart({ series: series.map((x) => ({ ...x, key: `l${x.key}`, kind: 'step' })), fmt: (v) => `${Math.round(v)}%`, height: 150, wide: true });
    charts = [rt, loss];
    panes.Overview.append(tilesBox,
      h('div', { class: 'chartgrid one' }, chartCard('Round trip, each request', rt, series)),
      h('div', { class: 'chartgrid one' }, chartCard('Lost so far', loss, null)),
      verdictBox);
  } else {
    const c1 = timeChart({ series: [S.rps, S.fps], fmt: fmtRate });
    // A load test's events carry each second on its own, as k6 plots
    // them; the other modes' are the run so far
    const perSecond = meta.mode === 'run';
    const lat = perSecond ? [S.p50, S.p90, S.p99] : [S.p50, S.p95, S.p99];
    const c2 = timeChart({ series: lat, fmt: ms });
    const c3 = timeChart({ series: [S.bps], fmt: (v) => bytes(v) });
    const c4 = timeChart({ series: [S.conc], fmt: (v) => (Number.isInteger(v) ? count(v) : '') });
    const c5 = timeChart({ series: [S.ttfb, S.read], fmt: ms, height: 220, wide: true });
    charts = [c1, c2, c3, c4, c5];
    panes.Overview.append(tilesBox, limitsBox,
      h('div', { class: 'chartgrid' },
        chartCard('Request rate', c1, [S.rps, S.fps]),
        chartCard(perSecond ? 'Request duration, each second' : 'Request duration, so far', c2, lat),
        chartCard('Data received', c3, [S.bps]),
        chartCard(isRamp ? 'Concurrency, step by step' : 'Concurrency', c4, [S.conc])),
      workersBox, verdictBox);
    panes.Timings.append(h('div', { class: 'chartgrid one' }, chartCard('Waiting for the first byte, and reading the body (medians so far)', c5, [S.ttfb, S.read])));
  }
  panes.Events.append(h('div', { class: 'card' }, feed));
  const timingsMore = h('div', {});
  if (panes.Timings) panes.Timings.append(timingsMore);
  const blank = (pane, text) => pane && pane.append(h('div', { class: 'card flat waiting' }, face('waiting', ''), h('p', {}, text)));
  blank(panes.Summary, 'The summary comes with the report, when the run ends.');
  blank(panes[modeTab], 'Each part of the run comes with the report, when it ends.');
  blank(panes.Targets, 'Each target in detail comes with the report, when the ping ends.');

  let queued = false;
  const redraw = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; for (const c of charts) c.update(rows, marks); });
  };

  const showSteer = (on, paused) => {
    steer.replaceChildren();
    if (!on) return;
    const say = async (what, body) => {
      try { await api('POST', `/runs/${encodeURIComponent(id)}/${what}`, body); } catch (e) { steer.append(error(e)); }
    };
    if (!isPing) {
      const n = h('input', { type: 'number', min: 1, 'aria-label': 'Concurrency', placeholder: 'at once' });
      steer.append(
        h('button', { class: 'primary', onclick: () => say(paused ? 'resume' : 'pause') }, paused ? '▶ Resume' : '❚❚ Pause'),
        n, h('button', { onclick: () => n.value && say('concurrency', { n: Number(n.value) }) }, 'Set concurrency'));
    }
    steer.append(h('button', { class: 'danger', onclick: () => say('stop') }, '■ Stop'));
  };
  showSteer(going, false);

  const line = (cls, text, t) => feed.prepend(h('li', {}, h('span', { class: 'mono faint at' }, t === undefined ? '' : secs(t)), h('span', { class: `d ${cls}` }), h('span', {}, text)));
  let planned = null, lastT = 0;
  let curConc = null;
  let startConc = (/(?:^|\s)-c=?(\d+)/.exec(meta.command) || [])[1];
  startConc = startConc ? +startConc : null;
  const live = (e) => {
    const t = e.elapsed_s ?? e.at_s ?? 0;
    lastT = t;
    if (e.targets) {
      const row = { t };
      e.targets.forEach((x, i) => { row[`t${i}`] = x.last_ms; row[`lt${i}`] = share((x.sent || 0) - (x.answered || 0), x.sent || 0); });
      rows.push(row);
      const first = e.targets[0] || {};
      if (!report) {
        tilesBox.replaceChildren(tiles([
          ['Sent', count(first.sent), `${count(first.answered)} answered`],
          ['Last', ms(first.last_ms), 'round trip', rows.map((r) => r.t0)],
          ['p50', ms(first.p50_ms), 'so far'],
          ['p99', ms(first.p99_ms), 'so far', null, 'red'],
          ['Lost', pct(share((first.sent || 0) - (first.answered || 0), first.sent || 0)), 'so far'],
        ]));
      }
    } else {
      const prev = rows.at(-1);
      const dt = prev ? t - prev.t : t;
      const sec = e.second;
      const row = {
        t, req: e.requests, failed: e.failed, bytes: e.bytes,
        rps: sec ? sec.requests_per_second : prev && dt > 0 ? Math.max(0, (e.requests - prev.req) / dt) : e.requests_per_second,
        fps: sec ? sec.errors_per_second : prev && dt > 0 ? Math.max(0, (e.failed - prev.failed) / dt) : 0,
        bps: prev && dt > 0 ? Math.max(0, ((e.bytes || 0) - (prev.bytes || 0)) / dt) : null,
        p50: sec ? sec.p50_ms : e.p50_ms, p90: sec ? sec.p90_ms : null, p95: e.p95_ms, p99: sec ? sec.p99_ms : e.p99_ms, conc: e.concurrency ?? curConc ?? (prev ? prev.conc : startConc), ttfb: e.ttfb_ms, read: e.read_ms,
      };
      // A rate over the first few milliseconds says nothing yet
      if (t >= 0.5 || prev) rows.push(row);
      if (e.done !== null && e.done !== undefined) {
        progressBar.hidden = false;
        progressBar.firstChild.style.width = `${Math.min(100, 100 * e.done)}%`;
        progressBar.lastChild.textContent = `${Math.round(100 * e.done)}% · ${secs(t)}${planned ? ` of ${secs(planned)}` : ''}${e.paused ? ' · paused' : ''}`;
      }
      if (!report) {
        const last = rows.slice(-60);
        const failedPct = share(e.failed, e.requests);
        tilesBox.replaceChildren(tiles([
          ['Request rate', rate(row.rps), `${count(e.requests)} so far`, last.map((r) => r.rps)],
          ['Duration p95', ms(e.p95_ms), `p50 ${ms(e.p50_ms)} · p99 ${ms(e.p99_ms)}`, last.map((r) => r.p95), 'ember'],
          ['Failed', pct(failedPct), `${count(e.failed)} requests`, last.map((r) => r.fps), e.failed ? 'bad' : ''],
          ['Received', `${bytes(row.bps)}/s`, `${bytes(e.bytes)} in all`, last.map((r) => r.bps), 'gold'],
          ['At once', row.conc ? count(row.conc) : '—', e.paused ? 'paused' : 'connections', last.map((r) => r.conc), 'leaf'],
          ['Elapsed', secs(t), planned ? `of ${secs(planned)}` : 'running'],
        ]));
      }
      if (e.workers) {
        workersBox.replaceChildren(section('Workers'), h('div', { class: 'card' }, h('table', {},
          h('thead', {}, h('tr', {}, h('th', {}, 'Worker'), h('th', { class: 'num' }, 'Requests'), h('th', { class: 'num' }, 'Throughput'), h('th', { class: 'num' }, 'p99'))),
          h('tbody', {}, Object.entries(e.workers).map(([name, w]) => h('tr', {}, h('td', { class: 'what' }, name), h('td', { class: 'num' }, count(w.requests)), h('td', { class: 'num' }, rate(w.requests_per_second)), h('td', { class: 'num' }, ms(w.p99_ms))))))));
      }
    }
    redraw();
  };

  const controller = new AbortController();
  leaving.push(() => controller.abort());
  follow(id, (type, e) => {
    if (type === 'start') { planned = e.planned_s; if (e.concurrency) startConc = e.concurrency; line('note', e.worker ? `${e.worker} started its share` : `started · ${[e.load, e.target].filter(Boolean).join(' · ')}`, 0); }
    else if (type === 'progress') live(e);
    else if (type === 'anomaly') { marks.push({ t: lastT, text: e.text }); line('warn', e.text, lastT); redraw(); }
    else if (type === 'steered') { if (e.concurrency) curConc = e.concurrency; showSteer(true, e.paused); line('note', `${e.what}${e.concurrency ? ` · ${e.concurrency} at once` : ''}`, lastT); }
    else if (type === 'worker_done') line(e.exit === 0 ? 'good' : 'bad', `${e.worker} ended its share${e.error ? ` · ${e.error}` : ''}`, lastT);
    else if (type === 'report') { report = e.report; showReport(e.report); }
    else if (type === 'end') {
      badge.className = `badge ${STATE[e.state] || ''}`;
      badge.textContent = e.state;
      progressBar.hidden = true;
      showSteer(false);
      line(e.state === 'done' ? 'good' : e.state === 'stopped' ? 'warn' : 'bad', `ended · ${e.state}${e.error ? ` · ${e.error}` : ''}`, lastT);
      if (e.error) verdictBox.prepend(error(e.error));
      if (e.state !== 'done' && e.state !== 'stopped') facePic.src = `/ui/img/pepe-${FACE[e.state] || 'sleeping'}.png`;
    }
  }, controller.signal);

  function showReport(r) {
    // A ramp has no verdict: its worst finding stands for one
    const RANK = ['healthy', 'degraded', 'failing'];
    const level = (r.verdict && r.verdict.level)
      || (r.findings || []).map((f) => f.level).sort((a, b) => RANK.indexOf(b) - RANK.indexOf(a))[0];
    if (level) facePic.src = `/ui/img/pepe-${FACE[level] || 'proud'}.png`;
    // The report's own anomalies are timed; the live ones were a guess
    const timed = anomalyMarks(r);
    if (timed.length) { marks.length = 0; marks.push(...timed); }
    redraw();
    const sum = r.summary;
    if (sum && !isPing) {
      const lat = sum.latency;
      const failedPct = share(sum.failed_requests, sum.total_requests);
      const recv = sum.duration_ms ? (sum.data_transfer_bytes * 1000) / sum.duration_ms : null;
      const g = r.generator || {};
      tilesBox.replaceChildren(tiles([
        ['Request rate', rate(sum.requests_per_second), `${count(sum.total_requests)} requests`, rows.map((x) => x.rps)],
        ['Duration p95', ms(lat.p95_ms), `p50 ${ms(lat.median_ms)} · p99 ${ms(lat.p99_ms)}`, rows.map((x) => x.p95), 'ember'],
        ['Failed', pct(failedPct), `${count(sum.failed_requests)} of ${count(sum.total_requests)}`, rows.map((x) => x.fps), sum.failed_requests ? 'bad' : 'good'],
        ['Received', `${bytes(recv)}/s`, `${bytes(sum.data_transfer_bytes)} in all`, rows.map((x) => x.bps), 'gold'],
        ['At once', count((rows.at(-1) && rows.at(-1).conc) || (r.target && r.target.concurrency)), g.rate_per_second ? `at ${rate(g.rate_per_second)} asked` : 'connections', rows.map((x) => x.conc), 'leaf'],
        ['Took', secs(sum.duration_ms / 1000), g.peak_busy_percent !== undefined ? `pepe ${g.peak_busy_percent}% busy at most` : ''],
      ]));
      if (limits.length) {
        const checked = limits.map((t) => checkLimit(t, sum));
        limitsBox.replaceChildren(h('div', { class: 'card limits' }, h('h2', {}, 'Thresholds'),
          h('div', { class: 'limitrows' }, checked.map((c) => h('div', { class: `limit ${c.held === false ? 'crossed' : ''}` },
            pill(c.held === null ? '?' : c.held ? 'held' : 'crossed', c.held === null ? 'faint' : c.held ? 'good' : 'bad'),
            h('code', {}, c.text), h('span', { class: 'mono faint' }, c.held === null ? '' : `measured ${c.measured}`))))));
      }
    }
    if (isRamp) {
      const cap = r.capacity;
      tilesBox.replaceChildren(tiles([
        ['Holds', r.holds_concurrency ? `${r.holds_concurrency}` : '—', 'at once, within the limits', null, 'leaf'],
        ['Capacity', cap ? `${rate(cap.requests_per_second)}` : 'not reached', cap ? (cap.extrapolated ? 'read past the ramp' : 'where the curve bent') : 'the curve hadn\'t bent'],
        ['Requests', compact(r.requests), `${count(r.failed_requests)} failed`, rows.map((x) => x.rps), r.failed_requests ? 'bad' : ''],
        ['Steps', count((r.steps || []).length), r.ended ? `ended: ${r.ended}` : ''],
        ['Took', secs(r.seconds), r.generator && r.generator.peak_busy_percent !== undefined ? `pepe ${r.generator.peak_busy_percent}% busy at most` : ''],
      ]));
    }
    if (isPing && r.targets) {
      const t = r.targets[0];
      tilesBox.replaceChildren(tiles(r.targets.length === 1 ? [
        ['Sent', count(t.sent), `${count(t.answered)} answered`],
        ['p50', ms(t.latency_ms.p50), `avg ${ms(t.latency_ms.avg)}`, rows.map((x) => x.t0)],
        ['p99', ms(t.latency_ms.p99), `max ${ms(t.latency_ms.max)}`, null, 'red'],
        ['Jitter', ms(t.latency_ms.jitter), `min ${ms(t.latency_ms.min)}`],
        ['Lost', pct(100 * (t.loss || 0)), `${count(t.timeouts)} timed out`, null, t.loss ? 'bad' : 'good'],
        ['TLS', t.tls && t.tls.cert ? `${t.tls.cert.days_left}d` : '—', t.tls && t.tls.cert ? 'left on the certificate' : (t.http_version || '')],
      ] : r.targets.map((x) => [x.target.replace(/^\w+:\/\//, '').replace(/\/$/, ''), x.answered ? ms(x.latency_ms.p50) : 'no answer', x.answered ? `p50 · p99 ${ms(x.latency_ms.p99)} · ${pct(100 * (x.loss || 0))} lost` : `${count(x.sent)} sent, ${pct(100 * (x.loss || 0))} lost`, rows.map((q) => q[`t${r.targets.indexOf(x)}`]), x.loss ? 'bad' : ''])));
      panes.Targets.replaceChildren(...r.targets.map(pingTarget));
    }
    if (sum && panes.Timings) timings(r);
    if (panes.Summary) panes.Summary.replaceChildren(...summaryOf(r));
    if (modeTab) panes[modeTab].replaceChildren(...parts(r));

    const findings = (r.verdict && r.verdict.findings) || r.findings || (r.targets || []).flatMap((t) => (t.findings || []).map((f) => ({ ...f, text: r.targets.length > 1 ? `${t.name}: ${f.text}` : f.text })));
    // The reports open as plain links, each with a ticket the server made
    // for this run alone: a page can't send the token with a link
    const links = h('span', { class: 'actions' });
    api('POST', `/runs/${encodeURIComponent(id)}/ticket`).then(({ ticket }) => {
      const link = (format, text, primary) => h('a', {
        class: `button${primary ? ' primary' : ''}`, target: '_blank', rel: 'noopener noreferrer',
        href: `/api/runs/${encodeURIComponent(id)}/report?format=${format}&ticket=${encodeURIComponent(ticket)}`,
      }, text);
      links.replaceChildren(...[!isPing ? link('html', 'HTML report ↗', true) : null, link('md', 'Markdown ↗'), link('json', 'JSON ↗'), !isPing && !isRamp ? link('csv', 'CSV ↗') : null].filter(Boolean));
    }).catch((e) => links.replaceChildren(error(e)));
    const words = { healthy: 'Healthy', degraded: 'Degraded', failing: 'Failing' };
    verdictBox.replaceChildren(h('div', { class: 'card' },
      h('h2', {}, 'Verdict'),
      level ? h('div', { class: 'verdict-head' }, pill(level, LEVEL[level]), h('strong', {}, words[level] || level)) : null,
      h('ul', { class: 'findings' }, findings.map((f) => {
        const cls = LEVEL[String(f.level).toLowerCase()] || 'note';
        return h('li', { class: cls === 'good' ? 'good' : cls === 'bad' ? 'bad' : '' }, h('span', { class: `d ${cls}` }), h('span', {}, f.text ?? [f.title, f.detail].filter(Boolean).join(': ')));
      })),
      h('div', { class: 'actions', style: 'margin-top:18px' },
        links,
        !isPing ? h('a', { class: 'button', href: `#/compare?after=${encodeURIComponent(id)}` }, 'Compare with…') : null)));
  }

  // The Timings tab: the distribution, the percentiles, the phases, and an
  // open loop's schedule
  function timings(r) {
    const lat = r.summary.latency;
    const out = [];
    const hist = histogramChart(lat.histogram_us, [['p50', lat.median_ms, 'c-leaf'], ['p95', lat.p95_ms, 'c-ember'], ['p99', lat.p99_ms, 'c-red']]);
    const qs = [[0, lat.min_ms], ...(lat.quantiles_ms || []).map((x) => [x.q, x.ms])];
    const curve = qs.length > 2 ? percentileChart([{ label: 'latency', cls: 'c-ink warm', area: true, points: qs }]) : null;
    if (hist || curve) {
      out.push(h('div', { class: 'chartgrid' },
        hist ? chartCard('Latency distribution', hist, null, `${count(r.summary.total_requests)} requests by how long each took, on a widening scale; min ${ms(lat.min_ms)}, max ${ms(lat.max_ms)}.`) : null,
        curve ? chartCard('Latency by percentile', curve, null, 'Each nine as wide as the last: the tail, spread out.') : null));
    }
    const p = lat.phases || {};
    const bars = phaseBars([
      p.dns_lookup_avg_ms ? { name: 'DNS', note: 'average, a probe a second', median: p.dns_lookup_avg_ms, cls: 'c-gold' } : null,
      p.connect ? { name: 'Connect', note: `${count(p.connect.count)} opened`, median: p.connect.median_ms, p99: p.connect.p99_ms, max: p.connect.max_ms, cls: 'c-leaf' } : null,
      p.first_byte ? { name: 'First byte', note: 'sent to headers', median: p.first_byte.median_ms, p99: p.first_byte.p99_ms, max: p.first_byte.max_ms, cls: 'c-ember' } : null,
      p.download ? { name: 'Body', note: 'headers to the end', median: p.download.median_ms, p99: p.download.p99_ms, max: p.download.max_ms, cls: 'c-ink' } : null,
      { name: 'Whole request', median: lat.median_ms, p99: lat.p99_ms, max: lat.max_ms, cls: 'c-red' },
    ]);
    if (bars) out.push(section('Where the time went'), h('div', { class: 'card' }, bars));
    const sc = r.summary.schedule;
    if (sc) {
      out.push(section('On schedule'), h('div', { class: 'card' },
        h('p', { class: 'note', style: 'margin-top:0' }, `${count(sc.paced_requests)} requests were paced; ${count(sc.late_requests)} (${pct(sc.late_percent)}) started late because every connection was busy. A user who arrived on time waited for the response time, not just the service time.`),
        trendTable([
          ['Service time', { avg: lat.avg_ms, min: lat.min_ms, med: lat.median_ms, max: lat.max_ms, p90: lat.p90_ms, p95: lat.p95_ms, p99: lat.p99_ms }],
          ['Waited for a slot', { med: sc.wait_ms.median, p90: sc.wait_ms.p90, p99: sc.wait_ms.p99, max: sc.wait_ms.max }],
          ['Response time', { med: sc.response_time_ms.median, p90: sc.response_time_ms.p90, p99: sc.response_time_ms.p99, max: sc.response_time_ms.max }],
        ])));
    }
    timingsMore.replaceChildren(...out);
  }

  // A ping target in full: its numbers, phases, network, TLS and findings
  function pingTarget(t) {
    const l = t.latency_ms || {}, p = t.phases_ms || {};
    const ph = (name, x, cls) => x ? { name, median: x.median_ms, p99: x.p99_ms, max: x.max_ms, cls } : null;
    const kv = (pairs) => h('dl', { class: 'kv' }, pairs.filter(([, v]) => v !== null && v !== undefined && v !== '').map(([k, v]) => [h('dt', {}, k), h('dd', { class: 'mono' }, String(v))]));
    const cert = t.tls && t.tls.cert;
    return h('div', { class: 'card target' },
      h('h2', {}, t.target.replace(/^\w+:\/\//, '').replace(/\/$/, ''), h('span', { class: 'faint mono small' }, ` ${t.target}`)),
      h('div', { class: 'nums inset' }, [['Sent', count(t.sent)], ['Answered', count(t.answered)], ['Lost', pct(100 * (t.loss || 0))], ['p50', ms(l.p50)], ['p95', ms(l.p95)], ['p99', ms(l.p99)], ['Jitter', ms(l.jitter)]]
        .map(([k, v]) => h('div', {}, h('small', {}, k), h('b', { class: 'small' }, v)))),
      h('div', { class: 'split' },
        h('div', {}, h('h3', { class: 'mini' }, 'Phases'), phaseBars([ph('DNS', p.dns, 'c-gold'), ph('Connect', p.connect, 'c-leaf'), ph('TLS, full', p.tls_full, 'c-blush'), ph('TLS, resumed', p.tls_resumed, 'c-blush'), ph('First byte', p.ttfb, 'c-ember'), ph('Body', p.download, 'c-ink')]) || h('p', { class: 'faint' }, 'Nothing answered.')),
        h('div', {}, h('h3', { class: 'mini' }, 'Connection'), kv([['remote', t.remote], ['local', t.local], ['http', t.http_version], ['reused', t.reused_connections], ['received', bytes(t.bytes)], ['min / avg / max', `${ms(l.min)} / ${ms(l.avg)} / ${ms(l.max)}`],
          ['tls', t.tls && (t.tls.version || t.tls.protocol)], ['cipher', t.tls && t.tls.cipher], ['certificate', cert ? `${cert.valid === false ? 'not valid · ' : ''}${cert.days_left} days left` : null], ['issuer', cert && cert.issuer], ['subject', cert && cert.subject],
          ['SLO', t.slo && (t.slo.pass ? 'held' : `${t.slo.violations} over`)]]))),
      t.status_codes ? h('div', { style: 'margin-top:18px' }, h('h3', { class: 'mini' }, 'Answers'), statusBar(t.status_codes)) : null,
      t.failures && Object.keys(t.failures).length ? h('div', { style: 'margin-top:18px' }, h('h3', { class: 'mini' }, 'Failures'), h('ul', { class: 'findings' }, Object.entries(t.failures).map(([c, n]) => h('li', { class: 'bad' }, h('span', { class: 'd bad' }), h('span', {}, `${count(n)} × ${c}`))))) : null,
      (t.findings || []).length ? h('ul', { class: 'findings', style: 'margin-top:18px' }, t.findings.map((f) => {
        const cls = LEVEL[f.level] || 'note';
        return h('li', { class: cls === 'good' ? 'good' : cls === 'bad' ? 'bad' : '' }, h('span', { class: `d ${cls}` }), h('span', {}, f.text));
      })) : null);
  }
}

// k6's trend table: avg, min, med, max, p90, p95, p99 a row
function trendTable(rows) {
  const cols = ['avg', 'min', 'med', 'max', 'p90', 'p95', 'p99'];
  return h('div', { class: 'table' }, h('table', { class: 'trends' },
    h('thead', {}, h('tr', {}, h('th', {}, 'Metric'), cols.map((c) => h('th', { class: `num ${['avg', 'min', 'p90'].includes(c) ? 'hide-narrow' : ''}` }, c)))),
    h('tbody', {}, rows.filter(Boolean).map(([name, v, note]) => h('tr', {},
      h('td', {}, h('span', { class: 'what' }, name), note ? h('span', { class: 'sub' }, note) : null),
      cols.map((c) => h('td', { class: `num ${['avg', 'min', 'p90'].includes(c) ? 'hide-narrow' : ''} ${c === 'p99' ? 'strong' : ''}` }, v[c] === undefined || v[c] === null ? h('span', { class: 'faint' }, '—') : ms(v[c]))))))));
}

// k6's counters: how many, how many a second, and their share
function counterTable(rows, seconds) {
  return h('div', { class: 'table' }, h('table', {},
    h('thead', {}, h('tr', {}, h('th', {}, 'Metric'), h('th', { class: 'num' }, 'count'), h('th', { class: 'num' }, 'rate'), h('th', { class: 'num hide-narrow' }, 'share'))),
    h('tbody', {}, rows.filter(Boolean).map(([name, n, of, fmt, cls]) => h('tr', {},
      h('td', { class: 'what' }, name),
      h('td', { class: `num ${cls || ''}` }, (fmt || count)(n)),
      h('td', { class: 'num' }, seconds ? (fmt === bytes ? `${bytes(n / seconds)}/s` : rate(n / seconds)) : '—'),
      h('td', { class: 'num hide-narrow faint' }, of ? pct(share(n, of)) : ''))))));
}

// The Summary tab: everything the report counted
function summaryOf(r) {
  const out = [];
  const sum = r.summary;
  if (sum) {
    const lat = sum.latency, p = lat.phases || {};
    const sp = (x) => x && { med: x.median_ms, p99: x.p99_ms, max: x.max_ms };
    const p999 = (lat.quantiles_ms || []).find((x) => x.q === 99.9);
    out.push(section('Trends'), h('div', { class: 'card' }, trendTable([
      ['Request duration', { avg: lat.avg_ms, min: lat.min_ms, med: lat.median_ms, max: lat.max_ms, p90: lat.p90_ms, p95: lat.p95_ms, p99: lat.p99_ms }, `std dev ${ms(lat.std_dev_ms)}${p999 ? ` · p99.9 ${ms(p999.ms)}` : ''}`],
      sum.schedule ? ['Response time, on schedule', { med: sum.schedule.response_time_ms.median, p90: sum.schedule.response_time_ms.p90, p99: sum.schedule.response_time_ms.p99, max: sum.schedule.response_time_ms.max }, 'counted from each request\'s slot'] : null,
      p.first_byte ? ['Waiting (first byte)', sp(p.first_byte), `${count(p.first_byte.count)} requests`] : null,
      p.download ? ['Receiving (body)', sp(p.download)] : null,
      p.connect ? ['Connecting', sp(p.connect), `${count(p.connect.count)} connections opened`] : null,
      p.dns_lookup_avg_ms ? ['DNS lookup', { avg: p.dns_lookup_avg_ms }, 'a probe a second'] : null,
    ])));
    const s = sum.duration_ms / 1000;
    const g = r.generator || {};
    out.push(section('Counters'), h('div', { class: 'card' }, counterTable([
      ['Requests', sum.total_requests, null],
      ['Succeeded', sum.successful_requests, sum.total_requests, null, 'good'],
      ['Failed', sum.failed_requests, sum.total_requests, null, sum.failed_requests ? 'bad' : ''],
      sum.http_errors ? ['HTTP errors', sum.http_errors, sum.total_requests, null, 'bad'] : null,
      sum.timeout_errors ? ['Timeouts', sum.timeout_errors, sum.total_requests, null, 'bad'] : null,
      sum.connection_errors ? ['Connection errors', sum.connection_errors, sum.total_requests, null, 'bad'] : null,
      ['Data received', sum.data_transfer_bytes, null, bytes],
      sum.schedule ? ['Started late', sum.schedule.late_requests, sum.schedule.paced_requests, null, sum.schedule.late_requests ? 'warn' : ''] : null,
      g.rate_missed ? ['Rate missed', g.rate_missed, null, null, 'warn'] : null,
      g.warmup_requests ? ['Warm-up (not counted)', g.warmup_requests, null] : null,
    ], s)));
    out.push(h('div', { class: 'two' },
      h('div', {}, section('Answers'), h('div', { class: 'card' }, statusBar(sum.status_codes) || h('p', { class: 'faint' }, 'No answer came.'))),
      h('div', {}, section('pepe itself'), h('div', { class: 'card' }, generatorOf(g, sum)))));
    if ((sum.failures || []).length) {
      out.push(section('Failures'), h('div', { class: 'card' }, h('div', { class: 'failures' }, sum.failures.map((f) => h('details', { class: 'failure' },
        h('summary', {}, h('span', { class: 'what mono' }, f.cause), h('span', { class: 'mono bad' }, `${count(f.count)} · ${pct(share(f.count, sum.total_requests))}`)),
        f.example_body ? h('pre', { class: 'body' }, f.example_body) : h('p', { class: 'faint' }, 'No body.'))))));
    }
    if ((sum.slowest_requests || []).length) {
      const spent = sum.slowest_requests.some((x) => x.spent && x.spent.length);
      out.push(section('The slowest requests'), h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
        h('thead', {}, h('tr', {}, h('th', { class: 'num' }, 'At'), h('th', { class: 'num' }, 'Took'), h('th', { class: 'num' }, 'Status'), h('th', { class: 'num hide-narrow' }, 'Server said'), h('th', {}, 'Request id · trace'), spent ? h('th', { class: 'hide-narrow' }, 'Spent in') : null)),
        h('tbody', {}, sum.slowest_requests.map((x) => h('tr', {},
          h('td', { class: 'num faint' }, secs(x.at_s)), h('td', { class: 'num strong' }, ms(x.latency_ms)), h('td', { class: 'num' }, x.status),
          h('td', { class: 'num hide-narrow' }, x.server_ms === undefined ? '—' : ms(x.server_ms)),
          h('td', { class: 'mono small' }, [x.request_id, x.trace_id].filter(Boolean).join(' · ') || h('span', { class: 'faint' }, '—')),
          spent ? h('td', { class: 'mono small hide-narrow' }, (x.spent || []).map((y) => `${y.name} ${ms(y.ms)}`).join(' · ')) : null)))))));
    }
    const st = sum.server_timing;
    if (st && st.segments) {
      out.push(section('Server-Timing'), h('div', { class: 'card' },
        h('p', { class: 'note', style: 'margin-top:0' }, `What the server said it spent, from ${count(st.responses)} responses.`),
        trendTable([['total', { med: st.total.median_ms, p90: st.total.p90_ms, p99: st.total.p99_ms }, `${count(st.total.count)} responses`],
          ...Object.entries(st.segments).map(([k, v]) => [k, { med: v.median_ms, p90: v.p90_ms, p99: v.p99_ms }, `${count(v.count)} responses`])])));
    }
    if (r.spans && (r.spans.operations || []).length) {
      out.push(section('Inside the service (OpenTelemetry)'), h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
        h('thead', {}, h('tr', {}, h('th', {}, 'Operation'), h('th', { class: 'num' }, 'median'), h('th', { class: 'num' }, 'calls/req'), h('th', { style: 'min-width:140px' }, 'share of the time'))),
        h('tbody', {}, r.spans.operations.map((o) => h('tr', {},
          h('td', {}, h('span', { class: 'what' }, o.name), o.service ? h('span', { class: 'sub' }, o.service) : null),
          h('td', { class: 'num' }, ms(o.median_ms)), h('td', { class: 'num' }, o.calls_per_request ?? '—'),
          h('td', {}, barCell(o.share_percent || 0, 100, pct(o.share_percent))))))))));
    }
    if ((sum.anomalies || []).length) {
      out.push(section('What changed as it went'), h('div', { class: 'card' }, h('ul', { class: 'feed' }, sum.anomalies.map((a) => h('li', {}, h('span', { class: 'mono faint at' }, secs(a.at_s)), h('span', { class: 'd warn' }), h('span', {}, a.text))))));
    }
  }
  if (r.steps && r.holds_concurrency !== undefined) {
    out.push(section('Each step'), h('div', { class: 'card' }, trendTable(r.steps.map((x) => [`${x.concurrency} at once`,
      { avg: x.avg_ms, min: x.min_ms, med: x.median_ms, max: x.max_ms, p90: x.p90_ms, p95: x.p95_ms, p99: x.p99_ms },
      `${rate(x.requests_per_second)} · ${pct(x.error_rate_percent)} errors${x.note ? ` · ${x.note}` : ''}`]))));
    out.push(section('pepe itself'), h('div', { class: 'card' }, generatorOf(r.generator || {}, null)));
  }
  return out;
}

// Whether pepe, rather than the target, was the limit
function generatorOf(g, sum) {
  const busy = g.peak_busy_percent;
  return h('div', {},
    busy !== undefined ? h('div', { class: 'meter' },
      h('div', { class: 'meterhead' }, h('span', {}, 'Busiest thread, at its busiest second'), h('b', { class: busy >= 90 ? 'bad' : busy >= 70 ? 'warn' : '' }, `${busy}%`)),
      h('div', { class: 'track' }, h('i', { class: `fillbar ${busy >= 90 ? 'c-red' : busy >= 70 ? 'c-gold' : 'c-leaf'}`, style: `width:${busy}%` })),
      h('p', { class: 'note' }, busy >= 90 ? 'Near 100, pepe was the limit, not the target: run again with --threads auto.' : 'pepe had room to spare: the numbers are the target\'s.')) : null,
    h('dl', { class: 'kv' },
      [['threads', g.threads], ['rate asked', g.rate_per_second ? rate(g.rate_per_second) : null], ['rate made', g.rate_per_second && sum ? rate(sum.requests_per_second) : null],
        ['warm-up', g.warmup_s ? `${secs(g.warmup_s)}, ${count(g.warmup_requests)} requests` : null]]
        .filter(([, v]) => v !== null && v !== undefined).map(([k, v]) => [h('dt', {}, k), h('dd', { class: 'mono' }, String(v))])));
}

// What a report has besides its totals, by mode: each endpoint, step, URL
// or ramp step, with bars to read it at a glance
function parts(report) {
  const codes = (sc) => h('span', { class: 'codechips' }, Object.entries(sc || {}).map(([c, n]) => h('span', { class: `codechip s${String(c)[0]}` }, `${c} `, h('b', {}, compact(n)))));
  const table = (title, head, rows, note) => rows.length ? [section(title), h('div', { class: 'card' },
    note ? h('p', { class: 'note', style: 'margin:0 0 14px' }, note) : null,
    h('div', { class: 'table' }, h('table', {}, h('thead', {}, h('tr', {}, head.map(([label, num, narrow]) => h('th', { class: `${num ? 'num' : ''} ${narrow ? 'hide-narrow' : ''}` }, label)))),
      h('tbody', {}, rows.map((cells) => h('tr', {}, cells.map((c, i) => h('td', { class: `${head[i][1] ? 'num' : ''} ${head[i][2] ? 'hide-narrow' : ''}` }, c))))))))] : [];
  const many = (list) => {
    const p99 = Math.max(...list.map((x) => x.p99_ms || 0));
    const req = Math.max(...list.map((x) => x.requests || 0));
    return (x) => [barCell(x.requests || 0, req, count(x.requests), 'quiet'), rate(x.requests_per_second), ms(x.median_ms), barCell(x.p99_ms || 0, p99, ms(x.p99_ms), x.p99_ms === p99 ? 'hot' : ''),
      h('span', { class: x.failed_requests ? 'bad' : '' }, count(x.failed_requests)), codes(x.status_codes)];
  };
  const usual = [['Requests'], ['req/s', 1, 1], ['Median', 1], ['p99'], ['Failed', 1], ['Answers', 0, 1]];
  if (report.endpoints) {
    const row = many(report.endpoints);
    return table('Endpoints', [['Endpoint'], ...usual], report.endpoints.map((e) => [h('span', { class: 'mono' }, e.endpoint), ...row(e)]), `${report.endpoints.length} endpoints; the slowest p99 is marked.`);
  }
  if (report.flow) {
    const f = report.flow;
    const row = many(f.steps);
    return [
      h('div', { class: 'funnel' }, f.steps.map((x, i) => h('div', {}, h('small', {}, `step ${i + 1}`), h('b', {}, x.step), h('span', { class: 'mono' }, `${count(x.requests)} · ${ms(x.p99_ms)} p99`)))),
      ...table('Steps', [['Step'], ...usual], f.steps.map((x) => [h('span', { class: 'what' }, x.step), ...row(x)]),
        `${count(f.chains_started)} chains started, ${count(f.chains_completed)} completed (${pct(share(f.chains_completed, f.chains_started))}).`)];
  }
  if (report.replay) {
    const r = report.replay;
    const urls = r.urls.slice(0, 50);
    const row = many(urls);
    const top = Math.max(...urls.map((u) => u.share_in_log || 0));
    return table('URLs', [['URL'], ['Share of the log', 0, 1], ...usual], urls.map((u) => [h('span', { class: 'mono' }, `${u.method} ${u.url}`), barCell(u.share_in_log || 0, top, pct(100 * u.share_in_log), 'quiet'), ...row(u)]),
      `${count(r.replayed_from_log)} of the log's ${count(r.requests_in_log)} requests, ${count(r.distinct_urls)} URLs${r.urls.length > 50 ? ', the 50 most sent shown' : ''}.`);
  }
  if (report.steps && report.holds_concurrency !== undefined) {
    const chart = capacityChart(report.steps, report.holds_concurrency);
    const top = Math.max(...report.steps.map((x) => x.requests_per_second || 0));
    return [
      chart ? h('div', { class: 'chartgrid one' }, chartCard('The curve: requests a second (bars) and p99 (line), by how many at once', chart,
        [{ cls: 'c-ember', label: 'req/s' }, { cls: 'c-red', label: 'p99' }], report.holds_concurrency ? `It held ${report.holds_concurrency} at once (shaded).` : 'No step held.')) : null,
      ...table('Steps', [['At once', 1], ['Requests', 1, 1], ['req/s'], ['Median', 1], ['p99', 1], ['Errors', 1], ['', 0, 1]],
        report.steps.map((x) => [h('b', {}, x.concurrency), count(x.requests), barCell(x.requests_per_second || 0, top, rate(x.requests_per_second), x.concurrency === report.holds_concurrency ? 'held' : ''),
          ms(x.median_ms), ms(x.p99_ms), h('span', { class: x.error_rate_percent ? 'bad' : '' }, pct(x.error_rate_percent)), h('span', { class: 'faint' }, x.note || '')])),
      ...(report.findings || []).length ? [section('Findings'), h('div', { class: 'card' }, h('ul', { class: 'findings' }, report.findings.map((f) => {
        const cls = LEVEL[String(f.level).toLowerCase()] || 'note';
        return h('li', { class: cls === 'good' ? 'good' : cls === 'bad' ? 'bad' : '' }, h('span', { class: `d ${cls}` }), h('span', {}, h('b', {}, f.title), f.detail ? ` · ${f.detail}` : ''));
      })))] : [],
    ].filter(Boolean);
  }
  return [];
}

// ─── A new run ───────────────────────────────────────────────────────────

// What each mode is, as the front page names them
const MODES = [
  ['run', 'pepe URL', 'Load test', 'One URL, at a concurrency or a rate'],
  ['api', 'pepe api', 'OpenAPI', 'Every endpoint of a spec, or the ones picked'],
  ['flow', 'pepe flow', 'Flow', 'Steps in order, each fed by the last'],
  ['replay', 'pepe replay', 'Replay', 'An access log\'s URLs, in their proportions'],
  ['ping', 'pepe ping', 'Ping', 'A request now and then, split by phase'],
  ['ramp', 'pepe ramp', 'Ramp', 'More at once, step by step, to the capacity'],
];

// A file in the project the run can name: one uploaded here, chosen from
// those uploaded before, or one already in the project typed by its path
function filePicker(label, accept, hint, onChosen) {
  const select = h('select', { 'aria-label': label }, h('option', { value: '' }, '— choose or upload —'));
  const upload = h('input', { type: 'file', accept, style: 'width:auto' });
  const said = h('div', { class: 'hint' }, hint || '');
  const load = async (pick) => {
    const { files } = await api('GET', '/files');
    select.replaceChildren(h('option', { value: '' }, '— choose or upload —'), ...files.map((f) => h('option', { value: f.path }, `${f.name} · ${size(f.bytes)}`)));
    if (pick) select.value = pick;
  };
  load().catch(() => {});
  upload.addEventListener('change', async () => {
    const file = upload.files[0];
    if (!file) return;
    const name = file.name.replace(/[^A-Za-z0-9._-]/g, '-').replace(/^[.-]+/, '').slice(0, 64) || 'file';
    said.textContent = `Uploading ${file.name}…`;
    try {
      const res = await fetch(`/api/files/${encodeURIComponent(name)}`, { method: 'PUT', headers: { Authorization: `Bearer ${token}` }, body: file });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error);
      await load(data.path);
      said.textContent = `Uploaded as ${data.path}`;
      onChosen && onChosen(data.path);
    } catch (e) { said.textContent = e.message; said.className = 'hint bad'; }
  });
  select.addEventListener('change', () => onChosen && onChosen(select.value));
  const field = h('div', { class: 'field' }, h('label', {}, label), h('div', { class: 'actions', style: 'margin-top:6px' }, select, upload), said);
  field.value = () => select.value || undefined;
  field.set = (path) => load(path);
  return field;
}

function size(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// A flow's steps, edited as fields and written as the flow file pepe reads
function flowEditor() {
  const steps = [];
  const list = h('div', {});
  const vars = h('textarea', { placeholder: 'host = https://shop.example.com', 'aria-label': 'Values every chain starts with' });
  const toml = (s) => JSON.stringify(s); // a TOML basic string is a JSON one
  const add = (seed = {}) => {
    const f = {
      name: h('input', { placeholder: `step ${steps.length + 1}`, value: seed.name || '' }),
      method: h('input', { placeholder: 'GET', value: seed.method || '' }),
      url: h('input', { placeholder: '{{host}}/login', value: seed.url || '' }),
      headers: h('textarea', { placeholder: 'Authorization: Bearer {{token}}' }),
      body: h('textarea', { placeholder: '{"user": "demo"}' }),
      expect: h('input', { type: 'number', placeholder: 'any 2xx', value: seed.expect || '' }),
      capture: h('textarea', { placeholder: 'token = json:$.token\nsession = header:Set-Cookie' }),
    };
    const card = h('div', { class: 'step' },
      h('span', { class: 'n' }, `STEP ${steps.length + 1}`),
      h('div', { class: 'actions' }, h('button', { type: 'button', class: 'danger', onclick: () => { steps.splice(steps.indexOf(f), 1); card.remove(); } }, 'Remove')),
      h('div', { class: 'fields' }, field('Name', f.name), field('Method', f.method), field('Expect', f.expect)),
      field('URL', f.url, '{{holes}} are filled from the values above, the CSV and earlier captures'),
      h('div', { class: 'fields' }, field('Headers, one a line', f.headers), field('Body', f.body)),
      field('Capture, one a line', f.capture, 'name = json:$.path · header:Name · regex:(…) · body'));
    steps.push(f);
    list.append(card);
  };
  add({ name: 'login', method: 'POST' });
  const write = () => {
    const out = [];
    const v = vars.value.split('\n').map((l) => l.trim()).filter(Boolean);
    if (v.length) {
      out.push('[vars]');
      for (const line of v) {
        const [k, ...rest] = line.split('=');
        if (!/^[A-Za-z0-9_-]+$/.test(k.trim())) throw new Error(`a value's name is letters, digits, - and _: ${k.trim()}`);
        out.push(`${k.trim()} = ${toml(rest.join('=').trim())}`);
      }
      out.push('');
    }
    for (const f of steps) {
      if (!f.url.value.trim()) throw new Error('every step has a URL');
      out.push('[[step]]');
      if (f.name.value.trim()) out.push(`name = ${toml(f.name.value.trim())}`);
      if (f.method.value.trim()) out.push(`method = ${toml(f.method.value.trim().toUpperCase())}`);
      out.push(`url = ${toml(f.url.value.trim())}`);
      const hs = f.headers.value.split('\n').map((l) => l.trim()).filter(Boolean);
      if (hs.length) out.push(`headers = [${hs.map(toml).join(', ')}]`);
      if (f.body.value) out.push(`body = ${toml(f.body.value)}`);
      if (f.expect.value) out.push(`expect = ${Number(f.expect.value)}`);
      const caps = f.capture.value.split('\n').map((l) => l.trim()).filter(Boolean).map((line) => {
        const [k, ...rest] = line.split('=');
        if (!/^[A-Za-z0-9_-]+$/.test(k.trim())) throw new Error(`a capture's name is letters, digits, - and _: ${k.trim()}`);
        return `${k.trim()} = ${toml(rest.join('=').trim())}`;
      });
      if (caps.length) out.push(`capture = { ${caps.join(', ')} }`);
      out.push('');
    }
    return out.join('\n');
  };
  const el = h('div', {},
    field('Values every chain starts with, one a line', vars, 'name = value; a CSV below gives each chain a row of its own'),
    list, h('button', { type: 'button', onclick: () => add() }, '+ Add a step'));
  el.write = write;
  return el;
}

async function viewNew(_, query) {
  const [workersData, testData] = await Promise.all([
    api('GET', '/workers').catch(() => ({ workers: [] })),
    query.test ? api('GET', `/tests/${encodeURIComponent(query.test)}`) : null,
  ]);
  const c = (testData && testData.config) || {};
  let mode = query.mode && MODES.some((m) => m[0] === query.mode) ? query.mode : (c.ramp ? 'ramp' : c.ping ? 'ping' : 'run');
  const inp = (attrs) => h('input', attrs);
  const lines = (t) => t.split('\n').map((x) => x.trim()).filter(Boolean);
  const num = (input) => (input.value === '' ? undefined : Number(input.value));
  const str = (input) => input.value.trim() || undefined;
  const check = (label, hint) => {
    const box = h('input', { type: 'checkbox' });
    const el = h('label', { class: 'check' }, box, h('span', {}, label, hint ? h('span', { class: 'hint' }, hint) : null));
    el.box = box;
    return el;
  };

  // Every field, whichever mode shows it
  const f = {
    url: inp({ type: 'url', placeholder: 'https://api.example.com/health', value: c.url || '' }),
    targets: h('textarea', { placeholder: 'https://api.example.com/health\nhttps://cdn.example.com/\ndb.internal:5432' }),
    method: inp({ placeholder: 'GET', value: c.method || '' }),
    headers: h('textarea', { placeholder: 'Authorization: Bearer …\nAccept: application/json' }),
    body: h('textarea', { placeholder: '{"a": 1}' }),
    requests: inp({ type: 'number', min: 1, placeholder: '200', value: c.requests || '' }),
    duration: inp({ placeholder: '30s', value: c.duration || '' }),
    concurrency: inp({ type: 'number', min: 1, placeholder: 'one per core', value: c.concurrency || '' }),
    rate: inp({ type: 'number', min: 0, step: 'any', placeholder: 'as fast as it answers', value: c.rate || '' }),
    warmup: inp({ placeholder: 'none', value: c.warmup || '' }),
    failIf: h('textarea', { placeholder: 'p99 > 300ms\nerrors > 1%' }),
    timeout: inp({ type: 'number', min: 1, placeholder: '20', value: c.timeout || '' }),
    threads: inp({ placeholder: '1, or auto', value: c.threads || '' }),
    userAgent: inp({ placeholder: 'pepe/…', value: c['user-agent'] || '' }),
    proxy: inp({ placeholder: 'http://proxy:3128', value: c.proxy || '' }),
    insecure: check('Accept any TLS certificate'),
    noKeepalive: check('A new connection for every request'),
    noRedirects: check('Take a redirect as the answer', 'rather than follow it'),
    noCompression: check('Don\'t ask for a compressed body'),
    every: inp({ placeholder: '1s' }),
    slo: inp({ placeholder: 'total=500,ttfb=200' }),
    keepAlive: check('Keep the connection', 'as a browser would: DNS, connect and TLS only in the first'),
    allIps: check('Every address the name resolves to'),
    tcp: check('TCP connect only', 'host:port targets'),
    from: inp({ type: 'number', min: 1, placeholder: '1' }),
    to: inp({ type: 'number', min: 1, placeholder: '100' }),
    step: inp({ type: 'number', min: 1, placeholder: '10' }),
    stepFor: inp({ placeholder: '5s' }),
    until: h('textarea', { placeholder: 'p99 > 500ms\nerrors > 1%' }),
    server: inp({ placeholder: 'the spec\'s own' }),
    auth: h('textarea', { placeholder: 'bearer=TOKEN\napiKey=KEY' }),
    set: h('textarea', { placeholder: 'id=1,2,3' }),
    includeWrites: check('Include POST, PUT, PATCH and DELETE', 'off: only reads are sent'),
    baseUrl: inp({ type: 'url', placeholder: 'https://staging.example.com' }),
  };
  f.headers.value = (c.headers || []).join('\n');
  f.body.value = c.body || '';
  if (c.insecure) f.insecure.box.checked = true;

  const curl = h('textarea', { placeholder: 'curl -X POST https://api.example.com/orders -H \'Authorization: Bearer …\' -d \'{"id": 1}\'', 'aria-label': 'A curl command' });
  const curlSaid = h('div', {});
  const fromCurl = async () => {
    curlSaid.replaceChildren();
    try {
      const { args, notes } = await api('POST', '/curl', { command: curl.value });
      f.url.value = args.url || '';
      f.method.value = args.method && args.method !== 'GET' ? args.method : '';
      f.headers.value = (args.headers || []).join('\n');
      f.body.value = args.body || '';
      if (args.timeout) f.timeout.value = args.timeout;
      if (args.user_agent) f.userAgent.value = args.user_agent;
      if (args.proxy) f.proxy.value = args.proxy;
      f.insecure.box.checked = !!args.insecure;
      f.noKeepalive.box.checked = !!args.no_keepalive;
      f.noRedirects.box.checked = !!args.no_redirects;
      curlSaid.append(h('div', { class: 'hint good' }, '✓ Filled in from the command.'), ...notes.map((n) => h('div', { class: 'hint warn' }, n)));
    } catch (e) { curlSaid.append(error(e)); }
  };

  // Files and the spec's endpoints
  const opsBox = h('div', {});
  let specOps = [];
  const showSpec = async (path) => {
    opsBox.replaceChildren();
    specOps = [];
    if (!path) return;
    try {
      const spec = await api('GET', `/spec?file=${encodeURIComponent(path)}`);
      const rows = spec.operations.map((o) => {
        const box = h('input', { type: 'checkbox', checked: o.method === 'GET', 'aria-label': `${o.method} ${o.path}` });
        specOps.push([box, `${o.method} ${o.path}`]);
        return h('tr', { class: 'link', onclick: (e) => { if (e.target !== box) box.checked = !box.checked; } },
          h('td', { style: 'width:28px' }, box), h('td', { class: 'mono', style: 'width:72px' }, h('b', {}, o.method)), h('td', { class: 'mono' }, o.path), h('td', { class: 'hide-narrow faint' }, o.tag), h('td', { class: 'hide-narrow' }, o.summary));
      });
      const all = (on) => specOps.forEach(([b]) => (b.checked = on));
      opsBox.append(
        h('div', { class: 'plan', style: 'margin:4px 0 16px' }, h('b', {}, spec.title || 'The spec'), ` ${spec.version || ''}`, h('br'),
          h('span', { class: 'k' }, `${spec.base_url || 'no server named'} · ${spec.operations.length} operations`)),
        h('div', { class: 'actions', style: 'margin-bottom:12px' }, h('button', { type: 'button', class: 'quiet', onclick: () => all(true) }, 'All'), h('button', { type: 'button', class: 'quiet', onclick: () => all(false) }, 'None'),
          h('button', { type: 'button', class: 'quiet', onclick: () => specOps.forEach(([b, k]) => (b.checked = k.startsWith('GET '))) }, 'Reads only')),
        h('div', { class: 'table', style: 'margin-bottom:16px' }, h('table', {}, h('tbody', {}, rows))),
        spec.auth.length ? h('p', { class: 'hint' }, `It asks for auth: ${spec.auth.join(', ')}. Give it below as name=value.`) : h('span'));
    } catch (e) { opsBox.append(error(e)); }
  };
  const specFile = filePicker('The spec', '.json,.yaml,.yml', 'An OpenAPI 3 or Swagger 2 file, JSON or YAML', showSpec);
  const flowFile = filePicker('A flow file', '.toml', 'The steps as pepe flow reads them');
  const logFile = filePicker('The access log', '.log,.gz,.txt', 'nginx\'s or Apache\'s combined format, as it is (gzip too)');
  const varsFile = filePicker('Values from a CSV', '.csv', 'Each request (or chain) takes the next row: its first line names the {{holes}}');
  const flowSteps = flowEditor();
  let flowFrom = 'steps';
  const flowSwitch = h('div', { class: 'actions', style: 'margin-bottom:18px' });

  const picks = workersData.workers.filter((w) => !w.busy).map((w) => {
    const box = h('input', { type: 'checkbox', value: w.name });
    return h('label', { class: 'check', style: 'display:inline-flex;margin-right:18px' }, box, h('span', { class: 'mono' }, w.name));
  });

  // What each mode asks for
  const request = (withBody = true) => card('The request',
    h('div', { class: 'narrow' }, field('Method', f.method)),
    h('div', { class: withBody ? 'fields' : '' }, field('Headers, one a line', f.headers), withBody ? field('Body', f.body) : null));
  const load = (unit = 'Requests') => card('How much',
    h('div', { class: 'fields' }, field(unit, f.requests), field('Or for', f.duration, '30s, 5m'), field('At once', f.concurrency), field('A second', f.rate, 'an arrival rate, as users arrive'), field('Warm-up', f.warmup, 'sent, not counted')),
    picks.length && mode === 'run' ? field('Share it out to', h('div', {}, picks), 'workers that joined and are free') : null);
  const limits = () => card('Limits', field('Fail if, one a line', f.failIf, 'the run fails, exit 4, when one is crossed: p99 > 300ms, errors > 1%, rps < 500'));
  const advanced = (withBody = true) => h('div', { class: 'card flat' }, h('details', {}, h('summary', {}, 'More options'),
    h('div', { class: 'fields' }, field('Timeout, seconds', f.timeout), mode !== 'ping' && mode !== 'ramp' ? field('Threads', f.threads) : null, field('User-Agent', f.userAgent), field('Proxy', f.proxy, 'held to --allow-host like a target')),
    f.insecure, mode !== 'ping' ? [f.noKeepalive, f.noRedirects, withBody ? f.noCompression : null] : null));

  const body = h('div', {});
  const tabs = h('div', { class: 'modes', role: 'tablist' }, MODES.map(([m, cmd, label, about]) =>
    h('button', { type: 'button', role: 'tab', 'data-mode': m, onclick: () => { mode = m; draw(); } }, h('code', {}, cmd), h('strong', {}, label), h('p', {}, about))));
  const draw = () => {
    for (const a of tabs.querySelectorAll('button')) {
      a.classList.toggle('on', a.dataset.mode === mode);
      a.setAttribute('aria-selected', String(a.dataset.mode === mode));
    }
    let parts;
    if (mode === 'run') {
      parts = [
        card('Where', field('URL', f.url), h('details', {}, h('summary', {}, 'Or paste a curl command'), curl,
          h('div', { class: 'actions', style: 'margin-top:10px' }, h('button', { type: 'button', onclick: fromCurl }, 'Fill in from it')), curlSaid)),
        request(), load(), limits(), card('Values', varsFile), advanced()];
    } else if (mode === 'api') {
      parts = [card('The spec', specFile, opsBox, h('div', { class: 'fields' }, field('Server', f.server, 'its URL instead of the spec\'s')), f.includeWrites,
        h('div', { class: 'fields', style: 'margin-top:14px' }, field('Auth, one a line', f.auth, 'scheme=value, as the spec names it'), field('Values, one a line', f.set, 'name=1,2,3: rotated through'))),
      request(), load(), limits(), advanced()];
    } else if (mode === 'flow') {
      flowSwitch.replaceChildren(
        h('button', { type: 'button', class: flowFrom === 'steps' ? 'primary' : 'quiet', onclick: () => { flowFrom = 'steps'; draw(); } }, 'Write the steps'),
        h('button', { type: 'button', class: flowFrom === 'file' ? 'primary' : 'quiet', onclick: () => { flowFrom = 'file'; draw(); } }, 'Use a flow file'));
      parts = [card('The steps', flowSwitch, flowFrom === 'file' ? flowFile : flowSteps),
        load('Chains'), limits(), card('Values', varsFile), advanced()];
    } else if (mode === 'replay') {
      parts = [card('The log', logFile, field('Send to', f.baseUrl, 'the log\'s paths, on this server'), f.includeWrites), load(), limits(), advanced(false)];
    } else if (mode === 'ping') {
      parts = [card('Targets', field('One a line', f.targets, 'URLs, or host:port with TCP only')),
        card('How often', h('div', { class: 'fields' }, field('Pings', f.requests), field('Or for', f.duration), field('One every', f.every, '1s'), field('SLO', f.slo, 'exit 4 when broken'))),
        request(false), card('How', f.keepAlive, f.allIps, f.tcp), advanced(false)];
    } else {
      parts = [card('Where', field('URL', f.url)),
        card('The steps', h('div', { class: 'fields' }, field('From, at once', f.from), field('Up to', f.to), field('In steps of', f.step), field('Each step for', f.stepFor, '5s'))),
        card('Stop at', field('Until, one a line', f.until, 'the capacity is where one is crossed: p99 > 500ms')), request(), limits(), card('Values', varsFile), advanced()];
    }
    body.replaceChildren(...parts.flat().filter(Boolean));
  };

  // The run's arguments, as POST /api/runs takes them
  const args = async () => {
    const b = { mode };
    const hs = lines(f.headers.value);
    if (hs.length) b.headers = hs;
    if (str(f.method)) b.method = str(f.method).toUpperCase();
    if (f.body.value.trim() && mode !== 'ping' && mode !== 'replay') b.body = f.body.value;
    if (num(f.timeout)) b.timeout = num(f.timeout);
    if (str(f.userAgent)) b.user_agent = str(f.userAgent);
    if (str(f.proxy)) b.proxy = str(f.proxy);
    if (f.insecure.box.checked) b.insecure = true;
    if (mode !== 'ping') {
      if (f.noKeepalive.box.checked) b.no_keepalive = true;
      if (f.noRedirects.box.checked) b.no_redirects = true;
      if (f.noCompression.box.checked) b.no_compression = true;
    }
    const fails = lines(f.failIf.value);
    const loadArgs = () => {
      if (num(f.requests)) b.requests = num(f.requests);
      if (str(f.duration)) b.duration = str(f.duration);
      if (num(f.concurrency)) b.concurrency = num(f.concurrency);
      if (num(f.rate)) b.rate = num(f.rate);
      if (str(f.warmup)) b.warmup = str(f.warmup);
      if (str(f.threads)) b.threads = /^\d+$/.test(str(f.threads)) ? Number(str(f.threads)) : str(f.threads);
      if (fails.length) b.fail_if = fails;
    };
    if (mode === 'run') {
      b.url = f.url.value.trim();
      loadArgs();
      if (varsFile.value()) b.vars = varsFile.value();
      const on = picks.map((p) => p.querySelector('input')).filter((i) => i.checked).map((i) => i.value);
      if (on.length) b.workers = on;
    } else if (mode === 'api') {
      b.spec = specFile.value();
      if (!b.spec) throw new Error('choose or upload a spec');
      const chosen = specOps.filter(([box]) => box.checked).map(([, key]) => key);
      if (specOps.length && !chosen.length) throw new Error('pick at least one endpoint');
      if (specOps.length && chosen.length === specOps.length) b.all = true;
      else b.only = chosen;
      if (str(f.server)) b.server = str(f.server);
      const auth = lines(f.auth.value);
      if (auth.length) b.auth = auth;
      const set = lines(f.set.value);
      if (set.length) b.set = set;
      if (f.includeWrites.box.checked || chosen.some((k) => !/^(GET|HEAD|OPTIONS) /.test(k))) b.include_writes = true;
      loadArgs();
    } else if (mode === 'flow') {
      if (flowFrom === 'file') {
        b.file = flowFile.value();
        if (!b.file) throw new Error('choose or upload a flow file');
      } else {
        // The steps, written as a flow file in the project, as a run names one
        const text = flowSteps.write();
        // Named by what it says: the same steps are the same file
        let hash = 2166136261;
        for (let i = 0; i < text.length; i++) hash = Math.imul(hash ^ text.charCodeAt(i), 16777619) >>> 0;
        const name = `flow-${hash.toString(16)}.toml`;
        const res = await fetch(`/api/files/${name}`, { method: 'PUT', headers: { Authorization: `Bearer ${token}` }, body: text });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error);
        b.file = data.path;
      }
      loadArgs();
      if (varsFile.value()) b.vars = varsFile.value();
    } else if (mode === 'replay') {
      b.log = logFile.value();
      if (!b.log) throw new Error('choose or upload a log');
      if (str(f.baseUrl)) b.base_url = str(f.baseUrl);
      if (f.includeWrites.box.checked) b.include_writes = true;
      delete b.body;
      loadArgs();
    } else if (mode === 'ping') {
      b.targets = lines(f.targets.value.trim() ? f.targets.value : f.url.value);
      if (num(f.requests)) b.count = num(f.requests);
      if (str(f.duration)) b.duration = str(f.duration);
      if (str(f.every)) b.every = str(f.every);
      if (str(f.slo)) b.slo = str(f.slo);
      if (f.keepAlive.box.checked) b.keep_alive = true;
      if (f.allIps.box.checked) b.all_ips = true;
      if (f.tcp.box.checked) b.tcp = true;
    } else {
      b.url = f.url.value.trim();
      if (num(f.from)) b.from = num(f.from);
      if (num(f.to)) b.to = num(f.to);
      if (num(f.step)) b.step = num(f.step);
      if (str(f.stepFor)) b.every = str(f.stepFor);
      const until = lines(f.until.value);
      if (until.length) b.until = until;
      if (fails.length) b.fail_if = fails;
      if (varsFile.value()) b.vars = varsFile.value();
    }
    return b;
  };

  const out = h('div', {});
  const plan = async () => {
    out.replaceChildren();
    try {
      const { plan } = await api('POST', '/runs', { ...(await args()), dry_run: true });
      out.append(h('div', { class: 'plan' },
        h('div', {}, h('b', {}, 'Would send '), plan.load),
        (plan.targets || []).slice(0, 12).map((t) => h('div', {}, h('span', { class: 'k' }, '→ '), t)),
        plan.more_targets ? h('div', { class: 'k' }, `… and ${plan.more_targets} more`) : null,
        (plan.settings || []).length ? h('div', { class: 'k' }, plan.settings.join(' · ')) : null,
        plan.guard ? h('div', { class: 'k' }, `held to ${plan.guard}`) : null,
        (plan.notes || []).map((n) => h('div', { class: 'warn' }, n)),
        h('div', { class: 'good', style: 'margin-top:6px' }, '✓ Nothing sent yet.')));
      out.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    } catch (e) { out.append(error(e)); }
  };
  const start = async () => {
    out.replaceChildren();
    try {
      const run = await api('POST', '/runs', await args());
      go(run.id);
    } catch (e) {
      out.append(error(e));
      out.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  };
  const save = async () => {
    out.replaceChildren();
    if (mode !== 'run') { out.append(error('Saving as a test is for a load test, for now; the others run as they are.')); return; }
    const name = prompt('Save as the test named', query.test || '');
    if (!name) return;
    const b = await args();
    const config = { url: b.url };
    const keys = { method: 'method', headers: 'headers', body: 'body', requests: 'requests', duration: 'duration', concurrency: 'concurrency', rate: 'rate', warmup: 'warmup', timeout: 'timeout', user_agent: 'user-agent', proxy: 'proxy', insecure: 'insecure', vars: 'vars' };
    for (const [from, to] of Object.entries(keys)) if (b[from] !== undefined) config[to] = b[from];
    if (b.no_keepalive) config['keep-alive'] = false;
    if (b.no_redirects) config.redirects = false;
    if (b.no_compression) config.compression = false;
    try {
      await api('PUT', `/tests/${encodeURIComponent(name)}`, { config });
      location.hash = `#/test/${encodeURIComponent(name)}`;
    } catch (e) { out.append(error(e)); }
  };

  view.append(
    query.test
      ? pagehead({ tag: 'Edit a test', before: 'Run ', sel: query.test, lede: 'Change what it sends, check the plan, start it or save it again.' })
      : pagehead({ tag: 'New run', before: 'Point it at something, ', sel: 'see where the time goes', lede: 'Pick a mode, say how much, and check the plan: it is held to the server\'s guardrails before anything is sent.' }),
    tabs,
    h('form', { onsubmit: (e) => { e.preventDefault(); start(); } },
      body, out,
      h('div', { class: 'bar' }, h('div', { class: 'actions' },
        h('button', { class: 'go', type: 'submit' }, '▶ Start'),
        h('button', { type: 'button', onclick: plan }, 'Check the plan'),
        h('button', { type: 'button', class: 'quiet', onclick: save }, 'Save as a test')))));
  draw();
}

// ─── Saved tests and schedules ───────────────────────────────────────────

async function viewTests() {
  const [data, sched] = await Promise.all([api('GET', '/tests'), api('GET', '/schedules')]);
  const out = h('div', {});
  const run = async (name) => {
    try { go((await api('POST', '/runs', { test: name })).id); } catch (e) { out.replaceChildren(error(e)); }
  };
  const remove = async (name) => {
    if (!confirm(`Delete the test ${name}? Its recorded runs stay.`)) return;
    try { await api('DELETE', `/tests/${encodeURIComponent(name)}`); route(); } catch (e) { out.replaceChildren(error(e)); }
  };
  view.append(pagehead({ tag: 'Tests', before: 'Worked out once, ', sel: 'run again', lede: 'The project\'s saved tests, .pepe/tests/: the same files as pepe --load NAME, and their recorded runs.', right: h('a', { class: 'button go', href: '#/new' }, '+ New test') }), out);
  if (!data.tests.length) {
    view.append(emptyState('sleeping', 'No saved tests yet', 'Set up a run, then Save as a test: it runs again from here, from a schedule, or as pepe --load NAME.',
      h('a', { class: 'button go', href: '#/new' }, 'Make one')));
  } else {
    view.append(h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Test'), h('th', { class: 'hide-narrow' }, 'Sends to'), h('th', { class: 'num' }, 'Runs'), h('th', {}, ''))),
      h('tbody', {}, data.tests.map((t) => h('tr', {},
        h('td', {}, h('a', { class: 'what', href: `#/test/${encodeURIComponent(t.name)}` }, t.name), t.error ? h('span', { class: 'sub bad' }, t.error) : null),
        h('td', { class: 'mono hide-narrow', style: 'font-size:13px' }, (t.config && t.config.url) || '—'),
        h('td', { class: 'num' }, t.runs),
        h('td', {}, h('div', { class: 'actions', style: 'justify-content:flex-end' },
          h('button', { class: 'primary', onclick: () => run(t.name) }, '▶ Run'),
          h('a', { class: 'button quiet', href: `#/new?test=${encodeURIComponent(t.name)}` }, 'Edit'),
          h('button', { class: 'danger', onclick: () => remove(t.name) }, 'Delete'))))))))));
  }

  // Schedules
  const which = h('select', { 'aria-label': 'Test' }, data.tests.map((t) => h('option', { value: t.name }, t.name)));
  const often = h('input', { placeholder: '1h', 'aria-label': 'Every', style: 'width:110px' });
  const said = h('div', {});
  view.append(section('Schedules'), h('div', { class: 'card' },
    sched.schedules.length ? h('div', { class: 'table' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Test'), h('th', {}, 'Every'), h('th', { class: 'hide-narrow' }, 'Next'), h('th', {}, 'Last'), h('th', {}, ''))),
      h('tbody', {}, sched.schedules.map((x) => h('tr', {},
        h('td', { class: 'what' }, x.test), h('td', { class: 'mono' }, every_(x.every_s)),
        h('td', { class: 'hide-narrow' }, new Date(x.next_unix * 1000).toLocaleString()),
        h('td', {}, x.last_run ? h('a', { href: `#/run/${encodeURIComponent(x.last_run)}` }, 'its run') : x.last_skipped ? h('span', { class: 'warn' }, `skipped: ${x.last_skipped}`) : h('span', { class: 'faint' }, 'not yet')),
        h('td', {}, h('div', { class: 'actions', style: 'justify-content:flex-end' }, h('button', { class: 'danger', onclick: async () => { await api('DELETE', `/schedules/${x.id}`); route(); } }, 'Remove')))))))) : h('p', { class: 'faint', style: 'margin:0 0 4px' }, 'Nothing runs on its own yet.'),
    data.tests.length ? h('form', { class: 'actions', style: 'margin-top:18px', onsubmit: async (e) => {
      e.preventDefault();
      try { await api('POST', '/schedules', { test: which.value, every: often.value.trim() }); route(); } catch (err) { said.replaceChildren(error(err)); }
    } }, h('span', { class: 'mono faint' }, 'run'), which, h('span', { class: 'mono faint' }, 'every'), often, h('button', { type: 'submit' }, '+ Add')) : null, said));
}

function every_(seconds) {
  if (seconds % 86400 === 0) return `${seconds / 86400}d`;
  if (seconds % 3600 === 0) return `${seconds / 3600}h`;
  if (seconds % 60 === 0) return `${seconds / 60}m`;
  return `${seconds}s`;
}

async function viewTest([name]) {
  const data = await api('GET', `/tests/${encodeURIComponent(name)}/runs`);
  let before = data.runs[1] && data.runs[1].id;
  let after = data.runs[0] && data.runs[0].id;
  const compare = h('a', { class: 'button primary' }, 'Compare the two');
  const setLink = () => compare.setAttribute('href', `#/compare?before=${encodeURIComponent(before || '')}&after=${encodeURIComponent(after || '')}`);
  setLink();
  view.append(pagehead({
    tag: 'Test', sel: name,
    lede: `${data.runs.length} recorded run${data.runs.length === 1 ? '' : 's'}, newest first: the same as pepe history ${name}.`,
    right: [h('button', { class: 'go', onclick: async () => go((await api('POST', '/runs', { test: name })).id) }, '▶ Run it now'),
      h('a', { class: 'button', href: `#/new?test=${encodeURIComponent(name)}` }, 'Edit')],
  }));
  if (!data.runs.length) {
    view.append(emptyState('sleeping', 'Not run yet', 'Its runs are kept here, to hold against each other.', null));
    return;
  }
  // How it has gone run after run, oldest left, as k6 Cloud trends a test
  const past = data.runs.slice().reverse().map((r, i) => ({ t: i, label: when(r.when), ...(r.summary || {}) }))
    .map((r) => ({ ...r, p50: r.median_ms, p99: r.p99_ms, rps: r.requests_per_second, failed: r.failed_percent }));
  if (past.length > 1) {
    const label = (i) => (past[Math.round(i)] ? `#${Math.round(i) + 1}` : '');
    const lat = [{ key: 'p50', label: 'p50', cls: 'c-leaf' }, { key: 'p99', label: 'p99', cls: 'c-red', dash: true }];
    const thr = [{ key: 'rps', label: 'req/s', cls: 'c-ink', kind: 'area' }, { key: 'failed', label: 'failed %', cls: 'c-red', kind: 'bars', fmt: pct }];
    const c1 = timeChart({ series: lat, fmt: ms, xfmt: label });
    const c2 = timeChart({ series: thr, fmt: (v) => rate(v).replace('/s', ''), xfmt: label });
    c1.update(past);
    c2.update(past);
    const newest = past.at(-1), first = past[0];
    view.append(tiles([
      ['Runs', count(past.length), `since ${first.label}`],
      ['p99, last', ms(newest.p99), `best ${ms(Math.min(...past.map((x) => x.p99 ?? Infinity)))}`, past.map((x) => x.p99), 'red'],
      ['p50, last', ms(newest.p50), `best ${ms(Math.min(...past.map((x) => x.p50 ?? Infinity)))}`, past.map((x) => x.p50), 'leaf'],
      ['req/s, last', rate(newest.rps), `best ${rate(Math.max(...past.map((x) => x.rps || 0)))}`, past.map((x) => x.rps)],
      ['Failed, last', pct(newest.failed), '', past.map((x) => x.failed), newest.failed ? 'bad' : 'good'],
    ]), h('div', { class: 'chartgrid' }, chartCard('Latency, run by run', c1, lat), chartCard('Throughput and failures, run by run', c2, thr)));
  }
  view.append(h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
    h('thead', {}, h('tr', {}, h('th', {}, 'Before'), h('th', {}, 'After'), h('th', {}, 'When'), h('th', { class: 'num' }, 'Requests'), h('th', { class: 'num' }, 'p50'), h('th', { class: 'num' }, 'p99'), h('th', { class: 'num hide-narrow' }, 'Throughput'), h('th', { class: 'num hide-narrow' }, 'Failed'))),
    h('tbody', {}, data.runs.map((r) => {
      const sm = r.summary || {};
      return h('tr', {},
        h('td', {}, h('input', { type: 'radio', name: 'before', checked: r.id === before, 'aria-label': 'Before', onchange: () => { before = r.id; setLink(); } })),
        h('td', {}, h('input', { type: 'radio', name: 'after', checked: r.id === after, 'aria-label': 'After', onchange: () => { after = r.id; setLink(); } })),
        h('td', {}, when(r.when)),
        h('td', { class: 'num' }, count(sm.requests)),
        h('td', { class: 'num' }, ms(sm.median_ms)),
        h('td', { class: 'num' }, ms(sm.p99_ms)),
        h('td', { class: 'num hide-narrow' }, rate(sm.requests_per_second)),
        h('td', { class: 'num hide-narrow' }, pct(sm.failed_percent)));
    })))), h('div', { class: 'actions', style: 'margin-top:18px' }, compare)));
}

// ─── Compare ─────────────────────────────────────────────────────────────

async function viewCompare(_, query) {
  const runs = (await api('GET', '/runs?limit=100')).runs.filter((r) => r.state === 'done' || r.state === 'stopped');
  const options = runs.map((r) => h('option', { value: r.id }, `${titleOf(r)} · ${when(r.started)}`));
  const before = h('input', { list: 'runs', value: query.before || '', placeholder: 'a run, or NAME/WHEN', 'aria-label': 'Before' });
  const after = h('input', { list: 'runs', value: query.after || '', placeholder: 'a run, or NAME/WHEN', 'aria-label': 'After' });
  const out = h('div', {});
  view.append(
    pagehead({ tag: 'Compare', before: 'Did it get ', sel: 'faster?', lede: 'Two runs held against each other: what moved more than a run of that size wobbles on its own.' }),
    h('datalist', { id: 'runs' }, options),
    h('form', { class: 'card', onsubmit: (e) => { e.preventDefault(); location.hash = `#/compare?before=${encodeURIComponent(before.value)}&after=${encodeURIComponent(after.value)}`; } },
      h('div', { class: 'fields' }, field('Before', before), field('After', after)),
      h('button', { class: 'primary', type: 'submit' }, 'Compare')),
    out);
  if (!query.before || !query.after) return;
  try {
    const c = await api('GET', `/compare?before=${encodeURIComponent(query.before)}&after=${encodeURIComponent(query.after)}&gate=1`);
    const cls = { faster: 'good', better: 'good', same: 'faint', slower: 'bad', worse: 'bad' }[c.verdict] || '';
    // Two runs of this server: their latency curves, one over the other
    const local = (x) => !x.includes('/');
    if (local(query.before) && local(query.after)) {
      Promise.all([api('GET', `/runs/${encodeURIComponent(query.before)}`), api('GET', `/runs/${encodeURIComponent(query.after)}`)]).then(([b, a]) => {
        const curve = (r) => { const l = r.report && r.report.summary && r.report.summary.latency; return l ? [[0, l.min_ms], ...(l.quantiles_ms || []).map((x) => [x.q, x.ms])] : null; };
        const cb = curve(b), ca = curve(a);
        if (!cb || !ca || cb.length < 3 || ca.length < 3) return;
        const chart = percentileChart([{ label: 'before', cls: 'c-ink', dash: true, points: cb }, { label: 'after', cls: 'c-ember', area: true, points: ca }], true);
        overlay.replaceChildren(h('div', { class: 'chartgrid one' }, chartCard('Latency by percentile, before and after', chart, [{ cls: 'c-ink', label: `before · ${titleOf(b)} · ${when(b.started)}` }, { cls: 'c-ember', label: `after · ${titleOf(a)} · ${when(a.started)}` }])));
      }).catch(() => {});
    }
    const overlay = h('div', {});
    const widest = Math.max(10, ...(c.changes || []).map((x) => Math.abs(x.change_percent || 0)), ...(c.changes || []).map((x) => x.noise_percent || 0));
    // A change as a bar from the middle, the run's own wobble shaded
    const moved = (x, bad) => h('span', { class: 'diverge', title: `±${x.noise_percent}% is noise` },
      h('i', { class: 'noise', style: `left:${50 - (50 * (x.noise_percent || 0)) / widest}%;width:${(100 * (x.noise_percent || 0)) / widest}%` }),
      x.change_percent === null ? null : h('i', { class: `delta ${x.significant ? (bad ? 'bad' : 'good') : 'faint'}`, style: x.change_percent >= 0
        ? `left:50%;width:${(50 * Math.min(widest, x.change_percent)) / widest}%` : `left:${50 - (50 * Math.min(widest, -x.change_percent)) / widest}%;width:${(50 * Math.min(widest, -x.change_percent)) / widest}%` }),
      h('i', { class: 'zero' }));
    out.append(section('What moved'), h('div', { class: 'card' },
      h('div', { class: 'verdict-head' }, pill(c.verdict, cls), c.regression ? pill('regression', 'bad') : null,
        h('strong', {}, { faster: 'Faster', better: 'Better', same: 'About the same', slower: 'Slower', worse: 'Worse' }[c.verdict] || c.verdict)),
      h('div', { class: 'table' }, h('table', {},
        h('thead', {}, h('tr', {}, h('th', {}, 'What'), h('th', { class: 'num' }, 'Before'), h('th', { class: 'num' }, 'After'), h('th', { class: 'hide-narrow', style: 'width:26%' }, 'Moved'), h('th', { class: 'num' }, 'Change'), h('th', { class: 'num hide-narrow' }, 'Wobbles by'))),
        h('tbody', {}, (c.changes || []).map((x) => {
          const fmt = x.metric.endsWith('_ms') ? ms : x.metric.includes('per_second') ? rate : (v) => (+v).toLocaleString();
          const up = x.change_percent > 0;
          const bad = x.significant && (x.metric.endsWith('_ms') || x.metric.includes('failed') ? up : !up);
          return h('tr', {},
            h('td', { class: 'what' }, x.metric.replace(/_ms$|_percent$/, '').replaceAll('_', ' ')),
            h('td', { class: 'num' }, fmt(x.before)), h('td', { class: 'num' }, fmt(x.after)),
            h('td', { class: 'hide-narrow' }, moved(x, bad)),
            h('td', { class: `num ${x.significant ? (bad ? 'bad' : 'good') : 'faint'}` }, h('b', {}, x.change_percent === null ? '—' : `${x.change_percent > 0 ? '+' : ''}${x.change_percent}%`)),
            h('td', { class: 'num faint hide-narrow' }, `±${x.noise_percent}%`));
        })))),
      h('ul', { class: 'findings', style: 'margin-top:18px' }, (c.findings || []).map((f) => {
        const k = LEVEL[String(f.level).toLowerCase()] || 'note';
        return h('li', { class: k === 'good' ? 'good' : k === 'bad' ? 'bad' : '' }, h('span', { class: `d ${k}` }), h('span', {}, f.text));
      }))), overlay);
  } catch (e) { out.append(error(e)); }
}

// ─── Workers ─────────────────────────────────────────────────────────────

async function viewWorkers() {
  const data = await api('GET', '/workers');
  view.append(pagehead({ tag: 'Workers', before: 'More machines, ', sel: 'one run', lede: 'Machines that joined this server: a load test can be shared out to them, each under its own guardrails, and its numbers added up exactly.' }));
  view.append(data.workers.length
    ? h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Worker'), h('th', {}, 'pepe'), h('th', { class: 'hide-narrow' }, 'Held to'), h('th', {}, 'Joined'), h('th', {}, 'Now'))),
      h('tbody', {}, data.workers.map((w) => h('tr', {},
        h('td', { class: 'what mono' }, w.name), h('td', { class: 'mono' }, w.pepe || '—'), h('td', { class: 'hide-narrow faint' }, w.guard || 'nothing of its own'),
        h('td', {}, when(w.since)), h('td', {}, w.busy ? h('a', { href: `#/run/${encodeURIComponent(w.busy)}` }, pill('busy', 'going')) : pill('free', 'faint'))))))))
    : emptyState('sleeping', 'No worker has joined', 'Start one on another machine, with a token from this server\'s token file.', null));
  view.append(section('Join one'), command(`pepe --allow-host HOST worker --join ${location.origin} --name NAME --token-file token`));
  every(3000, async () => {
    const now = await api('GET', '/workers').catch(() => null);
    if (now && JSON.stringify(now) !== JSON.stringify(data)) route();
  });
}

// ─── Go ──────────────────────────────────────────────────────────────────

themeToggle();
takeToken();
window.addEventListener('hashchange', route);
route();
