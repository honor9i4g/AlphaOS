use core::arch::{asm, global_asm};

const IDT_ENTRIES: usize = 256;
const KEYBOARD_VECTOR: usize = 0x21;
const TASK_STATE_SELECTOR: u16 = 0x28;
const INTERRUPT_STACK_SIZE: usize = 16 * 1024;

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
        Self {
            offset_low: handler as u16,
            selector: 0x08,
            ist: 0,
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
    ".global unhandled_exception_stub",
    "unhandled_exception_stub:",
    "mov dx, 0xE9",
    "mov al, 0x45",
    "out dx, al",
    "cli",
    "3:",
    "hlt",
    "jmp 3b",
    ".global keyboard_irq_stub",
    "keyboard_irq_stub:",
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
    "mov rbx, rsp",
    "and rsp, -16",
    "call keyboard_irq_handler",
    "mov rsp, rbx",
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
    "iretq",
);

unsafe extern "C" {
    fn load_gdt_and_segments(table: *const DescriptorTablePointer);
    fn unhandled_exception_stub();
    fn keyboard_irq_stub();
}

pub fn initialize() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
    unsafe {
        let stack_top = core::ptr::addr_of_mut!(INTERRUPT_STACK.0)
            .cast::<u8>()
            .add(INTERRUPT_STACK_SIZE) as u64;
        core::ptr::addr_of_mut!(TSS.rsp)
            .cast::<u64>()
            .write_unaligned(stack_top);

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
    }
}

unsafe fn initialize_idt() {
    let idt = core::ptr::addr_of_mut!(IDT).cast::<IdtEntry>();
    let exception = IdtEntry::interrupt_gate(unhandled_exception_stub as *const () as u64);
    for vector in 0..IDT_ENTRIES {
        unsafe {
            idt.add(vector).write_volatile(exception);
        }
    }
    unsafe {
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

pub fn enable_keyboard_irq() {
    unsafe {
        outb(0x21, 0xFD);
        outb(0xA1, 0xFF);
    }
}

pub fn end_of_interrupt() {
    unsafe {
        outb(0x20, 0x20);
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
