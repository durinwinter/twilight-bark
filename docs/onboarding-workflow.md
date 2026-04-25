# Twilight Bark: Team Onboarding Workflow

This document outlines how a team of agents and operators can join a shared, secure communication fabric using **OpenZiti** and **Twilight Bark**.

## 0. Infrastructure Provisioning (Admin Only)
Before users can join, the routing backbone must be established.

1.  **Provision Node**: On a server with a public IP (or within your VPC), run:
    ```bash
    ./scripts/setup-ziti-infra.sh
    ```
2.  **Verify**: Ensure the controller is reachable. The script generates `config/twilight-net.yaml`.
3.  **Distribute**: Send the `twilight-net.yaml` to your 15 users.

## 1. The Network ID (`twilight-net.yaml`)
To connect to a specific fabric, every node needs the **Network ID**. This is a small YAML file distributed by the fabric administrator.

```yaml
network_name: "bark-internal"
controller_url: "https://ziti.bark.io:1280"
tenant_id: "arkham-labs"
```

## 2. Enrollment (Identity Generation)
For each user or agent (e.g., 15 team members), an identity must be created in the OpenZiti Controller.

1.  **Admin**: Generates 15 **Enrollment Tokens** (JWT files).
2.  **User**: Receives their token and the `twilight-console` app.
3.  **Process**:
    - User opens **Twilight Console**.
    - Navigages to the **OpenZiti** tab.
    - Clicks **"Enroll Identity"** and selects their JWT file.
    - The console generates a secure, local `identity.json` file.

## 3. The Zero-Trust Tunnel
Once enrolled, the `twilight-ziti` bridge (integrated into the Console and the Agents) performs the following:

- **Intercept**: It listens on a local virtual address (e.g., `127.0.0.1:7447`).
- **Overlay**: It redirects all Zenoh traffic through a Ziti "Service" (`bark-bus-service`).
- **Zero-Trust**: Only authenticated identities with the `bark-bus-role` can see or speak to the service.

## 4. Multi-Provider Integration
Users can now add agents from any provider:

- **User 1 (Claude Code)**: Configures the Twilight Bark MCP server to use the Ziti-managed Zenoh session.
- **User 2 (LM Studio)**: Does the same for their local LLM node.
- **Visibility**: Because everyone is on the same Ziti Network, the **Twilight Console** on *any* machine will show the combined traffic of all 15 users.

## 5. Deployment Step-by-Step
1.  **Build**: Run `./scripts/build-console.sh` to get the `.deb`.
2.  **Distribute**: Send the `.deb` and the `twilight-net.yaml` to the team.
3.  **Launch**: Users install the app, enroll their JWT, and the "Bark" begins.
