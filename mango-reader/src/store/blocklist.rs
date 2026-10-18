//! What the operator of a shared server keeps out of the extension host, whoever asks.
//!
//! A file, `MANGO_BLOCKLIST`, one entry a line, `#` starts a note:
//!
//! ```text
//! eu.kanade.tachiyomi.extension.en.example    an extension, by its package
//! eu.kanade.tachiyomi.extension.all.*         every package that starts so
//! example.org/extensions/                     every store whose address starts so
//! ```
//!
//! An entry with a `/` in it is a store's address, scheme left out. Mango refuses to
//! add a store or install an extension the file names, and every account loses the
//! ones it has, along with what came from a blocked store. The file is read again when
//! it changes, so the operator need not restart anything. Without the setting nothing
//! is blocked.

use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

/// How long what was read is trusted before the file's time is looked at again.
const RELOAD: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Entries {
    packages: Vec<String>,
    /// Package prefixes, from entries that end in `*`.
    prefixes: Vec<String>,
    stores: Vec<String>,
}

impl Entries {
    fn parse(text: &str) -> Self {
        let mut entries = Entries::default();
        for line in text.lines() {
            let entry = line.split('#').next().unwrap_or("").trim().to_ascii_lowercase();
            if entry.is_empty() {
                continue;
            }
            if entry.contains('/') {
                entries.stores.push(bare(&entry));
            } else if let Some(prefix) = entry.strip_suffix('*') {
                entries.prefixes.push(prefix.to_owned());
            } else {
                entries.packages.push(entry);
            }
        }
        entries
    }
}

/// An address as the list compares it: in lower case, without its scheme.
fn bare(url: &str) -> String {
    let url = url.trim().to_ascii_lowercase();
    match url.split_once("://") {
        Some((_, rest)) => rest.to_owned(),
        None => url,
    }
}

struct Seen {
    entries: Entries,
    stamp: Option<SystemTime>,
    looked: Option<Instant>,
    /// Goes up each time what the file says changes, so every account knows to sweep again.
    generation: u64,
}

pub struct Blocklist {
    file: Option<PathBuf>,
    seen: Mutex<Seen>,
}

impl Blocklist {
    pub fn new(file: Option<PathBuf>) -> Self {
        Blocklist { file, seen: Mutex::new(Seen { entries: Entries::default(), stamp: None, looked: None, generation: 0 }) }
    }

    fn read<T>(&self, look: impl FnOnce(&Entries, u64) -> T) -> T {
        let mut seen = self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = &self.file {
            if seen.looked.is_none_or(|at| at.elapsed() >= RELOAD) {
                seen.looked = Some(Instant::now());
                // A Kubernetes config map's file is a link that is pointed at a new copy on a change.
                let stamp = std::fs::metadata(file).and_then(|meta| meta.modified()).ok();
                if stamp != seen.stamp {
                    match std::fs::read_to_string(file) {
                        Ok(text) => seen.entries = Entries::parse(&text),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => seen.entries = Entries::default(),
                        // What could not be read stays as it was: a passing fault opens nothing.
                        Err(e) => {
                            tracing::warn!("cannot read the blocklist {}: {e}", file.display());
                            return look(&seen.entries, seen.generation);
                        }
                    }
                    seen.stamp = stamp;
                    seen.generation += 1;
                }
            }
        }
        look(&seen.entries, seen.generation)
    }

    pub fn package(&self, pkg: &str) -> bool {
        let pkg = pkg.to_ascii_lowercase();
        self.read(|entries, _| entries.packages.contains(&pkg) || entries.prefixes.iter().any(|prefix| pkg.starts_with(prefix.as_str())))
    }

    pub fn store(&self, url: &str) -> bool {
        let url = bare(url);
        self.read(|entries, _| entries.stores.iter().any(|entry| url.starts_with(entry.as_str())))
    }

    /// Changes each time the entries do. 0 while there is no file to read.
    pub fn generation(&self) -> u64 {
        self.read(|_, generation| generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(text: &str) -> Blocklist {
        let file = std::env::temp_dir().join(format!("mango-blocklist-{}-{}", std::process::id(), text.len()));
        std::fs::write(&file, text).unwrap();
        Blocklist::new(Some(file))
    }

    #[test]
    fn packages_prefixes_and_stores() {
        let list = list("# the operator's\neu.kanade.tachiyomi.extension.en.bad\n  EU.Kanade.Tachiyomi.Extension.All.*  # a note\nhttps://Example.org/repo/\n");
        assert!(list.package("eu.kanade.tachiyomi.extension.en.bad"));
        assert!(list.package("eu.kanade.tachiyomi.extension.all.anything"));
        assert!(!list.package("eu.kanade.tachiyomi.extension.en.badder"), "a package without * is that package alone");
        assert!(!list.package("eu.kanade.tachiyomi.extension.en.good"));
        assert!(list.store("https://example.org/repo/index.min.json"));
        assert!(list.store("http://EXAMPLE.org/repo/repo.json"));
        assert!(!list.store("https://example.org/other/repo.json"));
        assert!(!list.store("https://example.net/repo/repo.json"));
    }

    #[test]
    fn no_file_blocks_nothing() {
        let none = Blocklist::new(None);
        assert!(!none.package("anything") && !none.store("https://example.org/") && none.generation() == 0);
        let missing = Blocklist::new(Some(std::env::temp_dir().join("mango-blocklist-not-there")));
        assert!(!missing.package("anything"));
    }

    #[test]
    fn a_changed_file_is_read_again() {
        let file = std::env::temp_dir().join(format!("mango-blocklist-{}-change", std::process::id()));
        std::fs::write(&file, "").unwrap();
        let list = Blocklist::new(Some(file.clone()));
        let before = list.generation();
        assert!(!list.package("eu.example.late"));
        std::fs::write(&file, "eu.example.late\n").unwrap();
        // The file's time can match the first one's within the clock's step.
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(SystemTime::now() + Duration::from_secs(2)).unwrap();
        list.seen.lock().unwrap().looked = None;
        assert!(list.package("eu.example.late"));
        assert!(list.generation() > before);
    }
}
