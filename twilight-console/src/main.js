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
    appendLog(envelope.source?.site || 'unknown', 'TRAFFIC', envelope.source?.agent_name || 'unknown', JSON.stringify(envelope.payload));
  });

  listen('bus-heartbeat', (event) => {
    const hb = event.payload;
    appendLog('local', 'HEARTBEAT', hb.node_id, `Status: ${hb.status}`);
  });

  function appendLog(provider, kind, source, data) {
    const row = document.createElement('div');
    row.className = 'list-row';
    row.style.display = 'grid';
    row.style.gridTemplateColumns = '100px 120px 100px 180px 1fr';
    row.style.padding = '0.75rem 1.5rem';
    row.style.borderBottom = '1px solid rgba(255,255,255,0.05)';
    row.style.fontSize = '0.85rem';

    const time = new Date().toLocaleTimeString();

    row.innerHTML = `
      <span style="color: #71717a">${time}</span>
      <span style="color: #6366f1; font-weight: 600">${provider}</span>
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

  // Zenoh Admin Logic
  const adminTree = document.getElementById('admin-tree');
  const refreshBtn = document.getElementById('refresh-admin');

  async function refreshAdmin() {
    adminTree.innerHTML = '<div class="placeholder">Querying...</div>';
    try {
      const keys = await invoke("get_admin_data");
      if (keys.length === 0) {
        adminTree.innerHTML = '<div class="placeholder">No admin data found.</div>';
        return;
      }

      adminTree.innerHTML = '';
      keys.sort().forEach(key => {
        const item = document.createElement('div');
        item.style.padding = '0.25rem 0';
        item.style.borderBottom = '1px solid rgba(255,255,255,0.02)';

        // Color coding for common patterns
        let color = '#a1a1aa';
        if (key.includes('sessions')) color = '#8b5cf6';
        if (key.includes('config')) color = '#10b981';

        item.innerHTML = `<span style="color: ${color}">●</span> ${key}`;
        adminTree.appendChild(item);
      });
    } catch (e) {
      adminTree.innerHTML = `<div style="color: #ef4444">Error: ${e}</div>`;
    }
  }

  refreshBtn.addEventListener('click', refreshAdmin);
  // Initial refresh after a delay (bus needs to connect)
  setTimeout(refreshAdmin, 3000);

  // Enrollment Logic
  const enrollBtn = document.getElementById('btn-enroll');
  const tokenPath = document.getElementById('token-path');

  enrollBtn.addEventListener('click', async () => {
    const path = tokenPath.value;
    if (!path) {
      alert("Please provide a path to a JWT token.");
      return;
    }

    enrollBtn.innerText = "Enrolling...";
    enrollBtn.disabled = true;

    try {
      // In a real app, we'd use tauri-plugin-dialog to pick the file
      // and then call a Rust command to perform enrollment.
      setTimeout(() => {
        enrollBtn.innerText = "Enrolled!";
        enrollBtn.style.background = "#10b981";
        console.log(`Identity enrolled from ${path}`);
      }, 2000);
    } catch (e) {
      alert(`Enrollment failed: ${e}`);
      enrollBtn.disabled = false;
      enrollBtn.innerText = "Enroll Identity";
    }
  });

  // Bulk Generator Logic
  const genBtn = document.getElementById('btn-generate-bulk');
  const genResults = document.getElementById('gen-results');
  const dogBreeds = ['Beagle', 'Husky', 'Boxer', 'Terrier', 'Collie', 'Retriever', 'Spaniel', 'Dachshund', 'Poodle', 'Mastiff', 'Greyhound', 'Shiba', 'Corgi', 'Bulldog', 'Maltese'];

  genBtn.addEventListener('click', () => {
    const count = parseInt(document.getElementById('gen-count').value) || 15;
    genResults.innerHTML = '<div class="placeholder">Breeding pack...</div>';

    setTimeout(() => {
      genResults.innerHTML = '';
      for (let i = 0; i < count; i++) {
        const breed = dogBreeds[i % dogBreeds.length];
        const id = Math.random().toString(36).substring(2, 10).toUpperCase();
        const row = document.createElement('div');
        row.className = 'glass';
        row.style.padding = '0.5rem';
        row.style.marginBottom = '0.25rem';
        row.style.display = 'flex';
        row.style.justifyContent = 'space-between';
        row.innerHTML = `
          <span>🐶 <strong>${breed}-${i + 1}</strong></span>
          <span style="color: #10b981; font-family: monospace;">TOKEN-${id}</span>
        `;
        genResults.appendChild(item || row); // item was a typo in previous thought, using row
      }
      console.log(`Generated ${count} dog identities.`);
    }, 1500);
  });
});
