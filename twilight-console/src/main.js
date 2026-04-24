import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Tab Switching Logic
document.addEventListener('DOMContentLoaded', async () => {
  const navItems = document.querySelectorAll('.nav-item');
  const tabContents = document.querySelectorAll('.tab-content');

  navItems.forEach(item => {
    item.addEventListener('click', () => {
      // Update sidebar
      navItems.forEach(i => i.classList.remove('active'));
      item.classList.add('active');

      // Update content
      const tabId = item.getAttribute('data-tab');
      tabContents.forEach(content => {
        content.classList.remove('active');
        if (content.id === `tab-${tabId}`) {
          content.classList.add('active');
        }
      });
    });
  });

  // Connect to the Bus
  try {
    console.log("Connecting to Twilight Bus...");
    await invoke("connect_bus", { tenant: "default", site: "local" });
    console.log("Connected.");
  } catch (e) {
    console.error("Failed to connect to bus:", e);
  }

  // Listen for Bus Events
  const monitor = document.getElementById('bus-monitor');

  listen('bus-traffic', (event) => {
    const envelope = event.payload;
    appendLog('TRAFFIC', envelope.source?.agent_name || 'unknown', JSON.stringify(envelope.payload));
  });

  listen('bus-heartbeat', (event) => {
    const hb = event.payload;
    appendLog('HEARTBEAT', hb.node_id, `Status: ${hb.status}`);
  });

  function appendLog(kind, source, data) {
    const row = document.createElement('div');
    row.className = 'list-row';
    row.style.display = 'grid';
    row.style.gridTemplateColumns = '120px 100px 180px 1fr';
    row.style.padding = '0.75rem 1.5rem';
    row.style.borderBottom = '1px solid rgba(255,255,255,0.05)';
    row.style.fontSize = '0.85rem';

    const time = new Date().toLocaleTimeString();

    row.innerHTML = `
      <span style="color: #71717a">${time}</span>
      <span style="color: #8b5cf6; font-weight: 700">${kind}</span>
      <span style="color: #a1a1aa">${source}</span>
      <span style="color: #d4d4d8; font-family: monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${data}</span>
    `;

    monitor.appendChild(row);
    if (monitor.children.length > 50) {
      monitor.removeChild(monitor.children[1]);
    }
    monitor.scrollTop = monitor.scrollHeight;
  }

  // Analytics Polling
  const sankeyDiv = document.getElementById('sankey-chart');
  setInterval(async () => {
    try {
      const stats = await invoke("get_analytics");
      if (stats.edges.length > 0) {
        sankeyDiv.innerHTML = `
          <div style="width: 100%; padding: 2rem;">
            <h4 style="margin-bottom: 1rem; color: #8b5cf6;">Active Traffic Edges: ${stats.edges.length}</h4>
            <div style="display: flex; flex-direction: column; gap: 0.5rem;">
              ${stats.edges.map(edge => `
                <div class="glass" style="padding: 1rem; display: flex; justify-content: space-between; align-items: center;">
                  <span>${edge.source.substring(0, 8)}... → ${edge.target.substring(0, 8)}...</span>
                  <span class="tag" style="background: #8b5cf6">${edge.weight} PKTS</span>
                </div>
              `).join('')}
            </div>
          </div>
        `;
      } else {
        sankeyDiv.innerHTML = '<div class="chart-placeholder">Waiting for traffic data...</div>';
      }
    } catch (e) {
      console.error("Failed to fetch analytics:", e);
    }
  }, 3000);
});
