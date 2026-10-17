use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use parking_lot::Mutex;

use super::level::Level;
use super::state::Source;

thread_local! {
    /// Per-thread monotonic id, assigned on first log. Stable for the
    /// lifetime of the thread; unique across all threads in the process.
    /// Replaces a DefaultHasher-of-ThreadId that could collide.
    static TID: u32 = {
        static NEXT: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
}

pub struct Logger {
    file: Mutex<File>,
    level: AtomicU8,
}

impl Logger {
    pub fn open(path: &Path, level: Level) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
            level: AtomicU8::new(level as u8),
        })
    }

    pub fn set_level(&self, level: Level) {
        self.level.store(level as u8, Ordering::Relaxed);
    }

    pub fn is_enabled(&self, level: Level) -> bool {
        (level as u8) >= self.level.load(Ordering::Relaxed)
    }

    pub fn log(&self, level: Level, msg: std::fmt::Arguments<'_>, src: &Source) {
        if !self.is_enabled(level) {
            return;
        }
        let now = chrono::Local::now();
        let tid = TID.with(|c| *c);
        let mut file = self.file.lock();
        let _ = writeln!(
            file,
            "[{}] [{}] [tid={}] [{}:{}] {}",
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            level.as_str(),
            tid,
            src.file,
            src.line,
            msg,
        );
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn logger_writes_and_respects_level() {
        let dir = std::env::temp_dir().join("ost_logger_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.log");
        let l = Logger::open(&path, Level::Info).unwrap();
        l.log(Level::Debug, format_args!("hidden"), &Source { file: "f", line: 1, function: "" });
        l.log(Level::Info,  format_args!("visible"), &Source { file: "f", line: 2, function: "" });
        drop(l);
        let mut s = String::new();
        std::fs::File::open(&path).unwrap().read_to_string(&mut s).unwrap();
        assert!(!s.contains("hidden"));
        assert!(s.contains("visible"));
        assert!(s.contains("f:2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
