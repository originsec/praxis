# praxis_node_tiny_c

A pure-C implementation of the minimal Praxis node, parity-equivalent in
scope with the Rust `praxis_node_tiny` (Praxis agent + ACP sessions only).

Runtime dependencies: **libc (and libpthread)**. No external libraries
are required at runtime; AMQP 0-9-1, JSON, HTTP/1.1, and the ACP JSON-RPC
plumbing are all hand-rolled and compiled into the binary.

## Size

`make release` produces a stripped, gc-sectioned binary around **~50 KB**
on x86_64 glibc.

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
- **Plain HTTP only for the AI endpoint.** TLS is not bundled in this
  version. Point the praxis agent at an OpenAI-compatible endpoint
  reachable over HTTP — for example a local llama.cpp/ollama server, or
  a TLS-terminating reverse-proxy in front of OpenAI/Anthropic. Adding
  mbedTLS or BearSSL (statically linked) is the obvious next step.
- **OpenAI-compatible chat-completions only.** No Anthropic or Gemini
  provider plumbing. The configured `endpoint_url` should be the API
  base; the suffix `/chat/completions` is added if missing.
- **Tool streaming is text-only.** The node emits `agent_message_chunk`
  notifications for assistant text and inline `[run_command] …` /
  result blocks. It does not emit proper ACP `tool_call` / `tool_call_update`
  updates. (Functionally equivalent for end users; the UI just renders
  them inline.)
- **No reset queue, no semantic-parser queue, no event-log forwarder,
  no Lua agents, no MCP, no intercept, no terminal capability.** The
  node only advertises `Session`.
- **Single in-flight prompt per session.** Concurrent prompts on the
  same session return `-32603` until the active worker finishes.

## Layout

```
src/
├── tiny.h     — shared declarations and types
├── util.c     — logging, /dev/urandom, UUIDv4, growing buffers
├── json.c     — JSON parser + escape-aware writer (no allocation hot path)
├── http.c     — HTTP/1.1 client with chunked + SSE decoding
├── amqp.c     — AMQP 0-9-1 client (PLAIN auth, no heartbeats)
├── praxis.c   — sessions, ACP dispatch, OpenAI chat loop, run_command
└── main.c     — registration, runtime, signal handling
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
