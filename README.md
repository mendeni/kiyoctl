# kiyoctl

A native macOS control panel for the Razer Kiyo Pro webcam, with a live video
preview so you can see changes as you make them. No Synapse, no sudo.

![GUI with live preview and sliders](https://img.shields.io/badge/gui-egui-blue)

## Features

- **Standard UVC controls** with real ranges queried from the camera:
  brightness, contrast, saturation, sharpness, gamma, hue, backlight
  compensation, powerline frequency, auto exposure mode, exposure time, gain,
  white balance (auto + temperature), autofocus, focus, and zoom.
- **Razer-specific controls** via the camera's vendor extension unit:
  HDR on/off, HDR mode (bright/dark), autofocus mode (passive/responsive),
  and field of view (wide/medium/narrow).
- **Save to camera** — sends the same save-to-NVRAM command Synapse uses, so
  settings persist across replugging.
- **Live preview** — embedded AVFoundation capture, so slider changes are
  visible immediately.
- **CLI mode** for scripting.

## Install

Requires Rust and libusb (`brew install libusb`).

```sh
git clone https://github.com/samjohnduke/kiyoctl.git
cd kiyoctl
cargo build --release
```

## Usage

```sh
# GUI with live preview
./target/release/kiyoctl

# list all controls with current values and ranges
./target/release/kiyoctl probe

# set controls (standard UVC and Razer XU controls both work)
./target/release/kiyoctl set brightness=160 hdr=on hdr_mode=bright fov=wide

# persist current settings to the camera's onboard memory
./target/release/kiyoctl save
```

No `sudo` needed: control transfers go over the default endpoint and don't
require claiming the interface from the macOS UVC driver.

macOS will ask for camera permission the first time the GUI preview starts.

## How it works

The Kiyo Pro is a standard UVC camera plus a Razer vendor extension unit
(GUID `23e49ed0-1178-4f31-ae52-d2fb8a8d3b48`). kiyoctl parses the USB
configuration descriptors to locate the processing unit, camera terminal, and
extension unit, then issues UVC class control transfers (`GET_CUR`/`GET_MIN`/
`GET_MAX`/`SET_CUR`) directly via libusb. The Razer settings are set-only —
the camera doesn't report their current state.

## Credits

The Razer extension-unit byte sequences come from
[soyersoyer/kiyoproctrls](https://github.com/soyersoyer/kiyoproctrls) (MIT),
via [johnteslade/kiyoproctrls-mac](https://github.com/johnteslade/kiyoproctrls-mac).

## License

MIT
