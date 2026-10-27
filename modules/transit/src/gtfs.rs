//! A GTFS schedule, the zip of CSV files a transit agency publishes (https://gtfs.org/schedule/):
//! the parts departures need.
//!
//! [`build`] reads the whole zip once into an [`Index`]: every stop (with the routes that serve
//! it, for search), the routes, the service calendars and the agency's time zone. The timetable
//! itself is big (SEPTA's buses: 3 million rows, 134 MB), so only the rows for the stops the user
//! saved are kept, as [`Visit`]s; [`visits`] reads them again for a stop added later.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Cursor, Read};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Stop {
    pub id: String,
    pub code: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Its station, for a platform.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
    /// A station (location_type 1) rather than a stop.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub station: bool,
    /// Route ids that stop here (for a station, at any of its platforms).
    #[serde(default)]
    pub routes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Route {
    pub short: String,
    pub long: String,
    /// "rrggbb", or empty.
    pub color: String,
    pub text: String,
    /// GTFS route_type: 0 tram, 1 subway, 2 rail, 3 bus, …
    pub kind: u16,
}

impl Route {
    /// What to call it on a chip: its number, or its name when it has none.
    pub fn label(&self) -> &str {
        if self.short.is_empty() { &self.long } else { &self.short }
    }
}

/// When a service runs: weekdays between two dates, plus and minus single dates.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Service {
    /// Monday first.
    pub days: [bool; 7],
    /// YYYYMMDD; 0 when only calendar_dates.txt names its days.
    pub start: u32,
    pub end: u32,
    pub added: Vec<u32>,
    pub removed: Vec<u32>,
}

impl Service {
    /// `date` is YYYYMMDD and `weekday` 0 for Monday.
    pub fn runs(&self, date: u32, weekday: usize) -> bool {
        if self.removed.contains(&date) {
            return false;
        }
        self.added.contains(&date) || (self.start <= date && date <= self.end && self.days.get(weekday).copied().unwrap_or(false))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Index {
    pub agency: String,
    /// The agency's time zone (IANA), which the timetable's times are in.
    pub tz: String,
    pub stops: Vec<Stop>,
    pub routes: HashMap<String, Route>,
    pub services: HashMap<String, Service>,
}

impl Index {
    /// A stop's id and, for a station, its platforms' ids.
    pub fn ids_of(&self, id: &str) -> Vec<String> {
        let mut v = vec![id.to_string()];
        v.extend(self.stops.iter().filter(|s| s.parent == id).map(|s| s.id.clone()));
        v
    }
}

/// A trip calling at a stop in the timetable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Visit {
    pub trip: String,
    pub route: String,
    pub service: String,
    pub headsign: String,
    pub stop: String,
    pub seq: u32,
    /// Leaving, in seconds from the service day's start (it can pass 24:00:00).
    pub secs: u32,
}

// ---------- CSV ----------

/// One CSV line's fields, borrowed when there are no quotes (the timetable's fast path).
fn fields(line: &str) -> Vec<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;
    let line = line.trim_end_matches(['\r', '\n']);
    if !line.contains('"') {
        return line.split(',').map(Cow::Borrowed).collect();
    }
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(Cow::Owned(std::mem::take(&mut cur))),
            c => cur.push(c),
        }
    }
    out.push(Cow::Owned(cur));
    out
}

/// Reads a CSV file row by row: `each(get)` where get("column") is that column's value or "".
fn csv(r: impl Read, mut each: impl FnMut(&dyn Fn(&str) -> String)) -> Result<(), String> {
    let mut lines = BufReader::with_capacity(1 << 16, r).lines();
    let head = match lines.next() {
        Some(h) => h.map_err(|e| e.to_string())?,
        None => return Ok(()),
    };
    let cols: HashMap<String, usize> = fields(head.trim_start_matches('\u{feff}')).iter().enumerate().map(|(i, c)| (c.trim().to_string(), i)).collect();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let f = fields(&line);
        each(&|c: &str| cols.get(c).and_then(|i| f.get(*i)).map(|v| v.trim().to_string()).unwrap_or_default());
    }
    Ok(())
}

/// "25:10:00" → seconds (GTFS times pass midnight for trips that run late).
pub fn secs(t: &str) -> Option<u32> {
    let mut p = t.trim().split(':').map(|x| x.parse::<u32>().ok());
    let (h, m, s) = (p.next()??, p.next()??, p.next().flatten().unwrap_or(0));
    Some(h * 3600 + m * 60 + s)
}

// ---------- reading a feed ----------

type Zip<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn open(bytes: &[u8]) -> Result<Zip<'_>, String> {
    zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("the timetable isn't a zip: {e}"))
}

fn file<'a, 'b>(z: &'a mut Zip<'b>, name: &str) -> Result<Option<zip::read::ZipFile<'a, Cursor<&'b [u8]>>>, String> {
    // Some agencies put the files in a folder inside the zip.
    let path = (0..z.len()).find_map(|i| z.name_for_index(i).filter(|n| n.rsplit('/').next() == Some(name)).map(String::from));
    match path {
        Some(p) => z.by_name(&p).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// Takes one zip out of another (SEPTA's download holds two feeds, buses and Regional Rail).
pub fn inner(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut z = open(bytes)?;
    let mut f = file(&mut z, name)?.ok_or_else(|| format!("{name} isn't in the download"))?;
    let mut out = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

struct Trip {
    route: u32,
    service: u32,
    headsign: u32,
}

/// Interns strings into small numbers, so 3 million timetable rows don't each own their names.
#[derive(Default)]
struct Names {
    list: Vec<String>,
    of: HashMap<String, u32>,
}

impl Names {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(i) = self.of.get(s) {
            return *i;
        }
        self.list.push(s.to_string());
        self.of.insert(s.to_string(), (self.list.len() - 1) as u32);
        (self.list.len() - 1) as u32
    }
}

/// The whole index, and the timetable rows for the stops in `want` (by stop id).
pub fn build(bytes: &[u8], want: &HashSet<String>) -> Result<(Index, HashMap<String, Vec<Visit>>), String> {
    let mut z = open(bytes)?;
    let mut ix = Index::default();
    if let Some(f) = file(&mut z, "agency.txt")? {
        let mut first = true;
        csv(f, |g| {
            if first {
                ix.agency = g("agency_name");
                ix.tz = g("agency_timezone");
                first = false;
            }
        })?;
    }
    if ix.tz.is_empty() {
        return Err("The timetable has no agency.txt with a time zone, so it isn't GTFS.".into());
    }
    let f = file(&mut z, "routes.txt")?.ok_or("The timetable has no routes.txt.")?;
    csv(f, |g| {
        ix.routes.insert(g("route_id"), Route { short: g("route_short_name"), long: g("route_long_name"), color: g("route_color").to_ascii_lowercase(), text: g("route_text_color").to_ascii_lowercase(), kind: g("route_type").parse().unwrap_or(3) });
    })?;
    let f = file(&mut z, "stops.txt")?.ok_or("The timetable has no stops.txt.")?;
    csv(f, |g| {
        let kind: u8 = g("location_type").parse().unwrap_or(0);
        // Stops and stations; entrances, generic nodes and boarding areas aren't places to wait.
        if kind <= 1 {
            ix.stops.push(Stop { id: g("stop_id"), code: g("stop_code"), name: g("stop_name"), lat: g("stop_lat").parse().unwrap_or(0.0), lon: g("stop_lon").parse().unwrap_or(0.0), parent: g("parent_station"), station: kind == 1, routes: vec![] });
        }
    })?;
    if let Some(f) = file(&mut z, "calendar.txt")? {
        csv(f, |g| {
            let d = |k: &str| g(k) == "1";
            ix.services.insert(g("service_id"), Service { days: [d("monday"), d("tuesday"), d("wednesday"), d("thursday"), d("friday"), d("saturday"), d("sunday")], start: g("start_date").parse().unwrap_or(0), end: g("end_date").parse().unwrap_or(0), ..Default::default() });
        })?;
    }
    if let Some(f) = file(&mut z, "calendar_dates.txt")? {
        csv(f, |g| {
            let s = ix.services.entry(g("service_id")).or_default();
            let date = g("date").parse().unwrap_or(0);
            if g("exception_type") == "1" { s.added.push(date) } else { s.removed.push(date) }
        })?;
    }
    let mut names = Names::default();
    let mut trips: HashMap<String, Trip> = HashMap::new();
    let f = file(&mut z, "trips.txt")?.ok_or("The timetable has no trips.txt.")?;
    csv(f, |g| {
        let t = Trip { route: names.id(&g("route_id")), service: names.id(&g("service_id")), headsign: names.id(&g("trip_headsign")) };
        trips.insert(g("trip_id"), t);
    })?;

    // The timetable: which routes stop where, and the rows for the stops wanted.
    let stop_at: HashMap<String, usize> = ix.stops.iter().enumerate().map(|(i, s)| (s.id.clone(), i)).collect();
    let mut serves: Vec<Vec<u32>> = vec![vec![]; ix.stops.len()];
    let mut visits: HashMap<String, Vec<Visit>> = HashMap::new();
    let f = file(&mut z, "stop_times.txt")?.ok_or("The timetable has no stop_times.txt.")?;
    timetable(f, |trip, stop, seq, time, headsign| {
        let Some(t) = trips.get(trip) else { return };
        if let Some(&i) = stop_at.get(stop) {
            if !serves[i].contains(&t.route) {
                serves[i].push(t.route);
            }
        }
        if want.contains(stop) {
            if let Some(s) = secs(time) {
                let hs = if headsign.is_empty() { names.list[t.headsign as usize].clone() } else { headsign.to_string() };
                visits.entry(stop.to_string()).or_default().push(Visit { trip: trip.into(), route: names.list[t.route as usize].clone(), service: names.list[t.service as usize].clone(), headsign: hs, stop: stop.into(), seq, secs: s });
            }
        }
    })?;
    for (i, s) in ix.stops.iter_mut().enumerate() {
        s.routes = serves[i].iter().map(|r| names.list[*r as usize].clone()).collect();
    }
    // A station gets its platforms' routes.
    let by_parent: HashMap<String, Vec<String>> = ix.stops.iter().filter(|s| !s.parent.is_empty()).fold(HashMap::new(), |mut m, s| {
        m.entry(s.parent.clone()).or_default().extend(s.routes.iter().cloned());
        m
    });
    for s in ix.stops.iter_mut().filter(|s| s.station) {
        let mut r: Vec<String> = by_parent.get(&s.id).cloned().unwrap_or_default();
        r.sort();
        r.dedup();
        s.routes = r;
    }
    Ok((ix, visits))
}

/// Only the timetable rows for `want`, for a stop added after the index was built.
pub fn visits(bytes: &[u8], want: &HashSet<String>) -> Result<HashMap<String, Vec<Visit>>, String> {
    let mut z = open(bytes)?;
    let mut trips: HashMap<String, (String, String, String)> = HashMap::new();
    let f = file(&mut z, "trips.txt")?.ok_or("The timetable has no trips.txt.")?;
    csv(f, |g| {
        trips.insert(g("trip_id"), (g("route_id"), g("service_id"), g("trip_headsign")));
    })?;
    let mut out: HashMap<String, Vec<Visit>> = HashMap::new();
    let f = file(&mut z, "stop_times.txt")?.ok_or("The timetable has no stop_times.txt.")?;
    timetable(f, |trip, stop, seq, time, headsign| {
        if !want.contains(stop) {
            return;
        }
        if let (Some((route, service, hs)), Some(s)) = (trips.get(trip), secs(time)) {
            out.entry(stop.to_string()).or_default().push(Visit { trip: trip.into(), route: route.clone(), service: service.clone(), headsign: if headsign.is_empty() { hs.clone() } else { headsign.into() }, stop: stop.into(), seq, secs: s });
        }
    })?;
    Ok(out)
}

/// stop_times.txt row by row, quickly: (trip, stop, sequence, departure or arrival time, headsign).
fn timetable(r: impl Read, mut each: impl FnMut(&str, &str, u32, &str, &str)) -> Result<(), String> {
    let mut rd = BufReader::with_capacity(1 << 20, r);
    let mut line = String::new();
    if rd.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
        return Ok(());
    }
    let head: Vec<String> = fields(line.trim_start_matches('\u{feff}')).iter().map(|c| c.trim().to_string()).collect();
    let col = |n: &str| head.iter().position(|c| c == n);
    let (trip, stop, seq) = (col("trip_id").ok_or("stop_times.txt has no trip_id")?, col("stop_id").ok_or("stop_times.txt has no stop_id")?, col("stop_sequence").unwrap_or(usize::MAX));
    let (dep, arr, hs) = (col("departure_time").unwrap_or(usize::MAX), col("arrival_time").unwrap_or(usize::MAX), col("stop_headsign").unwrap_or(usize::MAX));
    loop {
        line.clear();
        if rd.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Ok(());
        }
        let f = fields(&line);
        let get = |i: usize| f.get(i).map(|v| v.trim()).unwrap_or("");
        let time = if get(dep).is_empty() { get(arr) } else { get(dep) };
        if time.is_empty() {
            continue;
        }
        each(get(trip), get(stop), get(seq).parse().unwrap_or(0), time, get(hs));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// A small GTFS feed: route 42 (bus) and the trolley T1, two stops and a station with a platform.
    pub fn sample() -> Vec<u8> {
        let files: &[(&str, &str)] = &[
            ("agency.txt", "\u{feff}agency_id,agency_name,agency_url,agency_timezone\n1,SEPTA,https://septa.org/,America/New_York\n"),
            ("routes.txt", "route_id,route_short_name,route_long_name,route_type,route_color,route_text_color\n42,42,Spruce-Chestnut,3,0097D6,FFFFFF\nT1,T1,\"Route 10, Lancaster\",0,5A960A,FFFFFF\n"),
            ("stops.txt", "stop_id,stop_code,stop_name,stop_lat,stop_lon,location_type,parent_station\n100,100,Chestnut St & 33rd St,39.95,-75.19,0,\n200,200,Walnut St & 34th St,39.95,-75.19,0,\nS1,,30th St Station,39.95,-75.18,1,\nP1,,30th St Station Platform A,39.95,-75.18,0,S1\nE1,,Entrance,39.95,-75.18,2,S1\n"),
            ("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nWK,1,1,1,1,1,0,0,20261001,20261231\nSA,0,0,0,0,0,1,0,20261001,20261231\n"),
            ("calendar_dates.txt", "service_id,date,exception_type\nWK,20261126,2\nSA,20261126,1\n"),
            ("trips.txt", "route_id,service_id,trip_id,trip_headsign\n42,WK,t1,Wycombe\n42,WK,t2,Wycombe\nT1,SA,t3,13th St\n"),
            ("stop_times.txt", "trip_id,arrival_time,departure_time,stop_id,stop_sequence,stop_headsign\nt1,08:00:00,08:00:00,100,1,\nt1,08:05:00,08:06:00,200,2,\nt2,24:10:00,24:10:00,100,1,\nt2,24:15:00,24:15:00,200,2,To Wycombe\nt3,09:00:00,09:00:00,P1,1,\nt3,09:10:00,09:10:00,200,2,\n"),
        ];
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, text) in files {
                w.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
                w.write_all(text.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn csv_fields() {
        assert_eq!(fields("a,b,,c\r\n"), vec!["a", "b", "", "c"]);
        assert_eq!(fields("1,\"Route 10, Lancaster\",\"say \"\"hi\"\"\""), vec!["1", "Route 10, Lancaster", "say \"hi\""]);
        assert_eq!(secs("25:10:30"), Some(90630));
        assert_eq!(secs("8:05"), Some(29100));
        assert_eq!(secs("x"), None);
    }

    #[test]
    fn builds_the_index_and_the_wanted_rows() {
        let want: HashSet<String> = ["200".to_string()].into();
        let (ix, v) = build(&sample(), &want).unwrap();
        assert_eq!((ix.agency.as_str(), ix.tz.as_str()), ("SEPTA", "America/New_York"));
        assert_eq!(ix.routes["T1"].long, "Route 10, Lancaster");
        assert_eq!(ix.routes["42"].color, "0097d6");
        // The entrance is left out; the station gets its platform's routes.
        assert_eq!(ix.stops.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["100", "200", "S1", "P1"]);
        let st = |id: &str| ix.stops.iter().find(|s| s.id == id).unwrap().clone();
        assert_eq!(st("200").routes, ["42", "T1"]);
        assert_eq!(st("S1").routes, ["T1"]);
        assert_eq!(ix.ids_of("S1"), ["S1", "P1"]);
        // Departure times; a stop's own headsign wins; past midnight stays past midnight.
        let rows = &v["200"];
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].secs, rows[0].headsign.as_str(), rows[0].seq), (8 * 3600 + 6 * 60, "Wycombe", 2));
        assert_eq!((rows[1].secs, rows[1].headsign.as_str()), (24 * 3600 + 15 * 60, "To Wycombe"));
        // Thanksgiving: no weekday service, Saturday service instead.
        assert!(!ix.services["WK"].runs(20261126, 3) && ix.services["SA"].runs(20261126, 3));
        assert!(ix.services["WK"].runs(20261127, 4) && !ix.services["WK"].runs(20270104, 0));
        // The same rows again, for a stop added later.
        assert_eq!(visits(&sample(), &want).unwrap()["200"], *rows);
    }

    #[test]
    fn a_zip_in_a_zip() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            w.start_file("google_bus.zip", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&sample()).unwrap();
            w.finish().unwrap();
        }
        let outer = buf.into_inner();
        assert!(build(&inner(&outer, "google_bus.zip").unwrap(), &HashSet::new()).is_ok());
        assert!(inner(&outer, "google_rail.zip").unwrap_err().contains("isn't in"));
        assert!(build(b"not a zip", &HashSet::new()).unwrap_err().contains("zip"));
    }
}
