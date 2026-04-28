mod device;
mod framebuffer;
mod renderer;

#[cfg(target_os = "linux")]
mod daemon;
#[cfg(target_os = "linux")]
mod ipc;
#[cfg(target_os = "windows")]
mod windows_console;

use crate::device::Device;
use crate::renderer::{blank_frame, build_text_frame};

#[cfg(target_os = "linux")]
use crate::daemon::DaemonOptions;
#[cfg(target_os = "linux")]
use crate::ipc::{ClientCommand, ServerResponse, default_socket_path, send_command};
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Background OLED controller for the SteelSeries Nova Pro base station"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Daemon(DaemonArgs),
    Text(TextArgs),
    Clock(ClockArgs),
    Clear(SocketArgs),
    Brightness(BrightnessArgs),
    ReturnUi(SocketArgs),
    Status(SocketArgs),
    DumpDevices,
    DrawTest(DrawTestArgs),
    BlankTest(BlankTestArgs),
}

#[derive(Args)]
struct SocketArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,
}

#[derive(Args)]
struct ClockArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[arg(long, default_value_t = 3)]
    brightness: u8,

    #[arg(long, default_value_t = false, conflicts_with = "blank_on_exit")]
    restore_ui_on_exit: bool,

    #[arg(long, default_value_t = true, conflicts_with = "restore_ui_on_exit")]
    blank_on_exit: bool,
}

#[derive(Args)]
struct DaemonArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[arg(long, default_value_t = 3)]
    brightness: u8,

    #[arg(long, default_value_t = false, conflicts_with = "blank_on_exit")]
    restore_ui_on_exit: bool,

    #[arg(long, default_value_t = false, conflicts_with = "restore_ui_on_exit")]
    blank_on_exit: bool,
}

#[derive(Args)]
struct TextArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[arg(long)]
    ttl_secs: Option<u64>,

    text: String,
}

#[derive(Args)]
struct BrightnessArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    value: u8,
}

#[derive(Args)]
struct DrawTestArgs {
    #[arg(long, default_value_t = 3)]
    brightness: u8,

    #[arg(long, default_value_t = false)]
    return_ui: bool,

    #[arg(default_value = "TEST")]
    text: String,
}

#[derive(Args)]
struct BlankTestArgs {
    #[arg(long, default_value_t = 3)]
    brightness: u8,
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Daemon(args) => run_daemon(args),
        Command::Text(args) => run_text(args),
        Command::Clock(args) => run_clock(args),
        Command::Clear(args) => run_clear(args),
        Command::Brightness(args) => run_brightness(args),
        Command::ReturnUi(args) => run_return_ui(args),
        Command::Status(args) => run_status(args),
        Command::DumpDevices => device::Device::dump_supported_devices(),
        Command::DrawTest(args) => run_draw_test(args),
        Command::BlankTest(args) => run_blank_test(args),
    }
}

#[cfg(target_os = "linux")]
fn run_daemon(args: DaemonArgs) -> Result<()> {
    daemon::run(DaemonOptions {
        socket_path: args.socket.unwrap_or_else(default_socket_path),
        brightness: args.brightness,
        restore_ui_on_exit: args.restore_ui_on_exit,
        blank_on_exit: args.blank_on_exit,
    })
}

#[cfg(not(target_os = "linux"))]
fn run_daemon(_args: DaemonArgs) -> Result<()> {
    anyhow::bail!("daemon mode is currently Linux-only")
}

#[cfg(target_os = "linux")]
fn run_text(args: TextArgs) -> Result<()> {
    send_and_print(
        args.socket.as_deref(),
        ClientCommand::SetText {
            text: args.text,
            ttl_secs: args.ttl_secs,
        },
    )
}

#[cfg(target_os = "windows")]
fn run_text(args: TextArgs) -> Result<()> {
    windows_console::draw_text(&args.text, args.ttl_secs)
}

#[cfg(target_os = "linux")]
fn run_clock(args: ClockArgs) -> Result<()> {
    send_and_print(args.socket.as_deref(), ClientCommand::ShowClock)
}

#[cfg(target_os = "windows")]
fn run_clock(args: ClockArgs) -> Result<()> {
    windows_console::run_clock(windows_console::ClockOptions {
        brightness: args.brightness,
        restore_ui_on_exit: args.restore_ui_on_exit,
        blank_on_exit: args.blank_on_exit,
    })
}

#[cfg(target_os = "linux")]
fn run_clear(args: SocketArgs) -> Result<()> {
    send_and_print(args.socket.as_deref(), ClientCommand::Clear)
}

#[cfg(target_os = "windows")]
fn run_clear(_args: SocketArgs) -> Result<()> {
    windows_console::clear()
}

#[cfg(target_os = "linux")]
fn run_brightness(args: BrightnessArgs) -> Result<()> {
    send_and_print(
        args.socket.as_deref(),
        ClientCommand::SetBrightness { value: args.value },
    )
}

#[cfg(target_os = "windows")]
fn run_brightness(args: BrightnessArgs) -> Result<()> {
    windows_console::set_brightness(args.value)
}

#[cfg(target_os = "linux")]
fn run_return_ui(args: SocketArgs) -> Result<()> {
    send_and_print(args.socket.as_deref(), ClientCommand::ReturnToOfficialUi)
}

#[cfg(target_os = "windows")]
fn run_return_ui(_args: SocketArgs) -> Result<()> {
    windows_console::return_ui()
}

#[cfg(target_os = "linux")]
fn run_status(args: SocketArgs) -> Result<()> {
    send_and_print(args.socket.as_deref(), ClientCommand::GetStatus)
}

#[cfg(target_os = "windows")]
fn run_status(_args: SocketArgs) -> Result<()> {
    windows_console::status()
}

fn run_draw_test(args: DrawTestArgs) -> Result<()> {
    let device = Device::connect()?;
    device.set_brightness(args.brightness)?;
    let framebuffer = build_text_frame(&args.text);
    device.draw_frame(&framebuffer)?;
    if args.return_ui {
        device.return_to_official_ui()?;
    }
    println!("draw test sent");
    Ok(())
}

fn run_blank_test(args: BlankTestArgs) -> Result<()> {
    let device = Device::connect()?;
    device.set_brightness(args.brightness)?;
    device.draw_frame(&blank_frame())?;
    println!("blank test sent; process exiting");
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_and_print(socket: Option<&std::path::Path>, command: ClientCommand) -> Result<()> {
    let socket_path = socket
        .map(PathBuf::from)
        .unwrap_or_else(default_socket_path);
    match send_command(&socket_path, &command).with_context(|| {
        format!(
            "unable to talk to steel-clock daemon at {}",
            socket_path.display()
        )
    })? {
        ServerResponse::Ok { message } => {
            println!("{message}");
            Ok(())
        }
        ServerResponse::Status { status } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&status)
                    .context("failed to serialize status response")?
            );
            Ok(())
        }
        ServerResponse::Error { message } => Err(anyhow::anyhow!(message)),
    }
}
