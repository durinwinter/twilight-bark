import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ── Tab Switching ──────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', async () => {
  const navItems = document.querySelectorAll('.nav-item');
  const tabContents = document.querySelectorAll('.tab-content');

  navItems.forEach(item => {
    item.addEventListener('click', () => {
      navItems.forEach(i => i.classList.remove('active'));
      item.classList.add('active');
      const tabId = item.getAttribute('data-tab');
      tabContents.forEach(content => {
        content.classList.remove('active');
        if (content.id === `tab-${tabId}`) content.classList.add('active');
      });
    });
  });

  // ── Daemon Status ────────────────────────────────────────────────────────────
  const statusDot   = document.getElementById('daemon-status-dot');
  const statusLabel = document.getElementById('daemon-status-label');

  async function refreshDaemonStatus() {
    try {
      const s = await invoke('get_daemon_status');
      if (s.running) {
        statusDot.className = 'status-dot online';
        statusLabel.textContent = `Daemon Online (pid ${s.pid})`;
        daemonRunning = true;
        if (daemonBtn) {
          daemonBtn.innerText = 'Stop Daemon';
          daemonBtn.style.background = '#ef4444';
        }
      } else {
        statusDot.className = 'status-dot offline';
        statusLabel.textContent = 'Daemon Offline';
        daemonRunning = false;
        if (daemonBtn) {
          daemonBtn.innerText = 'Start Daemon';
          daemonBtn.style.removeProperty('background');
        }
      }
    } catch (_) {}
  }

  refreshDaemonStatus();
  setInterval(refreshDaemonStatus, 8000);

  // ── Connect to Bus ───────────────────────────────────────────────────────────
  try {
    const nodeId = await invoke('get_node_id');
    await invoke('connect_bus', { tenant: 'twilight-bark', nodeId });
  } catch (e) {
    console.error('Failed to connect to bus:', e);
  }

  // ── Live Bus Monitor ─────────────────────────────────────────────────────────
  const monitor = document.getElementById('bus-monitor');

  listen('bus-traffic', (event) => {
    const env = event.payload;
    appendLog(env.source?.node_id || 'unknown', 'TRAFFIC', env.source?.agent_name || 'unknown', JSON.stringify(env.payload));
  });

  listen('bus-heartbeat', (event) => {
    const hb = event.payload;
    appendLog('local', 'HEARTBEAT', hb.node_id, `status=${hb.status}`);
  });

  listen('bus-presence', () => {
    // Presence event — refresh agents panel if it's visible
    const agentsTab = document.getElementById('tab-agents');
    if (agentsTab && agentsTab.classList.contains('active')) refreshAgents();
  });

  function appendLog(provider, kind, source, data) {
    const row = document.createElement('div');
    row.style.cssText = 'display:grid;grid-template-columns:100px 120px 100px 180px 1fr;padding:0.75rem 1.5rem;border-bottom:1px solid rgba(255,255,255,0.05);font-size:0.85rem;';
    const time = new Date().toLocaleTimeString();
    row.innerHTML = `
      <span style="color:#71717a">${time}</span>
      <span style="color:#6366f1;font-weight:600">${provider}</span>
      <span style="color:#8b5cf6;font-weight:700">${kind}</span>
      <span style="color:#a1a1aa">${source}</span>
      <span style="color:#d4d4d8;font-family:monospace;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${data}</span>
    `;
    monitor.appendChild(row);
    if (monitor.children.length > 51) monitor.removeChild(monitor.children[1]);
    monitor.scrollTop = monitor.scrollHeight;
  }

  // ── Agents Panel ─────────────────────────────────────────────────────────────
  const agentsGrid   = document.getElementById('agents-grid');
  const refreshAgentsBtn = document.getElementById('refresh-agents');

  const STATUS_LABELS = { 0: 'Unknown', 1: 'Online', 2: 'Offline', 3: 'Busy' };
  const STATUS_COLORS = { 0: '#71717a', 1: '#10b981', 2: '#ef4444', 3: '#f59e0b' };

  async function refreshAgents() {
    try {
      const agents = await invoke('get_fabric_agents');
      agentsGrid.innerHTML = '';

      if (agents.length === 0) {
        agentsGrid.innerHTML = '<div class="card glass" style="color:var(--text-secondary);font-size:0.85rem;">No agents online. Start the daemon and connect an LLM client.</div>';
        return;
      }

      agents.forEach(snap => {
        const id   = snap.identity;
        const age  = Math.round((Date.now() - snap.last_seen_ms) / 1000);
        const ageStr = age < 60 ? `${age}s ago` : `${Math.round(age/60)}m ago`;
        const statusColor = STATUS_COLORS[snap.status] ?? '#71717a';
        const statusLabel = STATUS_LABELS[snap.status] ?? 'Unknown';
        const nodeLabel   = id.node_id || id.site || id.node_uuid?.slice(0, 8);
        const agentLabel  = id.agent_name || 'unnamed';

        const card = document.createElement('div');
        card.className = 'card glass';
        card.style.cssText = 'display:flex;flex-direction:column;gap:0.6rem;';
        card.innerHTML = `
          <div style="display:flex;justify-content:space-between;align-items:center;">
            <span style="font-weight:700;font-size:1rem;">${agentLabel}</span>
            <span style="padding:0.2rem 0.6rem;border-radius:99px;font-size:0.7rem;font-weight:700;background:${statusColor}22;color:${statusColor};border:1px solid ${statusColor}44;">${statusLabel}</span>
          </div>
          <div style="font-size:0.78rem;color:var(--text-secondary);display:flex;flex-direction:column;gap:0.2rem;">
            <div><span style="color:#71717a;">Node</span>&nbsp;&nbsp;&nbsp;${nodeLabel || '—'}</div>
            <div><span style="color:#71717a;">Role</span>&nbsp;&nbsp;&nbsp;${id.role || '—'}</div>
            <div><span style="color:#71717a;">Tenant</span>&nbsp;${id.tenant || '—'}</div>
            <div><span style="color:#71717a;">UUID</span>&nbsp;&nbsp;${(id.node_uuid || '').slice(0, 16)}...</div>
          </div>
          <div style="font-size:0.72rem;color:#52525b;text-align:right;">last seen ${ageStr}</div>
        `;
        agentsGrid.appendChild(card);
      });
    } catch (e) {
      agentsGrid.innerHTML = `<div class="card glass" style="color:#ef4444;">Error: ${e}</div>`;
    }
  }

  refreshAgentsBtn.addEventListener('click', refreshAgents);
  setTimeout(refreshAgents, 2000);
  setInterval(refreshAgents, 10000);

  // ── Traffic Analytics — SVG Sankey ───────────────────────────────────────────
  const sankeyDiv = document.getElementById('sankey-chart');

  function renderSankey(container, stats) {
    const { edges } = stats;
    if (!edges || edges.length === 0) {
      container.innerHTML = '<div class="chart-placeholder">Waiting for traffic data...</div>';
      return;
    }

    const W = container.clientWidth || 680;
    const sources = [...new Set(edges.map(e => e.source))].sort();
    const targets = [...new Set(edges.map(e => e.target))].sort();

    const ROW_H = 54;
    const NODE_W = 148;
    const NODE_H = 34;
    const PAD_V = 20;

    const H = Math.max(sources.length, targets.length) * ROW_H + PAD_V * 2;
    const maxW = Math.max(...edges.map(e => e.weight), 1);

    const COLORS = ['#6366f1', '#8b5cf6', '#ec4899', '#10b981', '#f59e0b', '#3b82f6', '#ef4444', '#14b8a6'];
    const col = (i) => COLORS[i % COLORS.length];

    const srcSpacing = H / Math.max(sources.length, 1);
    const tgtSpacing = H / Math.max(targets.length, 1);
    const srcY = (i) => srcSpacing * i + srcSpacing / 2;
    const tgtY = (i) => tgtSpacing * i + tgtSpacing / 2;

    const srcX2 = NODE_W;
    const tgtX1 = W - NODE_W;

    let svg = `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" xmlns="http://www.w3.org/2000/svg">`;

    // Gradient defs for flow paths
    svg += '<defs>';
    sources.forEach((_, i) => {
      svg += `<linearGradient id="sg${i}" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0%" stop-color="${col(i)}" stop-opacity="0.7"/>
        <stop offset="100%" stop-color="${col(i)}" stop-opacity="0.2"/>
      </linearGradient>`;
    });
    svg += '</defs>';

    // Flow paths (drawn behind nodes)
    edges.forEach(edge => {
      const si = sources.indexOf(edge.source);
      const ti = targets.indexOf(edge.target);
      if (si < 0 || ti < 0) return;

      const sw = Math.max(2, Math.round((edge.weight / maxW) * 22));
      const y1 = srcY(si);
      const y2 = tgtY(ti);
      const cpX = W / 2;

      svg += `<path d="M ${srcX2} ${y1} C ${cpX} ${y1}, ${cpX} ${y2}, ${tgtX1} ${y2}"
        fill="none" stroke="url(#sg${si})" stroke-width="${sw}" opacity="0.55"/>`;

      // Weight badge at midpoint
      const midY = (y1 + y2) / 2 - sw / 2 - 5;
      svg += `<text x="${W / 2}" y="${midY}" text-anchor="middle" fill="#a1a1aa" font-size="10" font-family="monospace">${edge.weight}</text>`;
    });

    // Source nodes (left column)
    sources.forEach((src, i) => {
      const cy = srcY(i);
      const y = cy - NODE_H / 2;
      const c = col(i);
      const lbl = src.length > 17 ? src.slice(0, 14) + '…' : src;
      const total = edges.filter(e => e.source === src).reduce((s, e) => s + e.weight, 0);

      svg += `<rect x="0" y="${y}" width="${NODE_W}" height="${NODE_H}" rx="6" fill="${c}" fill-opacity="0.15" stroke="${c}" stroke-width="1.5"/>`;
      svg += `<text x="${NODE_W / 2}" y="${cy - 4}" text-anchor="middle" fill="white" font-size="11" font-family="monospace" font-weight="600">${lbl}</text>`;
      svg += `<text x="${NODE_W / 2}" y="${cy + 10}" text-anchor="middle" fill="${c}" font-size="9" font-family="monospace">${total} sent</text>`;
    });

    // Target nodes (right column)
    targets.forEach((tgt, i) => {
      const cy = tgtY(i);
      const y = cy - NODE_H / 2;
      const si = sources.indexOf(tgt);
      const c = si >= 0 ? col(si) : col(sources.length + i);
      const lbl = tgt.length > 17 ? tgt.slice(0, 14) + '…' : tgt;
      const total = edges.filter(e => e.target === tgt).reduce((s, e) => s + e.weight, 0);

      svg += `<rect x="${tgtX1}" y="${y}" width="${NODE_W}" height="${NODE_H}" rx="6" fill="${c}" fill-opacity="0.15" stroke="${c}" stroke-width="1.5"/>`;
      svg += `<text x="${tgtX1 + NODE_W / 2}" y="${cy - 4}" text-anchor="middle" fill="white" font-size="11" font-family="monospace" font-weight="600">${lbl}</text>`;
      svg += `<text x="${tgtX1 + NODE_W / 2}" y="${cy + 10}" text-anchor="middle" fill="${c}" font-size="9" font-family="monospace">${total} recv</text>`;
    });

    svg += '</svg>';
    container.innerHTML = svg;
  }

  setInterval(async () => {
    try {
      const stats = await invoke('get_analytics');
      renderSankey(sankeyDiv, stats);
    } catch (e) {
      console.error('Analytics fetch failed:', e);
    }
  }, 3000);

  // ── Zenoh Admin Tree ─────────────────────────────────────────────────────────
  const adminTree = document.getElementById('admin-tree');
  const refreshBtn = document.getElementById('refresh-admin');

  async function refreshAdmin() {
    adminTree.innerHTML = '<div class="placeholder">Querying...</div>';
    try {
      const keys = await invoke('get_admin_data');
      if (keys.length === 0) {
        adminTree.innerHTML = '<div class="placeholder">No admin data found.</div>';
        return;
      }
      adminTree.innerHTML = '';
      keys.sort().forEach(key => {
        const item = document.createElement('div');
        item.style.cssText = 'padding:0.25rem 0;border-bottom:1px solid rgba(255,255,255,0.02);';
        let color = '#a1a1aa';
        if (key.includes('sessions')) color = '#8b5cf6';
        if (key.includes('config'))   color = '#10b981';
        item.innerHTML = `<span style="color:${color}">●</span> ${key}`;
        adminTree.appendChild(item);
      });
    } catch (e) {
      adminTree.innerHTML = `<div style="color:#ef4444">Error: ${e}</div>`;
    }
  }

  refreshBtn.addEventListener('click', refreshAdmin);
  setTimeout(refreshAdmin, 3000);

  // ── Security Tab — Enrollment & Daemon ───────────────────────────────────────
  const enrollBtn = document.getElementById('btn-enroll');
  const tokenPath = document.getElementById('token-path');

  enrollBtn.addEventListener('click', async () => {
    const path = tokenPath.value;
    if (!path) { alert('Please provide a path to a JWT token.'); return; }
    enrollBtn.innerText = 'Enrolling...';
    enrollBtn.disabled = true;
    try {
      await invoke('enroll_identity', { path });
      enrollBtn.innerText = 'Enrolled!';
      enrollBtn.style.background = '#10b981';
    } catch (e) {
      alert(`Enrollment failed: ${e}`);
      enrollBtn.disabled = false;
      enrollBtn.innerText = 'Enroll Identity';
    }
  });

  const daemonBtn  = document.getElementById('btn-toggle-daemon');
  const daemonRole = document.getElementById('daemon-role');
  let daemonRunning = false;

  if (daemonBtn) {
    daemonBtn.addEventListener('click', async () => {
      if (!daemonRunning) {
        daemonBtn.innerText = 'Starting...';
        daemonBtn.disabled = true;
        try {
          await invoke('start_daemon', { role: daemonRole.value });
          daemonRunning = true;
          daemonBtn.innerText = 'Stop Daemon';
          daemonBtn.style.background = '#ef4444';
          daemonBtn.disabled = false;
          setTimeout(refreshDaemonStatus, 1500);
        } catch (e) {
          alert(`Failed to start daemon: ${e}`);
          daemonBtn.innerText = 'Start Daemon';
          daemonBtn.disabled = false;
        }
      } else {
        daemonBtn.innerText = 'Stopping...';
        daemonBtn.disabled = true;
        try {
          await invoke('stop_daemon');
          daemonRunning = false;
          daemonBtn.innerText = 'Start Daemon';
          daemonBtn.style.removeProperty('background');
          daemonBtn.disabled = false;
          setTimeout(refreshDaemonStatus, 1500);
        } catch (e) {
          alert(`Failed to stop daemon: ${e}`);
          daemonBtn.innerText = 'Stop Daemon';
          daemonBtn.disabled = false;
        }
      }
    });
  }

  // ── Management Tab ───────────────────────────────────────────────────────────
  const genBtn     = document.getElementById('btn-generate-bulk');
  const genResults = document.getElementById('gen-results');

  genBtn.addEventListener('click', async () => {
    const count = parseInt(document.getElementById('gen-count').value) || 15;
    genResults.innerHTML = '<div class="placeholder">Generating slots...</div>';
    try {
      const identities = await invoke('generate_identities', { count });
      genResults.innerHTML = '';
      identities.forEach(([nodeId, status]) => {
        const row = document.createElement('div');
        row.className = 'glass';
        row.style.cssText = 'padding:0.5rem;margin-bottom:0.25rem;display:flex;justify-content:space-between;';
        const statusColor = status === 'jwt ready' ? '#10b981' : '#f59e0b';
        row.innerHTML = `<span><strong>${nodeId}</strong></span><span style="color:${statusColor};font-family:monospace;font-size:0.8rem;">${status}</span>`;
        genResults.appendChild(row);
      });
    } catch (e) {
      genResults.innerHTML = `<div class="placeholder" style="color:#ef4444">Failed: ${e}</div>`;
    }
  });

  const provisionBtn   = document.getElementById('btn-provision');
  const networkName    = document.getElementById('network-name');
  const controllerUrl  = document.getElementById('controller-url');

  if (provisionBtn) {
    provisionBtn.addEventListener('click', async () => {
      const name = networkName.value;
      const url  = controllerUrl.value;
      if (!name || !url) { alert('Please provide both Network Name and Controller URL'); return; }
      provisionBtn.innerText = 'Provisioning...';
      provisionBtn.disabled = true;
      try {
        const result = await invoke('provision_network', { name, controllerUrl: url });
        provisionBtn.innerText = 'Provisioned!';
        provisionBtn.style.background = '#10b981';
        console.log('Provisioned:', result);
      } catch (e) {
        alert(`Provisioning failed: ${e}`);
        provisionBtn.innerText = 'Provision Network';
        provisionBtn.disabled = false;
      }
    });
  }
});
