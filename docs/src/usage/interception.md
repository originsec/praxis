# Interception

Traffic interception lets you see the communication between AI agents and their LLM backends. You can watch prompts being sent, responses coming back, and tool calls being made.

## How It Works

```
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

## Interception Methods

Praxis supports four methods for routing traffic through the proxy. Each has tradeoffs.

### Proxy Mode

**How it works:** Configures system proxy settings so applications route HTTP/HTTPS through the Praxis proxy.

**Setup:**
- Linux: Sets `HTTP_PROXY` and `HTTPS_PROXY` environment variables
- Windows: Modifies registry proxy settings

**Advantages:**
- Easiest to set up
- Works without elevated privileges
- Minimal system changes

**Disadvantages:**
- Only captures HTTP/HTTPS
- Some applications ignore proxy settings
- May conflict with existing proxy configuration

**Best for:** Quick setup, applications that respect proxy settings

### VPN Mode

**How it works:** Creates a TUN network adapter and routes specific IPs through it at the packet level.

**Platform support:** Windows only (Linux support in development)

**Setup:**
1. TUN device created (wintun on Windows)
2. Intercept domains resolved to IP addresses
3. Routes added for those IPs through the TUN
4. Packet engine performs NAT to redirect to proxy

**Advantages:**
- Captures traffic from all applications
- Works even if apps ignore proxy settings
- More comprehensive coverage

**Disadvantages:**
- Windows only (Linux support in development)
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
1. IPv6 disabled system-wide (restored on cleanup)
2. Intercept domains resolved to IP addresses
3. iptables mangle rules added to mark packets to target IPs
4. Policy routing configured to route marked packets to loopback
5. TPROXY rule redirects packets to proxy port
6. Proxy uses `SO_ORIGINAL_DST` to get real destination

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

## Enabling Interception

1. Go to **Intercept** in the web UI
2. Select your node
3. Choose a method (Proxy, VPN, Hosts, or TPROXY)
4. Click **Enable**

The node will:
- Create and install a root CA certificate
- Generate leaf certificates for intercept domains
- Start the proxy server
- Configure system based on chosen method

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

WebSocket traffic is also supported - messages are coalesced into a single row per connection.

### Request Details

Click a row to see details:

**Request:**
- Full headers
- Request body (JSON formatted)
- Content type

**Response:**
- Status code
- Headers
- Response body (JSON formatted)

For LLM APIs, you'll see:
- The prompts being sent
- Tool call requests
- Model responses
- Token usage

## Traffic Rules

Rules let you match and process specific traffic.

### Creating Rules

1. Go to **Intercept** → **Rules**
2. Click **New Rule**
3. Configure:
   - **Name** - identifier for the rule
   - **Pattern** - regex to match
   - **Direction** - send, receive, or both
   - **Scope** - all traffic or specific node/agent
   - **Summarization prompt** - optional LLM analysis

### Rule Matching

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

- Windows only (Linux support in development)
- Requires Administrator privileges
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

IPv6 is automatically restored when interception is disabled. If the node crashes without cleanup, restore manually:
```bash
sudo sysctl -w net.ipv6.conf.all.disable_ipv6=0
```

### Performance issues

- Large traffic volumes can slow things down
- Consider filtering to specific domains
- Use rules to reduce stored traffic
