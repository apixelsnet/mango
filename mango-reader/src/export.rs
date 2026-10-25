//! Export of the library as a `.tachibk`: the backup Mihon and Suwayomi read, and the
//! one `import.rs` takes in. A gzipped protobuf.
//!
//! Library titles go out with their chapters, read state, categories, history and
//! tracker links. Titles are written one at a time, so memory holds the packed file
//! and one title, never the whole library as objects.

use crate::{
    error::Result,
    import::{BackupCategory, BackupChapter, BackupHistory, BackupManga, BackupSource, BackupTracking},
};
use prost::{
    encoding::{encode_key, encode_varint, WireType},
    Message,
};
use rusqlite::Connection;
use std::{collections::HashMap, io::Write};

/// Appends a message as field `field` of the message being built in `to`.
fn nest(to: &mut Vec<u8>, field: u32, body: &[u8]) {
    encode_key(field, WireType::LengthDelimited, to);
    encode_varint(body.len() as u64, to);
    to.extend_from_slice(body);
}

/// Writes a zero that would otherwise be left out. Mihon refuses a message that lacks
/// a field it requires, and a local title's source id or an unsent link's entry id is 0.
fn zero(to: &mut Vec<u8>, field: u32) {
    encode_key(field, WireType::Varint, to);
    to.push(0);
}

fn viewer_flags(mode: Option<&str>) -> Option<i32> {
    match mode? {
        "ltr" => Some(1),
        "rtl" => Some(2),
        "vertical" => Some(3),
        "webtoon" => Some(4),
        _ => None,
    }
}

/// The library as a backup file.
pub fn write(conn: &Connection) -> Result<Vec<u8>> {
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut field = Vec::new();

    // A title names its categories by their `order`: here, the place in the list.
    let categories: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, name FROM categories ORDER BY position, id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    let place: HashMap<i64, i64> = categories.iter().enumerate().map(|(place, (id, _))| (*id, place as i64)).collect();

    let mut chapters = conn.prepare(
        "SELECT url, name, scanlator, read, bookmark, last_page_read, fetched_at, date_upload, chapter_number, source_order
         FROM chapters WHERE manga_id = ?1 AND url <> '' ORDER BY source_order",
    )?;
    let mut in_categories = conn.prepare("SELECT category_id FROM manga_categories WHERE manga_id = ?1")?;
    let mut history = conn.prepare("SELECT c.url, h.read_at FROM history h JOIN chapters c ON c.id = h.chapter_id WHERE h.manga_id = ?1")?;
    let mut tracks = conn.prepare(
        "SELECT tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, started_at, finished_at
         FROM tracks WHERE manga_id = ?1 ORDER BY tracker",
    )?;
    let mut titles = conn.prepare(
        "SELECT id, source_id, url, title, artist, author, description, genre, status, thumbnail_url, added_at, reader_mode, update_strategy
         FROM manga WHERE in_library OR merged_into IS NOT NULL ORDER BY id",
    )?;
    let mut rows = titles.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let genre: Option<String> = r.get(7)?;
        let added: Option<i64> = r.get(10)?;
        let mode: Option<String> = r.get(11)?;
        let strategy: String = r.get(12)?;
        let member_of: Vec<i64> = in_categories.query_map([id], |c| c.get(0))?.collect::<rusqlite::Result<_>>()?;
        let manga = BackupManga {
            source: r.get(1)?,
            url: r.get(2)?,
            title: r.get(3)?,
            artist: r.get(4)?,
            author: r.get(5)?,
            description: r.get(6)?,
            genre: genre.iter().flat_map(|genre| genre.split(',')).map(str::trim).filter(|genre| !genre.is_empty()).map(str::to_owned).collect(),
            status: r.get(8)?,
            // A cover set by hand lives in Mango's own folder; another app has no way to it.
            thumbnail_url: r.get::<_, Option<String>>(9)?.filter(|url| !url.starts_with(crate::titles::CUSTOM_COVER)),
            // Epoch milliseconds, like every time in a backup. Mango keeps seconds.
            date_added: added.unwrap_or(0) * 1000,
            chapters: chapters
                .query_map([id], |c| {
                    let url: String = c.get(0)?;
                    let name: String = c.get(1)?;
                    Ok(BackupChapter {
                        // Mihon requires a name.
                        name: if name.is_empty() { url.clone() } else { name },
                        url,
                        scanlator: c.get(2)?,
                        read: c.get(3)?,
                        bookmark: c.get(4)?,
                        last_page_read: c.get(5)?,
                        date_fetch: c.get::<_, i64>(6)? * 1000,
                        date_upload: c.get(7)?,
                        chapter_number: c.get::<_, f64>(8)? as f32,
                        source_order: c.get(9)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?,
            categories: member_of.iter().filter_map(|category| place.get(category).copied()).collect(),
            tracking: Vec::new(),
            favorite: Some(true),
            viewer_flags: viewer_flags(mode.as_deref()),
            history: history
                .query_map([id], |h| Ok(BackupHistory { url: h.get(0)?, last_read: h.get::<_, i64>(1)? * 1000 }))?
                .collect::<rusqlite::Result<_>>()?,
            update_strategy: i32::from(strategy == "ONLY_FETCH_ONCE"),
        };
        let mut body = manga.encode_to_vec();
        if manga.source == 0 {
            zero(&mut body, 1);
        }
        // Tracker links are added by hand, for the sake of the zero.
        let links = tracks
            .query_map([id], |t| {
                let media: i64 = t.get(2)?;
                Ok(BackupTracking {
                    sync_id: t.get(0)?,
                    library_id: t.get(1)?,
                    // The older, narrower field. Mihon reads it first when it is set.
                    media_id_int: i32::try_from(media).unwrap_or(0),
                    tracking_url: t.get(3)?,
                    title: t.get(4)?,
                    last_chapter_read: t.get::<_, f64>(5)? as f32,
                    total_chapters: t.get(6)?,
                    score: t.get::<_, f64>(7)? as f32,
                    status: t.get(8)?,
                    started_reading_date: t.get(9)?,
                    finished_reading_date: t.get(10)?,
                    media_id: media,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for link in links {
            let mut inner = link.encode_to_vec();
            if link.library_id == 0 {
                zero(&mut inner, 2);
            }
            nest(&mut body, 18, &inner);
        }
        field.clear();
        nest(&mut field, 1, &body);
        out.write_all(&field)?;
    }

    for (place, (_, name)) in categories.into_iter().enumerate() {
        field.clear();
        nest(&mut field, 2, &BackupCategory { name, order: place as i64 }.encode_to_vec());
        out.write_all(&field)?;
    }

    // The names of the sources the titles come from, so a reader can say which are missing.
    let mut sources = conn.prepare("SELECT s.id, s.name FROM sources s WHERE s.id IN (SELECT source_id FROM manga WHERE in_library OR merged_into IS NOT NULL) ORDER BY s.id")?;
    let mut rows = sources.query([])?;
    while let Some(r) = rows.next()? {
        let source = BackupSource { source_id: r.get(0)?, name: r.get(1)? };
        let mut body = source.encode_to_vec();
        if source.source_id == 0 {
            zero(&mut body, 2);
        }
        field.clear();
        nest(&mut field, 101, &body);
        out.write_all(&field)?;
    }
    Ok(out.finish()?)
}

/// Year, month and day (UTC) of a time in epoch seconds. Howard Hinnant's `civil_from_days`.
pub(crate) fn date(seconds: i64) -> (i64, i64, i64) {
    let days = seconds.div_euclid(86_400) + 719_468;
    let (era, of_era) = (days.div_euclid(146_097), days.rem_euclid(146_097));
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// What the file is called when it is saved: today's date is in it.
pub fn file_name() -> String {
    let (year, month, day) = date(crate::db::now());
    format!("mango_{year:04}-{month:02}-{day:02}.tachibk")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn
    }

    /// A library with everything a backup carries: a local title, a title from a source,
    /// one that is not in the library, categories, history and tracker links.
    fn library() -> Connection {
        let conn = database();
        crate::local::Local::register(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO manga (id, source_id, url, title, description, genre, status, initialized, in_library, added_at, reader_mode, update_strategy) VALUES
                 (1, 7, '/a', 'Alpha', 'About Alpha', 'Action, Drama', 2, 1, 1, 5000, 'webtoon', 'ONLY_FETCH_ONCE'),
                 (2, 0, 'Beta', 'Beta', NULL, NULL, 0, 1, 1, 6000, NULL, 'ALWAYS_UPDATE'),
                 (3, 7, '/c', 'Not kept', NULL, NULL, 0, 1, 0, NULL, NULL, 'ALWAYS_UPDATE');
             INSERT INTO chapters (id, manga_id, url, name, date_upload, chapter_number, scanlator, source_order, fetched_at, read, last_page_read, bookmark) VALUES
                 (1, 1, '/a/2', 'Two', 1700000000000, 2, 'Group', 0, 2000, 0, 6, 0),
                 (2, 1, '/a/1', 'One', 0, 1, NULL, 1, 2000, 1, 0, 1),
                 (3, 2, 'Beta/1.cbz', '', 0, 1.5, NULL, 0, 3000, 1, 0, 0),
                 (4, 3, '/c/1', 'Gone', 0, 1, NULL, 0, 1, 1, 0, 0);
             INSERT INTO categories (id, name, position) VALUES (10, 'Later', 1), (11, 'Reading', 0), (12, 'Empty', 2);
             INSERT INTO manga_categories (manga_id, category_id) VALUES (1, 10), (1, 11), (2, 10);
             INSERT INTO history (chapter_id, manga_id, read_at) VALUES (2, 1, 9000), (3, 2, 9500);
             INSERT INTO tracks (manga_id, tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, started_at) VALUES
                 (1, 2, 44, 99, 'https://example.org/99', 'Alpha', 1, 12, 85, 1, 1700000000000),
                 (1, 7, 0, 55099564912, '', 'Alpha', 1, 0, 7.5, 0, 0);",
        )
        .unwrap();
        conn
    }

    /// What a backup carries, as text, to compare two databases.
    fn carried(conn: &Connection) -> String {
        let mut out = String::new();
        for sql in [
            "SELECT source_id, url, title, description, genre, status, in_library, added_at, reader_mode, update_strategy FROM manga WHERE in_library ORDER BY url",
            "SELECT m.url, c.url, CASE WHEN c.name = '' THEN c.url ELSE c.name END, c.scanlator, c.read, c.last_page_read, c.bookmark, c.date_upload, c.chapter_number, c.source_order, c.fetched_at
                 FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE m.in_library ORDER BY 1, 2",
            "SELECT name FROM categories ORDER BY position",
            "SELECT m.url, c.name FROM manga_categories mc JOIN manga m ON m.id = mc.manga_id JOIN categories c ON c.id = mc.category_id ORDER BY 1, 2",
            "SELECT c.url, h.read_at FROM history h JOIN chapters c ON c.id = h.chapter_id ORDER BY 1",
            "SELECT tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, started_at FROM tracks ORDER BY 1",
        ] {
            let mut stmt = conn.prepare(sql).unwrap();
            let columns = stmt.column_count();
            let mut rows = stmt.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                for column in 0..columns {
                    out += &format!("{:?}|", row.get_ref(column).unwrap());
                }
                out.push('\n');
            }
        }
        out
    }

    #[test]
    fn what_goes_out_comes_back_the_same() {
        let mut ours = library();
        let file = write(&ours).unwrap();

        let mut fresh = database();
        let report = import::run(&mut fresh, &file, &|_, _| {}).unwrap();
        assert_eq!((report.titles_added, report.chapters_added, report.categories_added), (2, 3, 3), "only the library goes out");
        assert_eq!((report.history_entries, report.tracker_links, report.skipped), (2, 2, 0));
        assert_eq!(carried(&fresh), carried(&ours));

        // And taking its own backup in changes nothing.
        let before = carried(&ours);
        let again = import::run(&mut ours, &file, &|_, _| {}).unwrap();
        assert_eq!((again.titles_added, again.titles_merged, again.chapters_added, again.chapters_updated), (0, 2, 0, 0));
        assert_eq!((again.categories_added, again.history_entries, again.tracker_links), (0, 0, 0));
        assert_eq!(carried(&ours), before);
    }

    #[test]
    fn required_zeros_are_written() {
        /// Reads the fields Mihon requires as "set or not", which the importer's own shapes cannot tell.
        #[derive(Clone, PartialEq, Message)]
        struct Probe {
            #[prost(int64, optional, tag = "1")]
            source: Option<i64>,
            #[prost(string, optional, tag = "2")]
            url: Option<String>,
            #[prost(message, repeated, tag = "16")]
            chapters: Vec<ProbeChapter>,
            #[prost(message, repeated, tag = "18")]
            tracking: Vec<ProbeTrack>,
        }
        #[derive(Clone, PartialEq, Message)]
        struct ProbeChapter {
            #[prost(string, optional, tag = "2")]
            name: Option<String>,
        }
        #[derive(Clone, PartialEq, Message)]
        struct ProbeTrack {
            #[prost(int64, optional, tag = "2")]
            library_id: Option<i64>,
            #[prost(int32, tag = "3")]
            media_id_int: i32,
            #[prost(int64, tag = "100")]
            media_id: i64,
        }

        let data = import::unpack(&write(&library()).unwrap()).unwrap();
        let mut titles = Vec::new();
        import::fields(&data, 1, |field| {
            titles.push(Probe::decode(field).unwrap());
            Ok(())
        })
        .unwrap();
        let local = titles.iter().find(|title| title.url.as_deref() == Some("Beta")).unwrap();
        assert_eq!(local.source, Some(0), "the local source's id is written though it is zero");
        assert!(local.chapters[0].name.as_deref().is_some_and(|name| !name.is_empty()), "a chapter always has a name");
        let alpha = titles.iter().find(|title| title.url.as_deref() == Some("/a")).unwrap();
        let links: Vec<_> = alpha.tracking.iter().map(|link| (link.library_id, link.media_id_int, link.media_id)).collect();
        assert_eq!(links, [(Some(44), 99, 99), (Some(0), 0, 55_099_564_912)], "an id too wide for the old field goes in the new one only");

        let mut sources = Vec::new();
        import::fields(&data, 101, |field| {
            sources.push(import::BackupSource::decode(field).unwrap().name);
            Ok(())
        })
        .unwrap();
        assert_eq!(sources, ["Local"], "sources that are installed are named");
    }

    #[test]
    fn dates() {
        assert_eq!(date(0), (1970, 1, 1));
        assert_eq!(date(1_700_000_000), (2023, 11, 14));
        assert_eq!(date(951_782_400), (2000, 2, 29));
        assert_eq!(viewer_flags(Some("rtl")), Some(2));
        assert_eq!(viewer_flags(None), None);
    }
}
