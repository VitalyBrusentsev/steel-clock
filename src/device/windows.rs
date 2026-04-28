use super::protocol::{
    DeviceEvent, OLED_BRIGHTNESS_COMMAND, OLED_REPORT_ID, OLED_RETURN_TO_UI_COMMAND, SCREEN_HEIGHT,
    SCREEN_REPORT_WIDTH, SCREEN_WIDTH, STEELSERIES_VENDOR_ID, SUPPORTED_PRODUCT_IDS,
    TARGET_INTERFACE_NUMBER, build_draw_report, parse_event,
};
use crate::framebuffer::Framebuffer;
use anyhow::{Context, Result, bail};
use hidapi::{HidApi, HidDevice, MAX_REPORT_DESCRIPTOR_SIZE};

pub struct Device {
    oled: HidDevice,
    info: Option<HidDevice>,
}

impl Device {
    pub fn connect() -> Result<Self> {
        let api = HidApi::new().context("failed to initialize hidapi")?;
        let matches: Vec<_> = api
            .device_list()
            .filter(|device| {
                device.vendor_id() == STEELSERIES_VENDOR_ID
                    && SUPPORTED_PRODUCT_IDS.contains(&device.product_id())
                    && (device.interface_number() == TARGET_INTERFACE_NUMBER
                        || device.interface_number() < 0)
            })
            .collect();

        if matches.is_empty() {
            bail!("no supported SteelSeries Nova base station found");
        }

        let opened = matches
            .iter()
            .filter_map(|device| {
                device
                    .open_device(&api)
                    .ok()
                    .map(|opened| (*device, opened))
            })
            .collect::<Vec<_>>();

        if opened.is_empty() {
            bail!(
                "supported SteelSeries Nova base station was found but no HID path could be opened"
            );
        }

        let (oled_index, info_index) = Self::classify_opened_devices(&opened);
        let oled_info = opened[oled_index].0;
        let oled = oled_info
            .open_device(&api)
            .context("failed to open OLED HID path")?;

        let info = info_index
            .and_then(|index| opened.get(index))
            .and_then(|(device, _)| device.open_device(&api).ok());

        Ok(Self { oled, info })
    }

    fn classify_opened_devices(
        opened: &[(&hidapi::DeviceInfo, HidDevice)],
    ) -> (usize, Option<usize>) {
        let mut oled_index = None;
        let mut info_index = None;

        for (index, (_, device)) in opened.iter().enumerate() {
            let mut buffer = [0u8; MAX_REPORT_DESCRIPTOR_SIZE];
            let Ok(size) = device.get_report_descriptor(&mut buffer) else {
                continue;
            };
            let descriptor = &buffer[..size];
            if descriptor.get(1) == Some(&0xc0) {
                oled_index = Some(index);
            } else if descriptor.get(1) == Some(&0x00) {
                info_index = Some(index);
            }
        }

        (oled_index.unwrap_or(0), info_index)
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
        self.send_output_report(&report)
            .context("failed to send brightness report")
    }

    pub fn return_to_official_ui(&self) -> Result<()> {
        let mut report = [0u8; 64];
        report[0] = OLED_REPORT_ID;
        report[1] = OLED_RETURN_TO_UI_COMMAND;
        self.send_output_report(&report)
            .context("failed to return OLED to official UI")
    }

    pub fn draw_frame(&self, framebuffer: &Framebuffer) -> Result<()> {
        if framebuffer.width() != SCREEN_WIDTH || framebuffer.height() != SCREEN_HEIGHT {
            bail!("expected a {SCREEN_WIDTH}x{SCREEN_HEIGHT} framebuffer");
        }

        for start_x in (0..framebuffer.width()).step_by(SCREEN_REPORT_WIDTH) {
            let chunk_width = (framebuffer.width() - start_x).min(SCREEN_REPORT_WIDTH);
            let report =
                build_draw_report(framebuffer, start_x, 0, chunk_width, framebuffer.height());
            self.send_feature_report(&report)
                .with_context(|| format!("failed to send OLED draw report at x={start_x}"))?;
        }

        Ok(())
    }

    fn send_feature_report(&self, report: &[u8]) -> Result<()> {
        self.oled
            .send_feature_report(report)
            .context("Windows HID feature report send failed")
    }

    fn send_output_report(&self, report: &[u8]) -> Result<()> {
        self.oled
            .send_output_report(report)
            .context("Windows HID output report send failed")
    }

    pub fn read_pending_events(&self) -> Result<Vec<DeviceEvent>> {
        let Some(info) = &self.info else {
            return Ok(Vec::new());
        };

        info.set_blocking_mode(false)
            .context("failed to switch event HID path into non-blocking mode")?;

        let mut events = Vec::new();
        loop {
            let mut buffer = [0u8; 64];
            let read = info
                .read(&mut buffer)
                .context("failed while reading event HID report")?;
            if read == 0 {
                break;
            }

            if let Some(event) = parse_event(buffer) {
                events.push(event);
            }
        }

        Ok(events)
    }
}
