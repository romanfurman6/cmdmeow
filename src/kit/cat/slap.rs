//! Slaps on the Mac's body, felt through the accelerometer of Apple silicon
//! laptops. `Detector` decides from the readings, as plain rules; `start`
//! and `stop` run the sensor on a thread of its own.

#[cfg(target_os = "macos")]
pub use sensor::{start, stop, woke};

/// A slap: the body jolts by this much, in g, from one reading to the next…
pub const SLAP_G: f64 = 0.16;
/// …out of stillness: no jolt from this big in the moment before, in g and
/// seconds. A slap on a resting Mac comes out of nothing; a lid swing or a
/// Mac being carried builds up first.
pub const QUIET_G: f64 = 0.04;
pub const QUIET_FOR: f64 = 0.3;
/// Jolts this close together are one slap, in seconds.
pub const SLAP_GAP: f64 = 1.0;
/// Nothing counts this soon after the sensor starts, in seconds.
pub const SETTLE: f64 = 1.0;
/// Nor this soon after the lid moved (the hinge thumps), in seconds.
pub const LID_SETTLE: f64 = 1.5;
/// The sensor reports this often, in microseconds (100 a second: a slap
/// lasts a few hundredths of a second).
const EVERY_US: i32 = 10_000;
/// The sensor's first reading must come this soon, in seconds.
const FIRST_WITHIN: f64 = 2.0;
/// With the log on: jolts from this big, at most this many lines a second,
/// and the largest jolt that was no slap this often, in seconds.
const LOG_G: f64 = 0.02;
const LOG_LINES: f64 = 5.0;
const LOG_LARGEST_EVERY: f64 = 10.0;

/// The accelerometer: a vendor HID device (usage page and usage) that a
/// normal process may read. The keyboard's own controller shares the usage,
/// so the transport tells them apart.
const PAGE: i32 = 0xFF00;
const USAGE: i32 = 3;
const TRANSPORT: &str = "SPU";

/// Decides which jolts are slaps.
pub struct Detector {
    started: f64,
    last: Option<[f64; 3]>,
    /// The last slap, the last jolt that broke the stillness, the last time
    /// the lid moved, and whether it is closed.
    slapped: f64,
    stirred: f64,
    lid_moved: f64,
    lid_closed: bool,
    /// The largest jolt that was no slap, since it was last asked for.
    largest: f64,
}

impl Detector {
    pub fn new(now: f64, lid_closed: bool) -> Detector {
        Detector { started: now, last: None, slapped: f64::MIN, stirred: f64::MIN, lid_moved: f64::MIN, lid_closed, largest: 0.0 }
    }

    /// The lid is `closed` at `now`.
    pub fn lid(&mut self, now: f64, closed: bool) {
        if closed != self.lid_closed {
            self.lid_closed = closed;
            self.lid_moved = now;
        }
    }

    /// A reading `a` (x, y, z in g) at `now`: its jolt, and whether it is
    /// a slap.
    pub fn reading(&mut self, now: f64, a: [f64; 3]) -> (f64, bool) {
        let Some(last) = self.last.replace(a) else { return (0.0, false) };
        let jolt = ((a[0] - last[0]).powi(2) + (a[1] - last[1]).powi(2) + (a[2] - last[2]).powi(2)).sqrt();
        let still = now - self.stirred >= QUIET_FOR;
        let calm = now - self.started >= SETTLE && now - self.lid_moved >= LID_SETTLE && now - self.slapped >= SLAP_GAP;
        if jolt >= QUIET_G {
            self.stirred = now;
        }
        let slap = jolt >= SLAP_G && still && calm && !self.lid_closed;
        if slap {
            self.slapped = now;
        } else if jolt < SLAP_G {
            self.largest = self.largest.max(jolt);
        }
        (jolt, slap)
    }

    pub fn take_largest(&mut self) -> f64 {
        std::mem::take(&mut self.largest)
    }
}

// ------------------------------------------------------------- the sensor

#[cfg(target_os = "macos")]
mod sensor {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFRunLoopGetCurrent() -> *const c_void;
        fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: bool) -> i32;
        fn CFRunLoopStop(run_loop: *const c_void);
        fn CFSetGetCount(set: *const c_void) -> isize;
        fn CFSetGetValues(set: *const c_void, values: *mut *const c_void);
        static kCFRunLoopDefaultMode: *const c_void;
    }

    type ReportCallback = extern "C" fn(context: *mut c_void, result: i32, sender: *mut c_void, kind: u32, id: u32, report: *const u8, len: isize);

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(main_port: u32, matching: *mut c_void, found: *mut u32) -> i32;
        fn IOIteratorNext(iterator: u32) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IORegistryEntryCreateCFProperty(entry: u32, key: *const c_void, alloc: *const c_void, options: u32) -> *const c_void;
        fn IORegistryEntrySetCFProperty(entry: u32, key: *const c_void, value: *const c_void) -> i32;
        fn IOHIDManagerCreate(alloc: *const c_void, options: u32) -> *mut c_void;
        fn IOHIDManagerSetDeviceMatching(manager: *mut c_void, matching: *const c_void);
        fn IOHIDManagerScheduleWithRunLoop(manager: *mut c_void, run_loop: *const c_void, mode: *const c_void);
        fn IOHIDManagerOpen(manager: *mut c_void, options: u32) -> i32;
        fn IOHIDManagerClose(manager: *mut c_void, options: u32) -> i32;
        fn IOHIDManagerCopyDevices(manager: *mut c_void) -> *const c_void;
        fn IOHIDDeviceRegisterInputReportCallback(device: *const c_void, report: *mut u8, len: isize, callback: ReportCallback, context: *mut c_void);
    }

    /// Whether the body should be felt now.
    static ON: AtomicBool = AtomicBool::new(false);
    /// The sensor thread's run loop, while it runs. Held while the sensor
    /// is switched on or off too, so a stop and the next start never cross.
    static RUNNING: Mutex<Option<usize>> = Mutex::new(None);

    fn running() -> std::sync::MutexGuard<'static, Option<usize>> {
        RUNNING.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the sensor thread keeps between readings.
    struct Felt {
        detector: Detector,
        t0: Instant,
        lid_closed: fn() -> bool,
        slapped: fn(),
        log: bool,
        readings: u64,
        logged: f64,
        told_largest: f64,
    }

    /// Feel the body for slaps: `slapped` is called (on the sensor's thread)
    /// for each one, `lid_closed` says whether the lid is closed. The readings
    /// come a hundred times a second, so they go to a thread of their own and
    /// never wake the main one.
    pub fn start(lid_closed: fn() -> bool, slapped: fn(), log: bool) {
        ON.store(true, Ordering::SeqCst);
        let _ = std::thread::Builder::new().name("cat-slap".into()).spawn(move || unsafe { run(lid_closed, slapped, log) });
    }

    /// Stop feeling: the thread closes the device and turns the sensor off.
    pub fn stop() {
        ON.store(false, Ordering::SeqCst);
        if let Some(run_loop) = *running() {
            unsafe { CFRunLoopStop(run_loop as *const c_void) };
        }
    }

    /// The Mac woke: the sensor may have lost its pace over the sleep.
    pub fn woke() {
        let running = running();
        if running.is_some() && ON.load(Ordering::SeqCst) {
            unsafe { report_every(EVERY_US) };
        }
    }

    unsafe fn run(lid_closed: fn() -> bool, slapped: fn(), log: bool) {
        let manager = IOHIDManagerCreate(std::ptr::null(), 0);
        if manager.is_null() {
            return;
        }
        let matching: Retained<AnyObject> = msg_send![class!(NSMutableDictionary), dictionary];
        for (key, value) in [("PrimaryUsagePage", PAGE), ("PrimaryUsage", USAGE)] {
            let value: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithInt: value];
            let _: () = msg_send![&*matching, setObject: &*value, forKey: &*NSString::from_str(key)];
        }
        let _: () = msg_send![&*matching, setObject: &*NSString::from_str(TRANSPORT), forKey: &*NSString::from_str("Transport")];
        IOHIDManagerSetDeviceMatching(manager, Retained::as_ptr(&matching).cast());
        let run_loop = CFRunLoopGetCurrent();
        IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopDefaultMode);
        let opened = IOHIDManagerOpen(manager, 0);
        let devices = if opened == 0 { IOHIDManagerCopyDevices(manager) } else { std::ptr::null() };
        let count = if devices.is_null() { 0 } else { CFSetGetCount(devices) };
        let mut felt = Box::new(Felt { detector: Detector::new(0.0, lid_closed()), t0: Instant::now(), lid_closed, slapped, log, readings: 0, logged: f64::MIN, told_largest: 0.0 });
        // a buffer for each device's reports, kept until it is closed
        let mut buffers = Vec::new();
        if count > 0 {
            let mut list = vec![std::ptr::null(); count as usize];
            CFSetGetValues(devices, list.as_mut_ptr());
            for device in list {
                let mut buffer = vec![0u8; 64].into_boxed_slice();
                IOHIDDeviceRegisterInputReportCallback(device, buffer.as_mut_ptr(), buffer.len() as isize, reading, (&mut *felt as *mut Felt).cast());
                buffers.push(buffer);
            }
        }
        if !devices.is_null() {
            CFRelease(devices);
        }
        if count > 0 {
            let me = run_loop as usize;
            {
                let mut running = running();
                report_every(EVERY_US);
                *running = Some(me);
            }
            // still wanted, and no newer thread took over
            let mine = || ON.load(Ordering::SeqCst) && *running() == Some(me);
            // 2: stopped; anything else is the first wait running out
            let first = if mine() { CFRunLoopRunInMode(kCFRunLoopDefaultMode, FIRST_WITHIN, false) } else { 2 };
            let silent = first != 2 && felt.readings == 0;
            if silent {
                if log {
                    eprintln!("[cat] slaps: no reading from the accelerometer in {FIRST_WITHIN} s");
                }
            } else if first != 2 {
                if log {
                    eprintln!("[cat] slaps: feeling the body ({count} accelerometer)");
                }
                while mine() && CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1e10, false) != 2 {}
            }
            let mut running = running();
            if running.is_none_or(|r| r == me) {
                *running = None;
                // left alone, the sensor goes on reporting to no one (a
                // start right after the stop sets its own pace)
                if silent || !ON.load(Ordering::SeqCst) {
                    report_every(0);
                }
            }
        } else if log {
            eprintln!("[cat] slaps: no accelerometer (open {opened:#x})");
        }
        IOHIDManagerClose(manager, 0);
        CFRelease(manager);
        drop(buffers);
        drop(felt);
    }

    /// Tell the accelerometer's driver how often to report, in microseconds
    /// (0: not at all). It reports nothing until told.
    unsafe fn report_every(us: i32) {
        let mut found = 0u32;
        if IOServiceGetMatchingServices(0, IOServiceMatching(c"AppleSPUHIDDriver".as_ptr()), &mut found) != 0 {
            return;
        }
        let number = |entry: u32, key: &str| -> Option<i32> {
            let v = IORegistryEntryCreateCFProperty(entry, Retained::as_ptr(&NSString::from_str(key)).cast(), std::ptr::null(), 0);
            let object = (v as *const AnyObject).as_ref()?;
            let n: i32 = msg_send![object, intValue];
            CFRelease(v);
            Some(n)
        };
        loop {
            let entry = IOIteratorNext(found);
            if entry == 0 {
                break;
            }
            if number(entry, "PrimaryUsagePage") == Some(PAGE) && number(entry, "PrimaryUsage") == Some(USAGE) {
                let on = i32::from(us > 0);
                for (key, value) in [("SensorPropertyReportingState", on), ("SensorPropertyPowerState", on), ("ReportInterval", us)] {
                    let value: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithInt: value];
                    IORegistryEntrySetCFProperty(entry, Retained::as_ptr(&NSString::from_str(key)).cast(), Retained::as_ptr(&value).cast());
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(found);
    }

    /// Each reading, on the sensor's thread: x, y and z from byte 6, each a
    /// little-endian 32-bit count of 1/65536 g.
    extern "C" fn reading(context: *mut c_void, _result: i32, _sender: *mut c_void, _kind: u32, _id: u32, report: *const u8, len: isize) {
        let Some(felt) = (unsafe { context.cast::<Felt>().as_mut() }) else { return };
        if report.is_null() || len < 18 {
            return;
        }
        let bytes = unsafe { std::slice::from_raw_parts(report, len as usize) };
        let g = |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as f64 / 65536.0;
        let now = felt.t0.elapsed().as_secs_f64();
        felt.readings += 1;
        felt.detector.lid(now, (felt.lid_closed)());
        let (jolt, slap) = felt.detector.reading(now, [g(6), g(10), g(14)]);
        if slap {
            if felt.log {
                eprintln!("[cat] slap: {jolt:.3} g");
            }
            (felt.slapped)();
        }
        if felt.log {
            if jolt >= LOG_G && now - felt.logged >= 1.0 / LOG_LINES {
                felt.logged = now;
                eprintln!("[cat] jolt {jolt:.3} g");
            }
            if now - felt.told_largest >= LOG_LARGEST_EVERY {
                felt.told_largest = now;
                eprintln!("[cat] largest jolt that was no slap, last {LOG_LARGEST_EVERY} s: {:.3} g", felt.detector.take_largest());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Readings at 100 a second from `t`, each jolted from the one before
    /// by the next of `jolts` (g): when slaps were felt.
    fn feel(d: &mut Detector, t: f64, jolts: &[f64]) -> Vec<f64> {
        let mut a = [0.005, 0.011, -0.99];
        let mut felt = Vec::new();
        for (i, j) in jolts.iter().enumerate() {
            let now = t + i as f64 * 0.01;
            a[1] += if i % 2 == 0 { *j } else { -*j };
            if d.reading(now, a).1 {
                felt.push(now);
            }
        }
        felt
    }

    /// `secs` of a resting Mac, with these jolts at these seconds in.
    fn rest(secs: f64, at: &[(f64, f64)]) -> Vec<f64> {
        (0..(secs * 100.0) as usize).map(|i| at.iter().find(|(t, _)| (i as f64 * 0.01 - t).abs() < 0.005).map_or(0.001, |(_, j)| *j)).collect()
    }

    #[test]
    fn a_slap_out_of_stillness_is_felt_at_once() {
        let mut d = Detector::new(0.0, false);
        let felt = feel(&mut d, 0.0, &rest(3.0, &[(2.0, 0.3)]));
        assert_eq!(felt.len(), 1);
        assert!((felt[0] - 2.0).abs() < 0.006);
    }

    #[test]
    fn small_jolts_and_the_first_second_are_no_slaps() {
        let mut d = Detector::new(0.0, false);
        assert!(feel(&mut d, 0.0, &rest(3.0, &[(1.5, 0.05), (2.0, 0.08)])).is_empty());
        assert!((d.take_largest() - 0.08).abs() < 1e-6);
        let mut d = Detector::new(0.0, false);
        assert!(feel(&mut d, 0.0, &rest(1.5, &[(0.5, 0.5)])).is_empty(), "settling");
    }

    #[test]
    fn one_slap_rings_once() {
        let mut d = Detector::new(0.0, false);
        // the body rings for a moment after a slap, and a slap one second on is new
        let felt = feel(&mut d, 0.0, &rest(5.0, &[(2.0, 0.3), (2.05, 0.2), (2.4, 0.4), (2.8, 0.3), (4.2, 0.3)]));
        assert_eq!(felt.len(), 2);
        assert!((felt[1] - 4.2).abs() < 0.006);
    }

    #[test]
    fn a_swing_that_builds_up_is_no_slap() {
        // the lid or a Mac being carried: the jolts grow over a few tenths
        let mut jolts = rest(2.0, &[]);
        jolts.extend((0..40).map(|i| 0.01 + i as f64 * 0.012));
        jolts.extend(rest(1.0, &[]));
        let mut d = Detector::new(0.0, false);
        assert!(feel(&mut d, 0.0, &jolts).is_empty());
    }

    #[test]
    fn the_lid_is_no_slap() {
        let mut d = Detector::new(0.0, true);
        assert!(feel(&mut d, 0.0, &rest(3.0, &[(2.0, 1.0)])).is_empty(), "closed");
        // opening: the hinge thumps after the Mac knows
        d.lid(3.0, false);
        assert!(feel(&mut d, 3.0, &rest(1.4, &[(0.5, 1.0), (1.2, 1.0)])).is_empty());
        assert_eq!(feel(&mut d, 4.6, &rest(1.0, &[(0.5, 1.0)])).len(), 1);
    }
}
