# AlphaOS

AlphaOS is an experimental x86-64 operating-system project written in Rust.
The repository keeps its original UEFI kernel and a separate freestanding
`x86_64-unknown-none` kernel prototype so the new boot/runtime path can evolve
without losing the existing QEMU-testable shell.

## Current status

- **UEFI path**: custom UEFI boot manager, framebuffer text console, PS/2
  keyboard shell, and basic shell commands.
- **Freestanding kernel path**: Limine-bootable prototype with syscall ABI,
  ring-3/syscall entry, RAMFS, input/framebuffer console, ELF64 loader, and
  M1 app-execution foundations (spawn/exec/wait/exit-to-kernel resume path).
- **Not yet a full app-capable OS**: running Paint/Calculator-style or
  third-party apps smoothly still requires major layers: per-process virtual
  memory/isolation, scheduler/preemption/threading, userspace libc/SDK/toolchain,
  persistent filesystem/storage drivers, richer device/input/graphics drivers,
  windowing/compositor/UI toolkit, IPC/services, packaging, and security
  hardening.

## Freestanding syscall ABI

The freestanding kernel defines **500** syscall IDs (`0-499`) in
[kernel-freestanding/syscall-abi](kernel-freestanding/syscall-abi) as the
planned kernel/application interface (ABI version 1.4). IDs are sequential and
stable; each ID has name/category/argument metadata. The kernel dispatches every
ID through a real handler (no reserved ENOSYS ABI slots), but many handlers are
pragmatic freestanding implementations and some unsupported features return
defined errors such as `ENOTSUP`.

Core areas covered by the ABI include process/control, file/FD, memory,
time/system info, device/input/framebuffer, IPC/sync, networking-style handles,
and app/OS extension slots intended for future runtime growth.

Test the syscall ABI:

```powershell
cargo test --manifest-path .\kernel-freestanding\syscall-abi\Cargo.toml --target x86_64-pc-windows-msvc
```

## Freestanding M1 app-execution foundations

The freestanding kernel prototype now includes initial M1 execution-path work:

- **ELF64 loader**: loads `PT_LOAD` ELF64 executables from RAMFS into the user
  region and validates basic executable/header constraints (`ENOEXEC` on invalid
  images).
- **Kernel resume/run loop**: runs user processes on a dedicated kernel resume
  stack; `sys_exit` resumes the kernel loop instead of halting.
- **Spawn/exec/wait**: `sys_spawn` loads an ELF from RAMFS and queues a process;
  `sys_exec`/`proc_exec` replace/requeue the current process image; `sys_wait`
  reaps exited processes and reports exit status.
- **Single-address-space limitation**: the current prototype does not yet provide
  per-process page tables/isolation. Spawning from an active user process is
  currently limited (`ENOTSUP`) to avoid clobbering the shared user address
  space.

No tiny userspace demo apps are included by default.

## UEFI shell/kernel features

- Custom UEFI text boot manager with timed, keyboard-selectable boot menu
- Separate UEFI boot manager and classic kernel images
- Framebuffer text using UEFI GOP and a built-in 8x8 bitmap font
- Basic keyboard-driven shell with Shift and Backspace support
- Built-in shell commands: `help`, `clear`, `info`, `version`, `uname`,
  `whoami`, `echo`, `uptime`, `time`, `ticks`, `calc`, `shutdown`, `restart`,
  `commands`
- GitHub Actions workflow that builds/ packages UEFI artifacts

## Requirements

- Rust toolchain with `rustup` and Cargo
- `x86_64-unknown-uefi` Rust target
- `x86_64-unknown-none` Rust target (for freestanding prototype)
- QEMU + OVMF for virtual-machine testing

Install targets once:

```sh
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
```

## Build

Build the UEFI boot manager:

```sh
cargo build --release --target x86_64-unknown-uefi -p alphaos-bootloader
```

Build the freestanding kernel prototype:

```powershell
cargo build --release --manifest-path .\kernel-freestanding\Cargo.toml --target x86_64-unknown-none
```

The freestanding build produces:

```text
kernel-freestanding/target/x86_64-unknown-none/release/alphaos-kernel-freestanding
```

## Prepare the ESP / Run in QEMU

The firmware launches `EFI/BOOT/BOOTX64.EFI`. The bootloader expects the classic
kernel at `EFI/BOOT/ALPHAKRN.EFI` on the same ESP. Prepare the `esp` folder:

```powershell
New-Item -ItemType Directory -Force .\esp\EFI\BOOT | Out-Null
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS-Bootloader.efi `
  .\esp\EFI\BOOT\BOOTX64.EFI -Force
```

To enable the freestanding Limine entry, also copy the Limine UEFI executable to
`esp/EFI/BOOT/LIMINE.EFI`, `kernel-freestanding/limine.conf` to
`esp/EFI/BOOT/limine.conf` and the ESP root as `limine.conf`, and the built
kernel ELF to `esp/boot/alphaos-kernel`.

Run an x86-64 QEMU machine with UEFI/OVMF firmware and attach the FAT-formatted
ESP as a bootable drive. Secure Boot must be disabled unless EFI images are
signed with keys trusted by firmware.

## Project layout

```text
bootloader/
  src/main.rs                 UEFI boot menu; classic/Limine/Windows entries
kernel/
  src/main.rs                 Existing UEFI kernel entry point
  src/font.rs                 Built-in bitmap font
  src/commands.rs             Shell command dispatcher
  src/commands/               Command groups
  src/port.rs                 x86 I/O-port access
  src/shell.rs                Keyboard input and command shell
  src/timer.rs                PIT counter
  src/video.rs                Framebuffer drawing helpers
kernel-freestanding/
  src/main.rs                 Freestanding kernel, syscall dispatch, M1 exec path
  src/keyboard.rs             PS/2 keyboard/input queue
  linker-x86_64.ld            x86-64 kernel ELF linker layout
  syscall-abi/                Shared syscall ABI IDs/metadata/tests
  limine.conf                 Limine boot entry
esp/                          ESP staging folder
```

## Versions

- 0.1.0 - Single EFI with kernel inside (no separate files)
- 0.2.0 - Cleaner/separate boot manager and kernel layout

## License

AlphaOS is licensed under the GNU General Public License, version 3 (GPL-3.0).

## Acknowledgments

This project was developed with assistance from GitHub Copilot and Leo Brave AI.
