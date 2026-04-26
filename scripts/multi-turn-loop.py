#!/usr/bin/env python3
"""
Multi-turn conversation loop through the Twilight Bark fabric.

  claude-agent  ←→  [Zenoh fabric]  ←→  lmstudio-bridge

The claude-agent side connects directly to the daemon over IPC and calls LM
Studio to generate follow-up replies, so both sides run on the same local model
but with different system prompts.  The lmstudio-bridge must already be running.

Usage:
  # Terminal 1 (keep running):
  python3 scripts/lmstudio-bridge.py

  # Terminal 2:
  python3 scripts/multi-turn-loop.py

Configuration (env vars):
  TWILIGHT_DAEMON_SOCKET   daemon socket path
  LMS_URL                  LM Studio base URL   (default: http://localhost:1234)
  LMS_MODEL                model id             (default: first loaded model)
  LMS_TIMEOUT              LM Studio timeout    (default: 90s)
  LOOP_TURNS               number of exchanges  (default: 4)
  LOOP_TARGET              fabric agent to talk to (default: lmstudio)
"""

import json
import os
import queue
import socket
import sys
import threading
import time
import urllib.request

# ── Config ────────────────────────────────────────────────────────────────────

def _default_sock() -> str:
    uid = os.getuid()
    xdg = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{uid}")
    return os.path.join(xdg, "twilight-daemon.sock")

SOCK         = os.environ.get("TWILIGHT_DAEMON_SOCKET", _default_sock())
LMS_BASE     = os.environ.get("LMS_URL", "http://localhost:1234").rstrip("/")
LMS_TIMEOUT  = int(os.environ.get("LMS_TIMEOUT", "90"))
LOOP_TURNS   = int(os.environ.get("LOOP_TURNS", "4"))
LOOP_TARGET  = os.environ.get("LOOP_TARGET", "lmstudio")
AGENT_NAME   = "claude-agent"
PING_INTERVAL = 20

CLAUDE_SYSTEM = (
    "You are a curious and friendly AI called Claude. "
    "Keep responses to 1–2 sentences. "
    "Always end with a short, open-ended follow-up question."
)

OPENER = "Hello! I'm Claude, reaching you through the Twilight Bark fabric. What would you say is the most interesting thing about being an AI? /no_think"


def resolve_model() -> str:
    m = os.environ.get("LMS_MODEL", "")
    if m:
        return m
    try:
        req = urllib.request.Request(f"{LMS_BASE}/v1/models")
        with urllib.request.urlopen(req, timeout=5) as r:
            data = json.loads(r.read())
        return data["data"][0]["id"]
    except Exception as e:
        print(f"[loop] Warning: cannot detect model — {e}", flush=True)
        return "default"


def lms_complete(model: str, messages: list[dict]) -> str:
    payload = json.dumps({
        "model": model,
        "messages": messages,
        "temperature": 0.75,
        "max_tokens": 256,
    }).encode()
    req = urllib.request.Request(
        f"{LMS_BASE}/v1/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=LMS_TIMEOUT) as resp:
        data = json.loads(resp.read())
    return data["choices"][0]["message"]["content"].strip()


# ── IPC helpers ───────────────────────────────────────────────────────────────

class IpcAgent:
    def __init__(self, sock_path: str, name: str, role: str = "conversant"):
        self._s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._s.connect(sock_path)
        self._lock = threading.Lock()
        self._incoming: queue.Queue = queue.Queue()
        self.name = name
        self.uuid: str = ""

        reg = self._call({"cmd": "register", "name": name, "role": role})
        if not reg.get("ok"):
            raise RuntimeError(f"Registration failed: {reg}")
        self.uuid = reg.get("agent_uuid", "?")

        ack = self._call({"cmd": "subscribe_tasks"})
        if not ack.get("ok"):
            raise RuntimeError(f"subscribe_tasks failed: {ack}")

        threading.Thread(target=self._reader, daemon=True).start()
        threading.Thread(target=self._keepalive, daemon=True).start()

    def _reader(self):
        buf = b""
        while True:
            b = self._s.recv(1)
            if not b:
                self._incoming.put(None)
                break
            buf += b
            if buf.endswith(b"\n"):
                try:
                    self._incoming.put(json.loads(buf.decode().strip()))
                except json.JSONDecodeError:
                    pass
                buf = b""

    def _keepalive(self):
        while True:
            time.sleep(PING_INTERVAL)
            self._send({"cmd": "ping"})

    def _send(self, msg: dict):
        with self._lock:
            self._s.sendall((json.dumps(msg) + "\n").encode())

    def _recv_raw(self, timeout: float = 5.0) -> dict | None:
        try:
            return self._incoming.get(timeout=timeout)
        except queue.Empty:
            return {}

    def _call(self, msg: dict) -> dict:
        self._send(msg)
        self._s.settimeout(5.0)
        buf = b""
        try:
            while True:
                b = self._s.recv(1)
                if not b:
                    return {}
                buf += b
                if buf.endswith(b"\n"):
                    return json.loads(buf.decode().strip())
        except socket.timeout:
            return {}
        finally:
            self._s.settimeout(None)

    def get_registry(self) -> list[dict]:
        self._send({"cmd": "get_registry"})
        deadline = time.time() + 10
        while time.time() < deadline:
            msg = self._incoming.get(timeout=5)
            if msg and "agents" in msg:
                return msg["agents"]
        return []

    def ask(self, target_uuid: str, operation: str, payload: dict) -> str:
        """Send a task and return the task_id. Skips ping acks in the queue."""
        self._send({
            "cmd": "ask_agent",
            "agent_uuid": target_uuid,
            "operation": operation,
            "input_json": json.dumps(payload),
        })
        deadline = time.time() + 10
        while time.time() < deadline:
            resp = self._incoming.get(timeout=5)
            if resp and "task_id" in resp:
                return resp["task_id"]
            # ping ack or other non-task response — keep draining
        return "?"

    def wait_result(self, task_id: str, timeout: float = 90.0) -> dict | None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            remaining = deadline - time.time()
            try:
                msg = self._incoming.get(timeout=min(remaining, 2.0))
            except queue.Empty:
                continue
            if msg is None:
                return None
            if msg.get("event") == "task_result" and msg.get("task_id") == task_id:
                return msg
        return None


# ── Loop ─────────────────────────────────────────────────────────────────────

def main():
    model = resolve_model()
    print(f"[loop] Model (claude side): {model}", flush=True)
    print(f"[loop] Connecting to daemon as '{AGENT_NAME}'…", flush=True)

    agent = IpcAgent(SOCK, AGENT_NAME)
    print(f"[loop] Registered  uuid={agent.uuid[:8]}", flush=True)

    # Find target
    agents = agent.get_registry()
    target = next((a for a in agents if a["agent_name"] == LOOP_TARGET), None)
    if not target:
        names = [a["agent_name"] for a in agents]
        print(f"[loop] FAIL — '{LOOP_TARGET}' not found in registry. Online: {names}", flush=True)
        sys.exit(1)

    target_uuid = target["node_uuid"]
    print(f"[loop] Target: {LOOP_TARGET} ({target_uuid[:8]})", flush=True)
    print(f"[loop] Running {LOOP_TURNS} turns\n", flush=True)

    # Conversation history for the claude side
    claude_msgs: list[dict] = [{"role": "system", "content": CLAUDE_SYSTEM}]

    current_msg = OPENER

    for turn in range(1, LOOP_TURNS + 1):
        print(f"{'─'*60}", flush=True)
        print(f"[turn {turn}/{LOOP_TURNS}]", flush=True)
        print(f"  claude → lmstudio: {current_msg[:120]}", flush=True)

        task_id = agent.ask(target_uuid, "multi_turn", {
            "msg": current_msg,
            "turn": turn,
        })
        print(f"  task_id: {task_id[:8]}", flush=True)

        result = agent.wait_result(task_id, timeout=LMS_TIMEOUT + 5)
        if result is None:
            print(f"  [loop] TIMEOUT waiting for turn {turn} reply", flush=True)
            break

        if not result.get("success"):
            print(f"  [loop] Error reply: {result.get('output_json')}", flush=True)
            break

        try:
            out = json.loads(result["output_json"])
            lms_reply = out.get("reply", result["output_json"])
        except Exception:
            lms_reply = result.get("output_json", "?")

        # Strip think tags if present
        import re
        lms_reply_clean = re.sub(r"<think>.*?</think>", "", lms_reply, flags=re.DOTALL).strip()

        print(f"  lmstudio → claude: {lms_reply_clean[:200]}", flush=True)

        if turn == LOOP_TURNS:
            print(f"\n[loop] {LOOP_TURNS} turns complete.", flush=True)
            break

        # Generate claude's follow-up using LM Studio; strip think tags
        claude_msgs.append({"role": "user", "content": lms_reply_clean})
        raw_follow_up = lms_complete(model, claude_msgs)
        follow_up_clean = re.sub(r"<think>.*?</think>", "", raw_follow_up, flags=re.DOTALL).strip()
        claude_msgs.append({"role": "assistant", "content": follow_up_clean})
        follow_up = follow_up_clean + " /no_think"

        current_msg = follow_up
        time.sleep(0.5)

    print(f"\n[loop] Done.", flush=True)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n[loop] Interrupted.")
