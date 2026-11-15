//! Is usage on pace? A limit window (or a month's budget) is used evenly
//! when the share used matches the share of time gone. Ahead of that, the
//! current rate runs out before the reset; the forecast says when.

use super::Limit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Used much less than the time gone.
    Room,
    OnPace,
    /// Used much more than the time gone.
    Ahead,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// Percent used if use were even over the window.
    pub expected: f32,
    pub state: State,
    /// When the current rate reaches 100%, if before the reset.
    pub out_at: Option<i64>,
}

/// How far from even use counts as ahead or room to spare, in points.
const SLACK: f32 = 8.0;

/// Pace of `used` percent in a window from `start` to `end`, at `now`.
pub fn of(used: f32, start: i64, end: i64, now: i64) -> Option<Pace> {
    let len = end - start;
    if len <= 0 {
        return None;
    }
    let gone = (now - start).clamp(0, len);
    let expected = gone as f32 / len as f32 * 100.0;
    let state = if used > expected + SLACK {
        State::Ahead
    } else if used < expected - SLACK {
        State::Room
    } else {
        State::OnPace
    };
    // too early in the window to tell a rate
    let out_at = if used > 0.0 && gone as f32 >= len as f32 * 0.03 && used < 100.0 {
        let at = start + (gone as f64 * 100.0 / used as f64) as i64;
        (at < end).then_some(at)
    } else if used >= 100.0 {
        Some(now)
    } else {
        None
    };
    Some(Pace { expected, state, out_at })
}

/// Pace of a plan limit, when its reset time and length are known.
pub fn of_limit(l: &Limit, now: i64) -> Option<Pace> {
    let end = l.resets_at?;
    if l.window_secs <= 0 || end <= now {
        return None;
    }
    of(l.pct, end - l.window_secs, end, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_use_is_on_pace() {
        let p = of(50.0, 0, 100, 50).unwrap();
        assert_eq!(p.expected, 50.0);
        assert_eq!(p.state, State::OnPace);
        assert_eq!(p.out_at, None);
    }

    #[test]
    fn ahead_runs_out_before_the_reset() {
        // 62% used 48% into the window: at this rate 100% at 77.4%
        let p = of(62.0, 0, 1000, 480).unwrap();
        assert_eq!(p.state, State::Ahead);
        assert_eq!(p.out_at, Some(774));
    }

    #[test]
    fn room_to_spare() {
        let p = of(19.0, 0, 5 * 3600, 2 * 3600 + 600).unwrap();
        assert_eq!(p.state, State::Room);
        assert_eq!(p.out_at, None);
    }

    #[test]
    fn edge_cases() {
        assert!(of(10.0, 10, 10, 10).is_none());
        // the window just started: no forecast yet
        assert_eq!(of(5.0, 0, 1000, 10).unwrap().out_at, None);
        assert_eq!(of(100.0, 0, 1000, 500).unwrap().out_at, Some(500));
        let l = Limit { pct: 30.0, resets_at: Some(1000), window_secs: 1000, ..Default::default() };
        assert_eq!(of_limit(&l, 500).unwrap().expected, 50.0);
        assert!(of_limit(&Limit { pct: 30.0, ..Default::default() }, 500).is_none());
    }
}
