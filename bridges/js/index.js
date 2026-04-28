"use strict";

const { createConnection } = require("node:net");
const { EventEmitter } = require("node:events");
const os = require("node:os");
const path = require("node:path");
const readline = require("node:readline");

class TwilightError extends Error {}

function defaultSocketPath() {
  if (process.env.TWILIGHT_DAEMON_SOCKET) {
    return process.env.TWILIGHT_DAEMON_SOCKET;
  }
  if (process.env.XDG_RUNTIME_DIR) {
    return path.join(process.env.XDG_RUNTIME_DIR, "twilight-daemon.sock");
  }
  return path.join("/tmp", `twilight-${os.userInfo().username}-daemon.sock`);
}

class TwilightClient extends EventEmitter {
  constructor(socket, agentUuid) {
    super();
    this.socket = socket;
    this.agentUuid = agentUuid;
    this.responses = [];
    this.waiters = [];
    this.callQueue = Promise.resolve();

    const lines = readline.createInterface({ input: socket });
    lines.on("line", (line) => this.#handleLine(line));
    socket.on("close", () => this.emit("close"));
    socket.on("error", (error) => this.emit("error", error));
  }

  static async connect({ name, role = "js-agent", socketPath = defaultSocketPath() }) {
    const socket = createConnection(socketPath);
    await once(socket, "connect");

    writeJson(socket, { cmd: "register", name, role });
    const registration = await readOneLine(socket);
    if (!registration.ok || typeof registration.agent_uuid !== "string") {
      socket.end();
      throw new TwilightError(`daemon rejected registration: ${JSON.stringify(registration)}`);
    }

    writeJson(socket, { cmd: "subscribe_tasks" });
    const subscribed = await readOneLine(socket);
    if (subscribed.ok !== true) {
      socket.end();
      throw new TwilightError(`daemon rejected task subscription: ${JSON.stringify(subscribed)}`);
    }

    return new TwilightClient(socket, registration.agent_uuid);
  }

  close() {
    this.socket.end();
  }

  async getRegistry() {
    const response = await this.#call({ cmd: "get_registry" });
    return expectArray(response, "agents");
  }

  async publishTask(operation, inputJson = {}) {
    const response = await this.#call({
      cmd: "publish_task",
      operation,
      input_json: jsonPayload(inputJson),
    });
    return expectString(response, "task_id");
  }

  async askAgent(agentUuid, operation, inputJson = {}) {
    const response = await this.#call({
      cmd: "ask_agent",
      agent_uuid: agentUuid,
      operation,
      input_json: jsonPayload(inputJson),
    });
    return expectString(response, "task_id");
  }

  async replyTask(taskId, outputJson = {}, success = true) {
    await this.#call({
      cmd: "reply_task",
      task_id: taskId,
      output_json: jsonPayload(outputJson),
      success,
    });
  }

  async listTasks() {
    const response = await this.#call({ cmd: "list_tasks" });
    return expectArray(response, "tasks");
  }

  async ping() {
    await this.#call({ cmd: "ping" });
  }

  #call(command) {
    this.callQueue = this.callQueue.then(async () => {
      writeJson(this.socket, command);
      const response = await this.#nextResponse();
      if (response.ok === false) {
        throw new TwilightError(response.error || JSON.stringify(response));
      }
      return response;
    });
    return this.callQueue;
  }

  #nextResponse() {
    if (this.responses.length > 0) {
      return Promise.resolve(this.responses.shift());
    }
    return new Promise((resolve) => this.waiters.push(resolve));
  }

  #handleLine(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      this.emit("error", error);
      return;
    }

    if (message.event) {
      this.emit("event", message);
      this.emit(message.event, message);
    } else if (this.waiters.length > 0) {
      this.waiters.shift()(message);
    } else {
      this.responses.push(message);
    }
  }
}

function writeJson(socket, message) {
  socket.write(`${JSON.stringify(message)}\n`);
}

function readOneLine(socket) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    function onData(chunk) {
      buffer += chunk.toString("utf8");
      const newline = buffer.indexOf("\n");
      if (newline === -1) {
        return;
      }
      socket.off("data", onData);
      socket.off("error", reject);
      resolve(JSON.parse(buffer.slice(0, newline)));
    }
    socket.on("data", onData);
    socket.once("error", reject);
  });
}

function once(emitter, eventName) {
  return new Promise((resolve, reject) => {
    emitter.once(eventName, resolve);
    emitter.once("error", reject);
  });
}

function jsonPayload(value) {
  return typeof value === "string" ? value : JSON.stringify(value);
}

function expectString(response, key) {
  if (typeof response[key] !== "string") {
    throw new TwilightError(`missing string field ${key}: ${JSON.stringify(response)}`);
  }
  return response[key];
}

function expectArray(response, key) {
  if (!Array.isArray(response[key])) {
    throw new TwilightError(`missing array field ${key}: ${JSON.stringify(response)}`);
  }
  return response[key];
}

module.exports = { TwilightClient, TwilightError, defaultSocketPath };
