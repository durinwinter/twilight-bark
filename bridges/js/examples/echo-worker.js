"use strict";

const { TwilightClient } = require("../index");

async function main() {
  const client = await TwilightClient.connect({ name: "js-echo", role: "worker" });
  console.log(`registered ${client.agentUuid}`);

  client.on("task_request", async (event) => {
    await client.replyTask(event.task_id, {
      echo: event.input_json || "{}",
      worker: client.agentUuid,
    });
  });
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
