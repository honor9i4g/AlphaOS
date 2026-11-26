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
- **Runtime status**: M0 is boot-verified in QEMU: Limine loads the static
  freestanding ELF and the kernel reaches its framebuffer console. This does
  not yet prove an interactive userspace, process isolation, or a stable OS.

## Freestanding syscall ABI

The freestanding kernel defines **500** syscall IDs (`0-499`) in
[kernel-freestanding/syscall-abi](kernel-freestanding/syscall-abi) as the
planned kernel/application interface (ABI version 1.4). IDs are sequential and
stable; each ID has name/category/argument metadata. This is the ABI table size,
not a claim that 500 complete OS services are implemented: many handlers are
minimal stubs or return defined errors such as `ENOTSUP`.

Core areas covered by the ABI include process/control, file/FD, memory,
time/system info, device/input/framebuffer, IPC/sync, networking-style handles,
and app/OS extension slots intended for future runtime growth.

Test the syscall ABI:

```powershell
cargo test --manifest-path .\kernel-freestanding\syscall-abi\Cargo.toml --target x86_64-pc-windows-msvc
```

## Freestanding M1 app-execution foundations

The freestanding kernel prototype now includes initial M1 execution-path work:

- **ELF64 loader**: validates ELF headers and program-table bounds, segment file
  and memory ranges, page alignment/overlap, executable entry-point membership,
  and writable-executable segment policy before mapping `PT_LOAD` segments from
  RAMFS. Segment write/execute permissions are reflected in page-table entries;
  non-executable pages use NX when supported. A four-page user stack is mapped,
  and syscall buffer checks validate present user mappings and write permission
  for kernel-to-user buffers. Invalid images return `ENOEXEC`; out-of-range
  segments return `EFAULT`.
- **Physical frames**: builds a free-page list from Limine-reported usable
  memory, zeroes frames on allocation, returns an unused frame if an ELF page
  cannot be mapped, and reports total/allocated/available frame bytes. General
  unmapping and process teardown still need to return their frames.
- **Kernel resume/run loop**: runs user processes on a dedicated kernel resume
  stack; `sys_exit` resumes the kernel loop instead of halting.
- **Process syscall groundwork**: process table, ELF loading, and
  spawn/exec/wait handlers exist, but there is no init process or userspace
  launcher. Active-process spawn/exec and complete lifecycle/reclamation are
  not yet a working isolated process model.
- **Single-address-space limitation**: the current prototype still has no
  per-process page tables/isolation or general user-page reclamation. Spawning
  from an active user process is limited (`ENOTSUP`) to avoid clobbering shared
  mappings. No init userspace process or bundled demo app is provided.

No tiny userspace demo apps are included by default.

Run the focused ELF validation tests from the repository root:

```powershell
rustc --edition=2024 --test .\kernel-freestanding\tests\elf_validation.rs -o .\kernel-freestanding\target\elf-validation.exe
.\kernel-freestanding\target\elf-validation.exe
```

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
Push-Location .\kernel-freestanding
cargo build --release --target x86_64-unknown-none
Pop-Location
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
