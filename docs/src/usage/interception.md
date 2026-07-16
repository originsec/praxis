# Interception

Traffic interception lets you see the communication between AI agents and their LLM backends. You can watch prompts being sent, responses coming back, and tool calls being made.

## How It Works

```diagram
┌─────────┐         ┌─────────────┐         ┌─────────────┐
│  Agent  │──HTTPS──│   Praxis    │──HTTPS──│   LLM API   │
│         │         │   Proxy     │         │             │
└─────────┘         └──────┬──────┘         └─────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │  Captured   │
                    │   Traffic   │
                    └─────────────┘
```

Praxis acts as a man-in-the-middle:
1. Installs a root CA certificate
2. Generates certificates for target domains
3. Terminates TLS and captures traffic
4. Re-encrypts and forwards to the real destination

## Intercept Targets

The set of domains and URL filters captured by the proxy is configured
centrally on the service and pushed to nodes — there is no per-agent
hard-coded list. The full list lives as a single **TOML virtual file**
on the service. Each `[section]` is one intercept target; the section
header is the `agent_short_name` used to route captured traffic to the
matching connector.

If multiple enabled targets contain the same domain, Praxis retains every
candidate agent instead of assigning the endpoint to whichever target was
loaded last. Traffic displays the candidates separated by `|`; agent filters
and agent-scoped rules match each candidate individually. URL capture occurs
when any matching target permits the URL.

```toml
[claudecode]
domains = ["api.anthropic.com", "a-api.anthropic.com"]
url_pattern = "messages"

[cursor]
domains = ["api.cursor.sh", "agent.api5.cursor.sh", "api2.cursor.sh", "cursor.sh"]
```

Fields per target:

| Field         | Required | Notes                                                |
|---------------|----------|------------------------------------------------------|
| `domains`     | yes      | One or more hostnames to capture for this target.    |
| `url_pattern` | no       | Optional regex matched against the request URL path. |

Section names cannot contain `|`, which is reserved for representing ambiguous
multi-agent attribution.

Lines starting with `#` are ignored. To disable a target without
deleting it, comment out the entire section. Praxis ships built-in
targets for the bundled connectors (Claude Code, Claude Desktop,
Cursor, Droid, Gemini, M365 Copilot); the defaults are seeded on first
boot.

### Managing targets

- **TUI:** Settings (`Ctrl+S`) → Intercept tab. The tab shows the
  currently-parsed targets and two actions:
  - **Edit virtual file in $EDITOR** — opens the raw TOML in your editor
    (`$VISUAL` / `$EDITOR`, falling back to `vi`/`notepad`). Save and
    exit to send the new contents to the service; parse errors are
    reported in the status bar and the stored file is left untouched.
  - **Reset to built-in defaults** — restores the file shipped with
    Praxis after a confirmation prompt.

Changes take effect immediately: the service broadcasts the parsed list
to all connected nodes. If interception is currently enabled on a node,
the new list is applied the next time interception is enabled.

## Interception Methods

Praxis supports four methods for routing traffic through the proxy. Each has tradeoffs.

### Proxy Mode

**How it works:** Configures system proxy settings so applications route HTTP/HTTPS through the Praxis proxy.

**Setup:**
- Linux: Sets `HTTP_PROXY` and `HTTPS_PROXY` environment variables
- Windows: Modifies registry proxy settings

**Advantages:**
- Easiest to set up (loopback listener only)
- Minimal system routing changes

**Disadvantages:**
- Only captures HTTP/HTTPS
- Some applications ignore proxy settings
- May conflict with existing proxy configuration
- In Praxis, enabling interception still requires a **privileged** node
  (the `Interception` capability is only advertised when running as root/admin)

**Best for:** Quick setup on a privileged node, apps that respect proxy settings

### VPN Mode

**How it works:** Creates a TUN network adapter and routes specific IPs through it at the packet level.

**Platform support:** Windows and Linux. TPROXY is the recommended Linux
default because it avoids userspace packet processing.

**Setup:**
1. Intercept domains resolved to IP addresses and conflict preflight runs
   (no system mutation yet)
2. TUN device created (wintun on Windows / Linux TUN) only after preflight
3. Routes added for resolved IPs through the TUN
4. Packet engine performs NAT to redirect to proxy
5. Proxy connects to real server, bypassing TUN via interface binding

**Internal details:**
- TUN uses IP 10.255.0.1, virtual client uses 10.255.0.100
- Packet engine maintains NAT table mapping client connections to proxy
- Proxy bypasses TUN by binding to the real network interface's IP (not 10.255.0.1)
- Packet engine distinguishes proxy traffic (src != 10.255.0.1) and passes it through

**Advantages:**
- Captures traffic from all applications
- Works even if apps ignore proxy settings
- More comprehensive coverage

**Disadvantages:**
- Requires elevated privileges (admin)
- More complex setup

**Best for:** Comprehensive capture on Windows, applications that bypass proxy

### Hosts Mode

**How it works:** Modifies the hosts file to redirect target domains to localhost where the proxy listens.

**Setup:**
- Adds entries to `/etc/hosts` (Linux) or `C:\Windows\System32\drivers\etc\hosts` (Windows)
- Flushes DNS cache

**Advantages:**
- Simple mechanism
- Works for static domains
- No packet-level complexity

**Disadvantages:**
- Requires elevated privileges
- Only works for known domains
- Doesn't handle DNS load balancing
- Applications using custom DNS may bypass

**Best for:** Simple setups with known domains

### TPROXY Mode (Linux only)

**How it works:** Uses iptables TPROXY to transparently redirect traffic to the proxy at the kernel level.

**Setup:**
1. Intercept domains resolved to IPv4 addresses before system changes
2. Policy routing configured to route marked packets to loopback
3. iptables mangle rules added for the resolved target IPs (mark 0x1)
4. TPROXY redirects matching packets to the proxy port
5. IPv6 disabled system-wide only after rule setup succeeds (restored on cleanup)
6. Proxy uses the accepted socket `local_addr` (IP_TRANSPARENT) as the real destination
7. Proxy's outbound connections use bypass mark 0x2 to skip interception rules

**Internal details:**
- Uses iptables mangle table with PREROUTING chain
- Bypass rule: `-m mark --mark 0x2 -j RETURN` placed before intercept rules
- Proxy sets `SO_MARK=0x2` on outbound sockets to avoid routing loop
- Policy routing table 100 handles marked packets

**Advantages:**
- No TUN device or userspace packet processing
- Lower overhead than VPN mode
- Standard Linux networking (works with any kernel supporting TPROXY)
- Works for all TCP traffic to target IPs

**Disadvantages:**
- Linux only
- Requires elevated privileges (root or `CAP_NET_ADMIN`)
- Modifies iptables rules (may conflict with existing firewall)
- Temporarily disables IPv6 (IPv4 only)

**Best for:** Linux systems needing efficient kernel-level interception

## Privilege Requirements

All interception methods require a privileged node in Praxis: the node only
advertises the `Interception` capability when running as root/admin. VPN,
Hosts, and TPROXY also need those privileges for system routing/cert changes;
Proxy is still gated the same way even though its listener is loopback-only.

Nodes report their privilege status automatically. In the praxis TUI, press
`i` on a non-privileged node shows an error — you must restart the node with
elevated privileges before enabling interception. Privileged nodes display a
**priv** pill in the Nodes window.

## Enabling Interception

1. Open the **Nodes** window in the praxis TUI
2. Select your node (must be running privileged — root/admin)
3. Press **`i`** and confirm

The TUI auto-picks a method by OS: **TPROXY** on Linux, **VPN** on Windows.
Other methods (Proxy, Hosts) are available via the node command path; the
default TUI flow uses the OS-appropriate privileged method.

The same operations are available without the TUI:

```bash
praxis_cli intercept status
praxis_cli intercept enable <node-prefix> --method tproxy
praxis_cli intercept disable <node-prefix>
```

The node will:
- Create and install a root CA certificate
- Generate leaf certificates for intercept domains
- Start the proxy server
- Configure system based on chosen method

View captured traffic in the **Intercept** window (`Ctrl+T`).

## Viewing Traffic

### Traffic Tab

The Traffic tab shows captured requests:

| Column | Description |
|--------|-------------|
| Time | When the request occurred |
| Agent | Which agent made the request |
| Method | HTTP method (GET, POST) |
| URL | Full request URL |
| Status | Response status code |

WebSocket traffic is also supported — frames for one connection are grouped
by a stable **flow id** (not merely URL), so concurrent sockets to the same
host do not merge in the TUI.

HTTP/2 is captured at the **frame** level (HEADERS/DATA and control frames
relayed). gRPC over HTTP/2 is therefore **frame-level capture**, not full
message reassembly across DATA frames — length-prefixed gRPC messages that
span frames may appear fragmented in capture, rules, and search. **This is
the supported scope for this release** (not a temporary gap pending
reassembly). Frames group by **connection flow + stream** in the TUI.

Live list/match streams and traffic log/search/matches responses are
**metadata-only** (bodies stripped). Opening a row issues a `TrafficGet`
for full request/response bodies (lazy load). Captured request/response
headers are stored as a map of name→single value (duplicate header names
collapse).

### Request Details

Click a row to see details:

**Request:**
- Headers (name→single value; duplicate names collapse)
- Request body (JSON formatted)
- Content type

**Response:**
- Status code
- Headers (name→single value; duplicate names collapse)
- Response body (JSON formatted)

For LLM APIs, you'll see:
- The prompts being sent
- Tool call requests
- Model responses
- Token usage

## Protocol Support

### HTTP/1.1

Standard HTTP traffic is captured as separate send and receive entries. Send
rules inspect only request fields; receive rules inspect only response fields.

### WebSocket

WebSocket upgrades require HTTP 101 plus both `Upgrade: websocket` and a
`Connection: upgrade` token (plain and TLS paths). Individual frames are
captured with method suffixes like `WS_TEXT#<flow>` used as internal grouping
keys (the TUI may hide the `#flow` suffix). Grouping is per flow id, not
solely by URL.

### HTTP/2 and gRPC

The proxy provides frame-level HTTP/2 interception for services using HTTP/2 (including gRPC streaming):

**Detection**: TLS ALPN is authoritative for HTTP/2, and the complete connection
preface (`PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`) is then read and validated without
consuming the first SETTINGS frame. Praxis prefers HTTP/1.1 during its client
handshake for broad origin compatibility; when a client requires HTTP/2, the
origin must also negotiate `h2` or the tunnel is rejected instead of relaying
bytes under mismatched protocols.

**Captured Frames**:
- `H2_HEADERS` - Request/response headers, HPACK-decoded into readable key/value
  pairs (including the `:method`, `:path`, and `:status` pseudo-headers)
- `H2_DATA` - Request/response body data

**Frame Relay**: All frame types are forwarded bidirectionally:
- SETTINGS, WINDOW_UPDATE (flow control)
- PING (keep-alive)
- RST_STREAM (stream reset)
- GOAWAY (connection close)

**gRPC Streaming**: Bidirectional DATA frames are relayed and captured, but
each DATA frame is treated independently for decompression/capture. **Supported
scope for this release is frame-level capture**, not full gRPC message
reassembly across frames.

**UI Display**: HTTP/2 traffic is grouped by connection flow and stream
(method fields use suffixes like `H2_HEADERS#<flow>:<stream>` as internal
keys), showing:
- Total frame count
- Send/receive counts
- Total bytes transferred
- Individual frames expandable with payload preview

**Header Decoding**: HEADERS frames are decoded with a stateful per-direction
HPACK decoder that tracks the dynamic table across frames and reassembles blocks
split across CONTINUATION frames. Decoded headers are shown as key/value pairs,
and the `:path` pseudo-header provides URL context for DATA frames in the same
stream. If a header block ever fails to decode, header decoding for that
direction is disabled for the rest of the connection rather than showing
corrupt output. Header blocks and HPACK dynamic tables are capped at 1 MiB,
padded DATA frames exclude padding from captured bodies, and invalid
interleaved CONTINUATION sequences disable only capture decoding while frames
continue to be forwarded.

**Capture completeness**: Captured traffic passes through bounded queues on
both the node and service. The service persists entries with a bounded worker
pool so database and rule processing cannot block node command/status
dispatch. If a burst outpaces either queue, entries are dropped rather than
blocking the intercepted connection, and Praxis logs that capture is lossy.
Forwarding of the live connection is independent of service storage, but
**local** proxy memory bounds (e.g. 64 MiB plain HTTP request/response
buffering) can still affect traffic under extreme concurrent load.

Trigger firing and LLM summarization run on a separate capacity budget and
are dropped immediately under load (no queueing) without blocking capture
persistence.

**Clear vs in-flight triggers:** Traffic Clear deletes stored rows and
advances the service clear-generation so new live batches and TUI state
reconcile. Trigger / LLM / external side effects that have already entered
the trigger engine for a match may still complete; Clear does not revoke
in-flight match side effects. A generation check skips only triggers that
have not yet entered the engine when Clear has already advanced.

## Cleanup and Recovery

Enable records recovery intent before changing proxy settings, hosts/routes,
TPROXY policy rules, or IPv6 sysctls. Disable removes only Praxis-owned rules
and retains the recovery file if any cleanup step fails, allowing a retry on
the next start.

Failed or cancelled enable whose rollback cannot fully clean up enters
**CleanupRequired**: re-enable is blocked until Disable or node Reset runs
`force_cleanup` successfully. Manager-owned CA/resource handles are retained
while cleanup is incomplete; recovery metadata is not overwritten by a fresh
enable. Clients surface `InterceptStatus.cleanup_required` via CLI
`intercept status`; the Intercept TUI window does not list per-node state
(fleets can be large — enable/disable lives in the Nodes window).

A node also refuses to proceed while stale cleanup is incomplete or its
recovery file cannot be parsed. Async enable phases (DNS, proxy start, packet
IP refresh) race an operation cancel token. Host child commands (iptables,
netsh, sysctl, cert tools, etc.) run through a bounded runner
(`output_bounded` / 30s default) that kills the process on timeout; timeout is
treated as unknown host state and keeps recovery ownership / CleanupRequired
rather than claiming Disabled.

## Limits

| Limit | Value |
|-------|-------|
| Captured body (stored / live) | 10 MiB per side |
| Captured headers (aggregate) | 1 MiB |
| WebSocket frame | 16 MiB |
| Plain non-CONNECT HTTP buffer | 64 MiB request + 64 MiB response (buffered, not streaming) |
| TUI traffic ring buffer | 2,000 entries |
| TUI matches buffer | 2,000 entries |
| TUI paused pending | 2,000 entries |
| TUI body cache | 64 MiB (insertion-order eviction) |
| Service traffic-query queue | 64 jobs, 4 workers |
| Service intercept-processing queue | 64 entries, 8 workers |
| Live broadcast batch queue | 4,096 (bodies stripped) |
| CLI live intercept entry/match channels | 32 batches (drop-when-full, rate-limited logs) |

## Compatibility and Trust

**Traffic request_id**: Client traffic log/search/matches/clear/get signals
include a client-generated `request_id` echoed in the matching response so
concurrent same-kind queries are not swapped. The field accepts an empty
default when older peers omit it (Serde `default`). Prefer **atomic**
CLI/service upgrades; mixed versions without a unique request_id can still
mis-pair concurrent traffic queries.

**Node identity on intercept traffic**: The service accepts
`InterceptedTraffic` / intercept status when the payload `node_id` names a
**registered** node. It does **not** bind the payload to an authenticated
transport sender. If multiple nodes share the same RabbitMQ credentials, a
node can attribute traffic or status under another registered node id.
Treat co-credentialed nodes as fully trusted for attribution, or isolate
broker credentials per node.

**Clear vs ingest**: Traffic clear takes an exclusive barrier shared with
ingest, prune, and queries. A generation counter advances on successful clear;
pre-clear entries that only reach the DB after clear are dropped. Multi-batch
regex/body scans use keyset pagination on `id` (not `OFFSET`).

Duplicate header **names** are preserved when **forwarding** plain HTTP, but
stored capture uses a map and collapses duplicates to a single value per name.

Server-side traffic list/search scans may skip rows under concurrent deletes
or pruning (pagination is best-effort without a snapshot transaction). Clear
is synchronized against concurrent list/search/get jobs on the service, but
ingest/pruning can still insert or delete around a clear — a successful
clear does **not** guarantee a global snapshot of “everything before this
request is gone.” Invalid search regex falls back to a literal substring
pattern; **rules reject invalid regex** on create/update.

## Traffic Rules

Rules let you match and process specific traffic.

### Creating Rules

1. Go to **Intercept** → **Rules**
2. Create a rule (`Ctrl+N` in the TUI)
3. Configure:
   - **Name** - identifier for the rule
   - **Pattern** - regex (must compile; invalid patterns are rejected)
   - **Direction** - send, receive, or both
   - **Scope** - all traffic or specific node/agent
   - **Summarization prompt** - optional LLM analysis

Matching covers host, URL, method, response status, headers, and UTF-8
bodies (respecting send/receive direction). The TUI rule-form “Recent
matches” preview uses the local traffic buffer and only sees bodies that
have already been loaded into the detail cache (live rows arrive bodyless).

### Rule Matching

Matching runs when traffic is **captured**. Creating or updating a rule
also backfills against the most recent ~500 stored entries so historical
body-only patterns appear in Matches without waiting for new traffic.

When traffic matches a rule:
- Entry is tagged with the rule
- Matches viewable separately

### Semantic Parsing

Rules can include a summarization prompt for semantic analysis. When a rule matches and has a summarization prompt configured, the Traffic Parser LLM processes the matched traffic - extracting prompts, summarizing responses, detecting tool calls, and highlighting key information.

Use rules to:
- Flag specific API calls
- Track sensitive operations
- Collect API keys
- Monitor for specific content

## Disabling Interception

Click **Disable** to stop interception. This:
- Removes the installed certificate
- Restores proxy settings (if modified)
- Cleans hosts file entries (if modified)
- Removes iptables TPROXY rules (if used)
- Stops the proxy server

## Shared IP Passthrough

When multiple domains share the same IP address (e.g., `claude.ai` and `api.anthropic.com` both resolve to `160.79.104.10`), traffic to non-intercepted domains may route through the proxy.

The proxy handles this transparently:
1. Extracts SNI (Server Name Indication) from TLS ClientHello
2. Checks if the domain should be intercepted
3. For non-intercepted domains, tunnels traffic through without TLS termination
4. Uses the same bypass mechanisms to connect to the real server

This ensures non-intercepted domains continue to work normally even when sharing IPs with intercepted domains.

## Security Considerations

### Certificate Trust

The generated root CA must be trusted by the system for HTTPS interception to work. This is done automatically but:
- Some applications have their own certificate stores
- Users may notice certificate changes
- Security tools may alert on unknown CAs

### Credential Exposure

Intercepted traffic may contain:
- API keys in headers
- Authentication tokens
- Sensitive prompts and responses

Handle captured data appropriately.

### Detection

Interception is not stealthy:
- Root CA installed in system store
- System proxy modified (Proxy mode)
- Hosts file modified (Hosts mode)
- Network adapter created (VPN mode)
- iptables rules modified (TPROXY mode)

This tool is designed for research, not covert operations.

## Troubleshooting

### Traffic not appearing

- Verify interception is enabled
- Check the agent uses intercepted domains
- Try a different interception method
- Ensure proxy certificate is trusted

### Certificate errors

- Some apps have pinned certificates
- Node.js: Set `NODE_EXTRA_CA_CERTS`
- Python: Set `REQUESTS_CA_BUNDLE`
- Browsers may need manual cert import

### VPN mode fails

- Requires Administrator privileges on Windows or root/CAP_NET_ADMIN on Linux
- Check for conflicting VPN software

### TPROXY mode fails

- Linux only
- Requires root or `CAP_NET_ADMIN` capability
- Verify iptables is available: `which iptables`
- Check for conflicting mangle rules: `iptables -t mangle -L`
- Ensure `route_localnet` can be enabled on loopback
- Check policy routing: `ip rule list` and `ip route show table 100`

### IPv6 connectivity issues during interception

TPROXY mode temporarily disables IPv6 system-wide (`net.ipv6.conf.all.disable_ipv6=1`) because:
- TPROXY rules only handle IPv4 traffic
- IPv6 traffic would bypass interception

IPv6 is automatically restored when interception is disabled or when the node
process next starts and finds recovery state. If automatic cleanup repeatedly
fails, restore it manually to the value appropriate for the host:
```bash
sudo sysctl -w net.ipv6.conf.all.disable_ipv6=0
```

### Performance issues

- Large traffic volumes can slow things down
- Consider filtering to specific domains
- Use rules to reduce stored traffic
