use alphaos_syscall_abi::{
    INPUT_EVENT_KEY, INPUT_MODIFIER_ALT, INPUT_MODIFIER_CAPS_LOCK, INPUT_MODIFIER_CONTROL,
    INPUT_MODIFIER_SHIFT, InputEvent, KEY_EVENT_PRESS, KEY_EVENT_RELEASE, KEY_EVENT_REPEAT,
    KeyCode,
};

struct Decoder {
    extended: bool,
    pause_bytes: u8,
    modifiers: u32,
    pressed: [u64; 4],
}

impl Decoder {
    const fn new() -> Self {
        Self {
            extended: false,
            pause_bytes: 0,
            modifiers: 0,
            pressed: [0; 4],
        }
    }

    fn feed(&mut self, scancode: u8) {
        if self.pause_bytes != 0 {
            self.pause_bytes -= 1;
            return;
        }
        if scancode == 0xE1 {
            self.extended = false;
            self.pause_bytes = 5;
            return;
        }
        if scancode == 0xE0 {
            self.extended = true;
            return;
        }

        let extended = core::mem::replace(&mut self.extended, false);
        let released = scancode & 0x80 != 0;
        let Some(key) = map_scancode(scancode & 0x7F, extended) else {
            return;
        };
        let code = key as u16;
        let (word, mask) = (usize::from(code) / 64, 1_u64 << (code % 64));
        let was_pressed = self.pressed[word] & mask != 0;
        if released {
            self.pressed[word] &= !mask;
        } else {
            self.pressed[word] |= mask;
        }

        match key {
            KeyCode::LeftShift | KeyCode::RightShift => {
                self.set_modifier(
                    INPUT_MODIFIER_SHIFT,
                    self.is_pressed(KeyCode::LeftShift) || self.is_pressed(KeyCode::RightShift),
                );
            }
            KeyCode::LeftControl | KeyCode::RightControl => {
                self.set_modifier(
                    INPUT_MODIFIER_CONTROL,
                    self.is_pressed(KeyCode::LeftControl)
                        || self.is_pressed(KeyCode::RightControl),
                );
            }
            KeyCode::LeftAlt | KeyCode::RightAlt => {
                self.set_modifier(
                    INPUT_MODIFIER_ALT,
                    self.is_pressed(KeyCode::LeftAlt) || self.is_pressed(KeyCode::RightAlt),
                );
            }
            KeyCode::CapsLock if !released && !was_pressed => {
                self.modifiers ^= INPUT_MODIFIER_CAPS_LOCK;
            }
            _ => {}
        }

        let value = if released {
            KEY_EVENT_RELEASE
        } else if was_pressed {
            KEY_EVENT_REPEAT
        } else {
            KEY_EVENT_PRESS
        };
        let _ = crate::input::enqueue(InputEvent {
            event_type: INPUT_EVENT_KEY,
            code,
            value,
            modifiers: self.modifiers,
            reserved: 0,
        });
    }

    fn set_modifier(&mut self, modifier: u32, enabled: bool) {
        if enabled {
            self.modifiers |= modifier;
        } else {
            self.modifiers &= !modifier;
        }
    }

    fn is_pressed(&self, key: KeyCode) -> bool {
        let code = key as usize;
        self.pressed[code / 64] & (1_u64 << (code % 64)) != 0
    }
}

static mut DECODER: Decoder = Decoder::new();

pub fn reset_decoder() {
    unsafe {
        core::ptr::addr_of_mut!(DECODER).write(Decoder::new());
    }
}

#[unsafe(no_mangle)]
extern "C" fn keyboard_irq_handler() {
    unsafe {
        let status = crate::ps2::read_status();
        if status & 1 != 0 {
            let scancode = crate::ps2::read_data();
            if status & 0x20 == 0 {
                core::ptr::addr_of_mut!(DECODER)
                    .as_mut()
                    .unwrap()
                    .feed(scancode);
            }
        }
    }
    crate::interrupts::end_of_interrupt();
}

pub fn event_to_ascii(event: &InputEvent) -> Option<u8> {
    if event.event_type != INPUT_EVENT_KEY
        || (event.value != KEY_EVENT_PRESS && event.value != KEY_EVENT_REPEAT)
        || event.modifiers & (INPUT_MODIFIER_CONTROL | INPUT_MODIFIER_ALT) != 0
    {
        return None;
    }

    let shifted = event.modifiers & INPUT_MODIFIER_SHIFT != 0;
    let caps_lock = event.modifiers & INPUT_MODIFIER_CAPS_LOCK != 0;
    let code = event.code;

    if (KeyCode::A as u16..=KeyCode::Z as u16).contains(&code) {
        let letter = b'a' + (code - KeyCode::A as u16) as u8;
        return Some(if shifted ^ caps_lock {
            letter.to_ascii_uppercase()
        } else {
            letter
        });
    }

    if (KeyCode::Digit1 as u16..=KeyCode::Digit0 as u16).contains(&code) {
        let index = (code - KeyCode::Digit1 as u16) as usize;
        let plain = *b"1234567890".get(index)?;
        let upper = *b"!@#$%^&*()".get(index)?;
        return Some(if shifted { upper } else { plain });
    }

    let key = match code {
        value if value == KeyCode::Enter as u16 => b'\n',
        value if value == KeyCode::Backspace as u16 => 8,
        value if value == KeyCode::Tab as u16 => b'\t',
        value if value == KeyCode::Space as u16 => b' ',
        value if value == KeyCode::Minus as u16 => shifted_byte(shifted, b'-', b'_'),
        value if value == KeyCode::Equal as u16 => shifted_byte(shifted, b'=', b'+'),
        value if value == KeyCode::LeftBracket as u16 => shifted_byte(shifted, b'[', b'{'),
        value if value == KeyCode::RightBracket as u16 => shifted_byte(shifted, b']', b'}'),
        value if value == KeyCode::Backslash as u16 => shifted_byte(shifted, b'\\', b'|'),
        value if value == KeyCode::Semicolon as u16 => shifted_byte(shifted, b';', b':'),
        value if value == KeyCode::Apostrophe as u16 => shifted_byte(shifted, b'\'', b'"'),
        value if value == KeyCode::Grave as u16 => shifted_byte(shifted, b'`', b'~'),
        value if value == KeyCode::Comma as u16 => shifted_byte(shifted, b',', b'<'),
        value if value == KeyCode::Period as u16 => shifted_byte(shifted, b'.', b'>'),
        value if value == KeyCode::Slash as u16 => shifted_byte(shifted, b'/', b'?'),
        _ => return None,
    };
    Some(key)
}

fn shifted_byte(shifted: bool, plain: u8, upper: u8) -> u8 {
    if shifted { upper } else { plain }
}

fn map_scancode(scancode: u8, extended: bool) -> Option<KeyCode> {
    use KeyCode as K;

    if extended {
        return Some(match scancode {
            0x1C => K::KeypadEnter,
            0x1D => K::RightControl,
            0x35 => K::KeypadSlash,
            0x38 => K::RightAlt,
            0x47 => K::Home,
            0x48 => K::Up,
            0x49 => K::PageUp,
            0x4B => K::Left,
            0x4D => K::Right,
            0x4F => K::End,
            0x50 => K::Down,
            0x51 => K::PageDown,
            0x52 => K::Insert,
            0x53 => K::Delete,
            0x5B => K::LeftMeta,
            0x5C => K::RightMeta,
            _ => return None,
        });
    }

    Some(match scancode {
        0x01 => K::Escape,
        0x02 => K::Digit1,
        0x03 => K::Digit2,
        0x04 => K::Digit3,
        0x05 => K::Digit4,
        0x06 => K::Digit5,
        0x07 => K::Digit6,
        0x08 => K::Digit7,
        0x09 => K::Digit8,
        0x0A => K::Digit9,
        0x0B => K::Digit0,
        0x0C => K::Minus,
        0x0D => K::Equal,
        0x0E => K::Backspace,
        0x0F => K::Tab,
        0x10 => K::Q,
        0x11 => K::W,
        0x12 => K::E,
        0x13 => K::R,
        0x14 => K::T,
        0x15 => K::Y,
        0x16 => K::U,
        0x17 => K::I,
        0x18 => K::O,
        0x19 => K::P,
        0x1A => K::LeftBracket,
        0x1B => K::RightBracket,
        0x1C => K::Enter,
        0x1D => K::LeftControl,
        0x1E => K::A,
        0x1F => K::S,
        0x20 => K::D,
        0x21 => K::F,
        0x22 => K::G,
        0x23 => K::H,
        0x24 => K::J,
        0x25 => K::K,
        0x26 => K::L,
        0x27 => K::Semicolon,
        0x28 => K::Apostrophe,
        0x29 => K::Grave,
        0x2A => K::LeftShift,
        0x2B => K::Backslash,
        0x2C => K::Z,
        0x2D => K::X,
        0x2E => K::C,
        0x2F => K::V,
        0x30 => K::B,
        0x31 => K::N,
        0x32 => K::M,
        0x33 => K::Comma,
        0x34 => K::Period,
        0x35 => K::Slash,
        0x36 => K::RightShift,
        0x38 => K::LeftAlt,
        0x39 => K::Space,
        0x3A => K::CapsLock,
        0x3B => K::F1,
        0x3C => K::F2,
        0x3D => K::F3,
        0x3E => K::F4,
        0x3F => K::F5,
        0x40 => K::F6,
        0x41 => K::F7,
        0x42 => K::F8,
        0x43 => K::F9,
        0x44 => K::F10,
        0x45 => K::NumLock,
        0x46 => K::ScrollLock,
        0x57 => K::F11,
        0x58 => K::F12,
        _ => return None,
    })
}
