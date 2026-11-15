//! Bluetooth beyond the system report, read in-process:
//! - AirPods adverts (case open, pods out) and the Bluetooth LE battery
//!   service of connected devices (iPhone, iPad, other makers' mice and
//!   keyboards), through CoreBluetooth;
//! - which devices are connected right now, through IOBluetooth, so a
//!   device that connects shows up without waiting for the next report;
//! - game controllers' batteries, through GameController.
//!
//! CoreBluetooth and IOBluetooth need the Bluetooth permission. Nothing
//! here asks for it until [`start`] runs (the first time the panel opens),
//! and nothing runs without the bundle's usage text (a bare `cargo run`
//! would be killed by macOS for asking).

use super::devices::{Extra, PodsAdvert};

/// Start listening (asks for the Bluetooth permission the first time).
/// Safe to call often.
pub fn start() {
    #[cfg(target_os = "macos")]
    mac::start();
}

/// Call every monitor round: runs a short scan now and then.
pub fn tick() {
    #[cfg(target_os = "macos")]
    mac::tick();
}

/// End a scan under way (Mac Health was turned off).
pub fn stop() {
    #[cfg(target_os = "macos")]
    mac::stop();
}

/// AirPods adverts heard in the last minute, newest per model.
pub fn adverts() -> Vec<PodsAdvert> {
    #[cfg(target_os = "macos")]
    return mac::adverts();
    #[cfg(not(target_os = "macos"))]
    vec![]
}

/// Batteries read from the Bluetooth LE battery service and from game
/// controllers.
pub fn extra() -> Vec<Extra> {
    #[cfg(target_os = "macos")]
    {
        let mut v = mac::gatt();
        v.extend(mac::controllers());
        v.extend(mac::classic());
        v
    }
    #[cfg(not(target_os = "macos"))]
    vec![]
}

/// Addresses of the connected Bluetooth devices, sorted; None when it
/// can't be read (no permission yet).
pub fn connected() -> Option<Vec<String>> {
    #[cfg(target_os = "macos")]
    return mac::connected();
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use super::super::devices::{parse_proximity, Extra, PodsAdvert};
    use crate::kit::sync::LockExt;
    use dispatch2::{DispatchQueue, DispatchRetained};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, define_class, msg_send, AnyThread};
    use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    #[link(name = "CoreBluetooth", kind = "framework")]
    extern "C" {}
    #[link(name = "IOBluetooth", kind = "framework")]
    extern "C" {}
    #[link(name = "GameController", kind = "framework")]
    extern "C" {}

    /// A scan this long…
    const SCAN_FOR: Duration = Duration::from_secs(8);
    /// …this often.
    const SCAN_EVERY: Duration = Duration::from_secs(60);
    /// Adverts older than this are forgotten.
    const ADVERT_TTL: Duration = Duration::from_secs(90);
    /// Battery service readings older than this are forgotten.
    const GATT_TTL: Duration = Duration::from_secs(10 * 60);
    /// CBManagerStatePoweredOn.
    const POWERED_ON: isize = 5;
    /// CBManagerAuthorizationAllowedAlways.
    const ALLOWED: isize = 3;

    const BATTERY_SERVICE: &str = "180F";
    const BATTERY_LEVEL: &str = "2A19";

    #[derive(Default)]
    struct State {
        adverts: HashMap<u16, (Instant, PodsAdvert)>,
        /// peripheral id -> (name, percent, when)
        gatt: HashMap<String, (String, u8, Instant)>,
        scanning_since: Option<Instant>,
        last_scan: Option<Instant>,
    }

    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    static POWER: AtomicIsize = AtomicIsize::new(0);

    fn state() -> &'static Mutex<State> {
        STATE.get_or_init(Default::default)
    }

    /// Objective-C objects that live for the whole run, used from the
    /// Bluetooth queue only.
    struct Keep<T: ?Sized>(Retained<T>);
    unsafe impl<T: ?Sized> Send for Keep<T> {}
    unsafe impl<T: ?Sized> Sync for Keep<T> {}
    type Shared = Keep<AnyObject>;

    struct Core {
        queue: DispatchRetained<DispatchQueue>,
        manager: Shared,
        delegate: Keep<Delegate>,
        /// Peripherals we connected to read their battery: CoreBluetooth
        /// drops the connection when the object goes away.
        held: Mutex<Vec<Shared>>,
    }
    static CORE: OnceLock<Core> = OnceLock::new();

    fn ns(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    /// The app's bundle explains why it uses Bluetooth (macOS kills an app
    /// that asks without it).
    fn has_usage_text() -> bool {
        let v: Option<Retained<AnyObject>> = unsafe { msg_send![&*NSBundle::mainBundle(), objectForInfoDictionaryKey: &*ns("NSBluetoothAlwaysUsageDescription")] };
        v.is_some()
    }

    /// CBManager.authorization: 0 not asked, 1 restricted, 2 denied, 3 allowed.
    fn authorization() -> isize {
        unsafe { msg_send![class!(CBManager), authorization] }
    }

    fn uuid(s: &str) -> Retained<AnyObject> {
        unsafe { msg_send![class!(CBUUID), UUIDWithString: &*ns(s)] }
    }

    fn is_uuid(obj: &AnyObject, s: &str) -> bool {
        let u: Retained<AnyObject> = unsafe { msg_send![obj, UUID] };
        let want = uuid(s);
        let eq: Bool = unsafe { msg_send![&*u, isEqual: &*want] };
        eq.as_bool()
    }

    fn string(obj: Option<Retained<NSString>>) -> Option<String> {
        obj.map(|s| s.to_string()).filter(|s| !s.trim().is_empty())
    }

    fn peripheral_id(p: &AnyObject) -> String {
        let id: Option<Retained<AnyObject>> = unsafe { msg_send![p, identifier] };
        id.and_then(|id| string(unsafe { msg_send![&*id, UUIDString] })).unwrap_or_default()
    }

    fn bytes_of(data: &AnyObject) -> Vec<u8> {
        unsafe {
            let len: usize = msg_send![data, length];
            let ptr: *const u8 = msg_send![data, bytes];
            if ptr.is_null() || len == 0 {
                return vec![];
            }
            std::slice::from_raw_parts(ptr, len).to_vec()
        }
    }

    define_class!(
        // Bluetooth queue only: central and peripheral delegate in one.
        #[unsafe(super(NSObject))]
        #[name = "CmdMeowBleDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        impl Delegate {
            #[unsafe(method(centralManagerDidUpdateState:))]
            fn did_update_state(&self, central: &AnyObject) {
                let st: isize = unsafe { msg_send![central, state] };
                POWER.store(st, Ordering::Relaxed);
                if st == POWERED_ON {
                    // a first scan right away
                    begin_scan(central);
                }
            }

            #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
            fn did_discover(&self, _central: &AnyObject, _p: &AnyObject, adv: &AnyObject, rssi: &AnyObject) {
                let data: Option<Retained<AnyObject>> = unsafe { msg_send![adv, objectForKey: &*ns("kCBAdvDataManufacturerData")] };
                let Some(data) = data else { return };
                let rssi: isize = unsafe { msg_send![rssi, integerValue] };
                // 127 = not available
                if rssi >= 0 {
                    return;
                }
                if let Some(a) = parse_proximity(&bytes_of(&data), rssi as i16) {
                    let mut s = state().lock_ok();
                    let keep = s.adverts.get(&a.model).is_none_or(|(t, old)| t.elapsed() > Duration::from_secs(10) || a.rssi >= old.rssi);
                    if keep {
                        s.adverts.insert(a.model, (Instant::now(), a));
                    }
                }
            }

            #[unsafe(method(centralManager:didConnectPeripheral:))]
            fn did_connect(&self, _central: &AnyObject, p: &AnyObject) {
                discover(p, self);
            }

            #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
            fn did_fail(&self, _central: &AnyObject, p: &AnyObject, _error: *mut AnyObject) {
                release(p);
            }

            #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
            fn did_disconnect(&self, _central: &AnyObject, p: &AnyObject, _error: *mut AnyObject) {
                state().lock_ok().gatt.remove(&peripheral_id(p));
                release(p);
            }

            #[unsafe(method(peripheral:didDiscoverServices:))]
            fn did_services(&self, p: &AnyObject, _error: *mut AnyObject) {
                let services: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![p, services] };
                for s in services.iter().flat_map(|a| a.iter()) {
                    if is_uuid(&s, BATTERY_SERVICE) {
                        let want = NSArray::from_retained_slice(&[uuid(BATTERY_LEVEL)]);
                        let _: () = unsafe { msg_send![p, discoverCharacteristics: &*want, forService: &*s] };
                    }
                }
            }

            #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
            fn did_characteristics(&self, p: &AnyObject, service: &AnyObject, _error: *mut AnyObject) {
                let chars: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![service, characteristics] };
                for c in chars.iter().flat_map(|a| a.iter()) {
                    if is_uuid(&c, BATTERY_LEVEL) {
                        unsafe {
                            let _: () = msg_send![p, readValueForCharacteristic: &*c];
                            let _: () = msg_send![p, setNotifyValue: true, forCharacteristic: &*c];
                        }
                    }
                }
            }

            #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
            fn did_value(&self, p: &AnyObject, c: &AnyObject, error: *mut AnyObject) {
                if !error.is_null() || !is_uuid(c, BATTERY_LEVEL) {
                    return;
                }
                let value: Option<Retained<AnyObject>> = unsafe { msg_send![c, value] };
                let Some(&pct) = value.map(|v| bytes_of(&v)).as_deref().and_then(|b| b.first()) else { return };
                let Some(name) = string(unsafe { msg_send![p, name] }) else { return };
                if pct <= 100 {
                    state().lock_ok().gatt.insert(peripheral_id(p), (name, pct, Instant::now()));
                }
            }
        }
    );

    fn release(p: &AnyObject) {
        if let Some(core) = CORE.get() {
            core.held.lock_ok().retain(|h| !std::ptr::eq(&*h.0, p));
        }
    }

    fn discover(p: &AnyObject, delegate: &Delegate) {
        let services = NSArray::from_retained_slice(&[uuid(BATTERY_SERVICE)]);
        unsafe {
            let _: () = msg_send![p, setDelegate: delegate];
            let _: () = msg_send![p, discoverServices: &*services];
        }
    }

    /// Scan for adverts (every advert, duplicates included, so levels
    /// update), and read batteries of the LE devices macOS is connected to.
    fn begin_scan(central: &AnyObject) {
        unsafe {
            let yes = NSNumber::numberWithBool(true);
            let opts = NSDictionary::<NSString, NSNumber>::from_slices(&[&*ns("kCBScanOptionAllowDuplicates")], &[&*yes]);
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![central, scanForPeripheralsWithServices: none, options: &*opts];
        }
        {
            let mut s = state().lock_ok();
            s.scanning_since = Some(Instant::now());
            s.last_scan = Some(Instant::now());
        }
        let Some(core) = CORE.get() else { return };
        let services = NSArray::from_retained_slice(&[uuid(BATTERY_SERVICE)]);
        let list: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![central, retrieveConnectedPeripheralsWithServices: &*services] };
        let delegate: &Delegate = &core.delegate.0;
        for p in list.iter().flat_map(|a| a.iter()) {
            let state_: isize = unsafe { msg_send![&*p, state] };
            let mut held = core.held.lock_ok();
            let ours = held.iter().any(|h| std::ptr::eq(&*h.0, &*p));
            if ours && state_ == 2 {
                // connected already: read again
                let _: () = unsafe { msg_send![&*p, discoverServices: &*services] };
                continue;
            }
            if !ours {
                let _: () = unsafe { msg_send![&*p, setDelegate: delegate] };
                held.push(Keep(p.clone()));
            }
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = unsafe { msg_send![central, connectPeripheral: &*p, options: none] };
        }
    }

    fn end_scan(central: &AnyObject) {
        let _: () = unsafe { msg_send![central, stopScan] };
        state().lock_ok().scanning_since = None;
    }

    pub fn start() {
        // a central manager asks for the permission when it isn't decided:
        // only start once it's given (setup's Permissions step asks)
        if STARTED.load(Ordering::Relaxed) || !has_usage_text() || authorization() != ALLOWED {
            return;
        }
        if STARTED.swap(true, Ordering::Relaxed) {
            return;
        }
        let queue = DispatchQueue::new("cmdmeow.bluetooth", None);
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc().set_ivars(());
            unsafe { msg_send![super(this), init] }
        };
        let manager: Retained<AnyObject> = unsafe {
            let alloc: Allocated<AnyObject> = msg_send![class!(CBCentralManager), alloc];
            let q: *mut AnyObject = (&*queue as *const DispatchQueue).cast_mut().cast();
            msg_send![alloc, initWithDelegate: &*delegate, queue: q]
        };
        let _ = CORE.set(Core { queue, manager: Keep(manager), delegate: Keep(delegate), held: Mutex::new(vec![]) });
        // game controllers report to a menu bar app only with this
        let _: () = unsafe { msg_send![class!(GCController), setShouldMonitorBackgroundEvents: true] };
    }

    pub fn stop() {
        let Some(core) = CORE.get() else { return };
        if state().lock_ok().scanning_since.is_none() {
            return;
        }
        let m: Shared = Keep(core.manager.0.clone());
        core.queue.exec_async(move || {
            let m = m;
            end_scan(&m.0);
        });
    }

    pub fn tick() {
        let Some(core) = CORE.get() else { return };
        if POWER.load(Ordering::Relaxed) != POWERED_ON {
            return;
        }
        let (stop, go) = {
            let s = state().lock_ok();
            (s.scanning_since.is_some_and(|t| t.elapsed() >= SCAN_FOR), s.scanning_since.is_none() && s.last_scan.is_none_or(|t| t.elapsed() >= SCAN_EVERY))
        };
        if !(stop || go) {
            return;
        }
        let m: Shared = Keep(core.manager.0.clone());
        core.queue.exec_async(move || {
            let m = m;
            if stop {
                end_scan(&m.0);
            } else {
                begin_scan(&m.0);
            }
        });
    }

    pub fn adverts() -> Vec<PodsAdvert> {
        let mut s = state().lock_ok();
        s.adverts.retain(|_, (t, _)| t.elapsed() < ADVERT_TTL);
        s.adverts.values().map(|(_, a)| *a).collect()
    }

    pub fn gatt() -> Vec<Extra> {
        let mut s = state().lock_ok();
        s.gatt.retain(|_, (_, _, t)| t.elapsed() < GATT_TTL);
        let mut v: Vec<Extra> = s.gatt.values().map(|(name, pct, _)| Extra { name: name.clone(), pct: *pct, charging: None, address: String::new() }).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// GCDeviceBattery: level 0…1; state -1 unknown, 0 on battery,
    /// 1 charging, 2 full (plugged in).
    pub fn controllers() -> Vec<Extra> {
        let mut out = Vec::new();
        let list: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class!(GCController), controllers] };
        for c in list.iter().flat_map(|a| a.iter()) {
            let battery: Option<Retained<AnyObject>> = unsafe { msg_send![&*c, battery] };
            let Some(b) = battery else { continue };
            let Some(name) = string(unsafe { msg_send![&*c, vendorName] }) else { continue };
            let level: f32 = unsafe { msg_send![&*b, batteryLevel] };
            let st: isize = unsafe { msg_send![&*b, batteryState] };
            if !(0.0..=1.0).contains(&level) {
                continue;
            }
            out.push(Extra { name, pct: (level * 100.0).round() as u8, charging: (st >= 0).then_some(st >= 1), address: String::new() });
        }
        out
    }

    pub fn connected() -> Option<Vec<String>> {
        // IOBluetooth asks for the permission too: only once it's given
        if !has_usage_text() || authorization() != ALLOWED {
            return None;
        }
        let list: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class!(IOBluetoothDevice), pairedDevices] };
        let mut v: Vec<String> = list
            .iter()
            .flat_map(|a| a.iter())
            .filter(|d| unsafe { msg_send![&**d, isConnected] })
            .filter_map(|d| string(unsafe { msg_send![&*d, addressString] }))
            .map(|a| a.to_lowercase().replace(':', "-"))
            .collect();
        v.sort();
        Some(v)
    }

    /// Single-battery headphones over classic Bluetooth (AirPods Max, some
    /// Beats): the system report and ioreg leave their charge out, but
    /// IOBluetoothDevice knows it (`batteryPercentSingle`, 0 when unknown).
    pub fn classic() -> Vec<Extra> {
        if !has_usage_text() || authorization() != ALLOWED {
            return Vec::new();
        }
        let list: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class!(IOBluetoothDevice), pairedDevices] };
        let single = objc2::sel!(batteryPercentSingle);
        list.iter()
            .flat_map(|a| a.iter())
            .filter(|d| unsafe { msg_send![&**d, isConnected] })
            .filter(|d| unsafe { msg_send![&**d, respondsToSelector: single] })
            .filter_map(|d| {
                let pct: u8 = unsafe { msg_send![&*d, batteryPercentSingle] };
                let name = string(unsafe { msg_send![&*d, name] })?;
                let address = string(unsafe { msg_send![&*d, addressString] }).map(|a| a.to_lowercase().replace(':', "-")).unwrap_or_default();
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                    eprintln!("[bt] classic battery: {name} {address} {pct}%");
                }
                (1..=100).contains(&pct).then_some(Extra { name, pct, charging: None, address })
            })
            .collect()
    }
}
