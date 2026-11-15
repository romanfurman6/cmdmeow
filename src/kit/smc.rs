//! The System Management Controller: read (and, as root, write) its keys
//! through the AppleSMC user client. Reading needs no admin rights.
//! Elsewhere than macOS only the byte coding is here.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn fourcc(s: &str) -> u32 {
    s.bytes().take(4).fold(0u32, |a, b| (a << 8) | b as u32)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn fourcc_str(v: u32) -> String {
    v.to_be_bytes().iter().map(|b| *b as char).collect()
}

/// Encode `v` as SMC bytes of a numeric type, `size` bytes long.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn encode(kind: u32, v: f32, size: usize) -> Option<Vec<u8>> {
    let b = match fourcc_str(kind).as_str() {
        "flt " => v.to_le_bytes().to_vec(),
        "fpe2" => ((v.max(0.0) * 4.0) as u16).to_be_bytes().to_vec(),
        "ui8 " => vec![v.clamp(0.0, 255.0) as u8],
        "ui16" => (v.clamp(0.0, 65535.0) as u16).to_be_bytes().to_vec(),
        "ui32" => (v.max(0.0) as u32).to_be_bytes().to_vec(),
        _ => return None,
    };
    (b.len() == size).then_some(b)
}

/// Decode SMC bytes of a numeric type.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn decode(kind: u32, b: &[u8]) -> Option<f32> {
    let t = fourcc_str(kind);
    Some(match t.as_str() {
        "flt " if b.len() >= 4 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        "fpe2" if b.len() >= 2 => u16::from_be_bytes([b[0], b[1]]) as f32 / 4.0,
        "sp78" if b.len() >= 2 => i16::from_be_bytes([b[0], b[1]]) as f32 / 256.0,
        "ui8 " if !b.is_empty() => b[0] as f32,
        "ui16" if b.len() >= 2 => u16::from_be_bytes([b[0], b[1]]) as f32,
        "ui32" if b.len() >= 4 => u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
pub use mac::{KeyInfo, Smc};

#[cfg(target_os = "macos")]
mod mac {
    use super::{decode, encode, fourcc, fourcc_str};
    use std::ffi::{c_void, CString};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const std::ffi::c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
        fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
        fn IOServiceClose(connect: u32) -> i32;
        fn IOObjectRelease(obj: u32) -> i32;
        fn IOConnectCallStructMethod(conn: u32, selector: u32, input: *const c_void, in_size: usize, output: *mut c_void, out_size: *mut usize) -> i32;
    }

    // ------------------------------------------------------------ SMC

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Vers {
        major: u8,
        minor: u8,
        build: u8,
        reserved: u8,
        release: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct PLimit {
        version: u16,
        length: u16,
        cpu: u32,
        gpu: u32,
        mem: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug)]
    pub struct KeyInfo {
        size: u32,
        kind: u32,
        attributes: u8,
    }

    /// `SMCKeyData_t` from Apple's SMC user client.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct KeyData {
        key: u32,
        vers: Vers,
        plimit: PLimit,
        info: KeyInfo,
        result: u8,
        status: u8,
        data8: u8,
        data32: u32,
        bytes: [u8; 32],
    }

    const _: () = assert!(std::mem::size_of::<KeyData>() == 80);

    const KERNEL_INDEX_SMC: u32 = 2;
    const CMD_READ_BYTES: u8 = 5;
    const CMD_WRITE_BYTES: u8 = 6;
    const CMD_READ_INDEX: u8 = 8;
    const CMD_READ_KEYINFO: u8 = 9;

    pub struct Smc {
        conn: u32,
    }

    impl Smc {
        pub fn open() -> Option<Smc> {
            unsafe {
                let name = CString::new("AppleSMC").unwrap();
                let matching = IOServiceMatching(name.as_ptr());
                if matching.is_null() {
                    return None;
                }
                let service = IOServiceGetMatchingService(0, matching);
                if service == 0 {
                    return None;
                }
                let mut conn = 0u32;
                #[allow(deprecated)]
                let task = libc::mach_task_self();
                let kr = IOServiceOpen(service, task, 0, &mut conn);
                IOObjectRelease(service);
                (kr == 0 && conn != 0).then_some(Smc { conn })
            }
        }

        fn call(&self, input: &KeyData) -> Option<KeyData> {
            let mut out = KeyData::default();
            let mut size = std::mem::size_of::<KeyData>();
            let kr = unsafe { IOConnectCallStructMethod(self.conn, KERNEL_INDEX_SMC, input as *const KeyData as *const c_void, std::mem::size_of::<KeyData>(), &mut out as *mut KeyData as *mut c_void, &mut size) };
            (kr == 0 && out.result == 0).then_some(out)
        }

        pub fn info(&self, key: &str) -> Option<KeyInfo> {
            let input = KeyData { key: fourcc(key), data8: CMD_READ_KEYINFO, ..Default::default() };
            self.call(&input).map(|o| o.info).filter(|i| i.size > 0 && i.size <= 32)
        }

        /// (size, type) of a key, if this SMC has it.
        pub fn info_of(&self, key: &str) -> Option<(u32, String)> {
            self.info(key).map(|i| (i.size, fourcc_str(i.kind)))
        }

        /// Write a number to a key in the key's own type. Needs root.
        pub fn write(&self, key: &str, v: f32) -> bool {
            let Some(info) = self.info(key) else { return false };
            let Some(b) = encode(info.kind, v, info.size as usize) else { return false };
            self.write_raw(key, info, &b)
        }

        /// Write raw bytes to a key; they must be exactly the key's size.
        /// Needs root.
        pub fn write_bytes(&self, key: &str, b: &[u8]) -> bool {
            self.info(key).is_some_and(|info| self.write_raw(key, info, b))
        }

        fn write_raw(&self, key: &str, info: KeyInfo, b: &[u8]) -> bool {
            if b.len() != info.size as usize {
                return false;
            }
            let mut input = KeyData { key: fourcc(key), info, data8: CMD_WRITE_BYTES, ..Default::default() };
            input.bytes[..b.len()].copy_from_slice(b);
            self.call(&input).is_some()
        }

        /// A key's raw bytes.
        pub fn read_bytes(&self, key: &str) -> Option<Vec<u8>> {
            let info = self.info(key)?;
            let out = self.call(&KeyData { key: fourcc(key), info, data8: CMD_READ_BYTES, ..Default::default() })?;
            Some(out.bytes[..info.size as usize].to_vec())
        }

        pub fn read_with(&self, key: &str, info: KeyInfo) -> Option<f32> {
            let input = KeyData { key: fourcc(key), info, data8: CMD_READ_BYTES, ..Default::default() };
            let out = self.call(&input)?;
            decode(info.kind, &out.bytes[..info.size as usize])
        }

        pub fn read(&self, key: &str) -> Option<f32> {
            self.read_with(key, self.info(key)?)
        }
    }

    impl KeyInfo {
        pub fn size(&self) -> usize {
            self.size as usize
        }
        /// The four-letter type, "flt ", "sp78"…
        pub fn kind(&self) -> String {
            fourcc_str(self.kind)
        }
    }

    impl Smc {
        /// How many keys this SMC has.
        pub fn key_count(&self) -> u32 {
            self.read("#KEY").unwrap_or(0.0) as u32
        }

        /// The name of key number `i`.
        pub fn key_at(&self, i: u32) -> Option<String> {
            let input = KeyData { data8: CMD_READ_INDEX, data32: i, ..Default::default() };
            self.call(&input).map(|k| fourcc_str(k.key))
        }
    }

    impl Drop for Smc {
        fn drop(&mut self) {
            unsafe {
                IOServiceClose(self.conn);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, fourcc};

    #[test]
    fn encodes_smc_types() {
        assert_eq!(encode(fourcc("flt "), 2140.0, 4), Some(2140.0f32.to_le_bytes().to_vec()));
        assert_eq!(encode(fourcc("fpe2"), 2140.0, 2), Some(vec![0x21, 0x70]));
        assert_eq!(encode(fourcc("ui8 "), 1.0, 1), Some(vec![1]));
        assert_eq!(encode(fourcc("ui8 "), 1.0, 2), None);
        assert_eq!(encode(fourcc("ch8*"), 1.0, 1), None);
        for (k, v, n) in [("flt ", 3196.5, 4), ("fpe2", 1350.0, 2), ("ui16", 7.0, 2)] {
            assert_eq!(decode(fourcc(k), &encode(fourcc(k), v, n).unwrap()), Some(v));
        }
    }

    #[test]
    fn decodes_smc_types() {
        assert_eq!(decode(fourcc("flt "), &2140.0f32.to_le_bytes()), Some(2140.0));
        assert_eq!(decode(fourcc("fpe2"), &[0x21, 0x70]), Some(2140.0));
        assert_eq!(decode(fourcc("sp78"), &[0x40, 0x80]), Some(64.5));
        assert_eq!(decode(fourcc("ui8 "), &[2]), Some(2.0));
        assert_eq!(decode(fourcc("ch8*"), &[1]), None);
    }
}
