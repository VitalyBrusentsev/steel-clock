mod daemon;
mod device;
mod framebuffer;
mod ipc;

use crate::daemon::DaemonOptions;
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
    }
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
