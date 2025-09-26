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

/// Lightweight metrics facade.
pub mod metrics {
    use super::*;

    use once_cell::sync::Lazy;
    use std::fmt::Write as FmtWrite;

    const DEFAULT_BUCKETS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0];

    pub const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

    #[derive(Default)]
    struct Registry {
        families: RwLock<Vec<MetricFamily>>,
    }

    impl Registry {
        fn register_counter(&self, counter: Arc<CounterVecInner>) {
            let mut guard = self.families.write().expect("lock poisoned");
            if guard.iter().any(|family| match family {
                MetricFamily::Counter(existing) => existing.name == counter.name,
                _ => false,
            }) {
                return;
            }
            guard.push(MetricFamily::Counter(counter));
        }

        fn register_histogram(&self, histogram: Arc<HistogramVecInner>) {
            let mut guard = self.families.write().expect("lock poisoned");
            if guard.iter().any(|family| match family {
                MetricFamily::Histogram(existing) => existing.name == histogram.name,
                _ => false,
            }) {
                return;
            }
            guard.push(MetricFamily::Histogram(histogram));
        }

        fn encode(&self) -> String {
            let mut output = String::new();
            let guard = self.families.read().expect("lock poisoned");
            for family in guard.iter() {
                match family {
                    MetricFamily::Counter(counter) => {
                        writeln!(output, "# HELP {} {}", counter.name, counter.help)
                            .expect("write metrics");
                        writeln!(output, "# TYPE {} counter", counter.name).expect("write metrics");

                        let mut samples = counter.collect();
                        samples.sort_by(|a, b| a.0.cmp(&b.0));
                        for (labels, value) in samples {
                            write!(output, "{}", counter.name).expect("write metrics");
                            write_labels(&mut output, counter.label_names, &labels);
                            writeln!(output, " {}", value).expect("write metrics");
                        }
                    }
                    MetricFamily::Histogram(histogram) => {
                        writeln!(output, "# HELP {} {}", histogram.name, histogram.help)
                            .expect("write metrics");
                        writeln!(output, "# TYPE {} histogram", histogram.name)
                            .expect("write metrics");

                        let mut samples = histogram.collect();
                        samples.sort_by(|a, b| a.0.cmp(&b.0));
                        for (labels, snapshot) in samples {
                            let mut cumulative = 0u64;
                            for (idx, bound) in histogram.buckets.iter().enumerate() {
                                cumulative += snapshot.counts[idx];
                                let mut label_names = histogram.label_names.to_vec();
                                label_names.push("le");
                                let mut label_values = labels.clone();
                                label_values.push(format_float(*bound));
                                write!(output, "{}", histogram.name).expect("write metrics");
                                write_labels(&mut output, &label_names, &label_values);
                                writeln!(output, " {}", cumulative).expect("write metrics");
                            }

                            cumulative += snapshot
                                .counts
                                .get(histogram.buckets.len())
                                .copied()
                                .unwrap_or(0);
                            let mut label_names = histogram.label_names.to_vec();
                            label_names.push("le");
                            let mut label_values = labels.clone();
                            label_values.push(String::from("+Inf"));
                            write!(output, "{}", histogram.name).expect("write metrics");
                            write_labels(&mut output, &label_names, &label_values);
                            writeln!(output, " {}", cumulative).expect("write metrics");

                            write!(output, "{}_sum", histogram.name).expect("write metrics");
                            write_labels(&mut output, histogram.label_names, &labels);
                            writeln!(output, " {:.6}", snapshot.sum).expect("write metrics");

                            write!(output, "{}_count", histogram.name).expect("write metrics");
                            write_labels(&mut output, histogram.label_names, &labels);
                            writeln!(output, " {}", snapshot.count).expect("write metrics");
                        }
                    }
                }
            }

            output
        }
    }

    fn registry() -> &'static Registry {
        static REGISTRY: OnceCell<Registry> = OnceCell::new();
        REGISTRY.get_or_init(Registry::default)
    }

    enum MetricFamily {
        Counter(Arc<CounterVecInner>),
        Histogram(Arc<HistogramVecInner>),
    }

    #[derive(Default)]
    struct CounterValue {
        value: AtomicU64,
    }

    impl CounterValue {
        fn increment(&self, amount: u64) {
            self.value.fetch_add(amount, Ordering::Relaxed);
        }

        fn get(&self) -> u64 {
            self.value.load(Ordering::Relaxed)
        }
    }

    struct CounterVecInner {
        name: &'static str,
        help: &'static str,
        label_names: &'static [&'static str],
        values: Mutex<HashMap<Vec<String>, Arc<CounterValue>>>,
    }

    impl CounterVecInner {
        fn new(
            name: &'static str,
            help: &'static str,
            label_names: &'static [&'static str],
        ) -> Self {
            Self {
                name,
                help,
                label_names,
                values: Mutex::new(HashMap::new()),
            }
        }

        fn get_or_create(&self, label_values: &[&str]) -> Arc<CounterValue> {
            assert_eq!(
                self.label_names.len(),
                label_values.len(),
                "label value count mismatch"
            );
            let mut guard = self.values.lock().expect("lock poisoned");
            let key: Vec<String> = label_values.iter().map(|value| value.to_string()).collect();
            Arc::clone(
                guard
                    .entry(key)
                    .or_insert_with(|| Arc::new(CounterValue::default())),
            )
        }

        fn collect(&self) -> Vec<(Vec<String>, u64)> {
            let guard = self.values.lock().expect("lock poisoned");
            guard
                .iter()
                .map(|(labels, value)| (labels.clone(), value.get()))
                .collect()
        }
    }

    #[derive(Clone)]
    pub struct CounterVec {
        inner: Arc<CounterVecInner>,
    }

    impl CounterVec {
        pub fn with_label_values(&self, labels: &[&str]) -> Counter {
            Counter {
                inner: self.inner.get_or_create(labels),
            }
        }

        pub fn inc(&self, labels: &[&str], amount: u64) {
            self.with_label_values(labels).inc(amount);
        }
    }

    #[derive(Clone)]
    pub struct Counter {
        inner: Arc<CounterValue>,
    }

    impl Counter {
        pub fn inc(&self, amount: u64) {
            self.inner.increment(amount);
        }
    }

    struct HistogramVecInner {
        name: &'static str,
        help: &'static str,
        label_names: &'static [&'static str],
        buckets: &'static [f64],
        values: Mutex<HashMap<Vec<String>, Arc<HistogramValue>>>,
    }

    impl HistogramVecInner {
        fn new(
            name: &'static str,
            help: &'static str,
            label_names: &'static [&'static str],
            buckets: &'static [f64],
        ) -> Self {
            Self {
                name,
                help,
                label_names,
                buckets,
                values: Mutex::new(HashMap::new()),
            }
        }

        fn get_or_create(&self, label_values: &[&str]) -> Arc<HistogramValue> {
            assert_eq!(
                self.label_names.len(),
                label_values.len(),
                "label value count mismatch"
            );
            let mut guard = self.values.lock().expect("lock poisoned");
            let key: Vec<String> = label_values.iter().map(|value| value.to_string()).collect();
            Arc::clone(
                guard
                    .entry(key)
                    .or_insert_with(|| HistogramValue::new(self.buckets.len())),
            )
        }

        fn collect(&self) -> Vec<(Vec<String>, HistogramSnapshot)> {
            let guard = self.values.lock().expect("lock poisoned");
            guard
                .iter()
                .map(|(labels, value)| (labels.clone(), value.snapshot()))
                .collect()
        }
    }

    struct HistogramValue {
        state: Mutex<HistogramState>,
    }

    impl HistogramValue {
        fn new(bucket_count: usize) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(HistogramState {
                    counts: vec![0; bucket_count + 1],
                    sum: 0.0,
                    count: 0,
                }),
            })
        }

        fn observe(&self, buckets: &[f64], value: f64) {
            let mut state = self.state.lock().expect("lock poisoned");
            state.count += 1;
            state.sum += value;

            let mut idx = buckets.len();
            for (i, bound) in buckets.iter().enumerate() {
                if value <= *bound {
                    idx = i;
                    break;
                }
            }
            if let Some(slot) = state.counts.get_mut(idx) {
                *slot += 1;
            }
        }

        fn snapshot(&self) -> HistogramSnapshot {
            let state = self.state.lock().expect("lock poisoned");
            HistogramSnapshot {
                counts: state.counts.clone(),
                sum: state.sum,
                count: state.count,
            }
        }
    }

    struct HistogramState {
        counts: Vec<u64>,
        sum: f64,
        count: u64,
    }

    #[derive(Clone)]
    pub struct HistogramVec {
        inner: Arc<HistogramVecInner>,
    }

    impl HistogramVec {
        pub fn with_label_values(&self, labels: &[&str]) -> Histogram {
            Histogram {
                inner: self.inner.get_or_create(labels),
                buckets: self.inner.buckets,
            }
        }

        pub fn observe(&self, labels: &[&str], value: f64) {
            self.with_label_values(labels).observe(value);
        }
    }

    #[derive(Clone)]
    pub struct Histogram {
        inner: Arc<HistogramValue>,
        buckets: &'static [f64],
    }

    impl Histogram {
        pub fn observe(&self, value: f64) {
            self.inner.observe(self.buckets, value);
        }
    }

    #[derive(Clone)]
    struct HistogramSnapshot {
        counts: Vec<u64>,
        sum: f64,
        count: u64,
    }

    fn write_labels(output: &mut String, names: &[&str], values: &[String]) {
        if names.is_empty() {
            return;
        }

        output.push('{');
        for (idx, (name, value)) in names.iter().zip(values.iter()).enumerate() {
            if idx > 0 {
                output.push(',');
            }
            let escaped = escape_label_value(value);
            write!(output, r#"{}="{}""#, name, escaped).expect("write metrics");
        }
        output.push('}');
    }

    fn escape_label_value(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());
        for ch in value.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                _ => escaped.push(ch),
            }
        }
        escaped
    }

    fn format_float(value: f64) -> String {
        let mut formatted = format!("{value:.6}");
        while formatted.contains('.') && formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.push('0');
        }
        if formatted.is_empty() {
            formatted.push('0');
        }
        formatted
    }

    pub fn default_buckets() -> &'static [f64] {
        DEFAULT_BUCKETS
    }

    pub fn register_counter(
        name: &'static str,
        help: &'static str,
        label_names: &'static [&'static str],
    ) -> CounterVec {
        let inner = Arc::new(CounterVecInner::new(name, help, label_names));
        registry().register_counter(inner.clone());
        CounterVec { inner }
    }

    pub fn register_histogram(
        name: &'static str,
        help: &'static str,
        label_names: &'static [&'static str],
        buckets: &'static [f64],
    ) -> HistogramVec {
        let inner = Arc::new(HistogramVecInner::new(name, help, label_names, buckets));
        registry().register_histogram(inner.clone());
        HistogramVec { inner }
    }

    pub fn encode_prometheus() -> String {
        let _ = http_requests_total();
        let _ = handler_latency_seconds();
        let _ = msgbus_publish_total();
        let _ = msgbus_subscribe_total();
        registry().encode()
    }

    static HTTP_REQUESTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
        register_counter(
            "http_requests_total",
            "Total HTTP requests received",
            &["service", "route", "code"],
        )
    });

    static HANDLER_LATENCY_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
        register_histogram(
            "handler_latency_seconds",
            "HTTP handler latency in seconds",
            &["service", "route"],
            DEFAULT_BUCKETS,
        )
    });

    static MSGBUS_PUBLISH_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
        register_counter(
            "msgbus_publish_total",
            "Total messages published to the message bus",
            &["service", "subject"],
        )
    });

    static MSGBUS_SUBSCRIBE_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
        register_counter(
            "msgbus_subscribe_total",
            "Total subscriptions created on the message bus",
            &["service", "subject"],
        )
    });

    pub fn http_requests_total() -> &'static CounterVec {
        &HTTP_REQUESTS_TOTAL
    }

    pub fn handler_latency_seconds() -> &'static HistogramVec {
        &HANDLER_LATENCY_SECONDS
    }

    pub fn msgbus_publish_total() -> &'static CounterVec {
        &MSGBUS_PUBLISH_TOTAL
    }

    pub fn msgbus_subscribe_total() -> &'static CounterVec {
        &MSGBUS_SUBSCRIBE_TOTAL
    }
}

pub use metrics::{
    encode_prometheus as encode_prometheus_metrics, handler_latency_seconds, http_requests_total,
    msgbus_publish_total, msgbus_subscribe_total, register_counter, register_histogram, Counter,
    CounterVec, Histogram, HistogramVec, PROMETHEUS_CONTENT_TYPE,
};

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
