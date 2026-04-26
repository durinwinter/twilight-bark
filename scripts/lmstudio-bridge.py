#!/usr/bin/env python3
"""
Twilight Bark ↔ LM Studio bridge.

Registers as an IPC agent named 'lmstudio', listens for incoming task_request
events pushed by the daemon, and handles each by calling LM Studio's REST API.

Configuration (env vars with defaults):
  TWILIGHT_DAEMON_SOCKET   Unix socket path  (default: /run/user/$UID/twilight-daemon.sock)
  TWILIGHT_AGENT_NAME      Agent name        (default: lmstudio)
  LMS_URL                  LM Studio base    (default: http://localhost:1234)
  LMS_MODEL                Model identifier  (default: auto-detect first loaded model)
  LMS_TIMEOUT              API timeout secs  (default: 90)
"""

import json
import os
import queue
import socket
import sys
import threading
import time
import urllib.request
import urllib.error

# ── Config ────────────────────────────────────────────────────────────────────

def _default_sock() -> str:
    uid = os.getuid()
    xdg = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{uid}")
    return os.path.join(xdg, "twilight-daemon.sock")

SOCK        = os.environ.get("TWILIGHT_DAEMON_SOCKET", _default_sock())
AGENT_NAME  = os.environ.get("TWILIGHT_AGENT_NAME", "lmstudio")
LMS_BASE    = os.environ.get("LMS_URL", "http://localhost:1234").rstrip("/")
LMS_TIMEOUT = int(os.environ.get("LMS_TIMEOUT", "90"))
_MODEL_ENV  = os.environ.get("LMS_MODEL", "")

PING_INTERVAL = 20  # seconds between IPC keepalive pings


def resolve_model() -> str:
    if _MODEL_ENV:
        return _MODEL_ENV
    try:
        req = urllib.request.Request(f"{LMS_BASE}/v1/models")
        with urllib.request.urlopen(req, timeout=5) as r:
            data = json.loads(r.read())
        return data["data"][0]["id"]
    except Exception as e:
        print(f"[bridge] Warning: cannot auto-detect model: {e}", flush=True)
        return "default"


# ── LM Studio API ─────────────────────────────────────────────────────────────

def lms_complete(model: str, prompt: str, system: str = "") -> str:
    messages = []
    if system:
        messages.append({"role": "system", "content": system})
    messages.append({"role": "user", "content": prompt})

    payload = json.dumps({
        "model": model,
        "messages": messages,
        "temperature": 0.7,
        "max_tokens": 512,
    }).encode()

    req = urllib.request.Request(
        f"{LMS_BASE}/v1/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=LMS_TIMEOUT) as resp:
        data = json.loads(resp.read())
    return data["choices"][0]["message"]["content"].strip()


# ── Daemon IPC ────────────────────────────────────────────────────────────────

class DaemonConn:
    def __init__(self, sock_path: str):
        self._s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._s.connect(sock_path)
        self._lock = threading.Lock()

    def send(self, msg: dict):
        with self._lock:
            self._s.sendall((json.dumps(msg) + "\n").encode())

    def recv_line(self) -> str | None:
        buf = b""
        while True:
            b = self._s.recv(1)
            if not b:
                return None
            buf += b
            if buf.endswith(b"\n"):
                return buf.decode().strip()

    def call(self, msg: dict, timeout: float = 5.0) -> dict:
        self.send(msg)
        self._s.settimeout(timeout)
        try:
            line = self.recv_line()
        except socket.timeout:
            return {}
        finally:
            self._s.settimeout(None)
        return json.loads(line) if line else {}


# ── Main ──────────────────────────────────────────────────────────────────────

RECONNECT_DELAY = 5   # seconds between reconnect attempts
RECONNECT_MAX   = 60  # cap backoff at this many seconds


def _run_once(model: str) -> bool:
    """Connect, register, and serve until the daemon disconnects.
    Returns True if we should reconnect, False to exit."""
    print(f"[bridge] Connecting to {SOCK}…", flush=True)
    try:
        conn = DaemonConn(SOCK)
    except Exception as e:
        print(f"[bridge] Cannot connect to daemon: {e}", flush=True)
        return True  # retry

    reg = conn.call({"cmd": "register", "name": AGENT_NAME, "role": "mcp-agent"})
    if not reg.get("ok"):
        print(f"[bridge] Registration failed: {reg}", flush=True)
        return True

    uuid = reg.get("agent_uuid", "?")
    print(f"[bridge] Registered as '{AGENT_NAME}'  uuid={uuid[:8]}", flush=True)

    ack = conn.call({"cmd": "subscribe_tasks"})
    if not ack.get("ok"):
        print(f"[bridge] subscribe_tasks failed: {ack}", flush=True)
        return True

    print("[bridge] Subscribed. Waiting for tasks…\n", flush=True)

    incoming: queue.Queue = queue.Queue()

    def reader():
        while True:
            line = conn.recv_line()
            if line is None:
                incoming.put(None)
                break
            try:
                incoming.put(json.loads(line))
            except json.JSONDecodeError:
                pass

    threading.Thread(target=reader, daemon=True).start()

    def keepalive():
        while True:
            time.sleep(PING_INTERVAL)
            try:
                conn.send({"cmd": "ping"})
            except Exception:
                break

    threading.Thread(target=keepalive, daemon=True).start()

    while True:
        try:
            msg = incoming.get(timeout=60)
        except queue.Empty:
            continue

        if msg is None:
            print("[bridge] Daemon closed connection — will reconnect.", flush=True)
            return True  # reconnect

        if msg.get("event") != "task_request":
            continue  # ping acks or other responses

        task_id   = msg.get("task_id", "?")
        operation = msg.get("operation", "?")
        input_json = msg.get("input_json", "{}")
        src       = msg.get("source_uuid", "?")

        print(f"[bridge] ← TASK  id={task_id[:8]}  op={operation}  from={src[:8]}", flush=True)

        try:
            data   = json.loads(input_json)
            prompt = data.get("msg") or data.get("prompt") or json.dumps(data)
            system = data.get("system", "")
        except Exception:
            prompt = input_json
            system = ""

        # /no_think disables chain-of-thought on Qwen3-family models
        prompt_to_send = prompt.rstrip() + " /no_think"

        try:
            reply_text = lms_complete(model, prompt_to_send, system)
            print(f"[bridge] → REPLY {reply_text[:100]!r}", flush=True)
            output  = json.dumps({"status": "ok", "agent": AGENT_NAME, "reply": reply_text})
            success = True
        except Exception as e:
            print(f"[bridge] LMS error: {e}", flush=True)
            output  = json.dumps({"status": "error", "error": str(e)})
            success = False

        conn.send({
            "cmd": "reply_task",
            "task_id": task_id,
            "output_json": output,
            "success": success,
        })
        print(f"[bridge] ✓ replied to {task_id[:8]}\n", flush=True)

    return False  # unreachable but satisfies type checker


def main():
    model = resolve_model()
    print(f"[bridge] Model: {model}", flush=True)

    delay = RECONNECT_DELAY
    while True:
        should_reconnect = _run_once(model)
        if not should_reconnect:
            break
        print(f"[bridge] Reconnecting in {delay}s…", flush=True)
        time.sleep(delay)
        delay = min(delay * 2, RECONNECT_MAX)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n[bridge] Stopped.")
