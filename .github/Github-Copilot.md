# AlphaOS Copilot Instructions

Use this file as project-specific guidance when changing AlphaOS. Treat the
source code and current build/test results as authoritative; update this
guidance when project goals or implementation status materially change.

## Project Goals

- AlphaOS is an experimental Rust x86-64 operating-system project, not a
  production-ready OS.
- Preserve the existing UEFI kernel and its working shell while developing
  the separate Limine-booted freestanding kernel.
- Keep the custom UEFI boot manager: AlphaOS Classic, AlphaOS Next through
  Limine, and the optional Windows Boot Manager entry.
- The long-term application target is AlphaOS's own `.apn` format and ABI.
  Application loading and runtime support are not yet implemented.

## Kernel and Input Requirements

- Keep keyboard input available in the freestanding kernel. Its current
  polled PS/2 input and framebuffer prompt are useful independently of the
  syscall work.
- Do not add a command interpreter or shell commands to the freestanding
  kernel unless the user explicitly asks for them. The existing classic UEFI
  shell is separate and should not be removed or unintentionally changed.
- Preserve the classic boot path and existing boot-menu choices when changing
  the bootloader, kernel, or ESP packaging.
- Keep the freestanding kernel bootable through Limine and test it in QEMU
  with UEFI/OVMF when changing boot, paging, privilege, or syscall code.

## System Call Direction

- Build a custom AlphaOS syscall interface for future AlphaOS applications.
  Do not copy Linux syscall numbering or claim Linux compatibility.
- Preserve every assigned syscall number and existing ABI behavior. Add new
  IDs only after the current range and document ABI changes.
- The shared ABI currently defines 31 slots (IDs 0-30). The user's priority
  is to make the requested existing calls genuinely functional before
  expanding the syscall set; do not prioritize adding calls over implementing
  and testing the current ones.
- Implement real services behind syscalls. Do not mark a call functional just
  because it has an ID, metadata, or a dispatcher branch. Return `ENOSYS`
  until the kernel has a real, safely usable backend.
- Validate user pointers, lengths, handles, and permissions before accessing
  user memory or kernel services. Do not expose kernel memory or trust
  user-provided addresses.
- Keep the syscall ABI, kernel dispatcher, and documentation consistent.
  Clearly distinguish tested functionality from reserved calls and prototype
  behavior.

## Current Implementation Baseline

- The freestanding kernel has a prototype ring-3 transition and x86-64
  `SYSCALL` entry, verified through a built-in QEMU test program.
- The prototype exercises `get_abi_version`, `getpid` (fixed PID 1), keyboard
  `read` from fd 0, and console `write` to fd 1 or 2. Other ABI slots are not
  implemented and return `ENOSYS`.
- This is not yet a general user-process or `.apn` runtime. Process
  management, a filesystem, device services, an application loader, and
  robust exception handling remain future work.
- Treat the baseline above as a checkpoint, not a promise that every local or
  packaged image is current. Rebuild and test the exact image before claiming
  new behavior works.

## Engineering and Verification

- Diagnose failures from evidence such as compiler output, QEMU logs, and
  reproducible tests. Fix root causes rather than hiding errors or suppressing
  diagnostics.
- Make focused changes that preserve existing behavior. Avoid unrelated
  refactors and do not remove user data or overwrite unrelated ESP files.
- For kernel, bootloader, or syscall changes, run the relevant formatting,
  build, and ABI tests. For changes to boot, paging, ring transitions, or
  syscalls, also boot the affected image in QEMU when feasible.
- Report the exact validation commands and whether each passed or failed.
  State clearly what was not tested, and never describe an unverified feature
  as working.
- Keep README.md and SECURITY.md accurate. Security reporting guidance should
  direct vulnerability reports to a private channel rather than requesting
  public disclosure of exploit details.