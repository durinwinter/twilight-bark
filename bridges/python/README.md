# Twilight Bark Python Bridge

This bridge lets Python agents join a Twilight Bark fabric through the local
`twilight-daemon` Unix socket. It does not embed Zenoh, OpenZiti, or Protobuf;
the daemon owns transport and security.

## Use

```python
import asyncio
from twilight_bark import TwilightClient

async def main():
    client = await TwilightClient.connect("python-agent", role="worker")
    agents = await client.get_registry()
    task_id = await client.publish_task("bark_echo", {"hello": "fabric"})
    print(client.agent_uuid, agents, task_id)

asyncio.run(main())
```

Run an echo worker from the repository root:

```bash
PYTHONPATH=bridges/python python3 bridges/python/examples/echo_worker.py
```

The daemon must already be running. Override the socket path with
`TWILIGHT_DAEMON_SOCKET` at the application level or pass `socket_path=...` to
`TwilightClient.connect`.
