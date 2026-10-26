use super::draw_bytes;

pub(super) fn show_ticks(output_y: u32) -> u32 {
    draw_unsigned(output_y, crate::timer::tick_count(), "PIT ticks: ");
    1
}

pub(super) fn show_uptime(output_y: u32) -> u32 {
    let seconds = crate::timer::tick_count() / 1000;
    let minutes = seconds / 60;
    let remaining_seconds = seconds % 60;
    let mut buffer = [0u8; 64];
    let prefix = b"Uptime: ";
    buffer[..prefix.len()].copy_from_slice(prefix);
    let mut pos = prefix.len();
    pos += write_unsigned(&mut buffer[pos..], minutes);
    buffer[pos] = b'm';
    pos += 1;
    buffer[pos] = b' ';
    pos += 1;
    pos += write_unsigned(&mut buffer[pos..], remaining_seconds);
    buffer[pos] = b's';
    pos += 1;
    draw_bytes(output_y, &buffer[..pos], 255, 255, 0);
    1
}

fn draw_unsigned(y: u32, value: u64, prefix: &str) {
    let mut buffer = [0u8; 32];
    let prefix = prefix.as_bytes();
    buffer[..prefix.len()].copy_from_slice(prefix);
    let end = prefix.len() + write_unsigned(&mut buffer[prefix.len()..], value);
    draw_bytes(y, &buffer[..end], 255, 255, 255);
}

fn write_unsigned(buffer: &mut [u8], mut value: u64) -> usize {
    if value == 0 {
        buffer[0] = b'0';
        return 1;
    }

    let mut digits = [0u8; 20];
    let mut count = 0;
    while value > 0 {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
    }
    for index in 0..count {
        buffer[index] = digits[count - index - 1];
    }
    count
}
