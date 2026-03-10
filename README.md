# fastly-compute-otel

OpenTelemetry **log and trace export** for Fastly Compute (Rust), using named log providers as transport.

This crate adapts the upstream [`opentelemetry`](https://crates.io/crates/opentelemetry) SDK for Fastly Compute's WASI environment, where the standard `opentelemetry-otlp` exporter cannot compile (it depends on `tonic`/`reqwest` which require threads and async runtimes). Instead, this crate serializes OTLP JSON and writes it to Fastly [named log endpoints](https://docs.rs/fastly/latest/fastly/log/struct.Endpoint.html), which stream the data to [any configured logging service](https://docs.fastly.com/en/guides/about-fastlys-realtime-log-streaming-features) (HTTPS, S3, Datadog, Splunk, BigQuery, Kafka, and many others).

> **Not affiliated with Fastly.** This is an independent open-source project.

## Quick start

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
fastly-compute-otel = { git = "https://github.com/case/fastly-compute-otel-rust" }
```

Then in your Compute service:

```rust
use fastly::{Error, Request, Response};
use fastly_compute_otel::FastlyOtel;

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    // Initialize OTel — creates root span from the incoming request.
    let otel = FastlyOtel::builder()
        .service_name("my-edge-app")
        .endpoint("otel-endpoint")         // Fastly named log endpoint
        .build_from_request(&req)?;

    // Backend fetch with automatic child span + traceparent injection.
    let bereq = req.clone_with_body();
    let beresp = otel.send(bereq, "origin")?;

    // Record response status on root span, flush all telemetry, return response.
    otel.finish(beresp)
}
```

## What it does

| Signal | Support | Details |
|--------|---------|---------|
| **Logs** | Yes | `LogExporter` that serializes log records to OTLP JSON and writes to a named log endpoint. Use the standard `opentelemetry` logging API. |
| **Metrics** | No | Use [`fastly-exporter`](https://github.com/fastly/fastly-exporter) for CDN platform metrics (request rates, cache hits, bandwidth, per-POP stats). |
| **Traces** | Yes | `SpanExporter` that serializes spans to OTLP JSON and writes to a named log endpoint. Automatic root span, child spans for backend fetches, W3C `traceparent` propagation. |

## Features

Both signals are enabled by default. You can enable only what you need:

```toml
# Traces only
fastly-compute-otel = { git = "...", default-features = false, features = ["trace"] }

# Logs only
fastly-compute-otel = { git = "...", default-features = false, features = ["logs"] }
```

## Fastly setup

### Named log endpoint

Your Compute service needs a named log endpoint that sends HTTPS POST requests to your OTel Collector.

**In the Fastly dashboard:**

1. Go to your service > **Logging** > **Create endpoint**
2. Choose **HTTPS** as the endpoint type
3. Set the endpoint name (e.g., `otel-endpoint`) — this must match what you pass to `.endpoint()`
4. Set the URL to your collector (e.g., `https://collector.example.com:4318`)
5. Set content type to `application/json`

**In `fastly.toml` (for local development with Viceroy):**

Named log endpoints are created dynamically by Viceroy — no configuration is needed in `fastly.toml` for local testing. Viceroy prints endpoint writes to stderr so you can see the OTLP JSON output.

## Collection setup

The named log endpoint streams raw OTLP JSON to whatever HTTPS endpoint you configure. The simplest receiver is the OpenTelemetry Collector's [`otlphttp` receiver](https://github.com/open-telemetry/opentelemetry-collector/tree/main/receiver/otlphttpreceiver):

```yaml
# otel-collector-config.yaml
receivers:
  otlphttp:
    protocols:
      http:
        endpoint: "0.0.0.0:4318"

exporters:
  # Route to any OTLP-compatible backend: SigNoz, Grafana, Datadog, etc.
  otlp:
    endpoint: "your-backend:4317"

service:
  pipelines:
    traces:
      receivers: [otlphttp]
      exporters: [otlp]
    logs:
      receivers: [otlphttp]
      exporters: [otlp]
```

Both traces and logs are written to the same named log endpoint. They use different OTLP schemas (`resourceSpans` vs `resourceLogs`), so the collector routes them to the correct pipeline automatically.

## Usage examples

### Custom spans

```rust
use opentelemetry::trace::{Tracer, TracerProvider};
use opentelemetry::KeyValue;

let tp = otel.tracer_provider().unwrap();
let tracer = tp.tracer("my-app");

let mut span = tracer.start_with_context("process-data", otel.root_context().unwrap());
span.set_attribute(KeyValue::new("item.count", 42));
// ... do work ...
span.end();
```

### Custom log records

```rust
use opentelemetry::logs::{LogRecord, Logger, LoggerProvider, Severity};

let lp = otel.logger_provider().unwrap();
let logger = lp.logger("my-app");

let mut record = logger.create_log_record();
record.set_severity_number(Severity::Info);
record.set_body("cache miss — fetching from origin".into());
record.add_attribute("url.path", "/api/data");
record.add_attribute("cache.status", "MISS");
logger.emit(record);
```

### Traces only (no logs)

```rust
let otel = FastlyOtel::builder()
    .service_name("my-edge-app")
    .trace_endpoint("otel-endpoint")
    .build_from_request(&req)?;
```

### Manual traceparent propagation

If you need more control than `otel.send()` provides:

```rust
use fastly_compute_otel::propagation;

let mut backend_req = fastly::Request::get("https://origin.example.com/api");
propagation::inject_context(otel.root_context().unwrap(), &mut backend_req);
let resp = backend_req.send("origin")?;
```

## How it works

```
Fastly Compute app (your code)
    │
    │  FastlyOtel serializes spans/logs to OTLP JSON
    │  and writes to fastly::log::Endpoint (fire-and-forget)
    ▼
Fastly named log endpoint (HTTPS streaming)
    │
    │  Fastly streams the raw JSON bytes untouched
    ▼
OTel Collector (or any OTLP-compatible receiver)
    │
    ▼
Your observability backend (SigNoz, Grafana, Datadog, etc.)
```

The named log endpoint is a **dumb pipe** — your Compute app fully controls the output format. No transformer service is needed.

## Requirements

- Rust 1.82+
- `wasm32-wasip1` target (`rustup target add wasm32-wasip1`)
- Fastly CLI and Viceroy for local development

## Development

- `bin/setup` will check for dependencies
- `bin/lint` will check for linting errors
- `bin/test` will run the tests

## License

Apache-2.0
