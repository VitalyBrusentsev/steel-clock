use crate::device::{Device, DeviceEvent};
use crate::framebuffer::Framebuffer;
use crate::ipc::{ClientCommand, DeviceSnapshot, ModeSnapshot, ServerResponse, StatusSnapshot};
use anyhow::{Context, Result, anyhow};
use chrono::{Local, Timelike};
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const OLED_WIDTH: usize = 128;
const OLED_HEIGHT: usize = 64;
const LOOP_SLEEP: Duration = Duration::from_millis(250);
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);

pub struct DaemonOptions {
    pub socket_path: PathBuf,
    pub brightness: u8,
    pub restore_ui_on_exit: bool,
}

#[derive(Debug, Clone)]
enum DisplayMode {
    Clock,
    Text {
        text: String,
        expires_at: Option<Instant>,
    },
    Cleared,
    OfficialUi,
}

#[derive(Debug, Default, Clone)]
struct RuntimeStatus {
    battery_headset_raw: Option<u8>,
    battery_charging_raw: Option<u8>,
    wireless_connected: Option<bool>,
    bluetooth_paired: Option<bool>,
    bluetooth_audio_active: Option<bool>,
    volume_percent: Option<u8>,
}

pub fn run(options: DaemonOptions) -> Result<()> {
    if let Some(parent) = options.socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    if options.socket_path.exists() {
        fs::remove_file(&options.socket_path).with_context(|| {
            format!(
                "failed to remove stale socket {}",
                options.socket_path.display()
            )
        })?;
    }

    let listener = UnixListener::bind(&options.socket_path).with_context(|| {
        format!(
            "failed to bind daemon socket at {}",
            options.socket_path.display()
        )
    })?;
    listener
        .set_nonblocking(true)
        .context("failed to switch daemon socket to non-blocking mode")?;

    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        stop_flag.store(false, Ordering::SeqCst);
    })
    .context("failed to install signal handler")?;

    let mut daemon = SteelClockDaemon::new(options.brightness, options.restore_ui_on_exit);
    while running.load(Ordering::SeqCst) {
        daemon.maybe_connect();
        daemon.handle_requests(&listener)?;
        daemon.tick();
        thread::sleep(LOOP_SLEEP);
    }

    daemon.shutdown();
    fs::remove_file(&options.socket_path).ok();
    Ok(())
}

struct SteelClockDaemon {
    device: Option<Device>,
    brightness: u8,
    restore_ui_on_exit: bool,
    mode: DisplayMode,
    device_status: RuntimeStatus,
    last_error: Option<String>,
    next_connect_attempt_at: Instant,
    last_clock_key: Option<String>,
    last_frame: Option<Framebuffer>,
    dirty: bool,
}

impl SteelClockDaemon {
    fn new(brightness: u8, restore_ui_on_exit: bool) -> Self {
        Self {
            device: None,
            brightness,
            restore_ui_on_exit,
            mode: DisplayMode::Clock,
            device_status: RuntimeStatus::default(),
            last_error: None,
            next_connect_attempt_at: Instant::now(),
            last_clock_key: None,
            last_frame: None,
            dirty: true,
        }
    }

    fn maybe_connect(&mut self) {
        if self.device.is_some() || Instant::now() < self.next_connect_attempt_at {
            return;
        }

        match Device::connect() {
            Ok(device) => {
                self.last_error = None;
                if let Err(error) = device.set_brightness(self.brightness) {
                    self.last_error = Some(format!("{error:#}"));
                }
                self.device = Some(device);
                self.last_frame = None;
                self.last_clock_key = None;
                self.dirty = true;
            }
            Err(error) => {
                self.last_error = Some(format!("{error:#}"));
                self.next_connect_attempt_at = Instant::now() + RECONNECT_BACKOFF;
            }
        }
    }

    fn tick(&mut self) {
        if self.device.is_none() {
            return;
        }

        if self.update_mode_deadlines() {
            self.dirty = true;
        }

        let render_result = match self.mode.clone() {
            DisplayMode::Clock => self.render_clock_if_needed(),
            DisplayMode::Text { text, .. } => self.render_text_if_needed(&text),
            DisplayMode::Cleared => self.render_clear_if_needed(),
            DisplayMode::OfficialUi => self.render_official_ui_if_needed(),
        };

        if let Err(error) = render_result {
            self.last_error = Some(format!("{error:#}"));
            self.device = None;
            self.next_connect_attempt_at = Instant::now() + RECONNECT_BACKOFF;
            return;
        }

        if let Err(error) = self.poll_events() {
            self.last_error = Some(format!("{error:#}"));
            self.device = None;
            self.next_connect_attempt_at = Instant::now() + RECONNECT_BACKOFF;
        }
    }

    fn poll_events(&mut self) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| anyhow!("device is not connected"))?;
        for event in device.read_pending_events()? {
            match event {
                DeviceEvent::Volume { value } => {
                    self.device_status.volume_percent = Some(((value as u16 * 100) / 0x38) as u8);
                }
                DeviceEvent::Battery { headset, charging } => {
                    self.device_status.battery_headset_raw = Some(headset);
                    self.device_status.battery_charging_raw = Some(charging);
                }
                DeviceEvent::HeadsetConnection {
                    wireless,
                    bluetooth,
                    bluetooth_on,
                } => {
                    self.device_status.wireless_connected = Some(wireless);
                    self.device_status.bluetooth_paired = Some(bluetooth);
                    self.device_status.bluetooth_audio_active = Some(bluetooth_on);
                }
            }
        }

        Ok(())
    }

    fn update_mode_deadlines(&mut self) -> bool {
        let DisplayMode::Text { expires_at, .. } = &self.mode else {
            return false;
        };

        if expires_at.is_some_and(|deadline| Instant::now() >= deadline) {
            self.mode = DisplayMode::Clock;
            self.last_clock_key = None;
            return true;
        }

        false
    }

    fn render_clock_if_needed(&mut self) -> Result<()> {
        let now = Local::now();
        let key = now.format("%Y-%m-%dT%H:%M").to_string();
        if !self.dirty && self.last_clock_key.as_ref() == Some(&key) {
            return Ok(());
        }

        let frame = self.build_clock_frame();
        self.push_frame_if_changed(frame)?;
        self.last_clock_key = Some(key);
        Ok(())
    }

    fn render_text_if_needed(&mut self, text: &str) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let frame = self.build_text_frame(text);
        self.push_frame_if_changed(frame)
    }

    fn render_clear_if_needed(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        self.push_frame_if_changed(Framebuffer::new(OLED_WIDTH, OLED_HEIGHT))
    }

    fn render_official_ui_if_needed(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let device = self
            .device
            .as_ref()
            .ok_or_else(|| anyhow!("device is not connected"))?;
        device.return_to_official_ui()?;
        self.last_frame = None;
        self.dirty = false;
        Ok(())
    }

    fn push_frame_if_changed(&mut self, frame: Framebuffer) -> Result<()> {
        if self.last_frame.as_ref() == Some(&frame) {
            self.dirty = false;
            return Ok(());
        }

        let device = self
            .device
            .as_ref()
            .ok_or_else(|| anyhow!("device is not connected"))?;
        device.draw_frame(&frame)?;
        self.last_frame = Some(frame);
        self.dirty = false;
        Ok(())
    }

    fn build_clock_frame(&self) -> Framebuffer {
        let now = Local::now();
        let minute = now.minute() as usize;
        let offsets = [-2, -1, 0, 1, 2];
        let offset = offsets[minute % offsets.len()];

        let mut frame = Framebuffer::new(OLED_WIDTH, OLED_HEIGHT);
        frame.draw_text_centered(&now.format("%H:%M").to_string(), 4, 2, offset);
        frame.draw_text_centered(&now.format("%a %d %b").to_string(), 24, 1, 0);
        frame.draw_text_centered("STEEL CLOCK", 38, 1, 0);
        frame.draw_text_centered(&self.status_line_one(), 48, 1, 0);
        frame.draw_text_centered(&self.status_line_two(), 56, 1, 0);
        frame
    }

    fn build_text_frame(&self, text: &str) -> Framebuffer {
        let mut frame = Framebuffer::new(OLED_WIDTH, OLED_HEIGHT);
        let line_count = text.lines().count().max(1);
        let scale = if line_count == 1 && text.chars().count() <= 8 {
            2
        } else {
            1
        };
        let text_height =
            (line_count as i32 * 8 * scale as i32) + (line_count.saturating_sub(1) as i32 * 2);
        let top = ((OLED_HEIGHT as i32 - text_height) / 2).max(0);
        frame.draw_multiline_centered(text, top, 2, scale);
        frame
    }

    fn status_line_one(&self) -> String {
        let rf = self
            .device_status
            .wireless_connected
            .map(|connected| if connected { "rf:on" } else { "rf:off" })
            .unwrap_or("rf:?");
        let bt = self
            .device_status
            .bluetooth_audio_active
            .map(|connected| if connected { "bt:on" } else { "bt:off" })
            .unwrap_or("bt:?");
        format!("{rf}  {bt}")
    }

    fn status_line_two(&self) -> String {
        let volume = self
            .device_status
            .volume_percent
            .map(|value| format!("vol:{value:>3}%"))
            .unwrap_or_else(|| "vol: ?".to_string());
        let battery = self
            .device_status
            .battery_headset_raw
            .map(|value| format!("bat:{value:>2}"))
            .unwrap_or_else(|| "bat:?".to_string());
        format!("{volume}  {battery}")
    }

    fn handle_requests(&mut self, listener: &UnixListener) -> Result<()> {
        loop {
            match listener.accept() {
                Ok((stream, _)) => self.handle_stream(stream)?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => {
                    return Err(error).context("failed while accepting daemon client connection");
                }
            }
        }
    }

    fn handle_stream(&mut self, stream: UnixStream) -> Result<()> {
        let read_stream = stream
            .try_clone()
            .context("failed to clone client stream for reading")?;
        let command: ClientCommand = serde_json::from_reader(BufReader::new(read_stream))
            .context("failed to decode client command")?;

        let response = match self.apply_command(command) {
            Ok(response) => response,
            Err(error) => ServerResponse::Error {
                message: format!("{error:#}"),
            },
        };

        let mut writer = BufWriter::new(stream);
        serde_json::to_writer(&mut writer, &response)
            .context("failed to encode daemon response")?;
        writer.flush().context("failed to flush daemon response")?;
        Ok(())
    }

    fn apply_command(&mut self, command: ClientCommand) -> Result<ServerResponse> {
        match command {
            ClientCommand::SetText { text, ttl_secs } => {
                let expires_at =
                    ttl_secs.map(|seconds| Instant::now() + Duration::from_secs(seconds));
                self.mode = DisplayMode::Text { text, expires_at };
                self.dirty = true;
                Ok(ServerResponse::Ok {
                    message: "text mode updated".to_string(),
                })
            }
            ClientCommand::ShowClock => {
                self.mode = DisplayMode::Clock;
                self.last_clock_key = None;
                self.dirty = true;
                Ok(ServerResponse::Ok {
                    message: "clock mode enabled".to_string(),
                })
            }
            ClientCommand::Clear => {
                self.mode = DisplayMode::Cleared;
                self.dirty = true;
                Ok(ServerResponse::Ok {
                    message: "screen cleared".to_string(),
                })
            }
            ClientCommand::SetBrightness { value } => {
                if !(1..=10).contains(&value) {
                    return Err(anyhow!("brightness must be between 1 and 10"));
                }
                self.brightness = value;
                if let Some(device) = self.device.as_ref() {
                    device.set_brightness(value)?;
                }
                Ok(ServerResponse::Ok {
                    message: format!("brightness set to {value}"),
                })
            }
            ClientCommand::ReturnToOfficialUi => {
                self.mode = DisplayMode::OfficialUi;
                self.dirty = true;
                Ok(ServerResponse::Ok {
                    message: "returned control to the official SteelSeries UI".to_string(),
                })
            }
            ClientCommand::GetStatus => Ok(ServerResponse::Status {
                status: self.status_snapshot(),
            }),
        }
    }

    fn status_snapshot(&self) -> StatusSnapshot {
        let mode = match &self.mode {
            DisplayMode::Clock => ModeSnapshot::Clock,
            DisplayMode::Text { text, expires_at } => ModeSnapshot::Text {
                text: text.clone(),
                ttl_secs_remaining: expires_at
                    .map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs()),
            },
            DisplayMode::Cleared => ModeSnapshot::Cleared,
            DisplayMode::OfficialUi => ModeSnapshot::OfficialUi,
        };

        StatusSnapshot {
            brightness: self.brightness,
            mode,
            device: DeviceSnapshot {
                connected: self.device.is_some(),
                battery_headset_raw: self.device_status.battery_headset_raw,
                battery_charging_raw: self.device_status.battery_charging_raw,
                wireless_connected: self.device_status.wireless_connected,
                bluetooth_paired: self.device_status.bluetooth_paired,
                bluetooth_audio_active: self.device_status.bluetooth_audio_active,
                volume_percent: self.device_status.volume_percent,
                last_error: self.last_error.clone(),
            },
        }
    }

    fn shutdown(&mut self) {
        if self.restore_ui_on_exit {
            if let Some(device) = self.device.as_ref() {
                let _ = device.return_to_official_ui();
            }
        }
    }
}
