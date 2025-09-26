# Observability

The API Gateway initializes the shared observability stack at startup via
`common-obs`, which installs a JSON tracing subscriber and lightweight in-process
metrics primitives. Logs are emitted as structured JSON with fields for service
metadata, trace identifiers, and request correlation.

## Structured logging

* Startup and shutdown events log with `event=service_start` and
  `event=service_stop` to mark lifecycle boundaries.
* Every HTTP request receives an `x-request-id`. If a caller omits the header the
  gateway generates a UUID, injects it into the request/response, and records it
  on the active tracing span. This request identifier is emitted with every log.
* Access logs capture `event=request_start` and `event=request_end` with the
  HTTP method, path, status, latency in milliseconds, remote address, and user
  agent so downstream systems can reconstruct traffic patterns.

Because the tracing layer integrates with our tracing propagation helpers, the
JSON payloads also include a `trace_id` for distributed tracing correlation.

## Metrics endpoint

The gateway exposes a Prometheus text endpoint at `GET /metrics`. The handler
exports a small static set of gauges and counters backed by the common metrics
facade:

```
# HELP process_uptime_seconds Service uptime in seconds
# TYPE process_uptime_seconds gauge
process_uptime_seconds <seconds>
# HELP api_gateway_requests_total Total HTTP requests handled
# TYPE api_gateway_requests_total counter
api_gateway_requests_total <count>
# HELP api_gateway_requests_inflight Current in-flight HTTP requests
# TYPE api_gateway_requests_inflight gauge
api_gateway_requests_inflight <count>
```

The uptime value is computed from the process start time, and the counters are
maintained by the request middleware that wraps every route.

## Diagnostics endpoints

To assist operators during incident response every control-plane service now
exposes lightweight diagnostic endpoints under the `/v1/diag/*` namespace:

* **API Gateway**
  * `GET /v1/diag/ping?target=<host>[&port=]` performs a TCP reachability probe
    and reports timing, resolution details, and errors.
  * `GET /v1/diag/routes` returns the RBAC policy surface paired with the
    required roles for each protected route along with public routes.
* **Rule Engine** – `GET /v1/diag/trace?rule_id=<id>` surfaces the most recent
  executions (up to 100) for a rule, including decision outcomes and runtime
  durations.
* **Radio Coordinator** – `GET /v1/diag/radio-map` snapshots the most recently
  applied Thread and Wi-Fi channels/configuration without contacting field
  hardware.

These endpoints are lightweight, require authentication, and integrate with the
existing observability middleware so access attempts and latencies remain
visible in logs and metrics.

## Grafana dashboards

Two curated Grafana dashboards live under `dashboards/grafana/` ready for import
into a Lokan Grafana instance:

* **Lokan Overview** (`lokan-overview.json`) brings together the top platform
  signals—request rate, aggregate error percentage, HTTP and rule-engine p95
  latency, updater health, rule engine throughput, and an optional energy
  service power-draw panel. The dashboard templatizes the `env` label so teams
  can pivot between production, staging, and on-site clusters without editing
  queries.
* **Device Registry** (`device-registry.json`) focuses on registry CRUD volume,
  subscription fan-out throughput vs. failures, and the size of the pending
  subscription backlog. The panel layout highlights saturation patterns when
  writes or fan-out begin to queue.

Exported JSON files adhere to Grafana schema v38 and assume a Prometheus data
source (`${DS_PROMETHEUS}`) is available.

## Prometheus alerting rules

Operator playbooks rely on the alert bundle in
`alerts/prometheus/observability-alerts.yaml`. The rules cover three high value
signals:

* **LokanHighErrorRate**: Pages when 5xx responses exceed 2% of request volume
  for a given service/environment pair across five minutes.
* **LokanLatencySLOBreach**: Tracks the shared 750 ms p95 HTTP latency SLO per
  service and raises a page if it is breached for ten minutes.
* **LokanMetricsScrapeMissing**: Files a ticket when any `lokan-*` scrape target
  is absent from Prometheus for at least five minutes, catching exporter or
  endpoint regressions quickly.

Drop the YAML file into an alertmanager-enabled Prometheus installation and
reload to activate the rules.
