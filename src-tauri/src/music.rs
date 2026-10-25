//! The Command Center's Music sheet: a song's full-size cover, year, genre and Apple Music link,
//! from Apple's public iTunes catalog (no account, no key). Looked up once per song and kept for the
//! rest of the run; only the title, artist and album are sent.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;

#[derive(Serialize, Clone, Default)]
pub struct Info {
    /// 1200 × 1200 cover art (an https URL on Apple's image servers), empty when not found.
    pub cover: String,
    pub year: String,
    pub genre: String,
    /// The song on music.apple.com.
    pub link: String,
    pub album: String,
    pub track: u32,
    pub tracks: u32,
}

static CACHE: Mutex<Option<HashMap<String, Info>>> = Mutex::new(None);

fn get(url: &str) -> Result<Value, String> {
    let e = |e: windows::core::Error| format!("iTunes lookup failed: {}", e.message());
    let client = HttpClient::new().map_err(e)?;
    let _ = client.DefaultRequestHeaders().and_then(|h| h.UserAgent()).and_then(|ua| ua.TryParseAdd(&HSTRING::from("IslandShell/1.0")));
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(e)?;
    let resp = client.GetAsync(&uri).and_then(|op| op.get()).map_err(e)?;
    let status = resp.StatusCode().map_err(e)?.0;
    if !(200..300).contains(&status) {
        return Err(format!("iTunes lookup failed: HTTP {status}"));
    }
    let body = resp.Content().and_then(|c| c.ReadAsStringAsync()).and_then(|op| op.get()).map_err(e)?;
    serde_json::from_str(&body.to_string()).map_err(|e| e.to_string())
}

fn norm(s: &str) -> String {
    // "Song (feat. X) - Remastered" and "Song" should still match.
    let s = s.to_lowercase();
    let s = s.split(" (").next().unwrap_or(&s).split(" - ").next().unwrap_or(&s).to_string();
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// How well a catalog result fits the song that's playing (higher is better).
fn score(r: &Value, title: &str, artist: &str, album: &str) -> i32 {
    let f = |k: &str| norm(r[k].as_str().unwrap_or(""));
    let (t, a, al) = (norm(title), norm(artist), norm(album));
    let mut s = 0;
    if r["trackName"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(title.trim())) {
        s += 5; // exactly the title, not a remix or a live take
    } else if f("trackName") == t {
        s += 4;
    } else if f("trackName").contains(&t) || t.contains(&f("trackName")) {
        s += 2;
    }
    if !a.is_empty() && (f("artistName").contains(&a) || a.contains(&f("artistName"))) {
        s += 3;
    }
    if !album.is_empty() && r["collectionName"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(album.trim())) {
        s += 4; // the very album, not its deluxe edition
    } else if !al.is_empty() && f("collectionName") == al {
        s += 3;
    } else if !al.is_empty() && (f("collectionName").contains(&al) || al.contains(&f("collectionName"))) {
        s += 1;
    }
    s
}

pub fn info(title: &str, artist: &str, album: &str) -> Result<Info, String> {
    let key = format!("{title}\n{artist}\n{album}");
    if let Some(v) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return Ok(v.clone());
    }
    let term = crate::util::url_encode(&format!("{artist} {title}"));
    let v = get(&format!("https://itunes.apple.com/search?media=music&entity=song&limit=25&term={term}"))?;
    let results = v["results"].as_array().cloned().unwrap_or_default();
    // On a tie the earlier result wins: iTunes lists the most relevant first.
    let best = results.iter().rev().map(|r| (score(r, title, artist, album), r)).filter(|(s, _)| *s >= 5).max_by_key(|(s, _)| *s).map(|(_, r)| r);
    let out = match best {
        Some(r) => Info {
            cover: r["artworkUrl100"].as_str().map(|u| u.replace("100x100bb", "1200x1200bb")).unwrap_or_default(),
            year: r["releaseDate"].as_str().and_then(|d| d.get(..4)).unwrap_or("").to_string(),
            genre: r["primaryGenreName"].as_str().unwrap_or("").to_string(),
            link: r["trackViewUrl"].as_str().unwrap_or("").to_string(),
            album: r["collectionName"].as_str().unwrap_or("").to_string(),
            track: r["trackNumber"].as_u64().unwrap_or(0) as u32,
            tracks: r["trackCount"].as_u64().unwrap_or(0) as u32,
        },
        None => Info::default(),
    };
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, out.clone());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_right_release() {
        let single = json!({ "trackName": "Blinding Lights", "artistName": "The Weeknd", "collectionName": "Blinding Lights - Single" });
        let album = json!({ "trackName": "Blinding Lights", "artistName": "The Weeknd", "collectionName": "After Hours" });
        let other = json!({ "trackName": "Blinding Lights (Cover)", "artistName": "Someone Else", "collectionName": "Covers" });
        assert!(score(&album, "Blinding Lights", "The Weeknd", "After Hours") > score(&single, "Blinding Lights", "The Weeknd", "After Hours"));
        assert!(score(&other, "Blinding Lights", "The Weeknd", "After Hours") < 5);
        assert_eq!(norm("Song (feat. X) - Remastered 2011"), "song");
    }

    #[test]
    #[ignore = "needs the network: looks up the iTunes catalog"]
    fn live_lookup() {
        let i = info("Blinding Lights", "The Weeknd", "After Hours").unwrap();
        println!("{} {} {} {} {}/{}", i.cover, i.year, i.genre, i.link, i.track, i.tracks);
        assert!(i.cover.contains("1200x1200"));
        assert_eq!(i.album, "After Hours");
    }
}
