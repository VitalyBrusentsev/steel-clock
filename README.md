# steel-clock

`steel-clock` is a small Rust daemon for the SteelSeries Arctis Nova Pro Wireless base station OLED on Linux.

It keeps a background process attached to the headset base station, renders a simple idle clock, and accepts commands over a Unix socket so other tools can change what appears on the screen.

On some Linux systems, simple HID feature reports are not enough to update the OLED reliably. The current implementation falls back to claiming USB interface `4` directly and sending HID `SET_REPORT` control transfers for OLED drawing and other display commands.

Tested so far with a SteelSeries Arctis Nova Pro Wireless base station (`1038:12e0`).

## Current protocol notes

These details were derived from local USB inspection and cross-checked against the public `JerwuQu/ggoled` reverse-engineering work:

- Vendor/product: `1038:12e0` for the Arctis Nova Pro Wireless base station.
- The OLED control path lives on HID interface `4`.
- OLED frame writes use HID report ID `0x06` with command `0x93`.
- Brightness uses command `0x85`.
- Returning to the SteelSeries UI uses command `0x95`.
- The same HID interface also emits event packets for volume, connection state, and battery-related fields.
- In the tested setup, OLED drawing only became visible once interface `4` was detached and claimed over raw USB.

The battery values are still treated as raw reverse-engineered bytes in this project. That is intentional until we validate their exact scale.

## Build

```sh
cargo build
```

## Run

Start the daemon:

```sh
cargo run -- daemon
```

Send a one-shot OLED test without the daemon:

```sh
cargo run -- draw-test "HELLO"
```

Send a one-shot blank screen without the daemon and exit immediately:

```sh
cargo run -- blank-test
```

These direct one-shot commands require the daemon to be stopped first, because the OLED interface is used exclusively while `steel-clock daemon` is running.

Send commands from another terminal:

```sh
cargo run -- text "Hello from Linux"
cargo run -- text --ttl-secs 10 "Temporary notice"
cargo run -- brightness 4
cargo run -- clock
cargo run -- clear
cargo run -- status
```

Probe matching SteelSeries HID paths:

```sh
cargo run -- dump-devices
```

## Linux permissions

To access the device without root, install the provided udev rule and reload udev:

```sh
sudo cp contrib/11-steelseries-arctis-nova.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

That rule grants access to both:

- `/dev/hidraw*` for descriptor discovery and event polling
- `/dev/bus/usb/*` for the direct USB control-transfer fallback used by OLED drawing

## systemd user service

Copy the sample unit into your user systemd directory:

```sh
mkdir -p ~/.config/systemd/user
cp contrib/steel-clock.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now steel-clock.service
```

The sample service starts the daemon with `--blank-on-exit`, so stopping the service leaves the OLED dark instead of returning to the stock SteelSeries UI.

You can also run the daemon manually with one of these exit behaviors:

```sh
steel-clock daemon --blank-on-exit
steel-clock daemon --restore-ui-on-exit
```

If you want the stock UI back explicitly while the daemon is running, use:

```sh
steel-clock return-ui
```

With the sample service installed, a simple stop is enough for night mode:

```sh
systemctl --user stop steel-clock
```

## Known limitations

- The OLED path is working, but battery and connection telemetry may stay empty while the daemon has claimed interface `4`.
- The official SteelSeries UI and `steel-clock` should not both try to control the display at the same time.
