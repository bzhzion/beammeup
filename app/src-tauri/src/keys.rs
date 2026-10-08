//! Named keys for `beammeup key`, as the bytes a terminal emulator would send.

const AVAILABLE: &str = "enter, tab, esc, space, backspace, delete, insert, up, down, left, \
    right, home, end, pageup, pagedown, f1 to f12, a single character, ctrl-<letter>, alt-<key>, \
    shift-tab, and ctrl/alt/shift combined with the arrows, home, end, pageup, pagedown, delete, \
    insert and f1 to f12";

#[derive(Clone, Copy, Default)]
struct Mods {
    shift: bool,
    alt: bool,
    ctrl: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }

    fn xterm(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }
}

/// xterm sequences in normal cursor mode. A program that switched to application cursor mode
/// expects `ESC O A` for the arrows and may ignore these: `send` with the exact sequence remains.
pub fn key_bytes(name: &str) -> Result<Vec<u8>, String> {
    let lowered = name.trim().to_lowercase().replace('+', "-");
    if lowered == "-" {
        return Ok(vec![b'-']);
    }
    let mut parts: Vec<&str> = lowered.split('-').collect();
    let base = parts.pop().unwrap_or("");
    let mut mods = Mods::default();
    for part in parts {
        match part {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "meta" | "option" => mods.alt = true,
            "shift" => mods.shift = true,
            _ => return Err(unknown(name)),
        }
    }
    encode(base, mods).ok_or_else(|| unknown(name))
}

fn unknown(name: &str) -> String {
    format!("unknown key: {name} (available: {AVAILABLE})")
}

fn encode(base: &str, mods: Mods) -> Option<Vec<u8>> {
    if let Some(letter) = cursor_final(base) {
        return Some(csi_letter(letter, mods));
    }
    if let Some(code) = tilde_code(base) {
        return Some(csi_tilde(code, mods));
    }
    if let Some(letter) = function_final(base) {
        return Some(if mods.any() {
            csi_letter(letter, mods)
        } else {
            vec![0x1b, b'O', letter]
        });
    }
    plain(base, mods)
}

fn cursor_final(base: &str) -> Option<u8> {
    Some(match base {
        "up" => b'A',
        "down" => b'B',
        "right" => b'C',
        "left" => b'D',
        "home" => b'H',
        "end" => b'F',
        _ => return None,
    })
}

fn tilde_code(base: &str) -> Option<u8> {
    Some(match base {
        "insert" => 2,
        "delete" | "del" => 3,
        "pageup" | "pgup" => 5,
        "pagedown" | "pgdn" => 6,
        "f5" => 15,
        "f6" => 17,
        "f7" => 18,
        "f8" => 19,
        "f9" => 20,
        "f10" => 21,
        "f11" => 23,
        "f12" => 24,
        _ => return None,
    })
}

fn function_final(base: &str) -> Option<u8> {
    Some(match base {
        "f1" => b'P',
        "f2" => b'Q',
        "f3" => b'R',
        "f4" => b'S',
        _ => return None,
    })
}

fn csi_letter(letter: u8, mods: Mods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.xterm(), letter as char).into_bytes()
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn csi_tilde(code: u8, mods: Mods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{};{}~", code, mods.xterm()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn with_alt(mut bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn plain(base: &str, mods: Mods) -> Option<Vec<u8>> {
    if base == "tab" && mods.shift {
        return if mods.ctrl {
            None
        } else {
            Some(with_alt(vec![0x1b, b'[', b'Z'], mods.alt))
        };
    }
    let mut named = true;
    let mut bytes = match base {
        "enter" | "return" => vec![b'\r'],
        "tab" => vec![b'\t'],
        "esc" | "escape" => vec![0x1b],
        "space" => vec![b' '],
        "backspace" => vec![0x7f],
        _ => {
            named = false;
            vec![single_char(base)?]
        }
    };
    if mods.shift {
        if named || !bytes[0].is_ascii_alphabetic() {
            return None;
        }
        bytes[0] = bytes[0].to_ascii_uppercase();
    }
    if mods.ctrl {
        if named || !bytes[0].is_ascii_alphabetic() {
            return None;
        }
        bytes[0] = bytes[0].to_ascii_lowercase() - b'a' + 1;
    }
    Some(with_alt(bytes, mods.alt))
}

fn single_char(base: &str) -> Option<u8> {
    let mut chars = base.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii_graphic() {
        return None;
    }
    Some(c as u8)
}

#[cfg(test)]
mod tests {
    use super::key_bytes;

    fn bytes(name: &str) -> Vec<u8> {
        key_bytes(name).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn historical_keys_keep_their_bytes() {
        assert_eq!(bytes("ctrl-c"), [0x03]);
        assert_eq!(bytes("ctrl-d"), [0x04]);
        assert_eq!(bytes("ctrl-z"), [0x1a]);
        assert_eq!(bytes("enter"), b"\r");
        assert_eq!(bytes("tab"), b"\t");
        assert_eq!(bytes("esc"), [0x1b]);
        assert_eq!(bytes("escape"), [0x1b]);
    }

    #[test]
    fn arrows_and_navigation() {
        assert_eq!(bytes("up"), b"\x1b[A");
        assert_eq!(bytes("down"), b"\x1b[B");
        assert_eq!(bytes("right"), b"\x1b[C");
        assert_eq!(bytes("left"), b"\x1b[D");
        assert_eq!(bytes("home"), b"\x1b[H");
        assert_eq!(bytes("end"), b"\x1b[F");
        assert_eq!(bytes("pageup"), b"\x1b[5~");
        assert_eq!(bytes("pagedown"), b"\x1b[6~");
        assert_eq!(bytes("delete"), b"\x1b[3~");
        assert_eq!(bytes("insert"), b"\x1b[2~");
        assert_eq!(bytes("backspace"), [0x7f]);
        assert_eq!(bytes("space"), b" ");
    }

    #[test]
    fn function_keys() {
        assert_eq!(bytes("f1"), b"\x1bOP");
        assert_eq!(bytes("f4"), b"\x1bOS");
        assert_eq!(bytes("f5"), b"\x1b[15~");
        assert_eq!(bytes("f10"), b"\x1b[21~");
        assert_eq!(bytes("f12"), b"\x1b[24~");
    }

    #[test]
    fn modifiers_follow_the_xterm_scheme() {
        assert_eq!(bytes("ctrl-left"), b"\x1b[1;5D");
        assert_eq!(bytes("shift-right"), b"\x1b[1;2C");
        assert_eq!(bytes("alt-up"), b"\x1b[1;3A");
        assert_eq!(bytes("ctrl-shift-end"), b"\x1b[1;6F");
        assert_eq!(bytes("ctrl-delete"), b"\x1b[3;5~");
        assert_eq!(bytes("shift-f5"), b"\x1b[15;2~");
        assert_eq!(bytes("ctrl-f1"), b"\x1b[1;5P");
        assert_eq!(bytes("shift-tab"), b"\x1b[Z");
    }

    #[test]
    fn letters_and_alt() {
        assert_eq!(bytes("ctrl-a"), [0x01]);
        assert_eq!(bytes("ctrl-l"), [0x0c]);
        assert_eq!(bytes("ctrl-w"), [0x17]);
        assert_eq!(bytes("alt-b"), b"\x1bb");
        assert_eq!(bytes("alt-backspace"), b"\x1b\x7f");
        assert_eq!(bytes("alt-enter"), b"\x1b\r");
        assert_eq!(bytes("y"), b"y");
        assert_eq!(bytes("shift-a"), b"A");
        assert_eq!(bytes("-"), b"-");
    }

    #[test]
    fn spelling_is_forgiving() {
        assert_eq!(bytes("CTRL-C"), [0x03]);
        assert_eq!(bytes("Ctrl+C"), [0x03]);
        assert_eq!(bytes(" Up "), b"\x1b[A");
        assert_eq!(bytes("pgup"), b"\x1b[5~");
        assert_eq!(bytes("meta-f"), b"\x1bf");
    }

    #[test]
    fn rejects_what_it_cannot_encode() {
        for name in [
            "",
            "ctrl-",
            "foo",
            "enterr",
            "ctrl-enter",
            "ctrl-1",
            "shift-1",
            "ctrl-tab",
            "hyper-a",
            "ab",
            "f13",
        ] {
            assert!(key_bytes(name).is_err(), "{name:?} should be rejected");
        }
    }

    #[test]
    fn error_lists_the_available_keys() {
        let message = key_bytes("nope").unwrap_err();
        assert!(message.contains("unknown key: nope"));
        assert!(message.contains("pagedown"));
    }
}
