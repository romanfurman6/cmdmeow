//! Dates and times on screen, with month and weekday names from the
//! locale (chrono's `%b`/`%a`/`%P` are English only). Numbers stay
//! numbers: 24-hour times still use `format("%H:%M")`.

use chrono::{Datelike, Timelike};

/// "Nov".
pub fn mon(d: &impl Datelike) -> &'static str {
    match d.month() {
        1 => ts!("kit.date.mon.jan"),
        2 => ts!("kit.date.mon.feb"),
        3 => ts!("kit.date.mon.mar"),
        4 => ts!("kit.date.mon.apr"),
        5 => ts!("kit.date.mon.may"),
        6 => ts!("kit.date.mon.jun"),
        7 => ts!("kit.date.mon.jul"),
        8 => ts!("kit.date.mon.aug"),
        9 => ts!("kit.date.mon.sep"),
        10 => ts!("kit.date.mon.oct"),
        11 => ts!("kit.date.mon.nov"),
        _ => ts!("kit.date.mon.dec"),
    }
}

/// "November".
pub fn month(d: &impl Datelike) -> &'static str {
    match d.month() {
        1 => ts!("kit.date.month.jan"),
        2 => ts!("kit.date.month.feb"),
        3 => ts!("kit.date.month.mar"),
        4 => ts!("kit.date.month.apr"),
        5 => ts!("kit.date.month.may"),
        6 => ts!("kit.date.month.jun"),
        7 => ts!("kit.date.month.jul"),
        8 => ts!("kit.date.month.aug"),
        9 => ts!("kit.date.month.sep"),
        10 => ts!("kit.date.month.oct"),
        11 => ts!("kit.date.month.nov"),
        _ => ts!("kit.date.month.dec"),
    }
}

/// "Sat".
pub fn wd(d: &impl Datelike) -> &'static str {
    use chrono::Weekday::*;
    match d.weekday() {
        Mon => ts!("kit.date.wd.mon"),
        Tue => ts!("kit.date.wd.tue"),
        Wed => ts!("kit.date.wd.wed"),
        Thu => ts!("kit.date.wd.thu"),
        Fri => ts!("kit.date.wd.fri"),
        Sat => ts!("kit.date.wd.sat"),
        Sun => ts!("kit.date.wd.sun"),
    }
}

/// "Saturday".
pub fn weekday(d: &impl Datelike) -> &'static str {
    use chrono::Weekday::*;
    match d.weekday() {
        Mon => ts!("kit.date.weekday.mon"),
        Tue => ts!("kit.date.weekday.tue"),
        Wed => ts!("kit.date.weekday.wed"),
        Thu => ts!("kit.date.weekday.thu"),
        Fri => ts!("kit.date.weekday.fri"),
        Sat => ts!("kit.date.weekday.sat"),
        Sun => ts!("kit.date.weekday.sun"),
    }
}

/// "Nov 8".
pub fn md(d: &impl Datelike) -> String {
    t!("kit.date.md", mon = mon(d), day = d.day()).into_owned()
}

/// "Sat Nov 8".
pub fn wmd(d: &impl Datelike) -> String {
    t!("kit.date.wmd", wd = wd(d), mon = mon(d), day = d.day()).into_owned()
}

/// "Saturday, Nov 8".
pub fn day_title(d: &impl Datelike) -> String {
    t!("kit.date.day_title", weekday = weekday(d), mon = mon(d), day = d.day()).into_owned()
}

fn ap(t: &impl Timelike) -> &'static str {
    if t.hour12().0 { ts!("kit.date.pm") } else { ts!("kit.date.am") }
}

/// "9:05 pm".
pub fn hm12(t: &impl Timelike) -> String {
    t!("kit.date.hm12", h = t.hour12().1, m = format!("{:02}", t.minute()), ap = ap(t)).into_owned()
}

/// "9:05pm", or "9pm" on the hour.
pub fn hm12_short(t: &impl Timelike) -> String {
    if t.minute() == 0 {
        h12_short(t)
    } else {
        t!("kit.date.hm12_short", h = t.hour12().1, m = format!("{:02}", t.minute()), ap = ap(t)).into_owned()
    }
}

/// "9pm".
pub fn h12_short(t: &impl Timelike) -> String {
    t!("kit.date.h12_short", h = t.hour12().1, ap = ap(t)).into_owned()
}

/// A date then a time: "Sat 14:00", "Nov 8 14:00".
pub fn then(date: &str, time: &str) -> String {
    t!("kit.date.then", date = date, time = time).into_owned()
}

/// A date, a comma, a time: "Nov 8, 9:05 pm".
pub fn comma(date: &str, time: &str) -> String {
    t!("kit.date.comma", date = date, time = time).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn matches_chrono_in_english() {
        for (y, m, d, h, min) in [(2026, 11, 8, 21, 5), (2026, 1, 31, 0, 0), (2026, 5, 4, 12, 30), (2026, 9, 13, 9, 0)] {
            let t = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap();
            assert_eq!(mon(&t), t.format("%b").to_string());
            assert_eq!(month(&t), t.format("%B").to_string());
            assert_eq!(wd(&t), t.format("%a").to_string());
            assert_eq!(weekday(&t), t.format("%A").to_string());
            assert_eq!(md(&t), t.format("%b %-d").to_string());
            assert_eq!(wmd(&t), t.format("%a %b %-d").to_string());
            assert_eq!(day_title(&t), t.format("%A, %b %-d").to_string());
            assert_eq!(hm12(&t), t.format("%-I:%M %P").to_string());
            assert_eq!(h12_short(&t), t.format("%-I%P").to_string());
            let short = if min == 0 { t.format("%-I%P") } else { t.format("%-I:%M%P") };
            assert_eq!(hm12_short(&t), short.to_string());
            assert_eq!(comma(&md(&t), &hm12(&t)), t.format("%b %-d, %-I:%M %P").to_string());
            assert_eq!(then(wd(&t), &t.format("%H:%M").to_string()), t.format("%a %H:%M").to_string());
        }
    }
}
