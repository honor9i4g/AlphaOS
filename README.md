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
  third-party apps smoothly still requires major layers: a complete
  process scheduler/preemption/threading model, userspace libc/SDK/toolchain,
  persistent filesystem/storage drivers, richer device/input/graphics drivers,
  windowing/compositor/UI toolkit, IPC/services, packaging, and security
  hardening. M3 work has begun on per-process address-space isolation.
- **Runtime status**: M0 is boot-verified in QEMU: Limine loads the static
  freestanding ELF and the kernel reaches its framebuffer console. The M3
  boot-time check verifies separate user mappings in two address spaces, but
  this does not yet prove end-to-end process execution, interactive userspace,
  or a stable OS.

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
  cannot be mapped, and reports total/allocated/available frame bytes. Virtual
  heap allocations can be unmapped and their data frames and empty page-table
  pages reclaimed; failed multi-page allocations roll back earlier mappings.
  A boot-time self-test verifies two-page access, rejects duplicate and
  unaligned frees, checks address reuse, and confirms the allocator returns to
  its original free-frame count.
- **Kernel resume/run loop**: runs user processes on a dedicated kernel resume
  stack; `sys_exit` resumes the kernel loop instead of halting.
- **Process syscall groundwork**: process table, ELF loading, and
  spawn/exec/wait handlers exist, but there is no init process or userspace
  launcher. The initial address-space isolation and teardown work is described
  under M3 below; it does not yet provide a complete scheduler.

No tiny userspace demo apps are included by default.

## M2 exception and interrupt foundations

The freestanding kernel installs vector-specific CPU exception stubs with
correct error-code stack handling. Fatal exceptions report the vector, error
code, instruction pointer, code segment, flags, and CR2 to the framebuffer and
QEMU debug port, then halt. The double-fault gate uses a dedicated IST stack.
The remapped PIC enables a 100 Hz PIT timer and the initialized PS/2 keyboard;
IRQ counters confirm both interrupt paths, and the kernel idle loop waits with
interrupts enabled. QEMU verification reached the kernel console, observed
timer and keyboard IRQ activity, and injected an NMI to verify fatal exception
reporting.

## M3 per-process address-space groundwork (in progress)

Each loaded user image now receives a private PML4 root while retaining the
kernel mappings needed for syscall and interrupt entry. Its ELF segments, stack,
and heap occupy private user page tables. Failed loads and exited processes
reclaim their owned user frames and page-table pages. A boot-time self-test maps
the same user virtual address in two roots, verifies the contents stay separate,
and checks that cleanup restores the physical-frame count.

This is not yet a complete process scheduler: execution is still serialized,
the yield syscall does not save/restore user CPU state, and a spawned child is
not scheduled until the current process returns to the kernel run loop. User
fault isolation, preemption, an init process, and a userspace launcher remain
unfinished.

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
