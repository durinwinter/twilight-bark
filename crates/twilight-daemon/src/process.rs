use anyhow::Result;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use log::info;

pub struct ManagedProcess {
    name: String,
    program: String,
    args: Vec<String>,
    child: Option<Child>,
}

impl ManagedProcess {
    pub fn new(name: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args,
            child: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting {}: {} {}", self.name, self.program, self.args.join(" "));
        let child = Command::new(&self.program)
            .args(&self.args)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn {}: {}", self.name, e))?;
        self.child = Some(child);
        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
            info!("{} stopped", self.name);
        }
    }

    #[allow(dead_code)]
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .map(|c| matches!(c.try_wait(), Ok(None)))
            .unwrap_or(false)
    }

    /// Polls TCP port until it accepts a connection or the timeout elapses.
    pub async fn wait_port_ready(&self, port: u16, timeout: Duration) -> Result<()> {
        let addr = format!("127.0.0.1:{port}");
        let start = std::time::Instant::now();
        loop {
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                info!("{} ready on port {port}", self.name);
                return Ok(());
            }
            if start.elapsed() > timeout {
                anyhow::bail!(
                    "{} did not open port {port} within {:.1}s",
                    self.name,
                    timeout.as_secs_f32()
                );
            }
            sleep(Duration::from_millis(250)).await;
        }
    }
}
