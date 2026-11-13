use core::arch::asm;

const WAIT_LIMIT: usize = 100_000;

pub fn initialize() -> bool {
    unsafe {
        drain_output();
        if !wait_for_input_clear() {
            return false;
        }
        outb(0x64, 0xAD);
        if !wait_for_input_clear() {
            return false;
        }
        outb(0x64, 0xA7);

        let Some(mut config) = controller_command_byte() else {
            return false;
        };
        config = (config | 0x70) & !0x03;
        if !write_controller_command_byte(config) {
            return false;
        }

        if !wait_for_input_clear() {
            return false;
        }
        outb(0x64, 0xAE);
        if !send_keyboard_command(0xF4) {
            return false;
        }

        config = (config | 0x61) & !0x12;
        if !write_controller_command_byte(config) {
            return false;
        }

        crate::keyboard::reset_decoder();
        true
    }
}

unsafe fn controller_command_byte() -> Option<u8> {
    if !unsafe { wait_for_input_clear() } {
        return None;
    }
    unsafe {
        outb(0x64, 0x20);
    }
    unsafe { read_response(false) }.map(|(value, _)| value)
}

unsafe fn write_controller_command_byte(value: u8) -> bool {
    if !unsafe { wait_for_input_clear() } {
        return false;
    }
    unsafe {
        outb(0x64, 0x60);
    }
    if !unsafe { wait_for_input_clear() } {
        return false;
    }
    unsafe {
        outb(0x60, value);
    }
    true
}

unsafe fn send_keyboard_command(command: u8) -> bool {
    if !unsafe { wait_for_input_clear() } {
        return false;
    }
    unsafe {
        outb(0x60, command);
    }
    matches!(unsafe { read_response(false) }, Some((0xFA, false)))
}

unsafe fn read_response(auxiliary: bool) -> Option<(u8, bool)> {
    for _ in 0..WAIT_LIMIT {
        let status = unsafe { inb(0x64) };
        if status & 1 == 0 {
            continue;
        }
        let is_auxiliary = status & 0x20 != 0;
        let value = unsafe { inb(0x60) };
        if is_auxiliary == auxiliary {
            return Some((value, is_auxiliary));
        }
    }
    None
}

unsafe fn wait_for_input_clear() -> bool {
    for _ in 0..WAIT_LIMIT {
        if unsafe { inb(0x64) } & 0x02 == 0 {
            return true;
        }
    }
    false
}

unsafe fn drain_output() {
    for _ in 0..256 {
        if unsafe { inb(0x64) } & 1 == 0 {
            break;
        }
        unsafe {
            inb(0x60);
        }
    }
}

pub unsafe fn read_status() -> u8 {
    unsafe { inb(0x64) }
}

pub unsafe fn read_data() -> u8 {
    unsafe { inb(0x60) }
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
