use crate::device::DeviceEvent;
use crate::framebuffer::Framebuffer;
use chrono::{Local, Timelike};

pub const OLED_WIDTH: usize = 128;
pub const OLED_HEIGHT: usize = 64;

const CLOCK_TIME_SCALE: usize = 3;
const CLOCK_TIME_FONT_HEIGHT: f32 = 36.0;
const CLOCK_DATE_GLYPH_SIZE: usize = 13;

#[derive(Debug, Default, Clone)]
pub struct RuntimeStatus {
    pub battery_headset_raw: Option<u8>,
    pub battery_charging_raw: Option<u8>,
    pub wireless_connected: Option<bool>,
    pub bluetooth_paired: Option<bool>,
    pub bluetooth_audio_active: Option<bool>,
    pub volume_percent: Option<u8>,
}

impl RuntimeStatus {
    pub fn apply_event(&mut self, event: DeviceEvent) {
        match event {
            DeviceEvent::Volume { value } => {
                self.volume_percent = Some(((value as u16 * 100) / 0x38) as u8);
            }
            DeviceEvent::Battery { headset, charging } => {
                self.battery_headset_raw = Some(headset);
                self.battery_charging_raw = Some(charging);
            }
            DeviceEvent::HeadsetConnection {
                wireless,
                bluetooth,
                bluetooth_on,
            } => {
                self.wireless_connected = Some(wireless);
                self.bluetooth_paired = Some(bluetooth);
                self.bluetooth_audio_active = Some(bluetooth_on);
            }
        }
    }
}

pub fn build_clock_frame(status: &RuntimeStatus) -> Framebuffer {
    let now = Local::now();
    let minute = now.minute() as usize;
    let offsets = [-2, -1, 0, 1, 2];
    let offset = offsets[minute % offsets.len()];
    let status_lines = status_lines(status);
    let status_count = status_lines.len();

    let time_y = match status_count {
        0 => 4,
        1 => 2,
        _ => 0,
    };
    let (date_y, weekday_y) = match status_count {
        0 => (39, 53),
        1 => (36, 49),
        _ => (31, 44),
    };
    let date_text = now.format("%d %b").to_string();
    let weekday_text = now.format("%A").to_string();

    let mut frame = Framebuffer::new(OLED_WIDTH, OLED_HEIGHT);
    if !frame.draw_clock_text_centered(
        &now.format("%H:%M").to_string(),
        time_y,
        CLOCK_TIME_FONT_HEIGHT,
        offset,
    ) {
        frame.draw_text_centered(
            &now.format("%H:%M").to_string(),
            time_y + 4,
            CLOCK_TIME_SCALE,
            offset,
        );
    }

    frame.draw_text_centered_scaled(
        &date_text,
        date_y,
        CLOCK_DATE_GLYPH_SIZE,
        CLOCK_DATE_GLYPH_SIZE,
        0,
    );
    frame.draw_text_centered(&weekday_text, weekday_y, 1, 0);

    if let Some(line) = status_lines.first() {
        frame.draw_text_centered(line, 48, 1, 0);
    }
    if let Some(line) = status_lines.get(1) {
        frame.draw_text_centered(line, 56, 1, 0);
    }
    frame
}

pub fn build_text_frame(text: &str) -> Framebuffer {
    Framebuffer::from_centered_text_screen(OLED_WIDTH, OLED_HEIGHT, text)
}

pub fn blank_frame() -> Framebuffer {
    Framebuffer::new(OLED_WIDTH, OLED_HEIGHT)
}

fn status_lines(status: &RuntimeStatus) -> Vec<String> {
    let mut items = Vec::new();

    if let Some(connected) = status.wireless_connected {
        items.push(if connected {
            "rf:on".to_string()
        } else {
            "rf:off".to_string()
        });
    }

    if let Some(active) = status.bluetooth_audio_active {
        items.push(if active {
            "bt:on".to_string()
        } else {
            "bt:off".to_string()
        });
    }

    if let Some(volume) = status.volume_percent {
        items.push(format!("vol:{volume:>3}%"));
    }

    if let Some(battery) = status.battery_headset_raw {
        items.push(format!("bat:{battery:>2}"));
    }

    match items.len() {
        0 => Vec::new(),
        1 | 2 => vec![items.join("  ")],
        _ => {
            let split_at = items.len().div_ceil(2);
            vec![items[..split_at].join("  "), items[split_at..].join("  ")]
        }
    }
}
