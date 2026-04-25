export async function invoke(cmd, args) {
    console.log(`Mock invoked: ${cmd}`, args);
    await new Promise(r => setTimeout(r, 300));

    if (cmd === 'get_node_id') return 'pop-os-earthling';

    if (cmd === 'get_daemon_status') {
        return { running: false, pid: null, socket: '/tmp/twilight-mock-daemon.sock' };
    }

    if (cmd === 'get_fabric_agents') {
        const now = Date.now();
        return [
            { identity: { agent_name: 'claude', node_id: 'pop-os-earthling', node_uuid: 'aaaa-1111-bbbb-2222', role: 'mcp-agent', tenant: 'twilight-bark' }, last_seen_ms: now - 4000, status: 1 },
            { identity: { agent_name: 'lmstudio', node_id: 'pop-os-earthling', node_uuid: 'cccc-3333-dddd-4444', role: 'mcp-agent', tenant: 'twilight-bark' }, last_seen_ms: now - 25000, status: 1 },
            { identity: { agent_name: 'twilight-daemon', node_id: 'pop-os-earthling', node_uuid: 'eeee-5555-ffff-6666', role: 'daemon', tenant: 'twilight-bark' }, last_seen_ms: now - 8000, status: 1 },
        ];
    }

    if (cmd === 'get_admin_data') {
        return [
            'zenoh/admin/routers/local',
            'zenoh/admin/sessions/client-1',
            'zenoh/admin/config/mode',
            'zenoh/admin/config/listen',
        ];
    }

    if (cmd === 'get_analytics') {
        return {
            nodes: ['claude', 'lmstudio', 'twilight-daemon'],
            edges: [
                { source: 'claude', target: 'lmstudio', weight: 14 },
                { source: 'lmstudio', target: 'claude', weight: 7 },
                { source: 'claude', target: 'twilight-daemon', weight: 42 },
            ],
        };
    }

    if (cmd === 'generate_identities') {
        const { count } = args;
        return Array.from({ length: count }, (_, i) => [`node-${String(i + 1).padStart(3, '0')}`, 'awaiting jwt']);
    }

    return `Success from ${cmd}`;
}

export function listen(event, cb) {
    console.log(`Mock listening to: ${event}`);
    // Simulate a heartbeat after 3s for the Live Bus tab demo
    if (event === 'bus-heartbeat') {
        setTimeout(() => cb({ payload: { node_id: 'pop-os-earthling', status: 1 } }), 3000);
    }
}
