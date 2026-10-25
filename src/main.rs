#![no_main]
#![no_std]

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
    if let Err(e) = uefi::helpers::init() {
        uefi::println!("Init failed: {:?}", e);
        return Status::ABORTED;
    }

    let handle = match uefi::boot::get_handle_for_protocol::<GraphicsOutput>() {
        Ok(h) => h,
        Err(e) => {
            uefi::println!("No GOP: {:?}", e);
            return Status::ABORTED;
        }
    };

    let mut gop = match uefi::boot::open_protocol_exclusive::<GraphicsOutput>(handle) {
        Ok(g) => g,
        Err(e) => {
            uefi::println!("GOP open failed: {:?}", e);
            return Status::ABORTED;
        }
    };

    let (w, h) = gop.current_mode_info().resolution();
    if w == 0 || h == 0 {
        uefi::println!("Bad resolution");
        return Status::ABORTED;
    }

    let width = w as usize;
    let height = h as usize;
    let fb = gop.frame_buffer().as_mut_ptr();

    video::init(fb, width, height);

    unsafe {
        video::draw_text(10, 10, "ALPHA OS v0.1", 0xFF, 0x00, 0x00);
        video::draw_text(10, 30, "Type help + Enter", 0x00, 0xFF, 0x00);
    }

    drop(gop);
    unsafe { core::arch::asm!("cli") };
    let _mm = unsafe { uefi::boot::exit_boot_services(None) };

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
