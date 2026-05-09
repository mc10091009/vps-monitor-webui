// vps-monitor SPA — vanilla JS, no dependencies.

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

const HEADERS_JSON = { 'content-type': 'application/json', 'x-requested-with': 'fetch' };
const HEADERS_XHR = { 'x-requested-with': 'fetch' };

async function api(path, opts = {}) {
  const r = await fetch(path, {
    ...opts,
    headers: { ...HEADERS_XHR, ...(opts.headers || {}) },
  });
  if (r.status === 401) {
    location.href = '/login.html';
    throw new Error('unauthorized');
  }
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `http ${r.status}`);
  }
  if (r.headers.get('content-type')?.includes('application/json')) return r.json();
  return r.text();
}

function fmtBytes(n) {
  if (!Number.isFinite(n)) return '—';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}
function fmtPct(n) { return `${(n || 0).toFixed(1)}%`; }
function fmtUptime(s) {
  if (!s) return '';
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return `up ${d}d ${h}h ${m}m`;
}
function fmtTs(ts) {
  return new Date(ts * 1000).toLocaleString();
}

// SVG sparkline
class Spark {
  constructor(svg, max = 60) {
    this.svg = svg;
    this.max = max;
    this.data = [];
  }
  push(v) {
    this.data.push(v);
    if (this.data.length > this.max) this.data.shift();
    this.render();
  }
  render() {
    const w = 200, h = 60;
    if (!this.data.length) { this.svg.innerHTML = ''; return; }
    const lo = Math.min(...this.data, 0);
    const hi = Math.max(...this.data, 1);
    const range = hi - lo || 1;
    const pts = this.data.map((v, i) => {
      const x = (i / (this.max - 1)) * w;
      const y = h - ((v - lo) / range) * h;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    }).join(' ');
    this.svg.innerHTML =
      `<polyline fill="none" stroke="currentColor" stroke-width="1.5" points="${pts}"/>`;
  }
}

const sparkCpu = new Spark($('#spark-cpu'));
const sparkMem = new Spark($('#spark-mem'));
const sparkNet = new Spark($('#spark-net'));
let lastNet = null;

function renderOverview(s) {
  $('#host').textContent = `${s.host || ''} · ${s.os || ''}`;
  $('#cpu-pct').textContent = fmtPct(s.cpu_percent);
  sparkCpu.push(s.cpu_percent || 0);
  $('#cpu-cores').textContent = `${(s.cpu_per_core || []).length} cores`;

  const memPct = s.mem_total ? (s.mem_used / s.mem_total) * 100 : 0;
  $('#mem-pct').textContent = fmtPct(memPct);
  sparkMem.push(memPct);
  $('#mem-detail').textContent = `${fmtBytes(s.mem_used)} / ${fmtBytes(s.mem_total)}` +
    (s.swap_total ? ` · swap ${fmtBytes(s.swap_used)} / ${fmtBytes(s.swap_total)}` : '');

  // network: rate from last sample
  let rateRx = 0, rateTx = 0;
  if (lastNet) {
    const dt = (s.ts - lastNet.ts) || 1;
    rateRx = Math.max(0, (s.net_rx_bps - lastNet.rx)) / dt;
    rateTx = Math.max(0, (s.net_tx_bps - lastNet.tx)) / dt;
  }
  lastNet = { ts: s.ts, rx: s.net_rx_bps, tx: s.net_tx_bps };
  $('#net-detail').textContent = `↓ ${fmtBytes(rateRx)}/s · ↑ ${fmtBytes(rateTx)}/s`;
  sparkNet.push(rateRx + rateTx);
  $('#net-tot').textContent = `total ↓ ${fmtBytes(s.net_rx_bps)} · ↑ ${fmtBytes(s.net_tx_bps)}`;

  $('#load').textContent = (s.load_avg || []).map((n) => n.toFixed(2)).join('  ');
  $('#uptime').textContent = fmtUptime(s.uptime);
  $('#kernel').textContent = s.kernel ? `kernel ${s.kernel}` : '';

  const tbody = $('#disks tbody');
  tbody.innerHTML = '';
  for (const d of (s.disks || [])) {
    const used = d.total - d.available;
    const pct = d.total ? (used / d.total) * 100 : 0;
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${escapeHtml(d.mount)}</td><td>${escapeHtml(d.fs)}</td>` +
      `<td>${fmtBytes(used)}</td><td>${fmtBytes(d.total)}</td>` +
      `<td><div class="bar"><span style="width:${pct.toFixed(1)}%"></span></div> ${pct.toFixed(1)}%</td>`;
    tbody.appendChild(tr);
  }
}

function escapeHtml(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

async function loadProcesses() {
  const sort = $('#proc-sort').value;
  const limit = $('#proc-limit').value;
  const rows = await api(`/api/processes?sort=${sort}&limit=${limit}`);
  const tbody = $('#processes tbody');
  tbody.innerHTML = '';
  for (const p of rows) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${p.pid}</td><td>${escapeHtml(p.user || '')}</td>` +
      `<td>${escapeHtml(p.name)}</td><td>${p.cpu.toFixed(1)}</td>` +
      `<td>${fmtBytes(p.mem)}</td><td class="cmd">${escapeHtml(p.cmd)}</td>`;
    tbody.appendChild(tr);
  }
}

async function loadPm2() {
  const apps = await api('/api/pm2');
  const list = $('#pm2-list');
  list.innerHTML = '';
  if (!apps.length) { list.innerHTML = '<p class="muted">No PM2 apps (or pm2 not installed).</p>'; return; }
  for (const a of apps) {
    const card = document.createElement('div');
    card.className = 'card';
    card.innerHTML = `
      <div class="row"><h3>${escapeHtml(a.name)}</h3>
        <span class="badge ${a.status === 'online' ? 'ok' : 'warn'}">${escapeHtml(a.status)}</span></div>
      <div class="muted small">PID ${a.pid || '—'} · CPU ${a.cpu.toFixed(1)}% · ${fmtBytes(a.memory)} · restarts ${a.restart_count}</div>
      <div class="muted small">${escapeHtml(a.cwd || '')}</div>
      <div class="row"><button class="logs">View logs</button></div>`;
    card.querySelector('.logs').addEventListener('click', () => openLogs('pm2', a.name, `PM2 · ${a.name}`));
    list.appendChild(card);
  }
}

async function loadDocker() {
  const cs = await api('/api/docker');
  const list = $('#docker-list');
  list.innerHTML = '';
  if (!cs.length) { list.innerHTML = '<p class="muted">No containers (or docker socket not accessible).</p>'; return; }
  for (const c of cs) {
    const name = c.names[0] || c.id.slice(0, 12);
    const card = document.createElement('div');
    card.className = 'card';
    card.innerHTML = `
      <div class="row"><h3>${escapeHtml(name)}</h3>
        <span class="badge ${c.state === 'running' ? 'ok' : 'warn'}">${escapeHtml(c.state)}</span></div>
      <div class="muted small">${escapeHtml(c.image)}</div>
      <div class="muted small">${escapeHtml(c.status)}</div>
      <div class="muted small">${escapeHtml(c.ports.join(', '))}</div>
      <div class="row"><button class="logs">View logs</button></div>`;
    card.querySelector('.logs').addEventListener('click', () => openLogs('docker', c.id, `Docker · ${name}`));
    list.appendChild(card);
  }
}

async function loadServices() {
  const f = $('#svc-filter').value;
  const url = f ? `/api/services?state=${f}` : '/api/services';
  const rows = await api(url);
  const tbody = $('#services tbody');
  tbody.innerHTML = '';
  for (const s of rows) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${escapeHtml(s.unit)}</td><td>${escapeHtml(s.load)}</td>` +
      `<td><span class="badge ${s.active === 'active' ? 'ok' : (s.active === 'failed' ? 'fail' : 'warn')}">${escapeHtml(s.active)}</span></td>` +
      `<td>${escapeHtml(s.sub)}</td><td>${escapeHtml(s.description)}</td>` +
      `<td><button class="logs" data-unit="${escapeHtml(s.unit)}">journal</button></td>`;
    tbody.appendChild(tr);
  }
  $$('#services button.logs').forEach((b) =>
    b.addEventListener('click', () => openLogs('journal', b.dataset.unit, `journal · ${b.dataset.unit}`)));
}

async function loadAudit() {
  const rows = await api('/api/audit?limit=200');
  const tbody = $('#audit tbody');
  tbody.innerHTML = '';
  for (const r of rows) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${fmtTs(r.ts)}</td><td>${escapeHtml(r.username || '')}</td>` +
      `<td>${escapeHtml(r.ip || '')}</td><td>${escapeHtml(r.action)}</td>` +
      `<td>${escapeHtml(r.target || '')}</td><td>${escapeHtml(r.detail || '')}</td>`;
    tbody.appendChild(tr);
  }
}

// Logs modal — WebSocket
let logSocket = null;
let logPaused = false;
const MAX_LOG_LINES = 5000;
function openLogs(kind, ident, title) {
  closeLogs();
  $('#modal-title').textContent = title;
  $('#log-output').textContent = '';
  $('#modal').classList.remove('hidden');
  logPaused = false;
  $('#log-pause').textContent = 'Pause';

  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const url = `${proto}://${location.host}/api/logs/${kind}/${encodeURIComponent(ident)}`;
  logSocket = new WebSocket(url);
  logSocket.addEventListener('message', (ev) => {
    if (logPaused) return;
    const out = $('#log-output');
    const filter = $('#log-filter').value.toLowerCase();
    const lines = String(ev.data).split('\n');
    let added = '';
    for (const line of lines) {
      if (!line) continue;
      if (filter && !line.toLowerCase().includes(filter)) continue;
      added += line + '\n';
    }
    if (added) {
      out.appendChild(document.createTextNode(added));
      // trim head if too many lines
      if (out.childNodes.length > 64) {
        // collapse old text into a single string we can trim safely
        const all = out.textContent.split('\n');
        if (all.length > MAX_LOG_LINES) {
          out.textContent = all.slice(all.length - MAX_LOG_LINES).join('\n');
        }
      }
      out.scrollTop = out.scrollHeight;
    }
  });
  logSocket.addEventListener('close', () => {
    const out = $('#log-output');
    out.appendChild(document.createTextNode('\n[connection closed]\n'));
  });
}
function closeLogs() {
  if (logSocket) { try { logSocket.close(); } catch {} logSocket = null; }
  $('#modal').classList.add('hidden');
}

function bindEvents() {
  $$('aside nav a').forEach((a) => a.addEventListener('click', () => activate(a.dataset.tab)));
  $('#logout').addEventListener('click', async () => {
    try {
      await fetch('/api/auth/logout', { method: 'POST', headers: HEADERS_JSON });
    } catch {}
    location.href = '/login.html';
  });
  $('#proc-refresh').addEventListener('click', loadProcesses);
  $('#proc-sort').addEventListener('change', loadProcesses);
  $('#proc-limit').addEventListener('change', loadProcesses);
  $('#pm2-refresh').addEventListener('click', loadPm2);
  $('#docker-refresh').addEventListener('click', loadDocker);
  $('#svc-refresh').addEventListener('click', loadServices);
  $('#svc-filter').addEventListener('change', loadServices);
  $('#audit-refresh').addEventListener('click', loadAudit);
  $('#log-close').addEventListener('click', closeLogs);
  $('#log-pause').addEventListener('click', () => {
    logPaused = !logPaused;
    $('#log-pause').textContent = logPaused ? 'Resume' : 'Pause';
  });
  $('#log-clear').addEventListener('click', () => { $('#log-output').textContent = ''; });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeLogs(); });
}

function activate(tab) {
  $$('aside nav a').forEach((a) => a.classList.toggle('active', a.dataset.tab === tab));
  $$('main .tab').forEach((s) => s.classList.toggle('active', s.id === `tab-${tab}`));
  if (tab === 'processes') loadProcesses();
  if (tab === 'pm2') loadPm2();
  if (tab === 'docker') loadDocker();
  if (tab === 'services') loadServices();
  if (tab === 'audit') loadAudit();
}

async function bootstrap() {
  try {
    const me = await api('/api/auth/me');
    $('#who').textContent = me.username;
    $('#app').classList.remove('hidden');
    bindEvents();
    activate('overview');

    // initial snapshot + SSE stream
    try {
      const initial = await api('/api/metrics');
      renderOverview(initial);
    } catch {}

    const es = new EventSource('/api/metrics/stream');
    es.addEventListener('metrics', (ev) => {
      try { renderOverview(JSON.parse(ev.data)); } catch {}
    });
  } catch {
    // unauthorized — api() already redirects
  }
}

bootstrap();
