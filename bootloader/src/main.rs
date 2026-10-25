#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};

const KERNEL_PATH: &uefi::CStr16 = uefi::cstr16!("\\EFI\\BOOT\\ALPHAKRN.EFI");
const READ_CHUNK_SIZE: usize = 8192;
const MAX_KERNEL_SIZE: usize = 32 * 1024 * 1024;

#[no_mangle]
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

    uefi::println!("AlphaOS bootloader: loading kernel...");
    let kernel = match read_kernel() {
        Ok(kernel) => kernel,
        Err(message) => {
            uefi::println!("AlphaOS bootloader: {}", message);
            return Status::ABORTED;
        }
    };

    let kernel_handle = match uefi::boot::load_image(
        uefi::boot::image_handle(),
        uefi::boot::LoadImageSource::FromBuffer {
            buffer: &kernel,
            file_path: None,
        },
    ) {
        Ok(handle) => handle,
        Err(error) => {
            uefi::println!("AlphaOS bootloader: kernel image load failed: {:?}", error);
            return Status::ABORTED;
        }
    };

    clear_firmware_screen();
    match uefi::boot::start_image(kernel_handle) {
        Ok(()) => {
            uefi::println!("AlphaOS kernel returned unexpectedly.");
            Status::ABORTED
        }
        Err(error) => {
            uefi::println!("AlphaOS bootloader: kernel start failed: {:?}", error);
            Status::ABORTED
        }
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
