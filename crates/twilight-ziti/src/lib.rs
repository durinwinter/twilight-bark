use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ZitiIdentity {
    pub name: String,
    pub identity_file: PathBuf,
    pub controller_url: String,
}

/// Manages the ziti-tunnel sidecar process configuration. Returns the
/// (program, args) pair for the daemon's ManagedProcess to spawn.
pub struct ZitiTunnel {
    pub binary: String,
    pub identity_file: PathBuf,
    pub service: String,
    pub local_port: u16,
}

impl ZitiTunnel {
    /// Returns (program, args) for ManagedProcess::new.
    /// Command: ziti tunnel proxy --identity <file> <service>:<port>
    pub fn build_args(&self) -> (String, Vec<String>) {
        (
            self.binary.clone(),
            vec![
                "tunnel".to_string(),
                "proxy".to_string(),
                "--identity".to_string(),
                self.identity_file.to_string_lossy().to_string(),
                format!("{}:{}", self.service, self.local_port),
            ],
        )
    }
}

/// Enrolls a Ziti identity from a JWT file, writing identity.json to out_file.
/// Shells out to: ziti edge enroll --jwt <jwt> --out <out>
pub async fn enroll(binary: &str, jwt_file: &Path, out_file: &Path) -> Result<()> {
    if let Some(parent) = out_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output = tokio::process::Command::new(binary)
        .args([
            "edge",
            "enroll",
            "--jwt",
            &jwt_file.to_string_lossy(),
            "--out",
            &out_file.to_string_lossy(),
        ])
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "ziti enroll failed (exit {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    log::info!("Enrolled identity written to {:?}", out_file);
    Ok(())
}

// Legacy bridge stub — retained as placeholder for future native SDK integration
pub struct ZitiBridge {
    pub identity: ZitiIdentity,
}

impl ZitiBridge {
    pub fn new(identity: ZitiIdentity) -> Self {
        Self { identity }
    }

    pub async fn dial(&self, service_name: &str) -> Result<String> {
        Ok(format!("ziti://{}", service_name))
    }
}
