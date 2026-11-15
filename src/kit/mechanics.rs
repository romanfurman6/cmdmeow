//! The numbers from docs/MECHANICS.md / docs/mechanics.json, used as written there.

// Mac Health thresholds (estimated: the original's Swift view models didn't decompile)
/// SharedPreferences sub_1c60/sub_5764: the low-disk alert threshold is the
/// preset nearest to 10% of the disk's capacity (presets from the table at
/// 0x1c1e0); 1 TB when the capacity can't be read.
pub const DISK_THRESHOLD_PRESETS: [u64; 7] = [2_000_000_000, 5_000_000_000, 10_000_000_000, 20_000_000_000, 30_000_000_000, 50_000_000_000, 100_000_000_000];
pub const DISK_THRESHOLD_CAPACITY_DIVISOR: u64 = 10;
pub const DISK_THRESHOLD_FALLBACK: u64 = 1_000_000_000_000;
/// SharedPreferences sub_5594: Trash alert = TrashSizeLimits[2] (table at 0x1c280).
pub const TRASH_SIZE_LIMITS: [u64; 11] = [500_000_000, 1_000_000_000, 2_000_000_000, 3_000_000_000, 5_000_000_000, 10_000_000_000, 15_000_000_000, 20_000_000_000, 30_000_000_000, 50_000_000_000, 100_000_000_000];
pub const TRASH_SIZE_THRESHOLD: u64 = TRASH_SIZE_LIMITS[2];
/// HealthIssuesController sub_a308: a Bluetooth device is ignored when its
/// battery is above 19% (cmp #0x13); Apple mobile devices are skipped.
pub const BT_LOW_BATTERY_MAX_PCT: u8 = 19;
/// Ours: a device at or below this is almost empty (red, second alert).
pub const BT_CRITICAL_BATTERY_MAX_PCT: u8 = 9;
/// Ours: a device charged to this forgets its earlier low-battery alerts.
pub const BT_ALERT_RESET_PCT: u8 = 30;
/// Ours: earbuds this close (left vs right) show as one number.
pub const BT_MERGE_PODS_PCT: u8 = 5;
/// AssistantDomain sub_20544 arguments: recommendation priorities.
pub const PRIORITY_DRIVE_HEALTH: u32 = 100;
pub const PRIORITY_BATTERY_HEALTH: u32 = 80;
pub const PRIORITY_OUT_OF_DISK: u32 = 20;
pub const PRIORITY_TRASH_OVERFILL: u32 = 5;

pub fn disk_alert_threshold(capacity: u64) -> u64 {
    if capacity == 0 {
        return DISK_THRESHOLD_FALLBACK;
    }
    let target = capacity / DISK_THRESHOLD_CAPACITY_DIVISOR;
    let mut best = DISK_THRESHOLD_PRESETS[0];
    for &p in DISK_THRESHOLD_PRESETS.iter() {
        if (p as f64 - target as f64).abs() < (best as f64 - target as f64).abs() {
            best = p;
        }
    }
    best
}
/// SystemResourcesMonitor sub_9504: kern.memorystatus_vm_pressure_level,
/// 1 → normal, 2 → warning, anything else → critical (lookup 0x02010003).
pub const KERNEL_PRESSURE_NORMAL: u32 = 1;
pub const KERNEL_PRESSURE_WARN: u32 = 2;

/// SystemResourcesMonitor sub_90b8: pressure fraction = (wired + compressed) / physical.
pub fn memory_pressure_pct(wired: u64, compressed: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    ((wired + compressed) as f64 / total as f64 * 100.0) as f32
}

/// BatteryMonitor sub_1bc8: temperature raw ÷ 100; times of 65535 mean unknown.
pub const BATTERY_TEMP_DIVISOR: f32 = 100.0;
pub const BATTERY_TIME_UNKNOWN: f64 = 65535.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryHealthState {
    Unknown,
    Good,
    Fair,
    Poor,
    Service,
}

/// BatteryMonitor sub_25d4: condition "Check Battery" / "Permanent Battery
/// Failure" or the failure flag → 4 (service); health "Good" 1, "Fair" 2, "Poor" 3.
pub fn battery_health_state(text: &str, permanent_failure: bool) -> BatteryHealthState {
    let t = text.trim();
    if permanent_failure || t.eq_ignore_ascii_case("Check Battery") || t.eq_ignore_ascii_case("Permanent Battery Failure") || t.eq_ignore_ascii_case("Service Recommended") {
        BatteryHealthState::Service
    } else if t.eq_ignore_ascii_case("Good") || t.eq_ignore_ascii_case("Normal") {
        BatteryHealthState::Good
    } else if t.eq_ignore_ascii_case("Fair") {
        BatteryHealthState::Fair
    } else if t.eq_ignore_ascii_case("Poor") {
        BatteryHealthState::Poor
    } else {
        BatteryHealthState::Unknown
    }
}
/// AssistantDomain sub_1ba28: UptimeHealthFactor.noRecentRestartInterval = 604800 s;
/// uptime days = trunc(seconds / 86400) (sub_1bbc0).
pub const UPTIME_RESTART_INTERVAL_SECS: u64 = 604_800;
pub const SECONDS_PER_DAY: u64 = 86_400;
pub const MONITOR_REFRESH_MS: u64 = 2000;

// Speed test ("downloads a small test file")
pub const SPEED_TEST_DOWN_URL: &str = "https://speed.cloudflare.com/__down";
pub const SPEED_TEST_UP_URL: &str = "https://speed.cloudflare.com/__up";
pub const SPEED_TEST_META_URL: &str = "https://speed.cloudflare.com/meta";
/// Each of download and upload runs this long, over several connections
/// (one TCP stream can't fill a fast line).
pub const SPEED_TEST_PHASE: std::time::Duration = std::time::Duration::from_secs(8);
/// The start of a transfer (TCP slow start) doesn't count.
pub const SPEED_TEST_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
pub const SPEED_TEST_STREAMS_DOWN: usize = 6;
pub const SPEED_TEST_STREAMS_UP: usize = 4;
/// Bytes per request: streams ask again until the phase ends.
pub const SPEED_TEST_CHUNK_DOWN: u64 = 25_000_000;
pub const SPEED_TEST_CHUNK_UP: u64 = 4_000_000;
/// Round trips for ping and jitter, after one that opens the connection.
pub const SPEED_TEST_PINGS: usize = 10;
/// Legend "Good for:" (labels from the Menu strings, Mbps estimated)
pub const GOOD_FOR: [(&str, f64); 6] = [
    ("Messaging", 0.5),
    ("Audio calls", 1.0),
    ("Music streaming", 2.0),
    ("Video calls", 4.0),
    ("Video streaming", 10.0),
    ("Online gaming", 25.0),
];

// Panel geometry (design A: 380 pt panel and sheets, 12 pt cards)
pub const PANEL_WIDTH: f32 = 380.0;
pub const PANEL_HEIGHT: f32 = 660.0;
pub const DETAIL_WIDTH: f32 = 380.0;
pub const CARD_RADIUS: f32 = 12.0;
pub const APP_WIDTH: f32 = 1180.0;
pub const APP_HEIGHT: f32 = 740.0;

// Claude
pub const CLAUDE_LIMITS_REFRESH_SECS: u64 = 300;
pub const CLAUDE_LOCAL_REFRESH_SECS: u64 = 60;
/// A Claude Code project counts as "in use" when touched within this many days.
pub const CLAUDE_PROJECT_ACTIVE_DAYS: u64 = 30;
/// File-history / uploads older than this are offered for review.
pub const CLAUDE_OLD_DATA_DAYS: u64 = 30;

// Cleanup
/// Items smaller than this are judged by the rules only (not sent to the AI).
pub const AI_MIN_BYTES: u64 = 1_000_000;
pub const AI_BATCH: usize = 120;
pub const AI_MODEL: &str = "sonnet";
/// Hide results smaller than this.
pub const MIN_ITEM_BYTES: u64 = 64 * 1024;
/// Shell snapshots younger than this may belong to a running session.
pub const SHELL_SNAPSHOT_MIN_AGE_DAYS: f64 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_threshold_picks_nearest_preset() {
        assert_eq!(disk_alert_threshold(0), DISK_THRESHOLD_FALLBACK);
        // 10% of 256 GB = 25.6 GB → 30 GB is nearer than 20 GB
        assert_eq!(disk_alert_threshold(256_000_000_000), 30_000_000_000);
        // 10% of 1 TB = 100 GB
        assert_eq!(disk_alert_threshold(1_000_000_000_000), 100_000_000_000);
        // 10% of 8 TB is above every preset → the largest
        assert_eq!(disk_alert_threshold(8_000_000_000_000), 100_000_000_000);
        // tiny disk → the smallest
        assert_eq!(disk_alert_threshold(1_000_000_000), 2_000_000_000);
    }

    #[test]
    fn memory_pressure() {
        assert_eq!(memory_pressure_pct(1, 1, 0), 0.0);
        assert!((memory_pressure_pct(25, 25, 100) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn battery_health_words() {
        use BatteryHealthState::*;
        assert_eq!(battery_health_state("Good", false), Good);
        assert_eq!(battery_health_state(" normal ", false), Good);
        assert_eq!(battery_health_state("Fair", false), Fair);
        assert_eq!(battery_health_state("Poor", false), Poor);
        assert_eq!(battery_health_state("Service Recommended", false), Service);
        assert_eq!(battery_health_state("Check Battery", false), Service);
        assert_eq!(battery_health_state("Good", true), Service);
        assert_eq!(battery_health_state("", false), Unknown);
    }
}
