//! A stop's departures board: the timetable's trips (gtfs.rs) for the next couple of hours, moved
//! by the live feed (rt.rs).
//!
//! GTFS times count from "noon minus 12 hours" of the service day in the agency's time zone (so a
//! 25:10:00 departure is 1:10 the next morning), and a trip may belong to yesterday's service. A
//! live update for the stop gives an exact time or a delay; a delay at an earlier stop carries on
//! to the later ones, as the GTFS-realtime spec says; a trip's own delay is the last resort.
//! SEPTA's buses send exact times for every coming stop; Regional Rail sends one delay per train.
//!
//! Agencies publish their next timetable before it starts, and some then stop publishing the one
//! that's running (SEPTA's download from 20 October only has service from 1 November). So a trip the
//! live feed says is running today counts whatever the calendar says, and on a day the timetable
//! doesn't cover at all, the same weekday from the period it does cover stands in ([`stand_in`]);
//! those departures are marked `approx`.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;

use crate::gtfs::{Index, Visit};
use crate::rt::TripUpdate;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Departure {
    pub trip: String,
    pub route: String,
    /// The route's number or name, for its chip.
    pub label: String,
    pub color: String,
    pub text: String,
    /// GTFS route_type (0 tram, 1 subway, 2 rail, 3 bus…).
    pub kind: u16,
    pub headsign: String,
    /// When it leaves (Unix seconds), live when the feed says so.
    pub at: i64,
    pub scheduled: i64,
    pub live: bool,
    /// Seconds late (negative: early), when live.
    pub delay: i64,
    pub canceled: bool,
    /// From a stand-in day of the timetable (today isn't in it), with no live update.
    pub approx: bool,
}

/// The agency's time zone, or New York's when the feed names one we don't know.
pub fn zone(name: &str) -> Tz {
    name.parse().unwrap_or(chrono_tz::America::New_York)
}

/// The service day's start in Unix seconds: noon minus 12 hours, local time (which differs from
/// midnight on the days the clocks change).
pub fn day_start(tz: Tz, d: NaiveDate) -> i64 {
    let noon = d.and_hms_opt(12, 0, 0).expect("noon exists");
    tz.from_local_datetime(&noon).earliest().map(|t| t.timestamp() - 12 * 3600).unwrap_or(0)
}

fn yyyymmdd(d: NaiveDate) -> u32 {
    d.year() as u32 * 10000 + d.month() * 100 + d.day()
}

fn runs(ix: &Index, service: &str, d: NaiveDate) -> bool {
    ix.services.get(service).is_some_and(|s| s.runs(yyyymmdd(d), d.weekday().num_days_from_monday() as usize))
}

/// When nothing at all runs on `day` in this timetable (it starts later, or it's run out), the
/// nearest same weekday that has service, up to eight weeks either way.
pub fn stand_in(ix: &Index, day: NaiveDate) -> Option<NaiveDate> {
    let any = |d: NaiveDate| ix.services.keys().any(|s| runs(ix, s, d));
    if any(day) {
        return None;
    }
    (1..=8).flat_map(|w| [day + Duration::weeks(w), day - Duration::weeks(w)]).find(|d| any(*d))
}

/// What the live feed says about one visit: (when, delay, live), or None when it skips the stop.
fn live(v: &Visit, scheduled: i64, ids: &HashSet<String>, tu: &TripUpdate) -> Option<(i64, i64, bool)> {
    // The update for this stop, by id or by its place in the trip.
    if let Some(s) = tu.stops.iter().find(|s| s.stop_id.as_ref().is_some_and(|i| ids.contains(i)) || (s.stop_id.is_none() && s.seq == Some(v.seq))) {
        if s.skipped {
            return None;
        }
        if let Some(e) = s.event() {
            if let Some(t) = e.time {
                return Some((t, t - scheduled, true));
            }
            if let Some(d) = e.delay {
                return Some((scheduled + d as i64, d as i64, true));
            }
        }
    }
    // A delay at the last stop before this one carries on.
    let before = tu.stops.iter().filter(|s| s.seq.is_some_and(|q| q < v.seq)).max_by_key(|s| s.seq);
    if let Some(d) = before.and_then(|s| s.event()).and_then(|e| e.delay) {
        return Some((scheduled + d as i64, d as i64, true));
    }
    if let Some(d) = tu.delay {
        return Some((scheduled + d as i64, d as i64, true));
    }
    // In the feed with nothing for this stop yet: on time as far as anyone knows.
    Some((scheduled, 0, false))
}

/// The departures from `ids` (a stop, or a station's platforms) between a minute ago and `ahead`
/// seconds from `now`, soonest first, only `routes` when it's not empty.
pub fn board(visits: &[Visit], ids: &HashSet<String>, ix: &Index, rt: &HashMap<String, TripUpdate>, now: i64, ahead: i64, routes: &[String]) -> Vec<Departure> {
    let tz = zone(&ix.tz);
    let today = tz.timestamp_opt(now, 0).single().map(|t| t.date_naive()).unwrap_or_default();
    let mut out = vec![];
    for day in [today - Duration::days(1), today] {
        let start = day_start(tz, day);
        let date = yyyymmdd(day);
        let stand = stand_in(ix, day);
        for v in visits.iter().filter(|v| ids.contains(&v.stop) && (routes.is_empty() || routes.contains(&v.route))) {
            let scheduled = start + v.secs as i64;
            // A late vehicle can still be coming for a while after its time.
            if scheduled < now - 3600 || scheduled > now + ahead {
                continue;
            }
            let on = runs(ix, &v.service, stand.unwrap_or(day));
            // The live feed has the last word on whether a trip runs that day.
            let tu = rt.get(&v.trip).filter(|t| t.start_date == date.to_string() || (t.start_date.is_empty() && on));
            if !on && tu.is_none() {
                continue;
            }
            let (at, delay, is_live) = match tu {
                Some(t) => match live(v, scheduled, ids, t) {
                    Some(x) => x,
                    None => continue,
                },
                None => (scheduled, 0, false),
            };
            if at < now - 60 || at > now + ahead {
                continue;
            }
            let r = ix.routes.get(&v.route).cloned().unwrap_or_default();
            out.push(Departure {
                trip: v.trip.clone(),
                route: v.route.clone(),
                label: if r.label().is_empty() { v.route.clone() } else { r.label().to_string() },
                color: r.color,
                text: r.text,
                kind: r.kind,
                headsign: v.headsign.clone(),
                at,
                scheduled,
                live: is_live,
                delay,
                canceled: tu.is_some_and(|t| t.canceled),
                approx: stand.is_some() && !is_live,
            });
        }
    }
    out.sort_by_key(|d| (d.at, d.canceled));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtfs;
    use crate::rt::{decode, encode};

    fn setup() -> (Index, Vec<Visit>) {
        let want: HashSet<String> = ["200".to_string()].into();
        let (ix, v) = gtfs::build(&gtfs::tests::sample(), &want).unwrap();
        (ix, v["200"].clone())
    }

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
        zone("America/New_York").with_ymd_and_hms(y, m, d, h, mi, 0).single().unwrap().timestamp()
    }

    fn ids() -> HashSet<String> {
        ["200".to_string()].into()
    }

    #[test]
    fn the_timetable_alone() {
        let (ix, v) = setup();
        // Tuesday 27 Oct 2026, 7:50: route 42 at 8:06 to Wycombe.
        let b = board(&v, &ids(), &ix, &HashMap::new(), at(2026, 10, 27, 7, 50), 7200, &[]);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].label.as_str(), b[0].headsign.as_str(), b[0].at, b[0].live, b[0].color.as_str()), ("42", "Wycombe", at(2026, 10, 27, 8, 6), false, "0097d6"));
        // 00:05 on Wednesday: Tuesday's 24:15:00 trip.
        let b = board(&v, &ids(), &ix, &HashMap::new(), at(2026, 10, 28, 0, 5), 7200, &[]);
        assert_eq!((b.len(), b[0].at, b[0].headsign.as_str()), (1, at(2026, 10, 28, 0, 15), "To Wycombe"));
        // Saturday: the trolley (T1) only, and only when asked for every route or for it.
        let sat = at(2026, 10, 31, 9, 0);
        assert_eq!(board(&v, &ids(), &ix, &HashMap::new(), sat, 7200, &[]).iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), ["T1"]);
        assert!(board(&v, &ids(), &ix, &HashMap::new(), sat, 7200, &["42".into()]).is_empty());
        // Sunday: nothing at this stop.
        assert!(board(&v, &ids(), &ix, &HashMap::new(), at(2026, 11, 1, 7, 0), 7200, &[]).is_empty());
    }

    #[test]
    fn days_the_timetable_doesnt_cover() {
        let (ix, v) = setup();
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        // The sample runs October to December 2026. A Tuesday in January borrows the last Tuesday in it.
        assert_eq!(stand_in(&ix, d(2026, 10, 27)), None);
        assert_eq!(stand_in(&ix, d(2027, 1, 5)), Some(d(2026, 12, 29)));
        let b = board(&v, &ids(), &ix, &HashMap::new(), at(2027, 1, 5, 7, 50), 7200, &[]);
        assert_eq!((b.len(), b[0].at, b[0].approx), (1, at(2027, 1, 5, 8, 6), true));
        // A trip the live feed says runs today counts, whatever the calendar says: t1 is weekdays
        // only, but here it is on a Saturday.
        let now = at(2026, 10, 31, 7, 50);
        let rt = decode(&encode::feed(now, &[encode::trip("t1", "20261031", 0, &[encode::stop(Some(2), Some("200"), None, Some(at(2026, 10, 31, 8, 7)))])])).unwrap().trips;
        let b = board(&v, &ids(), &ix, &rt, now, 7200, &[]);
        assert!(b.iter().any(|x| x.trip == "t1" && x.live && x.at == at(2026, 10, 31, 8, 7) && !x.approx));
    }

    #[test]
    fn live_times_delays_and_cancellations() {
        let (ix, v) = setup();
        let now = at(2026, 10, 27, 7, 50);
        let feed = |trips: Vec<Vec<u8>>| decode(&encode::feed(now, &trips)).unwrap().trips;
        // An exact time for the stop (buses).
        let rt = feed(vec![encode::trip("t1", "20261027", 0, &[encode::stop(Some(2), Some("200"), None, Some(at(2026, 10, 27, 8, 10)))])]);
        let b = board(&v, &ids(), &ix, &rt, now, 7200, &[]);
        assert_eq!((b[0].at, b[0].delay, b[0].live), (at(2026, 10, 27, 8, 10), 240, true));
        // A delay at an earlier stop carries on (Regional Rail).
        let rt = feed(vec![encode::trip("t1", "", 0, &[encode::stop(Some(1), None, Some(180), None)])]);
        assert_eq!(board(&v, &ids(), &ix, &rt, now, 7200, &[])[0].at, at(2026, 10, 27, 8, 9));
        // Yesterday's update doesn't move today's trip.
        let rt = feed(vec![encode::trip("t1", "20261026", 0, &[encode::stop(Some(1), None, Some(600), None)])]);
        assert!(!board(&v, &ids(), &ix, &rt, now, 7200, &[])[0].live);
        // Canceled.
        let rt = feed(vec![encode::trip("t1", "20261027", 3, &[])]);
        assert!(board(&v, &ids(), &ix, &rt, now, 7200, &[])[0].canceled);
        // Late enough to still be coming after its time.
        let late = at(2026, 10, 27, 8, 12);
        let rt = feed(vec![encode::trip("t1", "20261027", 0, &[encode::stop(Some(2), Some("200"), None, Some(at(2026, 10, 27, 8, 20)))])]);
        assert_eq!(board(&v, &ids(), &ix, &rt, late, 7200, &[]).len(), 1);
        assert!(board(&v, &ids(), &ix, &HashMap::new(), late, 7200, &[]).is_empty());
    }

    #[test]
    fn service_days_start_at_local_noon_minus_twelve_hours() {
        let tz = zone("America/New_York");
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        assert_eq!(day_start(tz, d(2026, 10, 27)), at(2026, 10, 27, 0, 0));
        // The clocks go back at 2:00 on 1 November: noon minus 12 hours is 1:00 EDT... 23:00 the night before in EST terms.
        assert_eq!(day_start(tz, d(2026, 11, 1)) + 12 * 3600, at(2026, 11, 1, 12, 0));
        assert_eq!(zone("Not/AZone"), chrono_tz::America::New_York);
    }
}
