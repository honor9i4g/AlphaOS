# AlphaOS v0.2.0

AlphaOS v0.2.0 introduces the first modular built-in command set for the
experimental x86-64 UEFI operating system. It includes a two-stage boot flow:
UEFI starts the AlphaOS bootloader, which loads the separate kernel EFI image.

> AlphaOS is an early learning project, not a general-purpose operating
> system. This release does not yet support a filesystem, networking, or
> running external applications.

## Highlights

- Separate UEFI bootloader and kernel images.
- A basic keyboard-driven shell with framebuffer text output.
- A modular command dispatcher, with math, timer, and power commands separated
  into command modules.
- UEFI runtime-service commands for requesting system shutdown and restart.

## Built-in commands

| Command | Description |
| --- | --- |
| `help [command]` | Show available commands or help for one command. |
| `clear` | Clear the shell screen. |
| `info` | Show AlphaOS information. |
| `version` | Show the kernel's version string. |
| `uname` | Show kernel and architecture information. |
| `whoami` | Show the current user name. |
| `echo <text>` | Print text. |
| `uptime` | Show elapsed timer ticks as uptime. |
| `time` | Alias for `uptime`. |
| `ticks` | Show the raw timer tick count. |
| `calc <a> <+|-|*|/> <b>` | Calculate with signed integers. |
| `shutdown` | Request system power-off through UEFI. |
| `restart` | Request a system restart through UEFI. |
| `commands` | List built-in shell commands. |

For example:

```text
calc 12 - 3
```

Expected result:

```text
9
```

## Build

Install the UEFI target if needed, then build the workspace from the repository
root:

```powershell
rustup target add x86_64-unknown-uefi
cargo build --release --target x86_64-unknown-uefi --workspace
```

The build produces:

```text
target/x86_64-unknown-uefi/release/AlphaOS-Bootloader.efi
target/x86_64-unknown-uefi/release/AlphaOS-Kernel.efi
```

Copy the images to the EFI System Partition using the expected boot paths:

```powershell
New-Item -ItemType Directory -Force .\esp\EFI\BOOT | Out-Null
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS-Bootloader.efi `
  .\esp\EFI\BOOT\BOOTX64.EFI -Force
Copy-Item .\target\x86_64-unknown-uefi\release\AlphaOS-Kernel.efi `
  .\esp\EFI\BOOT\ALPHAKRN.EFI -Force
```

Boot the ESP with an x86-64 UEFI environment. In QEMU, use OVMF firmware; the
firmware file paths depend on the QEMU installation. When the AlphaOS shell
appears, run `help` to see the available commands.

## Known limitations

- The kernel is currently a UEFI-loadable EFI application, not a freestanding
  ELF kernel.
- There is no filesystem or external application and driver loader.
- `apps`, `run`, and `drivers` are not implemented.
- The `version` command currently reports `0.1.0`; the displayed version has
  not yet been updated to match this release.
- The build and formatting checks pass, but the `shutdown` and `restart`
  commands have not yet been verified in a QEMU runtime test.

## Validation

- `cargo fmt --all -- --check` — passed.
- `cargo build --release --target x86_64-unknown-uefi --workspace` — passed.
- QEMU shell check for `calc 12 - 3` — returned `9`.

## License

AlphaOS is distributed under the GNU General Public License, version 3
(GPL-3.0). See [LICENCE](./LICENCE).

## Additional help
- This Version Was Also Helped By Github Copilot In VS Code
