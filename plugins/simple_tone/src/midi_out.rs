//! A virtual MIDI output port, so a host or any other app on the machine can
//! hear the test panel as if it were a hardware controller.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};

/// Every open instance takes a number, so two instances get distinct port
/// names and the first one keeps the plain, stable name.
static INSTANCES: AtomicU32 = AtomicU32::new(0);

const LOG_LEN: usize = 12;

/// How an endless encoder writes a signed step into a 7-bit CC value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relative {
    /// 64 is rest: 65 is +1, 63 is -1.
    Offset64,
    /// 1 is +1, 127 is -1.
    TwosComplement,
    /// Bit 6 is the sign: 1 is +1, 65 is -1.
    SignBit,
}

impl Relative {
    pub const ALL: [Relative; 3] = [
        Relative::Offset64,
        Relative::TwosComplement,
        Relative::SignBit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Relative::Offset64 => "Offset 64",
            Relative::TwosComplement => "Two's complement",
            Relative::SignBit => "Sign bit",
        }
    }

    /// One step of at most 63 in either direction; a zero step sends nothing.
    pub fn encode(self, delta: i32) -> Option<u8> {
        let d = delta.clamp(-63, 63);
        if d == 0 {
            return None;
        }
        let v = match self {
            Relative::Offset64 => 64 + d,
            Relative::TwosComplement if d > 0 => d,
            Relative::TwosComplement => 128 + d,
            Relative::SignBit if d > 0 => d,
            Relative::SignBit => 64 - d,
        };
        Some(v as u8)
    }
}

pub fn note_on(channel: u8, note: u8, velocity: u8) -> [u8; 3] {
    [0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F]
}

/// Many controllers release a key with a note-on at velocity 0 instead of a
/// note-off; `as_zero_velocity` sends that form.
pub fn note_off(channel: u8, note: u8, as_zero_velocity: bool) -> [u8; 3] {
    if as_zero_velocity {
        note_on(channel, note, 0)
    } else {
        [0x80 | (channel & 0x0F), note & 0x7F, 0]
    }
}

pub fn cc(channel: u8, number: u8, value: u8) -> [u8; 3] {
    [0xB0 | (channel & 0x0F), number & 0x7F, value & 0x7F]
}

/// `value` is 14-bit, 8192 is centre.
pub fn pitch_bend(channel: u8, value: u16) -> [u8; 3] {
    let v = value.min(16383);
    [0xE0 | (channel & 0x0F), (v & 0x7F) as u8, (v >> 7) as u8]
}

pub fn channel_pressure(channel: u8, value: u8) -> [u8; 2] {
    [0xD0 | (channel & 0x0F), value & 0x7F]
}

pub fn describe(bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let ch = (bytes[0] & 0x0F) + 1;
    let what = match bytes[0] & 0xF0 {
        0x80 => format!("note off {}", bytes[1]),
        0x90 if bytes[2] == 0 => format!("note on {} vel 0 (off)", bytes[1]),
        0x90 => format!("note on {} vel {}", bytes[1], bytes[2]),
        0xB0 => format!("CC {} = {}", bytes[1], bytes[2]),
        0xD0 => format!("pressure {}", bytes[1]),
        0xE0 => format!("pitch bend {}", bytes[1] as u16 | (bytes[2] as u16) << 7),
        _ => "other".into(),
    };
    format!("{}  ch {ch}  {what}", hex.join(" "))
}

/// The port and a log of what went out. Opened on first use from the editor
/// and kept for the plugin's lifetime, so closing the window does not unplug
/// the device. Only the GUI thread sends; the audio thread never touches it.
pub struct MidiOut {
    pub name: String,
    #[cfg(unix)]
    conn: Option<midir::MidiOutputConnection>,
    pub error: Option<String>,
    pub log: VecDeque<String>,
}

impl MidiOut {
    pub fn open() -> Self {
        let n = INSTANCES.fetch_add(1, Ordering::Relaxed);
        let name = if n == 0 {
            "SimpleTone MIDI".to_string()
        } else {
            format!("SimpleTone MIDI {}", n + 1)
        };
        Self::open_named(name)
    }

    #[cfg(unix)]
    fn open_named(name: String) -> Self {
        use midir::os::unix::VirtualOutput;
        let opened = midir::MidiOutput::new("SimpleTone")
            .map_err(|e| e.to_string())
            .and_then(|out| out.create_virtual(&name).map_err(|e| e.to_string()));
        let (conn, error) = match opened {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(format!("could not open the virtual port: {e}"))),
        };
        Self {
            name,
            conn,
            error,
            log: VecDeque::new(),
        }
    }

    #[cfg(not(unix))]
    fn open_named(name: String) -> Self {
        Self {
            name,
            error: Some(
                "Windows has no virtual MIDI ports. Install loopMIDI and route a DAW's MIDI out to it."
                    .into(),
            ),
            log: VecDeque::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        #[cfg(unix)]
        return self.conn.is_some();
        #[cfg(not(unix))]
        return false;
    }

    pub fn send(&mut self, bytes: &[u8]) {
        #[cfg(unix)]
        if let Some(conn) = self.conn.as_mut() {
            if let Err(e) = conn.send(bytes) {
                self.error = Some(format!("send failed: {e}"));
            }
        }
        if self.log.len() == LOG_LEN {
            self.log.pop_back();
        }
        self.log.push_front(describe(bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_64_centres_on_64() {
        assert_eq!(Relative::Offset64.encode(1), Some(65));
        assert_eq!(Relative::Offset64.encode(-1), Some(63));
        assert_eq!(Relative::Offset64.encode(63), Some(127));
        assert_eq!(Relative::Offset64.encode(-63), Some(1));
    }

    #[test]
    fn twos_complement_wraps_negatives() {
        assert_eq!(Relative::TwosComplement.encode(1), Some(1));
        assert_eq!(Relative::TwosComplement.encode(-1), Some(127));
        assert_eq!(Relative::TwosComplement.encode(-63), Some(65));
    }

    #[test]
    fn sign_bit_sets_bit_six() {
        assert_eq!(Relative::SignBit.encode(1), Some(1));
        assert_eq!(Relative::SignBit.encode(-1), Some(65));
        assert_eq!(Relative::SignBit.encode(-63), Some(127));
    }

    #[test]
    fn zero_and_overflow() {
        assert_eq!(Relative::Offset64.encode(0), None);
        assert_eq!(Relative::TwosComplement.encode(500), Some(63));
    }

    #[test]
    fn messages() {
        assert_eq!(note_on(0, 60, 100), [0x90, 60, 100]);
        assert_eq!(note_off(15, 60, false), [0x8F, 60, 0]);
        assert_eq!(note_off(0, 60, true), [0x90, 60, 0]);
        assert_eq!(cc(2, 7, 127), [0xB2, 7, 127]);
        assert_eq!(pitch_bend(0, 8192), [0xE0, 0, 64]);
        assert_eq!(channel_pressure(1, 50), [0xD1, 50]);
    }
}
