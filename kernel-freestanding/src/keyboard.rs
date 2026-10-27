use core::arch::asm;

pub enum KeyEvent {
    Character(u8),
    Enter,
    Backspace,
}

pub struct Keyboard {
    shift_keys: u8,
    extended: bool,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self {
            shift_keys: 0,
            extended: false,
        }
    }

    pub fn flush_pending(&mut self) {
        for _ in 0..256 {
            let status = unsafe { inb(0x64) };
            if status & 1 == 0 {
                break;
            }
            unsafe {
                inb(0x60);
            }
        }
    }

    pub fn poll(&mut self) -> Option<KeyEvent> {
        let status = unsafe { inb(0x64) };
        if status & 1 == 0 {
            return None;
        }

        let scancode = unsafe { inb(0x60) };
        if status & 0x20 != 0 {
            return None;
        }

        if scancode == 0xE0 || scancode == 0xE1 {
            self.extended = true;
            return None;
        }
        if self.extended {
            self.extended = false;
            return None;
        }

        match scancode {
            0x2A => {
                self.shift_keys |= 1;
                None
            }
            0x36 => {
                self.shift_keys |= 2;
                None
            }
            0xAA => {
                self.shift_keys &= !1;
                None
            }
            0xB6 => {
                self.shift_keys &= !2;
                None
            }
            0x0E => Some(KeyEvent::Backspace),
            0x1C => Some(KeyEvent::Enter),
            code if code & 0x80 != 0 => None,
            code => character(code, self.shift_keys != 0).map(KeyEvent::Character),
        }
    }
}

fn character(scancode: u8, shift: bool) -> Option<u8> {
    let letter = match scancode {
        0x10 => b'q',
        0x11 => b'w',
        0x12 => b'e',
        0x13 => b'r',
        0x14 => b't',
        0x15 => b'y',
        0x16 => b'u',
        0x17 => b'i',
        0x18 => b'o',
        0x19 => b'p',
        0x1E => b'a',
        0x1F => b's',
        0x20 => b'd',
        0x21 => b'f',
        0x22 => b'g',
        0x23 => b'h',
        0x24 => b'j',
        0x25 => b'k',
        0x26 => b'l',
        0x2C => b'z',
        0x2D => b'x',
        0x2E => b'c',
        0x2F => b'v',
        0x30 => b'b',
        0x31 => b'n',
        0x32 => b'm',
        _ => return punctuation(scancode, shift),
    };

    Some(if shift {
        letter.to_ascii_uppercase()
    } else {
        letter
    })
}

fn punctuation(scancode: u8, shift: bool) -> Option<u8> {
    let (plain, shifted) = match scancode {
        0x02 => (b'1', b'!'),
        0x03 => (b'2', b'@'),
        0x04 => (b'3', b'#'),
        0x05 => (b'4', b'$'),
        0x06 => (b'5', b'%'),
        0x07 => (b'6', b'^'),
        0x08 => (b'7', b'&'),
        0x09 => (b'8', b'*'),
        0x0A => (b'9', b'('),
        0x0B => (b'0', b')'),
        0x0C => (b'-', b'_'),
        0x0D => (b'=', b'+'),
        0x1A => (b'[', b'{'),
        0x1B => (b']', b'}'),
        0x27 => (b';', b':'),
        0x28 => (b'\'', b'"'),
        0x29 => (b'`', b'~'),
        0x2B => (b'\\', b'|'),
        0x33 => (b',', b'<'),
        0x34 => (b'.', b'>'),
        0x35 => (b'/', b'?'),
        0x39 => (b' ', b' '),
        _ => return None,
    };

    Some(if shift { shifted } else { plain })
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}
