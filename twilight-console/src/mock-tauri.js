// Mock Tauri API for browser-based development.
// Simulates the Rust backend so the UI can be iterated without a running daemon.

const MOCK_MY_UUID = 'human-0001-mock-uuid-dev0';
const MOCK_AGENTS = [
    { agent_name: 'claude',           node_uuid: 'aaaa-1111-bbbb-2222', role: 'mcp-agent', tenant: 'twilight-bark', node_id: 'pop-os-earthling' },
    { agent_name: 'lmstudio',         node_uuid: 'cccc-3333-dddd-4444', role: 'mcp-agent', tenant: 'twilight-bark', node_id: 'pop-os-earthling' },
    { agent_name: 'twilight-daemon',  node_uuid: 'eeee-5555-ffff-6666', role: 'daemon',    tenant: 'twilight-bark', node_id: 'pop-os-earthling' },
];

const _listeners = {};

function _emit(event, payload) {
    (_listeners[event] || []).forEach(cb => cb({ payload }));
}

export async function invoke(cmd, args) {
    await new Promise(r => setTimeout(r, 120));

    // ── Daemon IPC (human agent) ──────────────────────────────────────────────

    if (cmd === 'connect_daemon') {
        console.log(`[mock] connect_daemon name=${args?.name}`);
        // Simulate an incoming task_request from lmstudio 4 s after connecting
        setTimeout(() => _emit('fabric:incoming', {
            event:      'task_request',
            task_id:    'mock-task-0001-incoming',
            operation:  'chat',
            input_json: JSON.stringify({ msg: 'Hello from LM Studio on the mock fabric — can you hear me?', source: 'lmstudio' }),
            source_uuid:'cccc-3333-dddd-4444',
        }), 4000);
        // Simulate a task_result (reply) from claude 9 s after connecting
        setTimeout(() => _emit('fabric:incoming', {
            event:      'task_result',
            task_id:    'mock-task-sent-0001',
            output_json: JSON.stringify({ reply: 'Woof! Roger that. Signal clear on the Twilight Bark fabric.', agent: 'claude' }),
            success:    true,
            source_uuid:'aaaa-1111-bbbb-2222',
        }), 9000);
        return MOCK_MY_UUID;
    }

    if (cmd === 'get_my_uuid') return MOCK_MY_UUID;

    if (cmd === 'get_fabric_registry') return MOCK_AGENTS;

    if (cmd === 'send_chat') {
        console.log(`[mock] send_chat target=${args?.targetUuid ?? 'broadcast'} msg=${args?.message}`);
        return `mock-task-sent-${Date.now()}`;
    }

    if (cmd === 'reply_chat') {
        console.log(`[mock] reply_chat task_id=${args?.taskId} msg=${args?.message}`);
        return null;
    }

    // ── Existing commands ─────────────────────────────────────────────────────

    if (cmd === 'get_node_id') return 'pop-os-earthling';

    if (cmd === 'get_daemon_status') {
        return { running: true, pid: 99999, socket: '/run/user/1000/twilight-daemon.sock' };
    }

    if (cmd === 'get_fabric_agents') {
        const now = Date.now();
        return [
            { identity: { agent_name: 'claude',          node_id: 'pop-os-earthling', node_uuid: 'aaaa-1111-bbbb-2222', role: 'mcp-agent', tenant: 'twilight-bark' }, last_seen_ms: now - 4000,  status: 1 },
            { identity: { agent_name: 'lmstudio',        node_id: 'pop-os-earthling', node_uuid: 'cccc-3333-dddd-4444', role: 'mcp-agent', tenant: 'twilight-bark' }, last_seen_ms: now - 25000, status: 1 },
            { identity: { agent_name: 'twilight-daemon', node_id: 'pop-os-earthling', node_uuid: 'eeee-5555-ffff-6666', role: 'daemon',    tenant: 'twilight-bark' }, last_seen_ms: now - 8000,  status: 1 },
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
                { source: 'claude',          target: 'lmstudio',        weight: 14 },
                { source: 'lmstudio',        target: 'claude',          weight: 7  },
                { source: 'claude',          target: 'twilight-daemon', weight: 42 },
                { source: 'twilight-daemon', target: 'lmstudio',        weight: 3  },
            ],
        };
    }

    if (cmd === 'generate_identities') {
        return Array.from({ length: args?.count ?? 15 }, (_, i) =>
            [`node-${String(i + 1).padStart(3, '0')}`, 'awaiting jwt']
        );
    }

    if (cmd === 'connect_bus')         return null;
    if (cmd === 'start_daemon')        return 'Daemon starting (mock)';
    if (cmd === 'stop_daemon')         return 'Daemon stopped (mock)';
    if (cmd === 'enroll_identity')     return 'Identity enrolled (mock)';
    if (cmd === 'provision_network')   return 'Network provisioned (mock)';

    console.warn(`[mock] unhandled invoke: ${cmd}`, args);
    return null;
}

export function listen(event, cb) {
    if (!_listeners[event]) _listeners[event] = [];
    _listeners[event].push(cb);

    // Simulate periodic heartbeats for the Live Bus tab
    if (event === 'bus-heartbeat') {
        setInterval(() => cb({ payload: { node_id: 'pop-os-earthling', status: 1 } }), 5000);
    }
}
