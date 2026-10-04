use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static TRACE: OnceLock<Option<Mutex<PerfTrace>>> = OnceLock::new();

pub struct TimedSpan {
    event: &'static str,
    start: Instant,
    context: String,
    ended: bool,
}

impl TimedSpan {
    pub fn end_with(mut self, detail: impl fmt::Display) {
        write_event(
            self.event,
            self.start.elapsed().as_micros(),
            format_args!("{}{}", self.context, detail),
        );
        self.ended = true;
    }
}

impl Drop for TimedSpan {
    fn drop(&mut self) {
        if self.ended {
            return;
        }

        write_event(
            self.event,
            self.start.elapsed().as_micros(),
            format_args!("{}dropped", self.context),
        );
    }
}

struct PerfTrace {
    writer: BufWriter<File>,
    rows_since_flush: usize,
}

pub fn span(event: &'static str, context: impl fmt::Display) -> Option<TimedSpan> {
    enabled().then(|| TimedSpan {
        event,
        start: Instant::now(),
        context: context.to_string(),
        ended: false,
    })
}

pub fn event(event: &'static str, detail: impl fmt::Display) {
    write_event(event, 0, format_args!("{detail}"));
}

pub fn enabled() -> bool {
    trace().is_some()
}

fn write_event(event: &str, elapsed_us: u128, detail: fmt::Arguments<'_>) {
    let Some(trace) = trace() else {
        return;
    };

    let Ok(mut trace) = trace.lock() else {
        return;
    };

    let timestamp_us = timestamp_us();
    let _ = writeln!(
        trace.writer,
        "{timestamp_us},{event},{elapsed_us},{}",
        csv_escape(&detail.to_string())
    );
    trace.rows_since_flush += 1;

    if trace.rows_since_flush >= 32 {
        let _ = trace.writer.flush();
        trace.rows_since_flush = 0;
    }
}

fn trace() -> Option<&'static Mutex<PerfTrace>> {
    TRACE
        .get_or_init(|| {
            std::env::var_os("FRAGILE_PERF_TRACE")?;

            Some(Mutex::new(PerfTrace::new()?))
        })
        .as_ref()
}

impl PerfTrace {
    fn new() -> Option<Self> {
        let path = trace_path()?;
        Self::open(path)
    }

    fn open(path: PathBuf) -> Option<Self> {
        // Iced may already be writing to this shared trace. Unlinking it here
        // strands those handles on a different file and loses handoff evidence.
        // Profiling runners choose a fresh directory or reset before startup.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let mut writer = BufWriter::new(file);

        let _ = writeln!(writer, "timestamp_us,event,elapsed_us,detail");
        let _ = writeln!(
            writer,
            "{},trace_start,0,{}",
            timestamp_us(),
            csv_escape(&format!("path={}", path.display()))
        );

        Some(Self {
            writer,
            rows_since_flush: 0,
        })
    }
}

fn trace_path() -> Option<PathBuf> {
    let dir = std::env::var_os("FRAGILE_PERF_TRACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target").join("perf"));

    std::fs::create_dir_all(&dir).ok()?;

    Some(dir.join("fragile-perf.csv"))
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn timestamp_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_micros())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_application_trace_preserves_an_existing_renderer_writer() {
        let path = std::env::temp_dir().join(format!(
            "fragile-trace-{}-{}.csv",
            std::process::id(),
            timestamp_us()
        ));
        let mut renderer = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(renderer, "renderer_before").unwrap();
        let mut application = PerfTrace::open(path.clone()).unwrap();
        application.writer.flush().unwrap();
        writeln!(renderer, "renderer_after").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        drop(application);
        drop(renderer);
        std::fs::remove_file(path).unwrap();
        assert!(content.contains("renderer_before"));
        assert!(content.contains("trace_start"));
        assert!(content.contains("renderer_after"));
    }
}
