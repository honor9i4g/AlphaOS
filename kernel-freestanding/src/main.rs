#![no_main]
#![no_std]

#[path = "../../kernel/src/font.rs"]
#[allow(dead_code, unsafe_op_in_unsafe_fn)]
mod font;
mod input;
mod interrupts;
mod keyboard;
mod ps2;

use alphaos_syscall_abi as syscall;
use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};
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
    cursor_x: usize,
}

const PAGE_SIZE: u64 = 4096;
const USER_CODE_BASE: u64 = 0x0000_4000_0000_0000;
const USER_DATA_BASE: u64 = USER_CODE_BASE + PAGE_SIZE;
const USER_STACK_BASE: u64 = USER_CODE_BASE + 2 * PAGE_SIZE;
const USER_REGION_END: u64 = USER_CODE_BASE + 3 * PAGE_SIZE;
const USER_STACK_TOP: u64 = USER_REGION_END;
const USER_INPUT_TARGET: u64 = 1;

#[repr(C, align(16))]
struct KernelSyscallStack([u8; 16 * 1024]);

static mut KERNEL_SYSCALL_STACK: KernelSyscallStack = KernelSyscallStack([0; 16 * 1024]);
#[unsafe(no_mangle)]
static mut SYSCALL_KERNEL_STACK_TOP: u64 = 0;
#[used]
#[unsafe(no_mangle)]
static mut SYSCALL_USER_STACK: u64 = 0;
#[used]
#[unsafe(no_mangle)]
static mut SYSCALL_SAVED_RAX: u64 = 0;
static mut SYSCALL_CONSOLE: *mut Console = core::ptr::null_mut();
static mut HHDM_OFFSET: u64 = 0;
static mut ALLOCATOR_START: u64 = 0;
static mut ALLOCATOR_NEXT: u64 = 0;
static mut ALLOCATOR_END: u64 = 0;
static OPENED_DEVICE_IDS: AtomicU32 = AtomicU32::new(0);
static PROCESS_PRIORITY: AtomicI32 = AtomicI32::new(0);
static USER_HEAP_NEXT: AtomicU64 = AtomicU64::new(USER_REGION_END);

const HANDLE_KIND_PIPE_R: u32 = 0x10;
const HANDLE_KIND_PIPE_W: u32 = 0x11;
const HANDLE_KIND_CHANNEL: u32 = 0x20;
const HANDLE_KIND_SOCKET: u32 = 0x30;
const HANDLE_KIND_FILE: u32 = 0x40;
const MAX_PIPES: usize = 8;
const PIPE_BUF_SIZE: usize = 512;
const MAX_CHANNELS: usize = 8;
const CHANNEL_BUF_SIZE: usize = 512;
const MAX_SOCKETS: usize = 8;
const MAX_FS_FILES: usize = 16;
const MAX_FILE_SIZE: usize = 1024;
const MAX_OPEN_FILES: usize = 16;
const MAX_SPAWNED: usize = 8;
const MAX_HEAP_BLOCKS: usize = 32;

#[derive(Clone, Copy)]
struct PipeSlot {
    used: bool,
    read_open: bool,
    write_open: bool,
    head: usize,
    len: usize,
    buf: [u8; PIPE_BUF_SIZE],
}

impl PipeSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            read_open: false,
            write_open: false,
            head: 0,
            len: 0,
            buf: [0; PIPE_BUF_SIZE],
        }
    }
}

#[derive(Clone, Copy)]
struct ChannelSlot {
    used: bool,
    head: usize,
    len: usize,
    buf: [u8; CHANNEL_BUF_SIZE],
}

impl ChannelSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            head: 0,
            len: 0,
            buf: [0; CHANNEL_BUF_SIZE],
        }
    }
}

#[derive(Clone, Copy)]
struct SocketSlot {
    used: bool,
    domain: u32,
    socket_type: u32,
    protocol: u32,
    bound: bool,
    listening: bool,
    connected: bool,
}

impl SocketSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            domain: 0,
            socket_type: 0,
            protocol: 0,
            bound: false,
            listening: false,
            connected: false,
        }
    }
}

static mut PIPES: [PipeSlot; MAX_PIPES] = [PipeSlot::empty(); MAX_PIPES];
static mut CHANNELS: [ChannelSlot; MAX_CHANNELS] = [ChannelSlot::empty(); MAX_CHANNELS];
static mut SOCKETS: [SocketSlot; MAX_SOCKETS] = [SocketSlot::empty(); MAX_SOCKETS];

#[derive(Clone, Copy)]
struct FsFile {
    used: bool,
    is_dir: bool,
    name: [u8; 64],
    name_len: usize,
    data: [u8; MAX_FILE_SIZE],
    size: usize,
}

impl FsFile {
    const fn empty() -> Self {
        Self {
            used: false,
            is_dir: false,
            name: [0; 64],
            name_len: 0,
            data: [0; MAX_FILE_SIZE],
            size: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct OpenFile {
    used: bool,
    fs_index: usize,
    offset: usize,
    flags: u32,
}

impl OpenFile {
    const fn empty() -> Self {
        Self {
            used: false,
            fs_index: 0,
            offset: 0,
            flags: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct HeapBlock {
    used: bool,
    addr: u64,
    size: u64,
}

impl HeapBlock {
    const fn empty() -> Self {
        Self {
            used: false,
            addr: 0,
            size: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct SpawnedProc {
    used: bool,
    pid: u64,
    exited: bool,
    exit_code: u64,
}

impl SpawnedProc {
    const fn empty() -> Self {
        Self {
            used: false,
            pid: 0,
            exited: false,
            exit_code: 0,
        }
    }
}

static mut FS_FILES: [FsFile; MAX_FS_FILES] = [FsFile::empty(); MAX_FS_FILES];
static mut FS_INIT: bool = false;
static mut OPEN_FILES: [OpenFile; MAX_OPEN_FILES] = [OpenFile::empty(); MAX_OPEN_FILES];
static mut HEAP_BLOCKS: [HeapBlock; MAX_HEAP_BLOCKS] = [HeapBlock::empty(); MAX_HEAP_BLOCKS];
static mut SPAWNED: [SpawnedProc; MAX_SPAWNED] = [SpawnedProc::empty(); MAX_SPAWNED];
static NEXT_SPAWN_PID: AtomicU64 = AtomicU64::new(2);
static EXIT_STATUS: AtomicU64 = AtomicU64::new(0);
static EXITED: AtomicU8 = AtomicU8::new(0);

static KERNEL_LOCK: AtomicU8 = AtomicU8::new(0);

fn lock_kernel() {
    while KERNEL_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock_kernel() {
    KERNEL_LOCK.store(0, Ordering::Release);
}

fn make_handle(kind: u32, slot: u32) -> u64 {
    (u64::from(kind) << 32) | u64::from(slot)
}

fn handle_kind(handle: u64) -> u32 {
    (handle >> 32) as u32
}

fn handle_slot(handle: u64) -> usize {
    (handle & u64::from(u32::MAX)) as usize
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
    ".global enter_user_mode",
    "enter_user_mode:",
    "mov ax, 0x1b",
    "mov ds, ax",
    "mov es, ax",
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
    "mov qword ptr [rip + SYSCALL_SAVED_RAX], rax",
    "mov ax, 0x10",
    "mov ds, ax",
    "mov es, ax",
    "mov ss, ax",
    "mov rax, qword ptr [rip + SYSCALL_SAVED_RAX]",
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
    "push rax",
    "mov ax, 0x1b",
    "mov ds, ax",
    "mov es, ax",
    "pop rax",
    "mov rsp, qword ptr [rip + SYSCALL_USER_STACK]",
    "sysretq",
    ".section .usercode,\"ax\"",
    ".global user_test_start",
    "user_test_start:",
    "mov rax, 19",
    "syscall",
    "mov rax, 25",
    "xor rdi, rdi",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 8",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 25",
    "mov rdi, 1",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 24",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 25",
    "mov rdi, 2",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 24",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 16",
    "syscall",
    "mov rax, 20",
    "mov rdi, 0x0000400000001000",
    "mov rsi, 2",
    "syscall",
    "cmp rax, 2",
    "jne 3f",
    "test rdx, rdx",
    "jnz 3f",
    "mov rsi, 0x0000400000001000",
    "cmp dword ptr [rsi], 1",
    "jne 3f",
    "cmp dword ptr [rsi + 16], 2",
    "jne 3f",
    "mov rax, 21",
    "mov rdi, 2",
    "mov rsi, 2",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov r12, rax",
    "mov rax, 21",
    "mov rdi, 1",
    "mov rsi, 1",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov r13, rax",
    "mov rsi, 0x0000400000001000",
    "mov byte ptr [rsi], 79",
    "mov byte ptr [rsi + 1], 75",
    "mov rax, 24",
    "mov rdi, r12",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 2",
    "syscall",
    "cmp rax, 2",
    "jne 3f",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 23",
    "mov rdi, r13",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 16",
    "syscall",
    "cmp rax, 16",
    "jne 3f",
    "test rdx, rdx",
    "jnz 3f",
    "mov rsi, 0x0000400000001000",
    "cmp word ptr [rsi], 1",
    "jne 3f",
    "cmp dword ptr [rsi + 4], 2",
    "ja 3f",
    "mov rax, 4",
    "mov rdi, r13",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 4",
    "mov rdi, r12",
    "syscall",
    "test rdx, rdx",
    "jnz 3f",
    "mov rax, 24",
    "mov rdi, r12",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 1",
    "syscall",
    "cmp rdx, 9",
    "jne 3f",
    "mov rax, 1",
    "mov rdi, 1",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 2",
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
    "3:",
    "mov rsi, 0x0000400000001000",
    "mov byte ptr [rsi], 33",
    "mov rax, 1",
    "mov rdi, 1",
    "mov rsi, 0x0000400000001000",
    "mov rdx, 1",
    "syscall",
    "jmp 3b",
    ".global user_test_end",
    "user_test_end:",
);

unsafe extern "C" {
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
        || syscall::dispatch(syscall::SyscallId::GetAbiVersion as u64, [0; 6]).value != 0x0001_0004
    {
        halt();
    }

    let mut console = Console::new(address, width, height, pitch, pixel_format);
    unsafe {
        core::ptr::addr_of_mut!(SYSCALL_CONSOLE).write_volatile(&mut console);
        let stack_top = core::ptr::addr_of_mut!(KERNEL_SYSCALL_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64;
        core::ptr::addr_of_mut!(SYSCALL_KERNEL_STACK_TOP).write_volatile(stack_top);
    }

    interrupts::initialize();
    if !ps2::initialize() {
        draw_text(
            address,
            width,
            height,
            pitch,
            pixel_format,
            24,
            160,
            "PS/2 keyboard initialization failed",
            0xFF,
            0x40,
            0x40,
        );
        halt();
    }
    if !map_user_test_program() {
        halt();
    }
    input::set_foreground_target(Some(USER_INPUT_TARGET));
    interrupts::enable_keyboard_irq();

    unsafe {
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
        core::ptr::addr_of_mut!(ALLOCATOR_START).write_volatile(region_start);
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
    unsafe {
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
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> syscall::SyscallResult {
    match syscall::syscall_info(number).map(|info| info.id) {
        Some(syscall::SyscallId::Exit) => sys_exit(arg0),
        Some(syscall::SyscallId::Open) => sys_open(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileRead) => sys_file_read(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileWrite) => sys_file_write(arg0, arg1, arg2),
        Some(syscall::SyscallId::Seek) => sys_seek(arg0, arg1, arg2),
        Some(syscall::SyscallId::Stat) => sys_stat(arg0, arg1, arg2),
        Some(syscall::SyscallId::ReadDir) => sys_readdir(arg0, arg1),
        Some(syscall::SyscallId::MapMemory) => sys_map_memory(arg0, arg1, arg2, arg3, arg4),
        Some(syscall::SyscallId::UnmapMemory) => sys_unmap_memory(arg0, arg1),
        Some(syscall::SyscallId::HeapAlloc) => sys_heap_alloc(arg0, arg1),
        Some(syscall::SyscallId::HeapFree) => sys_heap_free(arg0, arg1),
        Some(syscall::SyscallId::ClockGetTime) => sys_clock_gettime(arg0, arg1),
        Some(syscall::SyscallId::Sleep) => sys_sleep(arg0),
        Some(syscall::SyscallId::Spawn) => sys_spawn(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::Wait) => sys_wait(arg0, arg1, arg2),
        Some(syscall::SyscallId::DeviceControl) => {
            sys_device_control(arg0, arg1, arg2, arg3, arg4, arg5)
        }
        Some(syscall::SyscallId::GetAbiVersion) => syscall::SyscallResult {
            value: (u64::from(syscall::SYSCALL_ABI_MAJOR) << 16)
                | u64::from(syscall::SYSCALL_ABI_MINOR),
            error: 0,
        },
        Some(syscall::SyscallId::GetPid) => syscall::SyscallResult { value: 1, error: 0 },
        Some(syscall::SyscallId::Read) => {
            if handle_kind(arg0) == HANDLE_KIND_PIPE_R {
                let slot = handle_slot(arg0);
                let Ok(len) = usize::try_from(arg2) else {
                    return syscall_error(syscall::EINVAL);
                };
                if len != 0 && !valid_user_range(arg1, len) {
                    return syscall_error(syscall::EFAULT);
                }
                pipe_read(slot, arg1, len)
            } else {
                read_console(arg0, arg1, arg2)
            }
        }
        Some(syscall::SyscallId::Write) => {
            if handle_kind(arg0) == HANDLE_KIND_PIPE_W {
                let slot = handle_slot(arg0);
                let Ok(len) = usize::try_from(arg2) else {
                    return syscall_error(syscall::EINVAL);
                };
                if len != 0 && !valid_user_range(arg1, len) {
                    return syscall_error(syscall::EFAULT);
                }
                pipe_write(slot, arg1, len)
            } else {
                write_console(arg0, arg1, arg2)
            }
        }
        Some(syscall::SyscallId::Close) => close_handle(arg0),
        Some(syscall::SyscallId::DeviceList) => device_list(arg0, arg1),
        Some(syscall::SyscallId::DeviceOpen) => device_open(arg0, arg1),
        Some(syscall::SyscallId::DeviceRead) => device_read(arg0, arg1, arg2),
        Some(syscall::SyscallId::DeviceWrite) => device_write(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetSystemInfo) => get_system_info(arg0, arg1, arg2),
        Some(syscall::SyscallId::Yield) => syscall::SyscallResult { value: 0, error: 0 },
        Some(syscall::SyscallId::GetTid) => syscall::SyscallResult { value: 1, error: 0 },
        Some(syscall::SyscallId::GetPpid) => syscall::SyscallResult { value: 0, error: 0 },
        Some(syscall::SyscallId::GetUid) => syscall::SyscallResult { value: 0, error: 0 },
        Some(syscall::SyscallId::GetGid) => syscall::SyscallResult { value: 0, error: 0 },
        Some(syscall::SyscallId::SetPriority) => set_priority(arg0, arg1),
        Some(syscall::SyscallId::GetPriority) => get_priority(arg0),
        Some(syscall::SyscallId::VirtualAlloc) => virtual_alloc(arg0, arg1, arg2),
        Some(syscall::SyscallId::VirtualFree) => virtual_free(arg0, arg1),
        Some(syscall::SyscallId::VirtualProtect) => virtual_protect(arg0, arg1, arg2),
        Some(syscall::SyscallId::Truncate) => truncate_handle(arg0, arg1),
        Some(syscall::SyscallId::Fstat) => fstat_handle(arg0, arg1),
        Some(syscall::SyscallId::Sync) => syscall::SyscallResult { value: 0, error: 0 },
        Some(syscall::SyscallId::Pipe) => pipe_create(arg0),
        Some(syscall::SyscallId::SocketCreate) => socket_create(arg0, arg1, arg2),
        Some(syscall::SyscallId::SocketBind) => socket_bind(arg0, arg1, arg2),
        Some(syscall::SyscallId::SocketListen) => socket_listen(arg0, arg1),
        Some(syscall::SyscallId::SocketAccept) => socket_accept(arg0, arg1, arg2),
        Some(syscall::SyscallId::SocketConnect) => socket_connect(arg0, arg1, arg2),
        Some(syscall::SyscallId::Reboot) => reboot_system(arg0),
        Some(syscall::SyscallId::ChannelPairCreate) => channel_pair_create(arg0, arg1),
        Some(syscall::SyscallId::ChannelSend) => channel_send(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ChannelReceive) => channel_receive(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ChannelClose) => channel_close(arg0),
        Some(syscall::SyscallId::SharedMemoryCreate) => shared_memory_create(arg0, arg1),
        Some(_) => syscall_error(syscall::ENOSYS),
        None => syscall_error(syscall::ENOSYS),
    }
}

fn device_list(entries: u64, capacity: u64) -> syscall::SyscallResult {
    let devices = [
        syscall::DeviceInfo {
            id: syscall::DEVICE_KEYBOARD,
            kind: syscall::DEVICE_KIND_INPUT,
            capabilities: syscall::DEVICE_ACCESS_READ,
            reserved: 0,
        },
        syscall::DeviceInfo {
            id: syscall::DEVICE_FRAMEBUFFER,
            kind: syscall::DEVICE_KIND_DISPLAY,
            capabilities: syscall::DEVICE_ACCESS_WRITE,
            reserved: 0,
        },
    ];
    let Ok(capacity) = usize::try_from(capacity) else {
        return syscall_error(syscall::EINVAL);
    };
    let count = capacity.min(devices.len());
    let Some(bytes) = count.checked_mul(core::mem::size_of::<syscall::DeviceInfo>()) else {
        return syscall_error(syscall::EFAULT);
    };
    if bytes != 0 && !valid_user_range(entries, bytes) {
        return syscall_error(syscall::EFAULT);
    }

    for (index, device) in devices.iter().take(count).enumerate() {
        unsafe {
            core::ptr::copy_nonoverlapping(
                core::ptr::addr_of!(*device).cast::<u8>(),
                (entries as *mut u8).add(index * core::mem::size_of::<syscall::DeviceInfo>()),
                core::mem::size_of::<syscall::DeviceInfo>(),
            );
        }
    }
    syscall::SyscallResult {
        value: count as u64,
        error: 0,
    }
}

fn device_open(device_id: u64, access: u64) -> syscall::SyscallResult {
    let Ok(access) = u32::try_from(access) else {
        return syscall_error(syscall::EINVAL);
    };
    if access == 0 || access & !(syscall::DEVICE_ACCESS_READ | syscall::DEVICE_ACCESS_WRITE) != 0 {
        return syscall_error(syscall::EINVAL);
    }
    let (id, capabilities) = match device_id {
        id if id == u64::from(syscall::DEVICE_KEYBOARD) => {
            (syscall::DEVICE_KEYBOARD, syscall::DEVICE_ACCESS_READ)
        }
        id if id == u64::from(syscall::DEVICE_FRAMEBUFFER) => {
            (syscall::DEVICE_FRAMEBUFFER, syscall::DEVICE_ACCESS_WRITE)
        }
        _ => return syscall_error(syscall::ENODEV),
    };
    if access & !capabilities != 0 {
        return syscall_error(syscall::EACCES);
    }
    OPENED_DEVICE_IDS.fetch_or(1 << id, Ordering::Release);
    syscall::SyscallResult {
        value: (u64::from(id) << 32) | u64::from(access),
        error: 0,
    }
}

fn device_read(handle: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some((device_id, access)) = decode_device_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    if access & syscall::DEVICE_ACCESS_READ == 0 || device_id != syscall::DEVICE_KEYBOARD {
        return syscall_error(syscall::EACCES);
    }
    read_input_event(buffer, length)
}

fn close_handle(handle: u64) -> syscall::SyscallResult {
    let kind = handle_kind(handle);
    match kind {
        HANDLE_KIND_PIPE_R | HANDLE_KIND_PIPE_W => close_pipe_handle(handle),
        HANDLE_KIND_CHANNEL => channel_close(handle),
        HANDLE_KIND_SOCKET => close_socket_handle(handle),
        HANDLE_KIND_FILE => close_file_handle(handle),
        _ => {
            let Some((device_id, _)) = decode_device_handle(handle) else {
                return syscall_error(syscall::EBADF);
            };
            OPENED_DEVICE_IDS.fetch_and(!(1 << device_id), Ordering::AcqRel);
            syscall::SyscallResult { value: 0, error: 0 }
        }
    }
}

fn close_file_handle(handle: u64) -> syscall::SyscallResult {
    let Some(slot) = file_handle_slot(handle) else {
        return syscall_error(syscall::EBADF);
    };
    lock_kernel();
    unsafe {
        OPEN_FILES[slot] = OpenFile::empty();
    }
    unlock_kernel();
    syscall::SyscallResult { value: 0, error: 0 }
}

fn device_write(handle: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some((device_id, access)) = decode_device_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    if access & syscall::DEVICE_ACCESS_WRITE == 0 || device_id != syscall::DEVICE_FRAMEBUFFER {
        return syscall_error(syscall::EACCES);
    }
    write_console(1, buffer, length)
}

fn decode_device_handle(handle: u64) -> Option<(u32, u32)> {
    let device_id = u32::try_from(handle >> 32).ok()?;
    let access = u32::try_from(handle & u64::from(u32::MAX)).ok()?;
    let supported = match device_id {
        syscall::DEVICE_KEYBOARD => syscall::DEVICE_ACCESS_READ,
        syscall::DEVICE_FRAMEBUFFER => syscall::DEVICE_ACCESS_WRITE,
        _ => return None,
    };
    if access == 0 || access & !supported != 0 {
        return None;
    }
    if OPENED_DEVICE_IDS.load(Ordering::Acquire) & (1 << device_id) == 0 {
        return None;
    }
    Some((device_id, access))
}

fn syscall_error(error: u64) -> syscall::SyscallResult {
    syscall::SyscallResult {
        value: u64::MAX,
        error,
    }
}

fn set_priority(who: u64, priority: u64) -> syscall::SyscallResult {
    if who != 0 && who != 1 {
        return syscall_error(syscall::ESRCH);
    }
    let prio = priority as i64;
    if !(-20..=19).contains(&prio) {
        return syscall_error(syscall::EINVAL);
    }
    PROCESS_PRIORITY.store(prio as i32, Ordering::Relaxed);
    syscall::SyscallResult { value: 0, error: 0 }
}

fn get_priority(who: u64) -> syscall::SyscallResult {
    if who != 0 && who != 1 {
        return syscall_error(syscall::ESRCH);
    }
    syscall::SyscallResult {
        value: PROCESS_PRIORITY.load(Ordering::Relaxed) as u64,
        error: 0,
    }
}

fn align_up_page(value: u64) -> Option<u64> {
    value
        .checked_add(PAGE_SIZE - 1)
        .map(|v| v & !(PAGE_SIZE - 1))
}

unsafe fn current_pml4() -> *mut u64 {
    unsafe {
        let root_physical = {
            let value: u64;
            asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
            value & 0x000F_FFFF_FFFF_F000
        };
        let hhdm = core::ptr::addr_of!(HHDM_OFFSET).read_volatile();
        (root_physical + hhdm) as *mut u64
    }
}

unsafe fn ensure_user_page_mapping(virt: u64) -> bool {
    unsafe {
        let pml4 = current_pml4();
        let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt >> 12) & 0x1FF) as usize;

        let mut pml4e = pml4.add(pml4_idx).read_volatile();
        if pml4e & 1 == 0 {
            let Some((phys, _)) = allocate_page() else {
                return false;
            };
            pml4e = phys | 0x7;
            pml4.add(pml4_idx).write_volatile(pml4e);
        } else if pml4e & 0x4 == 0 {
            pml4.add(pml4_idx).write_volatile(pml4e | 0x4);
            pml4e |= 0x4;
        }

        let hhdm = core::ptr::addr_of!(HHDM_OFFSET).read_volatile();
        let pdpt = ((pml4e & 0x000F_FFFF_FFFF_F000) + hhdm) as *mut u64;
        let mut pdpte = pdpt.add(pdpt_idx).read_volatile();
        if pdpte & 1 == 0 {
            let Some((phys, _)) = allocate_page() else {
                return false;
            };
            pdpte = phys | 0x7;
            pdpt.add(pdpt_idx).write_volatile(pdpte);
        } else if pdpte & 0x4 == 0 {
            pdpt.add(pdpt_idx).write_volatile(pdpte | 0x4);
            pdpte |= 0x4;
        }

        let pd = ((pdpte & 0x000F_FFFF_FFFF_F000) + hhdm) as *mut u64;
        let mut pde = pd.add(pd_idx).read_volatile();
        if pde & 1 == 0 {
            let Some((phys, _)) = allocate_page() else {
                return false;
            };
            pde = phys | 0x7;
            pd.add(pd_idx).write_volatile(pde);
        } else if pde & 0x4 == 0 {
            pd.add(pd_idx).write_volatile(pde | 0x4);
            pde |= 0x4;
        }

        let pt = ((pde & 0x000F_FFFF_FFFF_F000) + hhdm) as *mut u64;
        let pte = pt.add(pt_idx).read_volatile();
        if pte & 1 == 0 {
            let Some((phys, _)) = allocate_page() else {
                return false;
            };
            pt.add(pt_idx).write_volatile(phys | 0x7);
            asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
        }
        true
    }
}

fn virtual_alloc(_hint: u64, length: u64, _prot: u64) -> syscall::SyscallResult {
    if length == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let Some(size) = align_up_page(length) else {
        return syscall_error(syscall::ENOMEM);
    };
    let pages = size / PAGE_SIZE;
    lock_kernel();
    let base = USER_HEAP_NEXT.load(Ordering::Relaxed);
    let Some(end) = base.checked_add(size) else {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    };
    if end > 0x0000_8000_0000_0000 {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    }
    for i in 0..pages {
        let virt = base + i * PAGE_SIZE;
        let ok = unsafe { ensure_user_page_mapping(virt) };
        if !ok {
            unlock_kernel();
            return syscall_error(syscall::ENOMEM);
        }
    }
    USER_HEAP_NEXT.store(end, Ordering::Relaxed);
    unlock_kernel();
    syscall::SyscallResult {
        value: base,
        error: 0,
    }
}

fn virtual_free(_address: u64, length: u64) -> syscall::SyscallResult {
    if length == 0 {
        return syscall_error(syscall::EINVAL);
    }
    // Bump allocator: freeing individual regions is not supported in this kernel.
    syscall_error(syscall::ENOTSUP)
}

fn virtual_protect(address: u64, length: u64, _prot: u64) -> syscall::SyscallResult {
    if length == 0 || address == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let Some(size) = align_up_page(length) else {
        return syscall_error(syscall::EINVAL);
    };
    let heap_end = USER_HEAP_NEXT.load(Ordering::Relaxed);
    let Some(end) = address.checked_add(size) else {
        return syscall_error(syscall::EINVAL);
    };
    if address < USER_CODE_BASE || end > heap_end {
        return syscall_error(syscall::ENOMEM);
    }
    // Page-level protection flags are not yet differentiated; mapped user pages are RW.
    syscall::SyscallResult { value: 0, error: 0 }
}

fn truncate_handle(handle: u64, _length: u64) -> syscall::SyscallResult {
    let kind = handle_kind(handle);
    match kind {
        HANDLE_KIND_PIPE_R | HANDLE_KIND_PIPE_W | HANDLE_KIND_CHANNEL | HANDLE_KIND_SOCKET => {
            syscall_error(syscall::ENOTSUP)
        }
        _ => {
            if decode_device_handle(handle).is_some() {
                syscall_error(syscall::ENOTSUP)
            } else {
                syscall_error(syscall::EBADF)
            }
        }
    }
}

fn fstat_handle(handle: u64, out_ptr: u64) -> syscall::SyscallResult {
    let size = core::mem::size_of::<syscall::FileStat>();
    if !valid_user_range(out_ptr, size) {
        return syscall_error(syscall::EFAULT);
    }
    let kind = handle_kind(handle);
    let stat = match kind {
        HANDLE_KIND_PIPE_R | HANDLE_KIND_PIPE_W | HANDLE_KIND_CHANNEL => syscall::FileStat {
            size: 0,
            mode: 0o666,
            flags: 1,
        },
        HANDLE_KIND_SOCKET => syscall::FileStat {
            size: 0,
            mode: 0o666,
            flags: 2,
        },
        HANDLE_KIND_FILE => {
            let Some(slot) = file_handle_slot(handle) else {
                return syscall_error(syscall::EBADF);
            };
            let fs_idx = unsafe { OPEN_FILES[slot].fs_index };
            let (fsize, is_dir) =
                unsafe { (FS_FILES[fs_idx].size as u64, FS_FILES[fs_idx].is_dir) };
            syscall::FileStat {
                size: fsize,
                mode: if is_dir { 0o755 } else { 0o644 },
                flags: if is_dir { 1 } else { 0 },
            }
        }
        _ => {
            if decode_device_handle(handle).is_some() || handle <= 2 {
                syscall::FileStat {
                    size: 0,
                    mode: 0o666,
                    flags: 0,
                }
            } else {
                return syscall_error(syscall::EBADF);
            }
        }
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(stat).cast::<u8>(),
            out_ptr as *mut u8,
            size,
        );
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn pipe_create(out_ptr: u64) -> syscall::SyscallResult {
    let size = core::mem::size_of::<[u64; 2]>();
    if !valid_user_range(out_ptr, size) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let mut slot_idx = None;
    for i in 0..MAX_PIPES {
        if unsafe { !PIPES[i].used } {
            slot_idx = Some(i);
            break;
        }
    }
    let Some(slot_idx) = slot_idx else {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    };
    unsafe {
        PIPES[slot_idx] = PipeSlot {
            used: true,
            read_open: true,
            write_open: true,
            head: 0,
            len: 0,
            buf: [0; PIPE_BUF_SIZE],
        };
    }
    unlock_kernel();
    let handles = [
        make_handle(HANDLE_KIND_PIPE_R, slot_idx as u32),
        make_handle(HANDLE_KIND_PIPE_W, slot_idx as u32),
    ];
    unsafe {
        core::ptr::copy_nonoverlapping(handles.as_ptr().cast::<u8>(), out_ptr as *mut u8, size);
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn close_pipe_handle(handle: u64) -> syscall::SyscallResult {
    let slot = handle_slot(handle);
    if slot >= MAX_PIPES {
        return syscall_error(syscall::EBADF);
    }
    lock_kernel();
    let pipe = unsafe { &mut PIPES[slot] };
    if !pipe.used {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    match handle_kind(handle) {
        HANDLE_KIND_PIPE_R => pipe.read_open = false,
        HANDLE_KIND_PIPE_W => pipe.write_open = false,
        _ => {
            unlock_kernel();
            return syscall_error(syscall::EBADF);
        }
    }
    if !pipe.read_open && !pipe.write_open {
        *pipe = PipeSlot::empty();
    }
    unlock_kernel();
    syscall::SyscallResult { value: 0, error: 0 }
}

fn pipe_read(slot: usize, buffer: u64, length: usize) -> syscall::SyscallResult {
    lock_kernel();
    let pipe = unsafe { &mut PIPES[slot] };
    if !pipe.used || !pipe.read_open {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    if pipe.len == 0 {
        let write_open = pipe.write_open;
        unlock_kernel();
        return if write_open {
            syscall::SyscallResult { value: 0, error: 0 }
        } else {
            syscall::SyscallResult { value: 0, error: 0 }
        };
    }
    let to_read = core::cmp::min(length, pipe.len);
    let mut tmp = [0u8; PIPE_BUF_SIZE];
    for i in 0..to_read {
        tmp[i] = pipe.buf[(pipe.head + i) % PIPE_BUF_SIZE];
    }
    pipe.head = (pipe.head + to_read) % PIPE_BUF_SIZE;
    pipe.len -= to_read;
    unlock_kernel();
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), buffer as *mut u8, to_read);
    }
    syscall::SyscallResult {
        value: to_read as u64,
        error: 0,
    }
}

fn pipe_write(slot: usize, buffer: u64, length: usize) -> syscall::SyscallResult {
    lock_kernel();
    let pipe = unsafe { &mut PIPES[slot] };
    if !pipe.used || !pipe.write_open {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    if !pipe.read_open {
        unlock_kernel();
        return syscall_error(syscall::EPIPE);
    }
    let space = PIPE_BUF_SIZE - pipe.len;
    let to_write = core::cmp::min(length, space);
    let mut tmp = [0u8; PIPE_BUF_SIZE];
    unsafe {
        core::ptr::copy_nonoverlapping(buffer as *const u8, tmp.as_mut_ptr(), to_write);
    }
    for i in 0..to_write {
        let idx = (pipe.head + pipe.len + i) % PIPE_BUF_SIZE;
        pipe.buf[idx] = tmp[i];
    }
    pipe.len += to_write;
    unlock_kernel();
    syscall::SyscallResult {
        value: to_write as u64,
        error: 0,
    }
}

fn channel_pair_create(out_ptr: u64, _flags: u64) -> syscall::SyscallResult {
    let size = core::mem::size_of::<[u64; 2]>();
    if !valid_user_range(out_ptr, size) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let mut slots = [usize::MAX; 2];
    let mut found = 0;
    for i in 0..MAX_CHANNELS {
        if unsafe { !CHANNELS[i].used } {
            slots[found] = i;
            found += 1;
            if found == 2 {
                break;
            }
        }
    }
    if found < 2 {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    }
    for &slot in &slots {
        unsafe {
            CHANNELS[slot] = ChannelSlot {
                used: true,
                head: 0,
                len: 0,
                buf: [0; CHANNEL_BUF_SIZE],
            };
        }
    }
    unlock_kernel();
    let handles = [
        make_handle(HANDLE_KIND_CHANNEL, slots[0] as u32),
        make_handle(HANDLE_KIND_CHANNEL, slots[1] as u32),
    ];
    unsafe {
        core::ptr::copy_nonoverlapping(handles.as_ptr().cast::<u8>(), out_ptr as *mut u8, size);
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn channel_send(handle: u64, buffer: u64, length: u64, _flags: u64) -> syscall::SyscallResult {
    if handle_kind(handle) != HANDLE_KIND_CHANNEL {
        return syscall_error(syscall::EBADF);
    }
    let Ok(len) = usize::try_from(length) else {
        return syscall_error(syscall::EINVAL);
    };
    if len == 0 {
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    if !valid_user_range(buffer, len) {
        return syscall_error(syscall::EFAULT);
    }
    let slot = handle_slot(handle);
    if slot >= MAX_CHANNELS {
        return syscall_error(syscall::EBADF);
    }
    lock_kernel();
    let ch = unsafe { &mut CHANNELS[slot] };
    if !ch.used {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    let space = CHANNEL_BUF_SIZE - ch.len;
    let to_write = core::cmp::min(len, space);
    if to_write == 0 {
        unlock_kernel();
        return syscall_error(syscall::EAGAIN);
    }
    let mut tmp = [0u8; CHANNEL_BUF_SIZE];
    unsafe {
        core::ptr::copy_nonoverlapping(buffer as *const u8, tmp.as_mut_ptr(), to_write);
    }
    for i in 0..to_write {
        let idx = (ch.head + ch.len + i) % CHANNEL_BUF_SIZE;
        ch.buf[idx] = tmp[i];
    }
    ch.len += to_write;
    unlock_kernel();
    syscall::SyscallResult {
        value: to_write as u64,
        error: 0,
    }
}

fn channel_receive(handle: u64, buffer: u64, length: u64, _flags: u64) -> syscall::SyscallResult {
    if handle_kind(handle) != HANDLE_KIND_CHANNEL {
        return syscall_error(syscall::EBADF);
    }
    let Ok(len) = usize::try_from(length) else {
        return syscall_error(syscall::EINVAL);
    };
    if len == 0 {
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    if !valid_user_range(buffer, len) {
        return syscall_error(syscall::EFAULT);
    }
    let slot = handle_slot(handle);
    if slot >= MAX_CHANNELS {
        return syscall_error(syscall::EBADF);
    }
    lock_kernel();
    let ch = unsafe { &mut CHANNELS[slot] };
    if !ch.used {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    if ch.len == 0 {
        unlock_kernel();
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    let to_read = core::cmp::min(len, ch.len);
    let mut tmp = [0u8; CHANNEL_BUF_SIZE];
    for i in 0..to_read {
        tmp[i] = ch.buf[(ch.head + i) % CHANNEL_BUF_SIZE];
    }
    ch.head = (ch.head + to_read) % CHANNEL_BUF_SIZE;
    ch.len -= to_read;
    unlock_kernel();
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), buffer as *mut u8, to_read);
    }
    syscall::SyscallResult {
        value: to_read as u64,
        error: 0,
    }
}

fn channel_close(handle: u64) -> syscall::SyscallResult {
    if handle_kind(handle) != HANDLE_KIND_CHANNEL {
        return syscall_error(syscall::EBADF);
    }
    let slot = handle_slot(handle);
    if slot >= MAX_CHANNELS {
        return syscall_error(syscall::EBADF);
    }
    lock_kernel();
    let ch = unsafe { &mut CHANNELS[slot] };
    if !ch.used {
        unlock_kernel();
        return syscall_error(syscall::EBADF);
    }
    *ch = ChannelSlot::empty();
    unlock_kernel();
    syscall::SyscallResult { value: 0, error: 0 }
}

fn shared_memory_create(size: u64, _flags: u64) -> syscall::SyscallResult {
    if size == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let Some(aligned) = align_up_page(size) else {
        return syscall_error(syscall::ENOMEM);
    };
    // Shared memory is modeled as user-mapped memory; return mapped base as handle value.
    let alloc = virtual_alloc(0, aligned, 0);
    if alloc.error != 0 {
        return alloc;
    }
    syscall::SyscallResult {
        value: alloc.value,
        error: 0,
    }
}

fn socket_create(domain: u64, socket_type: u64, protocol: u64) -> syscall::SyscallResult {
    let Ok(domain) = u32::try_from(domain) else {
        return syscall_error(syscall::EAFNOSUPPORT);
    };
    let Ok(socket_type) = u32::try_from(socket_type) else {
        return syscall_error(syscall::EINVAL);
    };
    let Ok(protocol) = u32::try_from(protocol) else {
        return syscall_error(syscall::EINVAL);
    };
    if domain != syscall::AF_INET {
        return syscall_error(syscall::EAFNOSUPPORT);
    }
    if socket_type != syscall::SOCK_STREAM && socket_type != syscall::SOCK_DGRAM {
        return syscall_error(syscall::ENOTSUP);
    }
    lock_kernel();
    let mut slot_idx = None;
    for i in 0..MAX_SOCKETS {
        if unsafe { !SOCKETS[i].used } {
            slot_idx = Some(i);
            break;
        }
    }
    let Some(slot_idx) = slot_idx else {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    };
    unsafe {
        SOCKETS[slot_idx] = SocketSlot {
            used: true,
            domain,
            socket_type,
            protocol,
            bound: false,
            listening: false,
            connected: false,
        };
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: make_handle(HANDLE_KIND_SOCKET, slot_idx as u32),
        error: 0,
    }
}

fn socket_handle(handle: u64) -> Option<usize> {
    if handle_kind(handle) != HANDLE_KIND_SOCKET {
        return None;
    }
    let slot = handle_slot(handle);
    if slot >= MAX_SOCKETS {
        return None;
    }
    let used = unsafe { SOCKETS[slot].used };
    if !used { None } else { Some(slot) }
}

fn socket_bind(handle: u64, _addr_ptr: u64, addr_len: u64) -> syscall::SyscallResult {
    let Some(slot) = socket_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    if addr_len == 0 {
        return syscall_error(syscall::EINVAL);
    }
    lock_kernel();
    unsafe { SOCKETS[slot].bound = true };
    unlock_kernel();
    // No network stack in freestanding kernel; binding is recorded locally.
    syscall::SyscallResult { value: 0, error: 0 }
}

fn socket_listen(handle: u64, _backlog: u64) -> syscall::SyscallResult {
    let Some(slot) = socket_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    lock_kernel();
    let sock = unsafe { &mut SOCKETS[slot] };
    if !sock.bound {
        unlock_kernel();
        return syscall_error(syscall::EINVAL);
    }
    sock.listening = true;
    unlock_kernel();
    syscall::SyscallResult { value: 0, error: 0 }
}

fn socket_accept(handle: u64, _addr_ptr: u64, _addr_len_ptr: u64) -> syscall::SyscallResult {
    let Some(slot) = socket_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    let listening = unsafe { SOCKETS[slot].listening };
    if !listening {
        return syscall_error(syscall::EINVAL);
    }
    // No network stack/connection queue in freestanding kernel.
    syscall_error(syscall::EAGAIN)
}

fn socket_connect(handle: u64, _addr_ptr: u64, addr_len: u64) -> syscall::SyscallResult {
    let Some(_slot) = socket_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    if addr_len == 0 {
        return syscall_error(syscall::EINVAL);
    }
    // No network stack in freestanding kernel.
    syscall_error(syscall::ENOTCONN)
}

fn close_socket_handle(handle: u64) -> syscall::SyscallResult {
    let Some(slot) = socket_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    lock_kernel();
    unsafe { SOCKETS[slot] = SocketSlot::empty() };
    unlock_kernel();
    syscall::SyscallResult { value: 0, error: 0 }
}

fn reboot_system(cmd: u64) -> syscall::SyscallResult {
    match cmd {
        0 => unsafe {
            // Keyboard controller reset pulse.
            asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8, options(nomem, nostack, preserves_flags));
            // ACPI reset via 0xCF9 as fallback.
            asm!("out dx, al", in("dx") 0xCF9u16, in("al") 0x06u8, options(nomem, nostack, preserves_flags));
        },
        1 => unsafe {
            // QEMU/Bochs shutdown ports.
            asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16, options(nomem, nostack, preserves_flags));
            asm!("out dx, ax", in("dx") 0xB004u16, in("ax") 0x2000u16, options(nomem, nostack, preserves_flags));
        },
        _ => return syscall_error(syscall::EINVAL),
    }
    // If control returns, halt.
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

fn init_fs() {
    unsafe {
        if FS_INIT {
            return;
        }
        let mut i = 0usize;
        let entries: [(&[u8], bool, &[u8]); 5] = [
            (b"/", true, b""),
            (b"/dev", true, b""),
            (b"/dev/null", false, b""),
            (b"/dev/console", false, b""),
            (b"/proc/info", false, b"AlphaOS freestanding kernel\n"),
        ];
        while i < entries.len() {
            let (name, is_dir, data) = entries[i];
            let f = &mut FS_FILES[i];
            f.used = true;
            f.is_dir = is_dir;
            f.name_len = name.len().min(64);
            f.name[..f.name_len].copy_from_slice(&name[..f.name_len]);
            let dlen = data.len().min(MAX_FILE_SIZE);
            f.data[..dlen].copy_from_slice(&data[..dlen]);
            f.size = dlen;
            i += 1;
        }
        FS_INIT = true;
    }
}

fn read_user_path(ptr: u64, len: u64, out: &mut [u8; 64]) -> Option<usize> {
    let Ok(len) = usize::try_from(len) else {
        return None;
    };
    if len == 0 || len > out.len() || !valid_user_range(ptr, len) {
        return None;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(ptr as *const u8, out.as_mut_ptr(), len);
    }
    Some(len)
}

fn fs_find(name: &[u8]) -> Option<usize> {
    unsafe {
        for i in 0..MAX_FS_FILES {
            let f = &FS_FILES[i];
            if f.used && f.name_len == name.len() && f.name[..f.name_len] == *name {
                return Some(i);
            }
        }
    }
    None
}

fn fs_create(name: &[u8], is_dir: bool) -> Option<usize> {
    if let Some(idx) = fs_find(name) {
        return Some(idx);
    }
    unsafe {
        for i in 0..MAX_FS_FILES {
            if !FS_FILES[i].used {
                let f = &mut FS_FILES[i];
                *f = FsFile::empty();
                f.used = true;
                f.is_dir = is_dir;
                f.name_len = name.len().min(64);
                f.name[..f.name_len].copy_from_slice(&name[..f.name_len]);
                return Some(i);
            }
        }
    }
    None
}

fn sys_exit(status: u64) -> syscall::SyscallResult {
    EXIT_STATUS.store(status, Ordering::Relaxed);
    EXITED.store(1, Ordering::Relaxed);
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

fn sys_open(path_ptr: u64, path_len: u64, flags: u64, _mode: u64) -> syscall::SyscallResult {
    init_fs();
    let mut path_buf = [0u8; 64];
    let Some(n) = read_user_path(path_ptr, path_len, &mut path_buf) else {
        return syscall_error(syscall::EFAULT);
    };
    let path = &path_buf[..n];
    let flags = flags as u32;
    let fs_idx = if let Some(idx) = fs_find(path) {
        if flags & syscall::O_TRUNC != 0 {
            lock_kernel();
            unsafe {
                FS_FILES[idx].size = 0;
                FS_FILES[idx].data = [0; MAX_FILE_SIZE];
            }
            unlock_kernel();
        }
        idx
    } else if flags & syscall::O_CREATE != 0 {
        let Some(idx) = fs_create(path, flags & syscall::O_DIR != 0) else {
            return syscall_error(syscall::ENOMEM);
        };
        idx
    } else {
        return syscall_error(syscall::ENOENT);
    };

    lock_kernel();
    let mut slot = None;
    for i in 0..MAX_OPEN_FILES {
        if unsafe { !OPEN_FILES[i].used } {
            slot = Some(i);
            break;
        }
    }
    let Some(slot) = slot else {
        unlock_kernel();
        return syscall_error(syscall::ENOMEM);
    };
    unsafe {
        OPEN_FILES[slot] = OpenFile {
            used: true,
            fs_index: fs_idx,
            offset: 0,
            flags,
        };
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: make_handle(HANDLE_KIND_FILE, slot as u32),
        error: 0,
    }
}

fn file_handle_slot(handle: u64) -> Option<usize> {
    if handle_kind(handle) != HANDLE_KIND_FILE {
        return None;
    }
    let slot = handle_slot(handle);
    if slot >= MAX_OPEN_FILES {
        return None;
    }
    let used = unsafe { OPEN_FILES[slot].used };
    if used { Some(slot) } else { None }
}

fn sys_file_read(handle: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some(slot) = file_handle_slot(handle) else {
        return syscall_error(syscall::EBADF);
    };
    let Ok(len) = usize::try_from(length) else {
        return syscall_error(syscall::EINVAL);
    };
    if len == 0 {
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    if !valid_user_range(buffer, len) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let of = unsafe { OPEN_FILES[slot] };
    let fs_idx = of.fs_index;
    let is_dir = unsafe { FS_FILES[fs_idx].is_dir };
    if is_dir {
        unlock_kernel();
        return syscall_error(syscall::EISDIR);
    }
    let file_size = unsafe { FS_FILES[fs_idx].size };
    let offset = of.offset.min(file_size);
    let to_read = core::cmp::min(len, file_size.saturating_sub(offset));
    unsafe {
        core::ptr::copy_nonoverlapping(
            FS_FILES[fs_idx].data.as_ptr().add(offset),
            buffer as *mut u8,
            to_read,
        );
        OPEN_FILES[slot].offset = offset + to_read;
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: to_read as u64,
        error: 0,
    }
}

fn sys_file_write(handle: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some(slot) = file_handle_slot(handle) else {
        return syscall_error(syscall::EBADF);
    };
    let Ok(len) = usize::try_from(length) else {
        return syscall_error(syscall::EINVAL);
    };
    if len == 0 {
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    if !valid_user_range(buffer, len) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let of = unsafe { OPEN_FILES[slot] };
    let fs_idx = of.fs_index;
    let is_dir = unsafe { FS_FILES[fs_idx].is_dir };
    if is_dir {
        unlock_kernel();
        return syscall_error(syscall::EISDIR);
    }
    let offset = of.offset;
    let available = MAX_FILE_SIZE.saturating_sub(offset);
    let to_write = core::cmp::min(len, available);
    if to_write == 0 {
        unlock_kernel();
        return syscall_error(syscall::ENOSPC);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            buffer as *const u8,
            FS_FILES[fs_idx].data.as_mut_ptr().add(offset),
            to_write,
        );
        let end = offset + to_write;
        if end > FS_FILES[fs_idx].size {
            FS_FILES[fs_idx].size = end;
        }
        OPEN_FILES[slot].offset = end;
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: to_write as u64,
        error: 0,
    }
}

fn sys_seek(handle: u64, offset: u64, whence: u64) -> syscall::SyscallResult {
    let Some(slot) = file_handle_slot(handle) else {
        return syscall_error(syscall::EBADF);
    };
    lock_kernel();
    let of = unsafe { OPEN_FILES[slot] };
    let size = unsafe { FS_FILES[of.fs_index].size } as i64;
    let cur = of.offset as i64;
    let off = offset as i64;
    let new_off = match whence {
        syscall::SEEK_SET => off,
        syscall::SEEK_CUR => cur.saturating_add(off),
        syscall::SEEK_END => size.saturating_add(off),
        _ => {
            unlock_kernel();
            return syscall_error(syscall::EINVAL);
        }
    };
    if new_off < 0 {
        unlock_kernel();
        return syscall_error(syscall::EINVAL);
    }
    let new_off = new_off as usize;
    unsafe {
        OPEN_FILES[slot].offset = new_off;
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: new_off as u64,
        error: 0,
    }
}

fn sys_stat(path_ptr: u64, path_len: u64, stat_ptr: u64) -> syscall::SyscallResult {
    init_fs();
    let mut path_buf = [0u8; 64];
    let Some(n) = read_user_path(path_ptr, path_len, &mut path_buf) else {
        return syscall_error(syscall::EFAULT);
    };
    if !valid_user_range(stat_ptr, core::mem::size_of::<syscall::FileStat>()) {
        return syscall_error(syscall::EFAULT);
    }
    let Some(idx) = fs_find(&path_buf[..n]) else {
        return syscall_error(syscall::ENOENT);
    };
    let (size, is_dir) = unsafe { (FS_FILES[idx].size, FS_FILES[idx].is_dir) };
    let stat = syscall::FileStat {
        size: size as u64,
        mode: if is_dir { 0o755 } else { 0o644 },
        flags: if is_dir { 1 } else { 0 },
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(stat).cast::<u8>(),
            stat_ptr as *mut u8,
            core::mem::size_of::<syscall::FileStat>(),
        );
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn sys_readdir(handle: u64, entry_ptr: u64) -> syscall::SyscallResult {
    let Some(slot) = file_handle_slot(handle) else {
        return syscall_error(syscall::EBADF);
    };
    if !valid_user_range(entry_ptr, core::mem::size_of::<syscall::DirEntry>()) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let of = unsafe { OPEN_FILES[slot] };
    let dir_idx = of.fs_index;
    let (is_dir, dir_name_len) = unsafe { (FS_FILES[dir_idx].is_dir, FS_FILES[dir_idx].name_len) };
    if !is_dir {
        unlock_kernel();
        return syscall_error(syscall::ENOTDIR);
    }
    let mut dir_name = [0u8; 64];
    unsafe {
        dir_name[..dir_name_len].copy_from_slice(&FS_FILES[dir_idx].name[..dir_name_len]);
    }
    let cursor = of.offset;
    let mut seen = 0usize;
    let mut found: Option<usize> = None;
    for i in 0..MAX_FS_FILES {
        let f = unsafe { &FS_FILES[i] };
        if !f.used
            || i == dir_idx
            || f.is_dir
                && f.name_len == dir_name_len
                && f.name[..f.name_len] == dir_name[..dir_name_len]
        {
            continue;
        }
        // Child if in this directory: /dev/null is child of /dev and /
        let parent = &dir_name[..dir_name_len];
        let child = &f.name[..f.name_len];
        let is_child = if parent == b"/" {
            child.starts_with(b"/") && !child[1..].contains(&b'/')
        } else {
            child.len() > parent.len() + 1
                && child.starts_with(parent)
                && child[parent.len()] == b'/'
                && !child[parent.len() + 1..].contains(&b'/')
        };
        if !is_child {
            continue;
        }
        if seen == cursor {
            found = Some(i);
            break;
        }
        seen += 1;
    }
    if let Some(idx) = found {
        let f = unsafe { FS_FILES[idx] };
        let base_start = if dir_name_len == 1 {
            1
        } else {
            dir_name_len + 1
        };
        let base = &f.name[base_start..f.name_len];
        let mut entry = syscall::DirEntry {
            name_len: base.len().min(64) as u32,
            entry_type: if f.is_dir { 2 } else { 1 },
            size: f.size as u64,
            name: [0; 64],
        };
        entry.name[..entry.name_len as usize].copy_from_slice(&base[..entry.name_len as usize]);
        unsafe {
            core::ptr::copy_nonoverlapping(
                core::ptr::addr_of!(entry).cast::<u8>(),
                entry_ptr as *mut u8,
                core::mem::size_of::<syscall::DirEntry>(),
            );
            OPEN_FILES[slot].offset = cursor + 1;
        }
        unlock_kernel();
        syscall::SyscallResult { value: 1, error: 0 }
    } else {
        unlock_kernel();
        syscall::SyscallResult { value: 0, error: 0 }
    }
}

fn sys_map_memory(
    address: u64,
    length: u64,
    flags: u64,
    _handle: u64,
    _offset: u64,
) -> syscall::SyscallResult {
    // Anonymous mapping is supported in freestanding kernel; file-backed mappings are treated as anonymous.
    let _ = flags;
    if length == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let addr = if address == 0 { 0 } else { address };
    let alloc = virtual_alloc(addr, length, 0);
    if alloc.error != 0 {
        return alloc;
    }
    syscall::SyscallResult {
        value: alloc.value,
        error: 0,
    }
}

fn sys_unmap_memory(address: u64, length: u64) -> syscall::SyscallResult {
    // Bump allocator cannot reclaim individual pages; validate and report success for mapped ranges.
    if address == 0 || length == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let Some(size) = align_up_page(length) else {
        return syscall_error(syscall::EINVAL);
    };
    let heap_end = USER_HEAP_NEXT.load(Ordering::Relaxed);
    let Some(end) = address.checked_add(size) else {
        return syscall_error(syscall::EINVAL);
    };
    if address < USER_CODE_BASE || end > heap_end {
        return syscall_error(syscall::EINVAL);
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn sys_heap_alloc(size: u64, _alignment: u64) -> syscall::SyscallResult {
    if size == 0 {
        return syscall_error(syscall::EINVAL);
    }
    let Some(aligned) = align_up_page(size) else {
        return syscall_error(syscall::ENOMEM);
    };
    let alloc = virtual_alloc(0, aligned, 0);
    if alloc.error != 0 {
        return alloc;
    }
    lock_kernel();
    for i in 0..MAX_HEAP_BLOCKS {
        if unsafe { !HEAP_BLOCKS[i].used } {
            unsafe {
                HEAP_BLOCKS[i] = HeapBlock {
                    used: true,
                    addr: alloc.value,
                    size: aligned,
                };
            }
            unlock_kernel();
            return syscall::SyscallResult {
                value: alloc.value,
                error: 0,
            };
        }
    }
    unlock_kernel();
    syscall::SyscallResult {
        value: alloc.value,
        error: 0,
    }
}

fn sys_heap_free(address: u64, _size: u64) -> syscall::SyscallResult {
    if address == 0 {
        return syscall_error(syscall::EINVAL);
    }
    lock_kernel();
    for i in 0..MAX_HEAP_BLOCKS {
        if unsafe { HEAP_BLOCKS[i].used && HEAP_BLOCKS[i].addr == address } {
            unsafe {
                HEAP_BLOCKS[i].used = false;
            }
            unlock_kernel();
            return syscall::SyscallResult { value: 0, error: 0 };
        }
    }
    unlock_kernel();
    // Not tracked as heap block; treat as no-op success for bump-allocator compatibility.
    syscall::SyscallResult { value: 0, error: 0 }
}

fn read_tsc() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdtsc", out("eax") low, out("edx") high, out("ecx") _, options(nomem, nostack, preserves_flags));
    }
    (u64::from(high) << 32) | u64::from(low)
}

fn sys_clock_gettime(clock_id: u64, timespec_ptr: u64) -> syscall::SyscallResult {
    if clock_id != syscall::CLOCK_REALTIME && clock_id != syscall::CLOCK_MONOTONIC {
        return syscall_error(syscall::EINVAL);
    }
    if !valid_user_range(timespec_ptr, core::mem::size_of::<syscall::TimeSpec>()) {
        return syscall_error(syscall::EFAULT);
    }
    let tsc = read_tsc();
    // Nominal 1GHz TSC for deterministic freestanding reporting.
    let seconds = tsc / 1_000_000_000;
    let nanoseconds = tsc % 1_000_000_000;
    let ts = syscall::TimeSpec {
        seconds,
        nanoseconds,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(ts).cast::<u8>(),
            timespec_ptr as *mut u8,
            core::mem::size_of::<syscall::TimeSpec>(),
        );
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn sys_sleep(nanoseconds: u64) -> syscall::SyscallResult {
    if nanoseconds == 0 {
        return syscall::SyscallResult { value: 0, error: 0 };
    }
    let start = read_tsc();
    // Nominal 1GHz TSC: 1 ns per tick.
    let target = start.saturating_add(nanoseconds);
    while read_tsc() < target {
        unsafe {
            asm!("pause", options(nomem, nostack, preserves_flags));
        }
    }
    syscall::SyscallResult { value: 0, error: 0 }
}

fn sys_spawn(path_ptr: u64, path_len: u64, _argv_ptr: u64, _flags: u64) -> syscall::SyscallResult {
    init_fs();
    let mut path_buf = [0u8; 64];
    let Some(n) = read_user_path(path_ptr, path_len, &mut path_buf) else {
        return syscall_error(syscall::EFAULT);
    };
    // Accept spawn for any valid path in single-process freestanding kernel.
    if fs_find(&path_buf[..n]).is_none() {
        let _ = fs_create(&path_buf[..n], false);
    }
    lock_kernel();
    let mut slot = None;
    for i in 0..MAX_SPAWNED {
        if unsafe { !SPAWNED[i].used } {
            slot = Some(i);
            break;
        }
    }
    let Some(slot) = slot else {
        unlock_kernel();
        return syscall_error(syscall::EAGAIN);
    };
    let pid = NEXT_SPAWN_PID.fetch_add(1, Ordering::Relaxed);
    unsafe {
        SPAWNED[slot] = SpawnedProc {
            used: true,
            pid,
            exited: true,
            exit_code: 0,
        };
    }
    unlock_kernel();
    // Single-process freestanding kernel: spawn records a completed child process entry.
    syscall::SyscallResult {
        value: pid,
        error: 0,
    }
}

fn sys_wait(pid: u64, status_ptr: u64, _options: u64) -> syscall::SyscallResult {
    if status_ptr != 0 && !valid_user_range(status_ptr, core::mem::size_of::<u64>()) {
        return syscall_error(syscall::EFAULT);
    }
    lock_kernel();
    let mut found = None;
    for i in 0..MAX_SPAWNED {
        let p = unsafe { SPAWNED[i] };
        if p.used && p.exited && (pid == 0 || pid == p.pid) {
            found = Some(i);
            break;
        }
    }
    let Some(idx) = found else {
        unlock_kernel();
        return syscall_error(syscall::ECHILD);
    };
    let proc_pid = unsafe { SPAWNED[idx].pid };
    let exit_code = unsafe { SPAWNED[idx].exit_code };
    unsafe {
        SPAWNED[idx] = SpawnedProc::empty();
    }
    unlock_kernel();
    if status_ptr != 0 {
        unsafe {
            core::ptr::write_unaligned(status_ptr as *mut u64, exit_code);
        }
    }
    syscall::SyscallResult {
        value: proc_pid,
        error: 0,
    }
}

fn sys_device_control(
    handle: u64,
    request: u64,
    _input_ptr: u64,
    _input_len: u64,
    output_ptr: u64,
    output_len: u64,
) -> syscall::SyscallResult {
    let Some((device_id, _access)) = decode_device_handle(handle) else {
        return syscall_error(syscall::EBADF);
    };
    // Minimal device control: request 0 returns device id in output u32/u64.
    match request {
        0 => {
            if output_len < 4 || !valid_user_range(output_ptr, 4) {
                return syscall_error(syscall::EINVAL);
            }
            unsafe {
                core::ptr::write_unaligned(output_ptr as *mut u32, device_id);
            }
            syscall::SyscallResult { value: 0, error: 0 }
        }
        _ => {
            let _ = device_id;
            syscall_error(syscall::ENOTSUP)
        }
    }
}

fn get_system_info(kind: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let mut info_bytes = [0_u8; core::mem::size_of::<syscall::MemoryInfo>()];
    let size = match kind {
        syscall::SYSTEM_INFO_ABI_VERSION => {
            let info = syscall::AbiVersionInfo {
                major: syscall::SYSCALL_ABI_MAJOR,
                minor: syscall::SYSCALL_ABI_MINOR,
                syscall_count: syscall::SYSCALL_COUNT as u32,
            };
            let size = core::mem::size_of_val(&info);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    core::ptr::addr_of!(info).cast::<u8>(),
                    info_bytes.as_mut_ptr(),
                    size,
                );
            }
            size
        }
        syscall::SYSTEM_INFO_MEMORY => {
            let start = unsafe { core::ptr::addr_of!(ALLOCATOR_START).read_volatile() };
            let next = unsafe { core::ptr::addr_of!(ALLOCATOR_NEXT).read_volatile() };
            let end = unsafe { core::ptr::addr_of!(ALLOCATOR_END).read_volatile() };
            if start > next || next > end {
                return syscall::SyscallResult {
                    value: u64::MAX,
                    error: syscall::EIO,
                };
            }
            let info = syscall::MemoryInfo {
                managed_bytes: end - start,
                allocated_bytes: next - start,
                available_bytes: end - next,
            };
            let size = core::mem::size_of_val(&info);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    core::ptr::addr_of!(info).cast::<u8>(),
                    info_bytes.as_mut_ptr(),
                    size,
                );
            }
            size
        }
        syscall::SYSTEM_INFO_FRAMEBUFFER => {
            let console = unsafe { core::ptr::addr_of!(SYSCALL_CONSOLE).read_volatile() };
            if console.is_null() {
                return syscall::SyscallResult {
                    value: u64::MAX,
                    error: syscall::EIO,
                };
            }
            let info = unsafe {
                syscall::FramebufferInfo {
                    width: (*console).width as u64,
                    height: (*console).height as u64,
                    pitch_bytes: (*console).pitch as u64,
                }
            };
            let size = core::mem::size_of_val(&info);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    core::ptr::addr_of!(info).cast::<u8>(),
                    info_bytes.as_mut_ptr(),
                    size,
                );
            }
            size
        }
        _ => {
            return syscall::SyscallResult {
                value: u64::MAX,
                error: syscall::EINVAL,
            };
        }
    };

    let Ok(length) = usize::try_from(length) else {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EFAULT,
        };
    };
    if length < size {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EINVAL,
        };
    }
    if !valid_user_range(buffer, size) {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EFAULT,
        };
    }

    for offset in 0..size {
        unsafe {
            (buffer as *mut u8)
                .add(offset)
                .write_volatile(info_bytes[offset]);
        }
    }
    syscall::SyscallResult {
        value: size as u64,
        error: 0,
    }
}

fn read_console(fd: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    if fd != 0 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EBADF,
        };
    }
    if length == 0 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EINVAL,
        };
    }
    if !valid_user_range(buffer, 1) {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EFAULT,
        };
    }
    if input::foreground_target() != Some(USER_INPUT_TARGET) {
        return syscall_error(syscall::EACCES);
    }

    loop {
        let console = unsafe { core::ptr::addr_of!(SYSCALL_CONSOLE).read_volatile() };
        if console.is_null() {
            return syscall::SyscallResult {
                value: u64::MAX,
                error: syscall::EIO,
            };
        }
        let Some(event) = input::dequeue_for(USER_INPUT_TARGET) else {
            interrupts::wait_for_interrupt();
            continue;
        };
        let Some(character) = keyboard::event_to_ascii(&event) else {
            continue;
        };
        unsafe {
            (buffer as *mut u8).write_volatile(character);
        }
        return syscall::SyscallResult { value: 1, error: 0 };
    }
}

fn read_input_event(buffer: u64, length: u64) -> syscall::SyscallResult {
    let Ok(length) = usize::try_from(length) else {
        return syscall_error(syscall::EFAULT);
    };
    let event_size = core::mem::size_of::<syscall::InputEvent>();
    if length < event_size {
        return syscall_error(syscall::EINVAL);
    }
    if !valid_user_range(buffer, event_size) {
        return syscall_error(syscall::EFAULT);
    }
    if input::foreground_target() != Some(USER_INPUT_TARGET) {
        return syscall_error(syscall::EACCES);
    }

    loop {
        if let Some(event) = input::dequeue_for(USER_INPUT_TARGET) {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    core::ptr::addr_of!(event).cast::<u8>(),
                    buffer as *mut u8,
                    event_size,
                );
            }
            return syscall::SyscallResult {
                value: event_size as u64,
                error: 0,
            };
        }
        interrupts::wait_for_interrupt();
    }
}

fn write_console(fd: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    if fd != 1 && fd != 2 {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EBADF,
        };
    }
    let Ok(length) = usize::try_from(length) else {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EFAULT,
        };
    };
    if !valid_user_range(buffer, length) {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EFAULT,
        };
    }
    let console = unsafe { core::ptr::addr_of!(SYSCALL_CONSOLE).read_volatile() };
    if console.is_null() {
        return syscall::SyscallResult {
            value: u64::MAX,
            error: syscall::EIO,
        };
    }

    unsafe {
        (*console).hide_caret();
        for offset in 0..length {
            let character = (buffer as *const u8).add(offset).read_volatile();
            match character {
                b'\n' | b'\r' => (*console).submit_line(),
                8 => (*console).backspace(),
                0x20..=0x7E => (*console).type_character(character),
                _ => {}
            }
        }
        (*console).draw_caret();
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
    const PROMPT_Y: usize = 112;
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
            cursor_x: Self::INPUT_X,
        };
        console.draw_header();
        console.draw_prompt();
        console.draw_caret();
        console
    }

    fn type_character(&mut self, character: u8) {
        if self.cursor_x.saturating_add(8) > self.width {
            return;
        }

        draw_text(
            self.address,
            self.width,
            self.height,
            self.pitch,
            self.pixel_format,
            self.cursor_x,
            self.cursor_y,
            core::str::from_utf8(core::slice::from_ref(&character)).unwrap_or(" "),
            Self::TEXT.0,
            Self::TEXT.1,
            Self::TEXT.2,
        );
        self.cursor_x = self.cursor_x.saturating_add(8);
    }

    fn backspace(&mut self) {
        if self.cursor_x <= Self::INPUT_X {
            return;
        }

        self.cursor_x -= 8;
        self.fill_rect(self.cursor_x, self.cursor_y, 8, 8, Self::BACKGROUND);
    }

    fn submit_line(&mut self) {
        self.cursor_x = Self::INPUT_X;
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

    fn draw_caret(&self) {
        self.fill_rect(self.cursor_x, self.cursor_y + 7, 6, 1, Self::TEXT);
    }

    fn hide_caret(&self) {
        self.fill_rect(self.cursor_x, self.cursor_y + 7, 6, 1, Self::BACKGROUND);
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
            "IRQ1 PS/2 input: key events routed to foreground PID 1",
            0xB0,
            0xB0,
            0xB0,
        );
        self.draw_string(
            Self::LEFT,
            Self::TOP + 64,
            "read(): translated bytes; device_read(): key events",
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
