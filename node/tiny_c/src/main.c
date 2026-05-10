#include "tiny.h"

#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/utsname.h>
#include <unistd.h>
#include <pthread.h>

/* ============================================================== */
/* globals                                                          */
/* ============================================================== */

char tiny_node_id[64];

static amqp *G_amqp = NULL;
static pthread_mutex_t G_amqp_mu = PTHREAD_MUTEX_INITIALIZER;
static volatile int    G_shutdown = 0;
static char            G_node_queue[128];

#define NODE_SIGNAL_QUEUE      "NodeSignal"
#define NODE_BROADCAST_EXCHANGE "NodeBroadcast"

/* ============================================================== */
/* node id persistence                                              */
/* ============================================================== */

static int load_or_create_node_id(char out[64])
{
    const char *xdg = getenv("XDG_DATA_HOME");
    const char *home = getenv("HOME");
    if (!home) return -1;
    char dir[512];
    if (xdg && *xdg) snprintf(dir, sizeof(dir), "%s/praxis", xdg);
    else             snprintf(dir, sizeof(dir), "%s/.local/share/praxis", home);
    char path[640];
    snprintf(path, sizeof(path), "%s/node_id", dir);

    FILE *f = fopen(path, "r");
    if (f) {
        char buf2[64] = {0};
        if (fgets(buf2, sizeof(buf2), f)) {
            char *nl = strchr(buf2, '\n');
            if (nl) *nl = 0;
            if (buf2[0]) {
                snprintf(out, 64, "%s", buf2);
                fclose(f);
                return 0;
            }
        }
        fclose(f);
    }

    /* create */
    char id[37];
    uuid_v4(id);
    mkdir(dir, 0755);    /* may fail if intermediate doesn't exist; non-fatal */
    /* try parent.parent in case ~/.local/share doesn't exist */
    char parent[512];
    snprintf(parent, sizeof(parent), "%s/.local", home);
    mkdir(parent, 0755);
    snprintf(parent, sizeof(parent), "%s/.local/share", home);
    mkdir(parent, 0755);
    mkdir(dir, 0755);
    f = fopen(path, "w");
    if (f) { fputs(id, f); fclose(f); }
    snprintf(out, 64, "%s", id);
    return 0;
}

/* ============================================================== */
/* registration                                                     */
/* ============================================================== */

static void hostname_into(char *out, size_t cap)
{
    if (gethostname(out, cap) != 0) snprintf(out, cap, "unknown");
    out[cap - 1] = 0;
}

static void os_details_into(char *out, size_t cap)
{
    struct utsname u;
    if (uname(&u) == 0) snprintf(out, cap, "%s %s (%s)", u.sysname, u.release, u.machine);
    else                 snprintf(out, cap, "Unknown");
}

static int publish_registration(amqp *c)
{
    char host[256], os_d[256];
    hostname_into(host, sizeof(host));
    os_details_into(os_d, sizeof(os_d));

    buf body = {0};
    buf_puts(&body, "{\"Registration\":{\"node_id\":");
    jb_strz(&body, tiny_node_id);
    buf_puts(&body, ",\"node_type\":\"tiny-c\",\"machine_name\":");
    jb_strz(&body, host);
    buf_puts(&body, ",\"os_details\":");
    jb_strz(&body, os_d);
    buf_puts(&body, ",\"capabilities\":[\"Session\"]}}");

    int rc = amqp_basic_publish(c, "", NODE_SIGNAL_QUEUE, body.data, body.len);
    buf_free(&body);
    return rc;
}

/* parse RegistrationAck into the praxis config (or clear if disabled) */
static void apply_registration_ack(json *ack)
{
    int enabled = 0;
    json_get_bool(ack, "praxis_agent_enabled", &enabled);
    if (!enabled) {
        praxis_set_config(NULL);
        LOG_INFO("Registration ack: praxis agent disabled");
        return;
    }
    json *pc = json_get(ack, "praxis_agent_config");
    if (!pc || pc->type != JOBJ) {
        praxis_set_config(NULL);
        LOG_INFO("Registration ack: no praxis_agent_config");
        return;
    }
    praxis_cfg *c = calloc(1, sizeof(*c));
    const char *s; size_t n;
    if (json_get_str(pc, "provider",      &s, &n)) { c->provider     = strndup(s, n); }
    if (json_get_str(pc, "api_key",       &s, &n)) { c->api_key      = strndup(s, n); }
    if (json_get_str(pc, "endpoint_url",  &s, &n)) { c->endpoint_url = strndup(s, n); }
    if (json_get_str(pc, "model_name",    &s, &n)) { c->model_name   = strndup(s, n); }
    if (json_get_str(pc, "system_prompt", &s, &n)) { c->system_prompt= strndup(s, n); }
    long iv;
    if (json_get_int(pc, "max_tool_iterations", &iv))  c->max_tool_iters = (int)iv;
    if (json_get_int(pc, "command_timeout_secs", &iv)) c->command_timeout_secs = (int)iv;
    praxis_set_config(c);
    LOG_INFO("Praxis agent enabled (model=%s, endpoint=%s)",
             c->model_name ? c->model_name : "?",
             c->endpoint_url ? c->endpoint_url : "?");
}

/* ============================================================== */
/* outbound JSON-RPC helpers (used by praxis.c)                     */
/* ============================================================== */

/* publish a NodeSignalMessage::Acp envelope */
static void publish_acp_envelope(const char *client_id, const char *json_rpc, size_t rpc_len)
{
    pthread_mutex_lock(&G_amqp_mu);
    amqp *c = G_amqp;
    pthread_mutex_unlock(&G_amqp_mu);
    if (!c) return;

    buf env = {0};
    buf_puts(&env, "{\"Acp\":{\"node_id\":");
    jb_strz(&env, tiny_node_id);
    buf_puts(&env, ",\"client_id\":");
    jb_strz(&env, client_id);
    buf_puts(&env, ",\"json_rpc\":");
    jb_str(&env, json_rpc, rpc_len);
    buf_puts(&env, "}}");
    amqp_basic_publish(c, "", NODE_SIGNAL_QUEUE, env.data, env.len);
    buf_free(&env);
}

void acp_send_response(const char *client_id, const char *id_raw, const char *result_raw)
{
    buf r = {0};
    buf_puts(&r, "{\"jsonrpc\":\"2.0\",\"id\":");
    buf_puts(&r, id_raw ? id_raw : "null");
    buf_puts(&r, ",\"result\":");
    buf_puts(&r, result_raw ? result_raw : "null");
    buf_putc(&r, '}');
    publish_acp_envelope(client_id, r.data, r.len);
    buf_free(&r);
}

void acp_send_error(const char *client_id, const char *id_raw, int code, const char *msg)
{
    buf r = {0};
    buf_puts(&r, "{\"jsonrpc\":\"2.0\",\"id\":");
    buf_puts(&r, id_raw ? id_raw : "null");
    buf_putf(&r, ",\"error\":{\"code\":%d,\"message\":", code);
    jb_strz(&r, msg ? msg : "");
    buf_puts(&r, "}}");
    publish_acp_envelope(client_id, r.data, r.len);
    buf_free(&r);
}

void acp_send_session_notification(const char *client_id, const char *session_id,
                                   const char *update_raw)
{
    buf r = {0};
    buf_puts(&r, "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\","
                  "\"params\":{\"sessionId\":");
    jb_strz(&r, session_id);
    buf_puts(&r, ",\"update\":");
    buf_puts(&r, update_raw);
    buf_puts(&r, "}}");
    publish_acp_envelope(client_id, r.data, r.len);
    buf_free(&r);
}

/* ============================================================== */
/* periodic node information update                                 */
/* ============================================================== */

void send_node_information_update(void)
{
    pthread_mutex_lock(&G_amqp_mu);
    amqp *c = G_amqp;
    pthread_mutex_unlock(&G_amqp_mu);
    if (!c) return;

    char ts[64];
    {
        struct timespec t;
        clock_gettime(CLOCK_REALTIME, &t);
        struct tm tm;
        gmtime_r(&t.tv_sec, &tm);
        long ms = (long)(t.tv_nsec / 1000000);
        if (ms < 0) ms = 0;
        if (ms > 999) ms = 999;
        snprintf(ts, sizeof(ts), "%04d-%02d-%02dT%02d:%02d:%02d.%03ldZ",
                 tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday,
                 tm.tm_hour, tm.tm_min, tm.tm_sec, ms);
    }

    buf body = {0};
    buf_puts(&body, "{\"InformationUpdate\":{\"node_id\":");
    jb_strz(&body, tiny_node_id);
    buf_puts(&body, ",\"timestamp\":");
    jb_strz(&body, ts);
    buf_puts(&body, ",\"discovered_agents\":[");
    if (praxis_enabled()) {
        buf_puts(&body, "{\"name\":\"Praxis Agent\",\"short_name\":\"praxis\","
                          "\"available\":true}");
    }
    buf_puts(&body, "],\"selected_agent\":null,"
                     "\"intercept_supported\":false,"
                     "\"intercept_enabled\":false,"
                     "\"intercept_method\":null,"
                     "\"active_terminal_id\":null,"
                     "\"privileged\":");
    buf_puts(&body, is_privileged() ? "true" : "false");
    buf_puts(&body, "}}");
    amqp_basic_publish(c, "", NODE_SIGNAL_QUEUE, body.data, body.len);
    buf_free(&body);
}

/* ============================================================== */
/* main loop                                                        */
/* ============================================================== */

static void handle_signal(int s) { (void)s; G_shutdown = 1; if (G_amqp) amqp_request_shutdown(G_amqp); }

/* helper used while parsing the broadcast message envelope */
static void handle_node_broadcast(json *root)
{
    if (!root || root->type != JOBJ) return;
    if (root->u.obj.count == 0) return;

    /* externally-tagged: {"VariantName": payload} */
    const char *var = root->u.obj.keys[0];
    size_t vlen = root->u.obj.key_lens[0];
    json *p = root->u.obj.vals[0];

    if (vlen == 19 && memcmp(var, "PraxisAgentEnabled", 18) == 0) {
        /* could be 18 or 19; do a real comparison */
    }

#define KEQ(k) (vlen == sizeof(k) - 1 && memcmp(var, k, sizeof(k) - 1) == 0)

    if (KEQ("PraxisAgentEnabled")) {
        int enabled = 0;
        json_get_bool(p, "enabled", &enabled);
        if (!enabled) { praxis_set_config(NULL); send_node_information_update(); return; }
        json *cfg = p ? json_get(p, "config") : NULL;
        if (!cfg) { praxis_set_config(NULL); send_node_information_update(); return; }
        /* reuse apply_registration_ack-like logic */
        praxis_cfg *c = calloc(1, sizeof(*c));
        const char *s; size_t n;
        if (json_get_str(cfg, "provider",      &s, &n)) c->provider     = strndup(s, n);
        if (json_get_str(cfg, "api_key",       &s, &n)) c->api_key      = strndup(s, n);
        if (json_get_str(cfg, "endpoint_url",  &s, &n)) c->endpoint_url = strndup(s, n);
        if (json_get_str(cfg, "model_name",    &s, &n)) c->model_name   = strndup(s, n);
        if (json_get_str(cfg, "system_prompt", &s, &n)) c->system_prompt= strndup(s, n);
        long iv;
        if (json_get_int(cfg, "max_tool_iterations", &iv))  c->max_tool_iters = (int)iv;
        if (json_get_int(cfg, "command_timeout_secs", &iv)) c->command_timeout_secs = (int)iv;
        praxis_set_config(c);
        send_node_information_update();
    } else if (KEQ("NodeInformationUpdateRequest")) {
        send_node_information_update();
    } else if (KEQ("NodeRefreshRegistration")) {
        pthread_mutex_lock(&G_amqp_mu);
        amqp *c = G_amqp;
        pthread_mutex_unlock(&G_amqp_mu);
        if (c) publish_registration(c);
    }
    /* ignore EventLoggingSet, AgentRegistryUpdate, InterceptTargetsUpdate */
#undef KEQ
}

/* main: connect, register, consume forever */
static int run_once(const char *host, int port, const char *user, const char *pass)
{
    amqp *c = amqp_connect(host, port, user, pass);
    if (!c) return -1;

    pthread_mutex_lock(&G_amqp_mu);
    G_amqp = c;
    pthread_mutex_unlock(&G_amqp_mu);

    /* declare node-specific queue */
    snprintf(G_node_queue, sizeof(G_node_queue), "node-%s", tiny_node_id);
    if (amqp_queue_declare(c, G_node_queue) < 0) goto fail;

    /* publish registration */
    if (publish_registration(c) < 0) goto fail;

    /* consume from node queue */
    if (amqp_basic_consume(c, G_node_queue, "tiny-c-direct") < 0) goto fail;

    /* declare broadcast exchange + bind a private queue */
    if (amqp_exchange_declare_fanout(c, NODE_BROADCAST_EXCHANGE) < 0) goto fail;
    char bqname[128];
    if (amqp_queue_declare_exclusive(c, bqname, sizeof(bqname)) < 0) goto fail;
    if (amqp_queue_bind(c, bqname, NODE_BROADCAST_EXCHANGE, "") < 0) goto fail;
    if (amqp_basic_consume(c, bqname, "tiny-c-broadcast") < 0) goto fail;

    LOG_INFO("Listening on %s and %s (broadcast)", G_node_queue, bqname);

    int sent_initial_info = 0;

    while (!G_shutdown) {
        char *body = NULL; size_t blen = 0;
        char tag[256];
        int rc = amqp_next_delivery(c, &body, &blen, tag, sizeof(tag));
        if (rc == 0)  break;        /* shutdown */
        if (rc < 0) { LOG_WARN("Delivery error"); break; }

        json *root = json_parse(body, blen);
        if (!root) continue;

        int is_broadcast = (strcmp(tag, "tiny-c-broadcast") == 0);

        if (is_broadcast) {
            handle_node_broadcast(root);
        } else {
            /* NodeDirectMessage envelope is {"VariantName": payload} */
            if (root->type == JOBJ && root->u.obj.count > 0) {
                const char *var = root->u.obj.keys[0];
                size_t vlen = root->u.obj.key_lens[0];
                json *p = root->u.obj.vals[0];

                if (vlen == 15 && memcmp(var, "RegistrationAck", 15) == 0) {
                    apply_registration_ack(p);
                    if (!sent_initial_info) {
                        send_node_information_update();
                        sent_initial_info = 1;
                    }
                } else if (vlen == 3 && memcmp(var, "Acp", 3) == 0) {
                    const char *cid; size_t cn;
                    const char *rpc; size_t rn;
                    if (json_get_str(p, "client_id", &cid, &cn) &&
                        json_get_str(p, "json_rpc",  &rpc, &rn)) {
                        char cidbuf[64];
                        if (cn < sizeof(cidbuf)) {
                            memcpy(cidbuf, cid, cn); cidbuf[cn] = 0;
                            acp_handle_frame(cidbuf, rpc, rn);
                        }
                    }
                } else if (vlen == 5 && memcmp(var, "Reset", 5) == 0) {
                    LOG_INFO("Reset received");
                    json_free(root);
                    goto reset;
                }
                /* Command, SemanticParserResponse — ignored */
            }
        }
        json_free(root);
    }

reset:
    pthread_mutex_lock(&G_amqp_mu);
    G_amqp = NULL;
    pthread_mutex_unlock(&G_amqp_mu);
    amqp_close(c);
    return 0;

fail:
    pthread_mutex_lock(&G_amqp_mu);
    G_amqp = NULL;
    pthread_mutex_unlock(&G_amqp_mu);
    amqp_close(c);
    return -1;
}

/* ============================================================== */

static void parse_amqp_url(const char *url,
                           char host[256], int *port,
                           char user[64], char pass[64])
{
    /* defaults */
    snprintf(host, 256, "localhost");
    *port = 5672;
    snprintf(user, 64, "praxis");
    snprintf(pass, 64, "praxis");

    if (!url) return;
    if (strncmp(url, "amqp://", 7) != 0) return;
    const char *p = url + 7;

    /* user[:pass]@ */
    const char *at = strchr(p, '@');
    if (at) {
        const char *colon = memchr(p, ':', (size_t)(at - p));
        if (colon) {
            size_t un = (size_t)(colon - p);
            size_t pn = (size_t)(at - colon - 1);
            if (un >= 64) un = 63;
            if (pn >= 64) pn = 63;
            memcpy(user, p, un); user[un] = 0;
            memcpy(pass, colon + 1, pn); pass[pn] = 0;
        } else {
            size_t un = (size_t)(at - p);
            if (un >= 64) un = 63;
            memcpy(user, p, un); user[un] = 0;
        }
        p = at + 1;
    }

    /* host[:port]/ */
    const char *slash = strchr(p, '/');
    const char *end = slash ? slash : p + strlen(p);
    const char *colon = memchr(p, ':', (size_t)(end - p));
    if (colon) {
        size_t hn = (size_t)(colon - p);
        if (hn >= 256) hn = 255;
        memcpy(host, p, hn); host[hn] = 0;
        char tmp[16] = {0};
        size_t cn = (size_t)(end - colon - 1);
        if (cn >= sizeof(tmp)) cn = sizeof(tmp) - 1;
        memcpy(tmp, colon + 1, cn);
        *port = atoi(tmp);
    } else {
        size_t hn = (size_t)(end - p);
        if (hn >= 256) hn = 255;
        memcpy(host, p, hn); host[hn] = 0;
    }
}

int main(int argc, char **argv)
{
    (void)argc; (void)argv;

    /* signal handling */
    struct sigaction sa = {0};
    sa.sa_handler = handle_signal;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGTERM, &sa, NULL);
    signal(SIGPIPE, SIG_IGN);

    if (load_or_create_node_id(tiny_node_id) < 0) {
        LOG_ERROR("could not establish node id (HOME unset?)");
        return 1;
    }
    LOG_INFO("Tiny C node starting; node_id=%s", tiny_node_id);

    char host[256], user[64], pass[64];
    int port;
    parse_amqp_url(getenv("PRAXIS_RABBITMQ_URL"), host, &port, user, pass);
    LOG_INFO("RabbitMQ: amqp://%s@%s:%d/", user, host, port);

    while (!G_shutdown) {
        if (run_once(host, port, user, pass) < 0) {
            LOG_WARN("connect/register failed; retry in 5s");
            for (int i = 0; i < 50 && !G_shutdown; i++) usleep(100000);
            continue;
        }
        if (G_shutdown) break;
        LOG_INFO("Reset/disconnect; reconnecting in 1s");
        for (int i = 0; i < 10 && !G_shutdown; i++) usleep(100000);
    }

    LOG_INFO("Waiting for in-flight workers...");
    praxis_join_workers();
    LOG_INFO("Bye.");
    return 0;
}
