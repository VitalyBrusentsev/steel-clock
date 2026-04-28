use crate::framebuffer::Framebuffer;

pub const STEELSERIES_VENDOR_ID: u16 = 0x1038;
pub const SUPPORTED_PRODUCT_IDS: &[u16] = &[
    0x12cb, // Arctis Nova Pro Wired
    0x12cd, // Arctis Nova Pro Wired (Xbox)
    0x12e0, // Arctis Nova Pro Wireless
    0x12e5, // Arctis Nova Pro Wireless (Xbox)
    0x225d, // White Xbox variant
];
pub const TARGET_INTERFACE_NUMBER: i32 = 4;
pub const OLED_REPORT_ID: u8 = 0x06;
pub const OLED_DRAW_COMMAND: u8 = 0x93;
pub const OLED_BRIGHTNESS_COMMAND: u8 = 0x85;
pub const OLED_RETURN_TO_UI_COMMAND: u8 = 0x95;
#[cfg(target_os = "windows")]
pub const SCREEN_WIDTH: usize = 128;
#[cfg(target_os = "windows")]
pub const SCREEN_HEIGHT: usize = 64;
pub const SCREEN_REPORT_WIDTH: usize = 64;
pub const SCREEN_REPORT_SIZE: usize = 1024;
#[cfg(target_os = "linux")]
pub const HID_SET_REPORT_REQUEST: u8 = 0x09;
#[cfg(target_os = "linux")]
pub const HID_REPORT_TYPE_OUTPUT: u16 = 0x02;
#[cfg(target_os = "linux")]
pub const HID_REPORT_TYPE_FEATURE: u16 = 0x03;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    Volume {
        value: u8,
    },
    Battery {
        headset: u8,
        charging: u8,
    },
    HeadsetConnection {
        wireless: bool,
        bluetooth: bool,
        bluetooth_on: bool,
    },
}

pub fn build_draw_report(
    framebuffer: &Framebuffer,
    start_x: usize,
    start_y: usize,
    width: usize,
    height: usize,
) -> [u8; SCREEN_REPORT_SIZE] {
    let mut report = [0u8; SCREEN_REPORT_SIZE];
    report[0] = OLED_REPORT_ID;
    report[1] = OLED_DRAW_COMMAND;
    report[2] = start_x as u8;
    report[3] = start_y as u8;
    report[4] = width as u8;
    report[5] = height as u8;

    // The device packs pixels column-by-column in groups of 8 vertical bits.
    let stride_height = ((start_y % 8) + height).div_ceil(8) * 8;
    for y in 0..height {
        for x in 0..width {
            if !framebuffer.get(start_x + x, start_y + y) {
                continue;
            }

            let packed_index = x * stride_height + y;
            let byte_index = 6 + (packed_index / 8);
            let bit_index = packed_index % 8;
            report[byte_index] |= 1 << bit_index;
        }
    }

    report
}

pub fn parse_event(buffer: [u8; 64]) -> Option<DeviceEvent> {
    if buffer[0] != 0x07 {
        return None;
    }

    match buffer[1] {
        0x25 => Some(DeviceEvent::Volume {
            value: 0x38u8.saturating_sub(buffer[2]),
        }),
        0xb5 => Some(DeviceEvent::HeadsetConnection {
            wireless: buffer[4] == 8,
            bluetooth: buffer[3] == 1,
            bluetooth_on: buffer[2] == 4,
        }),
        0xb7 => Some(DeviceEvent::Battery {
            headset: buffer[2],
            charging: buffer[3],
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceEvent, build_draw_report, parse_event};

    #[test]
    fn draw_report_header_matches_expected_wire_format() {
        let framebuffer = crate::framebuffer::Framebuffer::new(128, 64);
        let report = build_draw_report(&framebuffer, 0, 0, 64, 64);
        assert_eq!(report[0], 0x06);
        assert_eq!(report[1], 0x93);
        assert_eq!(report[4], 64);
        assert_eq!(report[5], 64);
    }

    #[test]
    fn battery_event_is_parsed() {
        let mut packet = [0u8; 64];
        packet[0] = 0x07;
        packet[1] = 0xb7;
        packet[2] = 3;
        packet[3] = 8;
        assert_eq!(
            parse_event(packet),
            Some(DeviceEvent::Battery {
                headset: 3,
                charging: 8
            })
        );
    }
}
