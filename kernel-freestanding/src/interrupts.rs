use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU64, Ordering};

const IDT_ENTRIES: usize = 256;
const EXCEPTION_COUNT: usize = 32;
const TIMER_VECTOR: usize = 0x20;
const KEYBOARD_VECTOR: usize = 0x21;
const TASK_STATE_SELECTOR: u16 = 0x28;
const INTERRUPT_STACK_SIZE: usize = 16 * 1024;
const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;
const PIT_INPUT_HZ: u32 = 1_193_182;
const PIT_FREQUENCY_HZ: u32 = 100;

pub type ExceptionReporter = extern "C" fn(u64, u64, u64, u64, u64, u64) -> !;

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

#[repr(C, align(16))]
struct InterruptStack([u8; INTERRUPT_STACK_SIZE]);

#[repr(C, align(16))]
struct DoubleFaultStack([u8; DOUBLE_FAULT_STACK_SIZE]);

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_middle: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn interrupt_gate(handler: u64) -> Self {
        Self::interrupt_gate_with_ist(handler, 0)
    }

    fn interrupt_gate_with_ist(handler: u64, ist: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: 0x08,
            ist: ist & 0x7,
            attributes: 0x8E,
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

static mut GDT: [u64; 7] = [
    0,
    0x00AF_9B00_0000_FFFF,
    0x00CF_9300_0000_FFFF,
    0x00CF_F300_0000_FFFF,
    0x00AF_FB00_0000_FFFF,
    0,
    0,
];
static mut TSS: TaskStateSegment = TaskStateSegment {
    reserved0: 0,
    rsp: [0; 3],
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iomap_base: core::mem::size_of::<TaskStateSegment>() as u16,
};
static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::MISSING; IDT_ENTRIES];
static mut INTERRUPT_STACK: InterruptStack = InterruptStack([0; INTERRUPT_STACK_SIZE]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; DOUBLE_FAULT_STACK_SIZE]);
static mut EXCEPTION_REPORTER: Option<ExceptionReporter> = None;
static TIMER_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
static KEYBOARD_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

global_asm!(
    ".section .text",
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
    ".macro SAVE_REGISTERS",
    "cld",
    "push rax",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push rbx",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    ".endm",
    ".macro RESTORE_REGISTERS",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbx",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rax",
    ".endm",
    ".macro EXCEPTION_NO_ERROR vector",
    ".global exception_stub_\\vector",
    "exception_stub_\\vector:",
    "push 0",
    "push \\vector",
    "jmp exception_common",
    ".endm",
    ".macro EXCEPTION_HAS_ERROR vector",
    ".global exception_stub_\\vector",
    "exception_stub_\\vector:",
    "push \\vector",
    "jmp exception_common",
    ".endm",
    "EXCEPTION_NO_ERROR 0",
    "EXCEPTION_NO_ERROR 1",
    "EXCEPTION_NO_ERROR 2",
    "EXCEPTION_NO_ERROR 3",
    "EXCEPTION_NO_ERROR 4",
    "EXCEPTION_NO_ERROR 5",
    "EXCEPTION_NO_ERROR 6",
    "EXCEPTION_NO_ERROR 7",
    "EXCEPTION_HAS_ERROR 8",
    "EXCEPTION_NO_ERROR 9",
    "EXCEPTION_HAS_ERROR 10",
    "EXCEPTION_HAS_ERROR 11",
    "EXCEPTION_HAS_ERROR 12",
    "EXCEPTION_HAS_ERROR 13",
    "EXCEPTION_HAS_ERROR 14",
    "EXCEPTION_NO_ERROR 15",
    "EXCEPTION_NO_ERROR 16",
    "EXCEPTION_HAS_ERROR 17",
    "EXCEPTION_NO_ERROR 18",
    "EXCEPTION_NO_ERROR 19",
    "EXCEPTION_NO_ERROR 20",
    "EXCEPTION_HAS_ERROR 21",
    "EXCEPTION_NO_ERROR 22",
    "EXCEPTION_NO_ERROR 23",
    "EXCEPTION_NO_ERROR 24",
    "EXCEPTION_NO_ERROR 25",
    "EXCEPTION_NO_ERROR 26",
    "EXCEPTION_NO_ERROR 27",
    "EXCEPTION_NO_ERROR 28",
    "EXCEPTION_HAS_ERROR 29",
    "EXCEPTION_HAS_ERROR 30",
    "EXCEPTION_NO_ERROR 31",
    ".global exception_common",
    "exception_common:",
    "SAVE_REGISTERS",
    "mov rbx, rsp",
    "and rsp, -16",
    "mov rdi, qword ptr [rbx + 120]",
    "mov rsi, qword ptr [rbx + 128]",
    "mov rdx, qword ptr [rbx + 136]",
    "mov rcx, qword ptr [rbx + 144]",
    "mov r8, qword ptr [rbx + 152]",
    "mov r9, cr2",
    "call exception_dispatch",
    "mov rsp, rbx",
    "RESTORE_REGISTERS",
    "add rsp, 16",
    "iretq",
    ".global unexpected_interrupt_stub",
    "unexpected_interrupt_stub:",
    "push 0",
    "push 255",
    "jmp exception_common",
    ".macro IRQ_STUB name, handler",
    ".global \\name",
    "\\name:",
    "SAVE_REGISTERS",
    "mov rbx, rsp",
    "and rsp, -16",
    "call \\handler",
    "mov rsp, rbx",
    "RESTORE_REGISTERS",
    "iretq",
    ".endm",
    "IRQ_STUB timer_irq_stub, timer_irq_handler",
    "IRQ_STUB keyboard_irq_stub, keyboard_irq_handler",
    ".section .rodata",
    ".global exception_stub_table",
    "exception_stub_table:",
    ".quad exception_stub_0, exception_stub_1, exception_stub_2, exception_stub_3",
    ".quad exception_stub_4, exception_stub_5, exception_stub_6, exception_stub_7",
    ".quad exception_stub_8, exception_stub_9, exception_stub_10, exception_stub_11",
    ".quad exception_stub_12, exception_stub_13, exception_stub_14, exception_stub_15",
    ".quad exception_stub_16, exception_stub_17, exception_stub_18, exception_stub_19",
    ".quad exception_stub_20, exception_stub_21, exception_stub_22, exception_stub_23",
    ".quad exception_stub_24, exception_stub_25, exception_stub_26, exception_stub_27",
    ".quad exception_stub_28, exception_stub_29, exception_stub_30, exception_stub_31",
);

unsafe extern "C" {
    fn load_gdt_and_segments(table: *const DescriptorTablePointer);
    fn timer_irq_stub();
    fn keyboard_irq_stub();
    fn unexpected_interrupt_stub();
    static exception_stub_table: [u64; EXCEPTION_COUNT];
}

pub fn initialize(exception_reporter: ExceptionReporter) {
    unsafe {
        asm!("cli", options(nomem, nostack));
        core::ptr::addr_of_mut!(EXCEPTION_REPORTER).write_volatile(Some(exception_reporter));
    }
    unsafe {
        let stack_top = core::ptr::addr_of_mut!(INTERRUPT_STACK.0)
            .cast::<u8>()
            .add(INTERRUPT_STACK_SIZE) as u64;
        core::ptr::addr_of_mut!(TSS.rsp)
            .cast::<u64>()
            .write_unaligned(stack_top);
        let double_fault_stack_top = core::ptr::addr_of_mut!(DOUBLE_FAULT_STACK.0)
            .cast::<u8>()
            .add(DOUBLE_FAULT_STACK_SIZE) as u64;
        core::ptr::addr_of_mut!(TSS.ist)
            .cast::<u64>()
            .write_unaligned(double_fault_stack_top);

        let tss_base = core::ptr::addr_of!(TSS) as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        let descriptor_low = (tss_limit & 0xFFFF)
            | ((tss_base & 0x00FF_FFFF) << 16)
            | (0x89 << 40)
            | (((tss_limit >> 16) & 0x0F) << 48)
            | (((tss_base >> 24) & 0xFF) << 56);
        let gdt = core::ptr::addr_of_mut!(GDT).cast::<u64>();
        gdt.add(5).write_volatile(descriptor_low);
        gdt.add(6).write_volatile(tss_base >> 32);

        let gdt_pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
            base: core::ptr::addr_of!(GDT) as u64,
        };
        load_gdt_and_segments(&gdt_pointer);
        asm!("ltr ax", in("ax") TASK_STATE_SELECTOR, options(nostack, preserves_flags));

        initialize_idt();
        remap_pic();
        initialize_pit();
        outb(0x21, 0xFE);
        outb(0xA1, 0xFF);
    }
}

unsafe fn initialize_idt() {
    let idt = core::ptr::addr_of_mut!(IDT).cast::<IdtEntry>();
    let unexpected = IdtEntry::interrupt_gate(unexpected_interrupt_stub as *const () as u64);
    for vector in 0..IDT_ENTRIES {
        unsafe {
            idt.add(vector).write_volatile(unexpected);
        }
    }
    unsafe {
        let exception_stubs = core::ptr::addr_of!(exception_stub_table);
        for vector in 0..EXCEPTION_COUNT {
            let handler = exception_stubs.cast::<u64>().add(vector).read_volatile();
            let entry = if vector == 8 {
                IdtEntry::interrupt_gate_with_ist(handler, 1)
            } else {
                IdtEntry::interrupt_gate(handler)
            };
            idt.add(vector).write_volatile(entry);
        }
        idt.add(TIMER_VECTOR)
            .write_volatile(IdtEntry::interrupt_gate(timer_irq_stub as *const () as u64));
        idt.add(KEYBOARD_VECTOR)
            .write_volatile(IdtEntry::interrupt_gate(
                keyboard_irq_stub as *const () as u64,
            ));
    }

    let idt_pointer = DescriptorTablePointer {
        limit: (core::mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };
    unsafe {
        asm!(
            "lidt [{}]",
            in(reg) &idt_pointer,
            options(readonly, nostack, preserves_flags)
        );
    }
}

unsafe fn initialize_pit() {
    let divisor = (PIT_INPUT_HZ / PIT_FREQUENCY_HZ) as u16;
    unsafe {
        outb(0x43, 0x36);
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

unsafe fn remap_pic() {
    unsafe {
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
        outb(0x20, 0x11);
        io_wait();
        outb(0xA0, 0x11);
        io_wait();
        outb(0x21, 0x20);
        io_wait();
        outb(0xA1, 0x28);
        io_wait();
        outb(0x21, 0x04);
        io_wait();
        outb(0xA1, 0x02);
        io_wait();
        outb(0x21, 0x01);
        io_wait();
        outb(0xA1, 0x01);
        io_wait();
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

pub fn end_of_interrupt(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(0xA0, 0x20);
        }
        outb(0x20, 0x20);
    }
}

pub fn timer_irq_count() -> u64 {
    TIMER_IRQ_COUNT.load(Ordering::Acquire)
}

pub fn keyboard_irq_count() -> u64 {
    KEYBOARD_IRQ_COUNT.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn timer_irq_handler() {
    TIMER_IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
    end_of_interrupt(0);
}

pub fn record_keyboard_irq() {
    KEYBOARD_IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn enable_keyboard_irq() {
    unsafe {
        let current_mask = inb(0x21);
        outb(0x21, current_mask & !0x02);
    }
}

#[unsafe(no_mangle)]
extern "C" fn exception_dispatch(
    vector: u64,
    error: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    cr2: u64,
) -> ! {
    unsafe {
        let reporter = core::ptr::addr_of!(EXCEPTION_REPORTER).read_volatile();
        if let Some(reporter) = reporter {
            reporter(vector, error, rip, cs, rflags, cr2);
        }
    }
    halt_on_unreported_exception()
}

fn halt_on_unreported_exception() -> ! {
    unsafe {
        asm!("cli", options(nomem, nostack));
        loop {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

pub fn wait_for_interrupt() {
    unsafe {
        asm!("sti", "hlt", "cli", options(nostack));
    }
}

unsafe fn io_wait() {
    unsafe {
        outb(0x80, 0);
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
