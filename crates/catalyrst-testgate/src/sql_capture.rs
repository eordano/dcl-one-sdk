//! This is the credits-style global routing subscriber, not the comms-style
//! per-test `set_default` guard. Under cargo test's default parallelism,
//! every install/drop of a scoped `set_default` rebuilds tracing's
//! process-global callsite interest cache; a rebuild issued from a thread
//! whose default has already reverted to no-op stamps `Interest::never` onto
//! the sqlx callsites while another thread's capture is still mid-window, so
//! that capture silently records zero. A single global subscriber, installed
//! once behind `OnceLock`, routes each event to whichever thread registered
//! for it and never touches the interest cache again after that first
//! install -- safe under any number of parallel test threads.
//!
//! Both merged call sites still need the pool's query futures polled on the
//! same OS thread that called [`sql_capture`]: routing is keyed by
//! `ThreadId`, so a `#[tokio::test]` under this must stay on the default
//! current-thread flavor, never `flavor = "multi_thread"`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::ThreadId;

struct FieldCollector(String);

impl tracing::field::Visit for FieldCollector {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={:?}", field.name(), value);
    }
}

type SqlSink = Arc<Mutex<Vec<String>>>;
type SqlSinkMap = Mutex<HashMap<ThreadId, SqlSink>>;

fn sql_sinks() -> &'static SqlSinkMap {
    static SINKS: OnceLock<SqlSinkMap> = OnceLock::new();
    SINKS.get_or_init(Default::default)
}

struct RoutingSubscriber;

impl tracing::Subscriber for RoutingSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::DEBUG && metadata.target().starts_with("sqlx")
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let sink = sql_sinks()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&std::thread::current().id())
            .cloned();
        if let Some(sink) = sink {
            let mut collector = FieldCollector(String::new());
            event.record(&mut collector);
            sink.lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(collector.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

pub struct SqlCapture {
    events: SqlSink,
}

impl SqlCapture {
    pub fn count(&self) -> usize {
        self.events.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn count_containing(&self, needle: &str) -> usize {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|s| s.contains(needle))
            .count()
    }

    pub fn reset(&self) {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

impl Drop for SqlCapture {
    fn drop(&mut self) {
        sql_sinks()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&std::thread::current().id());
    }
}

pub fn sql_capture() -> SqlCapture {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        tracing::subscriber::set_global_default(RoutingSubscriber)
            .expect("catalyrst_testgate::sql_capture owns the process tracing subscriber");
        tracing::callsite::rebuild_interest_cache();
    });
    let events: SqlSink = Arc::new(Mutex::new(Vec::new()));
    sql_sinks()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(std::thread::current().id(), events.clone());
    SqlCapture { events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    fn emit_debug(summary: &str) {
        tracing::event!(target: "sqlx::query", tracing::Level::DEBUG, summary = summary);
    }

    fn emit_info(summary: &str) {
        tracing::event!(target: "sqlx::query", tracing::Level::INFO, summary = summary);
    }

    fn emit_other_target(summary: &str) {
        tracing::event!(target: "not_sqlx", tracing::Level::DEBUG, summary = summary);
    }

    #[test]
    fn captures_matching_target_and_level_only() {
        let cap = sql_capture();
        emit_debug("SELECT 1");
        emit_debug("SELECT 2 FROM widgets");
        emit_info("wrong level, ignored");
        emit_other_target("wrong target, ignored");

        assert_eq!(cap.count(), 2);
        assert_eq!(cap.count_containing("widgets"), 1);
    }

    #[test]
    fn reset_clears_without_unregistering() {
        let cap = sql_capture();
        emit_debug("SELECT 1");
        assert_eq!(cap.count(), 1);
        cap.reset();
        assert_eq!(cap.count(), 0);
        emit_debug("SELECT 2");
        assert_eq!(cap.count(), 1);
    }

    #[test]
    fn another_threads_statements_never_leak_into_this_capture() {
        let cap = sql_capture();
        let start = Arc::new(Barrier::new(2));
        let start2 = start.clone();
        let handle = std::thread::spawn(move || {
            let _other = sql_capture();
            start2.wait();
            emit_debug("SELECT from other thread");
        });
        start.wait();
        handle.join().unwrap();

        assert_eq!(
            cap.count(),
            0,
            "routing is per-ThreadId, so another thread's registration must not \
             feed this capture"
        );
    }

    #[test]
    fn drop_unregisters_this_threads_sink() {
        let cap = sql_capture();
        emit_debug("SELECT 1");
        assert_eq!(cap.count(), 1);
        drop(cap);

        emit_debug("SELECT 2, no sink registered");

        let cap2 = sql_capture();
        emit_debug("SELECT 3");
        assert_eq!(
            cap2.count(),
            1,
            "a fresh capture on the same thread must not see events emitted \
             while unregistered, nor inherit the prior capture's count"
        );
    }
}
