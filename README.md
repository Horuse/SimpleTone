# SimpleTone — Cross-Platform Audio Test Plugin (CLAP + VST3)

A lightweight audio plugin designed for testing DAW and audio host implementations across multiple operating systems and CPU architectures.

Supports both **CLAP** and **VST3** formats on **macOS**, **Linux**, and **Windows** for both **x86_64** and **ARM64**.

---

## Supported Platforms and Architectures

| Platform | Architectures | Formats | Output Bundle |
|---|---|---|---|
| **macOS** | `arm64` (Apple Silicon) + `x86_64` (Intel) | CLAP, VST3 | `SimpleTone-macos-universal.zip` (Universal Binary) |
| **Linux** | `x86_64` | CLAP, VST3 | `SimpleTone-linux-x86_64.zip` |
| **Linux** | `aarch64` (ARM64) | CLAP, VST3 | `SimpleTone-linux-arm64.zip` |
| **Windows** | `x86_64` | CLAP, VST3 | `SimpleTone-windows-x86_64.zip` |
| **Windows** | `arm64` (ARM64) | CLAP, VST3 | `SimpleTone-windows-arm64.zip` |

---

## Features

1. **Test Tone Generator**:
   - Built-in sine wave generator (440 Hz default, tunable from 20 Hz to 20,000 Hz).
   - Ideal for testing hosts inside virtual machines where audio input devices may not be configured.
2. **Audio Processing**:
   - Master Gain (-60 dB to +12 dB with logarithmic smoothing).
   - Fast Mute toggle.
3. **Graphical User Interface (egui GUI)**:
   - Lightweight, responsive interface.
   - Displays real-time platform and architecture info badge (`Platform: <os> | Arch: <arch>`) to immediately confirm that the host loaded the correct ARM64 or x86_64 binary.
   - Real-time Output Peak Meter (dBFS).
4. **Flexible Audio I/O Configurations**:
   - Stereo In / Stereo Out (Effect)
   - Mono In / Mono Out (Effect)
   - 0 In / Stereo Out (Instrument / Generator)

---

## Standard Plugin Installation Directories

### macOS
- **CLAP**: `~/Library/Audio/Plug-Ins/CLAP/`
- **VST3**: `~/Library/Audio/Plug-Ins/VST3/`

### Linux (`x86_64` & `aarch64`)
- **CLAP**: `~/.clap/` or `/usr/lib/clap/`
- **VST3**: `~/.vst3/` or `/usr/lib/vst3/`

### Windows (`x86_64` & `arm64`)
- **CLAP**: `%COMMONPROGRAMFILES%\CLAP\` (typically `C:\Program Files\Common Files\CLAP\`)
- **VST3**: `%COMMONPROGRAMFILES%\VST3\` (typically `C:\Program Files\Common Files\VST3\`)

---

## Local Build (macOS)

To build and package the plugin locally on macOS:

```bash
# Build and create SimpleTone.clap and SimpleTone.vst3 bundles:
cargo xtask bundle simple_tone --release

# The bundled plugins will be located in:
target/bundled/
```
