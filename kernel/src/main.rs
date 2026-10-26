#![no_main]
#![no_std]

mod commands;
mod font;
mod port;
mod shell;
mod timer;
mod video;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;

const STACK_SIZE: usize = 64 * 1024;

#[repr(align(16))]
struct KernelStack([u8; STACK_SIZE]);

static mut KERNEL_STACK: KernelStack = KernelStack([0; STACK_SIZE]);

#[no_mangle]
pub unsafe extern "C" fn wcslen(string: *const u16) -> usize {
    let mut length = 0;
    while unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    length
}

core::arch::global_asm!(
    ".global enter_kernel",
    "enter_kernel:",
    "mov rsp, rcx",
    "and rsp, -16",
    "sub rsp, 32",
    "mov rcx, rdx",
    "mov rdx, r8",
    "call {kernel_main}",
    "ud2",
    kernel_main = sym kernel_main,
);

extern "efiapi" {
    fn enter_kernel(stack_top: u64, width: u64, height: u64) -> !;
}

#[entry]
fn main() -> Status {
    if let Err(error) = uefi::helpers::init() {
        uefi::println!("Kernel UEFI initialization failed: {:?}", error);
        return Status::ABORTED;
    }

    let gop_handle = match uefi::boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(handle) => handle,
        Err(error) => {
            uefi::println!("Kernel could not find GOP: {:?}", error);
            return Status::ABORTED;
        }
    };
    let mut gop = match uefi::boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
        Ok(gop) => gop,
        Err(error) => {
            uefi::println!("Kernel could not open GOP: {:?}", error);
            return Status::ABORTED;
        }
    };

    let (width, height) = gop.current_mode_info().resolution();
    if width == 0 || height == 0 {
        uefi::println!("Kernel received an invalid graphics resolution.");
        return Status::ABORTED;
    }
    video::init(
        gop.frame_buffer().as_mut_ptr(),
        width as usize,
        height as usize,
    );

    unsafe {
        video::draw_text(10, 10, "ALPHA OS v0.1", 0xFF, 0x00, 0x00);
        video::draw_text(10, 30, "Type help + Enter", 0x00, 0xFF, 0x00);
    }

    drop(gop);
    unsafe { core::arch::asm!("cli") };
    let _memory_map = unsafe { uefi::boot::exit_boot_services(None) };

    let stack_top = unsafe {
        core::ptr::addr_of_mut!(KERNEL_STACK.0)
            .cast::<u8>()
            .add(STACK_SIZE) as u64
    };
    unsafe { enter_kernel(stack_top, width as u64, height as u64) }
}

fn kernel_main(_width: u64, _height: u64) -> ! {
    unsafe {
        core::arch::asm!("cli");
        timer::init();
    }

    shell::init();
    loop {
        shell::poll();
        core::hint::spin_loop();
    }
}
