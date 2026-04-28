"""Async Python client for Twilight Bark daemon IPC.

The daemon owns Zenoh, OpenZiti, registry state, and task routing. This bridge
only speaks the daemon's JSON-lines Unix socket protocol, matching the thin Rust
MCP shim.
"""

from __future__ import annotations

import asyncio
import getpass
import json
import os
from pathlib import Path
from typing import Any


class TwilightError(RuntimeError):
    """Raised when the daemon rejects a command or closes the socket."""


def default_socket_path() -> Path:
    """Return the same default socket path used by the Rust daemon client."""

    explicit_socket = os.environ.get("TWILIGHT_DAEMON_SOCKET")
    if explicit_socket:
        return Path(explicit_socket)

    runtime_dir = os.environ.get("XDG_RUNTIME_DIR")
    if runtime_dir:
        return Path(runtime_dir) / "twilight-daemon.sock"
    return Path("/tmp") / f"twilight-{getpass.getuser()}-daemon.sock"


class TwilightClient:
    """Small async client for non-Rust agents.

    Use :meth:`connect` to register an agent and subscribe to task events. Calls
    are serialized because daemon responses and push events share one socket.
    """

    def __init__(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        agent_uuid: str,
    ) -> None:
        self._reader = reader
        self._writer = writer
        self.agent_uuid = agent_uuid
        self._responses: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        self._events: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        self._call_lock = asyncio.Lock()
        self._reader_task = asyncio.create_task(self._read_loop())

    @classmethod
    async def connect(
        cls,
        name: str,
        role: str = "python-agent",
        socket_path: str | os.PathLike[str] | None = None,
    ) -> "TwilightClient":
        """Connect to a running daemon, register, and subscribe to task events."""

        path = Path(socket_path) if socket_path is not None else default_socket_path()
        reader, writer = await asyncio.open_unix_connection(str(path))

        await _write_json(writer, {"cmd": "register", "name": name, "role": role})
        registration = await _read_json(reader)
        agent_uuid = registration.get("agent_uuid")
        if not registration.get("ok") or not isinstance(agent_uuid, str):
            writer.close()
            await writer.wait_closed()
            raise TwilightError(f"daemon rejected registration: {registration!r}")

        await _write_json(writer, {"cmd": "subscribe_tasks"})
        subscribed = await _read_json(reader)
        if subscribed.get("ok") is not True:
            writer.close()
            await writer.wait_closed()
            raise TwilightError(f"daemon rejected task subscription: {subscribed!r}")

        return cls(reader, writer, agent_uuid)

    async def close(self) -> None:
        """Close the socket and stop the background reader task."""

        self._reader_task.cancel()
        self._writer.close()
        await self._writer.wait_closed()

    async def get_registry(self) -> list[dict[str, Any]]:
        response = await self._call({"cmd": "get_registry"})
        return _expect_list(response, "agents")

    async def publish_task(self, operation: str, input_json: str | dict[str, Any]) -> str:
        response = await self._call(
            {
                "cmd": "publish_task",
                "operation": operation,
                "input_json": _json_payload(input_json),
            }
        )
        return _expect_str(response, "task_id")

    async def ask_agent(
        self,
        agent_uuid: str,
        operation: str,
        input_json: str | dict[str, Any],
    ) -> str:
        response = await self._call(
            {
                "cmd": "ask_agent",
                "agent_uuid": agent_uuid,
                "operation": operation,
                "input_json": _json_payload(input_json),
            }
        )
        return _expect_str(response, "task_id")

    async def reply_task(
        self,
        task_id: str,
        output_json: str | dict[str, Any],
        success: bool = True,
    ) -> None:
        await self._call(
            {
                "cmd": "reply_task",
                "task_id": task_id,
                "output_json": _json_payload(output_json),
                "success": success,
            }
        )

    async def list_tasks(self) -> list[dict[str, Any]]:
        response = await self._call({"cmd": "list_tasks"})
        return _expect_list(response, "tasks")

    async def ping(self) -> None:
        await self._call({"cmd": "ping"})

    async def next_event(self) -> dict[str, Any]:
        """Wait for the next pushed task_request or task_result event."""

        return await self._events.get()

    async def drain_events(self) -> list[dict[str, Any]]:
        """Return currently queued task events without blocking."""

        events: list[dict[str, Any]] = []
        while not self._events.empty():
            events.append(self._events.get_nowait())
        return events

    async def _call(self, command: dict[str, Any]) -> dict[str, Any]:
        async with self._call_lock:
            await _write_json(self._writer, command)
            response = await self._responses.get()
        if response.get("ok") is False:
            raise TwilightError(str(response.get("error", response)))
        return response

    async def _read_loop(self) -> None:
        while True:
            message = await _read_json(self._reader)
            if "event" in message:
                await self._events.put(message)
            else:
                await self._responses.put(message)


async def _write_json(writer: asyncio.StreamWriter, message: dict[str, Any]) -> None:
    writer.write(json.dumps(message, separators=(",", ":")).encode("utf-8") + b"\n")
    await writer.drain()


async def _read_json(reader: asyncio.StreamReader) -> dict[str, Any]:
    line = await reader.readline()
    if not line:
        raise TwilightError("daemon connection closed")
    return json.loads(line.decode("utf-8"))


def _json_payload(value: str | dict[str, Any]) -> str:
    if isinstance(value, str):
        return value
    return json.dumps(value, separators=(",", ":"))


def _expect_str(response: dict[str, Any], key: str) -> str:
    value = response.get(key)
    if not isinstance(value, str):
        raise TwilightError(f"missing string field {key!r} in response: {response!r}")
    return value


def _expect_list(response: dict[str, Any], key: str) -> list[dict[str, Any]]:
    value = response.get(key)
    if not isinstance(value, list):
        raise TwilightError(f"missing list field {key!r} in response: {response!r}")
    return value
