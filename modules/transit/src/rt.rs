//! GTFS-realtime trip updates, the protobuf a transit agency's live feed sends, decoded by hand:
//! only the fields departures need (https://gtfs.org/realtime/reference/).
//!
//! FeedMessage { header = 1 { timestamp = 3 }, entity = 2 { trip_update = 3 } }
//! TripUpdate { trip = 1, stop_time_update = 2, delay = 5 }
//! TripDescriptor { trip_id = 1, start_date = 3, schedule_relationship = 4, route_id = 5 }
//! StopTimeUpdate { stop_sequence = 1, arrival = 2, departure = 3, stop_id = 4, schedule_relationship = 5 }
//! StopTimeEvent { delay = 1, time = 2 }

use std::collections::HashMap;

/// When a vehicle reaches a stop: a delay against the timetable, an exact time, or both.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Event {
    /// Seconds late (negative: early).
    pub delay: Option<i32>,
    /// Unix seconds.
    pub time: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StopUpdate {
    pub seq: Option<u32>,
    pub stop_id: Option<String>,
    pub arrival: Option<Event>,
    pub departure: Option<Event>,
    /// The vehicle won't stop here.
    pub skipped: bool,
}

impl StopUpdate {
    /// Leaving matters more than arriving at a stop someone waits at.
    pub fn event(&self) -> Option<Event> {
        self.departure.filter(|e| e.time.is_some() || e.delay.is_some()).or(self.arrival)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TripUpdate {
    pub trip_id: String,
    pub route_id: String,
    /// YYYYMMDD, the service day the trip runs on (empty when the feed leaves it out).
    pub start_date: String,
    pub canceled: bool,
    /// The whole trip's delay, when there's nothing per stop.
    pub delay: Option<i32>,
    pub stops: Vec<StopUpdate>,
}

#[derive(Debug, Default)]
pub struct Feed {
    pub timestamp: i64,
    pub trips: HashMap<String, TripUpdate>,
}

enum Wire<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, i: 0 }
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.i).ok_or("the live feed ends in the middle of a number")?;
            self.i += 1;
            v |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(v);
            }
        }
        Err("the live feed has a number that's too long".into())
    }

    /// The next (field number, value), or None at the end.
    fn next(&mut self) -> Result<Option<(u64, Wire<'a>)>, String> {
        if self.i >= self.b.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let v = match key & 7 {
            0 => Wire::Varint(self.varint()?),
            1 | 5 => {
                self.i += if key & 7 == 1 { 8 } else { 4 };
                Wire::Fixed
            }
            2 => {
                let n = self.varint()? as usize;
                let end = self.i.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("the live feed is cut short")?;
                let s = &self.b[self.i..end];
                self.i = end;
                Wire::Bytes(s)
            }
            w => return Err(format!("the live feed isn't GTFS-realtime (wire type {w})")),
        };
        if self.i > self.b.len() {
            return Err("the live feed is cut short".into());
        }
        Ok(Some((key >> 3, v)))
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn event(b: &[u8]) -> Result<Event, String> {
    let mut e = Event::default();
    let mut r = Reader::new(b);
    while let Some((f, v)) = r.next()? {
        match (f, v) {
            // int32 and int64 travel as their 64-bit two's complement.
            (1, Wire::Varint(x)) => e.delay = Some(x as i64 as i32),
            (2, Wire::Varint(x)) => e.time = Some(x as i64),
            _ => {}
        }
    }
    Ok(e)
}

fn stop_update(b: &[u8]) -> Result<StopUpdate, String> {
    let mut s = StopUpdate::default();
    let mut r = Reader::new(b);
    while let Some((f, v)) = r.next()? {
        match (f, v) {
            (1, Wire::Varint(x)) => s.seq = Some(x as u32),
            (2, Wire::Bytes(x)) => s.arrival = Some(event(x)?),
            (3, Wire::Bytes(x)) => s.departure = Some(event(x)?),
            (4, Wire::Bytes(x)) => s.stop_id = Some(text(x)),
            (5, Wire::Varint(x)) => s.skipped = x == 1,
            _ => {}
        }
    }
    Ok(s)
}

fn trip_update(b: &[u8]) -> Result<TripUpdate, String> {
    let mut t = TripUpdate::default();
    let mut r = Reader::new(b);
    while let Some((f, v)) = r.next()? {
        match (f, v) {
            (1, Wire::Bytes(d)) => {
                let mut r = Reader::new(d);
                while let Some((f, v)) = r.next()? {
                    match (f, v) {
                        (1, Wire::Bytes(x)) => t.trip_id = text(x),
                        (3, Wire::Bytes(x)) => t.start_date = text(x),
                        // CANCELED (3) and DELETED (7).
                        (4, Wire::Varint(x)) => t.canceled = x == 3 || x == 7,
                        (5, Wire::Bytes(x)) => t.route_id = text(x),
                        _ => {}
                    }
                }
            }
            (2, Wire::Bytes(x)) => t.stops.push(stop_update(x)?),
            (5, Wire::Varint(x)) => t.delay = Some(x as i64 as i32),
            _ => {}
        }
    }
    Ok(t)
}

/// A whole live feed: its time and its trip updates by trip id (vehicle positions and alerts are
/// skipped).
pub fn decode(b: &[u8]) -> Result<Feed, String> {
    let mut feed = Feed::default();
    let mut r = Reader::new(b);
    while let Some((f, v)) = r.next()? {
        match (f, v) {
            (1, Wire::Bytes(h)) => {
                let mut r = Reader::new(h);
                while let Some((f, v)) = r.next()? {
                    if let (3, Wire::Varint(x)) = (f, v) {
                        feed.timestamp = x as i64;
                    }
                }
            }
            (2, Wire::Bytes(e)) => {
                let mut r = Reader::new(e);
                while let Some((f, v)) = r.next()? {
                    if let (3, Wire::Bytes(x)) = (f, v) {
                        let t = trip_update(x)?;
                        if !t.trip_id.is_empty() {
                            feed.trips.insert(t.trip_id.clone(), t);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(feed)
}

/// Builds GTFS-realtime bytes, for tests here and in board.rs.
#[cfg(test)]
pub mod encode {
    pub fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    pub fn num(field: u64, v: i64) -> Vec<u8> {
        let mut o = vec![];
        varint(field << 3, &mut o);
        varint(v as u64, &mut o);
        o
    }
    pub fn bytes(field: u64, b: &[u8]) -> Vec<u8> {
        let mut o = vec![];
        varint((field << 3) | 2, &mut o);
        varint(b.len() as u64, &mut o);
        o.extend_from_slice(b);
        o
    }
    /// A stop update: (sequence, stop id, delay, time) for the departure.
    pub fn stop(seq: Option<u32>, stop_id: Option<&str>, delay: Option<i32>, time: Option<i64>) -> Vec<u8> {
        let mut ev = vec![];
        if let Some(d) = delay {
            ev.extend(num(1, d as i64));
        }
        if let Some(t) = time {
            ev.extend(num(2, t));
        }
        let mut o = vec![];
        if let Some(s) = seq {
            o.extend(num(1, s as i64));
        }
        o.extend(bytes(3, &ev));
        if let Some(id) = stop_id {
            o.extend(bytes(4, id.as_bytes()));
        }
        o
    }
    pub fn trip(trip_id: &str, start_date: &str, relationship: i64, stops: &[Vec<u8>]) -> Vec<u8> {
        let mut d = bytes(1, trip_id.as_bytes());
        if !start_date.is_empty() {
            d.extend(bytes(3, start_date.as_bytes()));
        }
        d.extend(num(4, relationship));
        let mut t = bytes(1, &d);
        for s in stops {
            t.extend(bytes(2, s));
        }
        t
    }
    pub fn feed(timestamp: i64, trips: &[Vec<u8>]) -> Vec<u8> {
        let mut o = bytes(1, &[bytes(1, b"2.0"), num(3, timestamp)].concat());
        for (i, t) in trips.iter().enumerate() {
            o.extend(bytes(2, &[bytes(1, format!("e{i}").as_bytes()), bytes(3, t)].concat()));
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::encode::*;
    use super::*;

    #[test]
    fn decodes_trip_updates() {
        let b = feed(1_793_064_587, &[
            trip("957743", "20261026", 0, &[stop(Some(115), Some("2122"), None, Some(1_793_064_634)), stop(Some(116), Some("24983"), None, Some(1_793_064_644))]),
            // Regional Rail: a delay by sequence only, and a negative one.
            trip("AIR464_20261019_SID189798", "", 0, &[stop(Some(8), None, Some(-60), None)]),
            trip("958737", "20261026", 3, &[]),
        ]);
        let f = decode(&b).unwrap();
        assert_eq!(f.timestamp, 1_793_064_587);
        assert_eq!(f.trips.len(), 3);
        let t = &f.trips["957743"];
        assert_eq!((t.start_date.as_str(), t.canceled, t.stops.len()), ("20261026", false, 2));
        assert_eq!(t.stops[1].stop_id.as_deref(), Some("24983"));
        assert_eq!(t.stops[0].event().unwrap().time, Some(1_793_064_634));
        assert_eq!(f.trips["AIR464_20261019_SID189798"].stops[0].event().unwrap().delay, Some(-60));
        assert!(f.trips["958737"].canceled);
    }

    #[test]
    fn broken_feeds_are_errors_not_panics() {
        let b = feed(1, &[trip("t", "", 0, &[stop(Some(1), Some("s"), Some(30), None)])]);
        for cut in 1..b.len() {
            let _ = decode(&b[..cut]);
        }
        assert!(decode(b"<html>not a feed</html>").is_err());
        assert_eq!(decode(&[]).unwrap().trips.len(), 0);
    }
}
