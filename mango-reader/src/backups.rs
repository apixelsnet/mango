//! Backups made by themselves: the library exported as a `.tachibk` on a schedule, into
//! `<data>/backups`, the newest few kept. Off until an account picks how often.

use crate::{
    db::{now, Db},
    error::{AppError, Result},
    export, settings,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// Hours between two backups. 0 or unset: none.
pub const HOURS: &str = "backup.hours";
/// How many backups are kept; an older one goes when a new one is made.
pub const KEEP: &str = "backup.keep";
const LAST_RUN: &str = "backup.lastRun";
const DEFAULT_KEEP: usize = 7;
const MOST_KEPT: usize = 60;

/// A backup on disk.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub name: String,
    pub bytes: u64,
    /// Epoch seconds.
    pub made_at: i64,
}

#[derive(Clone)]
pub struct Backups {
    db: Db,
    dir: PathBuf,
}

impl Backups {
    pub fn new(db: Db, data_dir: &Path) -> Self {
        Self { db, dir: data_dir.join("backups") }
    }

    /// Makes a backup whenever the account's interval has passed. Never returns.
    pub async fn schedule(self) {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let due = self
                .db
                .call(|conn| {
                    let hours = settings::get::<i64>(conn, HOURS).unwrap_or(0);
                    Ok(hours > 0 && settings::get::<i64>(conn, LAST_RUN).is_none_or(|last| now() - last >= hours * 3600))
                })
                .await
                .unwrap_or(false);
            if due {
                if let Err(e) = self.make().await {
                    tracing::warn!("backup: {e}");
                }
            }
        }
    }

    /// Makes a backup now, and lets go of the oldest beyond the number kept.
    pub async fn make(&self) -> Result<Saved> {
        let (file, keep) = self
            .db
            .call(|conn| {
                let keep = settings::get::<usize>(conn, KEEP).unwrap_or(DEFAULT_KEEP).clamp(1, MOST_KEPT);
                let file = export::write(conn)?;
                settings::set(conn, LAST_RUN, &now().into())?;
                Ok((file, keep))
            })
            .await?;
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || -> Result<Saved> {
            fs::create_dir_all(&dir)?;
            crate::db::owner_only(&dir);
            let made_at = now();
            let name = name_for(made_at);
            let part = dir.join(format!("{name}.part"));
            crate::db::write_private(&part, "")?;
            fs::write(&part, &file)?;
            fs::rename(&part, dir.join(&name))?;
            for old in listed(&dir).into_iter().skip(keep) {
                let _ = fs::remove_file(dir.join(old.name));
            }
            Ok(Saved { name, bytes: file.len() as u64, made_at })
        })
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
    }

    /// The backups kept, the newest first.
    pub async fn list(&self) -> Vec<Saved> {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || listed(&dir)).await.unwrap_or_default()
    }

    /// One backup's file, by its name as `list` gives it.
    pub fn file(&self, name: &str) -> Result<PathBuf> {
        let path = self.dir.join(name);
        if !proper(name) || !path.is_file() {
            return Err(AppError::not_found("No such backup"));
        }
        Ok(path)
    }

    pub async fn delete(&self, name: &str) -> Result<()> {
        let path = self.file(name)?;
        tokio::task::spawn_blocking(move || fs::remove_file(path)).await.map_err(|e| AppError::internal(e.to_string()))??;
        Ok(())
    }
}

/// `mango-20261025-093000.tachibk`, in UTC.
fn name_for(at: i64) -> String {
    let (day, second) = (at.div_euclid(86_400), at.rem_euclid(86_400));
    let (year, month, date) = civil(day);
    format!("mango-{year:04}{month:02}{date:02}-{:02}{:02}{:02}.tachibk", second / 3600, second % 3600 / 60, second % 60)
}

/// The calendar date of a day counted from 1970-01-01.
fn civil(day: i64) -> (i64, i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, date)
}

/// Whether a name is one Mango gives a backup: nothing that leads out of the folder.
fn proper(name: &str) -> bool {
    name.len() == "mango-20261025-093000.tachibk".len()
        && name.starts_with("mango-")
        && name.ends_with(".tachibk")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
}

fn listed(dir: &Path) -> Vec<Saved> {
    let mut found: Vec<Saved> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok().filter(|name| proper(name))?;
            let meta = entry.metadata().ok()?;
            let made_at = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
            Some(Saved { name, bytes: meta.len(), made_at })
        })
        .collect();
    // The name carries the time, so it sorts as the time does.
    found.sort_by(|a, b| b.name.cmp(&a.name));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_carry_the_time() {
        assert_eq!(name_for(0), "mango-19700101-000000.tachibk");
        assert_eq!(name_for(1_792_920_600), "mango-20261025-093000.tachibk");
        assert_eq!(civil(19_723), (2024, 1, 1));
        assert!(proper("mango-20261025-093000.tachibk"));
        for bad in ["../mango.db", "mango-20261025-093000.tachibk.part", "mango-2026102/-093000.tachibk", "other.tachibk"] {
            assert!(!proper(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn the_newest_are_kept() {
        let dir = std::env::temp_dir().join(format!("mango-backups-{}", now() ^ i64::from(std::process::id())));
        let db = Db::open(&dir.with_extension("db")).unwrap();
        db.call(|conn| settings::set(conn, KEEP, &2.into())).await.unwrap();
        let backups = Backups::new(db, &dir);
        fs::create_dir_all(&backups.dir).unwrap();
        for old in ["mango-20200101-000000.tachibk", "mango-20210101-000000.tachibk"] {
            fs::write(backups.dir.join(old), b"old").unwrap();
        }
        let made = backups.make().await.unwrap();
        let names: Vec<String> = backups.list().await.into_iter().map(|saved| saved.name).collect();
        assert_eq!(names, [made.name.clone(), "mango-20210101-000000.tachibk".to_owned()]);
        assert!(backups.file("../x").is_err());
        backups.delete(&made.name).await.unwrap();
        assert_eq!(backups.list().await.len(), 1);
        let _ = fs::remove_dir_all(&dir);
        for end in ["db", "db-wal", "db-shm"] {
            let _ = fs::remove_file(dir.with_extension(end));
        }
    }
}
