# steelseries-rr

Linux control for **SteelSeries KLC** RGB keyboards (the per-key lighting controller
found in many MSI laptops, USB id `1038:1122` and newer `1038:113a`).

SteelSeries' own software (SteelSeries GG / Engine) is Windows-only. This project is a
from-scratch Rust port of the reverse-engineered protocol so the keyboard can be
controlled natively on Linux.

## Status

Working: set the whole keyboard to a color, turn lighting off, random colors,
refresh/latch. Protocol captured from the open-source `msi-perkeyrgb` and OpenRGB
implementations (see `src/protocol.rs`).

## Build

```sh
cargo build --release
```

## Usage

```sh
# all keys red
./target/release/steelseries-rr set ff0000

# off
./target/release/steelseries-rr off

# random
./target/release/steelseries-rr random

# list devices
./target/release/steelseries-rr devices
```

Requires access to the `hidraw` node. A udev rule is usually needed:

```
KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1038", ATTRS{idProduct}=="1122", MODE="0666"
```

## License

MIT
EOF