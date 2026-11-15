//! DDC/CI, the way a Mac talks to a monitor over its cable: tiny I²C
//! messages that read or set one "VCP" control (brightness, volume, …).
//! Only bytes live here, so all of it is tested on any OS; `mac.rs` moves
//! the bytes.
//!
//! A request goes to the monitor's I²C address 0x37 (0x6E on the wire),
//! from the host's address 0x51:
//!   get: 0x82 0x01 vcp chk
//!   set: 0x84 0x03 vcp hi lo chk
//! where 0x80 | n is the payload length and chk is the XOR of 0x6E, 0x51 and
//! every byte before it. The reply to a get is
//!   0x6E 0x88 0x02 result vcp type max_hi max_lo cur_hi cur_lo chk
//! with chk the XOR of 0x50 and the ten bytes before it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// The monitor's I²C address.
pub const CHIP: u8 = 0x37;
/// The address behind one HDMI bridge chip (MCDP29xx) on some Macs.
pub const CHIP_MCDP: u8 = 0xB7;
/// The host's address, sent as the I²C sub-address of every request.
pub const HOST: u8 = 0x51;
/// The monitor's address as written on the wire (CHIP << 1).
const WIRE: u8 = CHIP << 1;
/// What a reply's checksum starts from.
const REPLY_SEED: u8 = 0x50;
/// Bytes to read for a reply.
pub const REPLY_LEN: usize = 11;

/// The controls we use.
pub mod vcp {
    pub const BRIGHTNESS: u8 = 0x10;
    pub const VOLUME: u8 = 0x62;
    /// 1 = muted, 2 = sound on.
    pub const MUTE: u8 = 0x8D;
}

pub const MUTE_ON: u16 = 1;
pub const MUTE_OFF: u16 = 2;

fn xor(seed: u8, bytes: &[u8]) -> u8 {
    bytes.iter().fold(seed, |a, b| a ^ b)
}

/// The bytes after the sub-address for "what is control `code`?".
pub fn get_request(code: u8) -> [u8; 4] {
    let mut b = [0x82, 0x01, code, 0];
    b[3] = xor(WIRE ^ HOST, &b[..3]);
    b
}

/// The bytes after the sub-address for "set control `code` to `value`".
pub fn set_request(code: u8, value: u16) -> [u8; 6] {
    let [hi, lo] = value.to_be_bytes();
    let mut b = [0x84, 0x03, code, hi, lo, 0];
    b[5] = xor(WIRE ^ HOST, &b[..5]);
    b
}

/// A control's value and its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub current: u16,
    pub max: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadReply {
    Short,
    /// All zero or all 0xFF: nobody answered.
    Silent,
    Checksum,
    /// Not a reply to a get, or a reply about another control.
    Other,
    /// The monitor says it has no such control.
    Unsupported,
}

/// Read a reply to `get_request(code)`.
pub fn parse_reply(code: u8, b: &[u8]) -> Result<Reading, BadReply> {
    if b.len() < REPLY_LEN {
        return Err(BadReply::Short);
    }
    let b = &b[..REPLY_LEN];
    if b.iter().all(|&x| x == 0) || b.iter().all(|&x| x == 0xFF) {
        return Err(BadReply::Silent);
    }
    if xor(REPLY_SEED, &b[..10]) != b[10] {
        return Err(BadReply::Checksum);
    }
    if b[2] != 0x02 || b[4] != code {
        return Err(BadReply::Other);
    }
    if b[3] != 0 {
        return Err(BadReply::Unsupported);
    }
    let max = u16::from_be_bytes([b[6], b[7]]);
    let current = u16::from_be_bytes([b[8], b[9]]);
    if max == 0 {
        return Err(BadReply::Unsupported);
    }
    Ok(Reading { current: current.min(max), max })
}

/// 0…1 → the monitor's scale.
pub fn to_raw(level: f32, max: u16) -> u16 {
    let l = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
    (l * max as f32).round() as u16
}

/// The monitor's scale → 0…1.
pub fn to_level(r: Reading) -> f32 {
    if r.max == 0 {
        return 0.0;
    }
    (r.current.min(r.max) as f32 / r.max as f32).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------- finding the cable

/// What the I/O registry walk meets, in order (Apple silicon): a
/// framebuffer (one per display output, by registry id) and the AV service
/// proxies that follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegEntry {
    Framebuffer(u64),
    /// A `DCPAVServiceProxy`. `external`: its Location is "External" (the
    /// built-in panel has one too). `mcdp`: it sits behind the MCDP29xx
    /// HDMI bridge.
    Proxy { external: bool, mcdp: bool },
}

/// For each framebuffer, the first external proxy after it (before the next
/// framebuffer): (framebuffer id, index of that proxy in `entries`, I²C chip).
pub fn link_proxies(entries: &[RegEntry]) -> Vec<(u64, usize, u8)> {
    let mut out: Vec<(u64, usize, u8)> = Vec::new();
    let mut fb: Option<u64> = None;
    for (i, e) in entries.iter().enumerate() {
        match *e {
            RegEntry::Framebuffer(id) => fb = Some(id),
            RegEntry::Proxy { external: true, mcdp } => {
                if let Some(id) = fb.take() {
                    out.push((id, i, if mcdp { CHIP_MCDP } else { CHIP }));
                }
            }
            RegEntry::Proxy { .. } => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply as a monitor would send it.
    fn reply(code: u8, result: u8, max: u16, cur: u16) -> Vec<u8> {
        let [mh, ml] = max.to_be_bytes();
        let [ch, cl] = cur.to_be_bytes();
        let mut b = vec![0x6E, 0x88, 0x02, result, code, 0x00, mh, ml, ch, cl];
        b.push(xor(REPLY_SEED, &b));
        b
    }

    #[test]
    fn requests_match_the_spec() {
        // known-good bytes: brightness get, and brightness set to 50
        assert_eq!(get_request(0x10), [0x82, 0x01, 0x10, 0x6E ^ 0x51 ^ 0x82 ^ 0x01 ^ 0x10]);
        assert_eq!(get_request(0x10)[3], 0xAC);
        let s = set_request(0x10, 50);
        assert_eq!(&s[..5], &[0x84, 0x03, 0x10, 0x00, 0x32]);
        assert_eq!(s[5], 0x6E ^ 0x51 ^ 0x84 ^ 0x03 ^ 0x10 ^ 0x00 ^ 0x32);
        // a big value splits into high and low bytes
        let s = set_request(vcp::VOLUME, 0x0123);
        assert_eq!((s[3], s[4]), (0x01, 0x23));
    }

    #[test]
    fn reads_a_good_reply() {
        let r = parse_reply(0x10, &reply(0x10, 0, 100, 42)).unwrap();
        assert_eq!(r, Reading { current: 42, max: 100 });
        assert!((to_level(r) - 0.42).abs() < 1e-6);
        // a longer buffer (we may read 12) is fine
        let mut b = reply(0x62, 0, 100, 7);
        b.push(0);
        assert_eq!(parse_reply(0x62, &b).unwrap().current, 7);
    }

    #[test]
    fn rejects_bad_replies() {
        assert_eq!(parse_reply(0x10, &[0x6E, 0x88]), Err(BadReply::Short));
        assert_eq!(parse_reply(0x10, &[0; 11]), Err(BadReply::Silent));
        assert_eq!(parse_reply(0x10, &[0xFF; 11]), Err(BadReply::Silent));
        let mut b = reply(0x10, 0, 100, 42);
        b[9] ^= 1;
        assert_eq!(parse_reply(0x10, &b), Err(BadReply::Checksum));
        assert_eq!(parse_reply(0x62, &reply(0x10, 0, 100, 42)), Err(BadReply::Other));
        assert_eq!(parse_reply(0x62, &reply(0x62, 1, 100, 0)), Err(BadReply::Unsupported));
        assert_eq!(parse_reply(0x62, &reply(0x62, 0, 0, 0)), Err(BadReply::Unsupported));
    }

    #[test]
    fn a_current_above_max_is_clamped() {
        let r = parse_reply(0x10, &reply(0x10, 0, 100, 250)).unwrap();
        assert_eq!(r.current, 100);
    }

    #[test]
    fn levels_round_trip() {
        for max in [1u16, 100, 255, 1000] {
            for i in 0..=20 {
                let l = i as f32 / 20.0;
                let raw = to_raw(l, max);
                assert!(raw <= max);
                assert!((to_level(Reading { current: raw, max }) - l).abs() <= 0.5 / max as f32 + 1e-6);
            }
        }
        assert_eq!(to_raw(-1.0, 100), 0);
        assert_eq!(to_raw(2.0, 100), 100);
        assert_eq!(to_raw(f32::NAN, 100), 0);
        assert_eq!(to_level(Reading { current: 5, max: 0 }), 0.0);
    }

    #[test]
    fn proxies_link_to_the_framebuffer_before_them() {
        use RegEntry::*;
        let e = [
            // built-in panel: its proxy is internal, so no link
            Framebuffer(1),
            Proxy { external: false, mcdp: false },
            // a monitor on USB-C: the first external proxy wins
            Framebuffer(2),
            Proxy { external: true, mcdp: false },
            Proxy { external: true, mcdp: false },
            // a framebuffer with nothing plugged in
            Framebuffer(3),
            // HDMI through the bridge chip
            Framebuffer(4),
            Proxy { external: true, mcdp: true },
        ];
        assert_eq!(link_proxies(&e), vec![(2, 3, CHIP), (4, 7, CHIP_MCDP)]);
        // a proxy before any framebuffer belongs to nobody
        assert!(link_proxies(&[Proxy { external: true, mcdp: false }]).is_empty());
    }
}
