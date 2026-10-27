#![no_main]
#![no_std]

#[path = "../../kernel/src/font.rs"]
#[allow(dead_code, unsafe_op_in_unsafe_fn)]
mod font;
mod keyboard;

use alphaos_syscall_abi as syscall;
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

struct Console {
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    pixel_format: PixelFormat,
    cursor_y: usize,
    input: [u8; 128],
    input_len: usize,
    keyboard: keyboard::Keyboard,
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
    if syscall::SYSCALLS.len() != syscall::SYSCALL_COUNT
        || syscall::dispatch(syscall::SyscallId::GetAbiVersion as u64, [0; 6]).value != 0x0001_0000
    {
        halt();
    }

    let mut console = Console::new(address, width, height, pitch, pixel_format);
    loop {
        console.poll();
        core::hint::spin_loop();
    }
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

impl Console {
    const LEFT: usize = 24;
    const INPUT_X: usize = Self::LEFT + 7 * 8;
    const TOP: usize = 24;
    const PROMPT_Y: usize = 80;
    const LINE_HEIGHT: usize = 16;
    const BACKGROUND: (u8, u8, u8) = (0x10, 0x14, 0x1C);
    const TEXT: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

    fn new(
        address: *mut u8,
        width: usize,
        height: usize,
        pitch: usize,
        pixel_format: PixelFormat,
    ) -> Self {
        let console = Self {
            address,
            width,
            height,
            pitch,
            pixel_format,
            cursor_y: Self::PROMPT_Y,
            input: [0; 128],
            input_len: 0,
            keyboard: {
                let mut keyboard = keyboard::Keyboard::new();
                keyboard.flush_pending();
                keyboard
            },
        };
        console.draw_header();
        console.draw_prompt();
        console
    }

    fn poll(&mut self) {
        match self.keyboard.poll() {
            Some(keyboard::KeyEvent::Character(character)) => self.type_character(character),
            Some(keyboard::KeyEvent::Backspace) => self.backspace(),
            Some(keyboard::KeyEvent::Enter) => self.submit_line(),
            None => {}
        }
    }

    fn type_character(&mut self, character: u8) {
        let max_chars = self.width.saturating_sub(Self::INPUT_X) / 8;
        if self.input_len >= self.input.len() - 1 || self.input_len >= max_chars {
            return;
        }

        self.input[self.input_len] = character;
        self.input_len += 1;
        draw_text(
            self.address,
            self.width,
            self.height,
            self.pitch,
            self.pixel_format,
            Self::INPUT_X + (self.input_len - 1) * 8,
            self.cursor_y,
            core::str::from_utf8(&self.input[self.input_len - 1..self.input_len]).unwrap_or(" "),
            Self::TEXT.0,
            Self::TEXT.1,
            Self::TEXT.2,
        );
    }

    fn backspace(&mut self) {
        if self.input_len == 0 {
            return;
        }

        self.input_len -= 1;
        self.input[self.input_len] = 0;
        self.fill_rect(
            Self::INPUT_X + self.input_len * 8,
            self.cursor_y,
            8,
            8,
            Self::BACKGROUND,
        );
    }

    fn submit_line(&mut self) {
        self.input_len = 0;
        self.input.fill(0);
        self.cursor_y = self.cursor_y.saturating_add(Self::LINE_HEIGHT);
        if self.cursor_y + 8 >= self.height {
            clear_screen(
                self.address,
                self.width,
                self.height,
                self.pitch,
                self.pixel_format,
            );
            self.cursor_y = Self::PROMPT_Y;
            self.draw_header();
        }
        self.draw_prompt();
    }

    fn draw_header(&self) {
        self.draw_string(
            Self::LEFT,
            Self::TOP,
            "ALPHAOS FREESTANDING KERNEL",
            0x45,
            0xD4,
            0x8B,
        );
        self.draw_string(
            Self::LEFT,
            Self::TOP + 24,
            "Type keys, Backspace edits, Enter submits.",
            0xB0,
            0xB0,
            0xB0,
        );
        self.draw_string(
            Self::LEFT,
            Self::TOP + 48,
            "Syscall ABI 1.0: 20 reserved calls",
            0xB0,
            0xB0,
            0xB0,
        );
    }

    fn draw_prompt(&self) {
        self.draw_string(
            Self::LEFT,
            self.cursor_y,
            "alpha> ",
            Self::TEXT.0,
            Self::TEXT.1,
            Self::TEXT.2,
        );
    }

    fn draw_string(&self, x: usize, y: usize, text: &str, red: u8, green: u8, blue: u8) {
        draw_text(
            self.address,
            self.width,
            self.height,
            self.pitch,
            self.pixel_format,
            x,
            y,
            text,
            red,
            green,
            blue,
        );
    }

    fn fill_rect(&self, x: usize, y: usize, width: usize, height: usize, color: (u8, u8, u8)) {
        for row in y..y.saturating_add(height).min(self.height) {
            for column in x..x.saturating_add(width).min(self.width) {
                write_pixel(
                    self.address,
                    self.width,
                    self.height,
                    self.pitch,
                    self.pixel_format,
                    column,
                    row,
                    color.0,
                    color.1,
                    color.2,
                );
            }
        }
    }
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
