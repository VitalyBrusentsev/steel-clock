use crate::device::Device;
use crate::renderer::{RuntimeStatus, blank_frame, build_clock_frame, build_text_frame};
use anyhow::{Context, Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const LOOP_SLEEP: Duration = Duration::from_millis(250);

pub struct ClockOptions {
    pub brightness: u8,
    pub restore_ui_on_exit: bool,
    pub blank_on_exit: bool,
}

pub fn run_clock(options: ClockOptions) -> Result<()> {
    let device = Device::connect()?;
    device.set_brightness(options.brightness)?;

    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        stop_flag.store(false, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler")?;

    println!("clock running; press Ctrl-C to blank and exit");

    let mut status = RuntimeStatus::default();
    let mut last_clock_key = None;
    let mut last_frame = None;
    while running.load(Ordering::SeqCst) {
        for event in device.read_pending_events()? {
            status.apply_event(event);
            last_clock_key = None;
        }

        let key = chrono::Local::now().format("%Y-%m-%dT%H:%M").to_string();
        if last_clock_key.as_ref() != Some(&key) {
            let frame = build_clock_frame(&status);
            if last_frame.as_ref() != Some(&frame) {
                device.draw_frame(&frame)?;
                last_frame = Some(frame);
            }
            last_clock_key = Some(key);
        }

        thread::sleep(LOOP_SLEEP);
    }

    if options.blank_on_exit {
        device.draw_frame(&blank_frame())?;
    } else if options.restore_ui_on_exit {
        device.return_to_official_ui()?;
    }

    Ok(())
}

pub fn draw_text(text: &str, ttl_secs: Option<u64>) -> Result<()> {
    if ttl_secs.is_some() {
        bail!("--ttl-secs requires the Linux daemon; Windows text is currently a one-shot draw");
    }

    let device = Device::connect()?;
    device.draw_frame(&build_text_frame(text))?;
    println!("text frame sent");
    Ok(())
}

pub fn clear() -> Result<()> {
    let device = Device::connect()?;
    device.draw_frame(&blank_frame())?;
    println!("screen cleared");
    Ok(())
}

pub fn set_brightness(value: u8) -> Result<()> {
    let device = Device::connect()?;
    device.set_brightness(value)?;
    println!("brightness set to {value}");
    Ok(())
}

pub fn return_ui() -> Result<()> {
    let device = Device::connect()?;
    device.return_to_official_ui()?;
    println!("returned control to the official SteelSeries UI");
    Ok(())
}

pub fn status() -> Result<()> {
    let device = Device::connect()?;
    let mut status = RuntimeStatus::default();
    let started_at = Instant::now();
    while started_at.elapsed() < Duration::from_millis(100) {
        for event in device.read_pending_events()? {
            status.apply_event(event);
        }
        thread::sleep(Duration::from_millis(10));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&status_snapshot(status))?
    );
    Ok(())
}

#[derive(serde::Serialize)]
struct WindowsStatusSnapshot {
    connected: bool,
    battery_headset_raw: Option<u8>,
    battery_charging_raw: Option<u8>,
    wireless_connected: Option<bool>,
    bluetooth_paired: Option<bool>,
    bluetooth_audio_active: Option<bool>,
    volume_percent: Option<u8>,
}

fn status_snapshot(status: RuntimeStatus) -> WindowsStatusSnapshot {
    WindowsStatusSnapshot {
        connected: true,
        battery_headset_raw: status.battery_headset_raw,
        battery_charging_raw: status.battery_charging_raw,
        wireless_connected: status.wireless_connected,
        bluetooth_paired: status.bluetooth_paired,
        bluetooth_audio_active: status.bluetooth_audio_active,
        volume_percent: status.volume_percent,
    }
}
