//! iCalendar (.ics) feeds, as Google Calendar, iCloud and Outlook publish them.
//!
//! - Times in UTC, in local time, or in a named zone (read as this PC's zone: the feed is the
//!   user's own calendar), all-day dates, DTEND or DURATION.
//! - Repeating events (RRULE): daily, weekly on given weekdays, monthly on a day or "2nd Tuesday" /
//!   "last Friday", yearly; INTERVAL, COUNT, UNTIL. EXDATE skips, RECURRENCE-ID moves one occurrence.
//! - Wall-clock math goes through Windows' own time-zone rules, so a weekly 9 AM class stays at
//!   9 AM across daylight-saving changes.

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime};

pub const DAY: i64 = 86_400_000;

#[derive(Debug, Clone, Default)]
pub struct VEvent {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub url: String,
    /// UTC ms.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub rrule: Option<String>,
    pub exdates: Vec<i64>,
    pub recurrence_id: Option<i64>,
    pub cancelled: bool,
}

// ---------- civil dates (days since 1970-01-01) ----------

pub fn days_from_ymd(y: i64, m: i64, d: i64) -> i64 {
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn ymd_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Monday = 0 … Sunday = 6.
pub fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_ymd(ny, nm, 1) - days_from_ymd(y, m, 1)
}

// ---------- local time through Windows ----------

fn st(ms: i64) -> SYSTEMTIME {
    let days = ms.div_euclid(DAY);
    let rem = ms.rem_euclid(DAY);
    let (y, m, d) = ymd_from_days(days);
    SYSTEMTIME { wYear: y as u16, wMonth: m as u16, wDay: d as u16, wDayOfWeek: 0, wHour: (rem / 3_600_000) as u16, wMinute: (rem / 60_000 % 60) as u16, wSecond: (rem / 1000 % 60) as u16, wMilliseconds: (rem % 1000) as u16 }
}

fn ms(t: &SYSTEMTIME) -> i64 {
    days_from_ymd(t.wYear as i64, t.wMonth as i64, t.wDay as i64) * DAY + t.wHour as i64 * 3_600_000 + t.wMinute as i64 * 60_000 + t.wSecond as i64 * 1000 + t.wMilliseconds as i64
}

/// UTC instant → this PC's wall-clock time (as ms on a UTC scale).
pub fn to_local(utc: i64) -> i64 {
    let mut out = SYSTEMTIME::default();
    match unsafe { SystemTimeToTzSpecificLocalTime(None, &st(utc), &mut out) } {
        Ok(()) => ms(&out),
        Err(_) => utc,
    }
}

/// This PC's wall-clock time → UTC instant.
pub fn to_utc(local: i64) -> i64 {
    let mut out = SYSTEMTIME::default();
    match unsafe { TzSpecificLocalTimeToSystemTime(None, &st(local), &mut out) } {
        Ok(()) => ms(&out),
        Err(_) => local,
    }
}

// ---------- parsing ----------

/// A DATE or DATE-TIME value → (UTC ms, all-day).
pub fn parse_time(value: &str, params: &str) -> Option<(i64, bool)> {
    let v = value.trim();
    let n = |a: usize, l: usize| v.get(a..a + l)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0, 4)?, n(4, 2)?, n(6, 2)?);
    let day = days_from_ymd(y, mo, d);
    if v.len() == 8 || (params.contains("VALUE=DATE") && !params.contains("DATE-TIME")) {
        return Some((to_utc(day * DAY), true));
    }
    let (h, mi, s) = (n(9, 2)?, n(11, 2)?, n(13, 2).unwrap_or(0));
    let wall = day * DAY + h * 3_600_000 + mi * 60_000 + s * 1000;
    Some((if v.ends_with('Z') { wall } else { to_utc(wall) }, false))
}

/// "PT1H30M", "P1D", "P2W" → ms.
fn parse_duration(v: &str) -> Option<i64> {
    let (neg, v) = v.strip_prefix('-').map(|r| (true, r)).unwrap_or((false, v.trim_start_matches('+')));
    let v = v.strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (0i64, String::new(), false);
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let k: i64 = num.parse().ok()?;
                num.clear();
                total += k * match (c, in_time) {
                    ('W', _) => 7 * DAY,
                    ('D', _) => DAY,
                    ('H', true) => 3_600_000,
                    ('M', true) => 60_000,
                    ('S', true) => 1000,
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

pub fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

pub fn parse(text: &str) -> Vec<VEvent> {
    let unfolded = text.replace("\r\n ", "").replace("\r\n\t", "").replace("\n ", "").replace("\n\t", "");
    let mut out = vec![];
    let mut cur: Option<VEvent> = None;
    let mut duration: Option<i64> = None;
    let mut has_end = false;
    let mut nested = 0; // VALARM and friends inside an event
    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            cur = Some(VEvent::default());
            duration = None;
            has_end = false;
            nested = 0;
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(mut ev) = cur.take() {
                if !has_end {
                    ev.end = ev.start + duration.unwrap_or(if ev.all_day { DAY } else { 0 });
                }
                if ev.start != 0 || ev.end != 0 {
                    out.push(ev);
                }
            }
            continue;
        }
        let Some(ev) = cur.as_mut() else { continue };
        if line.starts_with("BEGIN:") {
            nested += 1;
            continue;
        }
        if line.starts_with("END:") {
            nested -= 1;
            continue;
        }
        if nested > 0 {
            continue;
        }
        let Some((head, value)) = line.split_once(':') else { continue };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        match name {
            "UID" => ev.uid = value.to_string(),
            "SUMMARY" => ev.summary = unescape(value),
            "DESCRIPTION" => ev.description = unescape(value),
            "LOCATION" => ev.location = unescape(value),
            "URL" => ev.url = value.to_string(),
            "STATUS" => ev.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                if let Some((t, all)) = parse_time(value, params) {
                    ev.start = t;
                    ev.all_day = all;
                }
            }
            "DTEND" => {
                if let Some((t, _)) = parse_time(value, params) {
                    ev.end = t;
                    has_end = true;
                }
            }
            "DURATION" => duration = parse_duration(value),
            "RRULE" => ev.rrule = Some(value.to_string()),
            "EXDATE" => ev.exdates.extend(value.split(',').filter_map(|x| parse_time(x, params).map(|t| t.0))),
            "RECURRENCE-ID" => ev.recurrence_id = parse_time(value, params).map(|t| t.0),
            _ => {}
        }
    }
    out
}

// ---------- repeats ----------

fn weekday_code(s: &str) -> Option<i64> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| *d == s).map(|i| i as i64)
}

/// "2TU" → (2, Tuesday); "-1FR" → (-1, Friday); "MO" → (0, Monday).
fn byday(s: &str) -> Option<(i64, i64)> {
    let s = s.trim();
    let split = s.len().checked_sub(2)?;
    let (n, d) = s.split_at(split);
    Some((if n.is_empty() { 0 } else { n.trim_start_matches('+').parse().ok()? }, weekday_code(d)?))
}

/// Day number of the nth weekday `wd` of a month (negative n counts from the end).
fn nth_weekday(y: i64, m: i64, n: i64, wd: i64) -> Option<i64> {
    let first = days_from_ymd(y, m, 1);
    let len = days_in_month(y, m);
    if n > 0 {
        let d = first + (wd - weekday(first)).rem_euclid(7) + (n - 1) * 7;
        (d < first + len).then_some(d)
    } else if n < 0 {
        let last = first + len - 1;
        let d = last - (weekday(last) - wd).rem_euclid(7) + (n + 1) * 7;
        (d >= first).then_some(d)
    } else {
        None
    }
}

/// Start times (UTC) of an event's occurrences that overlap [from, to).
pub fn occurrences(ev: &VEvent, from: i64, to: i64) -> Vec<i64> {
    let dur = (ev.end - ev.start).max(0);
    let Some(rule) = ev.rrule.as_deref() else {
        return if ev.start < to && ev.start + dur.max(1) > from { vec![ev.start] } else { vec![] };
    };
    let parts: std::collections::HashMap<&str, &str> = rule.split(';').filter_map(|p| p.split_once('=')).collect();
    let freq = parts.get("FREQ").copied().unwrap_or("DAILY");
    let interval = parts.get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let count = parts.get("COUNT").and_then(|v| v.parse::<usize>().ok());
    let until = parts.get("UNTIL").and_then(|v| parse_time(v, "").map(|t| t.0));
    let bydays: Vec<(i64, i64)> = parts.get("BYDAY").map(|v| v.split(',').filter_map(byday).collect()).unwrap_or_default();
    let bymonthday: Vec<i64> = parts.get("BYMONTHDAY").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();

    let local0 = to_local(ev.start);
    let d0 = local0.div_euclid(DAY);
    let tod = local0.rem_euclid(DAY);
    let (y0, m0, dd0) = ymd_from_days(d0);
    let to_instant = |day: i64| to_utc(day * DAY + tod);

    let mut out = vec![];
    let mut seen = 0usize;
    let mut emit = |day: i64, out: &mut Vec<i64>| -> bool {
        // Returns false to stop.
        if day < d0 {
            return true;
        }
        let t = to_instant(day);
        if until.is_some_and(|u| t > u) || t >= to {
            return false;
        }
        seen += 1;
        if count.is_some_and(|c| seen > c) {
            return false;
        }
        if t + dur.max(1) > from && !ev.exdates.contains(&t) {
            out.push(t);
        }
        true
    };
    match freq {
        "WEEKLY" => {
            let days: Vec<i64> = if bydays.is_empty() { vec![weekday(d0)] } else { let mut v: Vec<i64> = bydays.iter().map(|b| b.1).collect(); v.sort(); v.dedup(); v };
            let week0 = d0 - weekday(d0);
            'w: for k in 0..5000 {
                for wd in &days {
                    if !emit(week0 + k * 7 * interval + wd, &mut out) {
                        break 'w;
                    }
                }
            }
        }
        "MONTHLY" => {
            'm: for k in 0..1200 {
                let mi = (m0 - 1) + k * interval;
                let (y, m) = (y0 + mi.div_euclid(12), mi.rem_euclid(12) + 1);
                let mut days: Vec<i64> = if !bydays.is_empty() {
                    bydays.iter().filter_map(|(n, wd)| nth_weekday(y, m, if *n == 0 { 1 } else { *n }, *wd)).collect()
                } else {
                    let want = if bymonthday.is_empty() { vec![dd0] } else { bymonthday.clone() };
                    want.iter().filter_map(|d| { let len = days_in_month(y, m); let d = if *d < 0 { len + d + 1 } else { *d }; (d >= 1 && d <= len).then(|| days_from_ymd(y, m, d)) }).collect()
                };
                days.sort();
                for d in days {
                    if !emit(d, &mut out) {
                        break 'm;
                    }
                }
            }
        }
        "YEARLY" => {
            for k in 0..200 {
                let y = y0 + k * interval;
                if dd0 > days_in_month(y, m0) {
                    continue;
                }
                if !emit(days_from_ymd(y, m0, dd0), &mut out) {
                    break;
                }
            }
        }
        _ => {
            for k in 0..20000 {
                if !emit(d0 + k * interval, &mut out) {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> i64 {
        parse_time(s, "").unwrap().0
    }

    #[test]
    fn dates() {
        for d in [-1000, 0, 19000, 20741] {
            let (y, m, dd) = ymd_from_days(d);
            assert_eq!(days_from_ymd(y, m, dd), d);
        }
        assert_eq!(weekday(days_from_ymd(2026, 10, 15)), 3); // a Thursday
        assert_eq!(nth_weekday(2026, 10, 2, 1), Some(days_from_ymd(2026, 10, 13))); // 2nd Tuesday
        assert_eq!(nth_weekday(2026, 10, -1, 4), Some(days_from_ymd(2026, 10, 30))); // last Friday
        assert_eq!(parse_duration("PT1H30M"), Some(90 * 60_000));
        assert_eq!(parse_duration("P1W"), Some(7 * DAY));
    }

    #[test]
    fn feed_with_repeats_and_exceptions() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:class\r\nSUMMARY:CS 260 lecture\r\nDTSTART:20261013T130000Z\r\nDTEND:20261013T142000Z\r\nRRULE:FREQ=WEEKLY;BYDAY=TU,TH;COUNT=6\r\nEXDATE:20261015T130000Z\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:ignored\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Reading day\\, no class\r\nDTSTART;VALUE=DATE:20261016\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = parse(ics);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].description, "");
        assert_eq!(ev[1].summary, "Reading day, no class");
        assert!(ev[1].all_day);
        assert_eq!(ev[1].end - ev[1].start, DAY);
        let occ = occurrences(&ev[0], utc("20261001T000000Z"), utc("20261201T000000Z"));
        // Tue 13, (Thu 15 skipped), Tue 20, Thu 22, Tue 27, Thu 29 → COUNT=6 includes the skipped one.
        assert_eq!(occ.len(), 5, "{occ:?}");
        assert_eq!(occ[0], utc("20261013T130000Z"));
        assert!(!occ.contains(&utc("20261015T130000Z")));
    }

    #[test]
    fn monthly_and_until() {
        let ev = VEvent { start: utc("20260113T180000Z"), end: utc("20260113T190000Z"), rrule: Some("FREQ=MONTHLY;BYDAY=2TU;UNTIL=20260601T000000Z".into()), ..Default::default() };
        let occ = occurrences(&ev, utc("20260101T000000Z"), utc("20270101T000000Z"));
        assert_eq!(occ.len(), 5); // Jan–May
    }
}
