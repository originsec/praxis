/*
 * praxis_node_tiny_c — minimal pure-C praxis node.
 *
 * Runtime dependencies: libc only. All protocol code (AMQP 0-9-1,
 * HTTP/1.1, JSON, ACP JSON-RPC) is hand-rolled and statically linked
 * into the resulting binary.
 *
 * Scope matches the Rust praxis_node_tiny: register with the praxis
 * service over RabbitMQ, host an ACP server for the native Praxis
 * agent, run shell commands as tool calls, stream chat completions
 * back to clients via session/update notifications.
 *
 * Limitations vs the Rust tiny node:
 *   - Linux only (uses /dev/urandom, gethostname).
 *   - HTTP only for the AI endpoint; TLS is not bundled in v1.
 *     Use a local OpenAI-compatible endpoint (e.g. an HTTP proxy in
 *     front of OpenAI, or a local llama.cpp/ollama server).
 *   - OpenAI-compatible chat-completions API only; no Anthropic or
 *     Gemini provider plumbing.
 */

#ifndef TINY_H
#define TINY_H

#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>
#include <sys/types.h>

/* ============================================================== */
/* util.c — logging, time, random, dynamic buffers, base64         */
/* ============================================================== */

void log_msg(const char *level, const char *fmt, ...);
#define LOG_INFO(...)  log_msg("INFO",  __VA_ARGS__)
#define LOG_WARN(...)  log_msg("WARN",  __VA_ARGS__)
#define LOG_ERROR(...) log_msg("ERROR", __VA_ARGS__)
#define LOG_DEBUG(...) do { if (tiny_debug) log_msg("DEBUG", __VA_ARGS__); } while (0)

extern int tiny_debug;

void rand_bytes(unsigned char *out, size_t n);
void uuid_v4(char out[37]);

uint64_t monotonic_ms(void);

/* Growing byte buffer. Owns its memory; double-free-safe via len=0/cap=0. */
typedef struct buf {
    char  *data;
    size_t len;
    size_t cap;
} buf;

void buf_reserve(buf *b, size_t need);
void buf_putc(buf *b, char c);
void buf_put(buf *b, const void *p, size_t n);
void buf_puts(buf *b, const char *s);
void buf_putf(buf *b, const char *fmt, ...);
void buf_free(buf *b);

/* current process is privileged (uid 0)? */
int is_privileged(void);

/* ============================================================== */
/* json.c — parser + writer                                        */
/* ============================================================== */

typedef enum {
    JNULL, JBOOL, JNUM, JSTR, JARR, JOBJ
} json_type;

typedef struct json {
    json_type type;
    union {
        int     b;
        double  n;
        struct { char *s; size_t len; } str;
        struct { struct json **items; size_t count; } arr;
        struct {
            char         **keys;
            size_t        *key_lens;
            struct json  **vals;
            size_t         count;
        } obj;
    } u;
} json;

/* Parse src..src+n. Returns owned tree on success, NULL on parse error.
 * The returned value owns all sub-allocations and is freed via json_free. */
json *json_parse(const char *src, size_t n);
void  json_free(json *j);

/* Path lookup: dot-separated keys (foo.bar.baz). NULL if not found. */
json *json_get(json *j, const char *path);

/* Type helpers: return 0/empty on type mismatch. */
const char *json_str(json *j, size_t *len_out);
int  json_get_str(json *j, const char *path, const char **out, size_t *len_out);
int  json_get_bool(json *j, const char *path, int *out);
int  json_get_int (json *j, const char *path, long *out);

/* Writer helpers — appends to a buf. The string variants quote+escape. */
void jb_str(buf *b, const char *s, size_t n);   /* "...escaped..." */
void jb_strz(buf *b, const char *s);            /* same but null-term */

/* ============================================================== */
/* amqp.c — AMQP 0-9-1 client                                       */
/* ============================================================== */

typedef struct amqp amqp;

amqp *amqp_connect(const char *host, int port, const char *user, const char *pass);
void  amqp_close(amqp *c);

int amqp_queue_declare(amqp *c, const char *queue);
int amqp_exchange_declare_fanout(amqp *c, const char *name);
/* Declare exclusive auto-delete server-named queue, write the actual
 * name into out (caller-owned buffer of out_cap bytes). */
int amqp_queue_declare_exclusive(amqp *c, char *out, size_t out_cap);
int amqp_queue_bind(amqp *c, const char *queue, const char *exchange, const char *routing_key);

int amqp_basic_publish(amqp *c, const char *exchange, const char *routing_key,
                       const void *body, size_t body_len);
int amqp_basic_consume(amqp *c, const char *queue, const char *consumer_tag);

/* Read one delivered message. *body / *body_len point into a buffer owned
 * by the amqp; valid until the next amqp_* call. Returns 1 on delivery,
 * 0 on shutdown signal, -1 on error. consumer_tag_out (optional) gets
 * the matching consumer tag. */
int amqp_next_delivery(amqp *c, char **body, size_t *body_len,
                       char *consumer_tag_out, size_t tag_cap);

/* Try to read a delivery, with timeout in milliseconds. -2 on timeout,
 * 0 on shutdown, 1 on delivery, -1 on error. */
int amqp_next_delivery_timeout(amqp *c, int timeout_ms,
                               char **body, size_t *body_len,
                               char *consumer_tag_out, size_t tag_cap);

/* Tear-down request: cause amqp_next_delivery* to return 0 ASAP. */
void amqp_request_shutdown(amqp *c);

/* ============================================================== */
/* http.c — minimal HTTP/1.1 client + SSE                           */
/* ============================================================== */

/* Parse url into host/port/path. Only http:// supported. Returns 0/-1.
 * Caller owns out_host/out_path (heap). */
int  http_parse_url(const char *url, char **out_host, int *out_port, char **out_path);

/* Send POST and stream back SSE chunks via on_chunk. headers is a
 * NULL-terminated array of "Key: value" strings.  Each "data:" line
 * payload is delivered to on_chunk. cancel is checked between reads;
 * if non-NULL and *cancel != 0, returns -2.  Returns 0 on clean stream
 * end, -1 on transport error, -2 on cancel. */
int http_post_sse(const char *host, int port, const char *path,
                  const char *const *headers,
                  const void *body, size_t body_len,
                  void (*on_chunk)(const char *data, size_t n, void *ud),
                  void *ud,
                  volatile int *cancel);

/* ============================================================== */
/* praxis.c — agent sessions, ACP dispatch, run_command             */
/* ============================================================== */

typedef struct praxis_cfg {
    char *provider;        /* unused, kept for parity */
    char *api_key;
    char *endpoint_url;
    char *model_name;
    char *system_prompt;   /* may be NULL */
    int   max_tool_iters;
    int   command_timeout_secs;
} praxis_cfg;

void praxis_cfg_free(praxis_cfg *c);

/* Apply a fresh praxis config (or NULL to disable). Takes ownership.
 * Subsequent session/new on the praxis connector uses this config. */
void praxis_set_config(praxis_cfg *cfg);
int  praxis_enabled(void);

/* Handle one inbound ACP JSON-RPC frame. Outbound frames are pushed via
 * acp_send_*(). Frame is parsed, dispatched, and freed.  Spawns a
 * background thread for session/prompt so the AMQP loop never blocks. */
void acp_handle_frame(const char *client_id, const char *rpc, size_t rpc_len);

/* Wire-out helpers used by acp dispatch. Defined in main.c so the AMQP
 * channel lives there. */
void acp_send_response(const char *client_id, const char *id_raw,
                       const char *result_raw);
void acp_send_error(const char *client_id, const char *id_raw,
                    int code, const char *msg);
void acp_send_session_notification(const char *client_id, const char *session_id,
                                   const char *update_raw);

/* main.c sets these so praxis.c can publish info updates. */
extern char tiny_node_id[64];

/* Build and send a NodeInformationUpdate. */
void send_node_information_update(void);

/* Wait for all in-flight session/prompt threads. */
void praxis_join_workers(void);

#endif /* TINY_H */
