use crate::device::DeviceEvent;
use crate::framebuffer::Framebuffer;
use chrono::{Local, Timelike};

pub const OLED_WIDTH: usize = 128;
pub const OLED_HEIGHT: usize = 64;

const CLOCK_TIME_SCALE: usize = 3;
const CLOCK_TIME_FONT_HEIGHT: f32 = 36.0;
const CLOCK_DATE_GLYPH_SIZE: usize = 13;
const STATUS_FOOTER_Y: i32 = 56;

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
    let has_status = status.has_indicators();

    let time_y = if has_status { 0 } else { 4 };
    let (date_y, weekday_y) = if has_status { (31, 44) } else { (39, 53) };
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

    if has_status {
        draw_status_footer(&mut frame, status, STATUS_FOOTER_Y);
    }

    frame
}

pub fn build_text_frame(text: &str) -> Framebuffer {
    Framebuffer::from_centered_text_screen(OLED_WIDTH, OLED_HEIGHT, text)
}

pub fn blank_frame() -> Framebuffer {
    Framebuffer::new(OLED_WIDTH, OLED_HEIGHT)
}

impl RuntimeStatus {
    fn has_indicators(&self) -> bool {
        self.wireless_connected.is_some()
            || self.bluetooth_audio_active.is_some()
            || self.battery_headset_raw.is_some()
    }
}

fn draw_status_footer(frame: &mut Framebuffer, status: &RuntimeStatus, y: i32) {
    draw_checkbox_label(
        frame,
        6,
        y,
        "RF",
        status.wireless_connected.unwrap_or(false),
    );
    draw_checkbox_label(
        frame,
        43,
        y,
        "BT",
        status.bluetooth_audio_active.unwrap_or(false),
    );
    draw_battery_indicator(frame, 80, y + 1, status.battery_headset_raw);
}

fn draw_checkbox_label(frame: &mut Framebuffer, x: i32, y: i32, label: &str, checked: bool) {
    draw_box(frame, x, y + 1, 6, 6);
    if checked {
        draw_line(frame, x + 1, y + 3, x + 2, y + 4);
        draw_line(frame, x + 2, y + 4, x + 4, y + 2);
    }
    frame.draw_text(label, x + 9, y, 1);
}

fn draw_battery_indicator(frame: &mut Framebuffer, x: i32, y: i32, level: Option<u8>) {
    const BODY_WIDTH: i32 = 38;
    const BODY_HEIGHT: i32 = 6;
    const SEGMENTS: i32 = 4;

    draw_box(frame, x, y, BODY_WIDTH, BODY_HEIGHT);
    draw_vertical_line(frame, x + BODY_WIDTH, y + 2, y + 3);

    let Some(level) = level else {
        draw_line(frame, x + 3, y + 4, x + BODY_WIDTH - 4, y + 1);
        return;
    };

    let filled_segments = (level as i32).clamp(0, SEGMENTS);
    let segment_width = 7;
    for segment in 0..filled_segments {
        let start_x = x + 3 + segment * (segment_width + 1);
        fill_rect(frame, start_x, y + 2, segment_width, BODY_HEIGHT - 3);
    }
}

fn draw_box(frame: &mut Framebuffer, x: i32, y: i32, width: i32, height: i32) {
    draw_horizontal_line(frame, x, x + width - 1, y);
    draw_horizontal_line(frame, x, x + width - 1, y + height - 1);
    draw_vertical_line(frame, x, y, y + height - 1);
    draw_vertical_line(frame, x + width - 1, y, y + height - 1);
}

fn fill_rect(frame: &mut Framebuffer, x: i32, y: i32, width: i32, height: i32) {
    for py in y..(y + height) {
        draw_horizontal_line(frame, x, x + width - 1, py);
    }
}

fn draw_horizontal_line(frame: &mut Framebuffer, start_x: i32, end_x: i32, y: i32) {
    for x in start_x..=end_x {
        set_pixel(frame, x, y);
    }
}

fn draw_vertical_line(frame: &mut Framebuffer, x: i32, start_y: i32, end_y: i32) {
    for y in start_y..=end_y {
        set_pixel(frame, x, y);
    }
}

fn draw_line(frame: &mut Framebuffer, start_x: i32, start_y: i32, end_x: i32, end_y: i32) {
    let dx = (end_x - start_x).abs();
    let dy = -(end_y - start_y).abs();
    let step_x = if start_x < end_x { 1 } else { -1 };
    let step_y = if start_y < end_y { 1 } else { -1 };
    let mut error = dx + dy;
    let mut x = start_x;
    let mut y = start_y;

    loop {
        set_pixel(frame, x, y);
        if x == end_x && y == end_y {
            break;
        }

        let next_error = 2 * error;
        if next_error >= dy {
            error += dy;
            x += step_x;
        }
        if next_error <= dx {
            error += dx;
            y += step_y;
        }
    }
}

fn set_pixel(frame: &mut Framebuffer, x: i32, y: i32) {
    if x >= 0 && y >= 0 {
        frame.set(x as usize, y as usize, true);
    }
}
