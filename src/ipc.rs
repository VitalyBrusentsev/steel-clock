use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{BufReader, BufWriter, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    SetText { text: String, ttl_secs: Option<u64> },
    ShowClock,
    Clear,
    SetBrightness { value: u8 },
    ReturnToOfficialUi,
    GetStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerResponse {
    Ok { message: String },
    Status { status: StatusSnapshot },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub brightness: u8,
    pub mode: ModeSnapshot,
    pub device: DeviceSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModeSnapshot {
    Clock,
    Text {
        text: String,
        ttl_secs_remaining: Option<u64>,
    },
    Cleared,
    OfficialUi,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceSnapshot {
    pub connected: bool,
    pub battery_headset_raw: Option<u8>,
    pub battery_charging_raw: Option<u8>,
    pub wireless_connected: Option<bool>,
    pub bluetooth_paired: Option<bool>,
    pub bluetooth_audio_active: Option<bool>,
    pub volume_percent: Option<u8>,
    pub last_error: Option<String>,
}

pub fn default_socket_path() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("steel-clock.sock");
    }

    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/steel-clock-{uid}.sock"))
}

pub fn send_command(socket_path: &Path, command: &ClientCommand) -> Result<ServerResponse> {
    let stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "failed to connect to daemon socket at {}",
            socket_path.display()
        )
    })?;
    let read_stream = stream
        .try_clone()
        .context("failed to clone Unix stream for reply")?;
    let write_stream = stream;

    let mut writer = BufWriter::new(write_stream);
    serde_json::to_writer(&mut writer, command).context("failed to encode client request")?;
    writer.flush().context("failed to flush client request")?;
    writer
        .get_ref()
        .shutdown(Shutdown::Write)
        .context("failed to half-close client request stream")?;

    let reader = BufReader::new(read_stream);
    let response: ServerResponse =
        serde_json::from_reader(reader).context("failed to decode daemon response")?;
    match &response {
        ServerResponse::Error { message } => Err(anyhow!(message.clone())),
        _ => Ok(response),
    }
}
