mod daemon;
mod device;
mod framebuffer;
mod ipc;

use crate::daemon::DaemonOptions;
use crate::device::Device;
use crate::framebuffer::Framebuffer;
use crate::ipc::{ClientCommand, ServerResponse, default_socket_path, send_command};
use anyhow::{Context, Result};
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
    Clock(SocketArgs),
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
struct DaemonArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[arg(long, default_value_t = 3)]
    brightness: u8,

    #[arg(long, default_value_t = true)]
    restore_ui_on_exit: bool,
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
        Command::Daemon(args) => daemon::run(DaemonOptions {
            socket_path: args.socket.unwrap_or_else(default_socket_path),
            brightness: args.brightness,
            restore_ui_on_exit: args.restore_ui_on_exit,
        }),
        Command::Text(args) => send_and_print(
            args.socket.as_deref(),
            ClientCommand::SetText {
                text: args.text,
                ttl_secs: args.ttl_secs,
            },
        ),
        Command::Clock(args) => send_and_print(args.socket.as_deref(), ClientCommand::ShowClock),
        Command::Clear(args) => send_and_print(args.socket.as_deref(), ClientCommand::Clear),
        Command::Brightness(args) => send_and_print(
            args.socket.as_deref(),
            ClientCommand::SetBrightness { value: args.value },
        ),
        Command::ReturnUi(args) => {
            send_and_print(args.socket.as_deref(), ClientCommand::ReturnToOfficialUi)
        }
        Command::Status(args) => send_and_print(args.socket.as_deref(), ClientCommand::GetStatus),
        Command::DumpDevices => device::Device::dump_supported_devices(),
        Command::DrawTest(args) => run_draw_test(args),
        Command::BlankTest(args) => run_blank_test(args),
    }
}

fn run_draw_test(args: DrawTestArgs) -> Result<()> {
    let device = Device::connect()?;
    device.set_brightness(args.brightness)?;
    let framebuffer = Framebuffer::from_centered_text_screen(128, 64, &args.text);
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
    device.draw_frame(&Framebuffer::new(128, 64))?;
    println!("blank test sent; process exiting");
    Ok(())
}

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
