static mut FB: *mut u8 = core::ptr::null_mut();
static mut WIDTH: usize = 0;
static mut HEIGHT: usize = 0;

pub fn init(fb: *mut u8, width: usize, height: usize) {
    unsafe {
        FB = fb;
        WIDTH = width;
        HEIGHT = height;
    }
}

pub unsafe fn clear() {
    fill_rect(0, 0, WIDTH as u32, HEIGHT as u32, 0, 0, 0);
}

unsafe fn write_pixel(x: u32, y: u32, r: u8, g: u8, b: u8) {
    if FB.is_null() || x as usize >= WIDTH || y as usize >= HEIGHT {
        return;
    }

    let index = (y as usize * WIDTH + x as usize) * 4;
    *FB.add(index) = b;
    *FB.add(index + 1) = g;
    *FB.add(index + 2) = r;
    *FB.add(index + 3) = 0xFF;
}
pub unsafe fn draw_char(x: u32, y: u32, ch: u8) {
    crate::font::draw_char(FB, WIDTH, HEIGHT, x, y, ch, 0xFF, 0xFF, 0xFF);
}

pub unsafe fn draw_text(x: u32, y: u32, text: &str, r: u8, g: u8, b: u8) {
    draw_bytes(x, y, text.as_bytes(), r, g, b);
}

pub unsafe fn draw_bytes(x: u32, y: u32, text: &[u8], r: u8, g: u8, b: u8) {
    let mut cx = x;
    for &ch in text {
        crate::font::draw_char(FB, WIDTH, HEIGHT, cx, y, ch, r, g, b);
        cx = cx.saturating_add(8);
    }
}

pub unsafe fn fill_rect(x: u32, y: u32, w: u32, h: u32, r: u8, g: u8, b: u8) {
    let max_x = (WIDTH as u32).min(x.saturating_add(w));
    let max_y = (HEIGHT as u32).min(y.saturating_add(h));
    for py in y..max_y {
        for px in x..max_x {
            write_pixel(px, py, r, g, b);
        }
    }
}
