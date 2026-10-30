//! Private, atomic, revision-checked notes. No content enters error messages.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub revision: u64,
    pub updated_ms: u64,
    pub title: String,
    pub body: String,
}
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Invalid,
    Conflict,
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "Notes storage unavailable; draft preserved",
            Self::Invalid => "Invalid note data; original preserved",
            Self::Conflict => "Note changed elsewhere; draft preserved. Save as copy (Ctrl+Shift+S)",
        })
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub notes: Vec<Note>,
    pub corrupt_count: usize,
}
impl Snapshot {
    pub fn count(&self) -> usize {
        self.notes.len()
    }
    pub fn latest(&self) -> Option<Note> {
        self.notes.first().cloned()
    }
}
fn valid(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn random_id() -> Result<String, Error> {
    let mut b = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|v| format!("{v:02x}")).collect())
}
impl Store {
    pub fn discover() -> Result<Self, Error> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| Path::new(p).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or(Error::Invalid)?;
        Ok(Self::in_dir(base.join("decebal/notes")))
    }
    /// Does not touch storage. Suitable for widgets and isolated tests.
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
    fn path(&self, id: &str) -> Result<PathBuf, Error> {
        if !valid(id) {
            return Err(Error::Invalid);
        }
        Ok(self.dir.join(format!("{id}.json")))
    }
    pub fn read(&self, id: &str) -> Result<Note, Error> {
        let f = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(self.path(id)?)?;
        if !f.metadata()?.is_file() || f.metadata()?.len() > 4 * 1024 * 1024 {
            return Err(Error::Invalid);
        }
        let n: Note = serde_json::from_reader(f).map_err(|_| Error::Invalid)?;
        if n.id != id || n.revision == 0 {
            return Err(Error::Invalid);
        }
        Ok(n)
    }
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::default()),
            Err(e) => return Err(e.into()),
        };
        let mut s = Snapshot::default();
        for e in entries {
            let e = e?;
            let name = e.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_suffix(".json") {
                if valid(id) {
                    match self.read(id) {
                        Ok(n) => s.notes.push(n),
                        Err(_) => s.corrupt_count += 1,
                    }
                }
            }
        }
        s.notes.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then_with(|| b.id.cmp(&a.id)));
        Ok(s)
    }
    pub fn list(&self) -> Result<Vec<Note>, Error> {
        Ok(self.snapshot()?.notes)
    }
    pub fn latest(&self) -> Result<Option<Note>, Error> {
        Ok(self.snapshot()?.latest())
    }
    pub fn count(&self) -> Result<usize, Error> {
        Ok(self.snapshot()?.count())
    }
    fn lock(&self) -> Result<File, Error> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.dir)?;
        let meta = fs::symlink_metadata(&self.dir)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Invalid);
        }
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(self.dir.join(".lock"))?; // SAFETY: valid owned fd; flock releases with File.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(f)
    }
    pub fn create(&self, title: &str, body: &str) -> Result<Note, Error> {
        self.write(None, title, body)
    }
    pub fn update(&self, base: &Note, title: &str, body: &str) -> Result<Note, Error> {
        self.write(Some(base), title, body)
    }
    fn write(&self, base: Option<&Note>, title: &str, body: &str) -> Result<Note, Error> {
        if title.len() + body.len() > 3 * 1024 * 1024 {
            return Err(Error::Invalid);
        }
        let _lock = self.lock()?;
        let (id, revision) = if let Some(b) = base {
            let current = self.read(&b.id)?;
            if current != *b {
                return Err(Error::Conflict);
            }
            (b.id.clone(), b.revision.checked_add(1).ok_or(Error::Invalid)?)
        } else {
            (random_id()?, 1)
        };
        let note = Note { id, revision, updated_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64, title: title.into(), body: body.into() };
        let target = self.path(&note.id)?;
        if base.is_none() && target.exists() {
            return Err(Error::Conflict);
        }
        let tmp = self.dir.join(format!(".{}.tmp", random_id()?));
        let result = (|| {
            let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
            serde_json::to_writer(&mut f, &note).map_err(|_| Error::Invalid)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
            fs::rename(&tmp, target)?;
            File::open(&self.dir)?.sync_all()?;
            Ok(note)
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
}
