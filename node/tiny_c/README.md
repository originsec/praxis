# praxis_node_tiny_c

A pure-C implementation of the minimal Praxis node, parity-equivalent in
scope with the Rust `praxis_node_tiny` (Praxis agent + ACP sessions only).

Runtime dependencies: **libc (and libpthread)**. No external runtime
libraries are required; AMQP 0-9-1, JSON, HTTP/1.1, and the ACP JSON-RPC
plumbing are all hand-rolled. TLS is provided by [BearSSL](https://www.bearssl.org/)
(MIT-licensed) which is downloaded and statically linked at build time.

## Size

`make release` produces a stripped, gc-sectioned binary around **~230 KB**
on x86_64 glibc — roughly 50 KB of node code plus ~180 KB of statically
linked BearSSL (X.509 minimal verifier, TLS 1.2 client) and the
generated trust-anchor table for the system CA bundle.

```
$ ldd praxis_node_tiny_c
        linux-vdso.so.1
        libc.so.6
        /lib64/ld-linux-x86-64.so.2
```

(Both `linux-vdso.so.1` and `ld-linux-*.so.2` are kernel/dynamic-linker
artefacts, not real dependencies.)

## Build

```sh
make            # debug-friendly: -O2 -g
make release    # -Os, gc-sections, stripped
```

The first build:

1. Downloads `bearssl-0.6.tar.gz` from <https://www.bearssl.org/> into
   `vendor/` and extracts it. (Subsequent builds skip the download.)
2. Compiles BearSSL into `vendor/bearssl-0.6/build/libbearssl.a`.
3. Generates `src/trust_anchors.inc` from the system CA bundle (see
   `TA_PEM` in the Makefile — it auto-detects
   `/etc/ssl/certs/ca-certificates.crt`,
   `/etc/ca-certificates/extracted/tls-ca-bundle.pem`, or
   `/etc/pki/tls/certs/ca-bundle.crt`). Override the path with
   `make TA_PEM=/custom/cabundle.pem`.
4. Links the node binary against the BearSSL static library.

`make distclean` wipes the vendored BearSSL tree so the next build
re-downloads and rebuilds from scratch.

Build prerequisites beyond a C compiler + GNU make: `curl` and `tar`
(only on first build, to fetch BearSSL).

## Run

The node looks for the broker URL in `PRAXIS_RABBITMQ_URL`
(`amqp://praxis:praxis@localhost:5672/` if unset) and persists its
node id in `~/.local/share/praxis/node_id`.

```sh
PRAXIS_RABBITMQ_URL=amqp://praxis:praxis@localhost:5672/ ./praxis_node_tiny_c
```

Use `make` (debug build) for verbose tracing — `LOG_DEBUG` is compiled
out of `make release` entirely, along with all assertions and unwind
tables.

## Limitations vs the Rust tiny node

- **Linux only.** Uses `/dev/urandom`, `gethostname(2)`, `sigaction`,
  `select(2)`. No Windows or macOS path.
- **OpenAI-compatible chat-completions only.** No Anthropic or Gemini
  provider plumbing. The configured `endpoint_url` should be the API
  base; the suffix `/chat/completions` is added if missing. Both
  `http://` and `https://` URLs are accepted.
- **No reset queue, no semantic-parser queue, no event-log forwarder,
  no Lua agents, no MCP, no intercept, no terminal capability.** The
  node only advertises `Session`.
- **Single in-flight prompt per session.** Concurrent prompts on the
  same session return `-32603` until the active worker finishes.

## Layout

```
src/
├── tiny.h            — shared declarations and types
├── util.c            — logging, /dev/urandom, UUIDv4, growing buffers
├── json.c            — JSON parser + escape-aware writer
├── conn.h, conn.c    — plain TCP / TLS (BearSSL) transport abstraction
├── http.c            — HTTP/1.1 client with chunked + SSE decoding
├── amqp.c            — AMQP 0-9-1 client (PLAIN auth, no heartbeats)
├── praxis.c          — sessions, ACP dispatch, OpenAI chat loop, run_command
├── main.c            — registration, runtime, signal handling
└── trust_anchors.inc — generated at build time from the system CA bundle
```

## Wire-protocol notes

- AMQP heartbeats are negotiated to `0` (disabled) so the node never
  needs a separate timer thread.
- `basic.publish` writes method + content-header (no properties) +
  body frames atomically under a per-connection write mutex.
- The AMQP read loop runs on the main thread; worker threads
  (one per active prompt) write through `amqp_basic_publish` without
  contending with reads.
- The ACP outbound envelope mirrors the Rust node:
  `{ "Acp": { "node_id": ..., "client_id": ..., "json_rpc": "..." } }`
  delivered to the `NodeSignal` queue.
