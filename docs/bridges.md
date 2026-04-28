# Twilight Bark Python and JavaScript Bridges

Phase 5 starts with non-Rust bridge clients. These bridges connect to the
persistent `twilight-daemon` over its JSON-lines Unix socket:

```text
Python or Node agent
  -> Twilight bridge client
  -> twilight-daemon Unix socket
  -> Zenoh and OpenZiti fabric
```

The bridges expose the same basic commands as the Rust MCP shim:

- `get_registry`
- `publish_task`
- `ask_agent`
- `reply_task`
- `list_tasks`
- `ping`
- pushed `task_request` and `task_result` events

## Python

```bash
PYTHONPATH=bridges/python python3 bridges/python/examples/echo_worker.py
```

## JavaScript

```bash
node bridges/js/examples/echo-worker.js
```

The daemon must be running before either bridge connects. The default socket
path is `$XDG_RUNTIME_DIR/twilight-daemon.sock` or
`/tmp/twilight-$USER-daemon.sock`.
