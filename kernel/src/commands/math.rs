use super::{draw_bytes, draw_line};

pub(super) fn calculate(args: &str, output_y: u32) -> u32 {
    let mut parts = args.split_whitespace();
    let (Some(left), Some(operator), Some(right)) = (parts.next(), parts.next(), parts.next())
    else {
        draw_line(
            output_y,
            "Usage: calc <number> <+|-|*|/> <number>",
            255,
            0,
            0,
        );
        return 1;
    };

    if parts.next().is_some() {
        draw_line(
            output_y,
            "Usage: calc <number> <+|-|*|/> <number>",
            255,
            0,
            0,
        );
        return 1;
    }

    let (Ok(left), Ok(right)) = (left.parse::<i64>(), right.parse::<i64>()) else {
        draw_line(output_y, "calc: expected integer values.", 255, 0, 0);
        return 1;
    };

    let result = match operator {
        "+" => left.checked_add(right),
        "-" => left.checked_sub(right),
        "*" => left.checked_mul(right),
        "/" => left.checked_div(right),
        _ => {
            draw_line(output_y, "calc: operator must be +, -, * or /.", 255, 0, 0);
            return 1;
        }
    };

    match result {
        Some(value) => {
            let mut buffer = [0u8; 21];
            let start = write_signed(&mut buffer, value);
            draw_bytes(output_y, &buffer[start..], 255, 255, 255);
        }
        None => draw_line(
            output_y,
            "calc: division by zero or integer overflow.",
            255,
            0,
            0,
        ),
    }
    1
}

fn write_signed(buffer: &mut [u8; 21], value: i64) -> usize {
    let negative = value < 0;
    let mut value = value.unsigned_abs();
    let mut pos = buffer.len();
    if value == 0 {
        pos -= 1;
        buffer[pos] = b'0';
    } else {
        while value > 0 {
            pos -= 1;
            buffer[pos] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    if negative {
        pos -= 1;
        buffer[pos] = b'-';
    }
    pos
}
