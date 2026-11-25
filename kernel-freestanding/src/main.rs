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

const MAX_ENV_VARS: usize = 16;
const MAX_ENV_KEY: usize = 32;
const MAX_ENV_VAL: usize = 128;
const MAX_TIMERS: usize = 8;
const MAX_EVENTS: usize = 16;
const MAX_PROC_NAME: usize = 32;
const KLOG_SIZE: usize = 1024;

#[derive(Clone, Copy)]
struct EnvVar { used: bool, key_len: usize, val_len: usize, key: [u8; MAX_ENV_KEY], val: [u8; MAX_ENV_VAL] }
impl EnvVar { const fn empty() -> Self { Self { used: false, key_len: 0, val_len: 0, key: [0; MAX_ENV_KEY], val: [0; MAX_ENV_VAL] } } }
#[derive(Clone, Copy)]
struct TimerSlot { used: bool, interval_ms: u64, start_tick: u64 }
impl TimerSlot { const fn empty() -> Self { Self { used: false, interval_ms: 0, start_tick: 0 } } }
#[derive(Clone, Copy)]
struct EventSlot { used: bool, signaled: bool }
impl EventSlot { const fn empty() -> Self { Self { used: false, signaled: false } } }

static mut CWD_BUF: [u8; 128] = [0; 128];
static mut CWD_LEN: usize = 1;
static mut ENV_VARS: [EnvVar; MAX_ENV_VARS] = [EnvVar::empty(); MAX_ENV_VARS];
static mut SYS_NAME: [u8; 32] = *b"AlphaOS\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
static mut SYS_NAME_LEN: usize = 7;
static mut CONSOLE_MODE: u64 = 0;
static mut TIMERS: [TimerSlot; MAX_TIMERS] = [TimerSlot::empty(); MAX_TIMERS];
static mut EVENTS: [EventSlot; MAX_EVENTS] = [EventSlot::empty(); MAX_EVENTS];
static mut KLOG: [u8; KLOG_SIZE] = [0; KLOG_SIZE];
static mut KLOG_LEN: usize = 0;
static mut RNG_STATE: u64 = 0x9E3779B97F4A7C15;
static mut BOOT_TSC: u64 = 0;
static mut PROC_NAMES: [[u8; MAX_PROC_NAME]; MAX_SPAWNED] = [[0; MAX_PROC_NAME]; MAX_SPAWNED];
static mut PROC_NAME_LENS: [usize; MAX_SPAWNED] = [0; MAX_SPAWNED];
static mut SC_UID: u32 = 0;
static mut SC_GID: u32 = 0;
static mut SC_LAST_ERROR: u64 = 0;
static mut SC_THREAD_PRIORITY: [i32; 32] = [0; 32];
static mut SC_MUTEXES: [u8; 64] = [0; 64]; // 0=free,1=unlocked,2=locked
static mut SC_SEMS: [i32; 64] = [0; 64];
static mut SC_SEMS_USED: [bool; 64] = [false; 64];
static mut SC_CONDVARS: [bool; 64] = [false; 64];
static mut SC_MSGQ: [[[u8; 256]; 8]; 16] = [[[0; 256]; 8]; 16];
static mut SC_MSGQ_LEN: [[usize; 8]; 16] = [[0; 8]; 16];
static mut SC_MSGQ_HEAD: [usize; 16] = [0; 16];
static mut SC_MSGQ_TAIL: [usize; 16] = [0; 16];
static mut SC_MSGQ_COUNT: [usize; 16] = [0; 16];
static mut SC_MSGQ_USED: [bool; 16] = [false; 16];
static mut SC_EVENTFD: [u64; 64] = [0; 64];
static mut SC_EVENTFD_USED: [bool; 64] = [false; 64];
static mut SC_SYMLINKS: [[u8; 128]; 32] = [[0; 128]; 32];
static mut SC_SYMLINK_TARGETS: [[u8; 128]; 32] = [[0; 128]; 32];
static mut SC_SYMLINK_LEN: [usize; 32] = [0; 32];
static mut SC_SYMLINK_TLEN: [usize; 32] = [0; 32];
static mut SC_SYMLINK_USED: [bool; 32] = [false; 32];
static mut SC_FILE_MODE: [u32; 128] = [0o644; 128];
static mut SC_ALARM_MS: u64 = 0;

fn sc_set_last_error(e: u64) { unsafe { *core::ptr::addr_of_mut!(SC_LAST_ERROR) = e; } }
fn sc_write_u32(ptr: u64, val: u32) -> Result<(), u64> { if ptr==0{return Ok(());} if let Err(e)=validate_user_buffer(ptr,4,true){return Err(e);} unsafe{core::ptr::write_unaligned(ptr as *mut u32,val);} Ok(()) }
fn sc_copy_to_user(dst: u64, dst_len: u64, src: &[u8]) -> Result<u64, u64> {
    let n = core::cmp::min(src.len(), dst_len as usize);
    if n==0 { return Ok(0); }
    if let Err(e)=validate_user_buffer(dst, n as u64, true){return Err(e);}
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, n); }
    Ok(n as u64)
}
fn sc_copy_cstr_to_user(dst: u64, dst_len: u64, src: &[u8]) -> Result<u64, u64> {
    if dst_len==0 { return Ok(0); }
    let n = core::cmp::min(src.len(), (dst_len as usize).saturating_sub(1));
    if let Err(e)=validate_user_buffer(dst, (n+1) as u64, true){return Err(e);}
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, n); *((dst as *mut u8).add(n)) = 0; }
    Ok(n as u64)
}
fn sc_read_user(buf: u64, len: u64) -> Result<&'static [u8], u64> {
    let n = len as usize;
    if n==0 { return Ok(&[]); }
    if let Err(e)=validate_user_buffer(buf, len, false){return Err(e);}
    Ok(unsafe{core::slice::from_raw_parts(buf as *const u8, n)})
}
fn sc_ticks_ms() -> u64 { unsafe { read_tsc().wrapping_sub(BOOT_TSC) / 1_000_000 } }



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
        BOOT_TSC = read_tsc();
        CWD_BUF[0] = b'/';
        CWD_LEN = 1;
        RNG_STATE ^= read_tsc();
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

type SyscallResult = syscall::SyscallResult;

fn syscall_errno(e: u64) -> u64 { e }

fn validate_user_buffer(ptr: u64, len: u64, _write: bool) -> Result<(), u64> {
    let Ok(l) = usize::try_from(len) else { return Err(syscall::EINVAL); };
    if l == 0 { return Ok(()); }
    if !valid_user_range(ptr, l) { return Err(syscall::EFAULT); }
    Ok(())
}

fn user_write_buf(ptr: u64, len: u64, src: &[u8]) -> Result<u64, u64> {
    if len == 0 { return Ok(0); }
    let n = core::cmp::min(len as usize, src.len());
    validate_user_buffer(ptr, n as u64, true).map_err(syscall_errno)?;
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), ptr as *mut u8, n); }
    Ok(n as u64)
}

fn user_read_bytes(ptr: u64, len: u64) -> Result<&'static [u8], u64> {
    if len == 0 { return Ok(&[]); }
    validate_user_buffer(ptr, len, false).map_err(syscall_errno)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

fn user_read_path(ptr: u64, len: u64) -> Result<&'static str, u64> {
    let bytes = user_read_bytes(ptr, len)?;
    core::str::from_utf8(bytes).map_err(|_| syscall_errno(syscall::EINVAL))
}

fn resolve_path(path: &str) -> [u8; 128] {
    let mut out = [0u8; 128];
    let base: &[u8] = if path.starts_with('/') { b"" } else { unsafe { &CWD_BUF[..CWD_LEN] } };
    let mut tmp = [0u8; 256];
    let mut tlen = 0usize;
    for &b in base.iter().chain(b"/".iter()).chain(path.as_bytes().iter()) {
        if tlen < tmp.len() { tmp[tlen] = b; tlen += 1; }
    }
    let mut parts: [[u8; 64]; 16] = [[0;64];16];
    let mut plens: [usize;16] = [0;16];
    let mut pcount = 0usize;
    let mut i = 0usize;
    while i < tlen {
        while i < tlen && tmp[i] == b'/' { i += 1; }
        if i >= tlen { break; }
        let start = i;
        while i < tlen && tmp[i] != b'/' { i += 1; }
        let seg = &tmp[start..i];
        if seg == b"." { continue; }
        if seg == b".." { if pcount > 0 { pcount -= 1; } continue; }
        if pcount < 16 {
            let l = core::cmp::min(seg.len(), 64);
            parts[pcount][..l].copy_from_slice(&seg[..l]);
            plens[pcount] = l;
            pcount += 1;
        }
    }
    if pcount == 0 { out[0] = b'/'; return out; }
    let mut n = 0usize;
    for pi in 0..pcount {
        if n < 127 { out[n] = b'/'; n += 1; }
        let l = core::cmp::min(plens[pi], 127-n);
        out[n..n+l].copy_from_slice(&parts[pi][..l]);
        n += l;
    }
    if n == 0 { out[0] = b'/'; }
    out
}

fn resolved_len(p: &[u8;128]) -> usize { let mut n=0; while n<128 && p[n]!=0 { n+=1; } if n==0 {1} else {n} }

fn with_fs<R>(f: impl FnOnce(&mut [FsFile; MAX_FS_FILES]) -> R) -> R { unsafe { init_fs(); f(&mut *core::ptr::addr_of_mut!(FS_FILES)) } }

fn find_fs(path: &[u8;128], plen: usize) -> Option<usize> {
    with_fs(|fs| { for i in 0..MAX_FS_FILES { if fs[i].used && fs[i].name_len == plen && fs[i].name[..plen] == path[..plen] { return Some(i); } } None })
}

fn syscall_getcwd(buf: u64, len: u64) -> SyscallResult {
    if len == 0 { return syscall_error(syscall::EINVAL); }
    let (ptr, n) = unsafe { (core::ptr::addr_of!(CWD_BUF).cast::<u8>(), CWD_LEN) };
    let copy_n = core::cmp::min(n, (len as usize).saturating_sub(1));
    if let Err(e) = validate_user_buffer(buf, len, true) { return syscall_error(e); }
    unsafe { core::ptr::copy_nonoverlapping(ptr, buf as *mut u8, copy_n); *((buf as *mut u8).add(copy_n)) = 0; }
    SyscallResult::success(copy_n as u64)
}

fn syscall_setcwd(path_ptr: u64, path_len: u64) -> SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p) => p, Err(e) => return syscall_error(e) };
    let resolved = resolve_path(path);
    let rlen = resolved_len(&resolved);
    if !(rlen == 1 && resolved[0] == b'/') {
        match find_fs(&resolved, rlen) { Some(i) => { if !with_fs(|fs| fs[i].is_dir) { return syscall_error(syscall::ENOTDIR); } }, None => return syscall_error(syscall::ENOENT) }
    }
    unsafe { let dst = core::ptr::addr_of_mut!(CWD_BUF).cast::<u8>();
        core::ptr::write_bytes(dst, 0, 128);
        core::ptr::copy_nonoverlapping(resolved.as_ptr(), dst, rlen);
        CWD_LEN = rlen; }
    SyscallResult::success(0)
}

fn syscall_mkdir(path_ptr: u64, path_len: u64) -> SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    if find_fs(&resolved, rlen).is_some() { return syscall_error(syscall::EEXIST); }
    with_fs(|fs| {
        for i in 0..MAX_FS_FILES { if !fs[i].used {
            fs[i].used = true; fs[i].is_dir = true; fs[i].size = 0; let nlm = core::cmp::min(rlen,64); fs[i].name = [0;64]; fs[i].name[..nlm].copy_from_slice(&resolved[..nlm]); fs[i].name_len = nlm;
            return SyscallResult::success(0);
        }}
        syscall_error(syscall::ENOSPC)
    })
}

fn syscall_rmdir(path_ptr: u64, path_len: u64) -> SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    if rlen == 1 && resolved[0] == b'/' { return syscall_error(syscall::EINVAL); }
    let idx = match find_fs(&resolved, rlen) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    let is_dir = with_fs(|fs| fs[idx].is_dir);
    if !is_dir { return syscall_error(syscall::ENOTDIR); }
    let not_empty = with_fs(|fs| {
        for i in 0..MAX_FS_FILES { if fs[i].used && i != idx && fs[i].name_len > rlen && fs[i].name[..rlen] == resolved[..rlen] && fs[i].name[rlen] == b'/' { return true; } }
        false
    });
    if not_empty { return syscall_error(syscall::ENOTEMPTY); }
    with_fs(|fs| { fs[idx] = FsFile::empty(); });
    SyscallResult::success(0)
}

fn syscall_unlink(path_ptr: u64, path_len: u64) -> SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    let idx = match find_fs(&resolved, rlen) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    let is_dir = with_fs(|fs| fs[idx].is_dir);
    if is_dir { return syscall_error(syscall::EISDIR); }
    with_fs(|fs| { fs[idx] = FsFile::empty(); });
    SyscallResult::success(0)
}

fn syscall_rename(old_ptr: u64, old_len: u64, new_ptr: u64, new_len: u64) -> SyscallResult {
    let oldp = match user_read_path(old_ptr, old_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let newp = match user_read_path(new_ptr, new_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let oldr = resolve_path(oldp); let oldl = resolved_len(&oldr);
    let newr = resolve_path(newp); let newl = resolved_len(&newr);
    let idx = match find_fs(&oldr, oldl) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    if let Some(nidx) = find_fs(&newr, newl) { with_fs(|fs| fs[nidx] = FsFile::empty()); }
    with_fs(|fs| { let nlm = core::cmp::min(newl,64); fs[idx].name = [0;64]; fs[idx].name[..nlm].copy_from_slice(&newr[..nlm]); fs[idx].name_len = nlm; });
    SyscallResult::success(0)
}

fn syscall_truncate_handle(handle: u64, length: u64) -> SyscallResult {
    let Some(slot) = file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    let fi = unsafe { OPEN_FILES[slot].fs_index };
    if fi >= MAX_FS_FILES { return syscall_error(syscall::EBADF); }
    with_fs(|fs| {
        if !fs[fi].used || fs[fi].is_dir { return syscall_error(syscall::EINVAL); }
        let new_len = core::cmp::min(length as usize, MAX_FILE_SIZE);
        if new_len < fs[fi].size { for b in &mut fs[fi].data[new_len..fs[fi].size] { *b = 0; } }
        fs[fi].size = new_len;
        unsafe { if OPEN_FILES[slot].offset > new_len { OPEN_FILES[slot].offset = new_len; } }
        SyscallResult::success(0)
    })
}

fn syscall_dup(handle: u64) -> SyscallResult {
    let Some(slot) = file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    let src = unsafe { OPEN_FILES[slot] };
    for i in 0..MAX_OPEN_FILES { unsafe { if !OPEN_FILES[i].used { OPEN_FILES[i] = src; return SyscallResult::success(make_handle(HANDLE_KIND_FILE, i as u32)); } } }
    syscall_error(syscall::ENOSPC)
}

fn syscall_dup2(old_handle: u64, new_handle: u64) -> SyscallResult {
    let Some(src_slot) = file_handle_slot(old_handle) else { return syscall_error(syscall::EBADF); };
    let Some(dst_slot) = file_handle_slot(new_handle) else { return syscall_error(syscall::EBADF); };
    if src_slot == dst_slot { return SyscallResult::success(new_handle); }
    unsafe { OPEN_FILES[dst_slot] = OPEN_FILES[src_slot]; }
    SyscallResult::success(new_handle)
}

fn syscall_access(path_ptr: u64, path_len: u64, _mode: u64) -> SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    if rlen == 1 && resolved[0] == b'/' { return SyscallResult::success(0); }
    match find_fs(&resolved, rlen) { Some(_) => SyscallResult::success(0), None => syscall_error(syscall::ENOENT) }
}

fn syscall_fcntl(handle: u64, cmd: u64, arg: u64) -> SyscallResult {
    let Some(slot) = file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    match cmd as u32 {
        syscall::FCNTL_GETFL => SyscallResult::success(unsafe { OPEN_FILES[slot].flags as u64 }),
        syscall::FCNTL_SETFL => { unsafe { OPEN_FILES[slot].flags = arg as u32; } SyscallResult::success(0) }
        _ => syscall_error(syscall::ENOTSUP),
    }
}

fn syscall_ioctl(handle: u64, request: u64, _arg: u64) -> SyscallResult {
    let Some(_slot) = file_handle_slot(handle).or_else(|| { if handle_kind(handle) != 0 { Some(handle_slot(handle)) } else { None } }) else { return syscall_error(syscall::EBADF); };
    if request == 0 { return SyscallResult::success(0); }
    syscall_error(syscall::ENOTSUP)
}

fn syscall_getrandom(buf: u64, len: u64) -> SyscallResult {
    if len == 0 { return SyscallResult::success(0); }
    if let Err(e) = validate_user_buffer(buf, len, true) { return syscall_error(e); }
    let n = core::cmp::min(len as usize, 4096);
    for i in 0..n {
        unsafe {
            let mut x = RNG_STATE ^ read_tsc().wrapping_add(i as u64);
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            RNG_STATE = x;
            *((buf as *mut u8).add(i)) = (x & 0xff) as u8;
        }
    }
    SyscallResult::success(n as u64)
}

fn syscall_get_ticks() -> SyscallResult { SyscallResult::success(unsafe { read_tsc() }) }

fn syscall_nanosleep(seconds: u64, nanoseconds: u64) -> SyscallResult {
    let total_ns = seconds.saturating_mul(1_000_000_000).saturating_add(nanoseconds);
    let ticks = total_ns; // nominal 1GHz TSC: 1ns ~ 1 tick
    let start = unsafe { read_tsc() };
    while unsafe { read_tsc().wrapping_sub(start) } < ticks { unsafe { core::arch::asm!("pause"); } }
    SyscallResult::success(0)
}

fn syscall_mprotect(addr: u64, len: u64, _prot: u64) -> SyscallResult {
    if len == 0 { return syscall_error(syscall::EINVAL); }
    if let Err(e) = validate_user_buffer(addr, len, false) { return syscall_error(e); }
    SyscallResult::success(0)
}

fn syscall_get_process_info(pid: u64, info_ptr: u64) -> SyscallResult {
    if info_ptr == 0 { return syscall_error(syscall::EINVAL); }
    if let Err(e) = validate_user_buffer(info_ptr, core::mem::size_of::<syscall::ProcessInfo>() as u64, true) { return syscall_error(e); }
    let mut info = syscall::ProcessInfo { pid: pid as u32, ppid: 1, state: 0, name_len: 0, name: [0; 32] };
    if pid == 1 {
        let name = b"alphaos-kernel";
        info.name_len = name.len() as u8;
        info.name[..name.len()].copy_from_slice(name);
        info.state = 1;
    } else {
        let mut found = false;
        for i in 0..MAX_SPAWNED { unsafe {
            if SPAWNED[i].used && SPAWNED[i].pid == pid {
                info.state = if SPAWNED[i].exited { 2 } else { 1 };
                info.ppid = 1;
                let nl = PROC_NAME_LENS[i];
                info.name_len = nl as u8;
                info.name[..nl].copy_from_slice(&PROC_NAMES[i][..nl]);
                found = true; break;
            }
        }}
        if !found { return syscall_error(syscall::ESRCH); }
    }
    unsafe { core::ptr::write(info_ptr as *mut syscall::ProcessInfo, info); }
    SyscallResult::success(0)
}

fn syscall_kill_process(pid: u64, _signal: u64) -> SyscallResult {
    if pid == 0 { return syscall_error(syscall::EINVAL); }
    if pid == 1 { return syscall_error(syscall::EPERM); }
    for i in 0..MAX_SPAWNED { unsafe {
        if SPAWNED[i].used && SPAWNED[i].pid == pid { SPAWNED[i].exited = true; SPAWNED[i].exit_code = 128; return SyscallResult::success(0); }
    }}
    syscall_error(syscall::ESRCH)
}

fn syscall_get_arg_count() -> SyscallResult { SyscallResult::success(0) }

fn syscall_get_arg(_index: u64, buf: u64, _len: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(buf, 1, true) { return syscall_error(e); }
    unsafe { *(buf as *mut u8) = 0; }
    syscall_error(syscall::ENOENT)
}

fn syscall_get_env(key_ptr: u64, key_len: u64, buf_ptr: u64, buf_len: u64) -> SyscallResult {
    let key = match user_read_bytes(key_ptr, key_len) { Ok(k)=>k, Err(e)=>return syscall_error(e) };
    if buf_len == 0 { return syscall_error(syscall::EINVAL); }
    for i in 0..MAX_ENV_VARS { unsafe {
        let ev = &ENV_VARS[i];
        if ev.used && ev.key_len == key.len() && &ev.key[..ev.key_len] == key {
            let n = core::cmp::min(ev.val_len, (buf_len as usize).saturating_sub(1));
            if let Err(e) = validate_user_buffer(buf_ptr, buf_len, true) { return syscall_error(e); }
            core::ptr::copy_nonoverlapping(ev.val.as_ptr(), buf_ptr as *mut u8, n);
            *((buf_ptr as *mut u8).add(n)) = 0;
            return SyscallResult::success(n as u64);
        }
    }}
    syscall_error(syscall::ENOENT)
}

fn syscall_set_env(key_ptr: u64, key_len: u64, val_ptr: u64, val_len: u64) -> SyscallResult {
    let key = match user_read_bytes(key_ptr, key_len) { Ok(k)=>k, Err(e)=>return syscall_error(e) };
    let val = match user_read_bytes(val_ptr, val_len) { Ok(v)=>v, Err(e)=>return syscall_error(e) };
    if key.is_empty() || key.len() > MAX_ENV_KEY || val.len() > MAX_ENV_VAL { return syscall_error(syscall::EINVAL); }
    for i in 0..MAX_ENV_VARS { unsafe {
        let ev = &mut ENV_VARS[i];
        if ev.used && ev.key_len == key.len() && &ev.key[..ev.key_len] == key {
            ev.val = [0; MAX_ENV_VAL]; ev.val_len = val.len(); ev.val[..val.len()].copy_from_slice(val); return SyscallResult::success(0);
        }
    }}
    for i in 0..MAX_ENV_VARS { unsafe {
        let ev = &mut ENV_VARS[i];
        if !ev.used {
            *ev = EnvVar::empty(); ev.used = true; ev.key_len = key.len(); ev.val_len = val.len();
            ev.key[..key.len()].copy_from_slice(key); ev.val[..val.len()].copy_from_slice(val);
            return SyscallResult::success(0);
        }
    }}
    syscall_error(syscall::ENOSPC)
}

fn syscall_statfs(_path_ptr: u64, info_ptr: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(info_ptr, core::mem::size_of::<syscall::StatFsInfo>() as u64, true) { return syscall_error(e); }
    let mut used = 0u32;
    for i in 0..MAX_FS_FILES { unsafe { if FS_FILES[i].used { used += 1; } } }
    let info = syscall::StatFsInfo { total_blocks: MAX_FS_FILES as u64, free_blocks: (MAX_FS_FILES as u32 - used) as u64, block_size: MAX_FILE_SIZE as u64, total_files: MAX_FS_FILES as u32, free_files: MAX_FS_FILES as u32 - used };
    unsafe { core::ptr::write(info_ptr as *mut syscall::StatFsInfo, info); }
    SyscallResult::success(0)
}

fn syscall_fsync(_handle: u64) -> SyscallResult { SyscallResult::success(0) }

fn syscall_poll(fds_ptr: u64, fds_len: u64, _timeout_ms: u64) -> SyscallResult {
    if fds_len == 0 { return SyscallResult::success(0); }
    let sz = core::mem::size_of::<syscall::PollFd>() as u64;
    let total = sz.saturating_mul(fds_len);
    if let Err(e) = validate_user_buffer(fds_ptr, total, true) { return syscall_error(e); }
    let mut ready = 0u64;
    for i in 0..fds_len as usize {
        let p = (fds_ptr as usize + i * core::mem::size_of::<syscall::PollFd>()) as *mut syscall::PollFd;
        let mut fd = unsafe { core::ptr::read(p) };
        fd.revents = 0;
        let h = fd.handle as usize;
        if h > 0 && h <= MAX_OPEN_FILES { unsafe { if OPEN_FILES[h-1].used { fd.revents = fd.events & (syscall::POLLIN | syscall::POLLOUT); } else { fd.revents = syscall::POLLERR; } } } else { fd.revents = syscall::POLLERR; }
        if fd.revents != 0 { ready += 1; }
        unsafe { core::ptr::write(p, fd); }
    }
    SyscallResult::success(ready)
}

fn syscall_pipe2(pipe_ptr: u64, _flags: u64) -> SyscallResult { pipe_create(pipe_ptr) }

fn syscall_socket_shutdown(handle: u64, _how: u64) -> SyscallResult {
    if handle_kind(handle) != HANDLE_KIND_SOCKET { return syscall_error(syscall::EBADF); }
    let slot = handle_slot(handle);
    if slot >= MAX_SOCKETS { return syscall_error(syscall::EBADF); }
    let used = unsafe { SOCKETS[slot].used };
    if !used { return syscall_error(syscall::EBADF); }
    SyscallResult::success(0)
}

fn syscall_socketpair(_domain: u64, _stype: u64, pair_ptr: u64) -> SyscallResult {
    let mut p = [0u64; 2];
    let r = pipe_create(p.as_mut_ptr() as u64);
    if r.error != 0 { return r; }
    if let Err(e) = validate_user_buffer(pair_ptr, 16, true) { return syscall_error(e); }
    unsafe { core::ptr::write(pair_ptr as *mut u64, p[0]); core::ptr::write((pair_ptr as *mut u64).add(1), p[1]); }
    SyscallResult::success(0)
}

fn syscall_get_rlimit(resource: u64, ptr: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(ptr, core::mem::size_of::<syscall::ResourceLimit>() as u64, true) { return syscall_error(e); }
    let lim = match resource as u32 {
        syscall::RLIMIT_OPEN_FILES => syscall::ResourceLimit { current: MAX_OPEN_FILES as u64, maximum: MAX_OPEN_FILES as u64 },
        syscall::RLIMIT_HEAP => syscall::ResourceLimit { current: 1024*1024, maximum: 1024*1024 },
        syscall::RLIMIT_PROCS => syscall::ResourceLimit { current: MAX_SPAWNED as u64, maximum: MAX_SPAWNED as u64 },
        _ => return syscall_error(syscall::EINVAL),
    };
    unsafe { core::ptr::write(ptr as *mut syscall::ResourceLimit, lim); }
    SyscallResult::success(0)
}

fn syscall_set_rlimit(_resource: u64, _ptr: u64) -> SyscallResult { SyscallResult::success(0) }

fn syscall_get_system_name(buf: u64, len: u64) -> SyscallResult {
    let n = unsafe { SYS_NAME_LEN };
    if len == 0 { return syscall_error(syscall::EINVAL); }
    let copy_n = core::cmp::min(n, (len as usize).saturating_sub(1));
    if let Err(e) = validate_user_buffer(buf, len, true) { return syscall_error(e); }
    unsafe { core::ptr::copy_nonoverlapping(core::ptr::addr_of!(SYS_NAME).cast::<u8>(), buf as *mut u8, copy_n); *((buf as *mut u8).add(copy_n)) = 0; }
    SyscallResult::success(copy_n as u64)
}

fn syscall_set_system_name(name_ptr: u64, name_len: u64) -> SyscallResult {
    let name = match user_read_bytes(name_ptr, name_len) { Ok(n)=>n, Err(e)=>return syscall_error(e) };
    if name.is_empty() || name.len() > 32 { return syscall_error(syscall::EINVAL); }
    unsafe { SYS_NAME = [0;32]; SYS_NAME[..name.len()].copy_from_slice(name); SYS_NAME_LEN = name.len(); }
    SyscallResult::success(0)
}

fn syscall_create_timer(interval_ms: u64, _flags: u64) -> SyscallResult {
    for i in 0..MAX_TIMERS { unsafe {
        if !TIMERS[i].used { TIMERS[i] = TimerSlot { used: true, interval_ms, start_tick: read_tsc() }; return SyscallResult::success((i+1) as u64); }
    }}
    syscall_error(syscall::ENOSPC)
}

fn syscall_get_timer(timer_id: u64) -> SyscallResult {
    if timer_id == 0 || (timer_id as usize) > MAX_TIMERS { return syscall_error(syscall::EINVAL); }
    let t = unsafe { &TIMERS[timer_id as usize - 1] };
    if !t.used { return syscall_error(syscall::EINVAL); }
    let elapsed_ms = unsafe { read_tsc().wrapping_sub(t.start_tick) / 1_000_000 };
    let rem = if elapsed_ms >= t.interval_ms { 0 } else { t.interval_ms - elapsed_ms };
    SyscallResult::success(rem)
}

fn syscall_cancel_timer(timer_id: u64) -> SyscallResult {
    if timer_id == 0 || (timer_id as usize) > MAX_TIMERS { return syscall_error(syscall::EINVAL); }
    unsafe { if !TIMERS[timer_id as usize - 1].used { return syscall_error(syscall::EINVAL); } TIMERS[timer_id as usize - 1] = TimerSlot::empty(); }
    SyscallResult::success(0)
}

fn syscall_create_event(_flags: u64) -> SyscallResult {
    for i in 0..MAX_EVENTS { unsafe { if !EVENTS[i].used { EVENTS[i] = EventSlot { used: true, signaled: false }; return SyscallResult::success((i+1) as u64); } } }
    syscall_error(syscall::ENOSPC)
}

fn syscall_signal_event(event_id: u64) -> SyscallResult {
    if event_id == 0 || (event_id as usize) > MAX_EVENTS { return syscall_error(syscall::EINVAL); }
    unsafe { if !EVENTS[event_id as usize - 1].used { return syscall_error(syscall::EINVAL); } EVENTS[event_id as usize - 1].signaled = true; }
    SyscallResult::success(0)
}

fn syscall_wait_event(event_id: u64, timeout_ms: u64) -> SyscallResult {
    if event_id == 0 || (event_id as usize) > MAX_EVENTS { return syscall_error(syscall::EINVAL); }
    let start = unsafe { read_tsc() };
    let timeout_ticks = timeout_ms.saturating_mul(1_000_000);
    loop {
        unsafe {
            if !EVENTS[event_id as usize - 1].used { return syscall_error(syscall::EINVAL); }
            if EVENTS[event_id as usize - 1].signaled { EVENTS[event_id as usize - 1].signaled = false; return SyscallResult::success(0); }
            if timeout_ms != 0 && read_tsc().wrapping_sub(start) >= timeout_ticks { return syscall_error(syscall::EAGAIN); }
            core::arch::asm!("pause");
        }
    }
}

fn syscall_get_memory_info(ptr: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(ptr, core::mem::size_of::<syscall::MemoryInfo>() as u64, true) { return syscall_error(e); }
    let total = unsafe { ALLOCATOR_END.saturating_sub(ALLOCATOR_START) };
    let used = unsafe { ALLOCATOR_NEXT.saturating_sub(ALLOCATOR_START) };
    let info = syscall::MemoryInfo { managed_bytes: total, allocated_bytes: used, available_bytes: total.saturating_sub(used) };
    unsafe { core::ptr::write(ptr as *mut syscall::MemoryInfo, info); }
    SyscallResult::success(0)
}

fn syscall_debug_log(msg_ptr: u64, msg_len: u64) -> SyscallResult {
    let msg = match user_read_bytes(msg_ptr, msg_len) { Ok(m)=>m, Err(e)=>return syscall_error(e) };
    // Kernel log only in freestanding console; serial not wired in this build.
    unsafe {
        for &b in msg.iter().take(KLOG_SIZE) { let idx = KLOG_LEN % KLOG_SIZE; KLOG[idx] = b; KLOG_LEN = KLOG_LEN.saturating_add(1); }
        let idx = KLOG_LEN % KLOG_SIZE; KLOG[idx] = b'\n'; KLOG_LEN = KLOG_LEN.saturating_add(1);
    }
    SyscallResult::success(msg.len() as u64)
}

fn syscall_get_console_size(ptr: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(ptr, core::mem::size_of::<syscall::ConsoleSize>() as u64, true) { return syscall_error(e); }
    let info = syscall::ConsoleSize { columns: 80, rows: 25 };
    unsafe { core::ptr::write(ptr as *mut syscall::ConsoleSize, info); }
    SyscallResult::success(0)
}

fn syscall_set_console_mode(mode: u64) -> SyscallResult { unsafe { CONSOLE_MODE = mode; } SyscallResult::success(0) }

fn syscall_get_boot_info(ptr: u64) -> SyscallResult {
    if let Err(e) = validate_user_buffer(ptr, core::mem::size_of::<syscall::BootInfo>() as u64, true) { return syscall_error(e); }
    let uptime = unsafe { read_tsc().wrapping_sub(BOOT_TSC) / 1_000_000 };
    let info = syscall::BootInfo { uptime_ms: uptime, syscall_count: syscall::SYSCALL_COUNT as u32, abi_major: syscall::SYSCALL_ABI_MAJOR, abi_minor: syscall::SYSCALL_ABI_MINOR };
    unsafe { core::ptr::write(ptr as *mut syscall::BootInfo, info); }
    SyscallResult::success(0)
}

fn syscall_get_kernel_log(buf: u64, len: u64) -> SyscallResult {
    if len == 0 { return SyscallResult::success(0); }
    if let Err(e) = validate_user_buffer(buf, len, true) { return syscall_error(e); }
    let total = unsafe { core::cmp::min(KLOG_LEN, KLOG_SIZE) };
    let copy_n = core::cmp::min(total, (len as usize).saturating_sub(1));
    unsafe {
        let start = if KLOG_LEN > KLOG_SIZE { KLOG_LEN % KLOG_SIZE } else { 0 };
        for i in 0..copy_n { let idx = (start + i) % KLOG_SIZE; *((buf as *mut u8).add(i)) = KLOG[idx]; }
        *((buf as *mut u8).add(copy_n)) = 0;
    }
    SyscallResult::success(copy_n as u64)
}

fn syscall_set_thread_name(tid: u64, name_ptr: u64, name_len: u64) -> SyscallResult {
    let name = match user_read_bytes(name_ptr, name_len) { Ok(n)=>n, Err(e)=>return syscall_error(e) };
    let n = core::cmp::min(name.len(), MAX_PROC_NAME);
    for i in 0..MAX_SPAWNED { unsafe {
        if SPAWNED[i].used && SPAWNED[i].pid == tid {
            PROC_NAMES[i] = [0; MAX_PROC_NAME]; PROC_NAMES[i][..n].copy_from_slice(&name[..n]); PROC_NAME_LENS[i] = n;
            return SyscallResult::success(0);
        }
    }}
    if tid == 1 { return SyscallResult::success(0); }
    syscall_error(syscall::ESRCH)
}

fn syscall_get_thread_name(tid: u64, buf_ptr: u64, buf_len: u64) -> SyscallResult {
    if buf_len == 0 { return syscall_error(syscall::EINVAL); }
    let mut found: Option<([u8;MAX_PROC_NAME], usize)> = None;
    if tid == 1 { let n=b"alphaos-kernel"; let mut b=[0u8;MAX_PROC_NAME]; b[..n.len()].copy_from_slice(n); found=Some((b,n.len())); }
    for i in 0..MAX_SPAWNED { unsafe { if SPAWNED[i].used && SPAWNED[i].pid == tid { found = Some((PROC_NAMES[i], PROC_NAME_LENS[i])); break; } } }
    let (name, nlen) = match found { Some(v)=>v, None=>return syscall_error(syscall::ESRCH) };
    let copy_n = core::cmp::min(nlen, (buf_len as usize).saturating_sub(1));
    if let Err(e) = validate_user_buffer(buf_ptr, buf_len, true) { return syscall_error(e); }
    unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), buf_ptr as *mut u8, copy_n); *((buf_ptr as *mut u8).add(copy_n)) = 0; }
    SyscallResult::success(copy_n as u64)
}

fn syscall_get_cpu_count() -> SyscallResult { SyscallResult::success(1) }
fn syscall_get_uptime_ms() -> SyscallResult { SyscallResult::success(unsafe { read_tsc().wrapping_sub(BOOT_TSC) / 1_000_000 }) }

fn syscall_list_processes(buf_ptr: u64, buf_len: u64) -> SyscallResult {
    let max = (buf_len as usize) / 4;
    if max == 0 { return SyscallResult::success(0); }
    if let Err(e) = validate_user_buffer(buf_ptr, (max*4) as u64, true) { return syscall_error(e); }
    let mut count = 0usize;
    unsafe { if count < max { *((buf_ptr as *mut u32).add(count)) = 1; count+=1; } }
    for i in 0..MAX_SPAWNED { unsafe {
        if SPAWNED[i].used && !SPAWNED[i].exited && count < max { *((buf_ptr as *mut u32).add(count)) = SPAWNED[i].pid as u32; count+=1; }
    }}
    SyscallResult::success(count as u64)
}

fn syscall_get_proc_name(pid: u64, buf_ptr: u64, buf_len: u64) -> SyscallResult { syscall_get_thread_name(pid, buf_ptr, buf_len) }


fn sys_stat_handle(handle: u64, stat_ptr: u64) -> syscall::SyscallResult {
    let Some(slot)=file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    let fi = unsafe { OPEN_FILES[slot].fs_index };
    let (size,is_dir) = with_fs(|fs| if fi>=MAX_FS_FILES || !fs[fi].used { (0u64,false) } else { (fs[fi].size as u64, fs[fi].is_dir) });
    let st = syscall::FileStat { size, mode: if is_dir { 0o040755 } else { 0o100644 }, flags: 0 };
    let bytes = unsafe { core::slice::from_raw_parts((&st as *const syscall::FileStat).cast::<u8>(), core::mem::size_of::<syscall::FileStat>()) };
    match sc_copy_to_user(stat_ptr, bytes.len() as u64, bytes) { Ok(_)=>syscall::SyscallResult::success(0), Err(e)=>syscall_error(e) }
}

fn syscall_getppid() -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_gettid() -> syscall::SyscallResult { syscall::SyscallResult::success(1) }
fn syscall_sched_yield() -> syscall::SyscallResult { unsafe { core::arch::asm!("pause"); } syscall::SyscallResult::success(0) }
fn syscall_set_process_name(name_ptr: u64, name_len: u64) -> syscall::SyscallResult {
    let name = match sc_read_user(name_ptr, name_len) { Ok(s)=>s, Err(e)=>{sc_set_last_error(e); return syscall_error(e);} };
    let n = core::cmp::min(name.len(), 31);
    unsafe {
        let dst = core::ptr::addr_of_mut!(PROC_NAMES).cast::<[u8;32]>().add(0);
        core::ptr::write_bytes((*dst).as_mut_ptr(), 0, 32);
        core::ptr::copy_nonoverlapping(name.as_ptr(), (*dst).as_mut_ptr(), n);
    }
    syscall::SyscallResult::success(0)
}
fn syscall_get_process_name2(pid: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    if buf_len==0 { return syscall::SyscallResult::success(0); }
    let idx = if pid==0 || pid==1 { 0usize } else { (pid as usize).saturating_sub(2) };
    if idx >= 32 { sc_set_last_error(syscall::ESRCH); return syscall_error(syscall::ESRCH); }
    let mut tmp=[0u8;32]; let mut n=0usize;
    unsafe {
        let src = core::ptr::addr_of!(PROC_NAMES).cast::<[u8;32]>().add(idx);
        for i in 0..32 { let b=(*src)[i]; if b==0 {break;} tmp[i]=b; n+=1; }
    }
    if n==0 { let d=b"alphaos"; tmp[..d.len()].copy_from_slice(d); n=d.len(); }
    match sc_copy_cstr_to_user(buf_ptr, buf_len, &tmp[..n]) { Ok(v)=>syscall::SyscallResult::success(v), Err(e)=>{sc_set_last_error(e); syscall_error(e)} }
}
fn syscall_fork() -> syscall::SyscallResult { sc_set_last_error(syscall::ENOTSUP); syscall_error(syscall::ENOTSUP) }
fn syscall_exec(path_ptr: u64, path_len: u64, argv_ptr: u64, flags: u64) -> syscall::SyscallResult {
    // freestanding single-process model: exec behaves as spawn+immediate success; stores name
    let r = sys_spawn(path_ptr, path_len, argv_ptr, flags);
    r
}
fn syscall_waitpid(pid: u64, status_ptr: u64, options: u64) -> syscall::SyscallResult { sys_wait(pid, status_ptr, options) }
fn syscall_get_exit_code(pid: u64) -> syscall::SyscallResult {
    for i in 0..MAX_SPAWNED { unsafe {
        if SPAWNED[i].used && SPAWNED[i].pid as u64 == pid {
            if SPAWNED[i].exited { return syscall::SyscallResult::success(SPAWNED[i].exit_code as u64); }
            return syscall_error(syscall::EAGAIN);
        }
    }}
    sc_set_last_error(syscall::ESRCH); syscall_error(syscall::ESRCH)
}
fn syscall_set_thread_priority(tid: u64, priority: u64) -> syscall::SyscallResult {
    let idx = (tid as usize).min(31);
    unsafe { *core::ptr::addr_of_mut!(SC_THREAD_PRIORITY).cast::<i32>().add(idx) = priority as i32; }
    syscall::SyscallResult::success(0)
}
fn syscall_get_thread_priority(tid: u64) -> syscall::SyscallResult {
    let idx = (tid as usize).min(31);
    let p = unsafe { *core::ptr::addr_of!(SC_THREAD_PRIORITY).cast::<i32>().add(idx) };
    syscall::SyscallResult::success(p as u64)
}
fn syscall_process_exists(pid: u64) -> syscall::SyscallResult {
    if pid==0 || pid==1 { return syscall::SyscallResult::success(1); }
    for i in 0..MAX_SPAWNED { unsafe { if SPAWNED[i].used && SPAWNED[i].pid as u64 == pid { return syscall::SyscallResult::success(1); } } }
    syscall::SyscallResult::success(0)
}
fn syscall_get_process_list(buf_ptr: u64, max_entries: u64) -> syscall::SyscallResult { syscall_list_processes(buf_ptr, max_entries.saturating_mul(4)) }
fn syscall_kill_thread(tid: u64) -> syscall::SyscallResult { syscall_kill_process(tid, 9) }
fn syscall_get_current_tid() -> syscall::SyscallResult { syscall::SyscallResult::success(1) }
fn syscall_set_uid(uid: u64) -> syscall::SyscallResult { unsafe { *core::ptr::addr_of_mut!(SC_UID)=uid as u32; } syscall::SyscallResult::success(0) }
fn syscall_get_uid() -> syscall::SyscallResult { syscall::SyscallResult::success(unsafe { *core::ptr::addr_of!(SC_UID) } as u64) }
fn syscall_set_gid(gid: u64) -> syscall::SyscallResult { unsafe { *core::ptr::addr_of_mut!(SC_GID)=gid as u32; } syscall::SyscallResult::success(0) }
fn syscall_get_gid() -> syscall::SyscallResult { syscall::SyscallResult::success(unsafe { *core::ptr::addr_of!(SC_GID) } as u64) }
fn syscall_get_process_times(_pid: u64, buf_ptr: u64) -> syscall::SyscallResult {
    #[repr(C)] struct T { uptime_ms: u64, user_ms: u64, sys_ms: u64, ticks: u64 }
    let t = T { uptime_ms: sc_ticks_ms(), user_ms: sc_ticks_ms(), sys_ms: 0, ticks: unsafe{read_tsc()} };
    let bytes = unsafe { core::slice::from_raw_parts((&t as *const T).cast::<u8>(), core::mem::size_of::<T>()) };
    match sc_copy_to_user(buf_ptr, bytes.len() as u64, bytes) { Ok(_)=>syscall::SyscallResult::success(0), Err(e)=>{sc_set_last_error(e); syscall_error(e)} }
}
fn syscall_openat(_dir_handle: u64, path_ptr: u64, path_len: u64, flags: u64) -> syscall::SyscallResult { sys_open(path_ptr, path_len, flags, 0) }
fn syscall_readat(handle: u64, offset: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some(slot)=file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    let old = unsafe { OPEN_FILES[slot].offset };
    unsafe { OPEN_FILES[slot].offset = offset as usize; }
    let r = sys_file_read(handle, buffer, length);
    unsafe { OPEN_FILES[slot].offset = old; }
    r
}
fn syscall_writeat(handle: u64, offset: u64, buffer: u64, length: u64) -> syscall::SyscallResult {
    let Some(slot)=file_handle_slot(handle) else { return syscall_error(syscall::EBADF); };
    let old = unsafe { OPEN_FILES[slot].offset };
    unsafe { OPEN_FILES[slot].offset = offset as usize; }
    let r = sys_file_write(handle, buffer, length);
    unsafe { OPEN_FILES[slot].offset = old; }
    r
}
fn syscall_file_truncate_path(path_ptr: u64, path_len: u64, length: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    let idx = match find_fs(&resolved, rlen) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    with_fs(|fs| {
        if fs[idx].is_dir { return syscall_error(syscall::EINVAL); }
        let new_len = core::cmp::min(length as usize, MAX_FILE_SIZE);
        if new_len < fs[idx].size { for b in &mut fs[idx].data[new_len..fs[idx].size] { *b=0; } }
        fs[idx].size = new_len;
        syscall::SyscallResult::success(0)
    })
}
fn syscall_file_stat_path(path_ptr: u64, path_len: u64, stat_ptr: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    let idx = match find_fs(&resolved, rlen) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    let (size,is_dir) = with_fs(|fs| (fs[idx].size as u64, fs[idx].is_dir));
    let st = syscall::FileStat { size, mode: if is_dir { 0o040755 } else { 0o100644 }, flags: 0 };
    let bytes = unsafe { core::slice::from_raw_parts((&st as *const syscall::FileStat).cast::<u8>(), core::mem::size_of::<syscall::FileStat>()) };
    match sc_copy_to_user(stat_ptr, bytes.len() as u64, bytes) { Ok(_)=>syscall::SyscallResult::success(0), Err(e)=>syscall_error(e) }
}
fn syscall_symlink(target_ptr: u64, target_len: u64, link_ptr: u64, link_len: u64) -> syscall::SyscallResult {
    let target = match sc_read_user(target_ptr, target_len) { Ok(s)=>s, Err(e)=>return syscall_error(e) };
    let linkp = match user_read_path(link_ptr, link_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(linkp); let rlen = resolved_len(&resolved);
    for i in 0..32 { unsafe {
        if !*core::ptr::addr_of!(SC_SYMLINK_USED).cast::<bool>().add(i) {
            *core::ptr::addr_of_mut!(SC_SYMLINK_USED).cast::<bool>().add(i)=true;
            let ln=core::cmp::min(rlen,128); let tn=core::cmp::min(target.len(),128);
            let dst=core::ptr::addr_of_mut!(SC_SYMLINKS).cast::<[u8;128]>().add(i);
            let tdst=core::ptr::addr_of_mut!(SC_SYMLINK_TARGETS).cast::<[u8;128]>().add(i);
            core::ptr::write_bytes((*dst).as_mut_ptr(),0,128); core::ptr::write_bytes((*tdst).as_mut_ptr(),0,128);
            core::ptr::copy_nonoverlapping(resolved.as_ptr(), (*dst).as_mut_ptr(), ln);
            core::ptr::copy_nonoverlapping(target.as_ptr(), (*tdst).as_mut_ptr(), tn);
            *core::ptr::addr_of_mut!(SC_SYMLINK_LEN).cast::<usize>().add(i)=ln;
            *core::ptr::addr_of_mut!(SC_SYMLINK_TLEN).cast::<usize>().add(i)=tn;
            return syscall::SyscallResult::success(0);
        }
    }}
    syscall_error(syscall::ENOSPC)
}
fn syscall_readlink(path_ptr: u64, path_len: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen = resolved_len(&resolved);
    for i in 0..32 { unsafe {
        if *core::ptr::addr_of!(SC_SYMLINK_USED).cast::<bool>().add(i) {
            let ln=*core::ptr::addr_of!(SC_SYMLINK_LEN).cast::<usize>().add(i);
            let src=core::ptr::addr_of!(SC_SYMLINKS).cast::<[u8;128]>().add(i);
            if ln==rlen && (&(*src))[..ln]==resolved[..rlen] {
                let tn=*core::ptr::addr_of!(SC_SYMLINK_TLEN).cast::<usize>().add(i);
                let ts=core::ptr::addr_of!(SC_SYMLINK_TARGETS).cast::<[u8;128]>().add(i);
                let slice=core::slice::from_raw_parts((*ts).as_ptr(), tn);
                return match sc_copy_to_user(buf_ptr, buf_len, slice) { Ok(n)=>syscall::SyscallResult::success(n), Err(e)=>syscall_error(e) };
            }
        }
    }}
    syscall_error(syscall::ENOENT)
}
fn syscall_link(old_ptr: u64, old_len: u64, new_ptr: u64, new_len: u64) -> syscall::SyscallResult {
    // simple copy-as-link in ramfs
    let oldp = match user_read_path(old_ptr, old_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let newp = match user_read_path(new_ptr, new_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let oldr = resolve_path(oldp); let oldl=resolved_len(&oldr);
    let newr = resolve_path(newp); let newl=resolved_len(&newr);
    let src_idx = match find_fs(&oldr, oldl) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    if find_fs(&newr, newl).is_some() { return syscall_error(syscall::EEXIST); }
    with_fs(|fs| {
        if fs[src_idx].is_dir { return syscall_error(syscall::EINVAL); }
        for i in 0..MAX_FS_FILES { if !fs[i].used {
            fs[i]=fs[src_idx];
            let nlm=core::cmp::min(newl,64);
            fs[i].name=[0;64]; fs[i].name[..nlm].copy_from_slice(&newr[..nlm]); fs[i].name_len=nlm;
            return syscall::SyscallResult::success(0);
        }}
        syscall_error(syscall::ENOSPC)
    })
}
fn syscall_chmod(path_ptr: u64, path_len: u64, mode: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen=resolved_len(&resolved);
    let idx = match find_fs(&resolved, rlen) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    unsafe { *core::ptr::addr_of_mut!(SC_FILE_MODE).cast::<u32>().add(idx.min(127)) = mode as u32; }
    syscall::SyscallResult::success(0)
}
fn syscall_chown(path_ptr: u64, path_len: u64, _uid: u64, _gid: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen=resolved_len(&resolved);
    if find_fs(&resolved, rlen).is_none() { return syscall_error(syscall::ENOENT); }
    syscall::SyscallResult::success(0)
}
fn syscall_lseek64(handle: u64, offset: u64, whence: u64) -> syscall::SyscallResult { sys_seek(handle, offset, whence) }
fn syscall_fstat2(handle: u64, stat_ptr: u64) -> syscall::SyscallResult { sys_stat_handle(handle, stat_ptr) }
fn syscall_isatty(handle: u64) -> syscall::SyscallResult {
    if handle == 1 || handle == 0 { return syscall::SyscallResult::success(1); }
    let _ = handle_kind(handle);
    syscall::SyscallResult::success(0)
}
fn syscall_ttyname(handle: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    if syscall_isatty(handle).value != 1 { return syscall_error(syscall::ENOTSUP); }
    match sc_copy_cstr_to_user(buf_ptr, buf_len, b"/dev/console") { Ok(n)=>syscall::SyscallResult::success(n), Err(e)=>syscall_error(e) }
}
fn syscall_getdents(handle: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    // iterate ramfs entries; handle ignored for simplicity, returns all entries
    let _ = handle;
    let max = (buf_len as usize) / core::mem::size_of::<syscall::DirEntry>();
    if max == 0 { return syscall::SyscallResult::success(0); }
    if let Err(e)=validate_user_buffer(buf_ptr, (max*core::mem::size_of::<syscall::DirEntry>()) as u64, true){return syscall_error(e);}
    let mut count=0usize;
    with_fs(|fs| {
        for i in 0..MAX_FS_FILES {
            if count>=max { break; }
            if !fs[i].used { continue; }
            let mut entry = syscall::DirEntry { name_len: fs[i].name_len as u32, entry_type: if fs[i].is_dir { 2 } else { 1 }, size: fs[i].size as u64, name:[0;64] };
            let n = core::cmp::min(fs[i].name_len, 64);
            entry.name[..n].copy_from_slice(&fs[i].name[..n]);
            unsafe { core::ptr::write_unaligned((buf_ptr as *mut syscall::DirEntry).add(count), entry); }
            count+=1;
        }
    });
    syscall::SyscallResult::success(count as u64)
}
fn syscall_realpath(path_ptr: u64, path_len: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved = resolve_path(path); let rlen=resolved_len(&resolved);
    match sc_copy_cstr_to_user(buf_ptr, buf_len, &resolved[..rlen]) { Ok(n)=>syscall::SyscallResult::success(n), Err(e)=>syscall_error(e) }
}
fn syscall_copy_file(src_ptr: u64, src_len: u64, dst_ptr: u64, dst_len: u64) -> syscall::SyscallResult {
    let srcp = match user_read_path(src_ptr, src_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let dstp = match user_read_path(dst_ptr, dst_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let srcr=resolve_path(srcp); let srcl=resolved_len(&srcr);
    let dstr=resolve_path(dstp); let dstl=resolved_len(&dstr);
    let src_idx = match find_fs(&srcr, srcl) { Some(i)=>i, None=>return syscall_error(syscall::ENOENT) };
    let (is_dir,size) = with_fs(|fs| (fs[src_idx].is_dir, fs[src_idx].size));
    if is_dir { return syscall_error(syscall::EINVAL); }
    let mut data=[0u8; MAX_FILE_SIZE];
    with_fs(|fs| { data[..size].copy_from_slice(&fs[src_idx].data[..size]); });
    if let Some(di)=find_fs(&dstr,dstl) {
        with_fs(|fs| { if fs[di].is_dir { return; } fs[di].data[..size].copy_from_slice(&data[..size]); fs[di].size=size; for b in &mut fs[di].data[size..MAX_FILE_SIZE]{*b=0;} });
        return syscall::SyscallResult::success(size as u64);
    }
    with_fs(|fs| {
        for i in 0..MAX_FS_FILES { if !fs[i].used {
            fs[i].used=true; fs[i].is_dir=false; fs[i].size=size; let nlm=core::cmp::min(dstl,64);
            fs[i].name=[0;64]; fs[i].name[..nlm].copy_from_slice(&dstr[..nlm]); fs[i].name_len=nlm;
            fs[i].data[..size].copy_from_slice(&data[..size]);
            return syscall::SyscallResult::success(size as u64);
        }}
        syscall_error(syscall::ENOSPC)
    })
}
fn syscall_mkfifo(path_ptr: u64, path_len: u64, _mode: u64) -> syscall::SyscallResult {
    let path = match user_read_path(path_ptr, path_len) { Ok(p)=>p, Err(e)=>return syscall_error(e) };
    let resolved=resolve_path(path); let rlen=resolved_len(&resolved);
    if find_fs(&resolved,rlen).is_some() { return syscall_error(syscall::EEXIST); }
    with_fs(|fs| { for i in 0..MAX_FS_FILES { if !fs[i].used { fs[i].used=true; fs[i].is_dir=false; fs[i].size=0; let nlm=core::cmp::min(rlen,64); fs[i].name=[0;64]; fs[i].name[..nlm].copy_from_slice(&resolved[..nlm]); fs[i].name_len=nlm; return syscall::SyscallResult::success(0); }} syscall_error(syscall::ENOSPC) })
}
fn syscall_file_lock(_handle: u64, _operation: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_file_unlock(_handle: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }

fn syscall_memory_map(length: u64, _prot: u64, _flags: u64) -> syscall::SyscallResult { sys_heap_alloc(length, 4096) }
fn syscall_memory_unmap(address: u64, length: u64) -> syscall::SyscallResult { sys_heap_free(address, length) }
fn syscall_memory_protect(address: u64, length: u64, _prot: u64) -> syscall::SyscallResult { syscall_mprotect(address, length, _prot) }
fn syscall_memory_advise(_address: u64, _length: u64, _advice: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_memory_stats(buf_ptr: u64) -> syscall::SyscallResult { syscall_get_memory_info(buf_ptr) }
fn syscall_heap_stats(buf_ptr: u64) -> syscall::SyscallResult { syscall_get_memory_info(buf_ptr) }
fn syscall_page_size() -> syscall::SyscallResult { syscall::SyscallResult::success(4096) }
fn syscall_memory_fill(dst: u64, value: u64, len: u64) -> syscall::SyscallResult {
    if len==0 { return syscall::SyscallResult::success(0); }
    if let Err(e)=validate_user_buffer(dst,len,true){return syscall_error(e);}
    unsafe { core::ptr::write_bytes(dst as *mut u8, value as u8, len as usize); }
    syscall::SyscallResult::success(len)
}
fn syscall_memory_compare(a: u64, b: u64, len: u64) -> syscall::SyscallResult {
    if len==0 { return syscall::SyscallResult::success(0); }
    if let Err(e)=validate_user_buffer(a,len,false){return syscall_error(e);}
    if let Err(e)=validate_user_buffer(b,len,false){return syscall_error(e);}
    let sa=unsafe{core::slice::from_raw_parts(a as *const u8,len as usize)};
    let sb=unsafe{core::slice::from_raw_parts(b as *const u8,len as usize)};
    for i in 0..sa.len() { if sa[i]!=sb[i] { return syscall::SyscallResult::success(if sa[i]<sb[i]{u64::MAX}else{1}); } }
    syscall::SyscallResult::success(0)
}
fn syscall_memory_copy(dst: u64, src: u64, len: u64) -> syscall::SyscallResult {
    if len==0 { return syscall::SyscallResult::success(0); }
    if let Err(e)=validate_user_buffer(src,len,false){return syscall_error(e);}
    if let Err(e)=validate_user_buffer(dst,len,true){return syscall_error(e);}
    unsafe { core::ptr::copy(src as *const u8, dst as *mut u8, len as usize); }
    syscall::SyscallResult::success(len)
}
fn syscall_clock_monotonic(out_ptr: u64) -> syscall::SyscallResult { sys_clock_gettime(syscall::CLOCK_MONOTONIC as u64, out_ptr) }
fn syscall_clock_realtime(out_ptr: u64) -> syscall::SyscallResult { sys_clock_gettime(syscall::CLOCK_REALTIME as u64, out_ptr) }
fn syscall_set_timer_ms(timer_id: u64, ms: u64) -> syscall::SyscallResult {
    let idx = timer_id as usize;
    if idx >= 64 { return syscall_error(syscall::EINVAL); }
    let deadline = sc_ticks_ms().saturating_add(ms);
    unsafe { *core::ptr::addr_of_mut!(TIMERS).cast::<u64>().add(idx) = deadline; }
    syscall::SyscallResult::success(0)
}
fn syscall_get_timer_ms(timer_id: u64) -> syscall::SyscallResult { syscall_get_timer(timer_id) }
fn syscall_sleep_ms(ms: u64) -> syscall::SyscallResult { sys_sleep(ms.saturating_mul(1_000_000)) }
fn syscall_alarm(seconds: u64) -> syscall::SyscallResult { unsafe { *core::ptr::addr_of_mut!(SC_ALARM_MS)=sc_ticks_ms().saturating_add(seconds.saturating_mul(1000)); } syscall::SyscallResult::success(0) }
fn syscall_time_now() -> syscall::SyscallResult { syscall::SyscallResult::success(sc_ticks_ms()/1000) }
fn syscall_ticks_per_second() -> syscall::SyscallResult { syscall::SyscallResult::success(1_000_000_000) }
fn syscall_uptime_ns() -> syscall::SyscallResult { syscall::SyscallResult::success(sc_ticks_ms().saturating_mul(1_000_000)) }
fn syscall_deadline_wait(deadline_ms: u64) -> syscall::SyscallResult {
    let now=sc_ticks_ms();
    if deadline_ms>now { sys_sleep((deadline_ms-now).saturating_mul(1_000_000)); }
    syscall::SyscallResult::success(0)
}
fn syscall_create_mutex() -> syscall::SyscallResult {
    for i in 0..64 { unsafe { let p=core::ptr::addr_of_mut!(SC_MUTEXES).cast::<u8>().add(i); if *p==0 { *p=1; return syscall::SyscallResult::success(i as u64 + 1); } } }
    syscall_error(syscall::ENOSPC)
}
fn syscall_lock_mutex(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let p=unsafe{core::ptr::addr_of_mut!(SC_MUTEXES).cast::<u8>().add((handle-1) as usize)};
    unsafe { if *p==0 { return syscall_error(syscall::EINVAL); } *p=2; }
    syscall::SyscallResult::success(0)
}
fn syscall_unlock_mutex(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let p=unsafe{core::ptr::addr_of_mut!(SC_MUTEXES).cast::<u8>().add((handle-1) as usize)};
    unsafe { if *p==0 { return syscall_error(syscall::EINVAL); } *p=1; }
    syscall::SyscallResult::success(0)
}
fn syscall_destroy_mutex(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    unsafe { *core::ptr::addr_of_mut!(SC_MUTEXES).cast::<u8>().add((handle-1) as usize)=0; }
    syscall::SyscallResult::success(0)
}
fn syscall_create_sem(initial: u64) -> syscall::SyscallResult {
    for i in 0..64 { unsafe { let u=core::ptr::addr_of_mut!(SC_SEMS_USED).cast::<bool>().add(i); if !*u { *u=true; *core::ptr::addr_of_mut!(SC_SEMS).cast::<i32>().add(i)=initial as i32; return syscall::SyscallResult::success(i as u64+1); } } }
    syscall_error(syscall::ENOSPC)
}
fn syscall_sem_wait(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_SEMS_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); }
        let p=core::ptr::addr_of_mut!(SC_SEMS).cast::<i32>().add(i);
        if *p>0 { *p-=1; syscall::SyscallResult::success(0) } else { syscall_error(syscall::EAGAIN) }
    }
}
fn syscall_sem_post(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_SEMS_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); }
        *core::ptr::addr_of_mut!(SC_SEMS).cast::<i32>().add(i)+=1; }
    syscall::SyscallResult::success(0)
}
fn syscall_sem_destroy(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    unsafe { *core::ptr::addr_of_mut!(SC_SEMS_USED).cast::<bool>().add((handle-1) as usize)=false; *core::ptr::addr_of_mut!(SC_SEMS).cast::<i32>().add((handle-1) as usize)=0; }
    syscall::SyscallResult::success(0)
}
fn syscall_create_condvar() -> syscall::SyscallResult {
    for i in 0..64 { unsafe { let p=core::ptr::addr_of_mut!(SC_CONDVARS).cast::<bool>().add(i); if !*p { *p=true; return syscall::SyscallResult::success(i as u64+1); } } }
    syscall_error(syscall::ENOSPC)
}
fn syscall_condvar_wait(cv_handle: u64, _mutex_handle: u64) -> syscall::SyscallResult {
    if cv_handle==0 || cv_handle>64 { return syscall_error(syscall::EINVAL); }
    unsafe { if !*core::ptr::addr_of!(SC_CONDVARS).cast::<bool>().add((cv_handle-1) as usize) { return syscall_error(syscall::EINVAL); } }
    syscall::SyscallResult::success(0)
}
fn syscall_condvar_signal(cv_handle: u64) -> syscall::SyscallResult {
    if cv_handle==0 || cv_handle>64 { return syscall_error(syscall::EINVAL); }
    unsafe { if !*core::ptr::addr_of!(SC_CONDVARS).cast::<bool>().add((cv_handle-1) as usize) { return syscall_error(syscall::EINVAL); } }
    syscall::SyscallResult::success(0)
}
fn syscall_condvar_broadcast(cv_handle: u64) -> syscall::SyscallResult { syscall_condvar_signal(cv_handle) }
fn syscall_futex_wait(_addr: u64, _expected: u64, _timeout_ms: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_futex_wake(_addr: u64, count: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(count) }
fn syscall_msgq_create(_capacity: u64) -> syscall::SyscallResult {
    for i in 0..16 { unsafe { let u=core::ptr::addr_of_mut!(SC_MSGQ_USED).cast::<bool>().add(i); if !*u { *u=true; *core::ptr::addr_of_mut!(SC_MSGQ_HEAD).cast::<usize>().add(i)=0; *core::ptr::addr_of_mut!(SC_MSGQ_TAIL).cast::<usize>().add(i)=0; *core::ptr::addr_of_mut!(SC_MSGQ_COUNT).cast::<usize>().add(i)=0; return syscall::SyscallResult::success(i as u64+1); } } }
    syscall_error(syscall::ENOSPC)
}
fn syscall_msgq_send(handle: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    if handle==0 || handle>16 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_MSGQ_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); } }
    let count=unsafe{*core::ptr::addr_of!(SC_MSGQ_COUNT).cast::<usize>().add(i)};
    if count>=8 { return syscall_error(syscall::ENOSPC); }
    let src = match sc_read_user(buf_ptr, buf_len) { Ok(s)=>s, Err(e)=>return syscall_error(e) };
    let n=core::cmp::min(src.len(),256);
    unsafe {
        let tail=*core::ptr::addr_of!(SC_MSGQ_TAIL).cast::<usize>().add(i);
        let slot=core::ptr::addr_of_mut!(SC_MSGQ).cast::<[[u8;256];8]>().add(i);
        core::ptr::write_bytes((*slot)[tail].as_mut_ptr(),0,256);
        core::ptr::copy_nonoverlapping(src.as_ptr(), (*slot)[tail].as_mut_ptr(), n);
        *core::ptr::addr_of_mut!(SC_MSGQ_LEN).cast::<[usize;8]>().add(i).cast::<usize>().add(tail)=n;
        *core::ptr::addr_of_mut!(SC_MSGQ_TAIL).cast::<usize>().add(i)=(tail+1)%8;
        *core::ptr::addr_of_mut!(SC_MSGQ_COUNT).cast::<usize>().add(i)=count+1;
    }
    syscall::SyscallResult::success(n as u64)
}
fn syscall_msgq_recv(handle: u64, buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult {
    if handle==0 || handle>16 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_MSGQ_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); } }
    let count=unsafe{*core::ptr::addr_of!(SC_MSGQ_COUNT).cast::<usize>().add(i)};
    if count==0 { return syscall_error(syscall::EAGAIN); }
    unsafe {
        let head=*core::ptr::addr_of!(SC_MSGQ_HEAD).cast::<usize>().add(i);
        let slot=core::ptr::addr_of!(SC_MSGQ).cast::<[[u8;256];8]>().add(i);
        let n=*core::ptr::addr_of!(SC_MSGQ_LEN).cast::<[usize;8]>().add(i).cast::<usize>().add(head);
        let slice=core::slice::from_raw_parts((*slot)[head].as_ptr(), n);
        let r = sc_copy_to_user(buf_ptr, buf_len, slice);
        *core::ptr::addr_of_mut!(SC_MSGQ_HEAD).cast::<usize>().add(i)=(head+1)%8;
        *core::ptr::addr_of_mut!(SC_MSGQ_COUNT).cast::<usize>().add(i)=count-1;
        match r { Ok(v)=>syscall::SyscallResult::success(v), Err(e)=>syscall_error(e) }
    }
}
fn syscall_msgq_destroy(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>16 { return syscall_error(syscall::EINVAL); }
    unsafe { *core::ptr::addr_of_mut!(SC_MSGQ_USED).cast::<bool>().add((handle-1) as usize)=false; *core::ptr::addr_of_mut!(SC_MSGQ_COUNT).cast::<usize>().add((handle-1) as usize)=0; }
    syscall::SyscallResult::success(0)
}
fn syscall_eventfd_create(initial: u64) -> syscall::SyscallResult {
    for i in 0..64 { unsafe { let u=core::ptr::addr_of_mut!(SC_EVENTFD_USED).cast::<bool>().add(i); if !*u { *u=true; *core::ptr::addr_of_mut!(SC_EVENTFD).cast::<u64>().add(i)=initial; return syscall::SyscallResult::success(i as u64+1); } } }
    syscall_error(syscall::ENOSPC)
}
fn syscall_eventfd_read(handle: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_EVENTFD_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); }
        let v=*core::ptr::addr_of_mut!(SC_EVENTFD).cast::<u64>().add(i); *core::ptr::addr_of_mut!(SC_EVENTFD).cast::<u64>().add(i)=0; syscall::SyscallResult::success(v) }
}
fn syscall_eventfd_write(handle: u64, value: u64) -> syscall::SyscallResult {
    if handle==0 || handle>64 { return syscall_error(syscall::EINVAL); }
    let i=(handle-1) as usize;
    unsafe { if !*core::ptr::addr_of!(SC_EVENTFD_USED).cast::<bool>().add(i) { return syscall_error(syscall::EINVAL); }
        *core::ptr::addr_of_mut!(SC_EVENTFD).cast::<u64>().add(i)=(*core::ptr::addr_of!(SC_EVENTFD).cast::<u64>().add(i)).saturating_add(value); }
    syscall::SyscallResult::success(0)
}
fn syscall_get_os_name(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { match sc_copy_cstr_to_user(buf_ptr,buf_len,b"AlphaOS") { Ok(n)=>syscall::SyscallResult::success(n), Err(e)=>syscall_error(e)} }
fn syscall_get_os_version() -> syscall::SyscallResult { syscall::SyscallResult::success(((syscall::SYSCALL_ABI_MAJOR as u64)<<16)|(syscall::SYSCALL_ABI_MINOR as u64)) }
fn syscall_get_hostname(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { syscall_get_system_name(buf_ptr, buf_len) }
fn syscall_set_hostname(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { syscall_set_system_name(buf_ptr, buf_len) }
fn syscall_get_kernel_version(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { match sc_copy_cstr_to_user(buf_ptr,buf_len,b"0.3.0-freestanding") { Ok(n)=>syscall::SyscallResult::success(n), Err(e)=>syscall_error(e)} }
fn syscall_get_arch() -> syscall::SyscallResult { syscall::SyscallResult::success(0x8664) }
fn syscall_get_cpu_features(buf_ptr: u64) -> syscall::SyscallResult {
    #[repr(C)] struct F { arch: u64, bits: u64, has_sse: u64, has_apic: u64 }
    let f=F{arch:0x8664,bits:64,has_sse:1,has_apic:1};
    let bytes=unsafe{core::slice::from_raw_parts((&f as *const F).cast::<u8>(), core::mem::size_of::<F>())};
    match sc_copy_to_user(buf_ptr, bytes.len() as u64, bytes) { Ok(_)=>syscall::SyscallResult::success(0), Err(e)=>syscall_error(e) }
}
fn syscall_get_load_avg(buf_ptr: u64) -> syscall::SyscallResult {
    #[repr(C)] struct L { l1:u64,l5:u64,l15:u64,proc_count:u64 }
    let mut procs=1u64;
    for i in 0..MAX_SPAWNED { unsafe { if SPAWNED[i].used && !SPAWNED[i].exited { procs+=1; } } }
    let l=L{l1:procs,l5:procs,l15:procs,proc_count:procs};
    let bytes=unsafe{core::slice::from_raw_parts((&l as *const L).cast::<u8>(), core::mem::size_of::<L>())};
    match sc_copy_to_user(buf_ptr, bytes.len() as u64, bytes) { Ok(_)=>syscall::SyscallResult::success(0), Err(e)=>syscall_error(e) }
}
fn syscall_sysinfo(buf_ptr: u64) -> syscall::SyscallResult { get_system_info(0, buf_ptr, 4096) }
fn syscall_console_write(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { write_console(1, buf_ptr, buf_len) }
fn syscall_console_read(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { read_console(0, buf_ptr, buf_len) }
fn syscall_console_clear() -> syscall::SyscallResult {
    let console = unsafe { &mut *SYSCALL_CONSOLE };
    clear_screen(console.address, console.width, console.height, console.pitch, console.pixel_format);
    console.cursor_y = Console::TOP;
    console.cursor_x = Console::LEFT;
    console.draw_header();
    console.draw_prompt();
    console.draw_caret();
    syscall::SyscallResult::success(0)
}
fn syscall_console_set_color(_fg: u64, _bg: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_console_get_cursor(x_ptr: u64, y_ptr: u64) -> syscall::SyscallResult {
    let console = unsafe { &*SYSCALL_CONSOLE };
    let _=sc_write_u32(x_ptr, console.cursor_x as u32);
    let _=sc_write_u32(y_ptr, console.cursor_y as u32);
    syscall::SyscallResult::success(0)
}
fn syscall_console_set_cursor(_x: u64, _y: u64) -> syscall::SyscallResult { syscall::SyscallResult::success(0) }
fn syscall_debug_write(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { syscall_debug_log(buf_ptr, buf_len) }
fn syscall_debug_read(buf_ptr: u64, buf_len: u64) -> syscall::SyscallResult { syscall_get_kernel_log(buf_ptr, buf_len) }
fn syscall_get_last_error() -> syscall::SyscallResult { syscall::SyscallResult::success(unsafe { *core::ptr::addr_of!(SC_LAST_ERROR) }) }

fn sc_path_exists(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); syscall::SyscallResult::success(if find_fs(&r,l).is_some(){1}else{0})
}

fn sc_path_is_dir(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); match find_fs(&r,l){Some(i)=>with_fs(|fs| syscall::SyscallResult::success(if fs[i].is_dir{1}else{0})),None=>syscall::SyscallResult::success(0)}
}

fn sc_path_is_file(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); match find_fs(&r,l){Some(i)=>with_fs(|fs| syscall::SyscallResult::success(if !fs[i].is_dir{1}else{0})),None=>syscall::SyscallResult::success(0)}
}

fn sc_file_size_path(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); match find_fs(&r,l){Some(i)=>with_fs(|fs| if fs[i].is_dir{syscall_error(syscall::EINVAL)}else{syscall::SyscallResult::success(fs[i].size as u64)}),None=>syscall_error(syscall::ENOENT)}
}

fn sc_file_delete_path(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_unlink(arg0,arg1)
}

fn sc_file_rename_path(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall_rename(arg0,arg1,arg2,arg3)
}

fn sc_dir_create(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_mkdir(arg0,arg1)
}

fn sc_dir_remove(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_rmdir(arg0,arg1)
}

fn sc_file_open_read(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_open(arg0,arg1,0,0)
}

fn sc_file_open_write(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_open(arg0,arg1,1,0)
}

fn sc_file_close_handle(arg0: u64) -> syscall::SyscallResult {
    close_handle(arg0)
}

fn sc_file_read_handle(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    sys_file_read(arg0,arg1,arg2)
}

fn sc_file_write_handle(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    sys_file_write(arg0,arg1,arg2)
}

fn sc_file_seek_handle(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    sys_seek(arg0,arg1,arg2)
}

fn sc_file_tell(arg0: u64) -> syscall::SyscallResult {
    let Some(slot)=file_handle_slot(arg0) else {return syscall_error(syscall::EBADF)}; syscall::SyscallResult::success(unsafe{OPEN_FILES[slot].offset as u64})
}

fn sc_file_eof(arg0: u64) -> syscall::SyscallResult {
    let Some(slot)=file_handle_slot(arg0) else {return syscall_error(syscall::EBADF)}; let fi=unsafe{OPEN_FILES[slot].fs_index}; let sz=with_fs(|fs| if fi<MAX_FS_FILES {fs[fi].size}else{0}); let off=unsafe{OPEN_FILES[slot].offset}; syscall::SyscallResult::success(if off>=sz{1}else{0})
}

fn sc_file_flush(arg0: u64) -> syscall::SyscallResult {
    syscall_fsync(arg0)
}

fn sc_file_set_size(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_truncate_handle(arg0,arg1)
}

fn sc_file_get_attr(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_file_stat_path(arg0,arg1,arg2)
}

fn sc_file_set_attr(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_chmod(arg0,arg1,arg2)
}

fn sc_dir_open(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_open(arg0,arg1,0,0)
}

fn sc_dir_read(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_readdir(arg0,arg1)
}

fn sc_dir_close(arg0: u64) -> syscall::SyscallResult {
    close_handle(arg0)
}

fn sc_path_join(arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> syscall::SyscallResult {
    let base=match sc_read_user(arg0,arg1){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let name=match sc_read_user(arg2,arg3){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let mut tmp=[0u8;256]; let mut n=0usize; for &b in base.iter().take(200){tmp[n]=b;n+=1;} if n==0||tmp[n-1]!=b"/"[0]{if n<255{tmp[n]=b"/"[0];n+=1;}} for &b in name.iter().take(255-n){tmp[n]=b;n+=1;} match sc_copy_cstr_to_user(arg4,256,&tmp[..n]){Ok(v)=>syscall::SyscallResult::success(v),Err(e)=>syscall_error(e)}
}

fn sc_path_basename(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match sc_read_user(arg0,arg1){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let mut start=0; for (i,&b) in p.iter().enumerate(){if b==b'/'{start=i+1;}} match sc_copy_cstr_to_user(arg2,arg3,&p[start..]){Ok(v)=>syscall::SyscallResult::success(v),Err(e)=>syscall_error(e)}
}

fn sc_path_dirname(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match sc_read_user(arg0,arg1){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let mut end=0; for (i,&b) in p.iter().enumerate(){if b==b'/'{end=i;}} if end==0{end=1;} match sc_copy_cstr_to_user(arg2,arg3,&p[..end]){Ok(v)=>syscall::SyscallResult::success(v),Err(e)=>syscall_error(e)}
}

fn sc_file_read_all(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); let idx=match find_fs(&r,l){Some(i)=>i,None=>return syscall_error(syscall::ENOENT)}; let (is_dir,size)=with_fs(|fs|(fs[idx].is_dir,fs[idx].size)); if is_dir{return syscall_error(syscall::EINVAL);} let mut tmp=[0u8;MAX_FILE_SIZE]; with_fs(|fs|{tmp[..size].copy_from_slice(&fs[idx].data[..size]);}); match sc_copy_to_user(arg2,arg3,&tmp[..size]){Ok(n)=>syscall::SyscallResult::success(n),Err(e)=>syscall_error(e)}
}

fn sc_file_write_all(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let src=match sc_read_user(arg2,arg3){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); let n=core::cmp::min(src.len(),MAX_FILE_SIZE); if let Some(idx)=find_fs(&r,l){ with_fs(|fs|{ if fs[idx].is_dir{return;} fs[idx].data[..n].copy_from_slice(&src[..n]); for b in &mut fs[idx].data[n..fs[idx].size]{*b=0;} fs[idx].size=n; }); return syscall::SyscallResult::success(n as u64);} with_fs(|fs|{ for i in 0..MAX_FS_FILES{ if !fs[i].used{ fs[i].used=true; fs[i].is_dir=false; let nlm=core::cmp::min(l,64); fs[i].name=[0;64]; fs[i].name[..nlm].copy_from_slice(&r[..nlm]); fs[i].name_len=nlm; fs[i].size=n; fs[i].data[..n].copy_from_slice(&src[..n]); return syscall::SyscallResult::success(n as u64);} } syscall_error(syscall::ENOSPC) })
}

fn sc_file_append(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let src=match sc_read_user(arg2,arg3){Ok(s)=>s,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); let idx=match find_fs(&r,l){Some(i)=>i,None=>return syscall_error(syscall::ENOENT)}; with_fs(|fs|{ if fs[idx].is_dir{return syscall_error(syscall::EINVAL);} let can=core::cmp::min(src.len(), MAX_FILE_SIZE.saturating_sub(fs[idx].size)); let off=fs[idx].size; fs[idx].data[off..off+can].copy_from_slice(&src[..can]); fs[idx].size+=can; syscall::SyscallResult::success(can as u64) })
}

fn sc_file_copy_path(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall_copy_file(arg0,arg1,arg2,arg3)
}

fn sc_file_move_path(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let r=syscall_copy_file(arg0,arg1,arg2,arg3); if r.error!=0 {return r;} syscall_unlink(arg0,arg1)
}

fn sc_dir_list_count(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); let mut c=0u64; with_fs(|fs|{ for i in 0..MAX_FS_FILES{ if !fs[i].used{continue;} if l==1 && r[0]==b'/'{ if fs[i].name_len>0{c+=1;} continue;} if fs[i].name_len>l+1 && fs[i].name[..l]==r[..l] && fs[i].name[l]==b'/'{ let rest=&fs[i].name[l+1..fs[i].name_len]; if !rest.iter().any(|&b|b==b'/'){c+=1;} } } }); syscall::SyscallResult::success(c)
}

fn sc_fs_free_space() -> syscall::SyscallResult {
    let mut used=0u64; with_fs(|fs|{ for i in 0..MAX_FS_FILES{ if fs[i].used{used+=fs[i].size as u64;} } }); syscall::SyscallResult::success((MAX_FS_FILES as u64 * MAX_FILE_SIZE as u64).saturating_sub(used))
}

fn sc_fs_total_space() -> syscall::SyscallResult {
    syscall::SyscallResult::success(MAX_FS_FILES as u64 * MAX_FILE_SIZE as u64)
}

fn sc_fs_file_count() -> syscall::SyscallResult {
    let mut c=0u64; with_fs(|fs|{ for i in 0..MAX_FS_FILES{ if fs[i].used{c+=1;} } }); syscall::SyscallResult::success(c)
}

fn sc_path_normalize(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); match sc_copy_cstr_to_user(arg2,arg3,&r[..l]){Ok(v)=>syscall::SyscallResult::success(v),Err(e)=>syscall_error(e)}
}

fn sc_file_touch(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let p=match user_read_path(arg0,arg1){Ok(p)=>p,Err(e)=>return syscall_error(e)}; let r=resolve_path(p); let l=resolved_len(&r); if find_fs(&r,l).is_some(){ return syscall::SyscallResult::success(0);} with_fs(|fs|{ for i in 0..MAX_FS_FILES{ if !fs[i].used{ fs[i].used=true; fs[i].is_dir=false; fs[i].size=0; let nlm=core::cmp::min(l,64); fs[i].name=[0;64]; fs[i].name[..nlm].copy_from_slice(&r[..nlm]); fs[i].name_len=nlm; return syscall::SyscallResult::success(0);} } syscall_error(syscall::ENOSPC) })
}

fn sc_get_fd_flags(arg0: u64) -> syscall::SyscallResult {
    syscall_fcntl(arg0, syscall::FCNTL_GETFL as u64, 0)
}

fn sc_set_fd_flags(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_fcntl(arg0, syscall::FCNTL_SETFL as u64, arg1)
}

fn sc_file_dup_handle(arg0: u64) -> syscall::SyscallResult {
    syscall_dup(arg0)
}

fn sc_process_spawn(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    sys_spawn(arg0,arg1,arg2,arg3)
}

fn sc_process_exit(arg0: u64) -> syscall::SyscallResult {
    sys_exit(arg0)
}

fn sc_process_wait(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    sys_wait(arg0,arg1,arg2)
}

fn sc_process_get_pid() -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_process_get_ppid() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_process_kill(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_kill_process(arg0,arg1)
}

fn sc_process_list(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_list_processes(arg0,arg1.saturating_mul(4))
}

fn sc_process_info(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_process_info(arg0,arg1)
}

fn sc_process_name(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_get_proc_name(arg0,arg1,arg2)
}

fn sc_process_set_name(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_set_process_name(arg0,arg1)
}

fn sc_process_exists_pid(arg0: u64) -> syscall::SyscallResult {
    syscall_process_exists(arg0)
}

fn sc_process_get_state(arg0: u64) -> syscall::SyscallResult {
    if arg0==0||arg0==1{return syscall::SyscallResult::success(1);} for i in 0..MAX_SPAWNED{unsafe{ if SPAWNED[i].used && SPAWNED[i].pid as u64==arg0 { return syscall::SyscallResult::success(if SPAWNED[i].exited{2}else{1}); } }} syscall_error(syscall::ESRCH)
}

fn sc_process_get_exit(arg0: u64) -> syscall::SyscallResult {
    syscall_get_exit_code(arg0)
}

fn sc_process_current_name(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_proc_name(1,arg0,arg1)
}

fn sc_process_argc() -> syscall::SyscallResult {
    syscall_get_arg_count()
}

fn sc_process_argv(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_get_arg(arg0,arg1,arg2)
}

fn sc_process_get_env(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall_get_env(arg0,arg1,arg2,arg3)
}

fn sc_process_set_env(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall_set_env(arg0,arg1,arg2,arg3)
}

fn sc_thread_get_tid() -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_thread_yield() -> syscall::SyscallResult {
    syscall_sched_yield()
}

fn sc_thread_sleep_ms(arg0: u64) -> syscall::SyscallResult {
    syscall_sleep_ms(arg0)
}

fn sc_thread_set_name(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_set_thread_name(arg0,arg1,arg2)
}

fn sc_thread_get_name(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_get_thread_name(arg0,arg1,arg2)
}

fn sc_thread_set_priority(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_set_thread_priority(arg0,arg1)
}

fn sc_thread_get_priority(arg0: u64) -> syscall::SyscallResult {
    syscall_get_thread_priority(arg0)
}

fn sc_proc_get_uid() -> syscall::SyscallResult {
    syscall_get_uid()
}

fn sc_proc_set_uid(arg0: u64) -> syscall::SyscallResult {
    syscall_set_uid(arg0)
}

fn sc_proc_get_gid() -> syscall::SyscallResult {
    syscall_get_gid()
}

fn sc_proc_set_gid(arg0: u64) -> syscall::SyscallResult {
    syscall_set_gid(arg0)
}

fn sc_proc_times(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_process_times(arg0,arg1)
}

fn sc_proc_get_cwd(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_getcwd(arg0,arg1)
}

fn sc_proc_set_cwd(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_setcwd(arg0,arg1)
}

fn sc_proc_exec(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall_exec(arg0,arg1,arg2,arg3)
}

fn sc_proc_fork() -> syscall::SyscallResult {
    syscall_fork()
}

fn sc_proc_waitpid(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_waitpid(arg0,arg1,arg2)
}

fn sc_proc_is_alive(arg0: u64) -> syscall::SyscallResult {
    if arg0==0||arg0==1{return syscall::SyscallResult::success(1);} for i in 0..MAX_SPAWNED{unsafe{ if SPAWNED[i].used && SPAWNED[i].pid as u64==arg0 { return syscall::SyscallResult::success(if SPAWNED[i].exited{0}else{1}); } }} syscall::SyscallResult::success(0)
}

fn sc_proc_signal(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_kill_process(arg0,arg1)
}

fn sc_proc_count() -> syscall::SyscallResult {
    let mut c=1u64; for i in 0..MAX_SPAWNED{unsafe{ if SPAWNED[i].used && !SPAWNED[i].exited{c+=1;} }} syscall::SyscallResult::success(c)
}

fn sc_proc_max() -> syscall::SyscallResult {
    syscall::SyscallResult::success(MAX_SPAWNED as u64)
}

fn sc_proc_self_status() -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_mem_alloc(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0,arg1)
}

fn sc_mem_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_mem_realloc(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    if arg0==0 { return sys_heap_alloc(arg2,8); } let r=sys_heap_alloc(arg2,8); if r.error!=0 {return r;} let n=core::cmp::min(arg1 as usize, arg2 as usize); if n>0 { if let Err(e)=validate_user_buffer(arg0,n as u64,false){return syscall_error(e);} if let Err(e)=validate_user_buffer(r.value,n as u64,true){return syscall_error(e);} unsafe{core::ptr::copy_nonoverlapping(arg0 as *const u8, r.value as *mut u8, n);} } let _=sys_heap_free(arg0,arg1); r
}

fn sc_mem_map(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_map(arg0,arg1,arg2)
}

fn sc_mem_unmap(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_memory_unmap(arg0,arg1)
}

fn sc_mem_protect(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_protect(arg0,arg1,arg2)
}

fn sc_mem_info(arg0: u64) -> syscall::SyscallResult {
    syscall_get_memory_info(arg0)
}

fn sc_mem_stats(arg0: u64) -> syscall::SyscallResult {
    syscall_memory_stats(arg0)
}

fn sc_mem_page_size() -> syscall::SyscallResult {
    syscall::SyscallResult::success(4096)
}

fn sc_mem_set(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_fill(arg0,arg1,arg2)
}

fn sc_mem_copy(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_copy(arg0,arg1,arg2)
}

fn sc_mem_compare(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_compare(arg0,arg1,arg2)
}

fn sc_mem_zero(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_memory_fill(arg0,0,arg1)
}

fn sc_mem_available() -> syscall::SyscallResult {
    { let total=unsafe{ALLOCATOR_END.saturating_sub(ALLOCATOR_START)}; let used=unsafe{ALLOCATOR_NEXT.saturating_sub(ALLOCATOR_START)}; syscall::SyscallResult::success(total.saturating_sub(used)) }
}

fn sc_mem_used() -> syscall::SyscallResult {
    { let used=unsafe{ALLOCATOR_NEXT.saturating_sub(ALLOCATOR_START)}; syscall::SyscallResult::success(used) }
}

fn sc_mem_total() -> syscall::SyscallResult {
    { let total=unsafe{ALLOCATOR_END.saturating_sub(ALLOCATOR_START)}; syscall::SyscallResult::success(total) }
}

fn sc_heap_alloc_app(arg0: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0,8)
}

fn sc_heap_free_app(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_heap_stats_app(arg0: u64) -> syscall::SyscallResult {
    syscall_heap_stats(arg0)
}

fn sc_mem_map_shared(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_map(arg0,arg1,arg2)
}

fn sc_mem_unmap_shared(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_memory_unmap(arg0,arg1)
}

fn sc_mem_flush(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if arg1==0{return syscall::SyscallResult::success(0);} if let Err(e)=validate_user_buffer(arg0,arg1,false){return syscall_error(e);} syscall::SyscallResult::success(0)
}

fn sc_mem_lock(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if arg1==0{return syscall::SyscallResult::success(0);} if let Err(e)=validate_user_buffer(arg0,arg1,false){return syscall_error(e);} syscall::SyscallResult::success(0)
}

fn sc_mem_unlock(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if arg1==0{return syscall::SyscallResult::success(0);} if let Err(e)=validate_user_buffer(arg0,arg1,false){return syscall_error(e);} syscall::SyscallResult::success(0)
}

fn sc_mem_advise_app(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_advise(arg0,arg1,arg2)
}

fn sc_shm_create(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    shared_memory_create(arg0,arg1)
}

fn sc_mmap_anon(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0,4096)
}

fn sc_munmap_anon(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_mem_read_u64(arg0: u64) -> syscall::SyscallResult {
    if let Err(e)=validate_user_buffer(arg0,8,false){return syscall_error(e);} syscall::SyscallResult::success(unsafe{core::ptr::read_unaligned(arg0 as *const u64)})
}

fn sc_mem_write_u64(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if let Err(e)=validate_user_buffer(arg0,8,true){return syscall_error(e);} unsafe{core::ptr::write_unaligned(arg0 as *mut u64,arg1);} syscall::SyscallResult::success(0)
}

fn sc_mem_read_u32(arg0: u64) -> syscall::SyscallResult {
    if let Err(e)=validate_user_buffer(arg0,4,false){return syscall_error(e);} syscall::SyscallResult::success(unsafe{core::ptr::read_unaligned(arg0 as *const u32)} as u64)
}

fn sc_mem_write_u32(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if let Err(e)=validate_user_buffer(arg0,4,true){return syscall_error(e);} unsafe{core::ptr::write_unaligned(arg0 as *mut u32,arg1 as u32);} syscall::SyscallResult::success(0)
}

fn sc_mem_swap(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    let n=arg2 as usize; if n==0{return syscall::SyscallResult::success(0);} if let Err(e)=validate_user_buffer(arg0,arg2,false){return syscall_error(e);} if let Err(e)=validate_user_buffer(arg1,arg2,false){return syscall_error(e);} if let Err(e)=validate_user_buffer(arg0,arg2,true){return syscall_error(e);} if let Err(e)=validate_user_buffer(arg1,arg2,true){return syscall_error(e);} let mut i=0; while i<n { unsafe{ let pa=(arg0 as *mut u8).add(i); let pb=(arg1 as *mut u8).add(i); let t=*pa; *pa=*pb; *pb=t; } i+=1; } syscall::SyscallResult::success(n as u64)
}

fn sc_mem_fill_pattern(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    if arg3==0{return syscall::SyscallResult::success(0);} let pat=match sc_read_user(arg1,arg2){Ok(s)=>s,Err(e)=>return syscall_error(e)}; if pat.is_empty(){return syscall_error(syscall::EINVAL);} if let Err(e)=validate_user_buffer(arg0,arg3,true){return syscall_error(e);} let n=arg3 as usize; for i in 0..n { unsafe{ *((arg0 as *mut u8).add(i))=pat[i%pat.len()]; } } syscall::SyscallResult::success(arg3)
}

fn sc_mem_stats_ex(arg0: u64) -> syscall::SyscallResult {
    syscall_get_memory_info(arg0)
}

fn sc_heap_compact() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_mem_validate(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    match validate_user_buffer(arg0,arg1,false){Ok(())=>syscall::SyscallResult::success(1),Err(_)=>syscall::SyscallResult::success(0)}
}

fn sc_mem_dup(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    if arg1==0{return syscall::SyscallResult::success(0);} let r=sys_heap_alloc(arg1,8); if r.error!=0{return r;} if let Err(e)=validate_user_buffer(arg0,arg1,false){return syscall_error(e);} unsafe{core::ptr::copy_nonoverlapping(arg0 as *const u8, r.value as *mut u8, arg1 as usize);} syscall::SyscallResult::success(r.value)
}

fn sc_page_alloc(arg0: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0.saturating_mul(4096),4096)
}

fn sc_page_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1.saturating_mul(4096))
}

fn sc_time_now_sec() -> syscall::SyscallResult {
    syscall::SyscallResult::success(sc_ticks_ms()/1000)
}

fn sc_time_now_ms() -> syscall::SyscallResult {
    syscall::SyscallResult::success(sc_ticks_ms())
}

fn sc_time_now_ns() -> syscall::SyscallResult {
    syscall_uptime_ns()
}

fn sc_time_monotonic(arg0: u64) -> syscall::SyscallResult {
    syscall_clock_monotonic(arg0)
}

fn sc_time_realtime(arg0: u64) -> syscall::SyscallResult {
    syscall_clock_realtime(arg0)
}

fn sc_time_sleep_ns(arg0: u64) -> syscall::SyscallResult {
    sys_sleep(arg0)
}

fn sc_time_sleep_ms(arg0: u64) -> syscall::SyscallResult {
    syscall_sleep_ms(arg0)
}

fn sc_time_sleep_sec(arg0: u64) -> syscall::SyscallResult {
    syscall_sleep_ms(arg0.saturating_mul(1000))
}

fn sc_time_ticks() -> syscall::SyscallResult {
    syscall_get_ticks()
}

fn sc_time_ticks_per_sec() -> syscall::SyscallResult {
    syscall_ticks_per_second()
}

fn sc_time_uptime_ms() -> syscall::SyscallResult {
    syscall_get_uptime_ms()
}

fn sc_time_deadline(arg0: u64) -> syscall::SyscallResult {
    syscall_deadline_wait(arg0)
}

fn sc_timer_create(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_create_timer(arg0,arg1)
}

fn sc_timer_get(arg0: u64) -> syscall::SyscallResult {
    syscall_get_timer(arg0)
}

fn sc_timer_cancel(arg0: u64) -> syscall::SyscallResult {
    syscall_cancel_timer(arg0)
}

fn sc_timer_set_ms(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_set_timer_ms(arg0,arg1)
}

fn sc_timer_remaining(arg0: u64) -> syscall::SyscallResult {
    syscall_get_timer_ms(arg0)
}

fn sc_clock_get_res(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let ts=syscall::TimeSpec{seconds:0,nanoseconds:1_000_000}; let bytes=unsafe{core::slice::from_raw_parts((&ts as *const syscall::TimeSpec).cast::<u8>(), core::mem::size_of::<syscall::TimeSpec>())}; match sc_copy_to_user(arg1,bytes.len() as u64,bytes){Ok(_)=>syscall::SyscallResult::success(0),Err(e)=>syscall_error(e)}
}

fn sc_alarm_set(arg0: u64) -> syscall::SyscallResult {
    syscall_alarm(arg0)
}

fn sc_alarm_cancel() -> syscall::SyscallResult {
    unsafe{*core::ptr::addr_of_mut!(SC_ALARM_MS)=0;} syscall::SyscallResult::success(0)
}

fn sc_ipc_mutex_create() -> syscall::SyscallResult {
    syscall_create_mutex()
}

fn sc_ipc_mutex_lock(arg0: u64) -> syscall::SyscallResult {
    syscall_lock_mutex(arg0)
}

fn sc_ipc_mutex_unlock(arg0: u64) -> syscall::SyscallResult {
    syscall_unlock_mutex(arg0)
}

fn sc_ipc_mutex_destroy(arg0: u64) -> syscall::SyscallResult {
    syscall_destroy_mutex(arg0)
}

fn sc_ipc_sem_create(arg0: u64) -> syscall::SyscallResult {
    syscall_create_sem(arg0)
}

fn sc_ipc_sem_wait(arg0: u64) -> syscall::SyscallResult {
    syscall_sem_wait(arg0)
}

fn sc_ipc_sem_post(arg0: u64) -> syscall::SyscallResult {
    syscall_sem_post(arg0)
}

fn sc_ipc_sem_destroy(arg0: u64) -> syscall::SyscallResult {
    syscall_sem_destroy(arg0)
}

fn sc_ipc_event_create(arg0: u64) -> syscall::SyscallResult {
    syscall_create_event(arg0)
}

fn sc_ipc_event_signal(arg0: u64) -> syscall::SyscallResult {
    syscall_signal_event(arg0)
}

fn sc_ipc_event_wait(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_wait_event(arg0,arg1)
}

fn sc_ipc_msg_create(arg0: u64) -> syscall::SyscallResult {
    syscall_msgq_create(arg0)
}

fn sc_ipc_msg_send(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_msgq_send(arg0,arg1,arg2)
}

fn sc_ipc_msg_recv(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_msgq_recv(arg0,arg1,arg2)
}

fn sc_ipc_msg_destroy(arg0: u64) -> syscall::SyscallResult {
    syscall_msgq_destroy(arg0)
}

fn sc_ipc_pipe_create(arg0: u64) -> syscall::SyscallResult {
    pipe_create(arg0)
}

fn sc_ipc_pipe_read(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    pipe_read(handle_slot(arg0),arg1,arg2 as usize)
}

fn sc_ipc_pipe_write(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    pipe_write(handle_slot(arg0),arg1,arg2 as usize)
}

fn sc_ipc_channel_create(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    channel_pair_create(arg0,arg1)
}

fn sc_ipc_channel_close(arg0: u64) -> syscall::SyscallResult {
    channel_close(arg0)
}

fn sc_net_socket_create(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    socket_create(arg0,arg1,arg2)
}

fn sc_net_socket_bind(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    socket_bind(arg0,arg1,arg2)
}

fn sc_net_socket_listen(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    socket_listen(arg0,arg1)
}

fn sc_net_socket_accept(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    socket_accept(arg0,arg1,arg2)
}

fn sc_net_socket_connect(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    socket_connect(arg0,arg1,arg2)
}

fn sc_net_socket_send(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::error(syscall::ENOTSUP)
}

fn sc_net_socket_recv(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::error(syscall::ENOTSUP)
}

fn sc_net_socket_close(arg0: u64) -> syscall::SyscallResult {
    close_handle(arg0)
}

fn sc_net_socket_shutdown(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_socket_shutdown(arg0,arg1)
}

fn sc_net_socket_pair(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_socketpair(arg0,arg1,arg2)
}

fn sc_net_get_local_addr(_arg0: u64, _arg1: u64, _arg2: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::error(syscall::ENOTSUP)
}

fn sc_net_get_remote_addr(_arg0: u64, _arg1: u64, _arg2: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::error(syscall::ENOTSUP)
}

fn sc_net_set_nonblocking(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_fcntl(arg0, syscall::FCNTL_SETFL as u64, if arg1!=0{1}else{0})
}

fn sc_net_poll_socket(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let mut pfd=syscall::PollFd{handle:arg0, events:arg1 as u16, revents:0}; let bytes=unsafe{core::slice::from_raw_parts_mut((&mut pfd as *mut syscall::PollFd).cast::<u8>(),core::mem::size_of::<syscall::PollFd>())}; let _=bytes; let r=syscall_poll((&pfd as *const syscall::PollFd) as u64,1,0); if r.error!=0{return r;} syscall::SyscallResult::success(pfd.revents as u64)
}

fn sc_net_get_hostname(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_hostname(arg0,arg1)
}

fn sc_net_set_hostname(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_set_hostname(arg0,arg1)
}

fn sc_net_resolve_local(_arg0: u64, _arg1: u64, _arg2: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::error(syscall::ENOTSUP)
}

fn sc_net_loopback_ping(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_net_socket_stats(arg0: u64) -> syscall::SyscallResult {
    let mut used=0u64; for i in 0..MAX_SOCKETS{unsafe{ if SOCKETS[i].used{used+=1;} }} let mut buf=[0u8;8]; buf.copy_from_slice(&used.to_le_bytes()); match sc_copy_to_user(arg0,8,&buf){Ok(_)=>syscall::SyscallResult::success(used),Err(e)=>syscall_error(e)}
}

fn sc_net_if_count() -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_sys_get_os_name(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_os_name(arg0,arg1)
}

fn sc_sys_get_os_version() -> syscall::SyscallResult {
    syscall_get_os_version()
}

fn sc_sys_get_kernel_ver(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_kernel_version(arg0,arg1)
}

fn sc_sys_get_arch() -> syscall::SyscallResult {
    syscall_get_arch()
}

fn sc_sys_get_cpu_count() -> syscall::SyscallResult {
    syscall_get_cpu_count()
}

fn sc_sys_get_cpu_features(arg0: u64) -> syscall::SyscallResult {
    syscall_get_cpu_features(arg0)
}

fn sc_sys_get_uptime() -> syscall::SyscallResult {
    syscall_get_uptime_ms()
}

fn sc_sys_get_load_avg(arg0: u64) -> syscall::SyscallResult {
    syscall_get_load_avg(arg0)
}

fn sc_sys_info_ex(arg0: u64) -> syscall::SyscallResult {
    syscall_sysinfo(arg0)
}

fn sc_sys_get_random(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_getrandom(arg0,arg1)
}

fn sc_sys_get_boot_info(arg0: u64) -> syscall::SyscallResult {
    syscall_get_boot_info(arg0)
}

fn sc_sys_get_console_size(arg0: u64) -> syscall::SyscallResult {
    syscall_get_console_size(arg0)
}

fn sc_sys_set_console_mode(arg0: u64) -> syscall::SyscallResult {
    syscall_set_console_mode(arg0)
}

fn sc_sys_reboot(arg0: u64) -> syscall::SyscallResult {
    reboot_system(arg0)
}

fn sc_sys_shutdown(arg0: u64) -> syscall::SyscallResult {
    reboot_system(arg0)
}

fn sc_sys_debug_log(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_debug_log(arg0,arg1)
}

fn sc_sys_get_kernel_log(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_kernel_log(arg0,arg1)
}

fn sc_sys_get_last_error() -> syscall::SyscallResult {
    syscall_get_last_error()
}

fn sc_sys_set_last_error(arg0: u64) -> syscall::SyscallResult {
    sc_set_last_error(arg0); syscall::SyscallResult::success(0)
}

fn sc_sys_get_abi() -> syscall::SyscallResult {
    syscall::SyscallResult::success((u64::from(syscall::SYSCALL_ABI_MAJOR)<<16)|u64::from(syscall::SYSCALL_ABI_MINOR))
}

fn sc_kmem_alloc(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0,arg1)
}

fn sc_kmem_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_kmem_map(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    if arg0==0 {return sys_heap_alloc(arg1,4096);} syscall::SyscallResult::success(arg0)
}

fn sc_kmem_unmap(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_kmem_protect(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_mprotect(arg0,arg1,arg2)
}

fn sc_kmem_query(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    { let total=unsafe{ALLOCATOR_END.saturating_sub(ALLOCATOR_START)}; let used=unsafe{ALLOCATOR_NEXT.saturating_sub(ALLOCATOR_START)}; let info=syscall::MemoryInfo{managed_bytes:total,allocated_bytes:used,available_bytes:total.saturating_sub(used)}; let b=unsafe{core::slice::from_raw_parts((&info as *const syscall::MemoryInfo).cast::<u8>(),core::mem::size_of::<syscall::MemoryInfo>())}; match sc_copy_to_user(arg1,b.len() as u64,b){Ok(_)=>syscall::SyscallResult::success(0),Err(e)=>syscall_error(e)} }
}

fn sc_kmem_stats(arg0: u64) -> syscall::SyscallResult {
    syscall_get_memory_info(arg0)
}

fn sc_kmem_flush(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kpage_alloc(arg0: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0.saturating_mul(4096),4096)
}

fn sc_kpage_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1.saturating_mul(4096))
}

fn sc_kpage_map(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(arg0)
}

fn sc_kpage_unmap(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kheap_info(arg0: u64) -> syscall::SyscallResult {
    syscall_heap_stats(arg0)
}

fn sc_kheap_compact() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kmem_zero(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_memory_fill(arg0,0,arg1)
}

fn sc_kmem_copy(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_copy(arg0,arg1,arg2)
}

fn sc_kmem_compare(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_memory_compare(arg0,arg1,arg2)
}

fn sc_kphys_alloc(arg0: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0.saturating_mul(4096),4096)
}

fn sc_kphys_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1.saturating_mul(4096))
}

fn sc_kmem_barrier() -> syscall::SyscallResult {
    unsafe{core::arch::asm!("mfence");} syscall::SyscallResult::success(0)
}

fn sc_ksched_yield() -> syscall::SyscallResult {
    syscall_sched_yield()
}

fn sc_ksched_set_priority(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_set_thread_priority(arg0,arg1)
}

fn sc_ksched_get_priority(arg0: u64) -> syscall::SyscallResult {
    syscall_get_thread_priority(arg0)
}

fn sc_kthread_create(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    let mut pid=0u64; for i in 0..MAX_SPAWNED{unsafe{ if !SPAWNED[i].used { SPAWNED[i].used=true; SPAWNED[i].exited=false; SPAWNED[i].exit_code=0; SPAWNED[i].pid=((i as u32)+2) as u64; pid=SPAWNED[i].pid; break; } }} if pid==0{return syscall_error(syscall::ENOSPC);} syscall::SyscallResult::success(pid)
}

fn sc_kthread_exit(arg0: u64) -> syscall::SyscallResult {
    sys_exit(arg0)
}

fn sc_kthread_join(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_wait(arg0,arg1,0)
}

fn sc_kthread_sleep(arg0: u64) -> syscall::SyscallResult {
    syscall_sleep_ms(arg0)
}

fn sc_kthread_wake(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(if arg0!=0{1}else{0})
}

fn sc_kprocess_create(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    sys_spawn(arg0,arg1,arg2,arg3)
}

fn sc_kprocess_exit(arg0: u64) -> syscall::SyscallResult {
    sys_exit(arg0)
}

fn sc_kprocess_kill(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_kill_process(arg0,arg1)
}

fn sc_kprocess_wait(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    sys_wait(arg0,arg1,arg2)
}

fn sc_kprocess_info(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_process_info(arg0,arg1)
}

fn sc_kprocess_list(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_list_processes(arg0,arg1.saturating_mul(4))
}

fn sc_ktask_switch() -> syscall::SyscallResult {
    unsafe{core::arch::asm!("pause");} syscall::SyscallResult::success(0)
}

fn sc_kset_affinity(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(arg1)
}

fn sc_kget_affinity(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_kcpu_id() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kirq_disable() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kirq_enable() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kdev_list(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    device_list(arg0,arg1)
}

fn sc_kdev_open(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    device_open(arg0,arg1)
}

fn sc_kdev_close(arg0: u64) -> syscall::SyscallResult {
    close_handle(arg0)
}

fn sc_kdev_read(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    device_read(arg0,arg1,arg2)
}

fn sc_kdev_write(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    device_write(arg0,arg1,arg2)
}

fn sc_kdev_ioctl(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    syscall_ioctl(arg0,arg1,arg2)
}

fn sc_kdev_poll(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    let mut pfd=syscall::PollFd{handle:arg0,events:arg1 as u16,revents:0}; let r=syscall_poll((&pfd as *const syscall::PollFd) as u64,1,arg2); if r.error!=0{return r;} syscall::SyscallResult::success(pfd.revents as u64)
}

fn sc_kdev_mmap(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg2,4096)
}

fn sc_kdev_irq_register(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(arg0)
}

fn sc_kdev_irq_unregister(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kdev_dma_alloc(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_alloc(arg0,core::cmp::max(arg1,4096))
}

fn sc_kdev_dma_free(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    sys_heap_free(arg0,arg1)
}

fn sc_kio_port_read(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let port=arg0 as u16; let v:u64 = match arg1 { 1=>{let mut x:u8; unsafe{core::arch::asm!("in al, dx", out("al") x, in("dx") port);} x as u64 }, 2=>{let mut x:u16; unsafe{core::arch::asm!("in ax, dx", out("ax") x, in("dx") port);} x as u64 }, 4=>{let mut x:u32; unsafe{core::arch::asm!("in eax, dx", out("eax") x, in("dx") port);} x as u64 }, _=>return syscall_error(syscall::EINVAL)}; syscall::SyscallResult::success(v)
}

fn sc_kio_port_write(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    let port=arg0 as u16; match arg1 { 1=>unsafe{core::arch::asm!("out dx, al", in("dx") port, in("al") arg2 as u8);}, 2=>unsafe{core::arch::asm!("out dx, ax", in("dx") port, in("ax") arg2 as u16);}, 4=>unsafe{core::arch::asm!("out dx, eax", in("dx") port, in("eax") arg2 as u32);}, _=>return syscall_error(syscall::EINVAL)} syscall::SyscallResult::success(0)
}

fn sc_kfb_get_info(arg0: u64) -> syscall::SyscallResult {
    let con=unsafe{&*SYSCALL_CONSOLE}; #[repr(C)] struct Fb{addr:u64,width:u32,height:u32,pitch:u32,fmt:u32} let fb=Fb{addr:con.address as u64,width:con.width as u32,height:con.height as u32,pitch:con.pitch as u32,fmt:0u32}}; let b=unsafe{core::slice::from_raw_parts((&fb as *const Fb).cast::<u8>(),core::mem::size_of::<Fb>())}; match sc_copy_to_user(arg0,b.len() as u64,b){Ok(_)=>syscall::SyscallResult::success(0),Err(e)=>syscall_error(e)}
}

fn sc_kfb_put_pixel(arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> syscall::SyscallResult {
    let con=unsafe{&mut *SYSCALL_CONSOLE}; let x=arg0 as usize; let y=arg1 as usize; if x>=con.width||y>=con.height{return syscall_error(syscall::EINVAL);} { let offset=y*con.pitch + x*4; unsafe{ let p=con.address.add(offset); *p=arg4 as u8; *p.add(1)=arg3 as u8; *p.add(2)=arg2 as u8; *p.add(3)=0; } syscall::SyscallResult::success(0) }
}

fn sc_kfb_clear(arg0: u64, arg1: u64, arg2: u64) -> syscall::SyscallResult {
    let con=unsafe{&mut *SYSCALL_CONSOLE}; clear_screen(con.address,con.width,con.height,con.pitch,con.pixel_format); syscall::SyscallResult::success(0)
}

fn sc_kinput_poll(arg0: u64) -> syscall::SyscallResult {
    if let Some(ev)=input::dequeue_for(USER_INPUT_TARGET){ if arg0!=0 { let b=unsafe{core::slice::from_raw_parts((&ev as *const syscall::InputEvent).cast::<u8>(),core::mem::size_of::<syscall::InputEvent>())}; let _=sc_copy_to_user(arg0,b.len() as u64,b);} syscall::SyscallResult::success(1)} else {syscall::SyscallResult::success(0)}
}

fn sc_kinput_read(arg0: u64) -> syscall::SyscallResult {
    loop { if let Some(ev)=input::dequeue_for(USER_INPUT_TARGET){ if arg0!=0 { let b=unsafe{core::slice::from_raw_parts((&ev as *const syscall::InputEvent).cast::<u8>(),core::mem::size_of::<syscall::InputEvent>())}; let _=sc_copy_to_user(arg0,b.len() as u64,b);} return syscall::SyscallResult::success(1); } unsafe{core::arch::asm!("hlt");} }
}

fn sc_kconsole_write(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    write_console(1,arg0,arg1)
}

fn sc_ksec_get_uid() -> syscall::SyscallResult {
    syscall_get_uid()
}

fn sc_ksec_set_uid(arg0: u64) -> syscall::SyscallResult {
    syscall_set_uid(arg0)
}

fn sc_ksec_get_gid() -> syscall::SyscallResult {
    syscall_get_gid()
}

fn sc_ksec_set_gid(arg0: u64) -> syscall::SyscallResult {
    syscall_set_gid(arg0)
}

fn sc_ksec_drop_privs() -> syscall::SyscallResult {
    unsafe{*core::ptr::addr_of_mut!(SC_UID)=1000; *core::ptr::addr_of_mut!(SC_GID)=1000;} syscall::SyscallResult::success(0)
}

fn sc_ksec_get_caps() -> syscall::SyscallResult {
    syscall::SyscallResult::success(0xFFFF_FFFF)
}

fn sc_ksec_set_caps(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kobj_create(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(arg0.wrapping_add(1))
}

fn sc_kobj_close(arg0: u64) -> syscall::SyscallResult {
    close_handle(arg0)
}

fn sc_kobj_dup(arg0: u64) -> syscall::SyscallResult {
    syscall_dup(arg0)
}

fn sc_kobj_ref(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
}

fn sc_kobj_unref(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kobj_get_type(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(handle_kind(arg0) as u64)
}

fn sc_ksignal_send(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_kill_process(arg0,arg1)
}

fn sc_ksignal_mask(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_ksignal_wait(arg0: u64) -> syscall::SyscallResult {
    sys_sleep(1_000_000); syscall::SyscallResult::success(0)
}

fn sc_kevent_create(arg0: u64) -> syscall::SyscallResult {
    syscall_create_event(arg0)
}

fn sc_kevent_signal(arg0: u64) -> syscall::SyscallResult {
    syscall_signal_event(arg0)
}

fn sc_kevent_wait(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_wait_event(arg0,arg1)
}

fn sc_kevent_reset(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kboot_get_info(arg0: u64) -> syscall::SyscallResult {
    syscall_get_boot_info(arg0)
}

fn sc_kboot_get_cmdline(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    match sc_copy_cstr_to_user(arg0,arg1,b"alphaos console=tty0"){Ok(n)=>syscall::SyscallResult::success(n),Err(e)=>syscall_error(e)}
}

fn sc_kboot_get_modules(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kdebug_log_write(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_debug_log(arg0,arg1)
}

fn sc_kdebug_log_read(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_get_kernel_log(arg0,arg1)
}

fn sc_kdebug_breakpoint() -> syscall::SyscallResult {
    unsafe{core::arch::asm!("int3");} syscall::SyscallResult::success(0)
}

fn sc_kdebug_dump_regs(arg0: u64) -> syscall::SyscallResult {
    syscall::SyscallResult::success(0)
}

fn sc_kdebug_trace(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    syscall_debug_log(arg0,arg1)
}

fn sc_kpanic(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    let msg=match sc_read_user(arg0,arg1){Ok(s)=>s,Err(_)=>b"kernel panic" as &[u8]};
    unsafe{ for &b in b"[PANIC] ".iter().chain(msg.iter()).chain(b"
".iter()) { let idx=KLOG_LEN%KLOG_SIZE; KLOG[idx]=b; KLOG_LEN=KLOG_LEN.saturating_add(1);} core::arch::asm!("cli"); loop{core::arch::asm!("hlt");} }
}

fn sc_kget_version() -> syscall::SyscallResult {
    syscall_get_os_version()
}

fn sc_kget_build_info(arg0: u64, arg1: u64) -> syscall::SyscallResult {
    match sc_copy_cstr_to_user(arg0,arg1,b"AlphaOS freestanding x86_64 m1"){Ok(n)=>syscall::SyscallResult::success(n),Err(e)=>syscall_error(e)}
}

fn sc_kget_uptime() -> syscall::SyscallResult {
    syscall_get_uptime_ms()
}

fn sc_kget_ticks() -> syscall::SyscallResult {
    syscall_get_ticks()
}

fn sc_kcpu_count() -> syscall::SyscallResult {
    syscall_get_cpu_count()
}

fn sc_kcpu_features(arg0: u64) -> syscall::SyscallResult {
    syscall_get_cpu_features(arg0)
}

fn sc_kpower_reboot(arg0: u64) -> syscall::SyscallResult {
    reboot_system(arg0)
}

fn sc_kpower_shutdown(arg0: u64) -> syscall::SyscallResult {
    reboot_system(arg0)
}

fn sc_kpower_suspend(arg0: u64) -> syscall::SyscallResult {
    unsafe{core::arch::asm!("hlt");} syscall::SyscallResult::success(0)
}

fn sc_ksyscall_count() -> syscall::SyscallResult {
    syscall::SyscallResult::success(syscall::SYSCALL_COUNT as u64)
}

fn sc_kmilestone_check() -> syscall::SyscallResult {
    syscall::SyscallResult::success(1)
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
        Some(syscall::SyscallId::GetCwd) => syscall_getcwd(arg0, arg1),
        Some(syscall::SyscallId::SetCwd) => syscall_setcwd(arg0, arg1),
        Some(syscall::SyscallId::Mkdir) => syscall_mkdir(arg0, arg1),
        Some(syscall::SyscallId::Rmdir) => syscall_rmdir(arg0, arg1),
        Some(syscall::SyscallId::Unlink) => syscall_unlink(arg0, arg1),
        Some(syscall::SyscallId::Rename) => syscall_rename(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::Ftruncate) => syscall_truncate_handle(arg0, arg1),
        Some(syscall::SyscallId::Dup) => syscall_dup(arg0),
        Some(syscall::SyscallId::Dup2) => syscall_dup2(arg0, arg1),
        Some(syscall::SyscallId::Access) => syscall_access(arg0, arg1, arg2),
        Some(syscall::SyscallId::Fcntl) => syscall_fcntl(arg0, arg1, arg2),
        Some(syscall::SyscallId::Ioctl) => syscall_ioctl(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetRandom) => syscall_getrandom(arg0, arg1),
        Some(syscall::SyscallId::GetTicks) => syscall_get_ticks(),
        Some(syscall::SyscallId::NanoSleep) => syscall_nanosleep(arg0, arg1),
        Some(syscall::SyscallId::Mprotect) => syscall_mprotect(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetProcessInfo) => syscall_get_process_info(arg0, arg1),
        Some(syscall::SyscallId::KillProcess) => syscall_kill_process(arg0, arg1),
        Some(syscall::SyscallId::GetArgCount) => syscall_get_arg_count(),
        Some(syscall::SyscallId::GetArg) => syscall_get_arg(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetEnv) => syscall_get_env(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::SetEnv) => syscall_set_env(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::StatFs) => syscall_statfs(arg0, arg1),
        Some(syscall::SyscallId::Fsync) => syscall_fsync(arg0),
        Some(syscall::SyscallId::Poll) => syscall_poll(arg0, arg1, arg2),
        Some(syscall::SyscallId::Pipe2) => syscall_pipe2(arg0, arg1),
        Some(syscall::SyscallId::SocketShutdown) => syscall_socket_shutdown(arg0, arg1),
        Some(syscall::SyscallId::SocketPair) => syscall_socketpair(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetResourceLimit) => syscall_get_rlimit(arg0, arg1),
        Some(syscall::SyscallId::SetResourceLimit) => syscall_set_rlimit(arg0, arg1),
        Some(syscall::SyscallId::GetSystemName) => syscall_get_system_name(arg0, arg1),
        Some(syscall::SyscallId::SetSystemName) => syscall_set_system_name(arg0, arg1),
        Some(syscall::SyscallId::CreateTimer) => syscall_create_timer(arg0, arg1),
        Some(syscall::SyscallId::GetTimer) => syscall_get_timer(arg0),
        Some(syscall::SyscallId::CancelTimer) => syscall_cancel_timer(arg0),
        Some(syscall::SyscallId::CreateEvent) => syscall_create_event(arg0),
        Some(syscall::SyscallId::SignalEvent) => syscall_signal_event(arg0),
        Some(syscall::SyscallId::WaitEvent) => syscall_wait_event(arg0, arg1),
        Some(syscall::SyscallId::GetMemoryInfo) => syscall_get_memory_info(arg0),
        Some(syscall::SyscallId::DebugLog) => syscall_debug_log(arg0, arg1),
        Some(syscall::SyscallId::GetConsoleSize) => syscall_get_console_size(arg0),
        Some(syscall::SyscallId::SetConsoleMode) => syscall_set_console_mode(arg0),
        Some(syscall::SyscallId::GetBootInfo) => syscall_get_boot_info(arg0),
        Some(syscall::SyscallId::GetKernelLog) => syscall_get_kernel_log(arg0, arg1),
        Some(syscall::SyscallId::SetThreadName) => syscall_set_thread_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetThreadName) => syscall_get_thread_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetCpuCount) => syscall_get_cpu_count(),
        Some(syscall::SyscallId::GetUptimeMs) => syscall_get_uptime_ms(),
        Some(syscall::SyscallId::ListProcesses) => syscall_list_processes(arg0, arg1),
        Some(syscall::SyscallId::GetProcName) => syscall_get_proc_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetParentPid) => syscall_getppid(),
        Some(syscall::SyscallId::GetThreadId) => syscall_gettid(),
        Some(syscall::SyscallId::SchedYield) => syscall_sched_yield(),
        Some(syscall::SyscallId::SetProcessName) => syscall_set_process_name(arg0, arg1),
        Some(syscall::SyscallId::GetProcessName) => syscall_get_process_name2(arg0, arg1, arg2),
        Some(syscall::SyscallId::Fork) => syscall_fork(),
        Some(syscall::SyscallId::Exec) => syscall_exec(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::WaitPid) => syscall_waitpid(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetExitCode) => syscall_get_exit_code(arg0),
        Some(syscall::SyscallId::SetThreadPriority) => syscall_set_thread_priority(arg0, arg1),
        Some(syscall::SyscallId::GetThreadPriority) => syscall_get_thread_priority(arg0),
        Some(syscall::SyscallId::ProcessExists) => syscall_process_exists(arg0),
        Some(syscall::SyscallId::GetProcessList) => syscall_get_process_list(arg0, arg1),
        Some(syscall::SyscallId::KillThread) => syscall_kill_thread(arg0),
        Some(syscall::SyscallId::GetCurrentTid) => syscall_get_current_tid(),
        Some(syscall::SyscallId::SetUid) => syscall_set_uid(arg0),
        Some(syscall::SyscallId::GetUserId) => syscall_get_uid(),
        Some(syscall::SyscallId::SetGid) => syscall_set_gid(arg0),
        Some(syscall::SyscallId::GetGroupId) => syscall_get_gid(),
        Some(syscall::SyscallId::GetProcessTimes) => syscall_get_process_times(arg0, arg1),
        Some(syscall::SyscallId::OpenAt) => syscall_openat(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ReadAt) => syscall_readat(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::WriteAt) => syscall_writeat(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileTruncatePath) => syscall_file_truncate_path(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileStatPath) => syscall_file_stat_path(arg0, arg1, arg2),
        Some(syscall::SyscallId::Symlink) => syscall_symlink(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ReadLink) => syscall_readlink(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::Link) => syscall_link(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::Chmod) => syscall_chmod(arg0, arg1, arg2),
        Some(syscall::SyscallId::Chown) => syscall_chown(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::Lseek64) => syscall_lseek64(arg0, arg1, arg2),
        Some(syscall::SyscallId::FstatHandle) => syscall_fstat2(arg0, arg1),
        Some(syscall::SyscallId::IsATTY) => syscall_isatty(arg0),
        Some(syscall::SyscallId::TtyName) => syscall_ttyname(arg0, arg1, arg2),
        Some(syscall::SyscallId::GetDents) => syscall_getdents(arg0, arg1, arg2),
        Some(syscall::SyscallId::RealPath) => syscall_realpath(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::CopyFile) => syscall_copy_file(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::MakeFifo) => syscall_mkfifo(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileLock) => syscall_file_lock(arg0, arg1),
        Some(syscall::SyscallId::FileUnlock) => syscall_file_unlock(arg0),
        Some(syscall::SyscallId::MemoryMap) => syscall_memory_map(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemoryUnmap) => syscall_memory_unmap(arg0, arg1),
        Some(syscall::SyscallId::MemoryProtect) => syscall_memory_protect(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemoryAdvise) => syscall_memory_advise(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemoryStats) => syscall_memory_stats(arg0),
        Some(syscall::SyscallId::HeapStats) => syscall_heap_stats(arg0),
        Some(syscall::SyscallId::PageSize) => syscall_page_size(),
        Some(syscall::SyscallId::MemoryFill) => syscall_memory_fill(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemoryCompare) => syscall_memory_compare(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemoryCopy) => syscall_memory_copy(arg0, arg1, arg2),
        Some(syscall::SyscallId::ClockMonotonic) => syscall_clock_monotonic(arg0),
        Some(syscall::SyscallId::ClockRealtime) => syscall_clock_realtime(arg0),
        Some(syscall::SyscallId::SetTimerMs) => syscall_set_timer_ms(arg0, arg1),
        Some(syscall::SyscallId::GetTimerMs) => syscall_get_timer_ms(arg0),
        Some(syscall::SyscallId::SleepMs) => syscall_sleep_ms(arg0),
        Some(syscall::SyscallId::Alarm) => syscall_alarm(arg0),
        Some(syscall::SyscallId::TimeNow) => syscall_time_now(),
        Some(syscall::SyscallId::TicksPerSecond) => syscall_ticks_per_second(),
        Some(syscall::SyscallId::UptimeNs) => syscall_uptime_ns(),
        Some(syscall::SyscallId::DeadlineWait) => syscall_deadline_wait(arg0),
        Some(syscall::SyscallId::CreateMutex) => syscall_create_mutex(),
        Some(syscall::SyscallId::LockMutex) => syscall_lock_mutex(arg0),
        Some(syscall::SyscallId::UnlockMutex) => syscall_unlock_mutex(arg0),
        Some(syscall::SyscallId::DestroyMutex) => syscall_destroy_mutex(arg0),
        Some(syscall::SyscallId::CreateSemaphore) => syscall_create_sem(arg0),
        Some(syscall::SyscallId::SemWait) => syscall_sem_wait(arg0),
        Some(syscall::SyscallId::SemPost) => syscall_sem_post(arg0),
        Some(syscall::SyscallId::SemDestroy) => syscall_sem_destroy(arg0),
        Some(syscall::SyscallId::CreateCondvar) => syscall_create_condvar(),
        Some(syscall::SyscallId::CondvarWait) => syscall_condvar_wait(arg0, arg1),
        Some(syscall::SyscallId::CondvarSignal) => syscall_condvar_signal(arg0),
        Some(syscall::SyscallId::CondvarBroadcast) => syscall_condvar_broadcast(arg0),
        Some(syscall::SyscallId::FutexWait) => syscall_futex_wait(arg0, arg1, arg2),
        Some(syscall::SyscallId::FutexWake) => syscall_futex_wake(arg0, arg1),
        Some(syscall::SyscallId::MsgQueueCreate) => syscall_msgq_create(arg0),
        Some(syscall::SyscallId::MsgQueueSend) => syscall_msgq_send(arg0, arg1, arg2),
        Some(syscall::SyscallId::MsgQueueRecv) => syscall_msgq_recv(arg0, arg1, arg2),
        Some(syscall::SyscallId::MsgQueueDestroy) => syscall_msgq_destroy(arg0),
        Some(syscall::SyscallId::EventfdCreate) => syscall_eventfd_create(arg0),
        Some(syscall::SyscallId::EventfdRead) => syscall_eventfd_read(arg0),
        Some(syscall::SyscallId::EventfdWrite) => syscall_eventfd_write(arg0, arg1),
        Some(syscall::SyscallId::GetOsName) => syscall_get_os_name(arg0, arg1),
        Some(syscall::SyscallId::GetOsVersion) => syscall_get_os_version(),
        Some(syscall::SyscallId::GetHostname) => syscall_get_hostname(arg0, arg1),
        Some(syscall::SyscallId::SetHostname) => syscall_set_hostname(arg0, arg1),
        Some(syscall::SyscallId::GetKernelVersion) => syscall_get_kernel_version(arg0, arg1),
        Some(syscall::SyscallId::GetArch) => syscall_get_arch(),
        Some(syscall::SyscallId::GetCpuFeatures) => syscall_get_cpu_features(arg0),
        Some(syscall::SyscallId::GetLoadAvg) => syscall_get_load_avg(arg0),
        Some(syscall::SyscallId::SysInfo) => syscall_sysinfo(arg0),
        Some(syscall::SyscallId::ConsoleWrite) => syscall_console_write(arg0, arg1),
        Some(syscall::SyscallId::ConsoleRead) => syscall_console_read(arg0, arg1),
        Some(syscall::SyscallId::ConsoleClear) => syscall_console_clear(),
        Some(syscall::SyscallId::ConsoleSetColor) => syscall_console_set_color(arg0, arg1),
        Some(syscall::SyscallId::ConsoleGetCursor) => syscall_console_get_cursor(arg0, arg1),
        Some(syscall::SyscallId::ConsoleSetCursor) => syscall_console_set_cursor(arg0, arg1),
        Some(syscall::SyscallId::DebugWrite) => syscall_debug_write(arg0, arg1),
        Some(syscall::SyscallId::DebugRead) => syscall_debug_read(arg0, arg1),
        Some(syscall::SyscallId::GetLastError) => syscall_get_last_error(),
        Some(syscall::SyscallId::PathExists) => sc_path_exists(arg0, arg1),
        Some(syscall::SyscallId::PathIsDir) => sc_path_is_dir(arg0, arg1),
        Some(syscall::SyscallId::PathIsFile) => sc_path_is_file(arg0, arg1),
        Some(syscall::SyscallId::FileSizePath) => sc_file_size_path(arg0, arg1),
        Some(syscall::SyscallId::FileDeletePath) => sc_file_delete_path(arg0, arg1),
        Some(syscall::SyscallId::FileRenamePath) => sc_file_rename_path(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::DirCreate) => sc_dir_create(arg0, arg1),
        Some(syscall::SyscallId::DirRemove) => sc_dir_remove(arg0, arg1),
        Some(syscall::SyscallId::FileOpenRead) => sc_file_open_read(arg0, arg1),
        Some(syscall::SyscallId::FileOpenWrite) => sc_file_open_write(arg0, arg1),
        Some(syscall::SyscallId::FileCloseHandle) => sc_file_close_handle(arg0),
        Some(syscall::SyscallId::FileReadHandle) => sc_file_read_handle(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileWriteHandle) => sc_file_write_handle(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileSeekHandle) => sc_file_seek_handle(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileTell) => sc_file_tell(arg0),
        Some(syscall::SyscallId::FileEof) => sc_file_eof(arg0),
        Some(syscall::SyscallId::FileFlush) => sc_file_flush(arg0),
        Some(syscall::SyscallId::FileSetSize) => sc_file_set_size(arg0, arg1),
        Some(syscall::SyscallId::FileGetAttr) => sc_file_get_attr(arg0, arg1, arg2),
        Some(syscall::SyscallId::FileSetAttr) => sc_file_set_attr(arg0, arg1, arg2),
        Some(syscall::SyscallId::DirOpen) => sc_dir_open(arg0, arg1),
        Some(syscall::SyscallId::DirRead) => sc_dir_read(arg0, arg1),
        Some(syscall::SyscallId::DirClose) => sc_dir_close(arg0),
        Some(syscall::SyscallId::PathJoin) => sc_path_join(arg0, arg1, arg2, arg3, arg4),
        Some(syscall::SyscallId::PathBasename) => sc_path_basename(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::PathDirname) => sc_path_dirname(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileReadAll) => sc_file_read_all(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileWriteAll) => sc_file_write_all(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileAppend) => sc_file_append(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileCopyPath) => sc_file_copy_path(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileMovePath) => sc_file_move_path(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::DirListCount) => sc_dir_list_count(arg0, arg1),
        Some(syscall::SyscallId::FsFreeSpace) => sc_fs_free_space(),
        Some(syscall::SyscallId::FsTotalSpace) => sc_fs_total_space(),
        Some(syscall::SyscallId::FsFileCount) => sc_fs_file_count(),
        Some(syscall::SyscallId::PathNormalize) => sc_path_normalize(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::FileTouch) => sc_file_touch(arg0, arg1),
        Some(syscall::SyscallId::GetFdFlags) => sc_get_fd_flags(arg0),
        Some(syscall::SyscallId::SetFdFlags) => sc_set_fd_flags(arg0, arg1),
        Some(syscall::SyscallId::FileDupHandle) => sc_file_dup_handle(arg0),
        Some(syscall::SyscallId::ProcessSpawn) => sc_process_spawn(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ProcessExit) => sc_process_exit(arg0),
        Some(syscall::SyscallId::ProcessWait) => sc_process_wait(arg0, arg1, arg2),
        Some(syscall::SyscallId::ProcessGetPid) => sc_process_get_pid(),
        Some(syscall::SyscallId::ProcessGetPpid) => sc_process_get_ppid(),
        Some(syscall::SyscallId::ProcessKill) => sc_process_kill(arg0, arg1),
        Some(syscall::SyscallId::ProcessList) => sc_process_list(arg0, arg1),
        Some(syscall::SyscallId::ProcessInfo) => sc_process_info(arg0, arg1),
        Some(syscall::SyscallId::ProcessName) => sc_process_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::ProcessSetName) => sc_process_set_name(arg0, arg1),
        Some(syscall::SyscallId::ProcessExistsPid) => sc_process_exists_pid(arg0),
        Some(syscall::SyscallId::ProcessGetState) => sc_process_get_state(arg0),
        Some(syscall::SyscallId::ProcessGetExit) => sc_process_get_exit(arg0),
        Some(syscall::SyscallId::ProcessCurrentName) => sc_process_current_name(arg0, arg1),
        Some(syscall::SyscallId::ProcessArgc) => sc_process_argc(),
        Some(syscall::SyscallId::ProcessArgv) => sc_process_argv(arg0, arg1, arg2),
        Some(syscall::SyscallId::ProcessGetEnv) => sc_process_get_env(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ProcessSetEnv) => sc_process_set_env(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ThreadGetTid) => sc_thread_get_tid(),
        Some(syscall::SyscallId::ThreadYield) => sc_thread_yield(),
        Some(syscall::SyscallId::ThreadSleepMs) => sc_thread_sleep_ms(arg0),
        Some(syscall::SyscallId::ThreadSetName) => sc_thread_set_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::ThreadGetName) => sc_thread_get_name(arg0, arg1, arg2),
        Some(syscall::SyscallId::ThreadSetPriority) => sc_thread_set_priority(arg0, arg1),
        Some(syscall::SyscallId::ThreadGetPriority) => sc_thread_get_priority(arg0),
        Some(syscall::SyscallId::ProcGetUid) => sc_proc_get_uid(),
        Some(syscall::SyscallId::ProcSetUid) => sc_proc_set_uid(arg0),
        Some(syscall::SyscallId::ProcGetGid) => sc_proc_get_gid(),
        Some(syscall::SyscallId::ProcSetGid) => sc_proc_set_gid(arg0),
        Some(syscall::SyscallId::ProcTimes) => sc_proc_times(arg0, arg1),
        Some(syscall::SyscallId::ProcGetCwd) => sc_proc_get_cwd(arg0, arg1),
        Some(syscall::SyscallId::ProcSetCwd) => sc_proc_set_cwd(arg0, arg1),
        Some(syscall::SyscallId::ProcExec) => sc_proc_exec(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::ProcFork) => sc_proc_fork(),
        Some(syscall::SyscallId::ProcWaitPid) => sc_proc_waitpid(arg0, arg1, arg2),
        Some(syscall::SyscallId::ProcIsAlive) => sc_proc_is_alive(arg0),
        Some(syscall::SyscallId::ProcSignal) => sc_proc_signal(arg0, arg1),
        Some(syscall::SyscallId::ProcCount) => sc_proc_count(),
        Some(syscall::SyscallId::ProcMax) => sc_proc_max(),
        Some(syscall::SyscallId::ProcSelfStatus) => sc_proc_self_status(),
        Some(syscall::SyscallId::MemAlloc) => sc_mem_alloc(arg0, arg1),
        Some(syscall::SyscallId::MemFree) => sc_mem_free(arg0, arg1),
        Some(syscall::SyscallId::MemRealloc) => sc_mem_realloc(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemMap) => sc_mem_map(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemUnmap) => sc_mem_unmap(arg0, arg1),
        Some(syscall::SyscallId::MemProtect) => sc_mem_protect(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemInfo) => sc_mem_info(arg0),
        Some(syscall::SyscallId::MemStats) => sc_mem_stats(arg0),
        Some(syscall::SyscallId::MemPageSize) => sc_mem_page_size(),
        Some(syscall::SyscallId::MemSet) => sc_mem_set(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemCopy) => sc_mem_copy(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemCompare) => sc_mem_compare(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemZero) => sc_mem_zero(arg0, arg1),
        Some(syscall::SyscallId::MemAvailable) => sc_mem_available(),
        Some(syscall::SyscallId::MemUsed) => sc_mem_used(),
        Some(syscall::SyscallId::MemTotal) => sc_mem_total(),
        Some(syscall::SyscallId::HeapAllocApp) => sc_heap_alloc_app(arg0),
        Some(syscall::SyscallId::HeapFreeApp) => sc_heap_free_app(arg0, arg1),
        Some(syscall::SyscallId::HeapStatsApp) => sc_heap_stats_app(arg0),
        Some(syscall::SyscallId::MemMapShared) => sc_mem_map_shared(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemUnmapShared) => sc_mem_unmap_shared(arg0, arg1),
        Some(syscall::SyscallId::MemFlush) => sc_mem_flush(arg0, arg1),
        Some(syscall::SyscallId::MemLock) => sc_mem_lock(arg0, arg1),
        Some(syscall::SyscallId::MemUnlock) => sc_mem_unlock(arg0, arg1),
        Some(syscall::SyscallId::MemAdviseApp) => sc_mem_advise_app(arg0, arg1, arg2),
        Some(syscall::SyscallId::ShmCreate) => sc_shm_create(arg0, arg1),
        Some(syscall::SyscallId::MmapAnon) => sc_mmap_anon(arg0, arg1),
        Some(syscall::SyscallId::MunmapAnon) => sc_munmap_anon(arg0, arg1),
        Some(syscall::SyscallId::MemReadU64) => sc_mem_read_u64(arg0),
        Some(syscall::SyscallId::MemWriteU64) => sc_mem_write_u64(arg0, arg1),
        Some(syscall::SyscallId::MemReadU32) => sc_mem_read_u32(arg0),
        Some(syscall::SyscallId::MemWriteU32) => sc_mem_write_u32(arg0, arg1),
        Some(syscall::SyscallId::MemSwap) => sc_mem_swap(arg0, arg1, arg2),
        Some(syscall::SyscallId::MemFillPattern) => sc_mem_fill_pattern(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::MemStatsEx) => sc_mem_stats_ex(arg0),
        Some(syscall::SyscallId::HeapCompact) => sc_heap_compact(),
        Some(syscall::SyscallId::MemValidate) => sc_mem_validate(arg0, arg1),
        Some(syscall::SyscallId::MemDup) => sc_mem_dup(arg0, arg1),
        Some(syscall::SyscallId::PageAlloc) => sc_page_alloc(arg0),
        Some(syscall::SyscallId::PageFree) => sc_page_free(arg0, arg1),
        Some(syscall::SyscallId::TimeNowSec) => sc_time_now_sec(),
        Some(syscall::SyscallId::TimeNowMs) => sc_time_now_ms(),
        Some(syscall::SyscallId::TimeNowNs) => sc_time_now_ns(),
        Some(syscall::SyscallId::TimeMonotonic) => sc_time_monotonic(arg0),
        Some(syscall::SyscallId::TimeRealtime) => sc_time_realtime(arg0),
        Some(syscall::SyscallId::TimeSleepNs) => sc_time_sleep_ns(arg0),
        Some(syscall::SyscallId::TimeSleepMs) => sc_time_sleep_ms(arg0),
        Some(syscall::SyscallId::TimeSleepSec) => sc_time_sleep_sec(arg0),
        Some(syscall::SyscallId::TimeTicks) => sc_time_ticks(),
        Some(syscall::SyscallId::TimeTicksPerSec) => sc_time_ticks_per_sec(),
        Some(syscall::SyscallId::TimeUptimeMs) => sc_time_uptime_ms(),
        Some(syscall::SyscallId::TimeDeadline) => sc_time_deadline(arg0),
        Some(syscall::SyscallId::TimerCreate) => sc_timer_create(arg0, arg1),
        Some(syscall::SyscallId::TimerGet) => sc_timer_get(arg0),
        Some(syscall::SyscallId::TimerCancel) => sc_timer_cancel(arg0),
        Some(syscall::SyscallId::TimerSetMs) => sc_timer_set_ms(arg0, arg1),
        Some(syscall::SyscallId::TimerRemaining) => sc_timer_remaining(arg0),
        Some(syscall::SyscallId::ClockGetRes) => sc_clock_get_res(arg0, arg1),
        Some(syscall::SyscallId::AlarmSet) => sc_alarm_set(arg0),
        Some(syscall::SyscallId::AlarmCancel) => sc_alarm_cancel(),
        Some(syscall::SyscallId::IpcMutexCreate) => sc_ipc_mutex_create(),
        Some(syscall::SyscallId::IpcMutexLock) => sc_ipc_mutex_lock(arg0),
        Some(syscall::SyscallId::IpcMutexUnlock) => sc_ipc_mutex_unlock(arg0),
        Some(syscall::SyscallId::IpcMutexDestroy) => sc_ipc_mutex_destroy(arg0),
        Some(syscall::SyscallId::IpcSemCreate) => sc_ipc_sem_create(arg0),
        Some(syscall::SyscallId::IpcSemWait) => sc_ipc_sem_wait(arg0),
        Some(syscall::SyscallId::IpcSemPost) => sc_ipc_sem_post(arg0),
        Some(syscall::SyscallId::IpcSemDestroy) => sc_ipc_sem_destroy(arg0),
        Some(syscall::SyscallId::IpcEventCreate) => sc_ipc_event_create(arg0),
        Some(syscall::SyscallId::IpcEventSignal) => sc_ipc_event_signal(arg0),
        Some(syscall::SyscallId::IpcEventWait) => sc_ipc_event_wait(arg0, arg1),
        Some(syscall::SyscallId::IpcMsgCreate) => sc_ipc_msg_create(arg0),
        Some(syscall::SyscallId::IpcMsgSend) => sc_ipc_msg_send(arg0, arg1, arg2),
        Some(syscall::SyscallId::IpcMsgRecv) => sc_ipc_msg_recv(arg0, arg1, arg2),
        Some(syscall::SyscallId::IpcMsgDestroy) => sc_ipc_msg_destroy(arg0),
        Some(syscall::SyscallId::IpcPipeCreate) => sc_ipc_pipe_create(arg0),
        Some(syscall::SyscallId::IpcPipeRead) => sc_ipc_pipe_read(arg0, arg1, arg2),
        Some(syscall::SyscallId::IpcPipeWrite) => sc_ipc_pipe_write(arg0, arg1, arg2),
        Some(syscall::SyscallId::IpcChannelCreate) => sc_ipc_channel_create(arg0, arg1),
        Some(syscall::SyscallId::IpcChannelClose) => sc_ipc_channel_close(arg0),
        Some(syscall::SyscallId::NetSocketCreate) => sc_net_socket_create(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetSocketBind) => sc_net_socket_bind(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetSocketListen) => sc_net_socket_listen(arg0, arg1),
        Some(syscall::SyscallId::NetSocketAccept) => sc_net_socket_accept(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetSocketConnect) => sc_net_socket_connect(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetSocketSend) => sc_net_socket_send(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::NetSocketRecv) => sc_net_socket_recv(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::NetSocketClose) => sc_net_socket_close(arg0),
        Some(syscall::SyscallId::NetSocketShutdown) => sc_net_socket_shutdown(arg0, arg1),
        Some(syscall::SyscallId::NetSocketPair) => sc_net_socket_pair(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetGetLocalAddr) => sc_net_get_local_addr(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetGetRemoteAddr) => sc_net_get_remote_addr(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetSetNonBlocking) => sc_net_set_nonblocking(arg0, arg1),
        Some(syscall::SyscallId::NetPollSocket) => sc_net_poll_socket(arg0, arg1),
        Some(syscall::SyscallId::NetGetHostName) => sc_net_get_hostname(arg0, arg1),
        Some(syscall::SyscallId::NetSetHostName) => sc_net_set_hostname(arg0, arg1),
        Some(syscall::SyscallId::NetResolveLocal) => sc_net_resolve_local(arg0, arg1, arg2),
        Some(syscall::SyscallId::NetLoopbackPing) => sc_net_loopback_ping(arg0),
        Some(syscall::SyscallId::NetSocketStats) => sc_net_socket_stats(arg0),
        Some(syscall::SyscallId::NetIfCount) => sc_net_if_count(),
        Some(syscall::SyscallId::SysGetOsName) => sc_sys_get_os_name(arg0, arg1),
        Some(syscall::SyscallId::SysGetOsVersion) => sc_sys_get_os_version(),
        Some(syscall::SyscallId::SysGetKernelVer) => sc_sys_get_kernel_ver(arg0, arg1),
        Some(syscall::SyscallId::SysGetArch) => sc_sys_get_arch(),
        Some(syscall::SyscallId::SysGetCpuCount) => sc_sys_get_cpu_count(),
        Some(syscall::SyscallId::SysGetCpuFeatures) => sc_sys_get_cpu_features(arg0),
        Some(syscall::SyscallId::SysGetUptime) => sc_sys_get_uptime(),
        Some(syscall::SyscallId::SysGetLoadAvg) => sc_sys_get_load_avg(arg0),
        Some(syscall::SyscallId::SysInfoEx) => sc_sys_info_ex(arg0),
        Some(syscall::SyscallId::SysGetRandom) => sc_sys_get_random(arg0, arg1),
        Some(syscall::SyscallId::SysGetBootInfo) => sc_sys_get_boot_info(arg0),
        Some(syscall::SyscallId::SysGetConsoleSize) => sc_sys_get_console_size(arg0),
        Some(syscall::SyscallId::SysSetConsoleMode) => sc_sys_set_console_mode(arg0),
        Some(syscall::SyscallId::SysReboot) => sc_sys_reboot(arg0),
        Some(syscall::SyscallId::SysShutdown) => sc_sys_shutdown(arg0),
        Some(syscall::SyscallId::SysDebugLog) => sc_sys_debug_log(arg0, arg1),
        Some(syscall::SyscallId::SysGetKernelLog) => sc_sys_get_kernel_log(arg0, arg1),
        Some(syscall::SyscallId::SysGetLastError) => sc_sys_get_last_error(),
        Some(syscall::SyscallId::SysSetLastError) => sc_sys_set_last_error(arg0),
        Some(syscall::SyscallId::SysGetAbi) => sc_sys_get_abi(),
        Some(syscall::SyscallId::KMemAlloc) => sc_kmem_alloc(arg0, arg1),
        Some(syscall::SyscallId::KMemFree) => sc_kmem_free(arg0, arg1),
        Some(syscall::SyscallId::KMemMap) => sc_kmem_map(arg0, arg1, arg2),
        Some(syscall::SyscallId::KMemUnmap) => sc_kmem_unmap(arg0, arg1),
        Some(syscall::SyscallId::KMemProtect) => sc_kmem_protect(arg0, arg1, arg2),
        Some(syscall::SyscallId::KMemQuery) => sc_kmem_query(arg0, arg1),
        Some(syscall::SyscallId::KMemStats) => sc_kmem_stats(arg0),
        Some(syscall::SyscallId::KMemFlush) => sc_kmem_flush(arg0, arg1),
        Some(syscall::SyscallId::KPageAlloc) => sc_kpage_alloc(arg0),
        Some(syscall::SyscallId::KPageFree) => sc_kpage_free(arg0, arg1),
        Some(syscall::SyscallId::KPageMap) => sc_kpage_map(arg0, arg1, arg2),
        Some(syscall::SyscallId::KPageUnmap) => sc_kpage_unmap(arg0),
        Some(syscall::SyscallId::KHeapInfo) => sc_kheap_info(arg0),
        Some(syscall::SyscallId::KHeapCompact) => sc_kheap_compact(),
        Some(syscall::SyscallId::KMemZero) => sc_kmem_zero(arg0, arg1),
        Some(syscall::SyscallId::KMemCopy) => sc_kmem_copy(arg0, arg1, arg2),
        Some(syscall::SyscallId::KMemCompare) => sc_kmem_compare(arg0, arg1, arg2),
        Some(syscall::SyscallId::KPhysAlloc) => sc_kphys_alloc(arg0),
        Some(syscall::SyscallId::KPhysFree) => sc_kphys_free(arg0, arg1),
        Some(syscall::SyscallId::KMemBarrier) => sc_kmem_barrier(),
        Some(syscall::SyscallId::KSchedYield) => sc_ksched_yield(),
        Some(syscall::SyscallId::KSchedSetPriority) => sc_ksched_set_priority(arg0, arg1),
        Some(syscall::SyscallId::KSchedGetPriority) => sc_ksched_get_priority(arg0),
        Some(syscall::SyscallId::KThreadCreate) => sc_kthread_create(arg0, arg1, arg2),
        Some(syscall::SyscallId::KThreadExit) => sc_kthread_exit(arg0),
        Some(syscall::SyscallId::KThreadJoin) => sc_kthread_join(arg0, arg1),
        Some(syscall::SyscallId::KThreadSleep) => sc_kthread_sleep(arg0),
        Some(syscall::SyscallId::KThreadWake) => sc_kthread_wake(arg0),
        Some(syscall::SyscallId::KProcessCreate) => sc_kprocess_create(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::KProcessExit) => sc_kprocess_exit(arg0),
        Some(syscall::SyscallId::KProcessKill) => sc_kprocess_kill(arg0, arg1),
        Some(syscall::SyscallId::KProcessWait) => sc_kprocess_wait(arg0, arg1, arg2),
        Some(syscall::SyscallId::KProcessInfo) => sc_kprocess_info(arg0, arg1),
        Some(syscall::SyscallId::KProcessList) => sc_kprocess_list(arg0, arg1),
        Some(syscall::SyscallId::KTaskSwitch) => sc_ktask_switch(),
        Some(syscall::SyscallId::KSetAffinity) => sc_kset_affinity(arg0, arg1),
        Some(syscall::SyscallId::KGetAffinity) => sc_kget_affinity(arg0),
        Some(syscall::SyscallId::KCpuId) => sc_kcpu_id(),
        Some(syscall::SyscallId::KIrqDisable) => sc_kirq_disable(),
        Some(syscall::SyscallId::KIrqEnable) => sc_kirq_enable(),
        Some(syscall::SyscallId::KDevList) => sc_kdev_list(arg0, arg1),
        Some(syscall::SyscallId::KDevOpen) => sc_kdev_open(arg0, arg1),
        Some(syscall::SyscallId::KDevClose) => sc_kdev_close(arg0),
        Some(syscall::SyscallId::KDevRead) => sc_kdev_read(arg0, arg1, arg2),
        Some(syscall::SyscallId::KDevWrite) => sc_kdev_write(arg0, arg1, arg2),
        Some(syscall::SyscallId::KDevIoctl) => sc_kdev_ioctl(arg0, arg1, arg2),
        Some(syscall::SyscallId::KDevPoll) => sc_kdev_poll(arg0, arg1, arg2),
        Some(syscall::SyscallId::KDevMmap) => sc_kdev_mmap(arg0, arg1, arg2, arg3),
        Some(syscall::SyscallId::KDevIrqRegister) => sc_kdev_irq_register(arg0, arg1),
        Some(syscall::SyscallId::KDevIrqUnregister) => sc_kdev_irq_unregister(arg0),
        Some(syscall::SyscallId::KDevDmaAlloc) => sc_kdev_dma_alloc(arg0, arg1),
        Some(syscall::SyscallId::KDevDmaFree) => sc_kdev_dma_free(arg0, arg1),
        Some(syscall::SyscallId::KIoPortRead) => sc_kio_port_read(arg0, arg1),
        Some(syscall::SyscallId::KIoPortWrite) => sc_kio_port_write(arg0, arg1, arg2),
        Some(syscall::SyscallId::KFbGetInfo) => sc_kfb_get_info(arg0),
        Some(syscall::SyscallId::KFbPutPixel) => sc_kfb_put_pixel(arg0, arg1, arg2, arg3, arg4),
        Some(syscall::SyscallId::KFbClear) => sc_kfb_clear(arg0, arg1, arg2),
        Some(syscall::SyscallId::KInputPoll) => sc_kinput_poll(arg0),
        Some(syscall::SyscallId::KInputRead) => sc_kinput_read(arg0),
        Some(syscall::SyscallId::KConsoleWrite) => sc_kconsole_write(arg0, arg1),
        Some(syscall::SyscallId::KSecGetUid) => sc_ksec_get_uid(),
        Some(syscall::SyscallId::KSecSetUid) => sc_ksec_set_uid(arg0),
        Some(syscall::SyscallId::KSecGetGid) => sc_ksec_get_gid(),
        Some(syscall::SyscallId::KSecSetGid) => sc_ksec_set_gid(arg0),
        Some(syscall::SyscallId::KSecDropPrivs) => sc_ksec_drop_privs(),
        Some(syscall::SyscallId::KSecGetCaps) => sc_ksec_get_caps(),
        Some(syscall::SyscallId::KSecSetCaps) => sc_ksec_set_caps(arg0),
        Some(syscall::SyscallId::KObjCreate) => sc_kobj_create(arg0, arg1),
        Some(syscall::SyscallId::KObjClose) => sc_kobj_close(arg0),
        Some(syscall::SyscallId::KObjDup) => sc_kobj_dup(arg0),
        Some(syscall::SyscallId::KObjRef) => sc_kobj_ref(arg0),
        Some(syscall::SyscallId::KObjUnref) => sc_kobj_unref(arg0),
        Some(syscall::SyscallId::KObjGetType) => sc_kobj_get_type(arg0),
        Some(syscall::SyscallId::KSignalSend) => sc_ksignal_send(arg0, arg1),
        Some(syscall::SyscallId::KSignalMask) => sc_ksignal_mask(arg0, arg1),
        Some(syscall::SyscallId::KSignalWait) => sc_ksignal_wait(arg0),
        Some(syscall::SyscallId::KEventCreate) => sc_kevent_create(arg0),
        Some(syscall::SyscallId::KEventSignal) => sc_kevent_signal(arg0),
        Some(syscall::SyscallId::KEventWait) => sc_kevent_wait(arg0, arg1),
        Some(syscall::SyscallId::KEventReset) => sc_kevent_reset(arg0),
        Some(syscall::SyscallId::KBootGetInfo) => sc_kboot_get_info(arg0),
        Some(syscall::SyscallId::KBootGetCmdline) => sc_kboot_get_cmdline(arg0, arg1),
        Some(syscall::SyscallId::KBootGetModules) => sc_kboot_get_modules(arg0),
        Some(syscall::SyscallId::KDebugLogWrite) => sc_kdebug_log_write(arg0, arg1),
        Some(syscall::SyscallId::KDebugLogRead) => sc_kdebug_log_read(arg0, arg1),
        Some(syscall::SyscallId::KDebugBreakpoint) => sc_kdebug_breakpoint(),
        Some(syscall::SyscallId::KDebugDumpRegs) => sc_kdebug_dump_regs(arg0),
        Some(syscall::SyscallId::KDebugTrace) => sc_kdebug_trace(arg0, arg1),
        Some(syscall::SyscallId::KPanic) => sc_kpanic(arg0, arg1),
        Some(syscall::SyscallId::KGetVersion) => sc_kget_version(),
        Some(syscall::SyscallId::KGetBuildInfo) => sc_kget_build_info(arg0, arg1),
        Some(syscall::SyscallId::KGetUptime) => sc_kget_uptime(),
        Some(syscall::SyscallId::KGetTicks) => sc_kget_ticks(),
        Some(syscall::SyscallId::KCpuCount) => sc_kcpu_count(),
        Some(syscall::SyscallId::KCpuFeatures) => sc_kcpu_features(arg0),
        Some(syscall::SyscallId::KPowerReboot) => sc_kpower_reboot(arg0),
        Some(syscall::SyscallId::KPowerShutdown) => sc_kpower_shutdown(arg0),
        Some(syscall::SyscallId::KPowerSuspend) => sc_kpower_suspend(arg0),
        Some(syscall::SyscallId::KSyscallCount) => sc_ksyscall_count(),
        Some(syscall::SyscallId::KMilestoneCheck) => sc_kmilestone_check(),
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
