#![no_main]
#![no_std]

#[path = "../../kernel/src/font.rs"]
#[allow(dead_code, unsafe_op_in_unsafe_fn)]
mod font;

use core::arch::asm;
use limine::BaseRevision;
use limine::framebuffer::MemoryModel;
use limine::request::{FramebufferRequest, RequestsEndMarker, RequestsStartMarker};

#[derive(Clone, Copy)]
struct PixelFormat {
    red_size: u8,
    red_shift: u8,
    green_size: u8,
    green_shift: u8,
    blue_size: u8,
    blue_shift: u8,
}

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    if !BASE_REVISION.is_supported() {
        halt();
    }

    let Some(response) = FRAMEBUFFER_REQUEST.get_response() else {
        halt();
    };
    let Some(framebuffer) = response.framebuffers().next() else {
        halt();
    };

    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize;
    let pixel_format = PixelFormat {
        red_size: framebuffer.red_mask_size(),
        red_shift: framebuffer.red_mask_shift(),
        green_size: framebuffer.green_mask_size(),
        green_shift: framebuffer.green_mask_shift(),
        blue_size: framebuffer.blue_mask_size(),
        blue_shift: framebuffer.blue_mask_shift(),
    };
    let valid_framebuffer = framebuffer.bpp() == 32
        && framebuffer.memory_model() == MemoryModel::RGB
        && width > 0
        && height > 0
        && pitch >= width.saturating_mul(4)
        && pitch % 4 == 0
        && (framebuffer.addr() as usize) % core::mem::align_of::<u32>() == 0
        && valid_mask(pixel_format.red_size, pixel_format.red_shift)
        && valid_mask(pixel_format.green_size, pixel_format.green_shift)
        && valid_mask(pixel_format.blue_size, pixel_format.blue_shift);
    if !valid_framebuffer {
        halt();
    }

    let address = framebuffer.addr();

    clear_screen(address, width, height, pitch, pixel_format);
    draw_text(
        address,
        width,
        height,
        pitch,
        pixel_format,
        24,
        24,
        "ALPHAOS FREESTANDING KERNEL",
        0x45,
        0xD4,
        0x8B,
    );
    draw_text(
        address,
        width,
        height,
        pitch,
        pixel_format,
        24,
        48,
        "BOOTED THROUGH LIMINE",
        0xFF,
        0xFF,
        0xFF,
    );
    draw_text(
        address,
        width,
        height,
        pitch,
        pixel_format,
        24,
        80,
        "NEXT: MEMORY, INTERRUPTS, AND INPUT",
        0xB0,
        0xB0,
        0xB0,
    );

    halt()
}

fn valid_mask(size: u8, shift: u8) -> bool {
    size > 0 && size <= 8 && shift < 32 && u16::from(size) + u16::from(shift) <= 32
}

fn clear_screen(
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    pixel_format: PixelFormat,
) {
    for y in 0..height {
        for x in 0..width {
            write_pixel(
                address,
                width,
                height,
                pitch,
                pixel_format,
                x,
                y,
                0x10,
                0x14,
                0x1C,
            );
        }
    }
}

fn draw_text(
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    pixel_format: PixelFormat,
    x: usize,
    y: usize,
    text: &str,
    red: u8,
    green: u8,
    blue: u8,
) {
    for (index, byte) in text.bytes().enumerate() {
        if !(32..=126).contains(&byte) {
            continue;
        }

        let glyph = &font::FONT8X8[((byte - 32) as usize) * 8..((byte - 32) as usize) * 8 + 8];
        let origin_x = x.saturating_add(index.saturating_mul(8));

        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..8 {
                if bits & (0x80 >> column) != 0 {
                    write_pixel(
                        address,
                        width,
                        height,
                        pitch,
                        pixel_format,
                        origin_x + column,
                        y + row,
                        red,
                        green,
                        blue,
                    );
                }
            }
        }
    }
}

fn write_pixel(
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    pixel_format: PixelFormat,
    x: usize,
    y: usize,
    red: u8,
    green: u8,
    blue: u8,
) {
    if x >= width || y >= height {
        return;
    }

    let pixel = unsafe { address.add(y * pitch + x * 4).cast::<u32>() };
    let color = channel(red, pixel_format.red_size, pixel_format.red_shift)
        | channel(green, pixel_format.green_size, pixel_format.green_shift)
        | channel(blue, pixel_format.blue_size, pixel_format.blue_shift);
    unsafe {
        pixel.write_volatile(color);
    }
}

fn channel(value: u8, size: u8, shift: u8) -> u32 {
    let max = (1_u32 << size) - 1;
    ((u32::from(value) * max + 127) / 255) << shift
}

fn halt() -> ! {
    loop {
        unsafe {
            asm!("cli");
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    halt()
}
