mod math;
mod power;
mod time;

const COMMANDS: &[(&str, &str)] = &[
    ("help", "help [command] - show command help"),
    ("clear", "clear - clear the screen"),
    ("info", "info - show AlphaOS information"),
    ("version", "version - show the AlphaOS version"),
    ("uname", "uname - show kernel and architecture"),
    ("whoami", "whoami - show the current user"),
    ("echo", "echo <text> - print text"),
    ("uptime", "uptime - show elapsed PIT time"),
    ("time", "time - alias for uptime"),
    ("ticks", "ticks - show raw timer ticks"),
    ("calc", "calc <a> <+|-|*|/> <b> - integer math"),
    ("shutdown", "shutdown - power off the system"),
    ("restart", "restart - reboot the system"),
    ("commands", "commands - list built-in shell commands"),
];

pub struct CommandResult {
    pub clear_screen: bool,
    pub output_lines: u32,
}

pub fn execute(command_line: &str, output_y: u32) -> CommandResult {
    let command_line = command_line.trim();
    let (name, args) = command_line
        .split_once(char::is_whitespace)
        .map(|(name, args)| (name, args.trim()))
        .unwrap_or((command_line, ""));

    if name.eq_ignore_ascii_case("clear") && args.is_empty() {
        return CommandResult {
            clear_screen: true,
            output_lines: 0,
        };
    }

    if name.eq_ignore_ascii_case("help") {
        return show_help(args, output_y);
    }
    if name.is_empty() {
        return CommandResult {
            clear_screen: false,
            output_lines: 0,
        };
    }

    let result = if name.eq_ignore_ascii_case("info") {
        draw_line(output_y, "AlphaOS - experimental x86-64 UEFI OS", 0, 255, 0);
        1
    } else if name.eq_ignore_ascii_case("version") {
        draw_line(output_y, "AlphaOS version 0.1.0", 255, 255, 255);
        1
    } else if name.eq_ignore_ascii_case("uname") {
        draw_line(output_y, "AlphaOS kernel | x86_64 | UEFI", 255, 255, 255);
        1
    } else if name.eq_ignore_ascii_case("whoami") {
        draw_line(output_y, "alpha", 255, 255, 255);
        1
    } else if name.eq_ignore_ascii_case("echo") {
        draw_line(output_y, args, 255, 255, 255);
        1
    } else if name.eq_ignore_ascii_case("uptime") || name.eq_ignore_ascii_case("time") {
        time::show_uptime(output_y)
    } else if name.eq_ignore_ascii_case("ticks") {
        time::show_ticks(output_y)
    } else if name.eq_ignore_ascii_case("calc") {
        math::calculate(args, output_y)
    } else if name.eq_ignore_ascii_case("shutdown") && args.is_empty() {
        power::shutdown(output_y)
    } else if name.eq_ignore_ascii_case("restart") && args.is_empty() {
        power::restart(output_y)
    } else if name.eq_ignore_ascii_case("commands") {
        show_commands(output_y)
    } else if name.eq_ignore_ascii_case("apps") || name.eq_ignore_ascii_case("run") {
        draw_line(
            output_y,
            "Apps are not supported yet (no filesystem or app loader).",
            255,
            0,
            0,
        );
        1
    } else if name.eq_ignore_ascii_case("drivers") {
        draw_line(output_y, "Driver loading is not supported yet.", 255, 0, 0);
        1
    } else {
        draw_line(output_y, "Unknown command. Type help.", 255, 0, 0);
        1
    };

    CommandResult {
        clear_screen: false,
        output_lines: result,
    }
}

fn show_help(topic: &str, output_y: u32) -> CommandResult {
    if !topic.is_empty() {
        if let Some((_, description)) = COMMANDS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(topic))
        {
            draw_line(output_y, description, 255, 255, 255);
            return CommandResult {
                clear_screen: false,
                output_lines: 1,
            };
        }

        draw_line(output_y, "No help found for that command.", 255, 0, 0);
        return CommandResult {
            clear_screen: false,
            output_lines: 1,
        };
    }

    for (index, (_, description)) in COMMANDS.iter().enumerate() {
        draw_line(output_y + index as u32 * 16, description, 255, 255, 255);
    }
    draw_line(
        output_y + COMMANDS.len() as u32 * 16,
        "Apps and drivers need filesystem/loader support first.",
        0,
        200,
        255,
    );

    CommandResult {
        clear_screen: false,
        output_lines: COMMANDS.len() as u32 + 1,
    }
}

fn show_commands(output_y: u32) -> u32 {
    let mut line = 0;
    for (name, _) in COMMANDS {
        draw_line(output_y + line * 16, name, 255, 255, 255);
        line += 1;
    }
    draw_line(
        output_y + line * 16,
        "Apps and drivers are not implemented yet.",
        0,
        200,
        255,
    );
    line + 1
}

pub(super) fn draw_line(y: u32, text: &str, red: u8, green: u8, blue: u8) {
    draw_bytes(y, text.as_bytes(), red, green, blue);
}

pub(super) fn draw_bytes(y: u32, text: &[u8], red: u8, green: u8, blue: u8) {
    unsafe {
        crate::video::draw_bytes(10, y, text, red, green, blue);
    }
}
