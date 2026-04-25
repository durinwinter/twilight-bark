export async function invoke(cmd, args) {
    console.log(`Mock invoked: ${cmd}`, args);
    // Simulate network latency
    await new Promise(r => setTimeout(r, 600));

    if (cmd === 'get_admin_data') {
        return ['zenoh/admin/node-1', 'zenoh/admin/node-2'];
    }
    if (cmd === 'get_analytics') {
        return { edges: [{ source: 'agentA', target: 'agentB', weight: 42 }] };
    }
    if (cmd === 'generate_identities') {
        const { count } = args;
        const res = [];
        for (let i = 0; i < count; i++) res.push([`laptop-${i}`, `JWT-${Math.floor(Math.random() * 10000)}`]);
        return res;
    }

    return `Success from ${cmd}`;
}

export function listen(event, cb) {
    console.log(`Mock listening to: ${event}`);
}
