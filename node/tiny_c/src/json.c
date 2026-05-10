#include "tiny.h"

#include <ctype.h>
#include <stdlib.h>
#include <string.h>

/* ----- parser --------------------------------------------------- */

typedef struct {
    const char *s;
    size_t      n;
    size_t      i;
} parser;

static void skip_ws(parser *p)
{
    while (p->i < p->n) {
        char c = p->s[p->i];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') p->i++;
        else break;
    }
}

static json *jnew(json_type t) {
    json *j = calloc(1, sizeof(*j));
    if (j) j->type = t;
    return j;
}

void json_free(json *j)
{
    if (!j) return;
    switch (j->type) {
    case JSTR: free(j->u.str.s); break;
    case JARR:
        for (size_t i = 0; i < j->u.arr.count; i++) json_free(j->u.arr.items[i]);
        free(j->u.arr.items);
        break;
    case JOBJ:
        for (size_t i = 0; i < j->u.obj.count; i++) {
            free(j->u.obj.keys[i]);
            json_free(j->u.obj.vals[i]);
        }
        free(j->u.obj.keys);
        free(j->u.obj.key_lens);
        free(j->u.obj.vals);
        break;
    default: break;
    }
    free(j);
}

static json *parse_value(parser *p);

static int parse_hex4(const char *s, unsigned *out)
{
    unsigned v = 0;
    for (int i = 0; i < 4; i++) {
        char c = s[i];
        v <<= 4;
        if (c >= '0' && c <= '9') v |= (unsigned)(c - '0');
        else if (c >= 'a' && c <= 'f') v |= (unsigned)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') v |= (unsigned)(c - 'A' + 10);
        else return -1;
    }
    *out = v;
    return 0;
}

/* parse a JSON string at p->i (which must be on the opening "). On
 * success, allocates and returns a NUL-terminated cstring with the
 * decoded value plus its byte length in *len_out. */
static char *parse_string(parser *p, size_t *len_out)
{
    if (p->i >= p->n || p->s[p->i] != '"') return NULL;
    p->i++;
    buf b = {0};
    while (p->i < p->n) {
        unsigned char c = (unsigned char)p->s[p->i++];
        if (c == '"') {
            buf_putc(&b, 0);
            *len_out = b.len - 1;
            return b.data;
        }
        if (c == '\\') {
            if (p->i >= p->n) goto fail;
            char e = p->s[p->i++];
            switch (e) {
            case '"': case '\\': case '/': buf_putc(&b, e); break;
            case 'b': buf_putc(&b, '\b'); break;
            case 'f': buf_putc(&b, '\f'); break;
            case 'n': buf_putc(&b, '\n'); break;
            case 'r': buf_putc(&b, '\r'); break;
            case 't': buf_putc(&b, '\t'); break;
            case 'u': {
                if (p->i + 4 > p->n) goto fail;
                unsigned cp;
                if (parse_hex4(p->s + p->i, &cp) < 0) goto fail;
                p->i += 4;
                if (cp >= 0xD800 && cp <= 0xDBFF) {
                    if (p->i + 6 > p->n || p->s[p->i] != '\\' || p->s[p->i + 1] != 'u')
                        goto fail;
                    unsigned lo;
                    if (parse_hex4(p->s + p->i + 2, &lo) < 0) goto fail;
                    if (lo < 0xDC00 || lo > 0xDFFF) goto fail;
                    p->i += 6;
                    cp = 0x10000 + (((cp - 0xD800) << 10) | (lo - 0xDC00));
                }
                if (cp < 0x80) buf_putc(&b, (char)cp);
                else if (cp < 0x800) {
                    buf_putc(&b, (char)(0xC0 | (cp >> 6)));
                    buf_putc(&b, (char)(0x80 | (cp & 0x3F)));
                } else if (cp < 0x10000) {
                    buf_putc(&b, (char)(0xE0 | (cp >> 12)));
                    buf_putc(&b, (char)(0x80 | ((cp >> 6) & 0x3F)));
                    buf_putc(&b, (char)(0x80 | (cp & 0x3F)));
                } else {
                    buf_putc(&b, (char)(0xF0 | (cp >> 18)));
                    buf_putc(&b, (char)(0x80 | ((cp >> 12) & 0x3F)));
                    buf_putc(&b, (char)(0x80 | ((cp >> 6) & 0x3F)));
                    buf_putc(&b, (char)(0x80 | (cp & 0x3F)));
                }
                break;
            }
            default: goto fail;
            }
        } else if (c < 0x20) {
            goto fail;
        } else {
            buf_putc(&b, (char)c);
        }
    }
fail:
    buf_free(&b);
    return NULL;
}

static json *parse_number(parser *p)
{
    size_t start = p->i;
    if (p->s[p->i] == '-') p->i++;
    while (p->i < p->n && isdigit((unsigned char)p->s[p->i])) p->i++;
    if (p->i < p->n && p->s[p->i] == '.') {
        p->i++;
        while (p->i < p->n && isdigit((unsigned char)p->s[p->i])) p->i++;
    }
    if (p->i < p->n && (p->s[p->i] == 'e' || p->s[p->i] == 'E')) {
        p->i++;
        if (p->i < p->n && (p->s[p->i] == '+' || p->s[p->i] == '-')) p->i++;
        while (p->i < p->n && isdigit((unsigned char)p->s[p->i])) p->i++;
    }
    char tmp[64];
    size_t n = p->i - start;
    if (n >= sizeof(tmp)) return NULL;
    memcpy(tmp, p->s + start, n);
    tmp[n] = 0;
    json *j = jnew(JNUM);
    if (!j) return NULL;
    j->u.n = strtod(tmp, NULL);
    return j;
}

static json *parse_object(parser *p)
{
    if (p->s[p->i] != '{') return NULL;
    p->i++;
    json *j = jnew(JOBJ);
    if (!j) return NULL;
    skip_ws(p);
    if (p->i < p->n && p->s[p->i] == '}') { p->i++; return j; }

    size_t cap = 0;
    while (1) {
        skip_ws(p);
        size_t klen = 0;
        char *k = parse_string(p, &klen);
        if (!k) goto fail;
        skip_ws(p);
        if (p->i >= p->n || p->s[p->i] != ':') { free(k); goto fail; }
        p->i++;
        skip_ws(p);
        json *v = parse_value(p);
        if (!v) { free(k); goto fail; }
        if (j->u.obj.count == cap) {
            cap = cap ? cap * 2 : 4;
            j->u.obj.keys     = realloc(j->u.obj.keys,     cap * sizeof(char *));
            j->u.obj.key_lens = realloc(j->u.obj.key_lens, cap * sizeof(size_t));
            j->u.obj.vals     = realloc(j->u.obj.vals,     cap * sizeof(json *));
        }
        j->u.obj.keys    [j->u.obj.count] = k;
        j->u.obj.key_lens[j->u.obj.count] = klen;
        j->u.obj.vals    [j->u.obj.count] = v;
        j->u.obj.count++;
        skip_ws(p);
        if (p->i >= p->n) goto fail;
        if (p->s[p->i] == ',') { p->i++; continue; }
        if (p->s[p->i] == '}') { p->i++; return j; }
        goto fail;
    }
fail:
    json_free(j);
    return NULL;
}

static json *parse_array(parser *p)
{
    if (p->s[p->i] != '[') return NULL;
    p->i++;
    json *j = jnew(JARR);
    if (!j) return NULL;
    skip_ws(p);
    if (p->i < p->n && p->s[p->i] == ']') { p->i++; return j; }
    size_t cap = 0;
    while (1) {
        skip_ws(p);
        json *v = parse_value(p);
        if (!v) goto fail;
        if (j->u.arr.count == cap) {
            cap = cap ? cap * 2 : 4;
            j->u.arr.items = realloc(j->u.arr.items, cap * sizeof(json *));
        }
        j->u.arr.items[j->u.arr.count++] = v;
        skip_ws(p);
        if (p->i >= p->n) goto fail;
        if (p->s[p->i] == ',') { p->i++; continue; }
        if (p->s[p->i] == ']') { p->i++; return j; }
        goto fail;
    }
fail:
    json_free(j);
    return NULL;
}

static json *parse_value(parser *p)
{
    skip_ws(p);
    if (p->i >= p->n) return NULL;
    char c = p->s[p->i];
    if (c == '"') {
        size_t len;
        char *s = parse_string(p, &len);
        if (!s) return NULL;
        json *j = jnew(JSTR);
        if (!j) { free(s); return NULL; }
        j->u.str.s = s;
        j->u.str.len = len;
        return j;
    }
    if (c == '{') return parse_object(p);
    if (c == '[') return parse_array(p);
    if (c == '-' || (c >= '0' && c <= '9')) return parse_number(p);
    if (p->i + 4 <= p->n && memcmp(p->s + p->i, "true",  4) == 0) {
        p->i += 4;
        json *j = jnew(JBOOL); if (j) j->u.b = 1; return j;
    }
    if (p->i + 5 <= p->n && memcmp(p->s + p->i, "false", 5) == 0) {
        p->i += 5;
        json *j = jnew(JBOOL); if (j) j->u.b = 0; return j;
    }
    if (p->i + 4 <= p->n && memcmp(p->s + p->i, "null",  4) == 0) {
        p->i += 4;
        return jnew(JNULL);
    }
    return NULL;
}

json *json_parse(const char *src, size_t n)
{
    parser p = {src, n, 0};
    json *j = parse_value(&p);
    if (!j) return NULL;
    skip_ws(&p);
    /* permit trailing junk; service messages may include nothing else */
    return j;
}

/* ----- accessors ------------------------------------------------ */

const char *json_str(json *j, size_t *len_out)
{
    if (!j || j->type != JSTR) { if (len_out) *len_out = 0; return NULL; }
    if (len_out) *len_out = j->u.str.len;
    return j->u.str.s;
}

static json *get_key(json *j, const char *k, size_t klen)
{
    if (!j || j->type != JOBJ) return NULL;
    for (size_t i = 0; i < j->u.obj.count; i++) {
        if (j->u.obj.key_lens[i] == klen && memcmp(j->u.obj.keys[i], k, klen) == 0)
            return j->u.obj.vals[i];
    }
    return NULL;
}

json *json_get(json *j, const char *path)
{
    if (!j || !path) return NULL;
    const char *p = path;
    json *cur = j;
    while (*p && cur) {
        const char *dot = strchr(p, '.');
        size_t kl = dot ? (size_t)(dot - p) : strlen(p);
        cur = get_key(cur, p, kl);
        if (!dot) break;
        p = dot + 1;
    }
    return cur;
}

int json_get_str(json *j, const char *path, const char **out, size_t *len_out)
{
    json *v = path ? json_get(j, path) : j;
    if (!v || v->type != JSTR) return 0;
    if (out) *out = v->u.str.s;
    if (len_out) *len_out = v->u.str.len;
    return 1;
}

int json_get_bool(json *j, const char *path, int *out)
{
    json *v = path ? json_get(j, path) : j;
    if (!v) return 0;
    if (v->type == JBOOL) { if (out) *out = v->u.b; return 1; }
    return 0;
}

int json_get_int(json *j, const char *path, long *out)
{
    json *v = path ? json_get(j, path) : j;
    if (!v || v->type != JNUM) return 0;
    if (out) *out = (long)v->u.n;
    return 1;
}

/* ----- writer --------------------------------------------------- */

void jb_str(buf *b, const char *s, size_t n)
{
    buf_putc(b, '"');
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        switch (c) {
        case '"':  buf_puts(b, "\\\""); break;
        case '\\': buf_puts(b, "\\\\"); break;
        case '\b': buf_puts(b, "\\b");  break;
        case '\f': buf_puts(b, "\\f");  break;
        case '\n': buf_puts(b, "\\n");  break;
        case '\r': buf_puts(b, "\\r");  break;
        case '\t': buf_puts(b, "\\t");  break;
        default:
            if (c < 0x20) buf_putf(b, "\\u%04x", c);
            else          buf_putc(b, (char)c);
        }
    }
    buf_putc(b, '"');
}

void jb_strz(buf *b, const char *s)
{
    jb_str(b, s, strlen(s));
}
