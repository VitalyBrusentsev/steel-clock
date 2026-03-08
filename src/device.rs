use crate::framebuffer::Framebuffer;
use anyhow::{Context, Result, anyhow, bail};
use hidapi::{HidApi, HidDevice, MAX_REPORT_DESCRIPTOR_SIZE};
use nusb::{
    Device as UsbDevice, Interface as UsbInterface, MaybeFuture, list_devices,
    transfer::{ControlOut, ControlType, Recipient},
};
use std::time::Duration;

const STEELSERIES_VENDOR_ID: u16 = 0x1038;
const SUPPORTED_PRODUCT_IDS: &[u16] = &[
    0x12cb, // Arctis Nova Pro Wired
    0x12cd, // Arctis Nova Pro Wired (Xbox)
    0x12e0, // Arctis Nova Pro Wireless
    0x12e5, // Arctis Nova Pro Wireless (Xbox)
    0x225d, // White Xbox variant
];
const TARGET_INTERFACE_NUMBER: i32 = 4;
const OLED_REPORT_ID: u8 = 0x06;
const OLED_DRAW_COMMAND: u8 = 0x93;
const OLED_BRIGHTNESS_COMMAND: u8 = 0x85;
const OLED_RETURN_TO_UI_COMMAND: u8 = 0x95;
const SCREEN_REPORT_WIDTH: usize = 64;
const SCREEN_REPORT_SIZE: usize = 1024;
const HID_SET_REPORT_REQUEST: u8 = 0x09;
const HID_REPORT_TYPE_OUTPUT: u16 = 0x02;
const HID_REPORT_TYPE_FEATURE: u16 = 0x03;
const USB_CONTROL_TIMEOUT: Duration = Duration::from_millis(1000);

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

pub struct Device {
    oled: HidDevice,
    info: HidDevice,
    usb: UsbDevice,
    usb_interface: UsbInterface,
}

impl Device {
    pub fn connect() -> Result<Self> {
        let api = HidApi::new().context("failed to initialize hidapi")?;
        let matches: Vec<_> = api
            .device_list()
            .filter(|device| {
                device.vendor_id() == STEELSERIES_VENDOR_ID
                    && SUPPORTED_PRODUCT_IDS.contains(&device.product_id())
                    && device.interface_number() == TARGET_INTERFACE_NUMBER
            })
            .collect();

        if matches.is_empty() {
            bail!("no supported SteelSeries Nova base station found on HID interface 4");
        }
        if matches.len() < 2 {
            bail!(
                "found only {} matching HID interface(s); expected 2 logical report paths",
                matches.len()
            );
        }
        if matches.len() > 2 {
            bail!(
                "found {} matching HID interface entries; not sure which pair to use",
                matches.len()
            );
        }

        let (oled, info) = if matches[0].path() == matches[1].path() {
            (
                matches[0]
                    .open_device(&api)
                    .context("failed to open OLED HID path")?,
                matches[0]
                    .open_device(&api)
                    .context("failed to open event HID path")?,
            )
        } else {
            let opened = matches
                .iter()
                .map(|device| device.open_device(&api))
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to open Nova base station HID devices")?;

            Self::split_devices_by_report_descriptor(opened)?
        };

        let (usb, usb_interface) = Self::open_usb_device()?;

        Ok(Self {
            oled,
            info,
            usb,
            usb_interface,
        })
    }

    fn open_usb_device() -> Result<(UsbDevice, UsbInterface)> {
        let mut matches = list_devices()
            .wait()
            .context("failed to enumerate USB devices for fallback OLED transport")?
            .filter(|device| {
                device.vendor_id() == STEELSERIES_VENDOR_ID
                    && SUPPORTED_PRODUCT_IDS.contains(&device.product_id())
                    && device.interfaces().any(|interface| {
                        interface.interface_number() == TARGET_INTERFACE_NUMBER as u8
                    })
            });

        let Some(device_info) = matches.next() else {
            bail!("no supported SteelSeries Nova base station found on USB interface 4");
        };

        if matches.next().is_some() {
            bail!(
                "found multiple supported SteelSeries USB devices; fallback transport is ambiguous"
            );
        }

        let usb = device_info
            .open()
            .wait()
            .context("failed to open USB device for fallback OLED transport")?;

        let usb_interface = usb
            .detach_and_claim_interface(TARGET_INTERFACE_NUMBER as u8)
            .wait()
            .context("failed to detach and claim USB interface 4 for OLED fallback transport")?;

        Ok((usb, usb_interface))
    }

    fn split_devices_by_report_descriptor(
        mut devices: Vec<HidDevice>,
    ) -> Result<(HidDevice, HidDevice)> {
        let descriptors = devices
            .iter()
            .map(|device| {
                let mut buffer = [0u8; MAX_REPORT_DESCRIPTOR_SIZE];
                let size = device
                    .get_report_descriptor(&mut buffer)
                    .context("failed to fetch HID report descriptor")?;
                Ok::<Vec<u8>, anyhow::Error>(buffer[..size].to_vec())
            })
            .collect::<Result<Vec<_>>>()?;

        let oled_index = descriptors
            .iter()
            .position(|descriptor| descriptor.get(1) == Some(&0xc0))
            .ok_or_else(|| anyhow!("could not identify OLED HID report descriptor"))?;
        let info_index = descriptors
            .iter()
            .position(|descriptor| descriptor.get(1) == Some(&0x00))
            .ok_or_else(|| anyhow!("could not identify event HID report descriptor"))?;

        if oled_index == info_index {
            bail!("OLED and info descriptors resolved to the same HID entry unexpectedly");
        }

        let high = oled_index.max(info_index);
        let low = oled_index.min(info_index);
        let second = devices.swap_remove(high);
        let first = devices.swap_remove(low);

        if oled_index < info_index {
            Ok((first, second))
        } else {
            Ok((second, first))
        }
    }

    pub fn dump_supported_devices() -> Result<()> {
        let api = HidApi::new().context("failed to initialize hidapi")?;
        for device in api
            .device_list()
            .filter(|device| device.vendor_id() == STEELSERIES_VENDOR_ID)
        {
            println!(
                "product={:?} pid=0x{:04x} interface={} usage={} path={}",
                device.product_string(),
                device.product_id(),
                device.interface_number(),
                device.usage(),
                device.path().to_string_lossy(),
            );

            if let Ok(opened) = device.open_device(&api) {
                let mut buffer = [0u8; MAX_REPORT_DESCRIPTOR_SIZE];
                match opened.get_report_descriptor(&mut buffer) {
                    Ok(size) => {
                        let preview_len = size.min(16);
                        println!(
                            "  report_descriptor_size={} first_bytes={:02x?}",
                            size,
                            &buffer[..preview_len]
                        );
                    }
                    Err(error) => {
                        println!("  report_descriptor_error={error}");
                    }
                }
            } else {
                println!("  open_error=permission denied or busy");
            }
        }

        Ok(())
    }

    pub fn set_brightness(&self, value: u8) -> Result<()> {
        if !(1..=10).contains(&value) {
            bail!("brightness must be between 1 and 10");
        }

        let mut report = [0u8; 64];
        report[0] = OLED_REPORT_ID;
        report[1] = OLED_BRIGHTNESS_COMMAND;
        report[2] = value;
        self.retry_output_report(&report)
            .context("failed to send brightness report")
    }

    pub fn return_to_official_ui(&self) -> Result<()> {
        let mut report = [0u8; 64];
        report[0] = OLED_REPORT_ID;
        report[1] = OLED_RETURN_TO_UI_COMMAND;
        self.retry_output_report(&report)
            .context("failed to return OLED to official UI")
    }

    pub fn draw_frame(&self, framebuffer: &Framebuffer) -> Result<()> {
        if framebuffer.width() != 128 || framebuffer.height() != 64 {
            bail!("expected a 128x64 framebuffer");
        }

        for start_x in (0..framebuffer.width()).step_by(SCREEN_REPORT_WIDTH) {
            let chunk_width = (framebuffer.width() - start_x).min(SCREEN_REPORT_WIDTH);
            let report =
                Self::build_draw_report(framebuffer, start_x, 0, chunk_width, framebuffer.height());
            self.retry_feature_report(&report)
                .with_context(|| format!("failed to send OLED draw report at x={start_x}"))?;
        }

        Ok(())
    }

    fn retry_feature_report(&self, report: &[u8]) -> Result<()> {
        self.retry_report(report, ReportKind::Feature)
    }

    fn retry_output_report(&self, report: &[u8]) -> Result<()> {
        self.retry_report(report, ReportKind::Output)
    }

    fn retry_report(&self, report: &[u8], kind: ReportKind) -> Result<()> {
        let mut attempt = 0u64;
        loop {
            match self.send_hid_report(report, kind) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    if self.try_usb_report(report, kind).is_ok() {
                        return Ok(());
                    }

                    if attempt >= 10 {
                        return Err(error.into());
                    }

                    attempt += 1;
                    spin_sleep::sleep(Duration::from_millis(attempt.pow(2)));
                }
            }
        }
    }

    fn send_hid_report(&self, report: &[u8], kind: ReportKind) -> Result<()> {
        match kind {
            ReportKind::Feature => self
                .oled
                .send_feature_report(report)
                .context("hidraw feature report send failed"),
            ReportKind::Output => self
                .oled
                .send_output_report(report)
                .context("hidraw output report send failed"),
        }
    }

    fn try_usb_report(&self, report: &[u8], kind: ReportKind) -> Result<()> {
        if report.is_empty() {
            bail!("cannot send empty USB HID report");
        }

        let report_id = report[0];
        let payload = if report_id == 0 { &report[1..] } else { report };
        let value = (kind.report_type() << 8) | report_id as u16;

        let _keep_device_alive = &self.usb;
        self.usb_interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request: HID_SET_REPORT_REQUEST,
                    value,
                    index: TARGET_INTERFACE_NUMBER as u16,
                    data: payload,
                },
                USB_CONTROL_TIMEOUT,
            )
            .wait()
            .with_context(|| match kind {
                ReportKind::Feature => {
                    "USB control-transfer fallback for OLED feature report failed"
                }
                ReportKind::Output => "USB control-transfer fallback for OLED output report failed",
            })
    }

    fn build_draw_report(
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

    pub fn read_pending_events(&self) -> Result<Vec<DeviceEvent>> {
        self.info
            .set_blocking_mode(false)
            .context("failed to switch event HID path into non-blocking mode")?;

        let mut events = Vec::new();
        loop {
            let mut buffer = [0u8; 64];
            let read = self
                .info
                .read(&mut buffer)
                .context("failed while reading event HID report")?;
            if read == 0 {
                break;
            }

            if let Some(event) = Self::parse_event(buffer) {
                events.push(event);
            }
        }

        Ok(events)
    }

    fn parse_event(buffer: [u8; 64]) -> Option<DeviceEvent> {
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
}

#[derive(Clone, Copy)]
enum ReportKind {
    Feature,
    Output,
}

impl ReportKind {
    fn report_type(self) -> u16 {
        match self {
            Self::Feature => HID_REPORT_TYPE_FEATURE,
            Self::Output => HID_REPORT_TYPE_OUTPUT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Device, DeviceEvent};

    #[test]
    fn draw_report_header_matches_expected_wire_format() {
        let framebuffer = crate::framebuffer::Framebuffer::new(128, 64);
        let report = Device::build_draw_report(&framebuffer, 0, 0, 64, 64);
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
            Device::parse_event(packet),
            Some(DeviceEvent::Battery {
                headset: 3,
                charging: 8
            })
        );
    }
}
