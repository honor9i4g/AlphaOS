#![no_main]
#![no_std]

#[path = "../../kernel/src/font.rs"]
#[allow(dead_code, unsafe_op_in_unsafe_fn)]
mod font;
mod keyboard;

use alphaos_syscall_abi as syscall;
use core::arch::{asm, global_asm};
use limine::BaseRevision;
use limine::framebuffer::MemoryModel;
use limine::memory_map::EntryType;
use limine::request::{
    FramebufferRequest, HhdmRequest, MemoryMapRequest, RequestsEndMarker, RequestsStartMarker,
};

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

const PAGE_SIZE: u64 = 4096;
const USER_CODE_BASE: u64 = 0x0000_4000_0000_0000;
const USER_DATA_BASE: u64 = USER_CODE_BASE + PAGE_SIZE;
const USER_STACK_BASE: u64 = USER_CODE_BASE + 2 * PAGE_SIZE;
const USER_REGION_END: u64 = USER_CODE_BASE + 3 * PAGE_SIZE;
const USER_STACK_TOP: u64 = USER_REGION_END;

#[repr(C, align(16))]
struct KernelSyscallStack([u8; 16 * 1024]);

static mut KERNEL_SYSCALL_STACK: KernelSyscallStack = KernelSyscallStack([0; 16 * 1024]);
#[unsafe(no_mangle)]
static mut SYSCALL_KERNEL_STACK_TOP: u64 = 0;
#[used]
#[unsafe(no_mangle)]
static mut SYSCALL_USER_STACK: u64 = 0;
static mut SYSCALL_CONSOLE: *mut Console = core::ptr::null_mut();
static mut HHDM_OFFSET: u64 = 0;
static mut ALLOCATOR_NEXT: u64 = 0;
static mut ALLOCATOR_END: u64 = 0;

#[repr(C, align(16))]
struct Gdt([u64; 5]);

static GDT: Gdt = Gdt([
    0,
    0x00AF_9B00_0000_FFFF,
    0x00CF_9300_0000_FFFF,
    0x00CF_F300_0000_FFFF,
    0x00AF_FB00_0000_FFFF,
]);

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
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
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

global_asm!(
    ".global load_gdt_and_segments",
    "load_gdt_and_segments:",
    "lgdt [rdi]",
    "push 0x08",
    "lea rax, [rip + 2f]",
    "push rax",
    "retfq",
    "2:",
    "mov ax, 0x10",
    "mov ds, ax",
    "mov es, ax",
    "mov ss, ax",
    "ret",
    ".global enter_user_mode",
    "enter_user_mode:",
    "mov rax, rdi",
    "mov rbx, rsi",
    "push 0x1b",
    "push rbx",
    "push 0x202",
    "push 0x23",
    "push rax",
    "iretq",
    ".global syscall_entry",
    "syscall_entry:",
    "mov qword ptr [rip + SYSCALL_USER_STACK], rsp",
    "mov rsp, qword ptr [rip + SYSCALL_KERNEL_STACK_TOP]",
    "push r11",
    "push rcx",
    "push r9",
    "push r8",
    "push r10",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rax",
    "push qword ptr [rsp + 48]",
    "mov rdi, qword ptr [rsp + 8]",
    "mov rsi, qword ptr [rsp + 16]",
    "mov rdx, qword ptr [rsp + 24]",
    "mov rcx, qword ptr [rsp + 32]",
    "mov r8, qword ptr [rsp + 40]",
    "mov r9, qword ptr [rsp + 48]",
    "call syscall_dispatch",
    "add rsp, 8",
    "mov qword ptr [rsp], rax",
    "mov qword ptr [rsp + 24], rdx",
    "pop rax",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop r10",
    "pop r8",
    "pop r9",
    "pop rcx",
    "pop r11",
    "mov rsp, qword ptr [rip + SYSCALL_USER_STACK]",
    "sysretq",
    ".section .usercode,\"ax\"",
    ".global user_test_start",
    "user_test_start:",
    "mov rax, 19",
    "syscall",
    "mov rax, 16",
    "syscall",
    "2:",
    "mov rax, 2",
    "xor rdi, rdi",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 1",
    "syscall",
    "test rdx, rdx",
    "jnz 2b",
    "mov rax, 1",
    "mov rdi, 1",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 1",
    "syscall",
    "jmp 2b",
    ".global user_test_end",
    "user_test_end:",
);

unsafe extern "C" {
    fn load_gdt_and_segments(table: *const DescriptorTablePointer);
    fn enter_user_mode(instruction_pointer: u64, stack_pointer: u64) -> !;
    fn syscall_entry();
    static user_test_start: u8;
    static user_test_end: u8;
}

#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    if !BASE_REVISION.is_supported() {
        halt();
    }
    if !initialize_page_allocator() {
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
        || syscall::dispatch(syscall::SyscallId::GetAbiVersion as u64, [0; 6]).value != 0x0001_0001
    {
        halt();
    }

    let mut console = Console::new(address, width, height, pitch, pixel_format);
    if !map_user_test_program() {
        halt();
    }

    unsafe {
        core::ptr::addr_of_mut!(SYSCALL_CONSOLE).write_volatile(&mut console);
        let stack_top = core::ptr::addr_of_mut!(KERNEL_SYSCALL_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64;
        core::ptr::addr_of_mut!(SYSCALL_KERNEL_STACK_TOP).write_volatile(stack_top);
        enter_user_mode(USER_CODE_BASE, USER_STACK_TOP);
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

fn initialize_page_allocator() -> bool {
    let Some(hhdm) = HHDM_REQUEST.get_response() else {
        return false;
    };
    let Some(memory_map) = MEMORY_MAP_REQUEST.get_response() else {
        return false;
    };

    let Some(region) = memory_map
        .entries()
        .iter()
        .find(|entry| entry.entry_type == EntryType::USABLE && entry.length >= 8 * 1024 * 1024)
    else {
        return false;
    };

    let Some(region_start) = region.base.checked_add(PAGE_SIZE - 1) else {
        return false;
    };
    let region_start = (region_start & !(PAGE_SIZE - 1)).max(0x10_0000);
    let Some(region_end) = region.base.checked_add(region.length) else {
        return false;
    };
    if region_start >= region_end {
        return false;
    }

    unsafe {
        core::ptr::addr_of_mut!(HHDM_OFFSET).write_volatile(hhdm.offset());
        core::ptr::addr_of_mut!(ALLOCATOR_NEXT).write_volatile(region_start);
        core::ptr::addr_of_mut!(ALLOCATOR_END).write_volatile(region_end);
    }
    true
}

unsafe fn allocate_page() -> Option<(u64, *mut u8)> {
    let next_ptr = core::ptr::addr_of_mut!(ALLOCATOR_NEXT);
    let next = unsafe { next_ptr.read_volatile() };
    let end = unsafe { core::ptr::addr_of!(ALLOCATOR_END).read_volatile() };
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    if next.checked_add(PAGE_SIZE)? > end {
        return None;
    }
    let virtual_address = next.checked_add(hhdm)?;
    unsafe {
        next_ptr.write_volatile(next + PAGE_SIZE);
        core::ptr::write_bytes(virtual_address as *mut u8, 0, PAGE_SIZE as usize);
    }
    Some((next, virtual_address as *mut u8))
}

fn map_user_test_program() -> bool {
    let Some(hhdm) = HHDM_REQUEST
        .get_response()
        .map(|response| response.offset())
    else {
        return false;
    };
    let root_physical = unsafe {
        let value: u64;
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
        value & 0x000F_FFFF_FFFF_F000
    };
    let Some(root_virtual) = root_physical.checked_add(hhdm) else {
        return false;
    };
    let pml4 = root_virtual as *mut u64;
    let pml4_index = ((USER_CODE_BASE >> 39) & 0x1FF) as usize;
    if unsafe { pml4.add(pml4_index).read_volatile() } & 1 != 0 {
        return false;
    }

    let Some((pdpt_physical, pdpt)) = (unsafe { allocate_page() }) else {
        return false;
    };
    let Some((pd_physical, pd)) = (unsafe { allocate_page() }) else {
        return false;
    };
    let Some((pt_physical, pt)) = (unsafe { allocate_page() }) else {
        return false;
    };
    let Some((code_physical, code)) = (unsafe { allocate_page() }) else {
        return false;
    };
    let Some((data_physical, _data)) = (unsafe { allocate_page() }) else {
        return false;
    };
    let Some((stack_physical, _stack)) = (unsafe { allocate_page() }) else {
        return false;
    };

    let code_length =
        core::ptr::addr_of!(user_test_end) as usize - core::ptr::addr_of!(user_test_start) as usize;
    if code_length == 0 || code_length > PAGE_SIZE as usize {
        return false;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(core::ptr::addr_of!(user_test_start), code, code_length);
        pml4.add(pml4_index).write_volatile(pdpt_physical | 0x7);
        (pdpt as *mut u64).write_volatile(pd_physical | 0x7);
        (pd as *mut u64).write_volatile(pt_physical | 0x7);
        let page_table = pt as *mut u64;
        page_table
            .add(((USER_CODE_BASE >> 12) & 0x1FF) as usize)
            .write_volatile(code_physical | 0x7);
        page_table
            .add(((USER_DATA_BASE >> 12) & 0x1FF) as usize)
            .write_volatile(data_physical | 0x7);
        page_table
            .add(((USER_STACK_BASE >> 12) & 0x1FF) as usize)
            .write_volatile(stack_physical | 0x7);
        asm!("mov cr3, {}", in(reg) root_physical, options(nostack, preserves_flags));
    }

    configure_syscall_entry();
    true
}

fn configure_syscall_entry() {
    let pointer = DescriptorTablePointer {
        limit: (core::mem::size_of::<Gdt>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    unsafe {
        load_gdt_and_segments(&pointer);
        let efer = read_msr(0xC000_0080) | 1;
        write_msr(0xC000_0080, efer);
        write_msr(0xC000_0081, (0x10_u64 << 48) | (0x08_u64 << 32));
        write_msr(0xC000_0082, syscall_entry as *const () as usize as u64);
        write_msr(0xC000_0084, 0x700);
    }
}

unsafe fn read_msr(register: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") register,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn write_msr(register: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") register,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn syscall_dispatch(
    number: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
) -> syscall::SyscallResult {
    let result = match syscall::syscall_info(number).map(|info| info.id) {
        Some(syscall::SyscallId::GetAbiVersion) => syscall::SyscallResult {
            value: (u64::from(syscall::SYSCALL_ABI_MAJOR) << 16)
                | u64::from(syscall::SYSCALL_ABI_MINOR),
            error: 0,
        },
        Some(syscall::SyscallId::GetPid) => syscall::SyscallResult { value: 1, error: 0 },
        Some(syscall::SyscallId::Read) => read_console(arg0, arg1, arg2),
        Some(syscall::SyscallId::Write) => write_console(arg0, arg1, arg2),
        _ => syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::ENOSYS,
        },
    };
    result
}

fn read_console(fd: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    if fd != 0 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 9,
        };
    }
    if length == 0 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 22,
        };
    }
    if !valid_user_range(buffer, 1) {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 14,
        };
    }

    loop {
        let console = unsafe { core::ptr::addr_of!(SYSCALL_CONSOLE).read_volatile() };
        if console.is_null() {
            return syscall::SyscallResult {
                value: u64::MAX,
                error: 5,
            };
        }
        let event = unsafe { (*console).keyboard.poll() };
        let character = match event {
            Some(keyboard::KeyEvent::Character(character)) => character,
            Some(keyboard::KeyEvent::Enter) => b'\n',
            Some(keyboard::KeyEvent::Backspace) => 8,
            None => {
                core::hint::spin_loop();
                continue;
            }
        };
        unsafe {
            (buffer as *mut u8).write_volatile(character);
        }
        return syscall::SyscallResult { value: 1, error: 0 };
    }
}

fn write_console(fd: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    if fd != 1 && fd != 2 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 9,
        };
    }
    let Ok(length) = usize::try_from(length) else {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 14,
        };
    };
    if !valid_user_range(buffer, length) {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 14,
        };
    }
    let console = unsafe { core::ptr::addr_of!(SYSCALL_CONSOLE).read_volatile() };
    if console.is_null() {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: 5,
        };
    }

    for offset in 0..length {
        let character = unsafe { (buffer as *const u8).add(offset).read_volatile() };
        unsafe {
            match character {
                b'\n' | b'\r' => (*console).submit_line(),
                8 => (*console).backspace(),
                0x20..=0x7E => (*console).type_character(character),
                _ => {}
            }
        }
    }
    syscall::SyscallResult {
        value: length as u64,
        error: 0,
    }
}

fn valid_user_range(address: u64, length: usize) -> bool {
    let Ok(length) = u64::try_from(length) else {
        return false;
    };
    let Some(end) = address.checked_add(length) else {
        return false;
    };
    address >= USER_CODE_BASE && end <= USER_REGION_END
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
            "User syscalls: ABI, PID, console read/write",
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
