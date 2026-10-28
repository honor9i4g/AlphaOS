#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::time::Duration;
use uefi::prelude::*;
use uefi::proto::BootPolicy;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};

const KERNEL_PATH: &uefi::CStr16 = uefi::cstr16!("\\EFI\\BOOT\\ALPHAKRN.EFI");
const LIMINE_PATH: &uefi::CStr16 = uefi::cstr16!("\\EFI\\BOOT\\LIMINE.EFI");
const WINDOWS_BOOT_MANAGER_PATH: &uefi::CStr16 =
    uefi::cstr16!("\\EFI\\Microsoft\\Boot\\bootmgfw.efi");
const READ_CHUNK_SIZE: usize = 8192;
const MAX_KERNEL_SIZE: usize = 32 * 1024 * 1024;
const MENU_TIMEOUT_TICKS: u64 = 50;
const BOOT_ENTRY_COUNT: usize = 3;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcslen(string: *const u16) -> usize {
    let mut length = 0;
    while unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    length
}

#[entry]
fn main() -> Status {
    if let Err(error) = uefi::helpers::init() {
        uefi::println!("AlphaOS bootloader initialization failed: {:?}", error);
        return Status::ABORTED;
    }

    loop {
        let entry = match boot_menu() {
            Ok(entry) => entry,
            Err(error) => {
                uefi::println!("AlphaOS boot manager input failed: {:?}", error);
                return Status::ABORTED;
            }
        };

        if let Err(message) = boot_entry(entry) {
            uefi::println!("AlphaOS boot manager: {}", message);
            uefi::println!("Press any key to return to the boot menu.");
            if let Err(error) = wait_for_key() {
                uefi::println!("AlphaOS boot manager input failed: {:?}", error);
                return Status::ABORTED;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum BootEntry {
    Classic,
    Freestanding,
    Windows,
}

fn boot_menu() -> uefi::Result<BootEntry> {
    let mut selected = 0;
    let mut ticks = 0;
    let mut countdown_active = true;
    let mut last_displayed = u64::MAX;

    loop {
        let remaining = MENU_TIMEOUT_TICKS.saturating_sub(ticks);
        let seconds = (remaining + 9) / 10;
        if seconds != last_displayed {
            render_menu(selected, countdown_active.then_some(seconds))?;
            last_displayed = seconds;
        }

        let key = uefi::system::with_stdin(|stdin| stdin.read_key())?;
        match key {
            Some(Key::Special(ScanCode::UP)) => {
                countdown_active = false;
                selected = if selected == 0 {
                    BOOT_ENTRY_COUNT - 1
                } else {
                    selected - 1
                };
                render_menu(selected, None)?;
                last_displayed = u64::MAX;
            }
            Some(Key::Special(ScanCode::DOWN)) => {
                countdown_active = false;
                selected = (selected + 1) % BOOT_ENTRY_COUNT;
                render_menu(selected, None)?;
                last_displayed = u64::MAX;
            }
            Some(Key::Printable(character)) if character == '\r' || character == '\n' => {
                return Ok(selected.into());
            }
            Some(Key::Printable(character))
                if character == '1' || character == '2' || character == '3' =>
            {
                return Ok(if character == '1' {
                    BootEntry::Classic
                } else if character == '2' {
                    BootEntry::Freestanding
                } else {
                    BootEntry::Windows
                });
            }
            Some(_) => {
                countdown_active = false;
                render_menu(selected, None)?;
                last_displayed = u64::MAX;
            }
            None => {}
        }

        if countdown_active && ticks >= MENU_TIMEOUT_TICKS {
            return Ok(BootEntry::Classic);
        }

        uefi::boot::stall(Duration::from_millis(100));
        if countdown_active {
            ticks += 1;
        }
    }
}

impl From<usize> for BootEntry {
    fn from(index: usize) -> Self {
        match index {
            0 => Self::Classic,
            1 => Self::Freestanding,
            _ => Self::Windows,
        }
    }
}

fn render_menu(selected: usize, countdown: Option<u64>) -> uefi::Result<()> {
    uefi::system::with_stdout(|stdout| stdout.clear())?;
    uefi::println!("+--------------------------------------------------+");
    uefi::println!("|                 ALPHAOS BOOT MANAGER             |");
    uefi::println!("+--------------------------------------------------+");
    uefi::println!();
    uefi::println!(
        "{} 1. AlphaOS Classic (UEFI kernel)",
        if selected == 0 { ">" } else { " " }
    );
    uefi::println!(
        "{} 2. AlphaOS Next (freestanding, via Limine)",
        if selected == 1 { ">" } else { " " }
    );
    uefi::println!(
        "{} 3. Windows Boot Manager (optional)",
        if selected == 2 { ">" } else { " " }
    );
    uefi::println!();
    uefi::println!("Use Up/Down and Enter, or press 1, 2, or 3.");
    uefi::println!("Option 2 needs LIMINE.EFI, limine.conf and the kernel on this ESP.");
    uefi::println!("Option 3 needs Windows at EFI\\Microsoft\\Boot\\bootmgfw.efi.");
    match countdown {
        Some(seconds) => {
            uefi::println!("Starting AlphaOS Classic in {} seconds...", seconds);
        }
        None => {
            uefi::println!("Automatic boot paused. Choose an entry and press Enter.");
        }
    }

    Ok(())
}

fn boot_entry(entry: BootEntry) -> Result<(), &'static str> {
    let kernel_handle = match entry {
        BootEntry::Classic => {
            uefi::println!("AlphaOS boot manager: loading the classic kernel...");
            let kernel = read_kernel()?;
            uefi::boot::load_image(
                uefi::boot::image_handle(),
                uefi::boot::LoadImageSource::FromBuffer {
                    buffer: &kernel,
                    file_path: None,
                },
            )
            .map_err(|error| {
                uefi::println!(
                    "AlphaOS boot manager: kernel image load failed: {:?}",
                    error
                );
                "could not load the classic kernel"
            })?
        }
        BootEntry::Freestanding => {
            uefi::println!("AlphaOS boot manager: loading Limine...");
            load_efi_image_from_esp(LIMINE_PATH)?
        }
        BootEntry::Windows => {
            uefi::println!("AlphaOS boot manager: loading Windows Boot Manager...");
            load_efi_image_from_esp(WINDOWS_BOOT_MANAGER_PATH)?
        }
    };

    clear_firmware_screen();
    uefi::boot::start_image(kernel_handle).map_err(|error| {
        uefi::println!("AlphaOS boot manager: selected image failed: {:?}", error);
        "selected boot image failed to start"
    })?;
    Err("selected boot image returned unexpectedly")
}

fn load_efi_image_from_esp(path: &'static uefi::CStr16) -> Result<Handle, &'static str> {
    let loaded_image =
        uefi::boot::open_protocol_exclusive::<LoadedImage>(uefi::boot::image_handle())
            .map_err(|_| "could not inspect the boot manager image location")?;
    let device = loaded_image
        .device()
        .ok_or("could not identify the EFI system partition")?;
    drop(loaded_image);

    let device_path = uefi::boot::open_protocol_exclusive::<DevicePath>(device)
        .map_err(|_| "could not construct a path to the EFI system partition")?;
    let mut path_storage = [core::mem::MaybeUninit::uninit(); 128];
    let image_path = DevicePathBuilder::with_buf(&mut path_storage)
        .push(&build::media::FilePath { path_name: path })
        .map_err(|_| "could not construct the selected EFI image path")?
        .finalize()
        .map_err(|_| "could not finish the selected EFI image path")?;
    let full_path = device_path
        .get()
        .ok_or("EFI partition has no device path")?
        .append_path(image_path)
        .map_err(|_| "could not combine the EFI partition and image paths")?;

    uefi::boot::load_image(
        uefi::boot::image_handle(),
        uefi::boot::LoadImageSource::FromDevicePath {
            device_path: &full_path,
            boot_policy: BootPolicy::ExactMatch,
        },
    )
    .map_err(|error| {
        uefi::println!("AlphaOS boot manager: EFI image load failed: {:?}", error);
        "could not load the selected EFI image from this ESP"
    })
}

fn wait_for_key() -> uefi::Result<()> {
    loop {
        if uefi::system::with_stdin(|stdin| stdin.read_key())?.is_some() {
            return Ok(());
        }
        uefi::boot::stall(Duration::from_millis(100));
    }
}

fn clear_firmware_screen() {
    uefi::system::with_stdout(|stdout| {
        if let Err(error) = stdout.clear() {
            uefi::println!("Could not clear UEFI text console: {:?}", error);
        }
    });

    let gop_handle = match uefi::boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(handle) => handle,
        Err(error) => {
            uefi::println!("Could not find GOP to clear firmware logo: {:?}", error);
            return;
        }
    };
    let mut gop = match uefi::boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
        Ok(gop) => gop,
        Err(error) => {
            uefi::println!("Could not open GOP to clear firmware logo: {:?}", error);
            return;
        }
    };

    let mut framebuffer = gop.frame_buffer();
    unsafe {
        core::ptr::write_bytes(framebuffer.as_mut_ptr(), 0, framebuffer.size());
    }
}

fn read_kernel() -> Result<Vec<u8>, &'static str> {
    let image_handle = uefi::boot::image_handle();
    let mut filesystem =
        uefi::boot::get_image_file_system(image_handle).map_err(|_| "cannot access boot volume")?;
    let mut root = filesystem
        .open_volume()
        .map_err(|_| "cannot open the ESP root directory")?;
    let file = root
        .open(KERNEL_PATH, FileMode::Read, FileAttribute::empty())
        .map_err(|_| "kernel file not found at \\EFI\\BOOT\\ALPHAKRN.EFI")?;

    let mut kernel_file = match file.into_type().map_err(|_| "cannot inspect kernel file")? {
        FileType::Regular(file) => file,
        FileType::Dir(_) => return Err("kernel path points to a directory"),
    };

    let mut kernel = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_SIZE];
    loop {
        let read = kernel_file
            .read(&mut chunk)
            .map_err(|_| "failed while reading kernel file")?;
        if read == 0 {
            break;
        }
        let new_len = kernel
            .len()
            .checked_add(read)
            .ok_or("kernel file size overflow")?;
        if new_len > MAX_KERNEL_SIZE {
            return Err("kernel image exceeds the 32 MiB size limit");
        }
        kernel
            .try_reserve(read)
            .map_err(|_| "not enough memory to load the kernel")?;
        kernel.extend_from_slice(&chunk[..read]);
    }

    if kernel.is_empty() {
        return Err("kernel image is empty");
    }
    Ok(kernel)
}
