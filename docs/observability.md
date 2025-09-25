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
