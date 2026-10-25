# AlphaOS

AlphaOS is a small experimental operating-system project written in Rust. It
boots as a 64-bit UEFI application, draws a simple shell using the UEFI
Graphics Output Protocol framebuffer, and reads keyboard input directly.

## Features

- UEFI x86-64 boot
- Framebuffer text using a built-in 8x8 bitmap font
- Basic keyboard input, including Shift and Backspace
- Shell commands: `help`, `clear`, `info`, and `time`
- Programmable interval timer counter

This is an early learning project, not a general-purpose operating system. It
does not currently provide a filesystem, networking, or process management.

## Requirements

- Rust toolchain with `rustup` and Cargo
- The `x86_64-unknown-uefi` Rust target
- A UEFI-capable virtual machine or computer
- QEMU and OVMF firmware for virtual-machine testing

Install the target once:

```powershell
rustup target add x86_64-unknown-uefi
```

## Build

From the project directory:

```powershell
cargo build --release --target x86_64-unknown-uefi
```

The EFI application is created at:

```text
target/x86_64-unknown-uefi/release/AlphaOS.efi
```

Cargo's `target` directory contains generated build files and does not need to
be committed to GitHub. Build it again locally with the command above.

## Prepare a UEFI boot image

Copy the built application to the standard removable-media UEFI boot path on
the EFI System Partition (ESP). For example, to prepare the repository's `esp`
folder in PowerShell:

```powershell
New-Item -ItemType Directory -Force .\esp\EFI\BOOT | Out-Null
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS.efi `
  .\esp\EFI\BOOT\BOOTX64.EFI -Force
```

Use `esp` as the source for a FAT-formatted ESP image, or copy its contents to
an existing ESP. The expected boot file path on the ESP is:

```text
EFI/BOOT/BOOTX64.EFI
```

## Run in QEMU

Use an x86-64 QEMU machine configured with UEFI/OVMF firmware, and attach the
FAT-formatted ESP containing `EFI/BOOT/BOOTX64.EFI` as a bootable drive. The
exact firmware and disk-image paths depend on your QEMU installation. Secure
Boot must be disabled unless you sign the EFI application with a key trusted
by the firmware.

After boot, type `help` and press Enter to see the available shell commands.

## Project layout

```text
src/
  font.rs     Bitmap font and character rendering
  main.rs     UEFI entry point, framebuffer setup, and kernel handoff
  port.rs     x86 I/O-port access
  shell.rs    Keyboard input and command shell
  timer.rs    Programmable interval timer setup and counter
  video.rs    Framebuffer drawing helpers
esp/          Optional folder used to prepare a UEFI boot image
```

## License

AlphaOS is licensed under the GNU General Public License, version 3 (GPL-3.0).

## Acknowledgments

This project was developed with assistance from GitHub Copilot and Leo Brave AI.