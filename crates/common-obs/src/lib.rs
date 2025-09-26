use std::{
    collections::HashMap,
    fmt, io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};

use axum::{routing::get, Json, Router};
use once_cell::sync::OnceCell;
use serde_json::json;
use tracing::{self, field::Visit, span};
use tracing_subscriber::{
    fmt::{self as tsfmt, format::Writer, FmtContext, FormatEvent, FormatFields, MakeWriter},
    layer::{Context, Layer, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    EnvFilter, Registry,
};

#[derive(Debug, thiserror::Error)]
pub enum ObsInitError {
    #[error("tracing subscriber already initialized")]
    AlreadyInitialized,
    #[error("failed to install tracing subscriber: {0}")]
    Install(#[from] tracing::subscriber::SetGlobalDefaultError),
}

/// Initialize observability for a service.
pub struct ObsInit;

impl ObsInit {
    /// Install a global tracing subscriber with JSON output, trace propagation,
    /// and the default metrics provider.
    pub fn init(service: &str) -> Result<(), ObsInitError> {
        let subscriber = Self::subscriber_with_writer(service, io::stderr);
        tracing::subscriber::set_global_default(subscriber).map_err(|err| {
            if tracing::dispatcher::has_been_set() {
                ObsInitError::AlreadyInitialized
            } else {
                ObsInitError::Install(err)
            }
        })
    }

    /// Build a tracing subscriber using the provided writer.
    pub fn subscriber_with_writer<W>(service: &str, writer: W) -> impl tracing::Subscriber
    where
        W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
    {
        metrics::init(service);
        let env_level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| {
            if cfg!(debug_assertions) {
                "debug".to_string()
            } else {
                "info".to_string()
            }
        });
        let env_filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(env_level))
            .unwrap_or_else(|_| EnvFilter::new("info"));

        let service_name: Arc<str> = Arc::from(service.to_string());
        let trace_layer = TraceLayer::new();
        let fmt_layer = tsfmt::layer()
            .with_ansi(false)
            .event_format(ObsJsonFormat::new(service_name.clone()))
            .with_writer(writer);

        Registry::default()
            .with(env_filter)
            .with(trace_layer)
            .with(fmt_layer)
    }
}

/// Build a simple health and info router for services.
pub fn health_router(service: &'static str) -> Router {
    let health_handler = {
        let service = service;
        get(move || async move { Json(json!({ "status": "ok", "service": service })) })
    };

    let info_handler = {
        let service = service;
        let version = env!("CARGO_PKG_VERSION");
        get(move || async move { Json(json!({ "service": service, "version": version })) })
    };

    Router::new()
        .route("/health", health_handler.clone())
        .route("/v1/health", health_handler)
        .route("/info", info_handler.clone())
        .route("/v1/info", info_handler)
}

/// Helper trait for request scoped metadata.
pub trait SpanExt {
    /// Record a request identifier on the span so that subsequent logs emit it.
    fn with_req(&self, request_id: &str);

    /// Retrieve the active trace identifier for the span.
    fn trace_id(&self) -> Option<String>;
}

impl SpanExt for tracing::Span {
    fn with_req(&self, request_id: &str) {
        if let Some(id) = self.id() {
            if let Some(state) = TRACE_STATE.get() {
                state.set_request_id(id.into_u64(), request_id);
            }
        }
    }

    fn trace_id(&self) -> Option<String> {
        self.id().and_then(|id| {
            TRACE_STATE
                .get()
                .and_then(|state| state.trace_id(id.into_u64()))
        })
    }
}

struct TraceLayer {
    state: Arc<TraceState>,
}

impl TraceLayer {
    fn new() -> Self {
        let state = TRACE_STATE
            .get_or_init(|| Arc::new(TraceState::default()))
            .clone();
        Self { state }
    }
}

impl<S> Layer<S> for TraceLayer
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &span::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span must exist");
        let trace_ctx = if let Some(parent) = span.parent() {
            parent
                .extensions()
                .get::<Arc<TraceContext>>()
                .cloned()
                .unwrap_or_else(|| Arc::new(self.state.make_context()))
        } else {
            Arc::new(self.state.make_context())
        };

        span.extensions_mut().insert(trace_ctx.clone());
        self.state.insert(id.into_u64(), trace_ctx);
    }

    fn on_close(&self, id: span::Id, _: Context<'_, S>) {
        self.state.remove(id.into_u64());
    }
}

#[derive(Default)]
struct TraceState {
    counter: AtomicU64,
    contexts: Mutex<HashMap<u64, Arc<TraceContext>>>,
}

impl TraceState {
    fn make_context(&self) -> TraceContext {
        let id = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        TraceContext::new(format!("{:016x}", id))
    }

    fn insert(&self, span_id: u64, ctx: Arc<TraceContext>) {
        let mut map = self.contexts.lock().expect("lock poisoned");
        map.insert(span_id, ctx);
    }

    fn remove(&self, span_id: u64) {
        let mut map = self.contexts.lock().expect("lock poisoned");
        map.remove(&span_id);
    }

    fn set_request_id(&self, span_id: u64, request_id: &str) {
        let ctx = {
            let map = self.contexts.lock().expect("lock poisoned");
            map.get(&span_id).cloned()
        };
        if let Some(ctx) = ctx {
            ctx.set_request_id(request_id);
        }
    }

    fn trace_id(&self, span_id: u64) -> Option<String> {
        let map = self.contexts.lock().expect("lock poisoned");
        map.get(&span_id).map(|ctx| ctx.trace_id().to_string())
    }
}

struct TraceContext {
    trace_id: String,
    request_id: RwLock<Option<String>>,
}

impl TraceContext {
    fn new(trace_id: String) -> Self {
        Self {
            trace_id,
            request_id: RwLock::new(None),
        }
    }

    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn request_id(&self) -> Option<String> {
        self.request_id.read().expect("lock poisoned").clone()
    }

    fn set_request_id(&self, value: &str) {
        let mut guard = self.request_id.write().expect("lock poisoned");
        *guard = Some(value.to_string());
    }
}

static TRACE_STATE: OnceCell<Arc<TraceState>> = OnceCell::new();

struct ObsJsonFormat {
    service: Arc<str>,
}

impl ObsJsonFormat {
    fn new(service: Arc<str>) -> Self {
        Self { service }
    }
}

impl<S, N> FormatEvent<S, N> for ObsJsonFormat
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);
        let fields = visitor.finish();

        let mut trace_id = None;
        let mut request_id = None;

        if let Some(span) = ctx.lookup_current() {
            if let Some(ctx) = find_trace_ctx(span) {
                trace_id = Some(ctx.trace_id().to_string());
                request_id = ctx.request_id();
            }
        }

        let level = metadata.level().as_str().to_ascii_lowercase();
        let mut json = JsonWriter::new(&mut writer);
        json.begin_object()?;
        json.write_string_field("level", &level)?;
        json.write_string_field("target", metadata.target())?;
        json.write_string_field("service", &self.service)?;
        match trace_id {
            Some(id) => json.write_string_field("trace_id", &id)?,
            None => json.write_null_field("trace_id")?,
        }
        match request_id {
            Some(req) => json.write_string_field("request_id", &req)?,
            None => json.write_null_field("request_id")?,
        }
        json.write_fields_object("fields", fields.iter())?;
        json.end_object()?;
        writeln!(json.writer)
    }
}

fn find_trace_ctx<'a, S>(span: SpanRef<'a, S>) -> Option<Arc<TraceContext>>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    for scope_span in span.scope().from_root() {
        if let Some(ctx) = scope_span.extensions().get::<Arc<TraceContext>>() {
            return Some(ctx.clone());
        }
    }
    None
}

#[derive(Default)]
struct JsonFieldVisitor {
    entries: Vec<(String, JsonValue)>,
}

impl JsonFieldVisitor {
    fn finish(self) -> Vec<(String, JsonValue)> {
        self.entries
    }
}

enum JsonValue {
    String(String),
    Number(String),
    Bool(bool),
}

impl JsonValue {
    fn write(&self, writer: &mut JsonWriter<'_, '_>) -> fmt::Result {
        match self {
            JsonValue::String(value) => writer.write_quoted(value),
            JsonValue::Number(value) => writer.write_raw(value),
            JsonValue::Bool(value) => writer.write_raw(if *value { "true" } else { "false" }),
        }
    }
}

impl Visit for JsonFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.entries.push((
            field.name().to_string(),
            JsonValue::String(format!("{:?}", value)),
        ));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.entries.push((
            field.name().to_string(),
            JsonValue::String(value.to_string()),
        ));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.entries
            .push((field.name().to_string(), JsonValue::Bool(value)));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.entries.push((
            field.name().to_string(),
            JsonValue::Number(value.to_string()),
        ));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.entries.push((
            field.name().to_string(),
            JsonValue::Number(value.to_string()),
        ));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.entries.push((
            field.name().to_string(),
            JsonValue::Number(value.to_string()),
        ));
    }
}

struct JsonWriter<'a, 'b> {
    writer: &'a mut Writer<'b>,
    needs_comma: bool,
}

impl<'a, 'b> JsonWriter<'a, 'b> {
    fn new(writer: &'a mut Writer<'b>) -> Self {
        Self {
            writer,
            needs_comma: false,
        }
    }

    fn begin_object(&mut self) -> fmt::Result {
        self.writer.write_char('{')
    }

    fn end_object(&mut self) -> fmt::Result {
        self.writer.write_char('}')
    }

    fn write_comma(&mut self) -> fmt::Result {
        if self.needs_comma {
            self.writer.write_char(',')?;
        }
        self.needs_comma = true;
        Ok(())
    }

    fn write_raw(&mut self, value: &str) -> fmt::Result {
        self.writer.write_str(value)
    }

    fn write_quoted(&mut self, value: &str) -> fmt::Result {
        self.writer.write_char('"')?;
        self.writer.write_str(&escape(value))?;
        self.writer.write_char('"')
    }

    fn write_string_field(&mut self, key: &str, value: &str) -> fmt::Result {
        self.write_comma()?;
        self.write_quoted(key)?;
        self.writer.write_str(":")?;
        self.write_quoted(value)
    }

    fn write_null_field(&mut self, key: &str) -> fmt::Result {
        self.write_comma()?;
        self.write_quoted(key)?;
        self.writer.write_str(":null")
    }

    fn write_fields_object<'c, I>(&mut self, key: &str, fields: I) -> fmt::Result
    where
        I: Iterator<Item = &'c (String, JsonValue)>,
    {
        self.write_comma()?;
        self.write_quoted(key)?;
        self.writer.write_str(":")?;
        self.writer.write_char('{')?;
        let mut first = true;
        for (name, value) in fields {
            if !first {
                self.writer.write_char(',')?;
            }
            first = false;
            self.write_quoted(name)?;
            self.writer.write_str(":")?;
            value.write(self)?;
        }
        self.writer.write_char('}')
    }
}

fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

mod metrics;

pub use metrics::{
    build_info, encode_prometheus as encode_prometheus_metrics, handler_latency_seconds,
    http_requests_total, msgbus_publish_total, msgbus_subscribe_total, process_uptime_seconds,
    register_counter, register_gauge, register_histogram, Counter, CounterVec, Gauge, GaugeVec,
    Histogram, HistogramVec, PROMETHEUS_CONTENT_TYPE,
};

pub fn service_name() -> Option<&'static str> {
    metrics::service_name()
}

#[macro_export]
macro_rules! histogram_observe {
    ($metric:ident, $labels:expr, $value:expr) => {{
        $crate::$metric().observe($labels, $value);
    }};
}

#[macro_export]
macro_rules! http_request_observe {
    ($route:expr, $code:expr, $value:expr) => {{
        if let Some(service) = $crate::service_name() {
            let route_ref = $route;
            let code_ref = $code;
            $crate::http_requests_total().inc(&[service, route_ref, code_ref], 1);
            $crate::handler_latency_seconds().observe(&[service, route_ref], $value);
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Once;
    use tracing::subscriber::with_default;

    static INIT: Once = Once::new();

    fn init_global() {
        INIT.call_once(|| {
            ObsInit::init("test-service").expect("init failed");
        });
    }

    #[test]
    fn init_is_idempotent() {
        init_global();
        assert!(matches!(
            ObsInit::init("test"),
            Err(ObsInitError::AlreadyInitialized)
        ));
    }

    #[test]
    fn histogram_export_writes_prometheus_fields() {
        init_global();
        let histogram = register_histogram(
            "test_histogram_seconds",
            "Test histogram output",
            &["service"],
            crate::metrics::default_buckets(),
        );
        histogram.ensure(&["test-service"]);
        histogram.observe(&["test-service"], 0.2);

        let encoded = encode_prometheus_metrics();
        assert!(encoded
            .contains("test_histogram_seconds_bucket{service=\"test-service\",le=\"0.25\"} 1"));
        assert!(encoded.contains("test_histogram_seconds_sum{service=\"test-service\"} 0.2"));
        assert!(encoded.contains("test_histogram_seconds_count{service=\"test-service\"} 1"));
    }

    #[test]
    fn json_logs_include_trace_and_request() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let make_writer = TestMakeWriter(buffer.clone());
        let subscriber = ObsInit::subscriber_with_writer("svc", make_writer);

        with_default(subscriber, || {
            let span = tracing::info_span!("request_span");
            let _guard = span.enter();
            span.with_req("req-123");
            tracing::info!(message = "hello world");
        });

        let output = {
            let guard = buffer.lock().unwrap();
            String::from_utf8(guard.clone()).expect("valid utf8")
        };

        assert!(output.contains("\"trace_id\""));
        assert!(output.contains("\"request_id\":\"req-123\""));
        assert!(output.contains("\"message\":\"hello world\""));
    }

    #[derive(Clone)]
    struct TestMakeWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for TestMakeWriter {
        type Writer = TestWriter;

        fn make_writer(&'a self) -> Self::Writer {
            TestWriter(self.0.clone())
        }
    }

    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl io::Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut guard = self.0.lock().unwrap();
            guard.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
