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

function gate(message) {
  document.querySelector('.top').hidden = true;
  const input = h('input', { type: 'password', placeholder: 'Token', autocomplete: 'off', 'aria-label': 'Token', required: true });
  const said = h('p', { class: 'bad', role: 'alert' }, message || '');
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
  }, input, h('button', { class: 'primary' }, 'Open'));
  view.replaceChildren(h('div', { class: 'gate' },
    face('waiting', ''),
    h('h1', {}, 'pepe serve'),
    h('p', { class: 'sub' }, 'Paste the token pepe serve printed when it started, or open the link it printed.'),
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
  document.querySelector('.top').hidden = false;
  document.querySelector('.who').replaceChildren(
    `token ••••${token.slice(-4)} · `,
    h('a', { href: '#', onclick: (e) => { e.preventDefault(); forget(); gate(); } }, 'forget it'));
  const [path, q] = location.hash.replace(/^#\/?/, '').split('?');
  const [name, ...rest] = path.split('/').map(decodeURIComponent);
  const query = Object.fromEntries(new URLSearchParams(q || ''));
  for (const a of document.querySelectorAll('nav a')) a.classList.toggle('on', a.dataset.nav === (name === 'run' ? 'runs' : name === 'test' ? 'tests' : name));
  const views = { runs: viewRuns, run: viewRun, new: viewNew, tests: viewTests, test: viewTest, compare: viewCompare, workers: viewWorkers };
  view.replaceChildren();
  (views[name] || viewRuns)(rest, query).catch((e) => view.append(error(e)));
}

function every(ms, fn) {
  const t = setInterval(fn, ms);
  leaving.push(() => clearInterval(t));
}

// ─── Runs ────────────────────────────────────────────────────────────────

async function viewRuns() {
  const title = h('div', {}, h('h1', {}, 'Runs'), h('p', { class: 'sub' }, ''));
  const body = h('div', {});
  view.append(title, body);
  const draw = async () => {
    const data = await api('GET', '/runs?limit=100');
    title.querySelector('.sub').textContent =
      `${data.going} going, ${data.max_runs} at most at once · newest first`;
    if (!data.runs.length) {
      body.replaceChildren(h('div', { class: 'empty card' }, face('sleeping', ''), 'No runs yet.', h('p', {}, h('a', { class: 'button primary', href: '#/new' }, 'Start one'))));
      return;
    }
    body.replaceChildren(h('div', { class: 'card' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'State'), h('th', {}, 'What'), h('th', { class: 'hide-narrow' }, 'Mode'), h('th', {}, 'Started'), h('th', { class: 'hide-narrow' }, 'By'))),
      h('tbody', {}, data.runs.map((r) => h('tr', { class: 'link', tabindex: 0, onclick: () => (location.hash = `#/run/${encodeURIComponent(r.id)}`), onkeydown: (e) => e.key === 'Enter' && (location.hash = `#/run/${encodeURIComponent(r.id)}`) },
        h('td', {}, pill(r.state, STATE[r.state])),
        h('td', {}, r.test ? h('strong', {}, r.test) : null, r.test ? ' ' : null, h('span', { class: 'mono faint' }, r.command.replace(/^pepe /, '')), r.workers && r.workers.length ? h('div', { class: 'faint' }, `on ${r.workers.join(', ')}`) : null),
        h('td', { class: 'hide-narrow' }, r.mode),
        h('td', {}, when(r.started)),
        h('td', { class: 'hide-narrow' }, r.by)))))));
    return data.going > 0;
  };
  await draw();
  every(2000, () => draw().catch(() => {}));
}

// A chart of series over time: [{ points: [[t, v]], cls, area }]
function chart(series, format, height = 150) {
  const W = 460, H = height, L = 52, R = 8, T = 8, B = 20;
  const all = series.flatMap((x) => x.points);
  const t1 = Math.max(1, ...all.map((p) => p[0]));
  const top = Math.max(1e-9, ...all.map((p) => p[1])) * 1.15;
  const x = (t) => L + (t / t1) * (W - L - R);
  const y = (v) => T + (1 - v / top) * (H - T - B);
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  for (const f of [0, 0.5, 1]) {
    svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: y(top * f / 1.15), y2: y(top * f / 1.15) }));
    svg.append(s('text', { class: 'axis', x: L - 6, y: y(top * f / 1.15) + 3, 'text-anchor': 'end' }, format(top * f / 1.15)));
  }
  svg.append(s('text', { class: 'axis', x: L, y: H - 4 }, '0s'));
  svg.append(s('text', { class: 'axis', x: W - R, y: H - 4, 'text-anchor': 'end' }, `${Math.round(t1)}s`));
  for (const one of series) {
    if (!one.points.length) continue;
    const d = one.points.map((p, i) => `${i ? 'L' : 'M'}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join('');
    if (one.area) svg.append(s('path', { class: `area ${one.cls}`, d: `${d}L${x(one.points.at(-1)[0])},${y(0)}L${x(one.points[0][0])},${y(0)}Z` }));
    svg.append(s('path', { class: `line ${one.cls}`, d }));
  }
  return svg;
}

const legend = (...items) => h('div', { class: 'legend' }, items.map(([cls, text]) => h('span', {}, h('span', { class: `sw ${cls}` }), text)));
const kpi = (label, value) => h('div', { class: 'kpi' }, h('div', { class: 'l' }, label), h('div', { class: 'v' }, value));

async function viewRun([id]) {
  const meta = await api('GET', `/runs/${encodeURIComponent(id)}`);
  const facePic = face(FACE[meta.state] || 'sleeping');
  const state = pill(meta.state, STATE[meta.state]);
  const kpis = h('div', { class: 'kpis' });
  const charts = h('div', { class: 'two' });
  const feed = h('ul', { class: 'feed' });
  const verdict = h('div', {});
  const workers = h('div', {});
  const steer = h('div', { class: 'actions' });
  view.append(
    h('div', { class: 'head' }, facePic, h('div', {},
      h('h1', {}, meta.test || meta.command.replace(/^pepe /, '').split(' ').at(-1)),
      h('div', { class: 'actions' }, state, h('span', { class: 'mono faint' }, meta.command)))),
    steer, kpis, h('div', { class: 'card' }, charts), workers, verdict,
    h('h2', {}, 'As it went'), h('div', { class: 'card' }, feed));

  const points = { rps: [], p50: [], p99: [] };
  let paused = false;
  const draw = () => {
    charts.replaceChildren(
      h('div', {}, h('h2', { style: 'margin-top:0' }, 'Requests a second'), chart([{ points: points.rps, cls: 'ember', area: true }], (v) => rate(v).replace('/s', ''))),
      h('div', {}, h('h2', { style: 'margin-top:0' }, 'Latency'), chart([{ points: points.p50, cls: 'violet' }, { points: points.p99, cls: 'bad' }], ms), legend(['violet', 'p50'], ['bad', 'p99'])));
  };
  draw();

  const showSteer = (going) => {
    steer.replaceChildren();
    if (!going) return;
    const say = async (what, body) => {
      try { await api('POST', `/runs/${encodeURIComponent(id)}/${what}`, body); } catch (e) { steer.append(error(e)); }
    };
    if (meta.mode !== 'ping') {
      const n = h('input', { type: 'number', min: 1, style: 'width:90px', 'aria-label': 'Concurrency', placeholder: 'at once' });
      steer.append(
        h('button', { onclick: () => say(paused ? 'resume' : 'pause') }, paused ? 'Resume' : 'Pause'),
        n, h('button', { onclick: () => n.value && say('concurrency', { n: Number(n.value) }) }, 'Set concurrency'));
    }
    steer.append(h('button', { class: 'danger', onclick: () => say('stop') }, 'Stop'));
  };
  showSteer(meta.state === 'running');

  const line = (cls, text) => feed.prepend(h('li', { class: 'finding' }, h('span', { class: `d ${cls}` }), text));
  const progress = (e) => {
    const t = e.elapsed_s ?? e.at_s ?? 0;
    if (e.targets) {
      // A ping: each target's numbers
      const first = e.targets[0] || {};
      points.p50.push([t, first.p50_ms || 0]);
      points.p99.push([t, first.p99_ms || 0]);
      kpis.replaceChildren(kpi('Sent', count(first.sent)), kpi('Answered', count(first.answered)), kpi('Last', ms(first.last_ms)), kpi('p50', ms(first.p50_ms)), kpi('p99', ms(first.p99_ms)));
    } else {
      // A rate over the first few milliseconds says nothing yet
      if (t >= 0.5) points.rps.push([t, e.requests_per_second || 0]);
      points.p50.push([t, e.p50_ms || 0]);
      points.p99.push([t, e.p99_ms || 0]);
      kpis.replaceChildren(kpi('Requests', count(e.requests)), kpi('Throughput', rate(e.requests_per_second)), kpi('p50', ms(e.p50_ms)), kpi('p99', ms(e.p99_ms)), kpi('Failed', count(e.failed)));
      if (e.workers) {
        workers.replaceChildren(h('h2', {}, 'Workers'), h('div', { class: 'card' }, h('table', {},
          h('thead', {}, h('tr', {}, h('th', {}, 'Worker'), h('th', { class: 'num' }, 'Requests'), h('th', { class: 'num' }, 'Throughput'), h('th', { class: 'num' }, 'p99'))),
          h('tbody', {}, Object.entries(e.workers).map(([name, w]) => h('tr', {}, h('td', {}, name), h('td', { class: 'num' }, count(w.requests)), h('td', { class: 'num' }, rate(w.requests_per_second)), h('td', { class: 'num' }, ms(w.p99_ms))))))));
      }
    }
    draw();
  };

  const controller = new AbortController();
  leaving.push(() => controller.abort());
  follow(id, (type, e) => {
    if (type === 'start') line('note', e.worker ? `${e.worker} started its share` : `Started: ${e.load || e.target || ''}`);
    else if (type === 'progress') progress(e);
    else if (type === 'anomaly') line('warn', e.text);
    else if (type === 'steered') { paused = e.paused; showSteer(true); line('note', `${e.what}${e.concurrency ? ` (${e.concurrency} at once)` : ''}`); }
    else if (type === 'worker_done') line(e.exit === 0 ? 'good' : 'bad', `${e.worker} ended its share${e.error ? `: ${e.error}` : ''}`);
    else if (type === 'report') showReport(e.report);
    else if (type === 'end') {
      state.replaceWith(pill(e.state, STATE[e.state]));
      showSteer(false);
      if (e.error) verdict.append(error(e.error));
      if (e.state !== 'done' && e.state !== 'stopped') facePic.src = `/ui/img/pepe-${FACE[e.state] || 'sleeping'}.png`;
    }
  }, controller.signal);

  function showReport(report) {
    const level = report.verdict && report.verdict.level;
    if (level) facePic.src = `/ui/img/pepe-${FACE[level] || 'proud'}.png`;
    const sum = report.summary;
    if (sum) {
      const failed = sum.total_requests ? (100 * sum.failed_requests) / sum.total_requests : 0;
      kpis.replaceChildren(kpi('Requests', count(sum.total_requests)), kpi('Throughput', rate(sum.requests_per_second)), kpi('p50', ms(sum.latency.median_ms)), kpi('p99', ms(sum.latency.p99_ms)), kpi('Failed', pct(failed)));
    }
    if (report.targets) {
      kpis.replaceChildren(...report.targets.map((t) => kpi(t.name || t.target, `${ms(t.latency_ms && t.latency_ms.p50)} · ${pct(t.loss_percent ?? 0)} lost`)));
    }
    const findings = (report.verdict && report.verdict.findings) || (report.targets || []).flatMap((t) => t.findings || []);
    // The reports open as plain links, each with a ticket the server made
    // for this run alone: a page can't send the token with a link
    const links = h('span', { class: 'actions' });
    api('POST', `/runs/${encodeURIComponent(id)}/ticket`).then(({ ticket }) => {
      const link = (format, text, primary) => h('a', {
        class: `button${primary ? ' primary' : ''}`, target: '_blank', rel: 'noopener noreferrer',
        href: `/api/runs/${encodeURIComponent(id)}/report?format=${format}&ticket=${encodeURIComponent(ticket)}`,
      }, text);
      links.replaceChildren(
        meta.mode !== 'ping' ? link('html', 'Full report', true) : null,
        link('md', 'Markdown'), link('json', 'JSON'));
    }).catch((e) => links.replaceChildren(error(e)));
    verdict.replaceChildren(h('h2', {}, 'Verdict'), h('div', { class: 'card' },
      level ? h('p', {}, pill(level, LEVEL[level])) : null,
      h('ul', {}, findings.map((f) => h('li', { class: 'finding' }, h('span', { class: `d ${LEVEL[String(f.level).toLowerCase()] || 'note'}` }), f.text))),
      h('div', { class: 'actions', style: 'margin-top:14px' },
        links,
        meta.mode === 'run' ? h('a', { class: 'button', href: `#/compare?after=${encodeURIComponent(id)}` }, 'Compare with…') : null)));
  }
}

// ─── A new run ───────────────────────────────────────────────────────────

async function viewNew(_, query) {
  const [workersData, testData] = await Promise.all([
    api('GET', '/workers').catch(() => ({ workers: [] })),
    query.test ? api('GET', `/tests/${encodeURIComponent(query.test)}`) : null,
  ]);
  const c = (testData && testData.config) || {};
  const field = (label, input, hint) => h('div', { class: 'field' }, h('label', {}, label, input), hint ? h('div', { class: 'hint' }, hint) : null);
  const inp = (attrs) => h('input', attrs);
  const f = {
    mode: h('select', { 'aria-label': 'Mode' }, ['run', 'ping', 'ramp'].map((m) => h('option', { value: m }, m))),
    url: inp({ type: 'url', placeholder: 'https://api.example.com/health', value: c.url || '', required: true }),
    method: inp({ placeholder: 'GET', value: c.method || '' }),
    requests: inp({ type: 'number', min: 1, placeholder: '200', value: c.requests || '' }),
    duration: inp({ placeholder: '30s', value: c.duration || '' }),
    concurrency: inp({ type: 'number', min: 1, placeholder: 'one per core', value: c.concurrency || '' }),
    rate: inp({ type: 'number', min: 0, step: 'any', placeholder: 'as fast as it answers', value: c.rate || '' }),
    warmup: inp({ placeholder: 'none', value: c.warmup || '' }),
    headers: h('textarea', { placeholder: 'Accept: application/json' }),
    body: h('textarea', { placeholder: '{"a": 1}' }),
    every: inp({ placeholder: '1s' }),
    to: inp({ type: 'number', min: 1, placeholder: '100' }),
    step: inp({ type: 'number', min: 1, placeholder: '10' }),
  };
  f.headers.value = (c.headers || []).join('\n');
  f.body.value = c.body || '';
  const free = workersData.workers.filter((w) => !w.busy);
  const picks = free.map((w) => h('label', { style: 'display:inline-flex;gap:6px;margin-right:14px;color:inherit' }, h('input', { type: 'checkbox', value: w.name, style: 'width:auto' }), w.name));
  const out = h('div', {});
  const lines = (t) => t.split('\n').map((x) => x.trim()).filter(Boolean);
  const num = (input) => (input.value === '' ? undefined : Number(input.value));
  const str = (input) => input.value.trim() || undefined;

  const body = () => {
    const mode = f.mode.value;
    const b = { mode, headers: lines(f.headers.value) };
    if (!b.headers.length) delete b.headers;
    if (mode === 'ping') {
      b.targets = [f.url.value.trim()];
      if (num(f.requests)) b.count = num(f.requests);
      if (str(f.duration)) b.duration = str(f.duration);
      if (str(f.every)) b.every = str(f.every);
    } else {
      b.url = f.url.value.trim();
      if (str(f.method)) b.method = str(f.method);
      if (f.body.value.trim()) b.body = f.body.value;
      if (mode === 'ramp') {
        if (num(f.to)) b.to = num(f.to);
        if (num(f.step)) b.step = num(f.step);
        if (str(f.every)) b.every = str(f.every);
      } else {
        if (num(f.requests)) b.requests = num(f.requests);
        if (str(f.duration)) b.duration = str(f.duration);
        if (num(f.concurrency)) b.concurrency = num(f.concurrency);
        if (num(f.rate)) b.rate = num(f.rate);
        if (str(f.warmup)) b.warmup = str(f.warmup);
        const on = picks.map((p) => p.querySelector('input')).filter((i) => i.checked).map((i) => i.value);
        if (on.length) b.workers = on;
      }
    }
    return b;
  };

  const plan = async () => {
    out.replaceChildren();
    try {
      const { plan } = await api('POST', '/runs', { ...body(), dry_run: true });
      out.append(h('div', { class: 'plan' },
        h('div', {}, h('strong', {}, 'Would send '), plan.load),
        (plan.targets || []).map((t) => h('div', { class: 'mono' }, t)),
        plan.guard ? h('div', { class: 'faint' }, `Held to ${plan.guard}`) : null,
        h('div', { class: 'faint' }, 'Nothing sent yet.')));
    } catch (e) { out.append(error(e)); }
  };
  const start = async () => {
    out.replaceChildren();
    try {
      const run = await api('POST', '/runs', body());
      location.hash = `#/run/${encodeURIComponent(run.id)}`;
    } catch (e) { out.append(error(e)); }
  };
  const save = async () => {
    out.replaceChildren();
    const name = prompt('Save as the test named', query.test || '');
    if (!name) return;
    const b = body();
    const config = { url: b.url };
    for (const [from, to] of [['method', 'method'], ['headers', 'headers'], ['body', 'body'], ['requests', 'requests'], ['duration', 'duration'], ['concurrency', 'concurrency'], ['rate', 'rate'], ['warmup', 'warmup']]) {
      if (b[from] !== undefined) config[to] = b[from];
    }
    try {
      await api('PUT', `/tests/${encodeURIComponent(name)}`, { config });
      location.hash = '#/tests';
    } catch (e) { out.append(error(e)); }
  };

  const modeFields = h('div', {});
  const showMode = () => {
    const m = f.mode.value;
    modeFields.replaceChildren(
      m === 'ramp'
        ? h('div', { class: 'fields' }, field('Up to, at once', f.to), field('In steps of', f.step), field('Each step for', f.every, '5s by default'))
        : m === 'ping'
          ? h('div', { class: 'fields' }, field('Pings', f.requests), field('Or for', f.duration), field('One every', f.every, '1s by default'))
          : h('div', {},
            h('div', { class: 'fields' }, field('Requests', f.requests), field('Or for', f.duration, '30s, 5m'), field('At once', f.concurrency), field('A second', f.rate, 'an arrival rate, as users arrive'), field('Warm-up', f.warmup)),
            picks.length ? field('Share it out to', h('div', {}, picks), 'workers that joined and are free') : null));
  };
  f.mode.addEventListener('change', showMode);
  showMode();

  view.append(
    h('h1', {}, query.test ? `Run ${query.test}` : 'New run'),
    h('p', { class: 'sub' }, 'Checked against the server\'s guardrails before anything is sent.'),
    h('form', { class: 'card', onsubmit: (e) => { e.preventDefault(); start(); } },
      h('div', { class: 'fields' }, field('Mode', f.mode), field('Method', f.method)),
      field('URL', f.url),
      modeFields,
      h('div', { class: 'two' }, field('Headers, one a line', f.headers), field('Body', f.body)),
      h('div', { class: 'actions' },
        h('button', { class: 'primary', type: 'submit' }, 'Start'),
        h('button', { type: 'button', onclick: plan }, 'Check the plan'),
        h('button', { type: 'button', onclick: save }, 'Save as a test')),
      out));
  f.url.focus();
}

// ─── Saved tests and schedules ───────────────────────────────────────────

async function viewTests() {
  const [data, sched] = await Promise.all([api('GET', '/tests'), api('GET', '/schedules')]);
  const out = h('div', {});
  const run = async (name) => {
    try {
      const r = await api('POST', '/runs', { test: name });
      location.hash = `#/run/${encodeURIComponent(r.id)}`;
    } catch (e) { out.replaceChildren(error(e)); }
  };
  const remove = async (name) => {
    if (!confirm(`Delete the test ${name}? Its recorded runs stay.`)) return;
    try { await api('DELETE', `/tests/${encodeURIComponent(name)}`); route(); } catch (e) { out.replaceChildren(error(e)); }
  };
  view.append(h('h1', {}, 'Tests'), h('p', { class: 'sub' }, 'The project\'s saved tests, .pepe/tests/: the same as pepe --load NAME.'), out);
  if (!data.tests.length) {
    view.append(h('div', { class: 'empty card' }, face('sleeping', ''), 'No saved tests yet.', h('p', {}, h('a', { class: 'button primary', href: '#/new' }, 'Make one'))));
  } else {
    view.append(h('div', { class: 'card' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Test'), h('th', { class: 'hide-narrow' }, 'URL'), h('th', { class: 'num' }, 'Runs'), h('th', {}, ''))),
      h('tbody', {}, data.tests.map((t) => h('tr', {},
        h('td', {}, h('a', { href: `#/test/${encodeURIComponent(t.name)}` }, t.name), t.error ? h('div', { class: 'bad' }, t.error) : null),
        h('td', { class: 'mono hide-narrow' }, (t.config && t.config.url) || '—'),
        h('td', { class: 'num' }, t.runs),
        h('td', {}, h('div', { class: 'actions', style: 'justify-content:flex-end' },
          h('button', { class: 'primary', onclick: () => run(t.name) }, 'Run'),
          h('a', { class: 'button', href: `#/new?test=${encodeURIComponent(t.name)}` }, 'Edit'),
          h('button', { class: 'danger', onclick: () => remove(t.name) }, 'Delete')))))))));
  }

  // Schedules
  const which = h('select', { 'aria-label': 'Test' }, data.tests.map((t) => h('option', { value: t.name }, t.name)));
  const often = h('input', { placeholder: '1h', 'aria-label': 'Every', style: 'max-width:120px' });
  const said = h('div', {});
  view.append(h('h2', {}, 'Schedules'), h('div', { class: 'card' },
    sched.schedules.length ? h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Test'), h('th', {}, 'Every'), h('th', { class: 'hide-narrow' }, 'Next'), h('th', {}, 'Last'), h('th', {}, ''))),
      h('tbody', {}, sched.schedules.map((x) => h('tr', {},
        h('td', {}, x.test), h('td', {}, every_(x.every_s)),
        h('td', { class: 'hide-narrow' }, new Date(x.next_unix * 1000).toLocaleString()),
        h('td', {}, x.last_run ? h('a', { href: `#/run/${encodeURIComponent(x.last_run)}` }, 'its run') : x.last_skipped ? h('span', { class: 'warn' }, `skipped: ${x.last_skipped}`) : '—'),
        h('td', {}, h('button', { class: 'danger', onclick: async () => { await api('DELETE', `/schedules/${x.id}`); route(); } }, 'Remove')))))) : h('p', { class: 'faint' }, 'None.'),
    data.tests.length ? h('form', { class: 'actions', style: 'margin-top:12px', onsubmit: async (e) => {
      e.preventDefault();
      try { await api('POST', '/schedules', { test: which.value, every: often.value.trim() }); route(); } catch (err) { said.replaceChildren(error(err)); }
    } }, 'Run', which, 'every', often, h('button', { type: 'submit' }, 'Add')) : null, said));
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
  const go = h('a', { class: 'button primary' }, 'Compare');
  const setLink = () => go.setAttribute('href', `#/compare?before=${encodeURIComponent(before || '')}&after=${encodeURIComponent(after || '')}`);
  setLink();
  view.append(
    h('h1', {}, name),
    h('p', { class: 'sub' }, `${data.runs.length} recorded run${data.runs.length === 1 ? '' : 's'}, newest first: the same as pepe history ${name}.`),
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => { const r = await api('POST', '/runs', { test: name }); location.hash = `#/run/${encodeURIComponent(r.id)}`; } }, 'Run it now'),
      h('a', { class: 'button', href: `#/new?test=${encodeURIComponent(name)}` }, 'Edit')));
  if (!data.runs.length) {
    view.append(h('div', { class: 'empty card' }, face('sleeping', ''), 'Not run yet.'));
    return;
  }
  view.append(h('div', { class: 'card' }, h('table', {},
    h('thead', {}, h('tr', {}, h('th', {}, 'Before'), h('th', {}, 'After'), h('th', {}, 'When'), h('th', { class: 'num' }, 'Requests'), h('th', { class: 'num' }, 'p50'), h('th', { class: 'num' }, 'p99'), h('th', { class: 'num hide-narrow' }, 'Throughput'), h('th', { class: 'num hide-narrow' }, 'Failed'))),
    h('tbody', {}, data.runs.map((r) => {
      const sm = r.summary || {};
      return h('tr', {},
        h('td', {}, h('input', { type: 'radio', name: 'before', style: 'width:auto', checked: r.id === before, 'aria-label': 'Before', onchange: () => { before = r.id; setLink(); } })),
        h('td', {}, h('input', { type: 'radio', name: 'after', style: 'width:auto', checked: r.id === after, 'aria-label': 'After', onchange: () => { after = r.id; setLink(); } })),
        h('td', {}, when(r.when)),
        h('td', { class: 'num' }, count(sm.requests)),
        h('td', { class: 'num' }, ms(sm.median_ms)),
        h('td', { class: 'num' }, ms(sm.p99_ms)),
        h('td', { class: 'num hide-narrow' }, rate(sm.requests_per_second)),
        h('td', { class: 'num hide-narrow' }, pct(sm.failed_percent)));
    }))), h('div', { class: 'actions', style: 'margin-top:12px' }, go)));
}

// ─── Compare ─────────────────────────────────────────────────────────────

async function viewCompare(_, query) {
  const runs = (await api('GET', '/runs?limit=100')).runs.filter((r) => r.state === 'done' || r.state === 'stopped');
  const options = runs.map((r) => h('option', { value: r.id }, `${r.test || r.command.replace(/^pepe /, '')} · ${when(r.started)}`));
  const before = h('input', { list: 'runs', value: query.before || '', placeholder: 'a run, or NAME/WHEN', 'aria-label': 'Before' });
  const after = h('input', { list: 'runs', value: query.after || '', placeholder: 'a run, or NAME/WHEN', 'aria-label': 'After' });
  const out = h('div', {});
  view.append(
    h('h1', {}, 'Compare'),
    h('p', { class: 'sub' }, 'Two runs held against each other: what moved more than a run of that size wobbles on its own.'),
    h('datalist', { id: 'runs' }, options),
    h('form', { class: 'card', onsubmit: (e) => { e.preventDefault(); location.hash = `#/compare?before=${encodeURIComponent(before.value)}&after=${encodeURIComponent(after.value)}`; } },
      h('div', { class: 'two' }, h('div', { class: 'field' }, h('label', {}, 'Before', before)), h('div', { class: 'field' }, h('label', {}, 'After', after))),
      h('button', { class: 'primary', type: 'submit' }, 'Compare')),
    out);
  if (!query.before || !query.after) return;
  try {
    const c = await api('GET', `/compare?before=${encodeURIComponent(query.before)}&after=${encodeURIComponent(query.after)}&gate=1`);
    const cls = { faster: 'good', better: 'good', same: 'faint', slower: 'bad', worse: 'bad' }[c.verdict] || '';
    out.append(h('div', { class: 'card' },
      h('div', { class: 'actions' }, pill(c.verdict, cls), c.regression ? pill('regression', 'bad') : null),
      h('table', { style: 'margin-top:12px' },
        h('thead', {}, h('tr', {}, h('th', {}, 'What'), h('th', { class: 'num' }, 'Before'), h('th', { class: 'num' }, 'After'), h('th', { class: 'num' }, 'Change'), h('th', { class: 'num hide-narrow' }, 'Wobbles'))),
        h('tbody', {}, (c.changes || []).map((x) => {
          const fmt = x.metric.endsWith('_ms') ? ms : x.metric.includes('per_second') ? rate : (v) => (+v).toLocaleString();
          const up = x.change_percent > 0;
          const bad = x.significant && (x.metric.endsWith('_ms') || x.metric.includes('failed') ? up : !up);
          return h('tr', {},
            h('td', {}, x.metric.replace(/_ms$|_percent$/, '').replaceAll('_', ' ')),
            h('td', { class: 'num' }, fmt(x.before)), h('td', { class: 'num' }, fmt(x.after)),
            h('td', { class: `num ${x.significant ? (bad ? 'bad' : 'good') : 'faint'}` }, x.change_percent === null ? '—' : `${x.change_percent > 0 ? '+' : ''}${x.change_percent}%`),
            h('td', { class: 'num faint hide-narrow' }, `±${x.noise_percent}%`));
        }))),
      h('ul', { style: 'margin-top:14px' }, (c.findings || []).map((f) => h('li', { class: 'finding' }, h('span', { class: `d ${LEVEL[String(f.level).toLowerCase()] || 'note'}` }), f.text)))));
  } catch (e) { out.append(error(e)); }
}

// ─── Workers ─────────────────────────────────────────────────────────────

async function viewWorkers() {
  const data = await api('GET', '/workers');
  view.append(h('h1', {}, 'Workers'), h('p', { class: 'sub' }, 'Machines that joined: a run can be shared out to them, each under its own guardrails.'));
  view.append(h('div', { class: 'card' }, data.workers.length
    ? h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Worker'), h('th', {}, 'pepe'), h('th', { class: 'hide-narrow' }, 'Held to'), h('th', {}, 'Joined'), h('th', {}, 'Busy with'))),
      h('tbody', {}, data.workers.map((w) => h('tr', {},
        h('td', {}, h('strong', {}, w.name)), h('td', {}, w.pepe || '—'), h('td', { class: 'hide-narrow' }, w.guard || 'nothing of its own'),
        h('td', {}, when(w.since)), h('td', {}, w.busy ? h('a', { href: `#/run/${encodeURIComponent(w.busy)}` }, 'a run') : h('span', { class: 'faint' }, 'free'))))))
    : h('div', { class: 'empty' }, face('sleeping', ''), 'None has joined.')));
  view.append(h('h2', {}, 'Join one'), h('div', { class: 'card' },
    h('p', { style: 'margin-top:0' }, 'On the other machine, with a token from this server\'s token file:'),
    h('pre', { class: 'mono plan', style: 'white-space:pre-wrap;margin:0' },
      `pepe --allow-host HOST worker --join ${location.origin} --name NAME --token-file token`)));
  every(3000, async () => {
    const now = await api('GET', '/workers').catch(() => null);
    if (now && JSON.stringify(now) !== JSON.stringify(data)) route();
  });
}

// ─── Go ──────────────────────────────────────────────────────────────────

takeToken();
window.addEventListener('hashchange', route);
route();
