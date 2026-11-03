//! The time zone's offset from UTC, for stamps kept as wall-clock time (a zip's).

/// Seconds to add to the UTC time `utc` (seconds since 1970) to get the wall-clock time then.
/// Zero where the zone can't be read.
pub fn offset_at_utc(utc: i64) -> i64 {
    imp::offset_at_utc(utc).unwrap_or(0)
}

/// Seconds to take off the wall-clock time `wall` (read as if it were UTC) to get UTC.
/// Zero where the zone can't be read.
pub fn offset_at_wall(wall: i64) -> i64 {
    imp::offset_at_wall(wall).unwrap_or(0)
}

#[cfg(windows)]
mod imp {
    use crate::civil::{civil_from_days, days_from_civil};
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::{
        SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime,
    };

    fn to_system(secs: i64) -> Option<SYSTEMTIME> {
        let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
        let rest = secs.rem_euclid(86_400);
        Some(SYSTEMTIME {
            wYear: u16::try_from(year).ok()?,
            wMonth: u16::from(month),
            wDay: u16::from(day),
            wHour: u16::try_from(rest / 3600).ok()?,
            wMinute: u16::try_from(rest % 3600 / 60).ok()?,
            wSecond: u16::try_from(rest % 60).ok()?,
            ..Default::default()
        })
    }

    fn from_system(t: &SYSTEMTIME) -> i64 {
        let days = days_from_civil(
            i64::from(t.wYear),
            u8::try_from(t.wMonth).unwrap_or(1),
            u8::try_from(t.wDay).unwrap_or(1),
        );
        days * 86_400 + i64::from(t.wHour) * 3600 + i64::from(t.wMinute) * 60 + i64::from(t.wSecond)
    }

    pub fn offset_at_utc(utc: i64) -> Option<i64> {
        let from = to_system(utc)?;
        let mut local = SYSTEMTIME::default();
        // SAFETY: both pointers refer to live locals; no time zone is given, so the current one is used.
        unsafe { SystemTimeToTzSpecificLocalTime(None, &raw const from, &raw mut local) }.ok()?;
        Some(from_system(&local) - utc)
    }

    pub fn offset_at_wall(wall: i64) -> Option<i64> {
        let from = to_system(wall)?;
        let mut utc = SYSTEMTIME::default();
        // SAFETY: both pointers refer to live locals; no time zone is given, so the current one is used.
        unsafe { TzSpecificLocalTimeToSystemTime(None, &raw const from, &raw mut utc) }.ok()?;
        Some(wall - from_system(&utc))
    }
}

#[cfg(unix)]
mod imp {
    pub fn offset_at_utc(utc: i64) -> Option<i64> {
        let t = libc::time_t::try_from(utc).ok()?;
        // SAFETY: `tm` is plain data, so all-zero is valid.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: `localtime_r` fills in `tm`; both pointers are live locals.
        if unsafe { libc::localtime_r(&raw const t, &raw mut tm) }.is_null() {
            return None;
        }
        #[expect(
            clippy::useless_conversion,
            reason = "`tm_gmtoff` is `i64` on 64-bit targets only"
        )]
        i64::try_from(tm.tm_gmtoff).ok()
    }

    pub fn offset_at_wall(wall: i64) -> Option<i64> {
        let t = libc::time_t::try_from(wall).ok()?;
        // SAFETY: `tm` is plain data, so all-zero is valid.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: `gmtime_r` splits the time into fields of `tm`; both pointers are live locals.
        if unsafe { libc::gmtime_r(&raw const t, &raw mut tm) }.is_null() {
            return None;
        }
        tm.tm_isdst = -1;
        // SAFETY: `mktime` reads the fields back as local time (the zone decides whether summer
        // time applies); `tm` is a live local.
        let utc = unsafe { libc::mktime(&raw mut tm) };
        #[expect(
            clippy::useless_conversion,
            reason = "`time_t` is `i64` on 64-bit targets only"
        )]
        (utc != -1).then(|| wall - i64::from(utc))
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn offset_at_utc(_utc: i64) -> Option<i64> {
        None
    }

    pub fn offset_at_wall(_wall: i64) -> Option<i64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_time_and_utc_convert_back_and_forth() {
        // Mid-month days, away from the hours where summer time starts or ends.
        for utc in [
            1_700_000_000_i64,
            1_720_000_000,
            1_650_000_000,
            1_000_000_000,
        ] {
            let wall = utc + offset_at_utc(utc);
            assert_eq!(wall - offset_at_wall(wall), utc);
        }
    }

    #[test]
    fn the_offset_is_a_plausible_zone() {
        let hours = offset_at_utc(1_700_000_000) / 3600;
        assert!((-12..=14).contains(&hours), "{hours}");
    }
}
