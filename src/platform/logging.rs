//! Logging facade. Source: `OSTPlatform/Log.cpp` + `include/Log.h`
//!
//! The C++ facade is a plain function-pointer sink installed by the host,
//! stored in a `std::atomic<Sink>` with acquire/release ordering. We use an
//! `Arc<dyn Fn>` behind an `RwLock` - reads are cheap, writes are once at
//! init, and the callback can be a closure rather than a bare fn pointer.
//!
//! Six convenience macros mirror the C++ `OSTP_LOG_TRACE`..`OSTP_LOG_ERROR`
//! family. The `ostp_log!` macro captures `file!()` / `line!()` at the call
//! site and formats the message eagerly.

use std::sync::{Arc, RwLock};

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info  = 2,
    Warn  = 3,
    Error = 4,
}

#[derive(Copy, Clone, Debug)]
pub struct Source {
    pub file: &'static str,
    pub line: u32,
    pub function: &'static str,
}

pub type Sink = Arc<dyn Fn(Level, &Source, &str) + Send + Sync + 'static>;

static G_SINK: RwLock<Option<Sink>> = RwLock::new(None);

pub fn set_sink(sink: Sink) {
    *G_SINK.write().expect("log sink lock poisoned") = Some(sink);
}

pub fn clear_sink() {
    *G_SINK.write().expect("log sink lock poisoned") = None;
}

pub fn dispatch(level: Level, src: &Source, msg: &str) {
    // Clone the Arc out of the lock so the callback runs without holding it.
    let sink = G_SINK
        .read()
        .expect("log sink lock poisoned")
        .as_ref()
        .cloned();
    if let Some(sink) = sink {
        sink(level, src, msg);
    }
}

/// Generic dispatch macro. Captures source location; formats eagerly.
#[macro_export]
macro_rules! ostp_log {
    ($level:expr, $($arg:tt)*) => {
        $crate::platform::logging::dispatch(
            $level,
            &$crate::platform::logging::Source {
                file:     file!(),
                line:     line!(),
                function: "",
            },
            &format!($($arg)*),
        )
    };
}

#[macro_export]
macro_rules! ostp_trace {
    ($($arg:tt)*) => { $crate::ostp_log!($crate::platform::logging::Level::Trace, $($arg)*) };
}

#[macro_export]
macro_rules! ostp_debug {
    ($($arg:tt)*) => { $crate::ostp_log!($crate::platform::logging::Level::Debug, $($arg)*) };
}

#[macro_export]
macro_rules! ostp_info {
    ($($arg:tt)*) => { $crate::ostp_log!($crate::platform::logging::Level::Info, $($arg)*) };
}

#[macro_export]
macro_rules! ostp_warn {
    ($($arg:tt)*) => { $crate::ostp_log!($crate::platform::logging::Level::Warn, $($arg)*) };
}

#[macro_export]
macro_rules! ostp_error {
    ($($arg:tt)*) => { $crate::ostp_log!($crate::platform::logging::Level::Error, $($arg)*) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Sink is a process-global. Serialise the tests that mutate it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn dispatch_without_sink_is_noop() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_sink();
        // Must not panic, must not do anything.
        dispatch(
            Level::Info,
            &Source { file: "x.rs", line: 1, function: "f" },
            "hello",
        );
    }

    #[test]
    fn sink_receives_messages() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_sink();

        let captured: Arc<Mutex<Vec<(Level, &'static str, String)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let cap = captured.clone();

        set_sink(Arc::new(move |lvl, src, msg| {
            cap.lock().unwrap().push((lvl, src.file, msg.to_string()));
        }));

        dispatch(
            Level::Warn,
            &Source { file: "a.rs", line: 42, function: "f" },
            "warning text",
        );

        let log = captured.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, Level::Warn);
        assert_eq!(log[0].1, "a.rs");
        assert_eq!(log[0].2, "warning text");

        clear_sink();
    }

    #[test]
    fn macro_fills_source_location() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_sink();

        let captured: Arc<Mutex<Option<(Level, &'static str, u32, String)>>> =
            Arc::new(Mutex::new(None));
        let cap = captured.clone();

        set_sink(Arc::new(move |lvl, src, msg| {
            *cap.lock().unwrap() = Some((lvl, src.file, src.line, msg.to_string()));
        }));

        ostp_info!("value = {}", 42);

        let got = captured.lock().unwrap();
        let (lvl, file, _line, msg) = got.as_ref().unwrap();
        assert_eq!(*lvl, Level::Info);
        assert!(file.ends_with("logging.rs"));
        assert_eq!(msg, "value = 42");

        clear_sink();
    }

    #[test]
    fn clears_and_reinstalls() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_sink();

        let count: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let c1 = count.clone();
        set_sink(Arc::new(move |_, _, _| { *c1.lock().unwrap() += 1; }));

        dispatch(Level::Info, &Source { file: "f", line: 1, function: "g" }, "a");
        assert_eq!(*count.lock().unwrap(), 1);

        clear_sink();
        dispatch(Level::Info, &Source { file: "f", line: 2, function: "g" }, "b");
        assert_eq!(*count.lock().unwrap(), 1); // unchanged

        let c2 = count.clone();
        set_sink(Arc::new(move |_, _, _| { *c2.lock().unwrap() += 10; }));
        dispatch(Level::Info, &Source { file: "f", line: 3, function: "g" }, "c");
        assert_eq!(*count.lock().unwrap(), 11);

        clear_sink();
    }
}
