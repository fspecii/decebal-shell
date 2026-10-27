//! Local time and the bar's `ddd d MMM  HH:mm` format. The bar only wakes once a minute.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The broken-down time fields the bar needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tm {
    /// 0 = Sunday.
    pub wday: u32,
    pub mday: u32,
    /// 0 = January.
    pub mon: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
}

/// `Mon 26 Oct  03:08` (day of month is not zero-padded, two spaces before the time).
pub fn format(tm: &Tm) -> String {
    format!("{} {} {}  {:02}:{:02}", DAYS[(tm.wday % 7) as usize], tm.mday, MONTHS[(tm.mon % 12) as usize], tm.hour, tm.min)
}

/// Local time for a unix timestamp, via the C library so the system timezone is honoured.
pub fn local_tm(unix_secs: i64) -> Option<Tm> {
    let t: libc::time_t = unix_secs as libc::time_t;
    // SAFETY: `tm` is plain-old-data that localtime_r fully initialises on success.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the duration of the call; localtime_r is thread-safe.
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    ok.then(|| Tm {
        wday: tm.tm_wday.max(0) as u32,
        mday: tm.tm_mday.max(1) as u32,
        mon: tm.tm_mon.max(0) as u32,
        hour: tm.tm_hour.max(0) as u32,
        min: tm.tm_min.max(0) as u32,
        sec: tm.tm_sec.max(0) as u32,
    })
}

pub fn now() -> Option<Tm> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    local_tm(secs as i64)
}

/// Today's local date as (year, month 0-based, day of month).
pub fn today() -> Option<(i32, u32, u32)> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as libc::time_t;
    // SAFETY: see `local_tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    ok.then(|| (tm.tm_year + 1900, tm.tm_mon.max(0) as u32, tm.tm_mday.max(1) as u32))
}

/// Time until the next minute boundary (at least 1 s so a tick never spins).
pub fn until_next_minute(tm: &Tm) -> Duration {
    Duration::from_secs(u64::from(60u32.saturating_sub(tm.sec)).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm(wday: u32, mday: u32, mon: u32, hour: u32, min: u32, sec: u32) -> Tm {
        Tm { wday, mday, mon, hour, min, sec }
    }

    #[test]
    fn formats_like_the_original_bar() {
        assert_eq!(format(&tm(1, 26, 9, 3, 8, 0)), "Mon 26 Oct  03:08");
    }

    #[test]
    fn day_is_not_padded_time_is() {
        assert_eq!(format(&tm(0, 5, 0, 14, 5, 0)), "Sun 5 Jan  14:05");
    }

    #[test]
    fn out_of_range_fields_do_not_panic() {
        assert_eq!(format(&tm(9, 1, 14, 0, 0, 0)), "Tue 1 Mar  00:00");
    }

    #[test]
    fn next_minute_is_between_1_and_60_seconds() {
        assert_eq!(until_next_minute(&tm(0, 1, 0, 0, 0, 0)), Duration::from_secs(60));
        assert_eq!(until_next_minute(&tm(0, 1, 0, 0, 0, 59)), Duration::from_secs(1));
        assert_eq!(until_next_minute(&tm(0, 1, 0, 0, 0, 30)), Duration::from_secs(30));
    }

    #[test]
    fn local_time_is_sane() {
        let t = now().expect("localtime");
        assert!(t.hour < 24 && t.min < 60 && t.mon < 12);
    }
}
