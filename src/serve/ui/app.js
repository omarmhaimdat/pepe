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

// ─── Runs ────────────────────────────────────────────────────────────────

// What a run is, in a few words: its test, or its mode and what it sends to
function titleOf(meta) {
  if (meta.test) return meta.test;
  const last = meta.command.replace(/^pepe /, '').split(' ').at(-1);
  const file = last.split('/').at(-1);
  return { api: `OpenAPI · ${file}`, flow: `Flow · ${file}`, replay: `Replay · ${file}`, ping: `Ping · ${last}` }[meta.mode] || last;
}

const go = (id) => (location.hash = `#/run/${encodeURIComponent(id)}`);

async function viewRuns() {
  const lede = h('p', { class: 'lede' }, '');
  const body = h('div', {});
  view.append(h('div', { class: 'pagehead' },
    h('span', { class: 'tag' }, 'Runs'),
    h('div', { class: 'row' },
      h('h1', {}, 'What ran, and ', selected('how it went')),
      h('div', { class: 'actions' }, h('a', { class: 'button go', href: '#/new' }, '+ New run'))),
    lede), body);
  const draw = async () => {
    const data = await api('GET', '/runs?limit=100');
    lede.textContent = data.going
      ? `${data.going} going now, ${data.max_runs} at most at once. Newest first.`
      : `Nothing going. ${data.max_runs} at most at once. Newest first.`;
    if (!data.runs.length) {
      body.replaceChildren(emptyState('sleeping', 'Nothing has run yet', 'A run started here, from a script or from a schedule shows up here, live.',
        h('a', { class: 'button go', href: '#/new' }, 'Start the first one')));
      return;
    }
    body.replaceChildren(h('div', { class: 'card' }, h('div', { class: 'table' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'State'), h('th', { style: 'width:100%' }, 'What'), h('th', { class: 'hide-narrow' }, 'Mode'), h('th', { class: 'hide-narrow' }, 'Started'), h('th', { class: 'hide-narrow' }, 'By'))),
      h('tbody', {}, data.runs.map((r) => h('tr', { class: 'link', tabindex: 0, onclick: () => go(r.id), onkeydown: (e) => e.key === 'Enter' && go(r.id) },
        h('td', {}, pill(r.state === 'running' ? 'live' : r.state, STATE[r.state])),
        h('td', {}, h('span', { class: 'what' }, titleOf(r)), h('span', { class: 'sub clip hide-narrow', title: r.command }, r.command.replace(/^pepe /, '')),
          r.workers && r.workers.length ? h('span', { class: 'sub' }, `shared out to ${r.workers.join(', ')}`) : null,
          h('span', { class: 'sub only-narrow' }, when(r.started))),
        h('td', { class: 'hide-narrow mono nowrap' }, r.mode),
        h('td', { class: 'nowrap hide-narrow' }, when(r.started)),
        h('td', { class: 'hide-narrow faint nowrap' }, r.by))))))));
  };
  await draw();
  every(2000, () => draw().catch(() => {}));
}

// A chart of series over time: [{ points: [[t, v]], cls, area }]
function chart(series, format, height = 170) {
  const W = 460, H = height, L = 56, R = 8, T = 10, B = 22;
  const all = series.flatMap((x) => x.points);
  const t1 = Math.max(1, ...all.map((p) => p[0]));
  const top = Math.max(1e-9, ...all.map((p) => p[1])) * 1.15;
  const x = (t) => L + (t / t1) * (W - L - R);
  const y = (v) => T + (1 - v / top) * (H - T - B);
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img' });
  for (const f of [0, 0.5, 1]) {
    const v = (top * f) / 1.15;
    svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: y(v), y2: y(v) }));
    svg.append(s('text', { class: 'axis', x: L - 8, y: y(v) + 3.5, 'text-anchor': 'end' }, format(v)));
  }
  svg.append(s('text', { class: 'axis', x: L, y: H - 5 }, '0s'));
  svg.append(s('text', { class: 'axis', x: W - R, y: H - 5, 'text-anchor': 'end' }, `${Math.round(t1)}s`));
  for (const one of series) {
    if (!one.points.length) continue;
    const d = one.points.map((p, i) => `${i ? 'L' : 'M'}${x(p[0]).toFixed(1)},${y(p[1]).toFixed(1)}`).join('');
    if (one.area) svg.append(s('path', { class: `area ${one.cls}`, d: `${d}L${x(one.points.at(-1)[0])},${y(0)}L${x(one.points[0][0])},${y(0)}Z` }));
    svg.append(s('path', { class: `line ${one.cls}`, d }));
  }
  return svg;
}

const legend = (...items) => h('div', { class: 'legend' }, items.map(([cls, text]) => h('span', {}, h('span', { class: `sw ${cls}` }), text)));

// What a report has besides its totals, by mode: a table each
function parts(report) {
  const codes = (sc) => Object.entries(sc || {}).map(([c, n]) => `${c} ×${count(n)}`).join(' · ');
  const table = (title, head, rows, note) => rows.length ? [section(title), h('div', { class: 'card' },
    note ? h('p', { class: 'faint mono', style: 'margin:0 0 14px;font-size:13px' }, note) : null,
    h('div', { class: 'table' }, h('table', {}, h('thead', {}, h('tr', {}, head.map(([label, num, narrow]) => h('th', { class: `${num ? 'num' : ''} ${narrow ? 'hide-narrow' : ''}` }, label)))),
      h('tbody', {}, rows.map((cells) => h('tr', {}, cells.map((c, i) => h('td', { class: `${head[i][1] ? 'num' : ''} ${head[i][2] ? 'hide-narrow' : ''}` }, c))))))))] : [];
  const usual = [['Requests', 1], ['Throughput', 1, 1], ['Median', 1], ['p99', 1], ['Failed', 1], ['Answers', 0, 1]];
  const row = (x) => [count(x.requests), rate(x.requests_per_second), ms(x.median_ms), ms(x.p99_ms), count(x.failed_requests), h('span', { class: 'mono faint' }, codes(x.status_codes))];
  if (report.endpoints) return table('Endpoints', [['Endpoint'], ...usual], report.endpoints.map((e) => [h('span', { class: 'mono' }, e.endpoint), ...row(e)]));
  if (report.flow) return table('Steps', [['Step'], ...usual], report.flow.steps.map((x) => [h('span', { class: 'what' }, x.step), ...row(x)]),
    `${count(report.flow.chains_started)} chains started, ${count(report.flow.chains_completed)} completed`);
  if (report.replay) {
    const r = report.replay;
    return table('URLs', [['URL'], ['Share of the log', 1, 1], ...usual], r.urls.slice(0, 25).map((u) => [h('span', { class: 'mono' }, `${u.method} ${u.url}`), pct(100 * u.share_in_log), ...row(u)]),
      `${count(r.replayed_from_log)} of the log's ${count(r.requests_in_log)} requests, ${count(r.distinct_urls)} URLs${r.urls.length > 25 ? ', the 25 most sent shown' : ''}`);
  }
  if (report.steps && report.holds_concurrency !== undefined) {
    return table('Steps', [['At once', 1], ['Requests', 1], ['Throughput', 1], ['Median', 1], ['p99', 1], ['Errors', 1], ['', 0, 1]],
      report.steps.map((x) => [x.concurrency, count(x.requests), rate(x.requests_per_second), ms(x.median_ms), ms(x.p99_ms), pct(x.error_rate_percent), h('span', { class: 'faint' }, x.note || '')]));
  }
  if (report.targets) {
    return table('Targets', [['Target'], ['Sent', 1], ['Lost', 1], ['p50', 1], ['p99', 1], ['TLS days left', 1, 1]],
      report.targets.map((t) => [h('span', { class: 'mono' }, t.target || t.name), count(t.sent), pct(100 * (t.loss ?? 0)), ms(t.latency_ms && t.latency_ms.p50), ms(t.latency_ms && t.latency_ms.p99),
        t.tls && t.tls.cert ? `${t.tls.cert.days_left}` : '—']));
  }
  return [];
}

const MODE_NAMES = { run: 'Load test', api: 'OpenAPI', flow: 'Flow', replay: 'Replay', ping: 'Ping', ramp: 'Ramp' };

async function viewRun([id]) {
  const meta = await api('GET', `/runs/${encodeURIComponent(id)}`);
  const going = meta.state === 'running';
  const facePic = face(FACE[meta.state] || 'sleeping');
  const badge = h('span', { class: `badge ${going ? 'going' : STATE[meta.state] || ''}` }, going ? 'live' : meta.state);
  const numbers = h('div', {});
  const charts = h('div', { class: 'charts' });
  const feed = h('ul', { class: 'feed' });
  const verdict = h('div', {});
  const breakdown = h('div', {});
  const workers = h('div', {});
  const steer = h('div', { class: 'steer' });
  const url = meta.command.replace(/^pepe /, '').split(' ').at(-1);
  view.append(
    h('div', { class: 'runhead' }, facePic, h('div', { style: 'min-width:0' },
      h('span', { class: 'tag' }, `${MODE_NAMES[meta.mode] || meta.mode} · ${when(meta.started)} · by ${meta.by}`),
      h('h1', {}, titleOf(meta)))),
    command(meta.command.replace(/^pepe /, 'pepe ')),
    h('div', { style: 'height:22px' }),
    steer, numbers,
    h('div', { class: 'panel' }, h('header', {}, h('span', { class: 'what' }, `pepe ${meta.mode === 'run' ? '' : meta.mode}`.trim()), h('span', { class: 'url' }, url), badge), charts),
    workers, verdict, breakdown,
    section('As it went'), h('div', { class: 'card' }, feed));

  const points = { rps: [], p50: [], p99: [] };
  let paused = false;
  const draw = () => {
    charts.replaceChildren(...[
      meta.mode === 'ping' ? null : h('div', {}, h('h3', {}, 'Requests a second'), chart([{ points: points.rps, cls: 'ember', area: true }], (v) => rate(v).replace('/s', ''))),
      h('div', {}, h('h3', {}, 'Latency'), chart([{ points: points.p50, cls: 'violet' }, { points: points.p99, cls: 'bad' }], ms), legend(['violet', 'p50'], ['bad', 'p99']))].filter(Boolean));
    if (meta.mode === 'ping') charts.style.gridTemplateColumns = '1fr';
  };
  draw();

  const showSteer = (on) => {
    steer.replaceChildren();
    if (!on) return;
    const say = async (what, body) => {
      try { await api('POST', `/runs/${encodeURIComponent(id)}/${what}`, body); } catch (e) { steer.append(error(e)); }
    };
    if (meta.mode !== 'ping') {
      const n = h('input', { type: 'number', min: 1, 'aria-label': 'Concurrency', placeholder: 'at once' });
      steer.append(
        h('button', { class: 'primary', onclick: () => say(paused ? 'resume' : 'pause') }, paused ? '▶ Resume' : '❚❚ Pause'),
        n, h('button', { onclick: () => n.value && say('concurrency', { n: Number(n.value) }) }, 'Set concurrency'));
    }
    steer.append(h('button', { class: 'danger', onclick: () => say('stop') }, '■ Stop'));
  };
  showSteer(going);

  const line = (cls, text) => feed.prepend(h('li', {}, h('span', { class: `d ${cls}` }), h('span', {}, text)));
  const progress = (e) => {
    const t = e.elapsed_s ?? e.at_s ?? 0;
    if (e.targets) {
      // A ping: each target's numbers
      const first = e.targets[0] || {};
      points.p50.push([t, first.p50_ms || 0]);
      points.p99.push([t, first.p99_ms || 0]);
      numbers.replaceChildren(nums([['Sent', count(first.sent)], ['Answered', count(first.answered)], ['Last', ms(first.last_ms)], ['p50', ms(first.p50_ms)], ['p99', ms(first.p99_ms)]]));
    } else {
      // A rate over the first few milliseconds says nothing yet
      if (t >= 0.5) points.rps.push([t, e.requests_per_second || 0]);
      points.p50.push([t, e.p50_ms || 0]);
      points.p99.push([t, e.p99_ms || 0]);
      numbers.replaceChildren(nums([['Requests', count(e.requests)], ['Throughput', rate(e.requests_per_second)], ['p50', ms(e.p50_ms)], ['p99', ms(e.p99_ms)], ['Failed', count(e.failed)]]));
      if (e.workers) {
        workers.replaceChildren(section('Workers'), h('div', { class: 'card' }, h('table', {},
          h('thead', {}, h('tr', {}, h('th', {}, 'Worker'), h('th', { class: 'num' }, 'Requests'), h('th', { class: 'num' }, 'Throughput'), h('th', { class: 'num' }, 'p99'))),
          h('tbody', {}, Object.entries(e.workers).map(([name, w]) => h('tr', {}, h('td', { class: 'what' }, name), h('td', { class: 'num' }, count(w.requests)), h('td', { class: 'num' }, rate(w.requests_per_second)), h('td', { class: 'num' }, ms(w.p99_ms))))))));
      }
    }
    draw();
  };

  const controller = new AbortController();
  leaving.push(() => controller.abort());
  follow(id, (type, e) => {
    if (type === 'start') line('note', e.worker ? `${e.worker} started its share` : `started · ${e.load || e.target || ''}`);
    else if (type === 'progress') progress(e);
    else if (type === 'anomaly') line('warn', e.text);
    else if (type === 'steered') { paused = e.paused; showSteer(true); line('note', `${e.what}${e.concurrency ? ` · ${e.concurrency} at once` : ''}`); }
    else if (type === 'worker_done') line(e.exit === 0 ? 'good' : 'bad', `${e.worker} ended its share${e.error ? ` · ${e.error}` : ''}`);
    else if (type === 'report') showReport(e.report);
    else if (type === 'end') {
      badge.className = `badge ${STATE[e.state] || ''}`;
      badge.textContent = e.state;
      showSteer(false);
      if (e.error) verdict.append(error(e.error));
      if (e.state !== 'done' && e.state !== 'stopped') facePic.src = `/ui/img/pepe-${FACE[e.state] || 'sleeping'}.png`;
    }
  }, controller.signal);

  function showReport(report) {
    // A ramp has no verdict: its worst finding stands for one
    const RANK = ['healthy', 'degraded', 'failing'];
    const level = (report.verdict && report.verdict.level)
      || (report.findings || []).map((f) => f.level).sort((a, b) => RANK.indexOf(b) - RANK.indexOf(a))[0];
    if (level) facePic.src = `/ui/img/pepe-${FACE[level] || 'proud'}.png`;
    const sum = report.summary;
    if (sum) {
      const failed = sum.total_requests ? (100 * sum.failed_requests) / sum.total_requests : 0;
      numbers.replaceChildren(nums([['Requests', count(sum.total_requests)], ['Throughput', rate(sum.requests_per_second)], ['p50', ms(sum.latency.median_ms)], ['p99', ms(sum.latency.p99_ms)], ['Failed', pct(failed)]]));
    }
    if (report.steps && report.holds_concurrency !== undefined) {
      // A ramp: what held, and where it stopped keeping up
      const cap = report.capacity;
      numbers.replaceChildren(nums([['Requests', count(report.requests)], ['Held', report.holds_concurrency ? `${report.holds_concurrency} at once` : '—', true],
        ['Capacity', cap ? `${rate(cap.requests_per_second)}${cap.extrapolated ? '*' : ''}` : 'not reached', !cap], ['Failed', count(report.failed_requests)]]));
    }
    if (report.targets) {
      numbers.replaceChildren(nums(report.targets.map((t) => [t.target || t.name, `${ms(t.latency_ms && t.latency_ms.p50)} · ${pct(100 * (t.loss ?? 0))} lost`, true])));
    }
    breakdown.replaceChildren(...parts(report));
    const findings = (report.verdict && report.verdict.findings) || report.findings || (report.targets || []).flatMap((t) => t.findings || []);
    // The reports open as plain links, each with a ticket the server made
    // for this run alone: a page can't send the token with a link
    const links = h('span', { class: 'actions' });
    api('POST', `/runs/${encodeURIComponent(id)}/ticket`).then(({ ticket }) => {
      const link = (format, text, primary) => h('a', {
        class: `button${primary ? ' primary' : ''}`, target: '_blank', rel: 'noopener noreferrer',
        href: `/api/runs/${encodeURIComponent(id)}/report?format=${format}&ticket=${encodeURIComponent(ticket)}`,
      }, text);
      links.replaceChildren(...[meta.mode !== 'ping' ? link('html', 'Full report ↗', true) : null, link('md', 'Markdown ↗'), link('json', 'JSON ↗')].filter(Boolean));
    }).catch((e) => links.replaceChildren(error(e)));
    const words = { healthy: 'Healthy', degraded: 'Degraded', failing: 'Failing' };
    verdict.replaceChildren(section('Verdict'), h('div', { class: 'card' },
      level ? h('div', { class: 'verdict-head' }, pill(level, LEVEL[level]), h('strong', {}, words[level] || level)) : null,
      h('ul', { class: 'findings' }, findings.map((f) => {
        const cls = LEVEL[String(f.level).toLowerCase()] || 'note';
        return h('li', { class: cls === 'good' ? 'good' : cls === 'bad' ? 'bad' : '' }, h('span', { class: `d ${cls}` }), h('span', {}, f.text ?? [f.title, f.detail].filter(Boolean).join(': ')));
      })),
      h('div', { class: 'actions', style: 'margin-top:18px' },
        links,
        meta.mode === 'run' ? h('a', { class: 'button', href: `#/compare?after=${encodeURIComponent(id)}` }, 'Compare with…') : null)));
  }
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
    out.append(section('What moved'), h('div', { class: 'card' },
      h('div', { class: 'verdict-head' }, pill(c.verdict, cls), c.regression ? pill('regression', 'bad') : null,
        h('strong', {}, { faster: 'Faster', better: 'Better', same: 'About the same', slower: 'Slower', worse: 'Worse' }[c.verdict] || c.verdict)),
      h('div', { class: 'table' }, h('table', {},
        h('thead', {}, h('tr', {}, h('th', {}, 'What'), h('th', { class: 'num' }, 'Before'), h('th', { class: 'num' }, 'After'), h('th', { class: 'num' }, 'Change'), h('th', { class: 'num hide-narrow' }, 'Wobbles by'))),
        h('tbody', {}, (c.changes || []).map((x) => {
          const fmt = x.metric.endsWith('_ms') ? ms : x.metric.includes('per_second') ? rate : (v) => (+v).toLocaleString();
          const up = x.change_percent > 0;
          const bad = x.significant && (x.metric.endsWith('_ms') || x.metric.includes('failed') ? up : !up);
          return h('tr', {},
            h('td', { class: 'what' }, x.metric.replace(/_ms$|_percent$/, '').replaceAll('_', ' ')),
            h('td', { class: 'num' }, fmt(x.before)), h('td', { class: 'num' }, fmt(x.after)),
            h('td', { class: `num ${x.significant ? (bad ? 'bad' : 'good') : 'faint'}` }, h('b', {}, x.change_percent === null ? '—' : `${x.change_percent > 0 ? '+' : ''}${x.change_percent}%`)),
            h('td', { class: 'num faint hide-narrow' }, `±${x.noise_percent}%`));
        })))),
      h('ul', { class: 'findings', style: 'margin-top:18px' }, (c.findings || []).map((f) => {
        const k = LEVEL[String(f.level).toLowerCase()] || 'note';
        return h('li', { class: k === 'good' ? 'good' : k === 'bad' ? 'bad' : '' }, h('span', { class: `d ${k}` }), h('span', {}, f.text));
      }))));
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
