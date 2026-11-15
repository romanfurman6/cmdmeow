//! Power and clock readings on Apple silicon, no admin rights: energy per
//! part (CPU, GPU, Neural Engine) and how long each core cluster and the
//! GPU spent in each performance state, from the IOReport framework; the
//! clock speed of each state from the power manager (`pmgr`) in the I/O
//! registry. The arithmetic is plain Rust and tested; the macOS part only
//! fetches the numbers.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// One IOReport channel's change over the sampling interval.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub group: String,
    pub subgroup: String,
    pub channel: String,
    pub unit: String,
    /// Simple channels (energy).
    pub value: i64,
    /// State channels: (state name, time spent in it).
    pub states: Vec<(String, i64)>,
}

/// Clock speed of each performance state, MHz, lowest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Freqs {
    pub ecpu: Vec<f32>,
    pub pcpu: Vec<f32>,
    pub gpu: Vec<f32>,
}

/// How busy one part was and at what average clock while busy.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// 0…1 of the interval.
    pub busy: f32,
    pub mhz: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    pub cpu_w: f32,
    pub gpu_w: f32,
    pub ane_w: f32,
    pub ecpu: Usage,
    pub pcpu: Usage,
    pub gpu: Usage,
}

/// Joules per unit of an energy channel.
fn joules(unit: &str) -> Option<f64> {
    match unit.trim() {
        "J" => Some(1.0),
        "mJ" => Some(1e-3),
        "uJ" | "µJ" => Some(1e-6),
        "nJ" => Some(1e-9),
        _ => None,
    }
}

/// States that mean "not running".
fn idle_state(name: &str) -> bool {
    matches!(name.trim(), "IDLE" | "DOWN" | "OFF")
}

/// Busy share and the average clock of the running states, given each
/// running state's clock in order. Several channels (one per core) are
/// averaged.
fn usage<'a>(channels: impl Iterator<Item = &'a Sample>, freqs: &[f32]) -> Usage {
    let (mut busy_sum, mut mhz_sum, mut n, mut n_mhz) = (0f64, 0f64, 0u32, 0u32);
    for c in channels {
        let total: i64 = c.states.iter().map(|s| s.1.max(0)).sum();
        if total <= 0 {
            continue;
        }
        let active: Vec<i64> = c.states.iter().filter(|s| !idle_state(&s.0)).map(|s| s.1.max(0)).collect();
        let on: i64 = active.iter().sum();
        busy_sum += on as f64 / total as f64;
        n += 1;
        if on > 0 && !freqs.is_empty() {
            // more states than known clocks: the extra ones count at the top clock
            let mhz: f64 = active.iter().enumerate().map(|(i, t)| *t as f64 * freqs[i.min(freqs.len() - 1)] as f64).sum::<f64>() / on as f64;
            mhz_sum += mhz;
            n_mhz += 1;
        }
    }
    Usage { busy: if n > 0 { (busy_sum / n as f64) as f32 } else { 0.0 }, mhz: if n_mhz > 0 { (mhz_sum / n_mhz as f64) as f32 } else { 0.0 } }
}

/// Turn one interval's samples into watts and usage.
pub fn summarize(samples: &[Sample], secs: f64, f: &Freqs) -> Reading {
    let mut r = Reading::default();
    if secs <= 0.0 {
        return r;
    }
    for s in samples.iter().filter(|s| s.group == "Energy Model") {
        let Some(j) = joules(&s.unit) else { continue };
        let w = (s.value.max(0) as f64 * j / secs) as f32;
        let name = s.channel.as_str();
        if name.ends_with("CPU Energy") {
            r.cpu_w += w;
        } else if name.starts_with("GPU") {
            r.gpu_w += w;
        } else if name.starts_with("ANE") {
            r.ane_w += w;
        }
    }
    let cores = |prefix: &'static str| samples.iter().filter(move |s| s.group == "CPU Stats" && s.subgroup == "CPU Core Performance States" && s.channel.starts_with(prefix));
    r.ecpu = usage(cores("ECPU"), &f.ecpu);
    r.pcpu = usage(cores("PCPU"), &f.pcpu);
    let gpu = samples.iter().filter(|s| s.group == "GPU Stats" && s.subgroup == "GPU Performance States" && s.channel == "GPUPH");
    r.gpu = usage(gpu, &f.gpu);
    r
}

/// The clocks in a `pmgr` `voltage-states…` property: pairs of little-endian
/// u32 (clock, voltage). Older chips give Hz, M4 and later kHz; zero
/// entries are padding.
pub fn parse_freqs(b: &[u8]) -> Vec<f32> {
    let raw: Vec<u32> = b.chunks_exact(8).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).filter(|v| *v > 0).collect();
    let hz = raw.iter().any(|v| *v >= 100_000_000);
    raw.into_iter().map(|v| if hz { v as f32 / 1e6 } else { v as f32 / 1e3 }).collect()
}

#[cfg(target_os = "macos")]
pub use mac::Sampler;

#[cfg(target_os = "macos")]
mod mac {
    use super::{parse_freqs, summarize, Freqs, Reading, Sample};
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::time::Instant;

    type Cf = *const c_void;

    #[link(name = "IOReport", kind = "dylib")]
    extern "C" {
        fn IOReportCopyChannelsInGroup(group: Cf, subgroup: Cf, a: u64, b: u64, c: u64) -> Cf;
        fn IOReportMergeChannels(a: Cf, b: Cf, nil: Cf);
        fn IOReportCreateSubscription(a: Cf, desired: Cf, subbed: *mut Cf, channel_id: u64, b: Cf) -> Cf;
        fn IOReportCreateSamples(sub: Cf, subbed: Cf, nil: Cf) -> Cf;
        fn IOReportCreateSamplesDelta(prev: Cf, cur: Cf, nil: Cf) -> Cf;
        fn IOReportChannelGetGroup(item: Cf) -> Cf;
        fn IOReportChannelGetSubGroup(item: Cf) -> Cf;
        fn IOReportChannelGetChannelName(item: Cf) -> Cf;
        fn IOReportChannelGetUnitLabel(item: Cf) -> Cf;
        fn IOReportSimpleGetIntegerValue(item: Cf, idx: i32) -> i64;
        fn IOReportStateGetCount(item: Cf) -> i32;
        fn IOReportStateGetNameForIndex(item: Cf, idx: i32) -> Cf;
        fn IOReportStateGetResidency(item: Cf, idx: i32) -> i64;
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(main_port: u32, matching: *mut c_void, iter: *mut u32) -> i32;
        fn IOIteratorNext(iter: u32) -> u32;
        fn IOObjectRelease(obj: u32) -> i32;
        fn IORegistryEntryGetName(entry: u32, name: *mut c_char) -> i32;
        fn IORegistryEntryCreateCFProperty(entry: u32, key: Cf, alloc: Cf, options: u32) -> Cf;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: Cf, s: *const c_char, encoding: u32) -> Cf;
        fn CFStringGetCString(s: Cf, buf: *mut c_char, len: isize, encoding: u32) -> u8;
        fn CFDictionaryGetCount(d: Cf) -> isize;
        fn CFDictionaryCreateMutableCopy(alloc: Cf, cap: isize, d: Cf) -> Cf;
        fn CFDictionaryGetValue(d: Cf, key: Cf) -> Cf;
        fn CFArrayGetCount(a: Cf) -> isize;
        fn CFArrayGetValueAtIndex(a: Cf, i: isize) -> Cf;
        fn CFDataGetLength(d: Cf) -> isize;
        fn CFDataGetBytePtr(d: Cf) -> *const u8;
        fn CFGetTypeID(cf: Cf) -> usize;
        fn CFDataGetTypeID() -> usize;
        fn CFRelease(cf: Cf);
    }

    const UTF8: u32 = 0x0800_0100;

    fn cfstr(s: &str) -> Cf {
        let c = CString::new(s).unwrap_or_default();
        unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
    }

    /// A CFString we don't own, as a Rust string ("" for null).
    fn text(cf: Cf) -> String {
        if cf.is_null() {
            return String::new();
        }
        let mut buf = [0 as c_char; 256];
        if unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) } == 0 {
            return String::new();
        }
        unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
    }

    /// The clock tables from `pmgr`.
    fn freqs() -> Freqs {
        let mut f = Freqs::default();
        unsafe {
            let name = CString::new("AppleARMIODevice").unwrap();
            let mut iter = 0u32;
            if IOServiceGetMatchingServices(0, IOServiceMatching(name.as_ptr()), &mut iter) != 0 {
                return f;
            }
            loop {
                let e = IOIteratorNext(iter);
                if e == 0 {
                    break;
                }
                let mut buf = [0 as c_char; 128];
                let is_pmgr = IORegistryEntryGetName(e, buf.as_mut_ptr()) == 0 && CStr::from_ptr(buf.as_ptr()).to_bytes() == b"pmgr";
                if is_pmgr {
                    let prop = |key: &str| {
                        let k = cfstr(key);
                        let d = IORegistryEntryCreateCFProperty(e, k, std::ptr::null(), 0);
                        CFRelease(k);
                        if d.is_null() {
                            return Vec::new();
                        }
                        let out = if CFGetTypeID(d) == CFDataGetTypeID() {
                            let n = CFDataGetLength(d).max(0) as usize;
                            let p = CFDataGetBytePtr(d);
                            if p.is_null() { Vec::new() } else { parse_freqs(std::slice::from_raw_parts(p, n)) }
                        } else {
                            Vec::new()
                        };
                        CFRelease(d);
                        out
                    };
                    f.ecpu = prop("voltage-states1-sram");
                    f.pcpu = prop("voltage-states5-sram");
                    f.gpu = prop("voltage-states9");
                }
                IOObjectRelease(e);
                if is_pmgr {
                    break;
                }
            }
            IOObjectRelease(iter);
        }
        f
    }

    /// An IOReport subscription to the energy, CPU and GPU channels. Each
    /// [`Sampler::read`] covers the time since the one before.
    pub struct Sampler {
        sub: Cf,
        chans: Cf,
        prev: Cf,
        at: Instant,
        freqs: Freqs,
    }

    // The CF objects are only used from the thread that owns the sampler.
    unsafe impl Send for Sampler {}

    impl Sampler {
        pub fn open() -> Option<Sampler> {
            unsafe {
                let groups = [("Energy Model", None), ("CPU Stats", Some("CPU Core Performance States")), ("GPU Stats", Some("GPU Performance States"))];
                let mut merged: Cf = std::ptr::null();
                for (g, sg) in groups {
                    let gs = cfstr(g);
                    let ss = sg.map(cfstr).unwrap_or(std::ptr::null());
                    let c = IOReportCopyChannelsInGroup(gs, ss, 0, 0, 0);
                    CFRelease(gs);
                    if !ss.is_null() {
                        CFRelease(ss);
                    }
                    if c.is_null() {
                        continue;
                    }
                    if merged.is_null() {
                        merged = c;
                    } else {
                        IOReportMergeChannels(merged, c, std::ptr::null());
                        CFRelease(c);
                    }
                }
                if merged.is_null() {
                    return None;
                }
                let chans = CFDictionaryCreateMutableCopy(std::ptr::null(), CFDictionaryGetCount(merged), merged);
                CFRelease(merged);
                if chans.is_null() {
                    return None;
                }
                let mut subbed: Cf = std::ptr::null();
                let sub = IOReportCreateSubscription(std::ptr::null(), chans, &mut subbed, 0, std::ptr::null());
                if sub.is_null() || subbed.is_null() {
                    CFRelease(chans);
                    return None;
                }
                CFRelease(chans);
                let prev = IOReportCreateSamples(sub, subbed, std::ptr::null());
                if prev.is_null() {
                    CFRelease(subbed);
                    CFRelease(sub);
                    return None;
                }
                Some(Sampler { sub, chans: subbed, prev, at: Instant::now(), freqs: freqs() })
            }
        }

        /// The interval since the last call (or since [`Sampler::open`]).
        pub fn read(&mut self) -> Option<Reading> {
            unsafe {
                let cur = IOReportCreateSamples(self.sub, self.chans, std::ptr::null());
                if cur.is_null() {
                    return None;
                }
                let secs = self.at.elapsed().as_secs_f64();
                self.at = Instant::now();
                let delta = IOReportCreateSamplesDelta(self.prev, cur, std::ptr::null());
                CFRelease(self.prev);
                self.prev = cur;
                if delta.is_null() {
                    return None;
                }
                let key = cfstr("IOReportChannels");
                let arr = CFDictionaryGetValue(delta, key);
                CFRelease(key);
                let mut samples = Vec::new();
                if !arr.is_null() {
                    for i in 0..CFArrayGetCount(arr) {
                        let item = CFArrayGetValueAtIndex(arr, i);
                        if item.is_null() {
                            continue;
                        }
                        let group = text(IOReportChannelGetGroup(item));
                        let mut s = Sample { subgroup: text(IOReportChannelGetSubGroup(item)), channel: text(IOReportChannelGetChannelName(item)), unit: text(IOReportChannelGetUnitLabel(item)), ..Default::default() };
                        if group == "Energy Model" {
                            s.value = IOReportSimpleGetIntegerValue(item, 0);
                        } else {
                            for k in 0..IOReportStateGetCount(item) {
                                s.states.push((text(IOReportStateGetNameForIndex(item, k)), IOReportStateGetResidency(item, k)));
                            }
                        }
                        s.group = group;
                        samples.push(s);
                    }
                }
                CFRelease(delta);
                Some(summarize(&samples, secs, &self.freqs))
            }
        }
    }

    impl Drop for Sampler {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.prev);
                CFRelease(self.chans);
                CFRelease(self.sub);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(name: &str, unit: &str, v: i64) -> Sample {
        Sample { group: "Energy Model".into(), channel: name.into(), unit: unit.into(), value: v, ..Default::default() }
    }

    fn core(name: &str, states: &[(&str, i64)]) -> Sample {
        Sample { group: "CPU Stats".into(), subgroup: "CPU Core Performance States".into(), channel: name.into(), states: states.iter().map(|(n, t)| (n.to_string(), *t)).collect(), ..Default::default() }
    }

    #[test]
    fn energy_becomes_watts() {
        let s = [energy("CPU Energy", "mJ", 4000), energy("GPU Energy", "nJ", 1_000_000_000), energy("ANE0", "mJ", 200), energy("DRAM", "mJ", 999), energy("odd", "kWh", 5)];
        let r = summarize(&s, 2.0, &Freqs::default());
        assert_eq!(r.cpu_w, 2.0);
        assert_eq!(r.gpu_w, 0.5);
        assert_eq!(r.ane_w, 0.1);
        assert_eq!(summarize(&s, 0.0, &Freqs::default()), Reading::default());
    }

    #[test]
    fn cluster_usage_and_clock() {
        let f = Freqs { ecpu: vec![600.0, 1000.0], pcpu: vec![1000.0, 3000.0], gpu: vec![] };
        let s = [
            // half busy: 1/4 at 600, 1/4 at 1000 -> 800 MHz
            core("ECPU000", &[("IDLE", 50), ("V0P5", 25), ("V1P4", 25)]),
            // idle
            core("ECPU001", &[("IDLE", 100), ("V0P5", 0), ("V1P4", 0)]),
            // fully at the top clock, with an extra state past the table
            core("PCPU000", &[("DOWN", 0), ("V0", 0), ("V1", 50), ("V2", 50)]),
        ];
        let r = summarize(&s, 1.0, &f);
        assert_eq!(r.ecpu.busy, 0.25);
        assert_eq!(r.ecpu.mhz, 800.0);
        assert_eq!(r.pcpu.busy, 1.0);
        assert_eq!(r.pcpu.mhz, 3000.0);
        // no samples
        assert_eq!(r.gpu, Usage::default());
    }

    #[test]
    fn gpu_states() {
        let f = Freqs { gpu: vec![400.0, 1400.0], ..Default::default() };
        let g = Sample { group: "GPU Stats".into(), subgroup: "GPU Performance States".into(), channel: "GPUPH".into(), states: vec![("OFF".into(), 80), ("P1".into(), 10), ("P2".into(), 10)], ..Default::default() };
        let r = summarize(&[g], 1.0, &f);
        assert!((r.gpu.busy - 0.2).abs() < 1e-6);
        assert_eq!(r.gpu.mhz, 900.0);
    }

    #[test]
    fn freq_tables_in_hz_or_khz() {
        let pairs = |v: &[u32]| v.iter().flat_map(|f| [f.to_le_bytes(), 800_000u32.to_le_bytes()].concat()).collect::<Vec<u8>>();
        assert_eq!(parse_freqs(&pairs(&[0, 600_000_000, 2_064_000_000])), vec![600.0, 2064.0]);
        assert_eq!(parse_freqs(&pairs(&[1_260_000, 4_512_000])), vec![1260.0, 4512.0]);
        assert_eq!(parse_freqs(&[1, 2, 3]), Vec::<f32>::new());
    }
}
