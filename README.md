# steel-clock

`steel-clock` is a small Rust daemon for the SteelSeries Arctis Nova Pro base station OLED on Linux.

It keeps a background process attached to the HID interface, renders a simple idle clock, and accepts commands over a Unix socket so other tools can change what appears on the screen.

## Current protocol notes

These details were derived from local USB inspection on this machine and cross-checked against the public `JerwuQu/ggoled` reverse-engineering work:

- Vendor/product: `1038:12e0` for the Arctis Nova Pro Wireless base station seen here.
- The OLED control path lives on HID interface `4`.
- OLED frame writes use HID report ID `0x06` with command `0x93`.
- Brightness uses command `0x85`.
- Returning to the SteelSeries UI uses command `0x95`.
- The same HID interface also emits event packets for volume, connection state, and battery-related fields.

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

## systemd user service

Copy the sample unit into your user systemd directory:

```sh
mkdir -p ~/.config/systemd/user
cp contrib/steel-clock.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now steel-clock.service
```
