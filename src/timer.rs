static mut LAST_COUNT: u16 = 0;
static mut TICK_COUNT: u64 = 0;

pub unsafe fn init() {
    let divisor: u16 = 1193;
    crate::port::out(0x43, 0x36);
    crate::port::out(0x40, (divisor & 0xFF) as u8);
    crate::port::out(0x40, (divisor >> 8) as u8);
    LAST_COUNT = divisor;
    TICK_COUNT = 0;
}

pub fn tick_count() -> u64 {
    unsafe {
        crate::port::out(0x43, 0x00);
        let low = crate::port::in_port(0x40);
        let high = crate::port::in_port(0x40);
        let current = u16::from_le_bytes([low, high]);

        if current > LAST_COUNT {
            TICK_COUNT = TICK_COUNT.wrapping_add(1);
        }
        LAST_COUNT = current;
        TICK_COUNT
    }
}
