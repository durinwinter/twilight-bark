use twilight_proto::twilight::TwilightEnvelope;
use std::fs::OpenOptions;
use std::io::Write;
use anyhow::Result;

pub struct EventLogger {
    path: String,
}

impl EventLogger {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
        }
    }

    pub fn log_envelope(&self, envelope: &TwilightEnvelope) -> Result<()> {
        let json = serde_json::to_string(envelope)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        
        writeln!(file, "{}", json)?;
        Ok(())
    }
}
