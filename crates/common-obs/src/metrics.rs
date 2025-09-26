use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

const DEFAULT_BUCKETS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0];

pub const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

static REGISTRY: OnceCell<Registry> = OnceCell::new();
static SERVICE_NAME: OnceCell<&'static str> = OnceCell::new();
static PROCESS_START: OnceCell<Instant> = OnceCell::new();

const BUILD_SHA: &str = match option_env!("BUILD_SHA") {
    Some(value) => value,
    None => "dev",
};
const BUILD_TIME: &str = match option_env!("BUILD_TIME") {
    Some(value) => value,
    None => "1970-01-01T00:00:00Z",
};
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn init(service: &str) {
    if SERVICE_NAME.get().is_some() {
        return;
    }

    let leaked = Box::leak(service.to_string().into_boxed_str());
    SERVICE_NAME.set(leaked).ok();
    PROCESS_START.get_or_init(Instant::now);

    build_info().set(&[leaked, PACKAGE_VERSION, BUILD_SHA, BUILD_TIME], 1.0);
    process_uptime_seconds().ensure(&[leaked]);
    http_requests_total().ensure(&[leaked, "/metrics", "200"]);
    handler_latency_seconds().ensure(&[leaked, "/metrics"]);
    msgbus_publish_total().ensure(&[leaked, ""]);
    msgbus_subscribe_total().ensure(&[leaked, ""]);
}

pub fn service_name() -> Option<&'static str> {
    SERVICE_NAME.get().copied()
}

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::default)
}

#[derive(Default)]
struct Registry {
    families: RwLock<Vec<MetricFamily>>,
}

impl Registry {
    fn register_counter(&self, counter: Arc<CounterVecInner>) {
        let mut guard = self.families.write().expect("lock poisoned");
        if guard
            .iter()
            .any(|family| matches!(family, MetricFamily::Counter(existing) if existing.name == counter.name))
        {
            return;
        }
        guard.push(MetricFamily::Counter(counter));
    }

    fn register_gauge(&self, gauge: Arc<GaugeVecInner>) {
        let mut guard = self.families.write().expect("lock poisoned");
        if guard.iter().any(
            |family| matches!(family, MetricFamily::Gauge(existing) if existing.name == gauge.name),
        ) {
            return;
        }
        guard.push(MetricFamily::Gauge(gauge));
    }

    fn register_histogram(&self, histogram: Arc<HistogramVecInner>) {
        let mut guard = self.families.write().expect("lock poisoned");
        if guard.iter().any(|family| {
            matches!(
                family,
                MetricFamily::Histogram(existing) if existing.name == histogram.name
            )
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
                MetricFamily::Gauge(gauge) => {
                    writeln!(output, "# HELP {} {}", gauge.name, gauge.help)
                        .expect("write metrics");
                    writeln!(output, "# TYPE {} gauge", gauge.name).expect("write metrics");

                    let mut samples = gauge.collect();
                    samples.sort_by(|a, b| a.0.cmp(&b.0));
                    for (labels, value) in samples {
                        write!(output, "{}", gauge.name).expect("write metrics");
                        write_labels(&mut output, gauge.label_names, &labels);
                        writeln!(output, " {}", format_float(value)).expect("write metrics");
                    }
                }
                MetricFamily::Histogram(histogram) => {
                    writeln!(output, "# HELP {} {}", histogram.name, histogram.help)
                        .expect("write metrics");
                    writeln!(output, "# TYPE {} histogram", histogram.name).expect("write metrics");

                    let mut samples = histogram.collect();
                    samples.sort_by(|a, b| a.0.cmp(&b.0));
                    for (labels, snapshot) in samples {
                        let mut cumulative = 0;
                        for (idx, bound) in histogram.buckets.iter().enumerate() {
                            cumulative += snapshot.counts.get(idx).copied().unwrap_or(0);
                            write!(output, "{}_bucket", histogram.name).expect("write metrics");
                            let mut label_names = histogram.label_names.to_vec();
                            label_names.push("le");
                            let mut label_values = labels.clone();
                            label_values.push(format_float(*bound));
                            write_labels(&mut output, &label_names, &label_values);
                            writeln!(output, " {}", cumulative).expect("write metrics");
                        }

                        cumulative += snapshot
                            .counts
                            .get(histogram.buckets.len())
                            .copied()
                            .unwrap_or(0);
                        write!(output, "{}_bucket", histogram.name).expect("write metrics");
                        let mut label_names = histogram.label_names.to_vec();
                        label_names.push("le");
                        let mut label_values = labels.clone();
                        label_values.push("+Inf".to_string());
                        write_labels(&mut output, &label_names, &label_values);
                        writeln!(output, " {}", cumulative).expect("write metrics");

                        write!(output, "{}_sum", histogram.name).expect("write metrics");
                        write_labels(&mut output, histogram.label_names, &labels);
                        writeln!(output, " {}", format_float(snapshot.sum)).expect("write metrics");

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

enum MetricFamily {
    Counter(Arc<CounterVecInner>),
    Gauge(Arc<GaugeVecInner>),
    Histogram(Arc<HistogramVecInner>),
}

#[derive(Default)]
struct CounterValue {
    value: std::sync::atomic::AtomicU64,
}

impl CounterValue {
    fn increment(&self, amount: u64) {
        self.value
            .fetch_add(amount, std::sync::atomic::Ordering::Relaxed);
    }

    fn get(&self) -> u64 {
        self.value.load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct CounterVecInner {
    name: &'static str,
    help: &'static str,
    label_names: &'static [&'static str],
    values: Mutex<HashMap<Vec<String>, Arc<CounterValue>>>,
}

impl CounterVecInner {
    fn new(name: &'static str, help: &'static str, label_names: &'static [&'static str]) -> Self {
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
            "label value count mismatch",
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

    pub fn ensure(&self, labels: &[&str]) {
        let _ = self.inner.get_or_create(labels);
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

#[derive(Default)]
struct GaugeValue {
    value: std::sync::atomic::AtomicU64,
}

impl GaugeValue {
    fn set(&self, new_value: f64) {
        self.value
            .store(new_value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    fn get(&self) -> f64 {
        f64::from_bits(self.value.load(std::sync::atomic::Ordering::Relaxed))
    }
}

struct GaugeVecInner {
    name: &'static str,
    help: &'static str,
    label_names: &'static [&'static str],
    values: Mutex<HashMap<Vec<String>, Arc<GaugeValue>>>,
}

impl GaugeVecInner {
    fn new(name: &'static str, help: &'static str, label_names: &'static [&'static str]) -> Self {
        Self {
            name,
            help,
            label_names,
            values: Mutex::new(HashMap::new()),
        }
    }

    fn get_or_create(&self, label_values: &[&str]) -> Arc<GaugeValue> {
        assert_eq!(
            self.label_names.len(),
            label_values.len(),
            "label value count mismatch",
        );
        let mut guard = self.values.lock().expect("lock poisoned");
        let key: Vec<String> = label_values.iter().map(|value| value.to_string()).collect();
        Arc::clone(
            guard
                .entry(key)
                .or_insert_with(|| Arc::new(GaugeValue::default())),
        )
    }

    fn collect(&self) -> Vec<(Vec<String>, f64)> {
        let guard = self.values.lock().expect("lock poisoned");
        guard
            .iter()
            .map(|(labels, value)| (labels.clone(), value.get()))
            .collect()
    }
}

#[derive(Clone)]
pub struct GaugeVec {
    inner: Arc<GaugeVecInner>,
}

impl GaugeVec {
    pub fn ensure(&self, labels: &[&str]) {
        let _ = self.inner.get_or_create(labels);
    }

    pub fn set(&self, labels: &[&str], value: f64) {
        self.with_label_values(labels).set(value);
    }

    pub fn with_label_values(&self, labels: &[&str]) -> Gauge {
        Gauge {
            inner: self.inner.get_or_create(labels),
        }
    }
}

#[derive(Clone)]
pub struct Gauge {
    inner: Arc<GaugeValue>,
}

impl Gauge {
    pub fn set(&self, value: f64) {
        self.inner.set(value);
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
            "label value count mismatch",
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

    pub fn ensure(&self, labels: &[&str]) {
        let _ = self.inner.get_or_create(labels);
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
    if formatted == "-0" {
        formatted = "0".to_string();
    }
    formatted
}

#[allow(dead_code)]
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

pub fn register_gauge(
    name: &'static str,
    help: &'static str,
    label_names: &'static [&'static str],
) -> GaugeVec {
    let inner = Arc::new(GaugeVecInner::new(name, help, label_names));
    registry().register_gauge(inner.clone());
    GaugeVec { inner }
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
    if let (Some(service), Some(start)) = (service_name(), PROCESS_START.get()) {
        let uptime = start.elapsed().as_secs_f64();
        process_uptime_seconds().set(&[service], uptime);
    }

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

static PROCESS_UPTIME_SECONDS: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge(
        "process_uptime_seconds",
        "Process uptime in seconds",
        &["service"],
    )
});

static BUILD_INFO: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge(
        "build_info",
        "Build information for the running service",
        &["service", "version", "build_sha", "build_time"],
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

pub fn process_uptime_seconds() -> &'static GaugeVec {
    &PROCESS_UPTIME_SECONDS
}

pub fn build_info() -> &'static GaugeVec {
    &BUILD_INFO
}
