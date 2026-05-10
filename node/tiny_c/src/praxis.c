#include "tiny.h"

#include <ctype.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if !defined(_WIN32)
  #include <fcntl.h>
  #include <signal.h>
  #include <sys/select.h>
  #include <sys/wait.h>
  #include <unistd.h>
#endif

/* ============================================================== */
/* config                                                           */
/* ============================================================== */

static pthread_mutex_t cfg_mu = PTHREAD_MUTEX_INITIALIZER;
static praxis_cfg     *active_cfg = NULL;

void praxis_cfg_free(praxis_cfg *c)
{
    if (!c) return;
    free(c->provider);
    free(c->api_key);
    free(c->endpoint_url);
    free(c->model_name);
    free(c->system_prompt);
    free(c);
}

/* swap in a new config (or NULL). Old one is freed. */
void praxis_set_config(praxis_cfg *cfg)
{
    pthread_mutex_lock(&cfg_mu);
    praxis_cfg *old = active_cfg;
    active_cfg = cfg;
    pthread_mutex_unlock(&cfg_mu);
    praxis_cfg_free(old);
}

int praxis_enabled(void)
{
    pthread_mutex_lock(&cfg_mu);
    int e = active_cfg != NULL;
    pthread_mutex_unlock(&cfg_mu);
    return e;
}

/* take a snapshot copy under lock; caller frees with praxis_cfg_free */
static praxis_cfg *snapshot_cfg(void)
{
    pthread_mutex_lock(&cfg_mu);
    praxis_cfg *src = active_cfg;
    if (!src) { pthread_mutex_unlock(&cfg_mu); return NULL; }
    praxis_cfg *out = calloc(1, sizeof(*out));
    if (!out) { pthread_mutex_unlock(&cfg_mu); return NULL; }
    out->provider             = src->provider     ? strdup(src->provider)     : NULL;
    out->api_key              = src->api_key      ? strdup(src->api_key)      : NULL;
    out->endpoint_url         = src->endpoint_url ? strdup(src->endpoint_url) : NULL;
    out->model_name           = src->model_name   ? strdup(src->model_name)   : NULL;
    out->system_prompt        = src->system_prompt? strdup(src->system_prompt): NULL;
    out->max_tool_iters       = src->max_tool_iters;
    out->command_timeout_secs = src->command_timeout_secs;
    pthread_mutex_unlock(&cfg_mu);
    return out;
}

/* ============================================================== */
/* sessions                                                         */
/* ============================================================== */

typedef struct ai_msg {
    char *role;       /* "system" | "user" | "assistant" */
    char *content;    /* utf-8 */
    struct ai_msg *next;
} ai_msg;

typedef struct session {
    char            id[40];          /* uuid string */
    char            client_id[64];
    char           *cwd;             /* may be NULL */
    ai_msg         *messages;
    int             busy;            /* 1 while a worker is running */
    volatile int    cancel;          /* set to 1 to abort */
    pthread_mutex_t mu;
    struct session *next;
} session;

static pthread_mutex_t sess_mu = PTHREAD_MUTEX_INITIALIZER;
static session *sess_head = NULL;

static void msg_free_all(ai_msg *m)
{
    while (m) {
        ai_msg *n = m->next;
        free(m->role);
        free(m->content);
        free(m);
        m = n;
    }
}

static void session_free(session *s)
{
    if (!s) return;
    free(s->cwd);
    msg_free_all(s->messages);
    pthread_mutex_destroy(&s->mu);
    free(s);
}

static session *session_find(const char *id)
{
    pthread_mutex_lock(&sess_mu);
    for (session *s = sess_head; s; s = s->next) {
        if (strcmp(s->id, id) == 0) {
            pthread_mutex_unlock(&sess_mu);
            return s;
        }
    }
    pthread_mutex_unlock(&sess_mu);
    return NULL;
}

static void session_remove(const char *id)
{
    session *target = NULL;
    pthread_mutex_lock(&sess_mu);
    session **pp = &sess_head;
    while (*pp) {
        if (strcmp((*pp)->id, id) == 0) {
            target = *pp;
            *pp = (*pp)->next;
            break;
        }
        pp = &(*pp)->next;
    }
    pthread_mutex_unlock(&sess_mu);
    session_free(target);
}

/* ============================================================== */
/* worker tracking (so we can wait at shutdown)                     */
/* ============================================================== */

typedef struct worker {
    pthread_t       thr;
    struct worker  *next;
} worker;

static pthread_mutex_t workers_mu = PTHREAD_MUTEX_INITIALIZER;
static worker *workers_head = NULL;

static void workers_track(pthread_t t)
{
    worker *w = calloc(1, sizeof(*w));
    if (!w) return;
    w->thr = t;
    pthread_mutex_lock(&workers_mu);
    w->next = workers_head;
    workers_head = w;
    pthread_mutex_unlock(&workers_mu);
}

void praxis_join_workers(void)
{
    pthread_mutex_lock(&workers_mu);
    worker *list = workers_head;
    workers_head = NULL;
    pthread_mutex_unlock(&workers_mu);
    while (list) {
        pthread_join(list->thr, NULL);
        worker *n = list->next;
        free(list);
        list = n;
    }
}

/* ============================================================== */
/* run_command                                                      */
/* ============================================================== */

#if !defined(_WIN32)

static int run_command(const char *command, const char *cwd, int timeout_secs,
                       volatile int *cancel, buf *out)
{
    int outp[2], errp[2];
    if (pipe(outp) < 0) return -1;
    if (pipe(errp) < 0) { close(outp[0]); close(outp[1]); return -1; }

    pid_t pid = fork();
    if (pid < 0) {
        close(outp[0]); close(outp[1]);
        close(errp[0]); close(errp[1]);
        return -1;
    }
    if (pid == 0) {
        /* child */
        dup2(outp[1], STDOUT_FILENO);
        dup2(errp[1], STDERR_FILENO);
        close(outp[0]); close(outp[1]);
        close(errp[0]); close(errp[1]);
        if (cwd && *cwd) (void)!chdir(cwd);
        execl("/bin/sh", "sh", "-c", command, (char *)NULL);
        _exit(127);
    }

    close(outp[1]);
    close(errp[1]);

    buf so = {0}, se = {0};
    int outdone = 0, errdone = 0;
    uint64_t deadline = monotonic_ms() + (uint64_t)timeout_secs * 1000ULL;
    int killed = 0;

    while (!(outdone && errdone)) {
        if (cancel && *cancel) {
            kill(pid, SIGTERM);
            killed = 1;
            break;
        }
        uint64_t now = monotonic_ms();
        if (now >= deadline) {
            kill(pid, SIGTERM);
            killed = 1;
            break;
        }
        fd_set rfds;
        FD_ZERO(&rfds);
        int maxfd = -1;
        if (!outdone) { FD_SET(outp[0], &rfds); if (outp[0] > maxfd) maxfd = outp[0]; }
        if (!errdone) { FD_SET(errp[0], &rfds); if (errp[0] > maxfd) maxfd = errp[0]; }
        struct timeval tv;
        uint64_t left = deadline - now;
        if (left > 1000) left = 1000;
        tv.tv_sec = left / 1000;
        tv.tv_usec = (left % 1000) * 1000;
        int s = select(maxfd + 1, &rfds, NULL, NULL, &tv);
        if (s < 0) { if (errno == EINTR) continue; break; }
        if (s == 0) continue;
        char b[4096];
        if (!outdone && FD_ISSET(outp[0], &rfds)) {
            ssize_t r = read(outp[0], b, sizeof(b));
            if (r <= 0) outdone = 1;
            else        buf_put(&so, b, (size_t)r);
        }
        if (!errdone && FD_ISSET(errp[0], &rfds)) {
            ssize_t r = read(errp[0], b, sizeof(b));
            if (r <= 0) errdone = 1;
            else        buf_put(&se, b, (size_t)r);
        }
    }

    int status = 0;
    if (killed) {
        /* give it a moment, then SIGKILL */
        for (int i = 0; i < 10; i++) {
            int r = waitpid(pid, &status, WNOHANG);
            if (r == pid) goto reaped;
            sleep_ms(100);
        }
        kill(pid, SIGKILL);
        waitpid(pid, &status, 0);
    } else {
        waitpid(pid, &status, 0);
    }
reaped:
    close(outp[0]); close(errp[0]);

    char code_buf[32];
    if (WIFEXITED(status)) snprintf(code_buf, sizeof(code_buf), "%d", WEXITSTATUS(status));
    else                    snprintf(code_buf, sizeof(code_buf), "terminated by signal");

    buf_putf(out, "exit_code: %s\nstdout:\n", code_buf);
    if (so.len) buf_put(out, so.data, so.len);
    buf_puts(out, "\nstderr:\n");
    if (se.len) buf_put(out, se.data, se.len);
    buf_free(&so);
    buf_free(&se);

    if (killed && cancel && *cancel)  return -2;
    if (killed)                        return -3;     /* timeout */
    return 0;
}

#else  /* _WIN32 */

/* Windows run_command via CreateProcess + anonymous pipes. The pipes are
 * inherited by the child; we read both halves on a polling loop and kill
 * the process on cancel/timeout. */
static int win_drain(HANDLE h, buf *out)
{
    DWORD avail = 0;
    if (!PeekNamedPipe(h, NULL, 0, NULL, &avail, NULL)) return -1;
    if (avail == 0) return 0;
    char tmp[4096];
    if (avail > sizeof(tmp)) avail = sizeof(tmp);
    DWORD got = 0;
    if (!ReadFile(h, tmp, avail, &got, NULL)) return -1;
    if (got == 0) return -1;
    buf_put(out, tmp, got);
    return (int)got;
}

static int run_command(const char *command, const char *cwd, int timeout_secs,
                       volatile int *cancel, buf *out)
{
    SECURITY_ATTRIBUTES sa = { sizeof(sa), NULL, TRUE };
    HANDLE outR = NULL, outW = NULL, errR = NULL, errW = NULL;
    if (!CreatePipe(&outR, &outW, &sa, 0)) return -1;
    if (!CreatePipe(&errR, &errW, &sa, 0)) {
        CloseHandle(outR); CloseHandle(outW);
        return -1;
    }
    SetHandleInformation(outR, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(errR, HANDLE_FLAG_INHERIT, 0);

    /* cmd /c <command> */
    size_t clen = strlen(command);
    char *cmdline = malloc(clen + 16);
    if (!cmdline) {
        CloseHandle(outR); CloseHandle(outW);
        CloseHandle(errR); CloseHandle(errW);
        return -1;
    }
    snprintf(cmdline, clen + 16, "cmd /c %s", command);

    STARTUPINFOA si = {0};
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdOutput = outW;
    si.hStdError = errW;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    PROCESS_INFORMATION pi = {0};

    BOOL ok = CreateProcessA(NULL, cmdline, NULL, NULL, TRUE,
                             CREATE_NO_WINDOW, NULL,
                             (cwd && *cwd) ? cwd : NULL, &si, &pi);
    free(cmdline);
    CloseHandle(outW);
    CloseHandle(errW);
    if (!ok) {
        CloseHandle(outR); CloseHandle(errR);
        return -1;
    }

    buf so = {0}, se = {0};
    uint64_t deadline = monotonic_ms() + (uint64_t)timeout_secs * 1000ULL;
    int killed = 0;
    while (1) {
        if (cancel && *cancel) { TerminateProcess(pi.hProcess, 1); killed = 1; break; }
        if (monotonic_ms() >= deadline) { TerminateProcess(pi.hProcess, 1); killed = 1; break; }
        win_drain(outR, &so);
        win_drain(errR, &se);
        DWORD wait = WaitForSingleObject(pi.hProcess, 50);
        if (wait == WAIT_OBJECT_0) break;
    }
    /* drain any remaining output */
    while (win_drain(outR, &so) > 0) {}
    while (win_drain(errR, &se) > 0) {}

    DWORD status = 0;
    GetExitCodeProcess(pi.hProcess, &status);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    CloseHandle(outR);
    CloseHandle(errR);

    char code_buf[32];
    snprintf(code_buf, sizeof(code_buf), "%lu", status);
    buf_putf(out, "exit_code: %s\nstdout:\n", code_buf);
    if (so.len) buf_put(out, so.data, so.len);
    buf_puts(out, "\nstderr:\n");
    if (se.len) buf_put(out, se.data, se.len);
    buf_free(&so);
    buf_free(&se);

    if (killed && cancel && *cancel)  return -2;
    if (killed)                        return -3;
    return 0;
}

#endif

/* ============================================================== */
/* tool call parsing — mirrors common::ai::parsing                  */
/* ============================================================== */

/* find the matching '}' for the '{' at start; returns -1 if none */
static long find_match_brace(const char *s, size_t n, size_t start)
{
    if (start >= n || s[start] != '{') return -1;
    int depth = 0, in_str = 0, esc = 0;
    for (size_t i = start; i < n; i++) {
        char c = s[i];
        if (esc) { esc = 0; continue; }
        if (c == '\\' && in_str) { esc = 1; continue; }
        if (c == '"') { in_str = !in_str; continue; }
        if (in_str) continue;
        if (c == '{') depth++;
        else if (c == '}') {
            depth--;
            if (depth == 0) return (long)i;
        }
    }
    return -1;
}

/* Returns 1 if a tool-call JSON block is found; fills out_tool, out_args.
 * out_tool: malloc'd cstring; out_args: the *raw JSON object* string for
 * the args field (malloc'd) plus its length. */
static int parse_tool_call(const char *text, size_t n,
                           char **out_tool, char **out_args_json, size_t *out_args_len)
{
    *out_tool = NULL;
    *out_args_json = NULL;
    *out_args_len = 0;

    /* search for the substring "tool": which is preceded by an opening
     * brace and zero or more whitespace */
    for (size_t i = 0; i + 6 < n; i++) {
        if (memcmp(text + i, "\"tool\"", 6) != 0) continue;
        /* walk backwards to nearest '{' */
        long bs = -1;
        for (long j = (long)i - 1; j >= 0; j--) {
            if (text[j] == '{') { bs = j; break; }
            if (!isspace((unsigned char)text[j])) { bs = -2; break; }
        }
        if (bs < 0) continue;
        long end = find_match_brace(text, n, (size_t)bs);
        if (end < 0) continue;

        json *j = json_parse(text + bs, (size_t)(end - bs + 1));
        if (!j) continue;
        const char *tname; size_t tlen;
        if (!json_get_str(j, "tool", &tname, &tlen)) { json_free(j); continue; }
        json *args = json_get(j, "args");
        if (!args || args->type != JOBJ) { json_free(j); continue; }

        /* We need the raw JSON of args to forward to run_command. We
         * built one from the parsed tree by re-scanning the source for
         * the args field. Simpler: walk the source and extract it. */
        /* find "args" key in source range, then the matching brace */
        size_t span = (size_t)(end - bs + 1);
        const char *p = text + bs;
        for (size_t k = 0; k + 6 < span; k++) {
            if (memcmp(p + k, "\"args\"", 6) != 0) continue;
            size_t off = k + 6;
            while (off < span && (p[off] == ' ' || p[off] == '\t')) off++;
            if (off < span && p[off] == ':') off++;
            while (off < span && (p[off] == ' ' || p[off] == '\t' || p[off] == '\n' || p[off] == '\r')) off++;
            if (off >= span || p[off] != '{') break;
            long ae = find_match_brace(p, span, off);
            if (ae < 0) break;
            size_t alen = (size_t)(ae - off + 1);
            char *adup = malloc(alen + 1);
            if (!adup) break;
            memcpy(adup, p + off, alen);
            adup[alen] = 0;
            *out_args_json = adup;
            *out_args_len = alen;
            break;
        }

        char *tdup = malloc(tlen + 1);
        if (!tdup) { json_free(j); free(*out_args_json); *out_args_json = NULL; return 0; }
        memcpy(tdup, tname, tlen);
        tdup[tlen] = 0;
        *out_tool = tdup;
        json_free(j);
        return 1;
    }
    return 0;
}

/* ============================================================== */
/* AI request builder + SSE chunk handler                          */
/* ============================================================== */

typedef struct {
    session     *sess;
    buf          assistant;            /* full streamed assistant text */
    int          stream_failed;
    char        *system_prompt;        /* tool-augmented; may be NULL */

    //
    // Once we detect the start of a tool-call JSON in the assistant
    // stream we stop forwarding agent_message_chunk notifications: the
    // raw JSON would otherwise leak into the user-visible transcript.
    // After the stream finishes the prompt loop will emit a proper
    // tool_call / tool_call_update pair instead.
    //
    // streamed_len: bytes already emitted as agent_message_chunk;
    //               used to compute what to send when we receive a
    //               new SSE delta but haven't yet seen the suppress
    //               marker.
    //
    int          tool_started;
    size_t       streamed_len;
} stream_ctx;

//
// Decide whether the buffer starting at p[0] is the start of a
// tool-call JSON object — `{` optionally followed by whitespace and
// then the literal `"tool"`. Returns:
//
//   1  — yes, this is a tool-call marker.
//  -1  — definitely not (we have enough bytes to rule it out).
//   0  — undecided (need more bytes to know either way; caller should
//         hold back streaming the buffer until more SSE deltas arrive
//         or the stream ends).
//

static int could_be_marker(const char *p, size_t n)
{
    if (n == 0) return 0;
    if (p[0] != '{') return -1;
    size_t i = 1;
    while (i < n && (p[i] == ' ' || p[i] == '\t' ||
                     p[i] == '\n' || p[i] == '\r')) i++;
    static const char want[] = "\"tool\"";
    static const size_t wlen = 6;
    size_t avail = n - i;
    if (avail == 0) return 0;
    size_t cmp_len = avail < wlen ? avail : wlen;
    if (memcmp(p + i, want, cmp_len) != 0) return -1;
    return avail < wlen ? 0 : 1;
}

static const char DEFAULT_SYSTEM_PROMPT[] =
    "You are Praxis, an autonomous agent running on the target system. "
    "You have access to a run_command tool that lets you execute shell "
    "commands. Use it carefully and only when necessary.";

static const char TOOL_CALLING_PROMPT[] =
    "## Tool Calling Format\n\n"
    "To call a tool, output the JSON directly as plain text (NO code fences):\n\n"
    "{\"tool\": \"tool_name\", \"args\": {\"param1\": \"value1\"}}\n\n"
    "IMPORTANT: Do NOT wrap tool calls in code fences. Output the raw JSON "
    "object directly. Output ONE tool call per message and STOP after it. "
    "Do not write text after the tool call. Wait for the tool result in a "
    "subsequent message.";

static char *build_system_prompt(const char *base)
{
    buf b = {0};
    buf_puts(&b, base ? base : DEFAULT_SYSTEM_PROMPT);
    buf_puts(&b, "\n\n## Available Tools\n\n");
    buf_puts(&b, "### run_command\n");
    buf_puts(&b, "Execute a shell command on the target system.\n\n");
    buf_puts(&b, "Parameters: {\"type\":\"object\",\"properties\":{"
                  "\"command\":{\"type\":\"string\",\"description\":\"The shell command to execute\"},"
                  "\"working_dir\":{\"type\":\"string\",\"description\":\"Optional working directory\"}"
                  "},\"required\":[\"command\"]}\n\n");
    buf_puts(&b, TOOL_CALLING_PROMPT);
    buf_putc(&b, 0);
    return b.data;
}

//
// Emit a slice of assistant text as an agent_message_chunk session
// update. Caller-provided len must be > 0.
//

static void send_agent_chunk(session *s, const char *txt, size_t len)
{
    buf u = {0};
    buf_puts(&u, "{\"sessionUpdate\":\"agent_message_chunk\","
                  "\"content\":{\"type\":\"text\",\"text\":");
    jb_str(&u, txt, len);
    buf_puts(&u, "}}");
    buf_putc(&u, 0);
    acp_send_session_notification(s->client_id, s->id, u.data);
    buf_free(&u);
}

//
// Drain as much of the accumulated assistant text as we can safely
// stream, holding back any tail that could be the start of a
// tool-call JSON marker. Once we recognise a marker `tool_started`
// is set and the suppressed tail is left in ctx->assistant for the
// prompt loop to parse.
//

static void drain_stream(stream_ctx *ctx)
{
    while (!ctx->tool_started) {
        size_t left = ctx->assistant.len - ctx->streamed_len;
        if (left == 0) return;
        const char *base = ctx->assistant.data + ctx->streamed_len;

        //
        // Stream everything up to the next `{` — that's the only
        // character that could begin a tool-call marker.
        //
        size_t p = 0;
        while (p < left && base[p] != '{') p++;
        if (p > 0) {
            send_agent_chunk(ctx->sess, base, p);
            ctx->streamed_len += p;
            base += p;
            left -= p;
            if (left == 0) return;
        }

        //
        // We're sitting on a `{`. Decide whether it's the start of a
        // tool-call marker, definitely not, or undecidable yet.
        //
        int verdict = could_be_marker(base, left);
        if (verdict == 1) {
            ctx->tool_started = 1;
            return;
        }
        if (verdict == 0) {
            //
            // Not enough bytes to decide — hold back everything from
            // this brace onward and wait for the next SSE delta.
            //
            return;
        }
        //
        // Not a marker — emit the brace and continue scanning.
        //
        send_agent_chunk(ctx->sess, base, 1);
        ctx->streamed_len += 1;
    }
}

/* called for each SSE "data:" payload */
static void on_sse_chunk(const char *data, size_t n, void *ud)
{
    stream_ctx *ctx = ud;

    /* OpenAI sends "[DONE]" sentinel */
    if (n == 6 && memcmp(data, "[DONE]", 6) == 0) return;

    json *j = json_parse(data, n);
    if (!j) return;

    /* navigate choices[0].delta.content */
    json *choices = json_get(j, "choices");
    if (choices && choices->type == JARR && choices->u.arr.count > 0) {
        json *first = choices->u.arr.items[0];
        json *delta = json_get(first, "delta");
        if (delta) {
            const char *txt; size_t tn;
            if (json_get_str(delta, "content", &txt, &tn) && tn > 0) {
                buf_put(&ctx->assistant, txt, tn);
                drain_stream(ctx);
            }
        }
    }
    json_free(j);
}

//
// Emit a `tool_call` session update announcing that a new tool call
// has started. raw_input is the JSON object string (no enclosing
// braces stripped) describing the parsed tool arguments — embedded
// verbatim under the rawInput field. Pass NULL/0 to omit it.
//

static void emit_tool_call(session *s, const char *tool_call_id,
                           const char *title,
                           const char *raw_input, size_t raw_input_len)
{
    buf u = {0};
    buf_puts(&u, "{\"sessionUpdate\":\"tool_call\",\"toolCallId\":");
    jb_strz(&u, tool_call_id);
    buf_puts(&u, ",\"title\":");
    jb_strz(&u, title);
    buf_puts(&u, ",\"kind\":\"execute\",\"status\":\"in_progress\"");
    if (raw_input && raw_input_len > 0) {
        buf_puts(&u, ",\"rawInput\":");
        buf_put(&u, raw_input, raw_input_len);
    }
    buf_putc(&u, '}');
    buf_putc(&u, 0);
    acp_send_session_notification(s->client_id, s->id, u.data);
    buf_free(&u);
}

//
// Emit a `tool_call_update` carrying the final status and result
// content for a previously-announced tool call.
//

static void emit_tool_result(session *s, const char *tool_call_id,
                             int success, const char *output, size_t out_len)
{
    buf u = {0};
    buf_puts(&u, "{\"sessionUpdate\":\"tool_call_update\",\"toolCallId\":");
    jb_strz(&u, tool_call_id);
    buf_puts(&u, ",\"status\":");
    buf_puts(&u, success ? "\"completed\"" : "\"failed\"");
    if (output && out_len > 0) {
        buf_puts(&u, ",\"content\":[{\"type\":\"content\","
                      "\"content\":{\"type\":\"text\",\"text\":");
        jb_str(&u, output, out_len);
        buf_puts(&u, "}}]");
    }
    buf_putc(&u, '}');
    buf_putc(&u, 0);
    acp_send_session_notification(s->client_id, s->id, u.data);
    buf_free(&u);
}

/* serialize the message history as an OpenAI chat-completions request body */
static void build_request_body(buf *out, const char *model, ai_msg *msgs)
{
    buf_puts(out, "{\"model\":");
    jb_strz(out, model);
    buf_puts(out, ",\"stream\":true,\"messages\":[");
    int first = 1;
    for (ai_msg *m = msgs; m; m = m->next) {
        if (!first) buf_putc(out, ',');
        first = 0;
        buf_puts(out, "{\"role\":");
        jb_strz(out, m->role);
        buf_puts(out, ",\"content\":");
        jb_strz(out, m->content ? m->content : "");
        buf_putc(out, '}');
    }
    buf_puts(out, "]}");
}

/* append a new message to the linked list */
static void msg_append(ai_msg **head, const char *role, const char *content)
{
    ai_msg *m = calloc(1, sizeof(*m));
    if (!m) return;
    m->role = strdup(role);
    m->content = content ? strdup(content) : strdup("");
    if (!*head) { *head = m; return; }
    ai_msg *cur = *head;
    while (cur->next) cur = cur->next;
    cur->next = m;
}

/* ============================================================== */
/* prompt worker thread                                             */
/* ============================================================== */

typedef struct {
    session  *sess;
    char     *prompt;
    char     *id_raw;        /* JSON-RPC id for the prompt request, raw bytes */
} prompt_args;

static int extract_run_command_args(const char *args_json, size_t alen,
                                    char **out_cmd, char **out_cwd)
{
    *out_cmd = NULL; *out_cwd = NULL;
    json *j = json_parse(args_json, alen);
    if (!j) return 0;
    const char *cs; size_t cn;
    if (!json_get_str(j, "command", &cs, &cn)) { json_free(j); return 0; }
    char *cmd = malloc(cn + 1); if (!cmd) { json_free(j); return 0; }
    memcpy(cmd, cs, cn); cmd[cn] = 0;
    const char *ws; size_t wn;
    char *wd = NULL;
    if (json_get_str(j, "working_dir", &ws, &wn) && wn > 0) {
        wd = malloc(wn + 1);
        if (wd) { memcpy(wd, ws, wn); wd[wn] = 0; }
    }
    *out_cmd = cmd;
    *out_cwd = wd;
    json_free(j);
    return 1;
}

static void *prompt_worker(void *arg)
{
    prompt_args *pa = arg;
    session *s = pa->sess;

    praxis_cfg *cfg = snapshot_cfg();
    if (!cfg) {
        acp_send_error(s->client_id, pa->id_raw, -32603, "Praxis agent not configured");
        free(pa->prompt); free(pa->id_raw); free(pa);
        s->busy = 0;
        return NULL;
    }

    /* parse endpoint url */
    char *host = NULL, *path = NULL;
    int port = 80;
    int use_tls = 0;
    if (!cfg->endpoint_url ||
        http_parse_url(cfg->endpoint_url, &host, &port, &path, &use_tls) < 0) {
        acp_send_error(s->client_id, pa->id_raw, -32603, "Invalid endpoint_url");
        praxis_cfg_free(cfg);
        free(pa->prompt); free(pa->id_raw); free(pa);
        s->busy = 0;
        return NULL;
    }

    /* path must end in /chat/completions for OpenAI-compatible APIs */
    char *full_path = NULL;
    {
        size_t plen = strlen(path);
        const char *suffix = "/chat/completions";
        size_t slen = strlen(suffix);
        if (plen >= slen && memcmp(path + plen - slen, suffix, slen) == 0) {
            full_path = strdup(path);
        } else {
            buf p = {0};
            if (plen > 0 && path[plen - 1] == '/') buf_put(&p, path, plen - 1);
            else                                    buf_puts(&p, path);
            buf_puts(&p, suffix);
            buf_putc(&p, 0);
            full_path = p.data;
        }
    }
    free(path);

    pthread_mutex_lock(&s->mu);
    s->cancel = 0;

    /* seed system + user message */
    if (!s->messages) {
        char *sysp = build_system_prompt(cfg->system_prompt);
        msg_append(&s->messages, "system", sysp);
        free(sysp);
    }
    msg_append(&s->messages, "user", pa->prompt);
    pthread_mutex_unlock(&s->mu);

    int max_iters = cfg->max_tool_iters > 0 ? cfg->max_tool_iters : 10;
    int cmd_timeout = cfg->command_timeout_secs > 0 ? cfg->command_timeout_secs : 60;
    int cancelled = 0;
    int erred = 0;

    char auth_header[1024];
    snprintf(auth_header, sizeof(auth_header), "Authorization: Bearer %s",
             cfg->api_key ? cfg->api_key : "");
    const char *headers[] = {
        auth_header,
        "Content-Type: application/json",
        NULL
    };

    for (int iter = 0; iter < max_iters; iter++) {
        if (s->cancel) { cancelled = 1; break; }

        buf body = {0};
        pthread_mutex_lock(&s->mu);
        build_request_body(&body, cfg->model_name ? cfg->model_name : "gpt-4o-mini",
                           s->messages);
        pthread_mutex_unlock(&s->mu);

        stream_ctx ctx = {0};
        ctx.sess = s;

        int rc = http_post_sse(host, port, use_tls, full_path, headers,
                               body.data, body.len,
                               on_sse_chunk, &ctx, &s->cancel);
        buf_free(&body);

        if (rc == -2) { cancelled = 1; buf_free(&ctx.assistant); break; }
        if (rc < 0) {
            erred = 1;
            buf_free(&ctx.assistant);
            break;
        }

        /* tool-call detection on the full streamed text */
        char *tool = NULL, *args_json = NULL;
        size_t alen = 0;
        char *full = NULL;
        size_t full_len = ctx.assistant.len;
        if (ctx.assistant.data) {
            buf_putc(&ctx.assistant, 0);
            full = ctx.assistant.data;
            ctx.assistant.data = NULL;
            ctx.assistant.cap = 0;
        } else {
            full = strdup("");
        }

        if (parse_tool_call(full, full_len, &tool, &args_json, &alen) &&
            tool && strcmp(tool, "run_command") == 0 && args_json) {

            /* persist assistant text (with the tool call) */
            pthread_mutex_lock(&s->mu);
            msg_append(&s->messages, "assistant", full);
            pthread_mutex_unlock(&s->mu);

            char *cmd = NULL, *cwd_extra = NULL;
            if (!extract_run_command_args(args_json, alen, &cmd, &cwd_extra)) {
                pthread_mutex_lock(&s->mu);
                msg_append(&s->messages, "user", "Tool result for run_command:\nerror: missing 'command'");
                pthread_mutex_unlock(&s->mu);
                free(tool); free(args_json); free(full);
                continue;
            }

            const char *cwd = cwd_extra && *cwd_extra ? cwd_extra : (s->cwd ? s->cwd : NULL);

            //
            // Emit a proper ACP tool_call session update for the
            // invocation, run the command, then emit a tool_call_update
            // with the captured output.
            //

            char tc_id[37];
            uuid_v4(tc_id);
            emit_tool_call(s, tc_id, cmd, args_json, alen);

            buf out = {0};
            int rc_cmd = run_command(cmd, cwd, cmd_timeout, &s->cancel, &out);
            buf_putc(&out, 0);

            emit_tool_result(s, tc_id, rc_cmd == 0,
                             out.data, out.data ? strlen(out.data) : 0);

            buf result_msg = {0};
            buf_puts(&result_msg, "Tool result for run_command:\n");
            buf_puts(&result_msg, out.data);
            buf_putc(&result_msg, 0);
            pthread_mutex_lock(&s->mu);
            msg_append(&s->messages, "user", result_msg.data);
            pthread_mutex_unlock(&s->mu);

            buf_free(&out);
            buf_free(&result_msg);
            free(cmd); free(cwd_extra);
            free(tool); free(args_json);
            free(full);
            continue;
        }

        //
        // No tool call → final answer. If we held back any tail of
        // the stream (either because of a false-positive tool marker
        // or because drain_stream paused on an undecided `{` that
        // never resolved) flush it now so the user sees what the LLM
        // actually produced.
        //

        if (ctx.streamed_len < full_len) {
            send_agent_chunk(s, full + ctx.streamed_len,
                             full_len - ctx.streamed_len);
        }

        pthread_mutex_lock(&s->mu);
        msg_append(&s->messages, "assistant", full);
        pthread_mutex_unlock(&s->mu);

        free(tool); free(args_json); free(full);
        break;
    }

    /* respond to the prompt request */
    if (cancelled) {
        char result[64];
        snprintf(result, sizeof(result), "{\"stopReason\":\"cancelled\"}");
        acp_send_response(s->client_id, pa->id_raw, result);
    } else if (erred) {
        acp_send_error(s->client_id, pa->id_raw, -32603, "transact failed");
    } else {
        acp_send_response(s->client_id, pa->id_raw, "{\"stopReason\":\"end_turn\"}");
    }

    free(host);
    free(full_path);
    praxis_cfg_free(cfg);
    free(pa->prompt); free(pa->id_raw); free(pa);
    s->busy = 0;
    return NULL;
}

/* ============================================================== */
/* ACP dispatch                                                     */
/* ============================================================== */

static void send_initialize_response(const char *client_id, const char *id_raw)
{
    buf r = {0};
    buf_puts(&r, "{\"protocolVersion\":1,\"agentInfo\":{\"name\":\"praxis-node-tiny-c\","
                 "\"version\":\"0.1.0\"},\"agentCapabilities\":{},"
                 "\"_meta\":{\"extensions\":{},\"connectors\":[");
    if (praxis_enabled()) {
        buf_puts(&r, "{\"shortName\":\"praxis\",\"name\":\"Praxis Agent\"}");
    }
    buf_puts(&r, "],\"nodeId\":");
    jb_strz(&r, tiny_node_id);
    buf_puts(&r, "}}");
    buf_putc(&r, 0);
    acp_send_response(client_id, id_raw, r.data);
    buf_free(&r);
}

static void handle_session_new(const char *client_id, const char *id_raw, json *params)
{
    /* require _meta.praxis.connector == "praxis" */
    const char *connector = NULL; size_t cn = 0;
    json *meta = json_get(params, "_meta");
    json *praxis = meta ? json_get(meta, "praxis") : NULL;
    if (!praxis || !json_get_str(praxis, "connector", &connector, &cn) ||
        cn != 6 || memcmp(connector, "praxis", 6) != 0) {
        acp_send_error(client_id, id_raw, -32602, "Missing _meta.praxis.connector");
        return;
    }
    if (!praxis_enabled()) {
        acp_send_error(client_id, id_raw, -32602, "Unknown connector 'praxis'");
        return;
    }

    session *s = calloc(1, sizeof(*s));
    if (!s) { acp_send_error(client_id, id_raw, -32603, "alloc failed"); return; }
    pthread_mutex_init(&s->mu, NULL);
    uuid_v4(s->id);
    snprintf(s->client_id, sizeof(s->client_id), "%s", client_id);
    const char *cwd; size_t cwlen;
    if (json_get_str(params, "cwd", &cwd, &cwlen) && cwlen > 0) {
        s->cwd = malloc(cwlen + 1);
        if (s->cwd) { memcpy(s->cwd, cwd, cwlen); s->cwd[cwlen] = 0; }
    }

    pthread_mutex_lock(&sess_mu);
    s->next = sess_head;
    sess_head = s;
    pthread_mutex_unlock(&sess_mu);

    buf r = {0};
    buf_puts(&r, "{\"sessionId\":");
    jb_strz(&r, s->id);
    buf_putc(&r, '}');
    buf_putc(&r, 0);
    acp_send_response(client_id, id_raw, r.data);
    buf_free(&r);
}

static void handle_session_prompt(const char *client_id, const char *id_raw, json *params)
{
    const char *sid; size_t snl;
    if (!json_get_str(params, "sessionId", &sid, &snl)) {
        acp_send_error(client_id, id_raw, -32602, "Missing sessionId");
        return;
    }
    char sidbuf[64];
    if (snl >= sizeof(sidbuf)) {
        acp_send_error(client_id, id_raw, -32602, "Invalid sessionId");
        return;
    }
    memcpy(sidbuf, sid, snl); sidbuf[snl] = 0;
    session *s = session_find(sidbuf);
    if (!s) {
        acp_send_error(client_id, id_raw, -32602, "Session not found");
        return;
    }

    /* concat all text blocks in prompt */
    json *prompt = json_get(params, "prompt");
    buf text = {0};
    if (prompt && prompt->type == JARR) {
        for (size_t i = 0; i < prompt->u.arr.count; i++) {
            json *blk = prompt->u.arr.items[i];
            const char *t; size_t tn;
            if (json_get_str(blk, "type", &t, &tn) && tn == 4 && memcmp(t, "text", 4) == 0) {
                const char *tx; size_t txn;
                if (json_get_str(blk, "text", &tx, &txn)) {
                    if (text.len) buf_putc(&text, '\n');
                    buf_put(&text, tx, txn);
                }
            }
        }
    }
    if (text.len == 0) {
        buf_free(&text);
        acp_send_error(client_id, id_raw, -32602, "Empty prompt");
        return;
    }
    buf_putc(&text, 0);

    if (s->busy) {
        buf_free(&text);
        acp_send_error(client_id, id_raw, -32603, "Session is already running a prompt");
        return;
    }
    s->busy = 1;

    prompt_args *pa = calloc(1, sizeof(*pa));
    pa->sess = s;
    pa->prompt = text.data;       /* take ownership; do not buf_free */
    pa->id_raw = strdup(id_raw ? id_raw : "null");

    pthread_t th;
    if (pthread_create(&th, NULL, prompt_worker, pa) != 0) {
        free(pa->prompt); free(pa->id_raw); free(pa);
        s->busy = 0;
        acp_send_error(client_id, id_raw, -32603, "Failed to spawn worker");
        return;
    }
    workers_track(th);
}

static void handle_session_cancel(const char *client_id, json *params)
{
    (void)client_id;
    const char *sid; size_t sn;
    if (!json_get_str(params, "sessionId", &sid, &sn)) return;
    char sidbuf[64];
    if (sn >= sizeof(sidbuf)) return;
    memcpy(sidbuf, sid, sn); sidbuf[sn] = 0;
    session *s = session_find(sidbuf);
    if (!s) return;
    s->cancel = 1;
}

static void handle_session_close(const char *client_id, const char *id_raw, json *params)
{
    const char *sid; size_t sn;
    if (json_get_str(params, "sessionId", &sid, &sn) && sn < 64) {
        char sidbuf[64];
        memcpy(sidbuf, sid, sn); sidbuf[sn] = 0;
        session_remove(sidbuf);
    }
    if (id_raw) acp_send_response(client_id, id_raw, "{}");
}

static void handle_session_list(const char *client_id, const char *id_raw)
{
    buf r = {0};
    buf_puts(&r, "{\"sessions\":[");
    pthread_mutex_lock(&sess_mu);
    int first = 1;
    for (session *s = sess_head; s; s = s->next) {
        if (!first) buf_putc(&r, ',');
        first = 0;
        buf_puts(&r, "{\"sessionId\":");
        jb_strz(&r, s->id);
        buf_puts(&r, ",\"cwd\":");
        jb_strz(&r, s->cwd ? s->cwd : ".");
        buf_puts(&r, ",\"title\":\"praxis\"}");
    }
    pthread_mutex_unlock(&sess_mu);
    buf_puts(&r, "]}");
    buf_putc(&r, 0);
    acp_send_response(client_id, id_raw, r.data);
    buf_free(&r);
}

void acp_handle_frame(const char *client_id, const char *rpc, size_t rpc_len)
{
    json *root = json_parse(rpc, rpc_len);
    if (!root) {
        LOG_WARN("acp: invalid JSON-RPC from %.16s", client_id);
        return;
    }

    const char *method; size_t mn;
    int has_method = json_get_str(root, "method", &method, &mn);
    json *id_node = json_get(root, "id");

    /* serialize the id as JSON to forward verbatim into responses */
    char id_buf[64];
    const char *id_raw = NULL;
    if (id_node) {
        if (id_node->type == JNUM) {
            long iv = (long)id_node->u.n;
            snprintf(id_buf, sizeof(id_buf), "%ld", iv);
            id_raw = id_buf;
        } else if (id_node->type == JSTR) {
            buf jb = {0};
            jb_str(&jb, id_node->u.str.s, id_node->u.str.len);
            buf_putc(&jb, 0);
            if (jb.len < sizeof(id_buf)) {
                memcpy(id_buf, jb.data, jb.len);
                id_raw = id_buf;
            }
            buf_free(&jb);
        }
    }

    if (!has_method) { json_free(root); return; }

    json *params = json_get(root, "params");

    if (mn == 10 && memcmp(method, "initialize", 10) == 0 && id_raw) {
        send_initialize_response(client_id, id_raw);
    } else if (mn == 11 && memcmp(method, "session/new", 11) == 0 && id_raw) {
        handle_session_new(client_id, id_raw, params);
    } else if (mn == 14 && memcmp(method, "session/prompt", 14) == 0 && id_raw) {
        handle_session_prompt(client_id, id_raw, params);
    } else if (mn == 14 && memcmp(method, "session/cancel", 14) == 0) {
        handle_session_cancel(client_id, params);
    } else if (mn == 13 && memcmp(method, "session/close", 13) == 0) {
        handle_session_close(client_id, id_raw, params);
    } else if (mn == 12 && memcmp(method, "session/list", 12) == 0 && id_raw) {
        handle_session_list(client_id, id_raw);
    } else {
        if (id_raw) acp_send_error(client_id, id_raw, -32601, "Method not found");
    }

    json_free(root);
}
