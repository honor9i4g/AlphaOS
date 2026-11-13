pub unsafe fn out(port: u16, value: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value);
    }
}

pub unsafe fn in_port(port: u16) -> u8 {
    unsafe {
        let val: u8;
        core::arch::asm!("in al, dx", out("al") val, in("dx") port);
        val
    }
}
