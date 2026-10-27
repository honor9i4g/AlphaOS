# AlphaOS

AlphaOS is an experimental x86-64 operating-system project written in Rust.
The repository currently keeps its original UEFI kernel and a separate
freestanding x86-64 kernel prototype so the new boot path can be developed
without losing the existing QEMU-testable shell.

## Features

- Separate UEFI bootloader and kernel images
- Framebuffer text using the UEFI Graphics Output Protocol and a built-in
  8x8 bitmap font
- Basic keyboard-driven shell with Shift and Backspace support
- Built-in shell commands: `help`, `clear`, `info`, `version`, `uname`,
  `whoami`, `echo`, `uptime`, `time`, `ticks`, `calc`, `shutdown`, `restart`,
  and `commands`
- GitHub Actions workflow that builds the UEFI image pair and the freestanding
  kernel prototype, then packages the UEFI ESP files
- A freestanding `x86_64-unknown-none` kernel prototype that receives a
  framebuffer through the Limine boot protocol

This is an early learning project, not a general-purpose operating system. It
does not currently provide a filesystem, networking, or process management.
The existing shell kernel is still built as a UEFI-loadable EFI application.
The new freestanding kernel is an early prototype with polled PS/2 keyboard
input and a basic text prompt. It does not yet replace the UEFI shell or
provide interrupts, memory management, commands, or application support.
Keyboard input currently supports printable US-layout keys, Shift, Backspace,
and Enter; Enter keeps the typed line on screen and opens a new prompt. Lines
are not executed as commands.

## Freestanding syscall ABI

The freestanding kernel defines 31 numbered syscall slots in
`kernel-freestanding/syscall-abi` as the planned kernel/application
interface (ABI version 1.1). Each ID has category and argument metadata;
assigned IDs are kept stable as the ABI grows. The custom AlphaOS set covers
process, file, memory, time, system-information, device, and communication
calls. Device access uses device IDs and handles; IPC uses message channels and
shared-memory handles rather than copying Linux's `ioctl`, `pipe`, or `socket`
interface. Existing memory mapping can map a shared-memory handle.

Only `get_abi_version` works through the kernel-side dispatcher; the other 30
slots return `ENOSYS` until their services exist. These definitions are the
planned ABI, not a fully operational syscall implementation. The ABI defines
an x86-64 register convention, but there is not yet a user-mode transition or
CPU syscall entry point, so `.apn` applications cannot invoke it yet. Safe
user-memory access, process management, filesystem/device services, and the
`.apn` loader must be added before app syscalls can be enabled.

The `alpha>` prompt is only for verifying keyboard input; Enter does not run
commands.

Test the syscall ABI from the `kernel-freestanding` directory:

```powershell
Set-Location .\kernel-freestanding\syscall-abi
cargo test --target x86_64-pc-windows-msvc
```

## Requirements

- Rust toolchain with `rustup` and Cargo
- The `x86_64-unknown-uefi` Rust target
- The `x86_64-unknown-none` Rust target (for the freestanding prototype)
- A UEFI-capable x86-64 virtual machine or computer
- QEMU and OVMF firmware for virtual-machine testing

Install the Rust target once:

```sh
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
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

Build the freestanding kernel prototype separately:

```powershell
Set-Location .\kernel-freestanding
cargo build --release
```

This produces
`kernel-freestanding/target/x86_64-unknown-none/release/alphaos-kernel-freestanding`,
a freestanding ELF kernel. It is not yet included in the existing UEFI ESP
image; bootable Limine media setup is a follow-up step.

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

The current QEMU instructions boot the existing UEFI shell. After the
bootloader starts that kernel, type `help` and press Enter to see its commands.
To test the freestanding kernel, build it and create a FAT ESP containing the
Limine UEFI executable at `EFI/BOOT/BOOTX64.EFI`, the repository's
`kernel-freestanding/limine.conf`, and the kernel ELF at
`boot/alphaos-kernel`. Boot that ESP using UEFI firmware. If the UEFI shell
opens instead of Limine, run `map -r`, switch to the filesystem containing the
ESP (usually `fs0:`), then run `\EFI\BOOT\BOOTX64.EFI`. Click the QEMU window
and try typing `hello`, Backspace, and Enter at the `alpha>` prompt. The
freestanding kernel currently polls the emulated PS/2 keyboard; it does not yet
support USB keyboard drivers.

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

GitHub Actions builds both EFI images and the freestanding kernel prototype.
It uploads the two EFI images together as the `alphaos-uefi` workflow artifact.

## Project layout

```text
bootloader/
  src/main.rs   Loads ALPHAKRN.EFI from the ESP and starts it through UEFI
kernel/
  src/main.rs   Existing UEFI kernel entry point, graphics setup, and shell handoff
  src/font.rs   Built-in bitmap font
  src/commands.rs   Shell command dispatcher
  src/commands/      Command groups, including math and timer commands
  src/port.rs   x86 I/O-port access
  src/shell.rs  Keyboard input and command shell
  src/timer.rs  Programmable interval timer counter
  src/video.rs  Framebuffer drawing helpers
esp/            Staging folder for the two EFI boot files
kernel-freestanding/
  src/main.rs   Freestanding Limine entry point and initial framebuffer screen
  src/keyboard.rs
                 Polled PS/2 keyboard input and US-layout scan-code mapping
  linker-x86_64.ld
                 x86-64 kernel ELF linker layout
  syscall-abi/  Shared reserved syscall IDs for the kernel and future apps
  limine.conf   Limine boot entry for the next packaging step
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
