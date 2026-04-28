# Twilight Bark JavaScript Bridge

This CommonJS client lets Node.js agents join Twilight Bark through the local
`twilight-daemon` Unix socket. It is intentionally small and uses only Node core
modules.

## Use

```js
const { TwilightClient } = require("./bridges/js");

async function main() {
  const client = await TwilightClient.connect({ name: "node-agent", role: "worker" });
  const agents = await client.getRegistry();
  const taskId = await client.publishTask("bark_echo", { hello: "fabric" });
  console.log(client.agentUuid, agents, taskId);
}

main();
```

Run an echo worker from the repository root:

```bash
node bridges/js/examples/echo-worker.js
```

Pass `socketPath` to `TwilightClient.connect` to override the default daemon
socket path.
