//! One process per library: an exclusive OS lock on `<library>/catalog.lock`, held for as long as
//! the library is open (issue #99).
//!
//! Two processes on the same library (the app and `lightcraft run --library`, two app instances,
//! two computers on a shared folder) would each keep their own catalog and `seq` counter, and
//! whichever wrote the last snapshot would silently drop the other's edits. So opening a library
//! takes the lock first, and a second opener gets [`LockError::InUse`] instead.
//!
//! - The lock is the OS's (`flock` on Unix, `LockFileEx` on Windows, via [`std::fs::File::try_lock`]):
//!   it is released explicitly when the [`LibraryLock`] is dropped and, after a crash or a kill, by
//!   the OS when the process ends. A `catalog.lock` file left behind is therefore never "stale":
//!   only a lock that's actually held refuses the open.
//! - Dropping the lock calls [`std::fs::File::unlock`] rather than relying on the handle closing.
//!   On Unix an `flock` lock belongs to the open file description and lasts while *any* descriptor
//!   on it is open; a process that another thread is spawning holds a copy of every descriptor
//!   (`O_CLOEXEC` only closes them at `execve`), so closing ours would leave the library "already
//!   open" for a moment and a re-open would be refused (issue #680). `LOCK_UN` through our
//!   descriptor frees the lock whatever copies a child still holds.
//! - Who holds it (process id, computer, program, version) is written to `catalog.lock.owner`, for
//!   the error message. It's a separate file because a Windows lock also blocks reading the
//!   locked file. It's only a hint: it may be missing or left over from a crash.
//! - File systems without lock support (some network shares): the owner note becomes a **lease**.
//!   The holder rewrites its `beat` every [`BEAT`]; a note from another process whose beat is
//!   younger than [`LEASE`] refuses the open ([`LockError::Leased`]), an older one (its program
//!   crashed or its computer went to sleep) is taken over. The library still opens with a warning:
//!   a lease is only as good as the share's view of time and files, and refusing every unlockable
//!   share would lock the user out of their own library.
//! - Browser storage (OPFS) has no file locks: see the web host for its own guard.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often a lease holder rewrites its note (file systems that can't lock).
pub const BEAT: Duration = Duration::from_secs(30);
/// A lease note whose beat is older than this is left from a program that is gone.
pub const LEASE: Duration = Duration::from_secs(120);

/// The lock file in the library directory.
pub const LOCK: &str = "catalog.lock";
/// Who holds the lock (JSON, see [`LockOwner`]).
pub const OWNER: &str = "catalog.lock.owner";

/// Who has the library open (as written in [`OWNER`]).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LockOwner {
    pub pid: u32,
    /// Computer name, if known.
    pub host: String,
    /// The program (`LightCraft`, a test binary…) and its version.
    pub program: String,
    pub version: String,
    /// When it opened the library (seconds since 1970).
    pub since: u64,
    /// When it last said it still has the library open (seconds since 1970; only on file systems
    /// that can't lock, see the module docs). 0: not a lease.
    pub beat: u64,
}

fn now_secs() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl LockOwner {
    /// This process, as `program`.
    pub fn this_process(program: &str) -> LockOwner {
        LockOwner {
            pid: std::process::id(),
            host: host_name(),
            program: program.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            since: now_secs(),
            beat: 0,
        }
    }

    /// "LightCraft 0.3.0 (process 123 on studio-mac)".
    pub fn describe(&self) -> String {
        let program = if self.program.is_empty() { "another LightCraft" } else { self.program.as_str() };
        let version = if self.version.is_empty() { String::new() } else { format!(" {}", self.version) };
        let host = if self.host.is_empty() {
            String::new()
        } else if self.host == host_name() {
            " on this computer".to_string()
        } else {
            format!(" on {}", self.host)
        };
        format!("{program}{version} (process {}{host})", self.pid)
    }
}

/// The owner note in `dir`, if there is a readable one.
fn read_owner(dir: &Path) -> Option<LockOwner> {
    std::fs::read(dir.join(OWNER)).ok().and_then(|b| serde_json::from_slice::<LockOwner>(&b).ok())
}

/// Write the owner note atomically: a reader never sees half of it.
fn write_owner(dir: &Path, owner: &LockOwner) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(owner).map_err(io::Error::other)?;
    crate::safe_file::write_atomic_nosync(&dir.join(OWNER), &json)
}

/// The computer's name, best effort (empty if unknown).
fn host_name() -> String {
    let env = ["COMPUTERNAME", "HOSTNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|h| !h.trim().is_empty()));
    env.or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process has the library open.
    #[error("This library is already open in {}. Close it there first.", .0.as_ref().map(LockOwner::describe).unwrap_or_else(|| "another LightCraft program".into()))]
    InUse(Option<LockOwner>),
    /// Another process holds the lease on a library whose file system can't lock.
    #[error(
        "This library is already open in {} (it is stored where files can't be locked, so LightCraft goes by the note the other program keeps up to date). Close it there first; if that program isn't running any more, try again in {} minutes.",
        .0.describe(),
        LEASE.as_secs() / 60
    )]
    Leased(LockOwner),
}

/// The exclusive lock on a library directory; released on drop (and by the OS if the process
/// dies).
#[derive(Debug)]
pub struct LibraryLock {
    dir: PathBuf,
    /// `None` when the file system can't lock (the library is open under a lease).
    file: Option<File>,
    held: bool,
    /// The lease's heartbeat (file systems that can't lock): stops when this is dropped.
    lease: Option<Lease>,
}

/// A heartbeat thread rewriting the owner note; dropping the sender stops it at once.
#[derive(Debug)]
struct Lease {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    owner: LockOwner,
}

impl LibraryLock {
    /// Lock `dir` (which must exist) for this process. Fails only when another process holds it
    /// (or, where the file system can't lock, holds a live lease on it).
    pub fn acquire(dir: &Path, program: &str) -> Result<LibraryLock, LockError> {
        Self::acquire_with(dir, program, false, BEAT)
    }

    /// [`Self::acquire`]; `no_os_lock` behaves as a file system that can't lock (tests), and the
    /// lease is renewed every `beat`.
    pub(crate) fn acquire_with(dir: &Path, program: &str, no_os_lock: bool, beat: Duration) -> Result<LibraryLock, LockError> {
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK));
        let file = match file {
            Ok(f) if !no_os_lock => f,
            Ok(_) => return Self::leased(dir, program, io::Error::from(io::ErrorKind::Unsupported), beat),
            Err(e) => return Self::leased(dir, program, e, beat),
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LockError::InUse(read_owner(dir))),
            Err(TryLockError::Error(e)) => return Self::leased(dir, program, e, beat),
        }
        let owner = LockOwner::this_process(program);
        if let Err(e) = write_owner(dir, &owner) {
            log::warn!("library {}: can't write {OWNER}: {e}", dir.display());
        }
        Ok(LibraryLock { dir: dir.to_path_buf(), file: Some(file), held: true, lease: None })
    }

    /// `dir` can't be locked (`why`): open it under a lease on the owner note, unless another
    /// process holds a live one.
    fn leased(dir: &Path, program: &str, why: io::Error, beat: Duration) -> Result<LibraryLock, LockError> {
        log::warn!("library {}: can't lock {LOCK} ({why}); going by the owner note instead", dir.display());
        let me = LockOwner::this_process(program);
        if let Some(other) = read_owner(dir)
            && other.beat > 0
            && !(other.pid == me.pid && other.host == me.host)
            && now_secs().saturating_sub(other.beat) < LEASE.as_secs()
        {
            return Err(LockError::Leased(other));
        }
        let owner = LockOwner { beat: now_secs(), ..me };
        if let Err(e) = write_owner(dir, &owner) {
            log::warn!("library {}: can't write {OWNER}: {e}", dir.display());
        }
        let (stop, ticks) = std::sync::mpsc::channel::<()>();
        let (note_dir, mine) = (dir.to_path_buf(), owner.clone());
        let thread = std::thread::Builder::new()
            .name("library-lease".into())
            .spawn(move || {
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = ticks.recv_timeout(beat) {
                    // another program took the library over (this one was asleep past the lease)
                    if read_owner(&note_dir).is_some_and(|o| o.pid != mine.pid || o.host != mine.host || o.since != mine.since) {
                        log::error!("library {}: another program took it over; changes made here may be lost", note_dir.display());
                        return;
                    }
                    if let Err(e) = write_owner(&note_dir, &LockOwner { beat: now_secs(), ..mine.clone() }) {
                        log::warn!("library {}: can't renew {OWNER}: {e}", note_dir.display());
                    }
                }
            })
            .map_err(|e| log::warn!("library {}: no thread to renew the lease ({e})", dir.display()))
            .ok();
        Ok(LibraryLock { dir: dir.to_path_buf(), file: None, held: false, lease: Some(Lease { stop: Some(stop), thread, owner }) })
    }

    /// The OS lock is held (`false`: the file system can't lock; opened unprotected).
    pub fn held(&self) -> bool {
        self.held
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Another descriptor on the locked file: what a process spawned by another thread holds
    /// between fork and exec (a `dup` refers to the same open file description). Tests only (the
    /// test that uses it is Unix-only: `flock` semantics).
    #[cfg(test)]
    #[cfg(unix)]
    pub(crate) fn descriptor_copy(&self) -> Option<File> {
        self.file.as_ref().and_then(|f| f.try_clone().ok())
    }
}

impl Drop for LibraryLock {
    /// Remove the owner note while still holding the lock (so it can't delete the next holder's),
    /// then release the lock explicitly. Closing the handle isn't enough on Unix: `flock` lasts
    /// while any copy of the descriptor is open, and a process another thread is spawning holds a
    /// copy of every descriptor until it execs (issue #680). If the unlock fails the handle still
    /// closes right after, which releases the lock once every copy is gone, as before.
    fn drop(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            drop(lease.stop.take());
            if let Some(t) = lease.thread.take() {
                let _ = t.join();
            }
            // only our own note: another program may have taken the library over meanwhile
            if read_owner(&self.dir).is_some_and(|o| o.pid == lease.owner.pid && o.host == lease.owner.host && o.since == lease.owner.since) {
                let _ = std::fs::remove_file(self.dir.join(OWNER));
            }
        }
        if self.held {
            let _ = std::fs::remove_file(self.dir.join(OWNER));
            if let Some(f) = &self.file
                && let Err(e) = f.unlock()
            {
                log::warn!("library {}: can't release {LOCK}: {e}", self.dir.display());
            }
        }
    }
}
