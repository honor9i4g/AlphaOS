# AlphaOS

AlphaOS is an experimental x86-64 operating-system project written in Rust. It
uses a two-stage boot flow: UEFI starts the AlphaOS bootloader, and the
bootloader loads and starts a separate AlphaOS kernel image from the EFI System
Partition (ESP).

## Features

- Separate UEFI bootloader and kernel images
- Framebuffer text using the UEFI Graphics Output Protocol and a built-in
  8x8 bitmap font
- Basic keyboard-driven shell with Shift and Backspace support
- Built-in shell commands: `help`, `clear`, `info`, `version`, `uname`,
  `whoami`, `echo`, `uptime`, `time`, `ticks`, `calc`, `shutdown`, `restart`,
  and `commands`
- GitHub Actions workflow that builds both images and packages the ESP files

This is an early learning project, not a general-purpose operating system. It
does not currently provide a filesystem, networking, or process management.
The kernel is currently built as a UEFI-loadable EFI application; it is not yet
a freestanding ELF kernel.

## Requirements

- Rust toolchain with `rustup` and Cargo
- The `x86_64-unknown-uefi` Rust target
- A UEFI-capable x86-64 virtual machine or computer
- QEMU and OVMF firmware for virtual-machine testing

Install the Rust target once:

```sh
rustup target add x86_64-unknown-uefi
```

## Build

Build both images from the repository root:

```sh
cargo build --release --target x86_64-unknown-uefi --workspace
```

The build creates:

```text
target/x86_64-unknown-uefi/release/AlphaOS-Bootloader.efi
target/x86_64-unknown-uefi/release/AlphaOS-Kernel.efi
```

## Prepare the ESP

The firmware launches the bootloader at `EFI/BOOT/BOOTX64.EFI`. The
bootloader expects the kernel at `EFI/BOOT/ALPHAKRN.EFI` on the same ESP.
Prepare the repository's `esp` folder in PowerShell:

```powershell
New-Item -ItemType Directory -Force .\esp\EFI\BOOT | Out-Null
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS-Bootloader.efi `
  .\esp\EFI\BOOT\BOOTX64.EFI -Force
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS-Kernel.efi `
  .\esp\EFI\BOOT\ALPHAKRN.EFI -Force
```

Use the `esp` folder to create a FAT-formatted ESP image, or copy its contents
to an existing ESP. Both EFI files must be present at the paths above.

## Run in QEMU

Run an x86-64 QEMU machine with UEFI/OVMF firmware and attach a FAT-formatted
ESP containing both files as a bootable drive. The exact firmware and disk
image paths depend on your QEMU installation. Secure Boot must be disabled
unless you sign both EFI images with keys trusted by the firmware.

After the bootloader starts the kernel, type `help` and press Enter to see the
available shell commands.

Use `calc 12 * 3`, `echo hello`, `uptime`, or `help calc` to try the shell.
The `shutdown` and `restart` commands use UEFI runtime services to power off or
reboot the machine.
The command dispatcher is designed to grow as kernel features are added.
Application and driver loading are not implemented yet: AlphaOS still needs a
kernel filesystem and executable-loading/runtime support before it can run
separate OS applications.

The bootloader clears the UEFI text console and graphics framebuffer before
starting the kernel, removing firmware boot messages and the TianoCore splash
once AlphaOS takes over. The firmware splash may still appear briefly before
the bootloader starts; changing that initial firmware behavior requires a
firmware setting or a custom OVMF build.

GitHub Actions builds both EFI images and uploads them together as the
`alphaos-uefi` workflow artifact.

## Project layout

```text
bootloader/
  src/main.rs   Loads ALPHAKRN.EFI from the ESP and starts it through UEFI
kernel/
  src/main.rs   Kernel UEFI entry point, graphics setup, and shell handoff
  src/font.rs   Built-in bitmap font
  src/commands.rs   Shell command dispatcher
  src/commands/      Command groups, including math and timer commands
  src/port.rs   x86 I/O-port access
  src/shell.rs  Keyboard input and command shell
  src/timer.rs  Programmable interval timer counter
  src/video.rs  Framebuffer drawing helpers
esp/            Staging folder for the two EFI boot files
```
## Versions
- 0.1.0
 Having one EFI With Kernel Inside Of It no Seperate Files
- 0.2.0 
Having Cleaner And Seperate

## License

AlphaOS is licensed under the GNU General Public License, version 3 (GPL-3.0).

## Acknowledgments

This project was developed with assistance from GitHub Copilot and Leo Brave AI.
