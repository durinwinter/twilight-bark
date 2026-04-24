# Twilight Bark

**Twilight Bark** is a distributed agent communication fabric designed for high-performance, observable, and multi-tenant agent coordination. Built on **Zenoh 1.9 (Longwang)** and **Protobuf**, it provides a robust infrastructure for agents to discover each other, exchange tasks, and share observations across heterogeneous environments.

## Core Concepts

- **Agent Identity**: Every participant in the fabric has a cryptographic-grade UUID and role-based identity.
- **The Bus**: A Zenoh-powered transport layer that handles real-time pub/sub and distributed queries.
- **Traffic Controller**: A decentralized registry and routing engine that tracks agent presence and directs message flow.
- **MCP Native**: Out-of-the-box support for Model Context Protocol (MCP), allowing AI agents to interact with the fabric as a suite of tools.

## System Architecture

Twilight Bark is organized into a modular Rust workspace:

- `twilight-bus`: Low-level Zenoh integration and transport.
- `twilight-traffic-controller`: Registry management and routing logic.
- `twilight-mcp-server`: Gateway for LLM-based agents to interact with the fabric.
- `twilight-eventlog`: High-fidelity JSONL logging for all fabric traffic.
- `adapters/`: Specialized connectors for external systems (e.g., Filesystem, Obsidian).

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Zenoh Router](https://zenoh.io/docs/getting-started/installation/) 1.1-alpha (optional but recommended for observability)

### Build

```bash
cargo build --workspace
```

### Usage

1. **Start the Traffic Controller**:
   ```bash
   cargo run -p twilight-cli -- start-node
   ```

2. **Connect an MCP Agent**:
   Point your MCP-compatible client to the `twilight-mcp-server` binary.

## Observability

All traffic in Twilight Bark is logged to structured JSONL files via the `twilight-eventlog`. This allows for post-mortem analysis, audit trails, and live monitoring integration with tools like Nuze.

## License

MIT / Apache 2.0
