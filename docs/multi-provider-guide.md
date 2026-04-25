# Twilight Bark: Multi-Provider LLM Integration Guide

This guide explains how to connect multiple LLM providers (including Antigravity/Claude and LM Studio) to a shared Twilight Bark fabric.

## 1. Prerequisites
- **Twilight Console** installed and running (`.deb`).
- **Network Enrolled**: You must have joined a network (Security tab).
- **Bridge Started**: Click **"Launch Bridge"** (Security tab, Port 8080).

## 2. Connecting Antigravity (Claude)
To connect me (or any Claude-based agent) to the fabric:

1.  **Configure MCP**: Add the following to your Claude/Antigravity configuration (e.g., `claude_desktop_config.json`):
    ```json
    {
      "mcpServers": {
        "twilight-bark": {
          "command": "twilight-cli",
          "args": ["mcp-server", "--port", "8080"]
        }
      }
    }
    ```
2.  **Verify**: Ask me: *"Are you connected to the Twilight Bark bus?"*. I should be able to list agents using the `list_fabric_agents` tool.

## 3. Connecting LM Studio
1.  **Start LM Studio**: Load your model (e.g., Qwen 3.5b).
2.  **Enable MCP**: In LM Studio's MCP settings, add a client that points to `http://localhost:8080`.
3.  **Identity**: The Console will see LM Studio as a participant on the bus (e.g., `lms-node-agent`).

## 4. The "Wake Up" Scenario
To test cross-provider coordination:

1.  **Observe**: Open the **Bark Bus** tab in the Console.
2.  **Command**: In my chat, tell me: *"Antigravity, send a wake-up signal to LM Studio on the bus."*
3.  **Action**: I will call the `dispatch_task` tool to send a `WAKE_UP` signal.
4.  **Verification**: 
    - You will see my `TaskRequest` in the **Monitor**.
    - You will see LM Studio's `TaskResult` (response) in the **Monitor**.
    - The **Analytics** tab will show a flow arrow between me and LM Studio.
