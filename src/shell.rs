static mut BUF: [u8; 128] = [0; 128];
static mut POS: usize = 0;
static mut SHIFT: bool = false;
static mut CX: u32 = 26;
static mut CY: u32 = 120;

const PROMPT_X: u32 = 10;
const INPUT_X: u32 = 34;
const LINE_HEIGHT: u32 = 16;

pub fn init() {
    unsafe {
        POS = 0;
        CX = INPUT_X;
        CY = 120;
        SHIFT = false;
        draw_prompt();
    }
}

pub fn poll() {
    unsafe {
        if crate::port::in_port(0x64) & 1 == 1 {
            let sc = crate::port::in_port(0x60);
            match sc {
                0x2A | 0x36 => {
                    SHIFT = true;
                    return;
                }
                0xAA | 0xB6 => {
                    SHIFT = false;
                    return;
                }
                _ if sc >= 0x80 => return,
                0x0E => {
                    if POS > 0 {
                        POS -= 1;
                        BUF[POS] = 0;
                        crate::video::fill_rect(INPUT_X + POS as u32 * 8, CY, 8, 8, 0, 0, 0);
                        redraw();
                    }
                    return;
                }
                0x1C => {
                    BUF[POS] = 0;
                    let cmd = core::str::from_utf8(&BUF[..POS]).unwrap_or("");
                    let cleared = run(cmd);
                    POS = 0;
                    BUF = [0; 128];
                    if !cleared {
                        CY = CY.saturating_add(LINE_HEIGHT * 2);
                        if CY > 380 {
                            crate::video::clear();
                            crate::video::draw_text(10, 10, "ALPHA OS v0.1", 0xFF, 0x00, 0x00);
                            CY = 120;
                        }
                        draw_prompt();
                    }
                    return;
                }
                _ => {}
            }
            let ch: u8 = key(sc);
            if ch == 0 {
                return;
            }
            if POS < 127 {
                BUF[POS] = ch;
                POS += 1;
                BUF[POS] = 0;
                crate::video::draw_char(CX, CY, ch);
                CX = CX.saturating_add(8);
            }
        }
    }
}

fn key(sc: u8) -> u8 {
    let s = unsafe { SHIFT };
    match sc {
        0x29 => b' ',
        0x02 => {
            if s {
                b'!'
            } else {
                b'1'
            }
        }
        0x03 => {
            if s {
                b'@'
            } else {
                b'2'
            }
        }
        0x04 => {
            if s {
                b'#'
            } else {
                b'3'
            }
        }
        0x05 => {
            if s {
                b'$'
            } else {
                b'4'
            }
        }
        0x06 => {
            if s {
                b'%'
            } else {
                b'5'
            }
        }
        0x07 => {
            if s {
                b'^'
            } else {
                b'6'
            }
        }
        0x08 => {
            if s {
                b'&'
            } else {
                b'7'
            }
        }
        0x09 => {
            if s {
                b'*'
            } else {
                b'8'
            }
        }
        0x0A => {
            if s {
                b'('
            } else {
                b'9'
            }
        }
        0x0B => {
            if s {
                b')'
            } else {
                b'0'
            }
        }
        0x10 => {
            if s {
                b'Q'
            } else {
                b'q'
            }
        }
        0x11 => {
            if s {
                b'W'
            } else {
                b'w'
            }
        }
        0x12 => {
            if s {
                b'E'
            } else {
                b'e'
            }
        }
        0x13 => {
            if s {
                b'R'
            } else {
                b'r'
            }
        }
        0x14 => {
            if s {
                b'T'
            } else {
                b't'
            }
        }
        0x15 => {
            if s {
                b'Y'
            } else {
                b'y'
            }
        }
        0x16 => {
            if s {
                b'U'
            } else {
                b'u'
            }
        }
        0x17 => {
            if s {
                b'I'
            } else {
                b'i'
            }
        }
        0x18 => {
            if s {
                b'O'
            } else {
                b'o'
            }
        }
        0x19 => {
            if s {
                b'P'
            } else {
                b'p'
            }
        }
        0x1E => {
            if s {
                b'A'
            } else {
                b'a'
            }
        }
        0x1F => {
            if s {
                b'S'
            } else {
                b's'
            }
        }
        0x20 => {
            if s {
                b'D'
            } else {
                b'd'
            }
        }
        0x21 => {
            if s {
                b'F'
            } else {
                b'f'
            }
        }
        0x22 => {
            if s {
                b'G'
            } else {
                b'g'
            }
        }
        0x23 => {
            if s {
                b'H'
            } else {
                b'h'
            }
        }
        0x24 => {
            if s {
                b'J'
            } else {
                b'j'
            }
        }
        0x25 => {
            if s {
                b'K'
            } else {
                b'k'
            }
        }
        0x26 => {
            if s {
                b'L'
            } else {
                b'l'
            }
        }
        0x2C => {
            if s {
                b'Z'
            } else {
                b'z'
            }
        }
        0x2D => {
            if s {
                b'X'
            } else {
                b'x'
            }
        }
        0x2E => {
            if s {
                b'C'
            } else {
                b'c'
            }
        }
        0x2F => {
            if s {
                b'V'
            } else {
                b'v'
            }
        }
        0x30 => {
            if s {
                b'B'
            } else {
                b'b'
            }
        }
        0x31 => {
            if s {
                b'N'
            } else {
                b'n'
            }
        }
        0x32 => {
            if s {
                b'M'
            } else {
                b'm'
            }
        }
        0x0D => {
            if s {
                b'+'
            } else {
                b'='
            }
        }
        0x1A => {
            if s {
                b'{'
            } else {
                b'['
            }
        }
        0x1B => {
            if s {
                b'}'
            } else {
                b']'
            }
        }
        0x27 => {
            if s {
                b':'
            } else {
                b';'
            }
        }
        0x28 => {
            if s {
                b'"'
            } else {
                b'\''
            }
        }
        0x2B => {
            if s {
                b'|'
            } else {
                b'\\'
            }
        }
        0x33 => {
            if s {
                b'<'
            } else {
                b','
            }
        }
        0x34 => {
            if s {
                b'>'
            } else {
                b'.'
            }
        }
        0x35 => {
            if s {
                b'?'
            } else {
                b'/'
            }
        }
        _ => 0,
    }
}

unsafe fn redraw() {
    for i in 0..POS {
        crate::video::draw_char(INPUT_X + i as u32 * 8, CY, BUF[i]);
    }
    CX = INPUT_X + POS as u32 * 8;
}

unsafe fn draw_prompt() {
    crate::video::draw_text(PROMPT_X, CY, "c: ", 0xFF, 0xFF, 0xFF);
    CX = INPUT_X;
}

fn run(cmd: &str) -> bool {
    let cmd = cmd.trim();
    let y = unsafe { CY.saturating_add(LINE_HEIGHT) };
    unsafe {
        match cmd {
            "help" => crate::video::draw_text(10, y, "help  clear  info  time", 0xFF, 0xFF, 0xFF),
            "clear" => {
                crate::video::clear();
                crate::video::draw_text(10, 10, "ALPHA OS v0.1", 0xFF, 0x00, 0x00);
                CY = 120;
                draw_prompt();
                return true;
            }
            "info" => {
                crate::video::draw_text(10, y, "ALPHA OS v0.1 | Rust + UEFI", 0x00, 0xFF, 0x00)
            }
            "time" => {
                let t = crate::timer::tick_count();
                let mut s = [0u8; 21];
                let mut v = t;
                let mut p = 20;
                if v == 0 {
                    s[p] = b'0';
                    p -= 1;
                }
                while v > 0 {
                    s[p] = b'0' + (v % 10) as u8;
                    v /= 10;
                    p -= 1;
                }
                let start = if s[0] == 0 { 1 } else { 0 };
                crate::video::draw_text(
                    10,
                    y,
                    core::str::from_utf8(&s[start..20]).unwrap_or(""),
                    0xFF,
                    0xFF,
                    0x00,
                );
            }
            "" => {}
            _ => crate::video::draw_text(10, y, "Unknown. Type help", 0xFF, 0x00, 0x00),
        }
    }
    false
}
