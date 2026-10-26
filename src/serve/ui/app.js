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
        h('td', {}, h('strong', {}, titleOf(r)), ' ', h('span', { class: 'mono faint hide-narrow' }, r.command.replace(/^pepe /, '')), r.workers && r.workers.length ? h('div', { class: 'faint' }, `on ${r.workers.join(', ')}`) : null),
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

// What a run is, in a few words: its test, or its mode and what it sends to
function titleOf(meta) {
  if (meta.test) return meta.test;
  const last = meta.command.replace(/^pepe /, '').split(' ').at(-1);
  const file = last.split('/').at(-1);
  return { api: `OpenAPI · ${file}`, flow: `Flow · ${file}`, replay: `Replay · ${file}`, ping: `Ping · ${last}` }[meta.mode] || last;
}

// What a report has besides its totals, by mode: a table each
function parts(report) {
  const codes = (sc) => Object.entries(sc || {}).map(([c, n]) => `${c} ×${count(n)}`).join(' · ');
  const table = (title, head, rows, note) => rows.length ? [h('h2', {}, title), h('div', { class: 'card' },
    note ? h('p', { class: 'faint', style: 'margin-top:0' }, note) : null,
    h('table', {}, h('thead', {}, h('tr', {}, head.map(([label, num, narrow]) => h('th', { class: `${num ? 'num' : ''} ${narrow ? 'hide-narrow' : ''}` }, label)))),
      h('tbody', {}, rows.map((cells) => h('tr', {}, cells.map((c, i) => h('td', { class: `${head[i][1] ? 'num' : ''} ${head[i][2] ? 'hide-narrow' : ''}` }, c)))))))] : [];
  const usual = [['Requests', 1], ['Throughput', 1, 1], ['Median', 1], ['p99', 1], ['Failed', 1], ['Answers', 0, 1]];
  const row = (x) => [count(x.requests), rate(x.requests_per_second), ms(x.median_ms), ms(x.p99_ms), count(x.failed_requests), codes(x.status_codes)];
  if (report.endpoints) return table('Endpoints', [['Endpoint'], ...usual], report.endpoints.map((e) => [h('span', { class: 'mono' }, e.endpoint), ...row(e)]));
  if (report.flow) return table('Steps', [['Step'], ...usual], report.flow.steps.map((x) => [x.step, ...row(x)]),
    `${count(report.flow.chains_started)} chains started, ${count(report.flow.chains_completed)} completed`);
  if (report.replay) {
    const r = report.replay;
    return table('URLs', [['URL'], ['Share of the log', 1, 1], ...usual], r.urls.slice(0, 25).map((u) => [h('span', { class: 'mono' }, `${u.method} ${u.url}`), pct(100 * u.share_in_log), ...row(u)]),
      `${count(r.replayed_from_log)} of the log's ${count(r.requests_in_log)} requests, ${count(r.distinct_urls)} URLs${r.urls.length > 25 ? ', the 25 most sent shown' : ''}`);
  }
  if (report.steps && report.holds_concurrency !== undefined) {
    return table('Steps', [['At once', 1], ['Requests', 1], ['Throughput', 1], ['Median', 1], ['p99', 1], ['Errors', 1], ['', 0, 1]],
      report.steps.map((x) => [x.concurrency, count(x.requests), rate(x.requests_per_second), ms(x.median_ms), ms(x.p99_ms), pct(x.error_rate_percent), x.note || '']));
  }
  if (report.targets) {
    return table('Targets', [['Target'], ['Sent', 1], ['Lost', 1], ['p50', 1], ['p99', 1], ['TLS days left', 1, 1]],
      report.targets.map((t) => [h('span', { class: 'mono' }, t.target || t.name), count(t.sent), pct(100 * (t.loss ?? 0)), ms(t.latency_ms && t.latency_ms.p50), ms(t.latency_ms && t.latency_ms.p99),
        t.tls && t.tls.cert ? `${t.tls.cert.days_left}` : '—']));
  }
  return [];
}

async function viewRun([id]) {
  const meta = await api('GET', `/runs/${encodeURIComponent(id)}`);
  const facePic = face(FACE[meta.state] || 'sleeping');
  const state = pill(meta.state, STATE[meta.state]);
  const kpis = h('div', { class: 'kpis' });
  const charts = h('div', { class: 'two' });
  const feed = h('ul', { class: 'feed' });
  const verdict = h('div', {});
  const breakdown = h('div', {});
  const workers = h('div', {});
  const steer = h('div', { class: 'actions' });
  view.append(
    h('div', { class: 'head' }, facePic, h('div', {},
      h('h1', {}, titleOf(meta)),
      h('div', { class: 'actions' }, state, h('span', { class: 'mono faint' }, meta.command)))),
    steer, kpis, h('div', { class: 'card' }, charts), workers, verdict, breakdown,
    h('h2', {}, 'As it went'), h('div', { class: 'card' }, feed));

  const points = { rps: [], p50: [], p99: [] };
  let paused = false;
  const draw = () => {
    charts.replaceChildren(...[
      meta.mode === 'ping' ? null : h('div', {}, h('h2', { style: 'margin-top:0' }, 'Requests a second'), chart([{ points: points.rps, cls: 'ember', area: true }], (v) => rate(v).replace('/s', ''))),
      h('div', {}, h('h2', { style: 'margin-top:0' }, 'Latency'), chart([{ points: points.p50, cls: 'violet' }, { points: points.p99, cls: 'bad' }], ms), legend(['violet', 'p50'], ['bad', 'p99']))].filter(Boolean));
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
    // A ramp has no verdict: its worst finding stands for one
    const RANK = ['healthy', 'degraded', 'failing'];
    const level = (report.verdict && report.verdict.level)
      || (report.findings || []).map((f) => f.level).sort((a, b) => RANK.indexOf(b) - RANK.indexOf(a))[0];
    if (level) facePic.src = `/ui/img/pepe-${FACE[level] || 'proud'}.png`;
    const sum = report.summary;
    if (sum) {
      const failed = sum.total_requests ? (100 * sum.failed_requests) / sum.total_requests : 0;
      kpis.replaceChildren(kpi('Requests', count(sum.total_requests)), kpi('Throughput', rate(sum.requests_per_second)), kpi('p50', ms(sum.latency.median_ms)), kpi('p99', ms(sum.latency.p99_ms)), kpi('Failed', pct(failed)));
    }
    if (report.steps && report.holds_concurrency !== undefined) {
      // A ramp: what held, and where it stopped keeping up
      const cap = report.capacity;
      kpis.replaceChildren(kpi('Requests', count(report.requests)), kpi('Held', report.holds_concurrency ? `${report.holds_concurrency} at once` : '—'),
        kpi('Capacity', cap ? `${rate(cap.requests_per_second)}${cap.extrapolated ? ' (read past the ramp)' : ''}` : 'not reached'),
        kpi('Failed', count(report.failed_requests)));
    }
    if (report.targets) {
      kpis.replaceChildren(...report.targets.map((t) => kpi(t.target || t.name, `${ms(t.latency_ms && t.latency_ms.p50)} · ${pct(100 * (t.loss ?? 0))} lost`)));
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
      links.replaceChildren(
        ...[meta.mode !== 'ping' ? link('html', 'Full report', true) : null, link('md', 'Markdown'), link('json', 'JSON')].filter(Boolean));
    }).catch((e) => links.replaceChildren(error(e)));
    verdict.replaceChildren(h('h2', {}, 'Verdict'), h('div', { class: 'card' },
      level ? h('p', {}, pill(level, LEVEL[level])) : null,
      h('ul', {}, findings.map((f) => h('li', { class: 'finding' }, h('span', { class: `d ${LEVEL[String(f.level).toLowerCase()] || 'note'}` }),
        f.text ?? [f.title, f.detail].filter(Boolean).join(': ')))),
      h('div', { class: 'actions', style: 'margin-top:14px' },
        links,
        meta.mode === 'run' ? h('a', { class: 'button', href: `#/compare?after=${encodeURIComponent(id)}` }, 'Compare with…') : null)));
  }
}

// ─── A new run ───────────────────────────────────────────────────────────

// What each mode is, as the tabs say it
const MODES = [
  ['run', 'Load test', 'One URL, at a concurrency or a rate'],
  ['api', 'OpenAPI', 'Every endpoint of a spec, or the ones picked'],
  ['flow', 'Flow', 'Steps in order, each fed by the last'],
  ['replay', 'Replay', 'An access log\'s URLs, in their proportions'],
  ['ping', 'Ping', 'A request now and then, split by phase'],
  ['ramp', 'Ramp', 'More at once, step by step, to find the capacity'],
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
  const field = h('div', { class: 'field' }, h('label', {}, label), h('div', { class: 'actions' }, select, upload), said);
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
    const card = h('div', { class: 'card step' },
      h('div', { class: 'actions', style: 'justify-content:space-between' }, h('strong', {}, `Step ${steps.length + 1}`),
        h('button', { type: 'button', class: 'danger', onclick: () => { steps.splice(steps.indexOf(f), 1); card.remove(); } }, 'Remove')),
      h('div', { class: 'fields' }, field('Name', f.name), field('Method', f.method), field('Expect', f.expect)),
      field('URL', f.url, '{{holes}} are filled from the values below, the CSV and earlier captures'),
      h('div', { class: 'two' }, field('Headers, one a line', f.headers), field('Body', f.body)),
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
    list, h('button', { type: 'button', onclick: () => add() }, 'Add a step'));
  el.write = write;
  return el;
}

const field = (label, input, hint) => h('div', { class: 'field' }, h('label', {}, label, input), hint ? h('div', { class: 'hint' }, hint) : null);

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
    const box = h('input', { type: 'checkbox', style: 'width:auto' });
    const el = h('label', { style: 'display:flex;gap:8px;align-items:baseline;color:inherit;font-size:14px;margin:6px 0' }, box, h('span', {}, label, hint ? h('span', { class: 'hint', style: 'display:block' }, hint) : null));
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
    // Ping
    every: inp({ placeholder: '1s' }),
    slo: inp({ placeholder: 'total=500,ttfb=200' }),
    keepAlive: check('Keep the connection', 'as a browser would: DNS, connect and TLS only in the first'),
    allIps: check('Every address the name resolves to'),
    tcp: check('TCP connect only', 'host:port targets'),
    // Ramp
    from: inp({ type: 'number', min: 1, placeholder: '1' }),
    to: inp({ type: 'number', min: 1, placeholder: '100' }),
    step: inp({ type: 'number', min: 1, placeholder: '10' }),
    stepFor: inp({ placeholder: '5s' }),
    until: h('textarea', { placeholder: 'p99 > 500ms\nerrors > 1%' }),
    // API
    server: inp({ placeholder: 'the spec\'s own' }),
    auth: h('textarea', { placeholder: 'bearer=TOKEN\napiKey=KEY' }),
    set: h('textarea', { placeholder: 'id=1,2,3' }),
    includeWrites: check('Include POST, PUT, PATCH and DELETE', 'off: only reads are sent'),
    // Replay
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
      curlSaid.append(h('div', { class: 'hint good' }, 'Filled in from the command.'), ...notes.map((n) => h('div', { class: 'hint warn' }, n)));
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
      const boxes = spec.operations.map((o) => {
        const box = h('input', { type: 'checkbox', style: 'width:auto', checked: o.method === 'GET' });
        specOps.push([box, `${o.method} ${o.path}`]);
        return h('tr', {}, h('td', {}, box), h('td', { class: 'mono' }, o.method), h('td', { class: 'mono' }, o.path), h('td', { class: 'hide-narrow faint' }, o.tag), h('td', { class: 'hide-narrow' }, o.summary));
      });
      const all = (on) => specOps.forEach(([b]) => (b.checked = on));
      opsBox.append(
        h('p', {}, h('strong', {}, spec.title || 'The spec'), ` ${spec.version || ''} · ${spec.base_url || 'no server named'} · ${spec.operations.length} operations`),
        h('div', { class: 'actions' }, h('button', { type: 'button', onclick: () => all(true) }, 'All'), h('button', { type: 'button', onclick: () => all(false) }, 'None'),
          h('button', { type: 'button', onclick: () => specOps.forEach(([b, k]) => (b.checked = k.startsWith('GET '))) }, 'Reads only')),
        h('table', { style: 'margin-top:8px' }, h('tbody', {}, boxes)),
        h('div', {}, spec.auth.length ? h('p', { class: 'hint' }, `It asks for auth: ${spec.auth.join(', ')}. Give it below as name=value.`) : null));
    } catch (e) { opsBox.append(error(e)); }
  };
  const specFile = filePicker('The spec', '.json,.yaml,.yml', 'An OpenAPI 3 or Swagger 2 file, JSON or YAML', showSpec);
  const flowFile = filePicker('A flow file', '.toml', 'Or write the steps below');
  const logFile = filePicker('The access log', '.log,.gz,.txt', 'nginx\'s or Apache\'s combined format, as it is (gzip too)');
  const varsFile = filePicker('Values from a CSV', '.csv', 'Each request (or chain) takes the next row: its first line names the {{holes}}');
  const flowSteps = flowEditor();
  let flowFrom = 'steps';
  const flowSwitch = h('div', { class: 'actions' });

  const picks = workersData.workers.filter((w) => !w.busy).map((w) => {
    const box = h('input', { type: 'checkbox', value: w.name, style: 'width:auto' });
    return h('label', { style: 'display:inline-flex;gap:6px;margin-right:14px;color:inherit' }, box, w.name);
  });

  // What each mode asks for
  const card = (title, ...kids) => h('div', { class: 'card' }, h('h2', { style: 'margin-top:0' }, title), kids);
  const request = (withBody = true) => card('The request',
    h('div', { class: 'fields' }, field('Method', f.method)),
    h('div', { class: withBody ? 'two' : '' }, field('Headers, one a line', f.headers), withBody ? field('Body', f.body) : null));
  const load = (unit = 'Requests') => card('How much',
    h('div', { class: 'fields' }, field(unit, f.requests), field('Or for', f.duration, '30s, 5m'), field('At once', f.concurrency), field('A second', f.rate, 'an arrival rate, as users arrive'), field('Warm-up', f.warmup, 'sent, not counted')),
    picks.length && (mode === 'run') ? field('Share it out to', h('div', {}, picks), 'workers that joined and are free') : null);
  const limits = (label = 'Fail if, one a line') => card('Limits', field(label, f.failIf, 'the run fails, exit 4, when one is crossed: p99 > 300ms, errors > 1%, rps < 500'));
  const advanced = (withBody = true) => h('details', { class: 'card' }, h('summary', {}, 'More'),
    h('div', { class: 'fields', style: 'margin-top:12px' }, field('Timeout, seconds', f.timeout), mode !== 'ping' && mode !== 'ramp' ? field('Threads', f.threads) : null, field('User-Agent', f.userAgent), field('Proxy', f.proxy, 'held to --allow-host like a target')),
    f.insecure, mode !== 'ping' ? [f.noKeepalive, f.noRedirects, withBody ? f.noCompression : null] : null);

  const body = h('div', {});
  const draw = () => {
    for (const a of tabs.querySelectorAll('button')) a.classList.toggle('on', a.dataset.mode === mode);
    const about = MODES.find((m) => m[0] === mode)[2];
    let parts;
    if (mode === 'run') {
      parts = [
        card('Where', field('URL', f.url), h('details', {}, h('summary', {}, 'Or paste a curl command'), curl, h('div', { class: 'actions', style: 'margin-top:8px' }, h('button', { type: 'button', onclick: fromCurl }, 'Fill in from it')), curlSaid)),
        request(), load(), limits(), card('Values', varsFile), advanced()];
    } else if (mode === 'api') {
      parts = [card('The spec', specFile, opsBox, h('div', { class: 'fields' }, field('Server', f.server, 'its URL instead of the spec\'s')), f.includeWrites,
        h('div', { class: 'two' }, field('Auth, one a line', f.auth, 'scheme=value, as the spec names it'), field('Values, one a line', f.set, 'name=1,2,3: rotated through'))),
      request(), load(), limits(), advanced()];
    } else if (mode === 'flow') {
      flowSwitch.replaceChildren(
        h('button', { type: 'button', class: flowFrom === 'steps' ? 'primary' : '', onclick: () => { flowFrom = 'steps'; draw(); } }, 'Write the steps'),
        h('button', { type: 'button', class: flowFrom === 'file' ? 'primary' : '', onclick: () => { flowFrom = 'file'; draw(); } }, 'Use a flow file'));
      parts = [card('The steps', flowSwitch, h('div', { style: 'margin-top:14px' }, flowFrom === 'file' ? flowFile : flowSteps)),
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
    body.replaceChildren(h('p', { class: 'sub' }, about), ...parts.flat().filter(Boolean));
  };
  const tabs = h('div', { class: 'tabs', role: 'tablist' }, MODES.map(([m, label]) =>
    h('button', { type: 'button', role: 'tab', 'data-mode': m, onclick: () => { mode = m; draw(); } }, label)));

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
        h('div', {}, h('strong', {}, 'Would send '), plan.load),
        (plan.targets || []).slice(0, 12).map((t) => h('div', { class: 'mono' }, t)),
        plan.more_targets ? h('div', { class: 'faint' }, `… and ${plan.more_targets} more`) : null,
        (plan.settings || []).length ? h('div', { class: 'faint' }, plan.settings.join(' · ')) : null,
        plan.guard ? h('div', { class: 'faint' }, `Held to ${plan.guard}`) : null,
        (plan.notes || []).map((n) => h('div', { class: 'warn' }, n)),
        h('div', { class: 'faint' }, 'Nothing sent yet.')));
    } catch (e) { out.append(error(e)); }
  };
  const start = async () => {
    out.replaceChildren();
    try {
      const run = await api('POST', '/runs', await args());
      location.hash = `#/run/${encodeURIComponent(run.id)}`;
    } catch (e) { out.append(error(e)); }
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
    h('h1', {}, query.test ? `Run ${query.test}` : 'New run'),
    h('p', { class: 'sub' }, 'Checked against the server\'s guardrails before anything is sent.'),
    tabs,
    h('form', { onsubmit: (e) => { e.preventDefault(); start(); } },
      body,
      h('div', { class: 'actions sticky' },
        h('button', { class: 'primary', type: 'submit' }, 'Start'),
        h('button', { type: 'button', onclick: plan }, 'Check the plan'),
        h('button', { type: 'button', onclick: save }, 'Save as a test')),
      out));
  draw();
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
