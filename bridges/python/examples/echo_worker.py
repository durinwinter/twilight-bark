"""Minimal Twilight Bark Python worker."""

import asyncio

from twilight_bark import TwilightClient


async def main() -> None:
    client = await TwilightClient.connect("python-echo", role="worker")
    print(f"registered {client.agent_uuid}")

    while True:
        event = await client.next_event()
        if event.get("event") == "task_request":
            await client.reply_task(
                event["task_id"],
                {"echo": event.get("input_json", "{}"), "worker": client.agent_uuid},
            )


if __name__ == "__main__":
    asyncio.run(main())
