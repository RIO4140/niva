// الشبكة الأصلية — خلفية C (مهمة 29).
// HTTP/1.1 عادي بمقابس POSIX؛ وHTTPS يُرفض برسالة
// عربية (يحتاج قرار TLS منفصلا). قواعد التحويل كما
// في الاجتماعات 26 و28 و29 و30 (network.rs).
#include "niva_rt.h"
#ifndef _WIN32
// ترويسات مقابس POSIX — غير موجودة في MinGW
// (ويندوز): كل دوال شبكة معطلة هناك برفض عربي
// (اجتماع 34 §49 والاجتماع 35 §50)، فلا تُضمَّن
// أصلا على ويندوز. وnetinet/in.h صراحة لأن
// INADDR_LOOPBACK غير معلن بدونه على ماك
// (arpa/inet.h وحده لا يكفي هناك).
#include <arpa/inet.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <strings.h>
#include <sys/socket.h>
#include <dirent.h>
#include <ifaddrs.h>
#include <net/if.h>
#ifdef __linux__
#include <linux/errqueue.h>
#endif
#endif
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>
#include <utime.h>
#include <math.h>

#define NV_NET_TIMEOUT_SECS 30
#define NV_NET_MAX_RESPONSE (5 * 1024 * 1024)
#define NV_NET_MAX_REQUEST (5 * 1024 * 1024)
#define NV_NET_MAX_REDIRECTS 5
#define NV_NET_HEADER_CAP (64 * 1024)

// كل تنفيذ الشبكة أدناه POSIX خالص — يُستبعد
// كاملا على ويندوز (فرع #else أدناه: رفض عربي).
#ifndef _WIN32

// ---------- ساعة رتيبة ومهلة ----------
static long long nv_net_now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

// ---------- رابط محلل ----------
typedef struct {
    char scheme[8]; // http أو https
    char *host;
    int port;
    char *path; // يبدأ بـ / ويحمل الاستعلام
} NvUrl;

static void nv_url_free(NvUrl *u) {
    free(u->host);
    free(u->path);
}

static void nv_net_err_runtime(int line, int col, const char *fmt, ...) __attribute__((noreturn));
static void nv_net_err_runtime(int line, int col, const char *fmt, ...) {
    char buf[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    nv_error(NV_ERR_RUNTIME, buf, line, col);
}

// حلل رابطا (رسائل parse_url نفسها).
static void nv_parse_url(const char *url_in, NvUrl *out, int line, int col) {
    while (*url_in == ' ' || *url_in == '\t' || *url_in == '\n' || *url_in == '\r') url_in++;
    size_t ulen = strlen(url_in);
    while (ulen > 0 && (url_in[ulen - 1] == ' ' || url_in[ulen - 1] == '\t' || url_in[ulen - 1] == '\n' || url_in[ulen - 1] == '\r')) ulen--;
    char *url = (char *)malloc(ulen + 1);
    memcpy(url, url_in, ulen);
    url[ulen] = '\0';
    if (ulen == 0) {
        free(url);
        nv_error(NV_ERR_RUNTIME, "رابط فارغ", line, col);
    }
    for (size_t i = 0; i < ulen; i++) {
        unsigned char c = (unsigned char)url[i];
        if (c < 0x20 || c == 0x7f || c == ' ') {
            nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
        }
    }
    char *sep = strstr(url, "://");
    if (!sep) {
        free(url);
        nv_error(NV_ERR_RUNTIME, "الرابط يجب أن يبدأ بـ http:// أو https://", line, col);
    }
    size_t slen = (size_t)(sep - url);
    char scheme[8];
    if (slen >= sizeof(scheme)) {
        free(url);
        nv_error(NV_ERR_RUNTIME, "الرابط يجب أن يبدأ بـ http:// أو https://", line, col);
    }
    for (size_t i = 0; i < slen; i++) {
        char c = url[i];
        scheme[i] = (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c;
    }
    scheme[slen] = '\0';
    if (strcmp(scheme, "http") != 0 && strcmp(scheme, "https") != 0) {
        free(url);
        nv_error(NV_ERR_RUNTIME, "الرابط يجب أن يبدأ بـ http:// أو https://", line, col);
    }
    const char *rest = sep + 3;
    size_t auth_end = strcspn(rest, "/?#");
    char *authority = (char *)malloc(auth_end + 1);
    memcpy(authority, rest, auth_end);
    authority[auth_end] = '\0';
    const char *pathpart = rest + auth_end;
    char *path = NULL;
    if (pathpart[0] == '\0' || pathpart[0] == '#') {
        path = strdup("/");
    } else {
        path = strdup(pathpart);
    }
    if (authority[0] == '\0' || strchr(authority, '@')) {
        free(authority);
        free(path);
        nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
    }
    char *host = NULL;
    int port = 0;
    if (authority[0] == '[') {
        char *close = strchr(authority, ']');
        if (!close || close == authority + 1) {
            free(authority);
            free(path);
            nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
        }
        host = (char *)malloc((size_t)(close - authority) + 1);
        size_t hlen = (size_t)(close - (authority + 1));
        memcpy(host, authority + 1, hlen);
        host[hlen] = '\0';
        const char *after = close + 1;
        if (after[0] == ':') {
            char *end = NULL;
            long p = strtol(after + 1, &end, 10);
            if (!end || *end || p <= 0 || p > 65535) {
                free(authority);
                free(path);
                free(host);
                nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
            }
            port = (int)p;
        } else if (after[0] != '\0') {
            free(authority);
            free(path);
            free(host);
            nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
        } else {
            port = strcmp(scheme, "http") == 0 ? 80 : 443;
        }
    } else {
        char *colon = strchr(authority, ':');
        if (colon) {
            if (colon == authority) {
                free(authority);
                free(path);
                nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
            }
            host = (char *)malloc((size_t)(colon - authority) + 1);
            size_t hlen = (size_t)(colon - authority);
            memcpy(host, authority, hlen);
            host[hlen] = '\0';
            char *end = NULL;
            long p = strtol(colon + 1, &end, 10);
            if (!end || *end || p <= 0 || p > 65535) {
                free(authority);
                free(path);
                free(host);
                nv_net_err_runtime(line, col, "رابط غير صالح: %s", url);
            }
            port = (int)p;
        } else {
            host = strdup(authority);
            port = strcmp(scheme, "http") == 0 ? 80 : 443;
        }
    }
    free(authority);
    free(url);
    strcpy(out->scheme, scheme);
    out->host = host;
    out->port = port;
    out->path = path;
}

// ---------- صنف عنوان IP (حلقي/خاص/...) ----------
// تعيد وصف الصنف أو NULL للعام.
static const char *nv_ip_class(const struct sockaddr *sa) {
    if (sa->sa_family == AF_INET) {
        const struct sockaddr_in *in = (const struct sockaddr_in *)sa;
        uint32_t a = ntohl(in->sin_addr.s_addr);
        if ((a >> 24) == 127) return "حلقي (محلي)";
        if (a == 0) return "غير محدد";
        if ((a >> 28) == 0xE) return "متعدد البث";
        if ((a >> 24) == 10) return "خاص";
        if ((a >> 20) == 0xAC1) return "خاص"; // 172.16/12
        if ((a >> 16) == 0xC0A8) return "خاص"; // 192.168/16
        if ((a >> 16) == 0xA9FE) return "رابط محلي"; // 169.254/16
        return NULL;
    }
    if (sa->sa_family == AF_INET6) {
        const struct sockaddr_in6 *in6 = (const struct sockaddr_in6 *)sa;
        const unsigned char *b = in6->sin6_addr.s6_addr;
        if (IN6_IS_ADDR_LOOPBACK(&in6->sin6_addr)) return "حلقي (محلي)";
        if (IN6_IS_ADDR_UNSPECIFIED(&in6->sin6_addr)) return "غير محدد";
        if (IN6_IS_ADDR_MULTICAST(&in6->sin6_addr)) return "متعدد البث";
        if ((b[0] & 0xFE) == 0xFC) return "خاص"; // fc00::/7
        if ((b[0] == 0xFE) && ((b[1] & 0xC0) == 0x80)) return "رابط محلي"; // fe80::/10
        return NULL;
    }
    return NULL;
}

static int nv_local_host_allowed(const char *host) {
    for (int i = 0; i < nv_n_local_hosts; i++) {
        if (strcasecmp(host, nv_local_hosts[i]) == 0) return 1;
    }
    return 0;
}

// ---------- ترميز هدف الطلب ----------
static int nv_hex_val(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static char *nv_encode_target(const char *path) {
    static const char *HEX = "0123456789ABCDEF";
    size_t len = strlen(path);
    char *out = (char *)malloc(len * 3 + 1);
    size_t o = 0;
    for (size_t i = 0; i < len; i++) {
        unsigned char b = (unsigned char)path[i];
        if (b == '%') {
            if (i + 2 < len && nv_hex_val(path[i + 1]) >= 0 && nv_hex_val(path[i + 2]) >= 0) {
                out[o++] = '%';
                out[o++] = path[i + 1];
                out[o++] = path[i + 2];
                i += 2;
                continue;
            }
            out[o++] = '%';
            out[o++] = '2';
            out[o++] = '5';
            continue;
        }
        if ((b >= 'A' && b <= 'Z') || (b >= 'a' && b <= 'z') || (b >= '0' && b <= '9')
            || b == '-' || b == '_' || b == '.' || b == '~'
            || b == '!' || b == '$' || b == '&' || b == '\'' || b == '(' || b == ')'
            || b == '*' || b == '+' || b == ',' || b == ';' || b == '='
            || b == ':' || b == '@' || b == '/' || b == '?') {
            out[o++] = (char)b;
        } else {
            out[o++] = '%';
            out[o++] = HEX[(b >> 4) & 0xF];
            out[o++] = HEX[b & 0xF];
        }
    }
    out[o] = '\0';
    return out;
}

// ---------- مصرف الجسم ----------
typedef struct {
    char *buf;
    size_t len;
    size_t cap;
    FILE *file; // أو ملف (للتنزيل)
    size_t limit; // سقف الملف (الحصة)
    size_t written;
    // لينكس8-طبقة1-مرآة: بيانات الرد الأخيرة (رمز + رؤوس).
    int meta_status;
    char **meta_names;
    char **meta_values;
    int meta_n;
    int meta_cap;
    // قياس التنزيل (مهلة نقل/حد سرعة/حد حجم).
    long long stall_secs;
    long long max_speed;
    long long max_size;
    long long started_ms;
    long long last_push_ms;
    size_t meter_total;
} NvSink;

static void nv_net_sleep_ms(long long ms) {
    if (ms <= 0) return;
    struct timespec ts;
    ts.tv_sec = (time_t)(ms / 1000);
    ts.tv_nsec = (long)((ms % 1000) * 1000000L);
    nanosleep(&ts, NULL);
}

static void nv_sink_free_meta(NvSink *s) {
    for (int i = 0; i < s->meta_n; i++) {
        free(s->meta_names[i]);
        free(s->meta_values[i]);
    }
    free(s->meta_names);
    free(s->meta_values);
    s->meta_names = NULL;
    s->meta_values = NULL;
    s->meta_n = 0;
    s->meta_cap = 0;
}

static void nv_sink_set_meta(NvSink *s, int status, char **names, char **values, int n) {
    nv_sink_free_meta(s);
    s->meta_status = status;
    for (int i = 0; i < n; i++) {
        if (s->meta_n >= s->meta_cap) {
            s->meta_cap = s->meta_cap ? s->meta_cap * 2 : 16;
            s->meta_names = (char **)realloc(s->meta_names, sizeof(char *) * (size_t)s->meta_cap);
            s->meta_values = (char **)realloc(s->meta_values, sizeof(char *) * (size_t)s->meta_cap);
        }
        s->meta_names[s->meta_n] = names[i];
        s->meta_values[s->meta_n] = values[i];
        s->meta_n++;
    }
}

static void nv_sink_meter_reset(NvSink *s) {
    s->meter_total = 0;
    s->started_ms = nv_net_now_ms();
    s->last_push_ms = 0;
}

static void nv_sink_push(NvSink *s, const unsigned char *chunk, size_t n, int line, int col) {
    // لينكس8-طبقة1-مرآة: قياس (مهلة نقل/حد سرعة/حد حجم).
    if (s->stall_secs > 0 && s->last_push_ms != 0) {
        if (nv_net_now_ms() - s->last_push_ms >= s->stall_secs * 1000) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة النقل — توقف تدفق البيانات", line, col);
        }
    }
    if (s->max_size > 0 && (long long)(s->meter_total + n) > s->max_size) {
        nv_net_err_runtime(line, col, "تجاوز حد الحجم (%lld)", s->max_size);
    }
    if (s->max_speed > 0) {
        long long expected = (long long)((s->meter_total + n) * 1000 / (size_t)s->max_speed);
        long long elapsed = nv_net_now_ms() - s->started_ms;
        if (expected > elapsed) {
            nv_net_sleep_ms(expected - elapsed);
        }
    }
    if (s->file) {
        if (s->written + n > s->limit) {
            nv_error(NV_ERR_RUNTIME, "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة", line, col);
        }
        if (fwrite(chunk, 1, n, s->file) != n) {
            nv_error(NV_ERR_RUNTIME, "تعذر كتابة الملف", line, col);
        }
        s->written += n;
        s->meter_total += n;
        s->last_push_ms = nv_net_now_ms();
        return;
    }
    if (s->len + n > (size_t)NV_NET_MAX_RESPONSE + 1) {
        nv_error(NV_ERR_RUNTIME, "الرد أكبر من الحد الأقصى (5 ميغابايت)", line, col);
    }
    if (s->len + n > s->cap) {
        size_t ncap = s->cap ? s->cap * 2 : 8192;
        while (ncap < s->len + n) ncap *= 2;
        s->buf = (char *)realloc(s->buf, ncap);
        s->cap = ncap;
    }
    memcpy(s->buf + s->len, chunk, n);
    s->len += n;
    s->meter_total += n;
    s->last_push_ms = nv_net_now_ms();
}

// ---------- قراءة بمهلة ----------
static long long nv_net_deadline_ms(void) {
    return nv_net_now_ms() + (long long)NV_NET_TIMEOUT_SECS * 1000;
}

// اضبط مهلة المقبس على ما تبقى من المهلة الإجمالية.
static void nv_sock_arm_timeout(int fd, long long deadline_ms) {
    long long left = deadline_ms - nv_net_now_ms();
    if (left < 1) left = 1;
    struct timeval tv;
    tv.tv_sec = (time_t)(left / 1000);
    tv.tv_usec = (suseconds_t)((left % 1000) * 1000);
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
}

// اتصال بمهلة (غير حاجب + poll).
static int nv_connect_timeout(const struct sockaddr *sa, socklen_t salen, long long deadline_ms) {
    int fd = socket(sa->sa_family, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int flags = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    int rc = connect(fd, sa, salen);
    if (rc == 0) {
        fcntl(fd, F_SETFL, flags);
        return fd;
    }
    if (errno != EINPROGRESS) {
        close(fd);
        return -1;
    }
    long long left = deadline_ms - nv_net_now_ms();
    if (left <= 0) {
        close(fd);
        errno = ETIMEDOUT;
        return -1;
    }
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLOUT;
    int pr = poll(&pfd, 1, (int)left);
    if (pr <= 0) {
        close(fd);
        errno = pr == 0 ? ETIMEDOUT : EINTR;
        return -1;
    }
    int err = 0;
    socklen_t elen = sizeof(err);
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &elen) != 0 || err != 0) {
        close(fd);
        errno = err ? err : ECONNREFUSED;
        return -1;
    }
    fcntl(fd, F_SETFL, flags);
    return fd;
}

// كتابة كل البايتات (بمهلة).
static int nv_write_all(int fd, const unsigned char *buf, size_t len, long long deadline_ms) {
    size_t sent = 0;
    while (sent < len) {
        nv_sock_arm_timeout(fd, deadline_ms);
        ssize_t n = send(fd, buf + sent, len - sent, 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (n == 0) return -1;
        sent += (size_t)n;
    }
    return 0;
}

// قراءة سطر (حتى \n) بسقف.
static int nv_read_line(int fd, char *buf, size_t cap, long long deadline_ms) {
    size_t n = 0;
    while (n + 1 < cap) {
        nv_sock_arm_timeout(fd, deadline_ms);
        char c = 0;
        ssize_t r = recv(fd, &c, 1, 0);
        if (r < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (r == 0) break;
        buf[n++] = c;
        if (c == '\n') break;
    }
    buf[n] = '\0';
    return (int)n;
}

// ---------- قراءة الرد ----------
// تعيد: 0 = جسم كامل، 1 = تحويل (status/location)،
// وتملأ location عند التحويل.
static int nv_read_response(int fd, long long deadline_ms, int line, int col,
                            NvSink *sink, int *status_out, char **location_out,
                            int return_all) {
    // الترويسات بايت بايت حتى \r\n\r\n (سقف 64KB).
    char *head = (char *)malloc(NV_NET_HEADER_CAP + 1);
    size_t hlen = 0;
    int found = 0;
    while (hlen < NV_NET_HEADER_CAP) {
        nv_sock_arm_timeout(fd, deadline_ms);
        char c = 0;
        ssize_t r = recv(fd, &c, 1, 0);
        if (r < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    free(head);
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                }
                continue;
            }
            free(head);
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
        }
        if (r == 0) break;
        head[hlen++] = c;
        if (hlen >= 4 && memcmp(head + hlen - 4, "\r\n\r\n", 4) == 0) {
            found = 1;
            break;
        }
    }
    if (!found) {
        free(head);
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم (لا ترويسات)", line, col);
    }
    head[hlen] = '\0';
    // سطر الحالة.
    char *save = NULL;
    char *status_line = strtok_r(head, "\r\n", &save);
    int status = 0;
    if (status_line) {
        char *sp1 = strchr(status_line, ' ');
        if (sp1) status = atoi(sp1 + 1);
    }
    if (status == 0) {
        free(head);
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم (لا رمز حالة)", line, col);
    }
    long content_length = -1;
    int chunked = 0;
    char *location = NULL;
    // لينكس8-طبقة1-مرآة: كل الرؤوس (اسم صغير + قيمة مقصوصة).
    char **all_names = NULL;
    char **all_values = NULL;
    int all_n = 0;
    int all_cap = 0;
    char *hline = NULL;
    while ((hline = strtok_r(NULL, "\r\n", &save)) != NULL) {
        char *colon = strchr(hline, ':');
        if (!colon) continue;
        *colon = '\0';
        char *name = hline;
        char *value = colon + 1;
        while (*value == ' ' || *value == '\t') value++;
        size_t vlen = strlen(value);
        while (vlen > 0 && (value[vlen - 1] == ' ' || value[vlen - 1] == '\t')) value[--vlen] = '\0';
        if (all_n >= all_cap) {
            all_cap = all_cap ? all_cap * 2 : 16;
            all_names = (char **)realloc(all_names, sizeof(char *) * (size_t)all_cap);
            all_values = (char **)realloc(all_values, sizeof(char *) * (size_t)all_cap);
        }
        char *lname = strdup(name);
        for (char *p = lname; *p; p++) {
            if (*p >= 'A' && *p <= 'Z') *p = (char)(*p + 32);
        }
        all_names[all_n] = lname;
        all_values[all_n] = strdup(value);
        all_n++;
        if (strcasecmp(name, "content-length") == 0) {
            content_length = atol(value);
        } else if (strcasecmp(name, "transfer-encoding") == 0) {
            if (strcasestr(value, "chunked")) chunked = 1;
        } else if (strcasecmp(name, "location") == 0) {
            free(location);
            location = strdup(value);
        }
    }
    free(head);
    *status_out = status;
    *location_out = location;
    // لينكس8-طبقة1-مرآة: بيانات الرد قبل قراءة الجسم.
    nv_sink_set_meta(sink, status, all_names, all_values, all_n);
    free(all_names);
    free(all_values);
    // الجسم.
    unsigned char buf[8192];
    if (chunked) {
        for (;;) {
            char size_line[1024];
            int sl = nv_read_line(fd, size_line, sizeof(size_line), deadline_ms);
            if (sl < 0) {
                nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
            }
            char *semi = strchr(size_line, ';');
            if (semi) *semi = '\0';
            char *end = NULL;
            long size = strtol(size_line, &end, 16);
            if (end == size_line || size < 0) {
                nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم (ترميز مجزأ)", line, col);
            }
            if (size == 0) {
                // المقطورات حتى سطر فارغ.
                for (;;) {
                    char tl[1024];
                    int t = nv_read_line(fd, tl, sizeof(tl), deadline_ms);
                    if (t < 0) nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
                    if (tl[0] == '\r' || tl[0] == '\n' || tl[0] == '\0') break;
                }
                break;
            }
            long remaining = size;
            while (remaining > 0) {
                size_t want = (size_t)remaining < sizeof(buf) ? (size_t)remaining : sizeof(buf);
                nv_sock_arm_timeout(fd, deadline_ms);
                ssize_t r = recv(fd, buf, want, 0);
                if (r < 0) {
                    if (errno == EINTR) continue;
                    if (errno == EAGAIN || errno == EWOULDBLOCK) {
                        if (nv_net_now_ms() >= deadline_ms) {
                            nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                        }
                        continue;
                    }
                    nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
                }
                if (r == 0) {
                    nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم (انقطع أثناء الجسم)", line, col);
                }
                nv_sink_push(sink, buf, (size_t)r, line, col);
                remaining -= r;
            }
            char crlf[16];
            int cr = nv_read_line(fd, crlf, sizeof(crlf), deadline_ms);
            if (cr < 0) nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
        }
    } else if (content_length >= 0) {
        long remaining = content_length;
        while (remaining > 0) {
            size_t want = (size_t)remaining < sizeof(buf) ? (size_t)remaining : sizeof(buf);
            nv_sock_arm_timeout(fd, deadline_ms);
            ssize_t r = recv(fd, buf, want, 0);
            if (r < 0) {
                if (errno == EINTR) continue;
                if (errno == EAGAIN || errno == EWOULDBLOCK) {
                    if (nv_net_now_ms() >= deadline_ms) {
                        nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                    }
                    continue;
                }
                nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
            }
            if (r == 0) break;
            nv_sink_push(sink, buf, (size_t)r, line, col);
            remaining -= r;
        }
    } else {
        for (;;) {
            nv_sock_arm_timeout(fd, deadline_ms);
            ssize_t r = recv(fd, buf, sizeof(buf), 0);
            if (r < 0) {
                if (errno == EINTR) continue;
                if (errno == EAGAIN || errno == EWOULDBLOCK) {
                    if (nv_net_now_ms() >= deadline_ms) {
                        nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                    }
                    continue;
                }
                nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
            }
            if (r == 0) break;
            nv_sink_push(sink, buf, (size_t)r, line, col);
        }
    }
    // لينكس8-طبقة1-مرآة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if (return_all && !(status >= 300 && status < 400 && location)) {
        return 0;
    }
    if (status >= 300 && status < 400) {
        if (location) return 1;
        nv_net_err_runtime(line, col, "الخادم رد برمز الحالة %d بلا عنوان إعادة توجيه", status);
    }
    if (status < 200 || status >= 300) {
        nv_net_err_runtime(line, col, "الخادم رد برمز الحالة %d", status);
    }
    return 0;
}

// ---------- خيارات التنفيذ (مرآة ExecOpts) ----------
typedef struct {
    long long timeout_secs;
    long long stall_secs;
    int retries;
    long long retry_delay_ms;
    long long max_retry_ms;
    int retry_on_refused;
    int retry_forever;
    int follow_redirects;
    int max_redirects;
    int redirect_http;
    int redirect_https;
    int keep_body_on_redirect;
    int trusted_only_redirect;
    int gzip;
    int check_status;
    int return_all;
    int allow_http;
    int http10;
    int chunked;
    long long max_size;
    long long max_speed_down;
    long long max_speed_up;
    int delete_on_fail;
    int no_overwrite;
    int skip_existing;
    int preserve_mtime;
    int resume;
    int has_range;
    long long range_from;
    long long range_to;
    int has_range_to;
    const char *etag;
    const char *if_modified_since;
    const char *user_agent;
    const char *referer;
    const char *language;
    const char *trace_file;
    int trace_text;
    int silent;
    int verbose;
    int errors_only;
    int show_progress;
    const char *proxy;
    const char *proxy_user;
    const char *proxy_pass;
    char **no_proxy;
    int n_no_proxy;
    const char *socks5;
    int tunnel_proxy;
    const char *ca_file;
    const char *ca_dir;
    const char *client_cert;
    const char *client_key;
    int no_userinfo;
    int auth_kind; // 0 بلا، 1 أساسية، 2 هضمية، 3 تفاوضية، 4 حامل، 5 أمازون، 6 توقيع
    const char *auth_user;
    const char *auth_pass;
    const char *auth_token;
    const char *auth_key;
    const char *auth_secret;
    const char *auth_region;
    const char *auth_service;
    const char *auth_key_id;
    const char *auth_key2;
    const char *auth_algo;
    char **removed;
    int n_removed;
    const char *save_to;
    const char *encoding; // ترميز_الرد (utf-8 أو latin1)
    int ignore_session_cookies;
} NvOpts;

static void nv_opts_default(NvOpts *o) {
    memset(o, 0, sizeof(*o));
    o->timeout_secs = 30;
    o->retries = 1;
    o->follow_redirects = 1;
    o->max_redirects = 5;
    o->redirect_http = 1;
    o->redirect_https = 1;
    o->gzip = 1;
    o->delete_on_fail = 1;
    o->no_userinfo = 1;
    o->trace_text = 1;
}

// ---------- محاولة واحدة ----------
typedef struct {
    int is_post;
    const unsigned char *body;
    size_t body_len;
    const char *content_type;
    // ترويسات المتصل: أزواج اسم/قيمة.
    const char **hdr_names;
    const char **hdr_values;
    int nhdr;
    // لينكس8-طبقة1-مرآة: طريقة مخصصة + خيارات.
    const char *method; // NULL = GET/POST عبر is_post
    const NvOpts *opts; // NULL = افتراضي
} NvRequest;

static const char *nv_req_method(const NvRequest *req) {
    if (req->method) return req->method;
    return req->is_post ? "POST" : "GET";
}

static int nv_req_sends_body(const NvRequest *req) {
    if (req->method) {
        if (req->body_len > 0) return 1;
        return strcmp(req->method, "POST") == 0 || strcmp(req->method, "PUT") == 0
            || strcmp(req->method, "PATCH") == 0 || strcmp(req->method, "DELETE") == 0;
    }
    return req->is_post;
}

static int nv_req_converts_to_get(const NvRequest *req) {
    if (req->method) {
        return strcmp(req->method, "POST") == 0 || strcmp(req->method, "PUT") == 0
            || strcmp(req->method, "PATCH") == 0 || strcmp(req->method, "DELETE") == 0;
    }
    return req->is_post;
}

static int nv_req_has_auth(const NvRequest *req) {
    for (int i = 0; i < req->nhdr; i++) {
        if (strcasecmp(req->hdr_names[i], "authorization") == 0
            || strcasecmp(req->hdr_names[i], "cookie") == 0) {
            return 1;
        }
    }
    return 0;
}

static int nv_req_removed(const NvRequest *req, const char *name) {
    if (!req->opts) return 0;
    for (int i = 0; i < req->opts->n_removed; i++) {
        if (strcasecmp(req->opts->removed[i], name) == 0) return 1;
    }
    return 0;
}



/// ترميز base64 لـ "مستخدم:كلمة_سر" (للمصادقة الأساسية).
static char *nv_b64_encode(const char *user, const char *pass) {
    static const char tbl[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t ulen = strlen(user), plen = pass ? strlen(pass) : 0;
    size_t inlen = ulen + 1 + plen;
    char *in = (char *)malloc(inlen + 1);
    memcpy(in, user, ulen);
    in[ulen] = ':';
    if (plen) memcpy(in + ulen + 1, pass, plen);
    in[inlen] = '\0';
    size_t outlen = ((inlen + 2) / 3) * 4;
    char *out = (char *)malloc(outlen + 1);
    size_t o = 0;
    for (size_t i = 0; i < inlen; i += 3) {
        int rem = (int)(inlen - i);
        unsigned int v = ((unsigned char)in[i]) << 16;
        if (rem > 1) v |= ((unsigned char)in[i + 1]) << 8;
        if (rem > 2) v |= (unsigned char)in[i + 2];
        out[o++] = tbl[(v >> 18) & 63];
        out[o++] = tbl[(v >> 12) & 63];
        out[o++] = rem > 1 ? tbl[(v >> 6) & 63] : '=';
        out[o++] = rem > 2 ? tbl[v & 63] : '=';
    }
    out[o] = '\0';
    free(in);
    return out;
}

static int nv_header_value_ok(const char *v);

// ----- لينكس8-طبقة1-مرآة: الوكيل -----

static int nv_tcp_connect(const struct sockaddr *sa, socklen_t salen, const char *host,
                          long long deadline_ms, int line, int col) {
    int fd = nv_connect_timeout(sa, salen, deadline_ms);
    if (fd < 0) {
        int e = errno;
        if (e == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاتصال (30 ثانية)", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر الاتصال بالمضيف %s", host);
    }
    return fd;
}

/// حل اسم + فحص IP (§7) — رسائل المباشر نفسها.
static struct addrinfo *nv_net_resolve_checked(const char *host, int port, int line, int col) {
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", port);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = NULL;
    int gai = getaddrinfo(host, port_str, &hints, &res);
    if (gai != 0 || !res) {
        if (res) freeaddrinfo(res);
        nv_net_err_runtime(line, col, "تعذر حل اسم المضيف %s", host);
    }
    struct addrinfo *ai = res;
    const char *cls = nv_ip_class(ai->ai_addr);
    if (cls && !nv_local_host_allowed(host)) {
        char ipbuf[64];
        if (ai->ai_family == AF_INET) {
            inet_ntop(AF_INET, &((struct sockaddr_in *)ai->ai_addr)->sin_addr, ipbuf, sizeof(ipbuf));
        } else {
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)ai->ai_addr)->sin6_addr, ipbuf, sizeof(ipbuf));
        }
        freeaddrinfo(res);
        nv_net_err_runtime(line, col, "الوصول إلى العنوان %s مرفوض (عنوان %s)", ipbuf, cls);
    }
    return res;
}

/// "مضيف:منفذ" أو "مخطط://مضيف:منفذ" (المنفذ افتراضي).
static int nv_parse_proxy_hostport(const char *spec, int default_port, char *host_out, size_t host_cap, int *port_out) {
    const char *s = spec;
    while (*s == ' ' || *s == '\t') s++;
    const char *scheme = strstr(s, "://");
    if (scheme) s = scheme + 3;
    const char *slash = strchr(s, '/');
    size_t len = slash ? (size_t)(slash - s) : strlen(s);
    while (len > 0 && (s[len - 1] == ' ' || s[len - 1] == '\t')) len--;
    if (len == 0 || len >= host_cap) return 0;
    char tmp[1024];
    if (len >= sizeof(tmp)) return 0;
    memcpy(tmp, s, len);
    tmp[len] = '\0';
    if (tmp[0] == '[') {
        char *close = strchr(tmp, ']');
        if (!close) return 0;
        *close = '\0';
        if (tmp[1] == '\0') return 0;
        snprintf(host_out, host_cap, "%s", tmp + 1);
        const char *after = close + 1;
        if (*after == ':') {
            int p = atoi(after + 1);
            if (p <= 0 || p > 65535) return 0;
            *port_out = p;
        } else if (*after == '\0') {
            *port_out = default_port;
        } else {
            return 0;
        }
        return 1;
    }
    char *colon = strrchr(tmp, ':');
    if (colon && colon != tmp) {
        int p = atoi(colon + 1);
        if (p > 0 && p <= 65535) {
            *colon = '\0';
            snprintf(host_out, host_cap, "%s", tmp);
            *port_out = p;
            return 1;
        }
        return 0;
    }
    snprintf(host_out, host_cap, "%s", tmp);
    *port_out = default_port;
    return 1;
}

static int nv_read_until_headers(int fd, long long deadline_ms, int line, int col,
                                 char *out, size_t cap) {
    size_t len = 0;
    while (len < cap - 1) {
        nv_sock_arm_timeout(fd, deadline_ms);
        char c = 0;
        ssize_t r = recv(fd, &c, 1, 0);
        if (r < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                }
                continue;
            }
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
        }
        if (r == 0) break;
        out[len++] = c;
        if (len >= 4 && memcmp(out + len - 4, "\r\n\r\n", 4) == 0) break;
    }
    out[len] = '\0';
    return (int)len;
}

static void nv_read_exact_timeout(int fd, unsigned char *buf, size_t n, long long deadline_ms,
                                  int line, int col) {
    size_t got = 0;
    while (got < n) {
        nv_sock_arm_timeout(fd, deadline_ms);
        ssize_t r = recv(fd, buf + got, n - got, 0);
        if (r < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة قراءة الرد (30 ثانية)", line, col);
                }
                continue;
            }
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الوكيل", line, col);
        }
        if (r == 0) {
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الوكيل", line, col);
        }
        got += (size_t)r;
    }
}

static int nv_http_proxy_connect(const NvUrl *url, const char *spec, const NvRequest *req,
                                 long long deadline_ms, int line, int col, int *absolute_out) {
    char phost[1024];
    int pport = 0;
    if (!nv_parse_proxy_hostport(spec, 8080, phost, sizeof(phost), &pport)) {
        nv_error(NV_ERR_RUNTIME, "وكيل غير صالح", line, col);
    }
    struct addrinfo *res = nv_net_resolve_checked(phost, pport, line, col);
    int fd = nv_tcp_connect(res->ai_addr, res->ai_addrlen, phost, deadline_ms, line, col);
    freeaddrinfo(res);
    if (strcmp(url->scheme, "https") == 0) {
        if (!req->opts || !req->opts->tunnel_proxy) {
            close(fd);
            nv_error(NV_ERR_RUNTIME, "الوكيل يحتاج نفقا لـ HTTPS — فعّل نفق_الوكيل", line, col);
        }
        char text[2048];
        int tn;
        if (req->opts && req->opts->proxy_user) {
            char *cred = nv_b64_encode(req->opts->proxy_user, req->opts->proxy_pass);
            tn = snprintf(text, sizeof(text),
                          "CONNECT %s:%d HTTP/1.1\r\nHost: %s:%d\r\nProxy-Authorization: Basic %s\r\n\r\n",
                          url->host, url->port, url->host, url->port, cred);
            free(cred);
        } else {
            tn = snprintf(text, sizeof(text), "CONNECT %s:%d HTTP/1.1\r\nHost: %s:%d\r\n\r\n",
                          url->host, url->port, url->host, url->port);
        }
        if (nv_write_all(fd, (const unsigned char *)text, (size_t)tn, deadline_ms) != 0) {
            int e = errno;
            close(fd);
            if (e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT) {
                nv_error(NV_ERR_RUNTIME, "انتهت مهلة إرسال الطلب", line, col);
            }
            nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", phost);
        }
        char head[65536];
        nv_read_until_headers(fd, deadline_ms, line, col, head, sizeof(head));
        int status = 0;
        char *sp1 = strchr(head, ' ');
        if (sp1) status = atoi(sp1 + 1);
        if (status != 200) {
            close(fd);
            nv_net_err_runtime(line, col, "الوكيل رفض النفق (رمز %d)", status);
        }
        *absolute_out = 0;
        return fd;
    }
    *absolute_out = 1;
    return fd;
}

static int nv_socks5_connect(const NvUrl *url, const char *spec, const NvRequest *req,
                             long long deadline_ms, int line, int col) {
    char phost[1024];
    int pport = 0;
    if (!nv_parse_proxy_hostport(spec, 1080, phost, sizeof(phost), &pport)) {
        nv_error(NV_ERR_RUNTIME, "وكيل غير صالح", line, col);
    }
    struct addrinfo *res = nv_net_resolve_checked(phost, pport, line, col);
    int fd = nv_tcp_connect(res->ai_addr, res->ai_addrlen, phost, deadline_ms, line, col);
    freeaddrinfo(res);
    unsigned char greeting[4];
    size_t glen;
    if (req->opts && req->opts->proxy_user) {
        greeting[0] = 5; greeting[1] = 2; greeting[2] = 0; greeting[3] = 2;
        glen = 4;
    } else {
        greeting[0] = 5; greeting[1] = 1; greeting[2] = 0;
        glen = 3;
    }
    if (nv_write_all(fd, greeting, glen, deadline_ms) != 0) {
        close(fd);
        nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", phost);
    }
    unsigned char resp[2];
    nv_read_exact_timeout(fd, resp, 2, deadline_ms, line, col);
    if (resp[0] != 5) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الوكيل", line, col);
    }
    if (resp[1] == 2) {
        const char *u = req->opts->proxy_user;
        const char *p = req->opts->proxy_pass ? req->opts->proxy_pass : "";
        size_t ulen = strlen(u), plen = strlen(p);
        unsigned char *auth = (unsigned char *)malloc(3 + ulen + plen);
        auth[0] = 1;
        auth[1] = (unsigned char)ulen;
        memcpy(auth + 2, u, ulen);
        auth[2 + ulen] = (unsigned char)plen;
        memcpy(auth + 3 + ulen, p, plen);
        int rc = nv_write_all(fd, auth, 3 + ulen + plen, deadline_ms);
        free(auth);
        if (rc != 0) {
            close(fd);
            nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", phost);
        }
        unsigned char aresp[2];
        nv_read_exact_timeout(fd, aresp, 2, deadline_ms, line, col);
        if (aresp[1] != 0) {
            close(fd);
            nv_error(NV_ERR_RUNTIME, "الوكيل رفض المصادقة", line, col);
        }
    } else if (resp[1] != 0) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "الوكيل رفض الاتصال", line, col);
    }
    size_t hlen = strlen(url->host);
    if (hlen > 255) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "اسم مضيف طويل للوكيل", line, col);
    }
    unsigned char *creq = (unsigned char *)malloc(hlen + 10);
    size_t clen = 0;
    creq[clen++] = 5; creq[clen++] = 1; creq[clen++] = 0;
    struct in_addr v4;
    struct in6_addr v6;
    if (inet_pton(AF_INET, url->host, &v4) == 1) {
        creq[clen++] = 1;
        memcpy(creq + clen, &v4, 4);
        clen += 4;
    } else if (inet_pton(AF_INET6, url->host, &v6) == 1) {
        creq[clen++] = 4;
        memcpy(creq + clen, &v6, 16);
        clen += 16;
    } else {
        creq[clen++] = 3;
        creq[clen++] = (unsigned char)hlen;
        memcpy(creq + clen, url->host, hlen);
        clen += hlen;
    }
    creq[clen++] = (unsigned char)((url->port >> 8) & 0xFF);
    creq[clen++] = (unsigned char)(url->port & 0xFF);
    int rc = nv_write_all(fd, creq, clen, deadline_ms);
    free(creq);
    if (rc != 0) {
        close(fd);
        nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", phost);
    }
    unsigned char head4[4];
    nv_read_exact_timeout(fd, head4, 4, deadline_ms, line, col);
    if (head4[0] != 5) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الوكيل", line, col);
    }
    if (head4[1] != 0) {
        close(fd);
        nv_net_err_runtime(line, col, "الوكيل SOCKS5 رفض الاتصال (رمز %d)", head4[1]);
    }
    size_t rest = 0;
    if (head4[3] == 1) {
        rest = 4 + 2;
    } else if (head4[3] == 3) {
        unsigned char l = 0;
        nv_read_exact_timeout(fd, &l, 1, deadline_ms, line, col);
        rest = (size_t)l + 2;
    } else if (head4[3] == 4) {
        rest = 16 + 2;
    } else {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الوكيل", line, col);
    }
    unsigned char *tail = (unsigned char *)malloc(rest ? rest : 1);
    nv_read_exact_timeout(fd, tail, rest, deadline_ms, line, col);
    free(tail);
    return fd;
}

/// الاتصال بالهدف: مباشر، أو عبر وكيل HTTP، أو عبر SOCKS5.
/// فحص IP (§7) يسري على الوكيل نفسه عند التوكيل.
static int nv_connect_target(const NvUrl *url, const NvRequest *req, long long deadline_ms,
                             int line, int col, int *absolute_out) {
    *absolute_out = 0;
    if (req->opts) {
        const char *host_l = url->host;
        int bypass = 0;
        for (int i = 0; i < req->opts->n_no_proxy; i++) {
            const char *p = req->opts->no_proxy[i];
            if (strcasecmp(host_l, p) == 0) { bypass = 1; break; }
            size_t hl = strlen(host_l), pl = strlen(p);
            if (pl > 0 && p[0] == '.' && hl > pl && strcasecmp(host_l + hl - pl, p) == 0) { bypass = 1; break; }
            if (hl > pl + 1 && host_l[hl - pl - 1] == '.' && strcasecmp(host_l + hl - pl, p) == 0) { bypass = 1; break; }
        }
        if (!bypass) {
            if (req->opts->socks5) {
                return nv_socks5_connect(url, req->opts->socks5, req, deadline_ms, line, col);
            }
            if (req->opts->proxy) {
                return nv_http_proxy_connect(url, req->opts->proxy, req, deadline_ms, line, col, absolute_out);
            }
        }
    }
    struct addrinfo *res = nv_net_resolve_checked(url->host, url->port, line, col);
    int fd = nv_tcp_connect(res->ai_addr, res->ai_addrlen, url->host, deadline_ms, line, col);
    freeaddrinfo(res);
    return fd;
}

/// كتابة مخنوقة بحد سرعة الرفع (0 = بلا حد).
static int nv_write_throttled(int fd, const unsigned char *data, size_t len, long long limit_bps,
                              long long deadline_ms, int line, int col) {
    (void)line;
    (void)col;
    if (limit_bps <= 0) {
        return nv_write_all(fd, data, len, deadline_ms);
    }
    long long started = nv_net_now_ms();
    size_t sent = 0;
    while (sent < len) {
        size_t n = len - sent < 8192 ? len - sent : 8192;
        if (nv_write_all(fd, data + sent, n, deadline_ms) != 0) return -1;
        sent += n;
        long long expected = (long long)(sent * 1000 / (size_t)limit_bps);
        long long elapsed = nv_net_now_ms() - started;
        if (expected > elapsed) {
            nv_net_sleep_ms(expected - elapsed);
        }
    }
    return 0;
}

static void nv_fetch_one(const NvUrl *url, const NvRequest *req, long long deadline_ms,
                         int line, int col, NvSink *sink,
                         int *status_out, char **location_out) {
    if (strcmp(url->scheme, "https") == 0) {
        nv_error(NV_ERR_RUNTIME, "HTTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق أو المتصفح للروابط المشفرة", line, col);
    }
    // لينكس8-طبقة1-مرآة: اتصال مباشر أو عبر وكيل.
    int absolute = 0;
    int fd = nv_connect_target(url, req, deadline_ms, line, col, &absolute);
    // بناء الطلب.
    char host_hdr[1024];
    if (url->port == 80) {
        snprintf(host_hdr, sizeof(host_hdr), "%s", url->host);
    } else {
        snprintf(host_hdr, sizeof(host_hdr), "%s:%d", url->host, url->port);
    }
    char *enc_target = nv_encode_target(url->path);
    char *target = enc_target;
    if (absolute) {
        size_t alen = strlen(url->scheme) + 3 + strlen(host_hdr) + strlen(enc_target) + 1;
        target = (char *)malloc(alen);
        snprintf(target, alen, "%s://%s%s", url->scheme, host_hdr, enc_target);
        free(enc_target);
    }
    // لينكس8-طبقة1-مرآة: ترويسات مُدارة (إصدار/وكيل/مرجع/
    // لغة/ضغط) مع فلترة المحذوفة، وجسم متقطع عند الطلب.
    int use_chunked = req->opts && req->opts->chunked && nv_req_sends_body(req);
    int sends = nv_req_sends_body(req);
    size_t head_cap = strlen(target) + strlen(host_hdr) + 8192;
    for (int i = 0; i < req->nhdr; i++) head_cap += strlen(req->hdr_names[i]) + strlen(req->hdr_values[i]) + 8;
    char *head = (char *)malloc(head_cap);
    int hn = snprintf(head, head_cap, "%s %s HTTP/%s\r\nHost: %s\r\n",
                      nv_req_method(req), target,
                      (req->opts && req->opts->http10) ? "1.0" : "1.1", host_hdr);
    int has_ua = 0, has_accept = 0, has_referer = 0, has_lang = 0, has_enc = 0;
    for (int i = 0; i < req->nhdr; i++) {
        const char *nm = req->hdr_names[i];
        if (strcasecmp(nm, "user-agent") == 0) has_ua = 1;
        if (strcasecmp(nm, "accept") == 0) has_accept = 1;
        if (strcasecmp(nm, "referer") == 0) has_referer = 1;
        if (strcasecmp(nm, "accept-language") == 0) has_lang = 1;
        if (strcasecmp(nm, "accept-encoding") == 0) has_enc = 1;
    }
    if (!has_ua && !nv_req_removed(req, "user-agent")) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "User-Agent: %s\r\n",
                       (req->opts && req->opts->user_agent) ? req->opts->user_agent : "Niva/1.0");
    }
    if (!has_accept && !nv_req_removed(req, "accept")) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Accept: */*\r\n");
    }
    if (!has_referer && req->opts && req->opts->referer && !nv_req_removed(req, "referer")) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Referer: %s\r\n", req->opts->referer);
    }
    if (!has_lang && req->opts && req->opts->language && !nv_req_removed(req, "accept-language")) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Accept-Language: %s\r\n", req->opts->language);
    }
    if (!has_enc && req->opts && req->opts->gzip && !nv_req_removed(req, "accept-encoding")) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Accept-Encoding: gzip\r\n");
    }
    if (req->opts && req->opts->proxy && req->opts->proxy_user && !nv_req_removed(req, "proxy-authorization")) {
        char *cred = nv_b64_encode(req->opts->proxy_user, req->opts->proxy_pass);
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Proxy-Authorization: Basic %s\r\n", cred);
        free(cred);
    }
    for (int i = 0; i < req->nhdr; i++) {
        const char *nm = req->hdr_names[i];
        if (strcasecmp(nm, "host") == 0 || strcasecmp(nm, "content-length") == 0
            || strcasecmp(nm, "content-type") == 0 || strcasecmp(nm, "connection") == 0
            || strcasecmp(nm, "transfer-encoding") == 0 || nv_req_removed(req, nm)) {
            continue;
        }
        hn += snprintf(head + hn, head_cap - (size_t)hn, "%s: %s\r\n", nm, req->hdr_values[i]);
    }
    if (sends) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Content-Type: %s\r\n",
                       req->content_type ? req->content_type : "text/plain; charset=utf-8");
        if (use_chunked) {
            hn += snprintf(head + hn, head_cap - (size_t)hn, "Transfer-Encoding: chunked\r\n");
        } else {
            hn += snprintf(head + hn, head_cap - (size_t)hn, "Content-Length: %zu\r\n", req->body_len);
        }
    }
    hn += snprintf(head + hn, head_cap - (size_t)hn, "Connection: close\r\n\r\n");
    // الجسم المتقطع يُبنى قبل الإرسال.
    const unsigned char *body_send = req->body;
    size_t body_send_len = req->body_len;
    unsigned char *chunked_buf = NULL;
    if (use_chunked) {
        size_t cap2 = req->body_len + req->body_len / 8192 * 16 + 16;
        chunked_buf = (unsigned char *)malloc(cap2 ? cap2 : 16);
        size_t off = 0, pos = 0;
        while (off < req->body_len) {
            size_t n = req->body_len - off < 8192 ? req->body_len - off : 8192;
            pos += (size_t)snprintf((char *)chunked_buf + pos, cap2 - pos, "%zx\r\n", n);
            memcpy(chunked_buf + pos, req->body + off, n);
            pos += n;
            memcpy(chunked_buf + pos, "\r\n", 2);
            pos += 2;
            off += n;
        }
        memcpy(chunked_buf + pos, "0\r\n\r\n", 5);
        pos += 5;
        body_send = chunked_buf;
        body_send_len = pos;
    }
    long long up_limit = req->opts ? req->opts->max_speed_up : 0;
    if (nv_write_throttled(fd, (const unsigned char *)head, (size_t)hn, up_limit, deadline_ms, line, col) != 0) {
        int e = errno;
        free(head);
        free(target);
        free(chunked_buf);
        close(fd);
        if (e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة إرسال الطلب", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", url->host);
    }
    free(head);
    if (sends && body_send_len > 0) {
        if (nv_write_throttled(fd, body_send, body_send_len, up_limit, deadline_ms, line, col) != 0) {
            int e = errno;
            free(target);
            free(chunked_buf);
            close(fd);
            if (e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT) {
                nv_error(NV_ERR_RUNTIME, "انتهت مهلة إرسال الطلب", line, col);
            }
            nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", url->host);
        }
    }
    free(target);
    free(chunked_buf);
    int st = 0;
    char *loc = NULL;
    int rc = nv_read_response(fd, deadline_ms, line, col, sink, &st, &loc,
                              req->opts ? req->opts->return_all : 0);
    close(fd);
    *status_out = st;
    *location_out = loc;
    (void)rc;
}

// ---------- حل التحويل النسبي ----------
static void nv_resolve_redirect(const NvUrl *cur, const char *location, NvUrl *out, int line, int col) {
    while (*location == ' ' || *location == '\t') location++;
    size_t llen = strlen(location);
    while (llen > 0 && (location[llen - 1] == ' ' || location[llen - 1] == '\t')) llen--;
    if (llen == 0) {
        nv_error(NV_ERR_RUNTIME, "إعادة توجيه بلا عنوان", line, col);
    }
    char *loc = (char *)malloc(llen + 1);
    memcpy(loc, location, llen);
    loc[llen] = '\0';
    if (strstr(loc, "://")) {
        nv_parse_url(loc, out, line, col);
        free(loc);
        return;
    }
    char *path = NULL;
    if (loc[0] == '/') {
        path = strdup(loc);
    } else {
        const char *dir_end = strrchr(cur->path, '/');
        size_t dlen = dir_end ? (size_t)(dir_end - cur->path) + 1 : 1;
        path = (char *)malloc(dlen + llen + 1);
        memcpy(path, cur->path, dlen);
        memcpy(path + dlen, loc, llen);
        path[dlen + llen] = '\0';
    }
    free(loc);
    strcpy(out->scheme, cur->scheme);
    out->host = strdup(cur->host);
    out->port = cur->port;
    out->path = path;
}

static int nv_redirect_allowlisted(const NvUrl *next) {
    return strcmp(next->scheme, "https") == 0
        && strcasecmp(next->host, "release-assets.githubusercontent.com") == 0
        && next->port == 443;
}

static int nv_same_host_port(const NvUrl *a, const NvUrl *b) {
    return strcasecmp(a->host, b->host) == 0 && a->port == b->port;
}

// ---------- حلقة الجلب ----------
static void nv_fetch_loop(const char *url_str, NvRequest *req, int line, int col, NvSink *sink) {
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
    NvUrl current;
    nv_parse_url(url_str, &current, line, col);
    // لينكس8-طبقة1-مرآة: مهلة من الخيارات + قياس من الخيارات.
    long long timeout_secs = req->opts ? req->opts->timeout_secs : (long long)NV_NET_TIMEOUT_SECS;
    long long deadline_ms = nv_net_now_ms() + timeout_secs * 1000;
    nv_sink_meter_reset(sink);
    if (req->opts) {
        sink->stall_secs = req->opts->stall_secs;
        sink->max_speed = req->opts->max_speed_down;
        sink->max_size = req->opts->max_size;
    }
    int redirects = 0;
    for (;;) {
        int status = 0;
        char *location = NULL;
        NvSink save_sink = *sink;
        nv_fetch_one(&current, req, deadline_ms, line, col, sink, &status, &location);
        if (status < 300 || status >= 400 || !location) {
            free(location);
            nv_url_free(&current);
            return;
        }
        // لينكس8-طبقة1-مرآة: بلا اتباع يُرجع الرد 3xx نفسه.
        if (req->opts && !req->opts->follow_redirects) {
            free(location);
            nv_url_free(&current);
            return;
        }
        int max_red = req->opts ? req->opts->max_redirects : NV_NET_MAX_REDIRECTS;
        if (max_red > NV_NET_MAX_REDIRECTS) max_red = NV_NET_MAX_REDIRECTS;
        if (redirects >= max_red) {
            free(location);
            nv_url_free(&current);
            nv_net_err_runtime(line, col, "عدد إعادات التوجيه تجاوز الحد (%d)", max_red);
        }
        redirects++;
        NvUrl next;
        nv_resolve_redirect(&current, location, &next, line, col);
        free(location);
        if (strcmp(current.scheme, "https") == 0 && strcmp(next.scheme, "http") == 0) {
            nv_url_free(&next);
            nv_url_free(&current);
            nv_error(NV_ERR_RUNTIME, "إعادة التوجيه من HTTPS إلى HTTP مرفوضة", line, col);
        }
        int cross = !nv_same_host_port(&current, &next);
        // لينكس8-طبقة1-مرآة: بروتوكول التوجيه المقيد.
        if (req->opts) {
            int ok_proto = (strcmp(next.scheme, "http") == 0 && req->opts->redirect_http)
                || (strcmp(next.scheme, "https") == 0 && req->opts->redirect_https);
            if (!ok_proto) {
                nv_url_free(&next);
                nv_url_free(&current);
                nv_net_err_runtime(line, col, "إعادة التوجيه إلى بروتوكول %s مرفوضة", next.scheme);
            }
            if (req->opts->trusted_only_redirect && cross && nv_req_has_auth(req)) {
                nv_url_free(&next);
                nv_url_free(&current);
                nv_error(NV_ERR_RUNTIME, "إعادة التوجيه إلى مضيف غير موثوق مرفوضة", line, col);
            }
        }
        if (nv_req_converts_to_get(req) && (status == 307 || status == 308)) {
            if (cross) {
                nv_net_err_runtime(line, col, "إعادة التوجيه إلى مضيف مختلف (%s) مرفوضة", next.host);
            }
        } else {
            if (nv_req_converts_to_get(req) && !(req->opts && req->opts->keep_body_on_redirect)) {
                req->is_post = 0;
                req->method = NULL;
                req->body = NULL;
                req->body_len = 0;
                req->content_type = "";
            }
            if (cross && !nv_redirect_allowlisted(&next)) {
                nv_net_err_runtime(line, col, "إعادة التوجيه إلى مضيف مختلف (%s) مرفوضة", next.host);
            }
        }
        if (!nv_same_host_port(&current, &next)) {
            req->nhdr = 0; // إسقاط ترويسات المتصل لمضيف مختلف
        }
        // جسم 3xx لا يلوث الجسم النهائي: أعد المصرف.
        if (sink->file) {
            if (fseek(sink->file, 0, SEEK_SET) == 0) {
                ftruncate(fileno(sink->file), 0);
            }
            sink->written = 0;
        } else {
            sink->len = 0;
        }
        nv_sink_free_meta(sink);
        nv_sink_meter_reset(sink);
        (void)save_sink;
        nv_url_free(&current);
        current = next;
    }
}

// ---------- فحص ترويسات المتصل (اجتماع 30) ----------
static void nv_validate_caller_headers(const char **names, const char **values, int n, int line, int col) {
    for (int i = 0; i < n; i++) {
        const char *nm = names[i];
        if (!nm || !*nm) {
            nv_error(NV_ERR_RUNTIME, "اسم ترويسة فارغ — الترويسة تحتاج اسما غير فارغ", line, col);
        }
        for (const unsigned char *p = (const unsigned char *)nm; *p; p++) {
            unsigned char b = *p;
            int ok = (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9')
                || b == '!' || b == '#' || b == '$' || b == '%' || b == '&' || b == '\''
                || b == '*' || b == '+' || b == '-' || b == '.' || b == '^' || b == '_'
                || b == '`' || b == '|' || b == '~';
            if (!ok) {
                nv_error(NV_ERR_RUNTIME, "اسم ترويسة غير صالح — الاسم حروف رمزية فقط", line, col);
            }
        }
        if (strchr(values[i], '\r') || strchr(values[i], '\n')) {
            nv_error(NV_ERR_RUNTIME, "قيمة ترويسة غير صالحة — لا يُسمح بأسطر جديدة في الترويسات", line, col);
        }
    }
}

// ---------- نقاط دخول الوحدة (تُستدعى من niva_std) ----------
// اجلب: 1 وسيط نصي → نص الرد.
NvValue nv_net_fetch(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اجلب تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اجلب تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    NvRequest req;
    memset(&req, 0, sizeof(req));
    NvSink sink;
    memset(&sink, 0, sizeof(sink));
    nv_fetch_loop(args[0].u.str->data, &req, line, col, &sink);
    NvValue out = nv_str_new(sink.buf ? sink.buf : "", sink.len);
    free(sink.buf);
    return out;
}

// أرسل: رابط، جسم، [نوع المحتوى، [ترويسات]] → نص الرد.
NvValue nv_net_post(NvValue *args, int nargs, int line, int col) {
    if (nargs < 2 || nargs > 4) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أرسل تتوقع 2 إلى 4 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    const char *content_type = "application/json";
    if (nargs >= 3) {
        if (args[2].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
        }
        content_type = args[2].u.str->data;
    }
    const char *body = args[1].u.str->data;
    size_t body_len = args[1].u.str->len;
    if (body_len > (size_t)NV_NET_MAX_REQUEST) {
        nv_error(NV_ERR_RUNTIME, "جسم الطلب أكبر من الحد الأقصى (5 ميغابايت)", line, col);
    }
    // ترويسات المتصل (قاموس).
    const char **hnames = NULL;
    const char **hvalues = NULL;
    int nhdr = 0;
    if (nargs >= 4) {
        if (args[3].tag != NV_DICT) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[3]));
        }
        NvDict *d = args[3].u.dict;
        nhdr = (int)d->len;
        hnames = (const char **)malloc((size_t)(nhdr ? nhdr : 1) * sizeof(char *));
        hvalues = (const char **)malloc((size_t)(nhdr ? nhdr : 1) * sizeof(char *));
        for (size_t i = 0; i < d->len; i++) {
            if (d->entries[i].val.tag != NV_STR) {
                free(hnames);
                free(hvalues);
                nv_error(NV_ERR_TYPE, "ترويسات المتصل قيم نصية فقط", line, col);
            }
            hnames[i] = d->entries[i].key;
            hvalues[i] = d->entries[i].val.u.str->data;
        }
        nv_validate_caller_headers(hnames, hvalues, nhdr, line, col);
    }
    NvRequest req;
    memset(&req, 0, sizeof(req));
    req.is_post = 1;
    req.body = (const unsigned char *)body;
    req.body_len = body_len;
    req.content_type = content_type;
    req.hdr_names = hnames;
    req.hdr_values = hvalues;
    req.nhdr = nhdr;
    NvSink sink;
    memset(&sink, 0, sizeof(sink));
    nv_fetch_loop(args[0].u.str->data, &req, line, col, &sink);
    free(hnames);
    free(hvalues);
    NvValue out = nv_str_new(sink.buf ? sink.buf : "", sink.len);
    free(sink.buf);
    return out;
}


// ----- لينكس8-طبقة1-مرآة: تشفير ذاتي (MD5/SHA-256/HMAC/CRC32) -----

static void nv_md5(const unsigned char *data, size_t len, unsigned char out[16]) {
    static const uint32_t K[64] = {
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391
    };
    static const int S[64] = {
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21
    };
    uint32_t a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
    size_t padded = ((len + 9 + 63) / 64) * 64;
    unsigned char *msg = (unsigned char *)calloc(padded, 1);
    memcpy(msg, data, len);
    msg[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) msg[padded - 8 + i] = (unsigned char)((bits >> (8 * i)) & 0xFF);
    for (size_t off = 0; off < padded; off += 64) {
        uint32_t m[16];
        for (int i = 0; i < 16; i++) {
            m[i] = (uint32_t)msg[off + 4 * i] | ((uint32_t)msg[off + 4 * i + 1] << 8)
                | ((uint32_t)msg[off + 4 * i + 2] << 16) | ((uint32_t)msg[off + 4 * i + 3] << 24);
        }
        uint32_t a = a0, b = b0, c = c0, d = d0;
        for (int i = 0; i < 64; i++) {
            uint32_t f;
            int g;
            if (i < 16) { f = (b & c) | ((~b) & d); g = i; }
            else if (i < 32) { f = (d & b) | ((~d) & c); g = (5 * i + 1) % 16; }
            else if (i < 48) { f = b ^ c ^ d; g = (3 * i + 5) % 16; }
            else { f = c ^ (b | (~d)); g = (7 * i) % 16; }
            uint32_t tmp = d;
            d = c;
            c = b;
            uint32_t x = a + f + K[i] + m[g];
            b = b + ((x << S[i]) | (x >> (32 - S[i])));
            a = tmp;
        }
        a0 += a; b0 += b; c0 += c; d0 += d;
    }
    free(msg);
    uint32_t w[4] = {a0, b0, c0, d0};
    for (int i = 0; i < 4; i++) {
        out[4 * i] = (unsigned char)(w[i] & 0xFF);
        out[4 * i + 1] = (unsigned char)((w[i] >> 8) & 0xFF);
        out[4 * i + 2] = (unsigned char)((w[i] >> 16) & 0xFF);
        out[4 * i + 3] = (unsigned char)((w[i] >> 24) & 0xFF);
    }
}

static void nv_sha256(const unsigned char *data, size_t len, unsigned char out[32]) {
    static const uint32_t K[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
    };
    uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    size_t padded = ((len + 9 + 63) / 64) * 64;
    unsigned char *msg = (unsigned char *)calloc(padded, 1);
    memcpy(msg, data, len);
    msg[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) msg[padded - 1 - i] = (unsigned char)((bits >> (8 * i)) & 0xFF);
    for (size_t off = 0; off < padded; off += 64) {
        uint32_t w[64];
        for (int i = 0; i < 16; i++) {
            w[i] = ((uint32_t)msg[off + 4 * i] << 24) | ((uint32_t)msg[off + 4 * i + 1] << 16)
                | ((uint32_t)msg[off + 4 * i + 2] << 8) | (uint32_t)msg[off + 4 * i + 3];
        }
        for (int i = 16; i < 64; i++) {
            uint32_t s0 = ((w[i - 15] >> 7) | (w[i - 15] << 25)) ^ ((w[i - 15] >> 18) | (w[i - 15] << 14)) ^ (w[i - 15] >> 3);
            uint32_t s1 = ((w[i - 2] >> 17) | (w[i - 2] << 15)) ^ ((w[i - 2] >> 19) | (w[i - 2] << 13)) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 64; i++) {
            uint32_t s1 = ((e >> 6) | (e << 26)) ^ ((e >> 11) | (e << 21)) ^ ((e >> 25) | (e << 7));
            uint32_t ch = (e & f) ^ ((~e) & g);
            uint32_t t1 = hh + s1 + ch + K[i] + w[i];
            uint32_t s0 = ((a >> 2) | (a << 30)) ^ ((a >> 13) | (a << 19)) ^ ((a >> 22) | (a << 10));
            uint32_t mj = (a & b) ^ (a & c) ^ (b & c);
            uint32_t t2 = s0 + mj;
            hh = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
    }
    free(msg);
    for (int i = 0; i < 8; i++) {
        out[4 * i] = (unsigned char)((h[i] >> 24) & 0xFF);
        out[4 * i + 1] = (unsigned char)((h[i] >> 16) & 0xFF);
        out[4 * i + 2] = (unsigned char)((h[i] >> 8) & 0xFF);
        out[4 * i + 3] = (unsigned char)(h[i] & 0xFF);
    }
}

static void nv_hex_bytes(const unsigned char *in, size_t len, char *out) {
    static const char hexd[] = "0123456789abcdef";
    for (size_t i = 0; i < len; i++) {
        out[2 * i] = hexd[(in[i] >> 4) & 0xF];
        out[2 * i + 1] = hexd[in[i] & 0xF];
    }
    out[2 * len] = '\0';
}

static void nv_md5_hex(const unsigned char *data, size_t len, char *out) {
    unsigned char d[16];
    nv_md5(data, len, d);
    nv_hex_bytes(d, 16, out);
}

static void nv_sha256_hex(const unsigned char *data, size_t len, char *out) {
    unsigned char d[32];
    nv_sha256(data, len, d);
    nv_hex_bytes(d, 32, out);
}

static void nv_hmac_sha256(const unsigned char *key, size_t klen, const unsigned char *msg, size_t mlen, unsigned char out[32]) {
    unsigned char k[64];
    memset(k, 0, sizeof(k));
    if (klen > 64) {
        nv_sha256(key, klen, k);
    } else {
        memcpy(k, key, klen);
    }
    unsigned char ipad[64], opad[64];
    for (int i = 0; i < 64; i++) {
        ipad[i] = (unsigned char)(k[i] ^ 0x36);
        opad[i] = (unsigned char)(k[i] ^ 0x5c);
    }
    size_t inner_len = 64 + mlen;
    unsigned char *inner = (unsigned char *)malloc(inner_len);
    memcpy(inner, ipad, 64);
    memcpy(inner + 64, msg, mlen);
    unsigned char ihash[32];
    nv_sha256(inner, inner_len, ihash);
    free(inner);
    unsigned char outer[64 + 32];
    memcpy(outer, opad, 64);
    memcpy(outer + 64, ihash, 32);
    nv_sha256(outer, 64 + 32, out);
}

static uint32_t nv_crc32(const unsigned char *data, size_t len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++) {
        crc ^= data[i];
        for (int k = 0; k < 8; k++) {
            if (crc & 1) crc = (crc >> 1) ^ 0xEDB88320u;
            else crc >>= 1;
        }
    }
    return crc ^ 0xFFFFFFFFu;
}

// ----- لينكس8-طبقة1-مرآة: فك ضغط DEFLATE/gzip (RFC 1951) -----

typedef struct {
    const unsigned char *data;
    size_t len;
    size_t pos;
    int bit;
} NvInflateBits;

static uint32_t nv_bits(NvInflateBits *r, int n, int line, int col) {
    uint32_t v = 0;
    for (int i = 0; i < n; i++) {
        if (r->pos >= r->len) {
            nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
        }
        int b = (r->data[r->pos] >> r->bit) & 1;
        v |= (uint32_t)b << i;
        if (++r->bit == 8) {
            r->bit = 0;
            r->pos++;
        }
    }
    return v;
}

static void nv_bits_align(NvInflateBits *r) {
    if (r->bit) {
        r->bit = 0;
        r->pos++;
    }
}

typedef struct {
    unsigned short counts[16];
    unsigned short symbols[288];
} NvHuff;

static void nv_huff_build(const unsigned char *lengths, int n, NvHuff *h, int line, int col) {
    memset(h->counts, 0, sizeof(h->counts));
    for (int i = 0; i < n; i++) h->counts[lengths[i]]++;
    int left = 1;
    for (int len = 1; len <= 15; len++) {
        left <<= 1;
        left -= h->counts[len];
        if (left < 0) {
            nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
        }
    }
    unsigned short offs[16] = {0};
    for (int len = 1; len < 15; len++) offs[len + 1] = (unsigned short)(offs[len] + h->counts[len]);
    for (int i = 0; i < n; i++) {
        if (lengths[i]) h->symbols[offs[lengths[i]]++] = (unsigned short)i;
    }
}

static int nv_huff_decode(NvInflateBits *r, const NvHuff *h, int line, int col) {
    int code = 0, first = 0, index = 0;
    for (int len = 1; len <= 15; len++) {
        code |= (int)nv_bits(r, 1, line, col);
        int count = h->counts[len];
        if (code - first < count) return h->symbols[index + (code - first)];
        index += count;
        first = (first + count) << 1;
        code <<= 1;
    }
    nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
    return -1;
}

static unsigned char *nv_inflate(const unsigned char *data, size_t len, size_t *out_len, int line, int col) {
    static const unsigned short LEN_BASE[29] = {
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
        67, 83, 99, 115, 131, 163, 195, 227, 258
    };
    static const unsigned char LEN_EXT[29] = {
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3,
        4, 4, 4, 4, 5, 5, 5, 5, 0
    };
    static const unsigned short DIST_BASE[30] = {
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
        1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577
    };
    static const unsigned char DIST_EXT[30] = {
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8,
        9, 9, 10, 10, 11, 11, 12, 12, 13, 13
    };
    NvInflateBits br = {data, len, 0, 0};
    size_t cap = 1 << 16, outn = 0;
    unsigned char *out = (unsigned char *)malloc(cap);
    for (;;) {
        int bfinal = (int)nv_bits(&br, 1, line, col);
        int btype = (int)nv_bits(&br, 2, line, col);
        if (btype == 0) {
            nv_bits_align(&br);
            if (br.pos + 4 > br.len) {
                free(out);
                nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
            }
            unsigned slen = br.data[br.pos] | ((unsigned)br.data[br.pos + 1] << 8);
            unsigned nlen = br.data[br.pos + 2] | ((unsigned)br.data[br.pos + 3] << 8);
            br.pos += 4;
            if (slen != (nlen ^ 0xFFFF)) {
                free(out);
                nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
            }
            if (br.pos + slen > br.len) {
                free(out);
                nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
            }
            if (outn + slen > 100 * 1024 * 1024) {
                free(out);
                nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
            }
            while (outn + slen > cap) {
                cap *= 2;
                out = (unsigned char *)realloc(out, cap);
            }
            memcpy(out + outn, br.data + br.pos, slen);
            outn += slen;
            br.pos += slen;
        } else if (btype == 1 || btype == 2) {
            NvHuff lit, dist;
            if (btype == 1) {
                unsigned char ll[288];
                for (int i = 0; i < 288; i++) ll[i] = (i < 144) ? 8 : (i < 256) ? 9 : (i < 280) ? 7 : 8;
                unsigned char dl[32];
                for (int i = 0; i < 32; i++) dl[i] = 5;
                nv_huff_build(ll, 288, &lit, line, col);
                nv_huff_build(dl, 32, &dist, line, col);
            } else {
                int hlit = (int)nv_bits(&br, 5, line, col) + 257;
                int hdist = (int)nv_bits(&br, 5, line, col) + 1;
                int hclen = (int)nv_bits(&br, 4, line, col) + 4;
                static const int ORDER[19] = {16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15};
                unsigned char cl[19] = {0};
                for (int i = 0; i < hclen; i++) cl[ORDER[i]] = (unsigned char)nv_bits(&br, 3, line, col);
                NvHuff ch;
                nv_huff_build(cl, 19, &ch, line, col);
                unsigned char lengths[288 + 32];
                int total = hlit + hdist;
                int i = 0;
                while (i < total) {
                    int sym = nv_huff_decode(&br, &ch, line, col);
                    if (sym < 16) {
                        lengths[i++] = (unsigned char)sym;
                    } else if (sym == 16) {
                        if (i == 0) {
                            free(out);
                            nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                        }
                        int rep = 3 + (int)nv_bits(&br, 2, line, col);
                        unsigned char prev = lengths[i - 1];
                        while (rep-- > 0 && i < total) lengths[i++] = prev;
                    } else if (sym == 17) {
                        int rep = 3 + (int)nv_bits(&br, 3, line, col);
                        while (rep-- > 0 && i < total) lengths[i++] = 0;
                    } else if (sym == 18) {
                        int rep = 11 + (int)nv_bits(&br, 7, line, col);
                        while (rep-- > 0 && i < total) lengths[i++] = 0;
                    } else {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                }
                if (i != total) {
                    free(out);
                    nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                }
                nv_huff_build(lengths, hlit, &lit, line, col);
                nv_huff_build(lengths + hlit, hdist, &dist, line, col);
            }
            for (;;) {
                int sym = nv_huff_decode(&br, &lit, line, col);
                if (sym < 256) {
                    if (outn + 1 > 100 * 1024 * 1024) {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                    if (outn + 1 > cap) {
                        cap *= 2;
                        out = (unsigned char *)realloc(out, cap);
                    }
                    out[outn++] = (unsigned char)sym;
                } else if (sym == 256) {
                    break;
                } else {
                    int li = sym - 257;
                    if (li >= 29) {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                    size_t length = LEN_BASE[li] + nv_bits(&br, LEN_EXT[li], line, col);
                    int dsym = nv_huff_decode(&br, &dist, line, col);
                    if (dsym >= 30) {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                    size_t d = DIST_BASE[dsym] + nv_bits(&br, DIST_EXT[dsym], line, col);
                    if (d > outn || d == 0) {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                    if (outn + length > 100 * 1024 * 1024) {
                        free(out);
                        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
                    }
                    while (outn + length > cap) {
                        cap *= 2;
                        out = (unsigned char *)realloc(out, cap);
                    }
                    for (size_t k = 0; k < length; k++) {
                        out[outn] = out[outn - d];
                        outn++;
                    }
                }
            }
        } else {
            free(out);
            nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
        }
        if (bfinal) break;
    }
    *out_len = outn;
    return out;
}

static unsigned char *nv_gunzip(const unsigned char *data, size_t len, size_t *out_len, int line, int col) {
    if (len < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8) {
        nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
    }
    int flg = data[3];
    size_t p = 10;
    if (flg & 0x04) {
        if (p + 2 > len) nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
        unsigned xlen = data[p] | ((unsigned)data[p + 1] << 8);
        p += 2 + xlen;
    }
    if (flg & 0x08) {
        while (p < len && data[p]) p++;
        p++;
    }
    if (flg & 0x10) {
        while (p < len && data[p]) p++;
        p++;
    }
    if (flg & 0x02) p += 2;
    if (p + 8 > len) nv_error(NV_ERR_RUNTIME, "بيانات مضغوطة غير صالحة", line, col);
    size_t dlen = 0;
    unsigned char *out = nv_inflate(data + p, len - p - 8, &dlen, line, col);
    uint32_t crc = (uint32_t)data[len - 8] | ((uint32_t)data[len - 7] << 8)
        | ((uint32_t)data[len - 6] << 16) | ((uint32_t)data[len - 5] << 24);
    uint32_t isize = (uint32_t)data[len - 4] | ((uint32_t)data[len - 3] << 8)
        | ((uint32_t)data[len - 2] << 16) | ((uint32_t)data[len - 1] << 24);
    if (nv_crc32(out, dlen) != crc || (uint32_t)(dlen & 0xFFFFFFFFu) != isize) {
        free(out);
        nv_error(NV_ERR_RUNTIME, "فحص gzip غير مطابق", line, col);
    }
    *out_len = dlen;
    return out;
}

// ----- لينكس8-طبقة1-مرآة: تواريخ HTTP -----

static const char *NV_MONTHS[12] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
static const char *NV_DAYS[7] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};

static void nv_http_date_from_unix(long long t, char *out, size_t cap) {
    time_t tt = (time_t)t;
    struct tm tmv;
    gmtime_r(&tt, &tmv);
    snprintf(out, cap, "%s, %02d %s %04d %02d:%02d:%02d GMT",
             NV_DAYS[tmv.tm_wday], tmv.tm_mday, NV_MONTHS[tmv.tm_mon],
             tmv.tm_year + 1900, tmv.tm_hour, tmv.tm_min, tmv.tm_sec);
}

static long long nv_days_from_civil(long long y, unsigned m, unsigned d) {
    y -= m <= 2;
    long long era = (y >= 0 ? y : y - 399) / 400;
    unsigned yoe = (unsigned)(y - era * 400);
    unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (long long)doe - 719468;
}

static long long nv_parse_http_date(const char *s) {
    int dd, yy, hh, mm, ss;
    char mon[4] = {0};
    char dow[4] = {0};
    if (sscanf(s, "%3s, %d %3s %d %d:%d:%d", dow, &dd, mon, &yy, &hh, &mm, &ss) != 7) return -1;
    int mo = -1;
    for (int i = 0; i < 12; i++) {
        if (strcmp(mon, NV_MONTHS[i]) == 0) mo = i + 1;
    }
    if (mo < 0) return -1;
    long long days = nv_days_from_civil(yy, (unsigned)mo, (unsigned)dd);
    return days * 86400 + hh * 3600 + mm * 60 + ss;
}

static void nv_amz_datetime(long long secs, char *datestamp, size_t dcap, char *amzdate, size_t acap) {
    long long days = secs / 86400;
    long long rem = secs % 86400;
    long long z = days + 719468;
    long long era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned doe = (unsigned)(z - era * 146097);
    unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long long y = (long long)yoe + era * 400;
    unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    unsigned mp = (5 * doy + 2) / 153;
    unsigned d = doy - (153 * mp + 2) / 5 + 1;
    unsigned m = mp < 10 ? mp + 3 : mp - 9;
    y += (m <= 2);
    snprintf(datestamp, dcap, "%04lld%02u%02u", y, m, d);
    snprintf(amzdate, acap, "%04lld%02u%02uT%02lld%02lld%02lldZ", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60);
}


// ----- لينكس8-طبقة1-مرآة: رد المصادقة الهضمية MD5 -----

static int nv_digest_get(const char *challenge, const char *key, char *out, size_t cap) {
    size_t klen = strlen(key);
    char *pat = (char *)malloc(klen + 2);
    memcpy(pat, key, klen);
    pat[klen] = '=';
    pat[klen + 1] = '\0';
    const char *i = strstr(challenge, pat);
    free(pat);
    if (!i) return 0;
    const char *rest = i + klen + 1;
    if (*rest == '"') {
        rest++;
        const char *end = strchr(rest, '"');
        if (!end) return 0;
        size_t n = (size_t)(end - rest);
        if (n >= cap) n = cap - 1;
        memcpy(out, rest, n);
        out[n] = '\0';
        return 1;
    }
    const char *end = rest;
    while (*end && *end != ',' && *end != ' ') end++;
    size_t n = (size_t)(end - rest);
    while (n > 0 && (rest[n - 1] == ' ' || rest[n - 1] == '\t')) n--;
    if (n >= cap) n = cap - 1;
    memcpy(out, rest, n);
    out[n] = '\0';
    return 1;
}

/// رد Digest (MD5) من تحدٍ — NULL إن تعذر (بلا realm/
/// nonce أو بخوارزمية غير MD5).
static char *nv_digest_response(const char *challenge, const char *user, const char *pass,
                                const char *method, const char *uri) {
    char realm[512], nonce[512], qop_full[256] = "", opaque[512] = "", algo[64] = "MD5";
    if (!nv_digest_get(challenge, "realm", realm, sizeof(realm))) return NULL;
    if (!nv_digest_get(challenge, "nonce", nonce, sizeof(nonce))) return NULL;
    nv_digest_get(challenge, "algorithm", algo, sizeof(algo));
    for (char *p = algo; *p; p++) {
        if (*p >= 'a' && *p <= 'z') *p = (char)(*p - 32);
    }
    if (strcmp(algo, "MD5") != 0) return NULL;
    nv_digest_get(challenge, "qop", qop_full, sizeof(qop_full));
    char qop[64] = "";
    {
        const char *comma = strchr(qop_full, ',');
        size_t n = comma ? (size_t)(comma - qop_full) : strlen(qop_full);
        while (n > 0 && (qop_full[n - 1] == ' ' || qop_full[n - 1] == '\t')) n--;
        if (n >= sizeof(qop)) n = sizeof(qop) - 1;
        memcpy(qop, qop_full, n);
        qop[n] = '\0';
    }
    int has_opaque = nv_digest_get(challenge, "opaque", opaque, sizeof(opaque));
    char ha1_in[1200], ha2_in[1200];
    snprintf(ha1_in, sizeof(ha1_in), "%s:%s:%s", user, realm, pass);
    snprintf(ha2_in, sizeof(ha2_in), "%s:%s", method, uri);
    char ha1[33], ha2[33];
    nv_md5_hex((const unsigned char *)ha1_in, strlen(ha1_in), ha1);
    nv_md5_hex((const unsigned char *)ha2_in, strlen(ha2_in), ha2);
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    unsigned long long nanos = (unsigned long long)ts.tv_sec * 1000000000ull + (unsigned long long)ts.tv_nsec;
    char cnonce[32];
    snprintf(cnonce, sizeof(cnonce), "%llx", nanos);
    const char *nc = "00000001";
    char resp_in[2600];
    char response[33];
    if (strcmp(qop, "auth") == 0) {
        snprintf(resp_in, sizeof(resp_in), "%s:%s:%s:%s:%s:%s", ha1, nonce, nc, cnonce, qop, ha2);
    } else {
        snprintf(resp_in, sizeof(resp_in), "%s:%s:%s", ha1, nonce, ha2);
    }
    nv_md5_hex((const unsigned char *)resp_in, strlen(resp_in), response);
    size_t cap = strlen(user) + strlen(realm) + strlen(nonce) + strlen(uri) + strlen(response)
        + strlen(cnonce) + strlen(opaque) + 160;
    char *out = (char *)malloc(cap);
    int n = snprintf(out, cap,
                     "Digest username=\"%s\", realm=\"%s\", nonce=\"%s\", uri=\"%s\", response=\"%s\"",
                     user, realm, nonce, uri, response);
    if (strcmp(qop, "auth") == 0) {
        n += snprintf(out + n, cap - (size_t)n,
                      ", algorithm=MD5, qop=auth, nc=%s, cnonce=\"%s\"", nc, cnonce);
    } else {
        n += snprintf(out + n, cap - (size_t)n, ", algorithm=MD5");
    }
    if (has_opaque) {
        snprintf(out + n, cap - (size_t)n, ", opaque=\"%s\"", opaque);
    }
    return out;
}

// ----- لينكس8-طبقة1-مرآة: قاموس → NvOpts -----

static NvValue *nv_opt_get(NvDict *d, const char *key) {
    int i = nv_dict_find(d, key);
    return i < 0 ? NULL : &d->entries[i].val;
}

static void nv_opt_bad(const char *display, const char *key, int line, int col) {
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s %s غير صالح", display, key);
}

static int nv_opt_bool(NvDict *d, const char *key, const char *display, int line, int col, int *out) {
    NvValue *v = nv_opt_get(d, key);
    if (!v) return 0;
    if (v->tag != NV_BOOL) nv_opt_bad(display, key, line, col);
    *out = v->u.boolean;
    return 1;
}

static int nv_opt_num(NvDict *d, const char *key, const char *display, int line, int col, double *out) {
    NvValue *v = nv_opt_get(d, key);
    if (!v) return 0;
    if (v->tag != NV_NUM) nv_opt_bad(display, key, line, col);
    *out = v->u.num;
    return 1;
}

static int nv_opt_int(NvDict *d, const char *key, const char *display, int line, int col, long long *out) {
    double n;
    if (!nv_opt_num(d, key, display, line, col, &n)) return 0;
    if (n != floor(n) || fabs(n) >= 9e15) nv_opt_bad(display, key, line, col);
    *out = (long long)n;
    return 1;
}

static const char *nv_opt_str(NvDict *d, const char *key, const char *display, int line, int col, int *found) {
    NvValue *v = nv_opt_get(d, key);
    if (!v) {
        *found = 0;
        return NULL;
    }
    *found = 1;
    if (v->tag != NV_STR) nv_opt_bad(display, key, line, col);
    return v->u.str->data;
}

static int nv_opt_list_str(NvDict *d, const char *key, const char *display, int line, int col,
                           char ***out, int *nout) {
    NvValue *v = nv_opt_get(d, key);
    if (!v) return 0;
    if (v->tag != NV_LIST) nv_opt_bad(display, key, line, col);
    NvList *l = v->u.list;
    char **arr = (char **)malloc(sizeof(char *) * (l->len ? (size_t)l->len : 1));
    for (int i = 0; i < l->len; i++) {
        if (l->items[i].tag != NV_STR) nv_opt_bad(display, key, line, col);
        arr[i] = (char *)l->items[i].u.str->data;
    }
    *out = arr;
    *nout = l->len;
    return 1;
}

static int nv_opt_key_safe(const char *k) {
    if (!k || !*k) return 0;
    for (const unsigned char *p = (const unsigned char *)k; *p; p++) {
        unsigned char b = *p;
        if (b < 0x80) {
            int ok = (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '_';
            if (!ok) return 0;
        }
    }
    return 1;
}

static void nv_opts_from_dict(NvDict *d, const char *display, int line, int col, NvOpts *o) {
    static const char *KNOWN[] = {
        "مهلة", "مهلة_النقل", "محاولات", "تأخير", "أقصى_زمن_المحاولات",
        "أعد_عند_الرفض", "أعد_بلا_نهاية", "اتباع_التوجيه", "أقصى_التوجيه",
        "بروتوكولات_التوجيه", "أبق_الجسم_بعد_التوجيه", "توجيه_لموثوق_فقط",
        "تحقق_الشهادة", "ضغط", "تحقق_من_الحالة", "اسمح_http", "إصدار_http",
        "إرسال_متقطع", "ترميز_الرد", "حفظ_في", "حد_الحجم", "منع_الكتابة",
        "تخط_الموجود", "حفظ_وقت_التعديل", "استئناف", "مدى", "وسم_الكيان",
        "شرط_زمني", "وكيل_المستخدم", "إحالة", "لغة", "تتبع_في", "تتبع_نصي",
        "صامت", "مفصل", "أخطاء_فقط", "أظهر_التقدم", "حد_سرعة_التنزيل",
        "حد_سرعة_الرفع", "وكيل", "مصادقة_الوكيل", "بلا_وكيل_ل", "وكيل_socks5",
        "نفق_الوكيل", "ملف_شهادات", "مجلد_شهادات", "شهادة_عميل",
        "منع_بيانات_الدخول", "رؤوس_محذوفة", "تجاهل_كوكيز_الجلسة",
        "مصادقة", "سجل_التنزيل", "إلحاق_السجل", "مجلد_التنزيل",
        "عمق_التكرار", "حوّل_الروابط", "احترام_الروبوتات", "البقاء_في_المضيف",
        "البقاء_في_المجلد", "أنواع_مقبولة", "أنواع_مرفوضة", "أنماط_مقبولة",
        "تتبع_الصور", "تتبع_css", "سطّح_المجلدات", "حد_الملفات",
        "بنية_المجلدات", "أنشئ_المجلدات", "أنشئ_مجلدات_ftp", "حذف_عند_الفشل",
        NULL
    };
    static const char *STRUCT[] = {"رابط", "طريقة", "رؤوس", "كوكيز", "جسم", "نوع_المحتوى", NULL};
    for (int i = 0; i < d->len; i++) {
        const char *k = d->entries[i].key;
        int known = 0;
        for (int j = 0; KNOWN[j]; j++) {
            if (strcmp(k, KNOWN[j]) == 0) { known = 1; break; }
        }
        if (!known) {
            for (int j = 0; STRUCT[j]; j++) {
                if (strcmp(k, STRUCT[j]) == 0) { known = 1; break; }
            }
        }
        if (!known) {
            if (nv_opt_key_safe(k)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s خيار غير معروف: %s", display, k);
            } else {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s خيار غير معروف", display);
            }
        }
    }
    double num;
    long long iv;
    int b;
    int found;
    if (nv_opt_num(d, "مهلة", display, line, col, &num)) {
        if (!(num > 0) || !isfinite(num)) nv_opt_bad(display, "مهلة", line, col);
        o->timeout_secs = (long long)num;
    }
    if (nv_opt_num(d, "مهلة_النقل", display, line, col, &num)) {
        if (!(num > 0) || !isfinite(num)) nv_opt_bad(display, "مهلة_النقل", line, col);
        o->stall_secs = (long long)num;
    }
    if (nv_opt_int(d, "محاولات", display, line, col, &iv)) {
        if (iv < 1) nv_opt_bad(display, "محاولات", line, col);
        o->retries = (int)iv;
    }
    if (nv_opt_num(d, "تأخير", display, line, col, &num)) {
        if (num < 0 || !isfinite(num)) nv_opt_bad(display, "تأخير", line, col);
        o->retry_delay_ms = (long long)(num * 1000);
    }
    if (nv_opt_num(d, "أقصى_زمن_المحاولات", display, line, col, &num)) {
        if (!(num > 0) || !isfinite(num)) nv_opt_bad(display, "أقصى_زمن_المحاولات", line, col);
        o->max_retry_ms = (long long)(num * 1000);
    }
    if (nv_opt_bool(d, "أعد_عند_الرفض", display, line, col, &b)) o->retry_on_refused = b;
    if (nv_opt_bool(d, "أعد_بلا_نهاية", display, line, col, &b)) o->retry_forever = b;
    if (nv_opt_bool(d, "اتباع_التوجيه", display, line, col, &b)) o->follow_redirects = b;
    if (nv_opt_int(d, "أقصى_التوجيه", display, line, col, &iv)) {
        if (iv < 0) nv_opt_bad(display, "أقصى_التوجيه", line, col);
        o->max_redirects = (int)iv;
    }
    {
        char **arr = NULL;
        int n = 0;
        if (nv_opt_list_str(d, "بروتوكولات_التوجيه", display, line, col, &arr, &n)) {
            o->redirect_http = 0;
            o->redirect_https = 0;
            for (int i = 0; i < n; i++) {
                if (strcmp(arr[i], "http") == 0) o->redirect_http = 1;
                else if (strcmp(arr[i], "https") == 0) o->redirect_https = 1;
                else nv_opt_bad(display, "بروتوكولات_التوجيه", line, col);
            }
            free(arr);
        }
    }
    if (nv_opt_bool(d, "أبق_الجسم_بعد_التوجيه", display, line, col, &b)) o->keep_body_on_redirect = b;
    if (nv_opt_bool(d, "توجيه_لموثوق_فقط", display, line, col, &b)) o->trusted_only_redirect = b;
    if (nv_opt_bool(d, "تحقق_الشهادة", display, line, col, &b)) {
        if (!b) {
            nv_error(NV_ERR_RUNTIME, "تجاهل التحقق من الشهادة مرفوض — الشهادة تُتحقق دائما", line, col);
        }
    }
    if (nv_opt_bool(d, "ضغط", display, line, col, &b)) o->gzip = b;
    if (nv_opt_bool(d, "تحقق_من_الحالة", display, line, col, &b)) o->check_status = b;
    if (nv_opt_bool(d, "اسمح_http", display, line, col, &b)) o->allow_http = b;
    {
        const char *v = nv_opt_str(d, "إصدار_http", display, line, col, &found);
        if (found) {
            if (strcmp(v, "1.0") != 0 && strcmp(v, "1.1") != 0) nv_opt_bad(display, "إصدار_http", line, col);
            o->http10 = strcmp(v, "1.0") == 0;
        }
    }
    if (nv_opt_bool(d, "إرسال_متقطع", display, line, col, &b)) o->chunked = b;
    {
        const char *v = nv_opt_str(d, "ترميز_الرد", display, line, col, &found);
        if (found) {
            if (strcmp(v, "utf-8") != 0 && strcmp(v, "latin1") != 0) nv_opt_bad(display, "ترميز_الرد", line, col);
            o->encoding = v;
        }
    }
    {
        const char *v = nv_opt_str(d, "حفظ_في", display, line, col, &found);
        if (found) {
            if (v[0] == '\0') nv_opt_bad(display, "حفظ_في", line, col);
            o->save_to = v;
        }
    }
    if (nv_opt_int(d, "حد_الحجم", display, line, col, &iv)) {
        if (iv <= 0) nv_opt_bad(display, "حد_الحجم", line, col);
        o->max_size = iv;
    }
    if (nv_opt_bool(d, "منع_الكتابة", display, line, col, &b)) o->no_overwrite = b;
    if (nv_opt_bool(d, "تخط_الموجود", display, line, col, &b)) o->skip_existing = b;
    if (nv_opt_bool(d, "حفظ_وقت_التعديل", display, line, col, &b)) o->preserve_mtime = b;
    if (nv_opt_bool(d, "استئناف", display, line, col, &b)) o->resume = b;
    {
        NvValue *v = nv_opt_get(d, "مدى");
        if (v) {
            if (v->tag != NV_DICT) nv_opt_bad(display, "مدى", line, col);
            NvDict *md = v->u.dict;
            NvValue *fv = nv_opt_get(md, "من");
            NvValue *tv = nv_opt_get(md, "إلى");
            long long from = -1, to = -1;
            int has_to = 0;
            if (!fv || fv->tag != NV_NUM) nv_opt_bad(display, "مدى", line, col);
            from = (long long)fv->u.num;
            if (tv) {
                if (tv->tag != NV_NUM) nv_opt_bad(display, "مدى", line, col);
                to = (long long)tv->u.num;
                has_to = 1;
            }
            if (from < 0 || (has_to && to < from)) nv_opt_bad(display, "مدى", line, col);
            o->has_range = 1;
            o->range_from = from;
            o->range_to = to;
            o->has_range_to = has_to;
        }
    }
    {
        const char *v = nv_opt_str(d, "وسم_الكيان", display, line, col, &found);
        if (found) o->etag = v;
    }
    {
        const char *v = nv_opt_str(d, "شرط_زمني", display, line, col, &found);
        if (found) {
            if (v[0] == '\0') nv_opt_bad(display, "شرط_زمني", line, col);
            o->if_modified_since = v;
        }
    }
    {
        const char *v = nv_opt_str(d, "وكيل_المستخدم", display, line, col, &found);
        if (found) o->user_agent = v;
    }
    {
        const char *v = nv_opt_str(d, "إحالة", display, line, col, &found);
        if (found) o->referer = v;
    }
    {
        const char *v = nv_opt_str(d, "لغة", display, line, col, &found);
        if (found) o->language = v;
    }
    {
        const char *v = nv_opt_str(d, "تتبع_في", display, line, col, &found);
        if (found) o->trace_file = v;
    }
    if (nv_opt_bool(d, "تتبع_نصي", display, line, col, &b)) o->trace_text = b;
    if (nv_opt_bool(d, "صامت", display, line, col, &b)) o->silent = b;
    if (nv_opt_bool(d, "مفصل", display, line, col, &b)) o->verbose = b;
    if (nv_opt_bool(d, "أخطاء_فقط", display, line, col, &b)) o->errors_only = b;
    if (nv_opt_bool(d, "أظهر_التقدم", display, line, col, &b)) o->show_progress = b;
    if (nv_opt_int(d, "حد_سرعة_التنزيل", display, line, col, &iv)) {
        if (iv <= 0) nv_opt_bad(display, "حد_سرعة_التنزيل", line, col);
        o->max_speed_down = iv;
    }
    if (nv_opt_int(d, "حد_سرعة_الرفع", display, line, col, &iv)) {
        if (iv <= 0) nv_opt_bad(display, "حد_سرعة_الرفع", line, col);
        o->max_speed_up = iv;
    }
    {
        const char *v = nv_opt_str(d, "وكيل", display, line, col, &found);
        if (found) o->proxy = v;
    }
    {
        NvValue *v = nv_opt_get(d, "مصادقة_الوكيل");
        if (v) {
            if (v->tag != NV_DICT) nv_opt_bad(display, "مصادقة_الوكيل", line, col);
            NvDict *ad = v->u.dict;
            NvValue *uv = nv_opt_get(ad, "مستخدم");
            NvValue *pv = nv_opt_get(ad, "كلمة_سر");
            if (!uv || uv->tag != NV_STR || !pv || pv->tag != NV_STR) {
                nv_opt_bad(display, "مصادقة_الوكيل", line, col);
            }
            o->proxy_user = uv->u.str->data;
            o->proxy_pass = pv->u.str->data;
        }
    }
    {
        char **arr = NULL;
        int n = 0;
        if (nv_opt_list_str(d, "بلا_وكيل_ل", display, line, col, &arr, &n)) {
            o->no_proxy = arr;
            o->n_no_proxy = n;
        }
    }
    {
        const char *v = nv_opt_str(d, "وكيل_socks5", display, line, col, &found);
        if (found) o->socks5 = v;
    }
    if (nv_opt_bool(d, "نفق_الوكيل", display, line, col, &b)) o->tunnel_proxy = b;
    {
        const char *v = nv_opt_str(d, "ملف_شهادات", display, line, col, &found);
        if (found) o->ca_file = v;
    }
    {
        const char *v = nv_opt_str(d, "مجلد_شهادات", display, line, col, &found);
        if (found) o->ca_dir = v;
    }
    {
        NvValue *v = nv_opt_get(d, "شهادة_عميل");
        if (v) {
            if (v->tag != NV_DICT) nv_opt_bad(display, "شهادة_عميل", line, col);
            NvDict *cd = v->u.dict;
            NvValue *cv = nv_opt_get(cd, "شهادة");
            NvValue *kv = nv_opt_get(cd, "مفتاح");
            if (!cv || cv->tag != NV_STR || !kv || kv->tag != NV_STR) {
                nv_opt_bad(display, "شهادة_عميل", line, col);
            }
            o->client_cert = cv->u.str->data;
            o->client_key = kv->u.str->data;
        }
    }
    if (nv_opt_bool(d, "منع_بيانات_الدخول", display, line, col, &b)) o->no_userinfo = b;
    {
        char **arr = NULL;
        int n = 0;
        if (nv_opt_list_str(d, "رؤوس_محذوفة", display, line, col, &arr, &n)) {
            o->removed = arr;
            o->n_removed = n;
        }
    }
    if (nv_opt_bool(d, "تجاهل_كوكيز_الجلسة", display, line, col, &b)) o->ignore_session_cookies = b;
    {
        NvValue *v = nv_opt_get(d, "مصادقة");
        if (v) {
            if (v->tag != NV_DICT) nv_opt_bad(display, "مصادقة", line, col);
            NvDict *ad = v->u.dict;
            NvValue *tv = nv_opt_get(ad, "نوع");
            if (!tv || tv->tag != NV_STR) nv_opt_bad(display, "مصادقة", line, col);
            const char *type = tv->u.str->data;
            NvValue *uv, *pv;
            if (strcmp(type, "أساسية") == 0 || strcmp(type, "هضمية") == 0 || strcmp(type, "تفاوضية") == 0) {
                uv = nv_opt_get(ad, "مستخدم");
                pv = nv_opt_get(ad, "كلمة_سر");
                if (!uv || uv->tag != NV_STR || !pv || pv->tag != NV_STR) {
                    nv_opt_bad(display, "مصادقة", line, col);
                }
                o->auth_kind = strcmp(type, "أساسية") == 0 ? 1 : strcmp(type, "هضمية") == 0 ? 2 : 3;
                o->auth_user = uv->u.str->data;
                o->auth_pass = pv->u.str->data;
            } else if (strcmp(type, "حامل") == 0) {
                NvValue *rv = nv_opt_get(ad, "رمز");
                if (!rv || rv->tag != NV_STR) nv_opt_bad(display, "مصادقة", line, col);
                o->auth_kind = 4;
                o->auth_token = rv->u.str->data;
            } else if (strcmp(type, "أمازون") == 0) {
                NvValue *kv = nv_opt_get(ad, "مفتاح");
                NvValue *sv = nv_opt_get(ad, "سر");
                if (!kv || kv->tag != NV_STR || !sv || sv->tag != NV_STR) {
                    nv_opt_bad(display, "مصادقة", line, col);
                }
                o->auth_kind = 5;
                o->auth_key = kv->u.str->data;
                o->auth_secret = sv->u.str->data;
                NvValue *rg = nv_opt_get(ad, "منطقة");
                NvValue *sc = nv_opt_get(ad, "خدمة");
                o->auth_region = (rg && rg->tag == NV_STR) ? rg->u.str->data : "us-east-1";
                o->auth_service = (sc && sc->tag == NV_STR) ? sc->u.str->data : "s3";
            } else if (strcmp(type, "توقيع") == 0) {
                NvValue *iv2 = nv_opt_get(ad, "معرف");
                NvValue *kv = nv_opt_get(ad, "مفتاح");
                if (!iv2 || iv2->tag != NV_STR || !kv || kv->tag != NV_STR) {
                    nv_opt_bad(display, "مصادقة", line, col);
                }
                o->auth_kind = 6;
                o->auth_key_id = iv2->u.str->data;
                o->auth_key2 = kv->u.str->data;
                NvValue *av = nv_opt_get(ad, "خوارزمية");
                o->auth_algo = (av && av->tag == NV_STR) ? av->u.str->data : "hmac-sha256";
            } else {
                nv_opt_bad(display, "مصادقة", line, col);
            }
        }
    }
    if (o->retry_forever && o->max_retry_ms == 0) {
        nv_error(NV_ERR_RUNTIME, "إعادة المحاولة بلا نهاية تتطلب أقصى زمن للمحاولات", line, col);
    }
}


// ----- لينكس8-طبقة1-مرآة: تنفيذ الطلب الموحد -----

typedef struct {
    char **names;
    char **values;
    int n;
    int cap;
} NvHdrList;

static void nv_hdr_push(NvHdrList *l, const char *name, const char *value) {
    if (l->n >= l->cap) {
        l->cap = l->cap ? l->cap * 2 : 16;
        l->names = (char **)realloc(l->names, sizeof(char *) * (size_t)l->cap);
        l->values = (char **)realloc(l->values, sizeof(char *) * (size_t)l->cap);
    }
    l->names[l->n] = strdup(name);
    l->values[l->n] = strdup(value);
    l->n++;
}

static const char *nv_meta_find(NvSink *s, const char *lower_name) {
    for (int i = 0; i < s->meta_n; i++) {
        if (strcmp(s->meta_names[i], lower_name) == 0) return s->meta_values[i];
    }
    return NULL;
}

static int nv_retryable_msg(const char *msg) {
    if (!msg) return 0;
    return strstr(msg, "تعذر الاتصال") != NULL || strstr(msg, "مهلة") != NULL
        || strstr(msg, "تعذر حل اسم") != NULL || strstr(msg, "تعذر قراءة") != NULL
        || strstr(msg, "انقطع") != NULL || strstr(msg, "رد غير صالح") != NULL;
}

static char *nv_utf8_lossy(const unsigned char *in, size_t len, size_t *out_len) {
    char *out = (char *)malloc(len * 3 + 1);
    size_t o = 0, i = 0;
    while (i < len) {
        unsigned char c = in[i];
        if (c < 0x80) {
            out[o++] = (char)c;
            i++;
            continue;
        }
        int need = 0;
        uint32_t cp = 0;
        if ((c & 0xE0) == 0xC0) { need = 2; cp = c & 0x1F; }
        else if ((c & 0xF0) == 0xE0) { need = 3; cp = c & 0x0F; }
        else if ((c & 0xF8) == 0xF0) { need = 4; cp = c & 0x07; }
        else {
            out[o++] = (char)0xEF; out[o++] = (char)0xBF; out[o++] = (char)0xBD;
            i++;
            continue;
        }
        if (i + (size_t)need > len) {
            out[o++] = (char)0xEF; out[o++] = (char)0xBF; out[o++] = (char)0xBD;
            i = len;
            continue;
        }
        int ok = 1;
        for (int k = 1; k < need; k++) {
            if ((in[i + k] & 0xC0) != 0x80) ok = 0;
        }
        if (ok) {
            for (int k = 1; k < need; k++) cp = (cp << 6) | (in[i + k] & 0x3F);
            if ((need == 2 && cp < 0x80) || (need == 3 && cp < 0x800)
                || (need == 4 && cp < 0x10000) || cp > 0x10FFFF
                || (cp >= 0xD800 && cp <= 0xDFFF)) {
                ok = 0;
            }
        }
        if (!ok) {
            out[o++] = (char)0xEF; out[o++] = (char)0xBF; out[o++] = (char)0xBD;
            i++;
            continue;
        }
        memcpy(out + o, in + i, (size_t)need);
        o += (size_t)need;
        i += (size_t)need;
    }
    out[o] = '\0';
    *out_len = o;
    return out;
}

static char *nv_latin1_to_utf8(const unsigned char *in, size_t len, size_t *out_len) {
    char *out = (char *)malloc(len * 2 + 1);
    size_t o = 0;
    for (size_t i = 0; i < len; i++) {
        unsigned char b = in[i];
        if (b < 0x80) {
            out[o++] = (char)b;
        } else {
            out[o++] = (char)(0xC0 | (b >> 6));
            out[o++] = (char)(0x80 | (b & 0x3F));
        }
    }
    out[o] = '\0';
    *out_len = o;
    return out;
}

/// تحقق اسم ترويسة (نسخة نفذ الصارمة: رموز ASCII + -_).
static int nv_std_header_name_ok(const char *name) {
    if (!name || !*name) return 0;
    for (const unsigned char *p = (const unsigned char *)name; *p; p++) {
        unsigned char b = *p;
        int ok = (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9')
            || b == '-' || b == '_';
        if (!ok) return 0;
    }
    return 1;
}

/// قراءة قاموس نصي (رؤوس/كوكيز) برسائل نفذ.
static void nv_net_text_dict(NvValue *v, const char *display, int line, int col,
                             const char ***keys_out, const char ***vals_out, int *n_out) {
    if (v->tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(*v));
    }
    NvDict *d = v->u.dict;
    const char **keys = (const char **)malloc(sizeof(char *) * (d->len ? (size_t)d->len : 1));
    const char **vals = (const char **)malloc(sizeof(char *) * (d->len ? (size_t)d->len : 1));
    for (int i = 0; i < d->len; i++) {
        if (d->entries[i].val.tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قيما نصية فقط لكن أعطيت %s",
                         display, nv_type_name(d->entries[i].val));
        }
        keys[i] = d->entries[i].key;
        vals[i] = d->entries[i].val.u.str->data;
    }
    *keys_out = keys;
    *vals_out = vals;
    *n_out = d->len;
}

NvValue nv_net_execute(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نفذ تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نفذ تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    NvDict *d = args[0].u.dict;
    NvValue *uv = nv_opt_get(d, "رابط");
    if (!uv) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.نفذ تتوقع مفتاح رابط في الطلب", line, col);
    }
    if (uv->tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نفذ تتوقع رابطا نصا لكن أعطيت %s", nv_type_name(*uv));
    }
    const char *url_in = uv->u.str->data;
    if (url_in[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.نفذ تتوقع رابطا غير فارغ", line, col);
    }
    const char *method = "GET";
    NvValue *mv = nv_opt_get(d, "طريقة");
    if (mv) {
        if (mv->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نفذ تتوقع طريقة نصا لكن أعطيت %s", nv_type_name(*mv));
        }
        method = mv->u.str->data;
    }
    if (method[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.نفذ طريقة غير صالحة", line, col);
    }
    for (const unsigned char *p = (const unsigned char *)method; *p; p++) {
        if (*p <= ' ' || *p == 0x7f) {
            nv_error(NV_ERR_TYPE, "الدالة شبكة.نفذ طريقة غير صالحة", line, col);
        }
    }
    const char **hnames = NULL, **hvalues = NULL;
    int nhdr = 0;
    NvValue *hv = nv_opt_get(d, "رؤوس");
    if (hv) {
        nv_net_text_dict(hv, "شبكة.نفذ", line, col, &hnames, &hvalues, &nhdr);
        for (int i = 0; i < nhdr; i++) {
            if (hnames[i][0] == '\0') {
                nv_error(NV_ERR_RUNTIME, "اسم ترويسة فارغ — الترويسة تحتاج اسما غير فارغ", line, col);
            }
            if (!nv_std_header_name_ok(hnames[i])) {
                nv_error(NV_ERR_RUNTIME, "اسم ترويسة غير صالح — الاسم حروف رمزية فقط", line, col);
            }
            if (!nv_header_value_ok(hvalues[i])) {
                nv_error(NV_ERR_RUNTIME, "قيمة ترويسة غير صالحة — لا يُسمح بأسطر جديدة في الترويسات", line, col);
            }
        }
    }
    const unsigned char *body = NULL;
    size_t body_len = 0;
    unsigned char *body_alloc = NULL;
    NvValue *bv = nv_opt_get(d, "جسم");
    if (bv) {
        if (bv->tag == NV_STR) {
            body = (const unsigned char *)bv->u.str->data;
            body_len = bv->u.str->len;
        } else if (bv->tag == NV_LIST) {
            NvList *l = bv->u.list;
            body_alloc = (unsigned char *)malloc(l->len ? (size_t)l->len : 1);
            for (int i = 0; i < l->len; i++) {
                NvValue *it = &l->items[i];
                if (it->tag != NV_NUM || it->u.num != floor(it->u.num) || it->u.num < 0 || it->u.num > 255) {
                    nv_error(NV_ERR_TYPE, "الدالة شبكة.نفذ جسم غير صالح — القائمة بايتات 0 إلى 255", line, col);
                }
                body_alloc[i] = (unsigned char)it->u.num;
            }
            body = body_alloc;
            body_len = (size_t)l->len;
        } else {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نفذ تتوقع جسما نصا أو قائمة بايتات لكن أعطيت %s", nv_type_name(*bv));
        }
    }
    const char *content_type = "text/plain; charset=utf-8";
    NvValue *cv = nv_opt_get(d, "نوع_المحتوى");
    if (cv) {
        if (cv->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نفذ تتوقع نوع محتوى نصا لكن أعطيت %s", nv_type_name(*cv));
        }
        content_type = cv->u.str->data;
    }
    NvOpts opts;
    nv_opts_default(&opts);
    nv_opts_from_dict(d, "شبكة.نفذ", line, col, &opts);
    const char *encoding = opts.encoding ? opts.encoding : "utf-8";
    const char **ckeys = NULL, **cvals = NULL;
    int nck = 0;
    NvValue *kv = nv_opt_get(d, "كوكيز");
    if (kv) {
        nv_net_text_dict(kv, "شبكة.نفذ", line, col, &ckeys, &cvals, &nck);
    }
    // تتبع: حل المسار قبل بوابة الشبكة (كما في نفذ).
    char *trace_resolved = NULL;
    if (opts.trace_file) {
        trace_resolved = nv_resolve_path(opts.trace_file, line, col);
        opts.trace_file = trace_resolved;
    }
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
    // بيانات دخول في العنوان (تُستعمل أساسية فقط إن سمح الخيار).
    char *url_owned = strdup(url_in);
    char *ui_user = NULL, *ui_pass = NULL;
    if (!opts.no_userinfo) {
        char *sep = strstr(url_owned, "://");
        if (sep) {
            char *after = sep + 3;
            char *auth_end = after;
            while (*auth_end && *auth_end != '/' && *auth_end != '?' && *auth_end != '#') auth_end++;
            char *at = NULL;
            for (char *p = after; p < auth_end; p++) {
                if (*p == '@') at = p;
            }
            if (at) {
                size_t ulen = (size_t)(at - after);
                char *ui = (char *)malloc(ulen + 1);
                memcpy(ui, after, ulen);
                ui[ulen] = '\0';
                char *colon = strchr(ui, ':');
                if (colon) {
                    *colon = '\0';
                    ui_user = strdup(ui);
                    ui_pass = strdup(colon + 1);
                } else {
                    ui_user = strdup(ui);
                    ui_pass = strdup("");
                }
                free(ui);
                size_t pre = (size_t)(sep + 3 - url_owned);
                char *rebuilt = (char *)malloc(pre + strlen(at + 1) + 1);
                memcpy(rebuilt, url_owned, pre);
                strcpy(rebuilt + pre, at + 1);
                free(url_owned);
                url_owned = rebuilt;
            }
        }
    }
    NvUrl parsed;
    nv_parse_url(url_owned, &parsed, line, col);
    if (strcmp(parsed.scheme, "http") == 0 && !opts.allow_http && !nv_local_host_allowed(parsed.host)) {
        nv_error(NV_ERR_RUNTIME, "المخطط http غير مسموح — استخدم https أو فعّل اسمح_http", line, col);
    }
    nv_validate_caller_headers(hnames, hvalues, nhdr, line, col);
    // الطريقة مكبّرة (GET/POST أساس، وغيرهما مخصصة).
    char *method_up = strdup(method);
    for (char *p = method_up; *p; p++) {
        if (*p >= 'a' && *p <= 'z') *p = (char)(*p - 32);
    }
    // الترويسات الفعالة: المتصل، ثم المصادقة/التوقيع.
    NvHdrList eff = {0};
    for (int i = 0; i < nhdr; i++) nv_hdr_push(&eff, hnames[i], hvalues[i]);
    if (opts.auth_kind == 0 && ui_user) {
        char *cred = nv_b64_encode(ui_user, ui_pass);
        size_t cap = strlen(cred) + 16;
        char *auth = (char *)malloc(cap);
        snprintf(auth, cap, "Basic %s", cred);
        free(cred);
        nv_hdr_push(&eff, "Authorization", auth);
        free(auth);
    }
    if (opts.auth_kind == 5) {
        struct timespec ts;
        clock_gettime(CLOCK_REALTIME, &ts);
        long long now = (long long)ts.tv_sec;
        char datestamp[16], amzdate[24];
        nv_amz_datetime(now, datestamp, sizeof(datestamp), amzdate, sizeof(amzdate));
        char payload_hash[65];
        nv_sha256_hex(body ? body : (const unsigned char *)"", body_len, payload_hash);
        const char *q = strchr(parsed.path, '?');
        size_t uri_len = q ? (size_t)(q - parsed.path) : strlen(parsed.path);
        char *uri = (char *)malloc(uri_len + 1);
        memcpy(uri, parsed.path, uri_len);
        uri[uri_len] = '\0';
        const char *query = q ? q + 1 : "";
        char host_hdr[1024];
        if ((strcmp(parsed.scheme, "http") == 0 && parsed.port == 80)
            || (strcmp(parsed.scheme, "https") == 0 && parsed.port == 443)) {
            snprintf(host_hdr, sizeof(host_hdr), "%s", parsed.host);
        } else {
            snprintf(host_hdr, sizeof(host_hdr), "%s:%d", parsed.host, parsed.port);
        }
        size_t ch_cap = strlen(host_hdr) + strlen(payload_hash) + strlen(amzdate) + 128;
        char *ch = (char *)malloc(ch_cap);
        snprintf(ch, ch_cap, "host:%s\nx-amz-content-sha256:%s\nx-amz-date:%s\n",
                 host_hdr, payload_hash, amzdate);
        size_t cr_cap = strlen(method_up) + uri_len + strlen(query) + strlen(ch) + strlen(payload_hash) + 64;
        char *cr = (char *)malloc(cr_cap);
        snprintf(cr, cr_cap, "%s\n%s\n%s\n%shost;x-amz-content-sha256;x-amz-date\n%s",
                 method_up, uri, query, ch, payload_hash);
        char cr_hash[65];
        nv_sha256_hex((const unsigned char *)cr, strlen(cr), cr_hash);
        char scope[512];
        snprintf(scope, sizeof(scope), "%s/%s/%s/aws4_request", datestamp, opts.auth_region, opts.auth_service);
        size_t ts_cap = strlen(amzdate) + strlen(scope) + 65 + 32;
        char *to_sign = (char *)malloc(ts_cap);
        snprintf(to_sign, ts_cap, "AWS4-HMAC-SHA256\n%s\n%s\n%s", amzdate, scope, cr_hash);
        char aws4_key[512];
        snprintf(aws4_key, sizeof(aws4_key), "AWS4%s", opts.auth_secret);
        unsigned char k_date[32], k_region[32], k_service[32], k_signing[32], sig_raw[32];
        nv_hmac_sha256((const unsigned char *)aws4_key, strlen(aws4_key),
                       (const unsigned char *)datestamp, strlen(datestamp), k_date);
        nv_hmac_sha256(k_date, 32, (const unsigned char *)opts.auth_region, strlen(opts.auth_region), k_region);
        nv_hmac_sha256(k_region, 32, (const unsigned char *)opts.auth_service, strlen(opts.auth_service), k_service);
        nv_hmac_sha256(k_service, 32, (const unsigned char *)"aws4_request", 12, k_signing);
        nv_hmac_sha256(k_signing, 32, (const unsigned char *)to_sign, strlen(to_sign), sig_raw);
        char signature[65];
        nv_hex_bytes(sig_raw, 32, signature);
        nv_hdr_push(&eff, "x-amz-date", amzdate);
        nv_hdr_push(&eff, "x-amz-content-sha256", payload_hash);
        size_t au_cap = strlen(opts.auth_key) + strlen(scope) + strlen(signature) + 128;
        char *auth = (char *)malloc(au_cap);
        snprintf(auth, au_cap,
                 "AWS4-HMAC-SHA256 Credential=%s/%s, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=%s",
                 opts.auth_key, scope, signature);
        nv_hdr_push(&eff, "Authorization", auth);
        free(auth);
        free(uri); free(ch); free(cr); free(to_sign);
    }
    if (opts.auth_kind == 6) {
        if (strcmp(opts.auth_algo, "hmac-sha256") != 0) {
            nv_error(NV_ERR_RUNTIME, "خوارزمية التوقيع غير مدعومة — المتاح hmac-sha256 فقط", line, col);
        }
        struct timespec ts;
        clock_gettime(CLOCK_REALTIME, &ts);
        char date[64];
        nv_http_date_from_unix((long long)ts.tv_sec, date, sizeof(date));
        char host_hdr[1024];
        if ((strcmp(parsed.scheme, "http") == 0 && parsed.port == 80)
            || (strcmp(parsed.scheme, "https") == 0 && parsed.port == 443)) {
            snprintf(host_hdr, sizeof(host_hdr), "%s", parsed.host);
        } else {
            snprintf(host_hdr, sizeof(host_hdr), "%s:%d", parsed.host, parsed.port);
        }
        char *method_lo = strdup(method_up);
        for (char *p = method_lo; *p; p++) {
            if (*p >= 'A' && *p <= 'Z') *p = (char)(*p + 32);
        }
        size_t sg_cap = strlen(method_lo) + strlen(parsed.path) + strlen(host_hdr) + strlen(date) + 64;
        char *signing = (char *)malloc(sg_cap);
        snprintf(signing, sg_cap, "(request-target): %s %s\nhost: %s\ndate: %s",
                 method_lo, parsed.path, host_hdr, date);
        unsigned char sig_raw[32];
        nv_hmac_sha256((const unsigned char *)opts.auth_key2, strlen(opts.auth_key2),
                       (const unsigned char *)signing, strlen(signing), sig_raw);
        // ترميز base64 للبايتات الخام.
        char *sig_b64;
        {
            static const char tbl[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            size_t outlen = ((32 + 2) / 3) * 4;
            sig_b64 = (char *)malloc(outlen + 1);
            size_t o = 0;
            for (size_t i = 0; i < 32; i += 3) {
                int rem = (int)(32 - i);
                unsigned int v = ((unsigned int)sig_raw[i]) << 16;
                if (rem > 1) v |= ((unsigned int)sig_raw[i + 1]) << 8;
                if (rem > 2) v |= sig_raw[i + 2];
                sig_b64[o++] = tbl[(v >> 18) & 63];
                sig_b64[o++] = tbl[(v >> 12) & 63];
                sig_b64[o++] = rem > 1 ? tbl[(v >> 6) & 63] : '=';
                sig_b64[o++] = rem > 2 ? tbl[v & 63] : '=';
            }
            sig_b64[o] = '\0';
        }
        nv_hdr_push(&eff, "date", date);
        size_t au_cap = strlen(opts.auth_key_id) + strlen(sig_b64) + 128;
        char *auth = (char *)malloc(au_cap);
        snprintf(auth, au_cap,
                 "Signature keyId=\"%s\",algorithm=\"hmac-sha256\",headers=\"(request-target) host date\",signature=\"%s\"",
                 opts.auth_key_id, sig_b64);
        nv_hdr_push(&eff, "Authorization", auth);
        free(auth);
        free(sig_b64);
        free(signing);
        free(method_lo);
    }
    if (opts.auth_kind == 1) {
        char *cred = nv_b64_encode(opts.auth_user, opts.auth_pass);
        size_t cap = strlen(cred) + 16;
        char *auth = (char *)malloc(cap);
        snprintf(auth, cap, "Basic %s", cred);
        free(cred);
        nv_hdr_push(&eff, "Authorization", auth);
        free(auth);
    }
    if (opts.auth_kind == 4) {
        size_t cap = strlen(opts.auth_token) + 16;
        char *auth = (char *)malloc(cap);
        snprintf(auth, cap, "Bearer %s", opts.auth_token);
        nv_hdr_push(&eff, "Authorization", auth);
        free(auth);
    }
    // إعداد الحفظ.
    char *resolved = NULL, *display = NULL;
    size_t existing = 0;
    size_t limit = 0;
    long long resume_from = 0;
    if (opts.save_to) {
        resolved = nv_resolve_path(opts.save_to, line, col);
        display = nv_display_path(resolved);
        struct stat st;
        if (stat(resolved, &st) == 0) existing = (size_t)st.st_size;
        limit = nv_quota_remaining() + existing;
        if (opts.no_overwrite && existing > 0) {
            nv_error(NV_ERR_RUNTIME, "الملف موجود — الكتابة مرفوضة", line, col);
        }
        if (opts.skip_existing && existing > 0) {
            NvValue out = nv_dict_new();
            nv_dict_set_raw(out.u.dict, "رمز", nv_num(0));
            nv_dict_set_raw(out.u.dict, "جسم", nv_str_new("", 0));
            NvValue hdrs = nv_dict_new();
            nv_dict_set_raw(out.u.dict, "رؤوس", hdrs);
            nv_dict_set_raw(out.u.dict, "بايتات", nv_num((double)existing));
            return out;
        }
        if (opts.resume) resume_from = (long long)existing;
    }
    // المدى / الشرط الزمني / وسم الكيان / الكوكيز.
    if (opts.has_range) {
        char range[64];
        if (opts.has_range_to) {
            snprintf(range, sizeof(range), "bytes=%lld-%lld", opts.range_from, opts.range_to);
        } else {
            snprintf(range, sizeof(range), "bytes=%lld-", opts.range_from);
        }
        nv_hdr_push(&eff, "Range", range);
    } else if (resolved && resume_from > 0) {
        char range[64];
        snprintf(range, sizeof(range), "bytes=%lld-", resume_from);
        nv_hdr_push(&eff, "Range", range);
    }
    if (opts.etag) nv_hdr_push(&eff, "If-None-Match", opts.etag);
    if (opts.if_modified_since) nv_hdr_push(&eff, "If-Modified-Since", opts.if_modified_since);
    if (nck > 0 && !opts.ignore_session_cookies) {
        size_t cap = 4;
        for (int i = 0; i < nck; i++) cap += strlen(ckeys[i]) + strlen(cvals[i]) + 4;
        char *cookie = (char *)malloc(cap);
        cookie[0] = '\0';
        for (int i = 0; i < nck; i++) {
            if (i) strcat(cookie, "; ");
            strcat(cookie, ckeys[i]);
            strcat(cookie, "=");
            strcat(cookie, cvals[i]);
        }
        nv_hdr_push(&eff, "Cookie", cookie);
        free(cookie);
    }
    int show_progress = opts.show_progress && !opts.silent && !opts.errors_only;
    int trace_on = !opts.silent && opts.trace_text;
    int verbose = opts.verbose;
    if (show_progress) {
        fprintf(stderr, "جلب %s ...\n", url_owned);
    }
    // قالب الطلب (يُنسخ في كل محاولة — فالتوجيه يحوّله).
    NvRequest treq;
    memset(&treq, 0, sizeof(treq));
    if (strcmp(method_up, "GET") == 0) {
        treq.method = NULL;
        treq.is_post = 0;
    } else if (strcmp(method_up, "POST") == 0) {
        treq.method = NULL;
        treq.is_post = 1;
    } else {
        treq.method = method_up;
        treq.is_post = 1;
    }
    treq.body = body;
    treq.body_len = body_len;
    treq.content_type = content_type;
    treq.opts = &opts;
    /* لينكس8-طبقة1-مرآة: نفذ يعيد كل رمز (مرآة opts2.return_all_status = true في Rust) */
    opts.return_all = 1;
    // حلقة المحاولات (الخارجية تحمي الحفظ عند الفشل).
    NvValue result = nv_nil();
    int have_result = 0;
    NvHandler h0;
    volatile NvCheckpoint cp0 = nv_checkpoint();
    h0.prev = nv_handlers;
    NvHandler *volatile hp0 = h0.prev;
    h0.has_except = 0;
    nv_handlers = &h0;
    if (setjmp(h0.jb) == 0) {
        long long total_attempts = opts.retry_forever ? (long long)4000000000000000000LL : (long long)opts.retries;
        long long attempt = 0;
        char *digest_auth = NULL;
        long long start_ms = nv_net_now_ms();
        NvSink sink;
        memset(&sink, 0, sizeof(sink));
        int done = 0;
        while (!done) {
            attempt++;
            free(sink.buf);
            sink.buf = NULL;
            sink.len = 0;
            sink.cap = 0;
            nv_sink_free_meta(&sink);
            sink.meta_status = 0;
            FILE *f = NULL;
            if (resolved) {
                f = fopen(resolved, resume_from > 0 ? "ab" : "wb");
                if (!f) {
                    nv_net_err_runtime(line, col, "تعذر كتابة الملف %s", display);
                }
                if (resume_from > 0) {
                    ftruncate(fileno(f), (off_t)resume_from);
                }
                sink.file = f;
                sink.limit = limit;
                sink.written = (size_t)resume_from;
            }
            NvHdrList try_hdrs = {0};
            for (int i = 0; i < eff.n; i++) {
                if (try_hdrs.n >= try_hdrs.cap) {
                    try_hdrs.cap = try_hdrs.cap ? try_hdrs.cap * 2 : 16;
                    try_hdrs.names = (char **)realloc(try_hdrs.names, sizeof(char *) * (size_t)try_hdrs.cap);
                    try_hdrs.values = (char **)realloc(try_hdrs.values, sizeof(char *) * (size_t)try_hdrs.cap);
                }
                try_hdrs.names[try_hdrs.n] = eff.names[i];
                try_hdrs.values[try_hdrs.n] = eff.values[i];
                try_hdrs.n++;
            }
            if (digest_auth) {
                if (try_hdrs.n >= try_hdrs.cap) {
                    try_hdrs.cap = try_hdrs.cap ? try_hdrs.cap * 2 : 16;
                    try_hdrs.names = (char **)realloc(try_hdrs.names, sizeof(char *) * (size_t)try_hdrs.cap);
                    try_hdrs.values = (char **)realloc(try_hdrs.values, sizeof(char *) * (size_t)try_hdrs.cap);
                }
                try_hdrs.names[try_hdrs.n] = "Authorization";
                try_hdrs.values[try_hdrs.n] = digest_auth;
                try_hdrs.n++;
            }
            NvRequest areq = treq;
            areq.hdr_names = (const char **)try_hdrs.names;
            areq.hdr_values = (const char **)try_hdrs.values;
            areq.nhdr = try_hdrs.n;
            if (verbose) {
                fprintf(stderr, "%s %s\n", method_up, url_owned);
            }
            NvHandler h;
            volatile NvCheckpoint cp = nv_checkpoint();
            h.prev = nv_handlers;
            NvHandler *volatile hp = h.prev;
            h.has_except = 0;
            nv_handlers = &h;
            int fetch_ok = 0;
            if (setjmp(h.jb) == 0) {
                nv_fetch_loop(url_owned, &areq, line, col, &sink);
                fetch_ok = 1;
            } else {
                NvError *e = nv_current_error();
                const char *msg = (e && e->message) ? e->message : "";
                int is_refused = strstr(msg, "تعذر الاتصال") != NULL;
                int retryable = nv_retryable_msg(msg);
                nv_restore_checkpoint(cp);
                nv_handlers = hp;
                if (f) fclose(f);
                sink.file = NULL;
                free(try_hdrs.names);
                free(try_hdrs.values);
                if (is_refused && !opts.retry_on_refused) nv_reraise_current();
                if (!retryable) nv_reraise_current();
                if (attempt >= total_attempts) nv_reraise_current();
                if (!opts.retry_forever && opts.max_retry_ms > 0
                    && nv_net_now_ms() - start_ms >= opts.max_retry_ms) {
                    nv_reraise_current();
                }
                nv_net_sleep_ms(opts.retry_delay_ms);
                continue;
            }
            nv_handlers = hp;
            (void)fetch_ok;
            if (f) {
                fclose(f);
                sink.file = NULL;
            }
            int status = sink.meta_status;
            if (verbose) {
                fprintf(stderr, "%s %s -> %d\n", method_up, url_owned, status);
                for (int i = 0; i < sink.meta_n; i++) {
                    fprintf(stderr, "  %s: %s\n", sink.meta_names[i], sink.meta_values[i]);
                }
            }
            if (status == 401 && !digest_auth) {
                if (opts.auth_kind == 2) {
                    const char *ch = nv_meta_find(&sink, "www-authenticate");
                    if (ch) {
                        char *dg = nv_digest_response(ch, opts.auth_user, opts.auth_pass, method_up, parsed.path);
                        if (dg) {
                            digest_auth = dg;
                            free(try_hdrs.names);
                            free(try_hdrs.values);
                            continue;
                        }
                    }
                }
                if (opts.auth_kind == 3) {
                    free(try_hdrs.names);
                    free(try_hdrs.values);
                    nv_error(NV_ERR_RUNTIME, "المصادقة التفاوضية غير مدعومة في هذا البناء", line, col);
                }
            }
            if ((status == 429 || (status >= 500 && status < 600)) && attempt < total_attempts) {
                int over_time = !opts.retry_forever && opts.max_retry_ms > 0
                    && nv_net_now_ms() - start_ms >= opts.max_retry_ms;
                if (!over_time) {
                    free(try_hdrs.names);
                    free(try_hdrs.values);
                    nv_net_sleep_ms(opts.retry_delay_ms);
                    continue;
                }
            }
            if (opts.check_status && !(status >= 200 && status < 300)) {
                free(try_hdrs.names);
                free(try_hdrs.values);
                nv_net_err_runtime(line, col, "الخادم رد برمز الحالة %d", status);
            }
            int is_gzip = 0;
            if (opts.gzip) {
                const char *ce = nv_meta_find(&sink, "content-encoding");
                if (ce) {
                    for (const char *p = ce; *p; p++) {
                        if (strncasecmp(p, "gzip", 4) == 0) { is_gzip = 1; break; }
                    }
                }
            }
            unsigned long long nbytes = 0;
            unsigned char *raw = NULL;
            size_t raw_len = 0;
            if (resolved) {
                if (is_gzip) {
                    FILE *rf = fopen(resolved, "rb");
                    if (rf) {
                        fseek(rf, 0, SEEK_END);
                        long sz = ftell(rf);
                        fseek(rf, 0, SEEK_SET);
                        unsigned char *data = (unsigned char *)malloc(sz > 0 ? (size_t)sz : 1);
                        if (sz > 0) {
                            if (fread(data, 1, (size_t)sz, rf) != (size_t)sz) sz = 0;
                        }
                        fclose(rf);
                        size_t dlen = 0;
                        unsigned char *dec = nv_gunzip(data, (size_t)sz, &dlen, line, col);
                        free(data);
                        FILE *wf = fopen(resolved, "wb");
                        if (wf) {
                            if (dlen > 0) fwrite(dec, 1, dlen, wf);
                            fclose(wf);
                        }
                        nbytes = dlen;
                        free(dec);
                    }
                } else {
                    nbytes = (unsigned long long)sink.written;
                }
            } else {
                raw = (unsigned char *)sink.buf;
                raw_len = sink.len;
                if (is_gzip) {
                    size_t dlen = 0;
                    unsigned char *dec = nv_gunzip(raw, raw_len, &dlen, line, col);
                    raw = dec;
                    raw_len = dlen;
                }
                nbytes = (unsigned long long)raw_len;
            }
            // بناء قاموس الرد.
            NvValue out = nv_dict_new();
            nv_dict_set_raw(out.u.dict, "رمز", nv_num((double)status));
            if (resolved) {
                nv_dict_set_raw(out.u.dict, "جسم", nv_str_new("", 0));
            } else {
                size_t blen = 0;
                char *bstr = NULL;
                if (strcmp(encoding, "latin1") == 0) {
                    bstr = nv_latin1_to_utf8(raw, raw_len, &blen);
                } else {
                    bstr = nv_utf8_lossy(raw, raw_len, &blen);
                }
                nv_dict_set_raw(out.u.dict, "جسم", nv_str_new(bstr, blen));
                free(bstr);
                if (is_gzip) free(raw);
            }
            NvValue hdrs = nv_dict_new();
            for (int i = 0; i < sink.meta_n; i++) {
                nv_dict_set_raw(hdrs.u.dict, sink.meta_names[i], nv_str_cstr(sink.meta_values[i]));
            }
            nv_dict_set_raw(out.u.dict, "رؤوس", hdrs);
            nv_dict_set_raw(out.u.dict, "بايتات", nv_num((double)nbytes));
            result = out;
            have_result = 1;
            free(try_hdrs.names);
            free(try_hdrs.values);
            done = 1;
        }
        if (show_progress && have_result) {
            int bi2 = nv_dict_find(result.u.dict, "بايتات");
            double nb = bi2 >= 0 ? result.u.dict->entries[bi2].val.u.num : 0;
            int si = nv_dict_find(result.u.dict, "رمز");
            double st = si >= 0 ? result.u.dict->entries[si].val.u.num : 0;
            fprintf(stderr, "اكتمل %s — رمز %d (%d بايت)\n", url_owned, (int)st, (int)nb);
        }
        if (trace_on && trace_resolved) {
            FILE *tf = fopen(trace_resolved, "a");
            if (tf) {
                int si = nv_dict_find(result.u.dict, "رمز");
                int bi2 = nv_dict_find(result.u.dict, "بايتات");
                double st = si >= 0 ? result.u.dict->entries[si].val.u.num : 0;
                double nb = bi2 >= 0 ? result.u.dict->entries[bi2].val.u.num : 0;
                fprintf(tf, "%s %s -> %d (%d بايت)\n", method_up, url_owned, (int)st, (int)nb);
                fclose(tf);
            }
        }
        if (opts.preserve_mtime && resolved) {
            const char *lm = nv_meta_find(&sink, "last-modified");
            if (lm) {
                long long t = nv_parse_http_date(lm);
                if (t >= 0) {
                    struct utimbuf tb;
                    tb.actime = (time_t)t;
                    tb.modtime = (time_t)t;
                    utime(resolved, &tb);
                }
            }
        }
        // تحرير مصرف المحاولة الأخيرة.
        free(sink.buf);
        nv_sink_free_meta(&sink);
    } else {
        nv_restore_checkpoint(cp0);
        nv_handlers = hp0;
        if (resolved && opts.delete_on_fail) {
            remove(resolved);
        }
        nv_reraise_current();
    }
    nv_handlers = hp0;
    (void)have_result;
    return result;
}

NvValue nv_net_req_new(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.طلب_جديد تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.طلب_جديد تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *url = args[0].u.str->data;
    if (url[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.طلب_جديد تتوقع رابطا غير فارغ", line, col);
    }
    NvValue out = nv_dict_new();
    nv_dict_set_raw(out.u.dict, "رابط", nv_str_cstr(url));
    nv_dict_set_raw(out.u.dict, "طريقة", nv_str_cstr("GET"));
    return out;
}

NvValue nv_net_req_show(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اعرض_الطلب تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اعرض_الطلب تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    NvDict *d = args[0].u.dict;
    const char *url = "";
    const char *method = "GET";
    int n_headers = 0;
    int n_opts = 0;
    static const char *STRUCT6[] = {"رابط", "طريقة", "رؤوس", "كوكيز", "جسم", "نوع_المحتوى", NULL};
    for (int i = 0; i < d->len; i++) {
        const char *k = d->entries[i].key;
        NvValue *v = &d->entries[i].val;
        if (strcmp(k, "رابط") == 0 && v->tag == NV_STR) url = v->u.str->data;
        if (strcmp(k, "طريقة") == 0 && v->tag == NV_STR) method = v->u.str->data;
        if (strcmp(k, "رؤوس") == 0 && v->tag == NV_DICT) n_headers = v->u.dict->len;
        int structural = 0;
        for (int j = 0; STRUCT6[j]; j++) {
            if (strcmp(k, STRUCT6[j]) == 0) { structural = 1; break; }
        }
        if (!structural) n_opts++;
    }
    size_t cap = strlen(method) + strlen(url) + 64;
    char *out = (char *)malloc(cap);
    int n = snprintf(out, cap, "%s %s", method, url);
    if (n_headers > 0) n += snprintf(out + n, cap - (size_t)n, "\nرؤوس: %d", n_headers);
    if (n_opts > 0) n += snprintf(out + n, cap - (size_t)n, "\nخيارات: %d", n_opts);
    NvValue r = nv_str_new(out, (size_t)n);
    free(out);
    return r;
}


// ---------- لينكس8-طبقة2: أغلفة فورية وتنزيل (اجتماع 90 §105) ----------
typedef struct { unsigned char *data; size_t len; } Nv2Bytes;

static void nv2_buf_add(char **buf, size_t *len, size_t *cap, const void *data, size_t n) {
    if (*len + n + 1 > *cap) {
        while (*len + n + 1 > *cap) *cap = *cap ? *cap * 2 : 256;
        *buf = (char *)realloc(*buf, *cap);
    }
    memcpy(*buf + *len, data, n);
    *len += n;
    (*buf)[*len] = '\0';
}

static void nv2_buf_adds(char **buf, size_t *len, size_t *cap, const char *s) {
    nv2_buf_add(buf, len, cap, s, strlen(s));
}

static void nv2_json_write(NvValue v, char **buf, size_t *len, size_t *cap, int depth, int line, int col) {
    if (depth > 100) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.أرسل_json قيمة لا تقبل كتابة JSON", line, col);
    }
    char tmp[64];
    switch (v.tag) {
    case NV_NIL:
        nv2_buf_adds(buf, len, cap, "null");
        return;
    case NV_BOOL:
        nv2_buf_adds(buf, len, cap, v.u.boolean ? "true" : "false");
        return;
    case NV_NUM: {
        double n = v.u.num;
        if (!isfinite(n)) {
            nv_error(NV_ERR_TYPE, "الدالة شبكة.أرسل_json قيمة لا تقبل كتابة JSON", line, col);
        }
        if (n == trunc(n) && fabs(n) < 9e15) {
            snprintf(tmp, sizeof(tmp), "%lld", (long long)n);
        } else {
            snprintf(tmp, sizeof(tmp), "%.15g", n);
        }
        nv2_buf_adds(buf, len, cap, tmp);
        return;
    }
    case NV_STR: {
        nv2_buf_adds(buf, len, cap, "\"");
        const unsigned char *s = (const unsigned char *)v.u.str->data;
        size_t n = v.u.str->len;
        for (size_t i = 0; i < n; i++) {
            unsigned char c = s[i];
            switch (c) {
            case '"': nv2_buf_adds(buf, len, cap, "\\\""); break;
            case '\\': nv2_buf_adds(buf, len, cap, "\\\\"); break;
            case '\n': nv2_buf_adds(buf, len, cap, "\\n"); break;
            case '\r': nv2_buf_adds(buf, len, cap, "\\r"); break;
            case '\t': nv2_buf_adds(buf, len, cap, "\\t"); break;
            default:
                if (c < 0x20) {
                    snprintf(tmp, sizeof(tmp), "\\u%04x", c);
                    nv2_buf_adds(buf, len, cap, tmp);
                } else {
                    nv2_buf_add(buf, len, cap, &c, 1);
                }
                break;
            }
        }
        nv2_buf_adds(buf, len, cap, "\"");
        return;
    }
    case NV_LIST: {
        NvList *l = v.u.list;
        nv2_buf_adds(buf, len, cap, "[");
        for (int i = 0; i < l->len; i++) {
            if (i > 0) nv2_buf_adds(buf, len, cap, ",");
            nv2_json_write(l->items[i], buf, len, cap, depth + 1, line, col);
        }
        nv2_buf_adds(buf, len, cap, "]");
        return;
    }
    case NV_DICT: {
        NvDict *d = v.u.dict;
        nv2_buf_adds(buf, len, cap, "{");
        for (int i = 0; i < d->len; i++) {
            if (i > 0) nv2_buf_adds(buf, len, cap, ",");
            NvValue kv = nv_str_cstr(d->entries[i].key);
            nv2_json_write(kv, buf, len, cap, depth + 1, line, col);
            nv2_buf_adds(buf, len, cap, ":");
            nv2_json_write(d->entries[i].val, buf, len, cap, depth + 1, line, col);
        }
        nv2_buf_adds(buf, len, cap, "}");
        return;
    }
    default:
        nv_error(NV_ERR_TYPE, "الدالة شبكة.أرسل_json قيمة لا تقبل كتابة JSON", line, col);
        return;
    }
}

static NvValue nv2_call_exec(NvValue req, int line, int col) {
    NvValue a[1];
    a[0] = req;
    return nv_net_execute(a, 1, line, col);
}

static void nv2_need_args(int nargs, int min, int max, const char *name, int line, int col) {
    if (nargs < min || nargs > max) {
        if (min == max) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع %d معاملات لكن أعطيت %d", name, min, nargs);
        } else {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع %d أو %d معاملات لكن أعطيت %d", name, min, max, nargs);
        }
    }
}

static const char *nv2_url_arg(NvValue *args, int i, const char *name, int line, int col) {
    if (args[i].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع رابطا نصا لكن أعطيت %s", name, nv_type_name(args[i]));
    }
    if (args[i].u.str->data[0] == '\0') {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع رابطا غير فارغ", name);
    }
    return args[i].u.str->data;
}

static const char *nv2_path_arg(NvValue *args, int i, const char *name, int line, int col) {
    if (args[i].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع مسارا نصا لكن أعطيت %s", name, nv_type_name(args[i]));
    }
    if (args[i].u.str->data[0] == '\0') {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع مسارا غير فارغ", name);
    }
    return args[i].u.str->data;
}

static Nv2Bytes nv2_body_arg(NvValue v, const char *name, int line, int col) {
    Nv2Bytes out;
    out.data = NULL;
    out.len = 0;
    if (v.tag == NV_STR) {
        out.len = v.u.str->len;
        out.data = (unsigned char *)malloc(out.len ? out.len : 1);
        memcpy(out.data, v.u.str->data, out.len);
        return out;
    }
    if (v.tag == NV_LIST) {
        NvList *l = v.u.list;
        out.data = (unsigned char *)malloc(l->len ? (size_t)l->len : 1);
        for (int i = 0; i < l->len; i++) {
            NvValue it = l->items[i];
            if (it.tag != NV_NUM || it.u.num != trunc(it.u.num) || it.u.num < 0 || it.u.num > 255) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s جسم غير صالح — القائمة بايتات 0 إلى 255", name);
            }
            out.data[out.len++] = (unsigned char)it.u.num;
        }
        return out;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع جسما نصا أو قائمة بايتات لكن أعطيت %s", name, nv_type_name(v));
    return out;
}

static NvValue nv2_req_new2(const char *url, const char *method) {
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "رابط", nv_str_cstr(url));
    nv_dict_set_raw(d.u.dict, "طريقة", nv_str_cstr(method));
    return d;
}

static void nv2_req_set_str(NvValue d, const char *key, const char *val) {
    nv_dict_set_raw(d.u.dict, key, nv_str_cstr(val));
}

static void nv2_req_set_bytes(NvValue d, const char *key, const unsigned char *data, size_t len) {
    NvValue l = nv_list_new();
    for (size_t i = 0; i < len; i++) nv_list_push_raw(l.u.list, nv_num((double)data[i]));
    nv_dict_set_raw(d.u.dict, key, l);
}

static void nv2_req_merge_opts(NvValue d, NvValue opts, const char *name, int line, int col) {
    if (opts.tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع خيارات قاموسا لكن أعطيت %s", name, nv_type_name(opts));
    }
    NvDict *od = opts.u.dict;
    for (int i = 0; i < od->len; i++) {
        nv_dict_set_raw(d.u.dict, od->entries[i].key, nv_clone_value(od->entries[i].val));
    }
}

static Nv2Bytes nv2_read_file(const char *path, const char *name, int line, int col) {
    Nv2Bytes out;
    out.data = NULL;
    out.len = 0;
    char *resolved = nv_resolve_path(path, line, col);
    FILE *f = fopen(resolved, "rb");
    if (!f) {
        free(resolved);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s الملف غير موجود", name);
    }
    size_t cap = 8192;
    out.data = (unsigned char *)malloc(cap);
    size_t n;
    out.len = 0;
    while (1) {
        if (out.len == cap) {
            cap *= 2;
            out.data = (unsigned char *)realloc(out.data, cap);
        }
        n = fread(out.data + out.len, 1, cap - out.len, f);
        if (n == 0) break;
        out.len += n;
    }
    fclose(f);
    free(resolved);
    return out;
}

static void nv2_write_file(const char *path, const unsigned char *data, size_t len, const char *name, int line, int col) {
    char *resolved = nv_resolve_path(path, line, col);
    FILE *f = fopen(resolved, "wb");
    if (!f) {
        free(resolved);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تعذر كتابة الملف", name);
    }
    if (len > 0 && fwrite(data, 1, len, f) != len) {
        fclose(f);
        free(resolved);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تعذر كتابة الملف", name);
    }
    fclose(f);
    free(resolved);
}

static Nv2Bytes nv2_read_stdin(int line, int col) {
    Nv2Bytes out;
    out.data = NULL;
    out.len = 0;
    size_t cap = 8192;
    out.data = (unsigned char *)malloc(cap + 1);
    size_t n;
    while (1) {
        if (out.len == cap) {
            cap *= 2;
            out.data = (unsigned char *)realloc(out.data, cap + 1);
        }
        n = fread(out.data + out.len, 1, cap - out.len, stdin);
        if (n == 0) break;
        out.len += n;
        if (out.len > 1000000) {
            free(out.data);
            nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
        }
    }
    out.data[out.len] = '\0';
    return out;
}

static int nv2_hex_val(int c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static char *nv2_url_encode(const char *s) {
    size_t cap = strlen(s) * 3 + 1, len = 0;
    char *out = (char *)malloc(cap);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        unsigned char b = *p;
        if ((b >= 'A' && b <= 'Z') || (b >= 'a' && b <= 'z') || (b >= '0' && b <= '9') || b == '-' || b == '_' || b == '.' || b == '~') {
            out[len++] = (char)b;
        } else if (b == ' ') {
            out[len++] = '+';
        } else {
            len += (size_t)snprintf(out + len, cap - len, "%%%02X", b);
        }
    }
    out[len] = '\0';
    return out;
}

static char *nv2_percent_decode(const char *s) {
    size_t cap = strlen(s) + 1, len = 0;
    char *out = (char *)malloc(cap);
    for (size_t i = 0; s[i]; i++) {
        if (s[i] == '%' && s[i + 1] && s[i + 2] && nv2_hex_val((unsigned char)s[i + 1]) >= 0 && nv2_hex_val((unsigned char)s[i + 2]) >= 0) {
            out[len++] = (char)(nv2_hex_val((unsigned char)s[i + 1]) * 16 + nv2_hex_val((unsigned char)s[i + 2]));
            i += 2;
        } else {
            out[len++] = s[i];
        }
    }
    out[len] = '\0';
    return out;
}

/* قيمة حقل نموذج/رفع: نص كما هو، عدد بصيغة JSON، منطقي true/false */
static char *nv2_form_value(NvValue v, const char *name, int line, int col) {
    if (v.tag == NV_STR) return strdup(v.u.str->data);
    if (v.tag == NV_NUM) {
        char tmp[64];
        double n = v.u.num;
        if (n == trunc(n) && fabs(n) < 9e15) snprintf(tmp, sizeof(tmp), "%lld", (long long)n);
        else snprintf(tmp, sizeof(tmp), "%.15g", n);
        return strdup(tmp);
    }
    if (v.tag == NV_BOOL) return strdup(v.u.boolean ? "true" : "false");
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s قيمة حقل غير صالحة", name);
    return NULL;
}

typedef struct { char **keys; char **vals; int n; int cap; } Nv2Fields;

static void nv2_fields_push(Nv2Fields *f, const char *k, const char *v) {
    if (f->n >= f->cap) {
        f->cap = f->cap ? f->cap * 2 : 8;
        f->keys = (char **)realloc(f->keys, sizeof(char *) * (size_t)f->cap);
        f->vals = (char **)realloc(f->vals, sizeof(char *) * (size_t)f->cap);
    }
    f->keys[f->n] = strdup(k);
    f->vals[f->n] = strdup(v);
    f->n++;
}

static Nv2Fields nv2_form_fields(NvValue v, const char *name, int line, int col) {
    Nv2Fields f;
    memset(&f, 0, sizeof(f));
    if (v.tag == NV_DICT) {
        NvDict *d = v.u.dict;
        for (int i = 0; i < d->len; i++) {
            char *val = nv2_form_value(d->entries[i].val, name, line, col);
            nv2_fields_push(&f, d->entries[i].key, val);
            free(val);
        }
        return f;
    }
    if (v.tag == NV_LIST) {
        NvList *l = v.u.list;
        for (int i = 0; i < l->len; i++) {
            NvValue it = l->items[i];
            if (it.tag != NV_LIST || it.u.list->len != 2) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s زوج غير صالح — الزوج [اسم، قيمة]", name);
            }
            NvValue k = it.u.list->items[0];
            if (k.tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع اسم حقل نصا لكن أعطيت %s", name, nv_type_name(k));
            }
            char *val = nv2_form_value(it.u.list->items[1], name, line, col);
            nv2_fields_push(&f, k.u.str->data, val);
            free(val);
        }
        return f;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع حقولا قاموسا أو قائمة أزواج لكن أعطيت %s", name, nv_type_name(v));
    return f;
}

static Nv2Bytes nv2_multipart(Nv2Fields *fields, const char *file_name, const unsigned char *file_bytes, size_t file_len, char **ct_out) {
    const char *b = "----niva-multipart-8----";
    char *buf = NULL;
    size_t len = 0, cap = 0;
    char tmp[512];
    for (int i = 0; i < fields->n; i++) {
        snprintf(tmp, sizeof(tmp), "--%s\r\nContent-Disposition: form-data; name=\"%s\"\r\n\r\n%s\r\n", b, fields->keys[i], fields->vals[i]);
        nv2_buf_adds(&buf, &len, &cap, tmp);
    }
    snprintf(tmp, sizeof(tmp), "--%s\r\nContent-Disposition: form-data; name=\"file\"; filename=\"%s\"\r\nContent-Type: application/octet-stream\r\n\r\n", b, file_name);
    nv2_buf_adds(&buf, &len, &cap, tmp);
    nv2_buf_add(&buf, &len, &cap, file_bytes, file_len);
    snprintf(tmp, sizeof(tmp), "\r\n--%s--\r\n", b);
    nv2_buf_adds(&buf, &len, &cap, tmp);
    *ct_out = (char *)malloc(strlen(b) + 64);
    snprintf(*ct_out, strlen(b) + 64, "multipart/form-data; boundary=%s", b);
    Nv2Bytes out;
    out.data = (unsigned char *)buf;
    out.len = len;
    return out;
}

static char *nv2_url_basename(const char *url) {
    const char *end = url;
    for (const char *p = url; *p; p++) {
        if (*p == '?' || *p == '#') { end = p; break; }
        end = p + 1;
    }
    const char *slash = NULL;
    for (const char *p = url; p < end; p++) {
        if (*p == '/') slash = p;
    }
    const char *base = slash ? slash + 1 : url;
    size_t n = (size_t)(end - base);
    char *raw = (char *)malloc(n + 1);
    memcpy(raw, base, n);
    raw[n] = '\0';
    if (n == 0) {
        free(raw);
        return strdup("تنزيل");
    }
    char *dec = nv2_percent_decode(raw);
    free(raw);
    return dec;
}

typedef struct { double status; unsigned char *body; size_t body_len; char **hnames; char **hvals; int nhdr; double nbytes; } Nv2Resp;

static Nv2Resp nv2_resp_parts(NvValue resp) {
    Nv2Resp r;
    memset(&r, 0, sizeof(r));
    if (resp.tag != NV_DICT) return r;
    NvDict *d = resp.u.dict;
    int i = nv_dict_find(d, "رمز");
    if (i >= 0 && d->entries[i].val.tag == NV_NUM) r.status = d->entries[i].val.u.num;
    i = nv_dict_find(d, "جسم");
    if (i >= 0 && d->entries[i].val.tag == NV_STR) {
        r.body_len = d->entries[i].val.u.str->len;
        r.body = (unsigned char *)malloc(r.body_len ? r.body_len : 1);
        memcpy(r.body, d->entries[i].val.u.str->data, r.body_len);
    }
    i = nv_dict_find(d, "رؤوس");
    if (i >= 0 && d->entries[i].val.tag == NV_DICT) {
        NvDict *h = d->entries[i].val.u.dict;
        r.nhdr = h->len;
        r.hnames = (char **)malloc(sizeof(char *) * (size_t)(r.nhdr ? r.nhdr : 1));
        r.hvals = (char **)malloc(sizeof(char *) * (size_t)(r.nhdr ? r.nhdr : 1));
        for (int j = 0; j < h->len; j++) {
            r.hnames[j] = strdup(h->entries[j].key);
            r.hvals[j] = (h->entries[j].val.tag == NV_STR) ? strdup(h->entries[j].val.u.str->data) : strdup("");
        }
    }
    i = nv_dict_find(d, "بايتات");
    if (i >= 0 && d->entries[i].val.tag == NV_NUM) r.nbytes = d->entries[i].val.u.num;
    return r;
}

static const char *nv2_resp_header(Nv2Resp *r, const char *name) {
    for (int i = 0; i < r->nhdr; i++) {
        if (strcmp(r->hnames[i], name) == 0) return r->hvals[i];
    }
    return NULL;
}

static NvValue nv2_resp_rebuild(Nv2Resp *r, const unsigned char *bytes, size_t len) {
    size_t olen = 0;
    char *body = nv_utf8_lossy(bytes, len, &olen);
    NvValue out = nv_dict_new();
    nv_dict_set_raw(out.u.dict, "رمز", nv_num(r->status));
    nv_dict_set_raw(out.u.dict, "جسم", nv_str_new(body ? body : "", olen));
    NvValue hdrs = nv_dict_new();
    for (int i = 0; i < r->nhdr; i++) {
        nv_dict_set_raw(hdrs.u.dict, r->hnames[i], nv_str_cstr(r->hvals[i]));
    }
    nv_dict_set_raw(out.u.dict, "رؤوس", hdrs);
    nv_dict_set_raw(out.u.dict, "بايتات", nv_num(r->nbytes));
    free(body);
    return out;
}

/* عكس ترميز latin1: فك UTF-8 إلى نقاط شيفرة ثم بايتاتها الدنيا */
static Nv2Bytes nv2_latin1_back(const unsigned char *s, size_t len) {
    Nv2Bytes out;
    out.data = (unsigned char *)malloc(len ? len : 1);
    out.len = 0;
    size_t i = 0;
    while (i < len) {
        unsigned char c = s[i];
        unsigned int cp = c;
        size_t adv = 1;
        if ((c & 0xE0) == 0xC0 && i + 1 < len && (s[i + 1] & 0xC0) == 0x80) {
            cp = ((c & 0x1F) << 6) | (s[i + 1] & 0x3F);
            adv = 2;
        } else if ((c & 0xF0) == 0xE0 && i + 2 < len && (s[i + 1] & 0xC0) == 0x80 && (s[i + 2] & 0xC0) == 0x80) {
            cp = ((c & 0x0F) << 12) | ((s[i + 1] & 0x3F) << 6) | (s[i + 2] & 0x3F);
            adv = 3;
        } else if ((c & 0xF8) == 0xF0 && i + 3 < len && (s[i + 1] & 0xC0) == 0x80 && (s[i + 2] & 0xC0) == 0x80 && (s[i + 3] & 0xC0) == 0x80) {
            cp = ((c & 0x07) << 18) | ((s[i + 1] & 0x3F) << 12) | ((s[i + 2] & 0x3F) << 6) | (s[i + 3] & 0x3F);
            adv = 4;
        }
        out.data[out.len++] = (unsigned char)(cp & 0xFF);
        i += adv;
    }
    return out;
}

static char *nv2_cd_filename(Nv2Resp *r) {
    const char *v = nv2_resp_header(r, "content-disposition");
    if (!v) return NULL;
    const char *p = strstr(v, "filename*=");
    if (p) {
        p += 10;
        while (*p == ' ') p++;
        if (strncmp(p, "UTF-8''", 7) == 0) p += 7;
        else if (strncmp(p, "utf-8''", 7) == 0) p += 7;
        const char *end = strchr(p, ';');
        size_t n = end ? (size_t)(end - p) : strlen(p);
        while (n > 0 && (p[n - 1] == '"' || p[n - 1] == ' ')) n--;
        char *raw = (char *)malloc(n + 1);
        memcpy(raw, p, n);
        raw[n] = '\0';
        char *dec = nv2_percent_decode(raw);
        free(raw);
        if (dec[0]) return dec;
        free(dec);
        return NULL;
    }
    p = strstr(v, "filename=");
    if (p) {
        p += 9;
        while (*p == ' ') p++;
        if (*p == '"') p++;
        const char *end = strchr(p, '"');
        size_t n = end ? (size_t)(end - p) : strlen(p);
        if (n == 0) return NULL;
        char *out = (char *)malloc(n + 1);
        memcpy(out, p, n);
        out[n] = '\0';
        return out;
    }
    return NULL;
}

static char *nv2_join_dir(const char *dir, const char *name) {
    if (!dir || !dir[0]) return strdup(name);
    size_t dl = strlen(dir);
    while (dl > 0 && dir[dl - 1] == '/') dl--;
    char *out = (char *)malloc(dl + strlen(name) + 2);
    memcpy(out, dir, dl);
    out[dl] = '/';
    strcpy(out + dl + 1, name);
    return out;
}

static char *nv2_resolve_link(const char *base, const char *link) {
    while (*link == ' ' || *link == '\t' || *link == '\n' || *link == '\r') link++;
    if (!*link) return NULL;
    if (*link == '#') return NULL;
    if (strncmp(link, "javascript:", 11) == 0) return NULL;
    if (strncmp(link, "mailto:", 7) == 0) return NULL;
    const char *hash = strchr(link, '#');
    size_t llen = hash ? (size_t)(hash - link) : strlen(link);
    while (llen > 0 && (link[llen - 1] == ' ' || link[llen - 1] == '\t')) llen--;
    if (llen == 0) return NULL;
    char *l = (char *)malloc(llen + 1);
    memcpy(l, link, llen);
    l[llen] = '\0';
    if (strncmp(l, "http://", 7) == 0 || strncmp(l, "https://", 8) == 0) return l;
    const char *scheme = strstr(base, "://");
    if (!scheme) {
        free(l);
        return NULL;
    }
    const char *after = scheme + 3;
    const char *slash = strchr(after, '/');
    size_t shlen = slash ? (size_t)(slash - base) : strlen(base);
    char *scheme_host = (char *)malloc(shlen + 1);
    memcpy(scheme_host, base, shlen);
    scheme_host[shlen] = '\0';
    char *out = NULL;
    if (l[0] == '/') {
        out = (char *)malloc(shlen + llen + 1);
        memcpy(out, scheme_host, shlen);
        memcpy(out + shlen, l, llen + 1);
    } else {
        const char *base_path = slash ? slash : "/";
        const char *last = strrchr(base_path, '/');
        size_t dirlen = last ? (size_t)(last - base_path) + 1 : 0;
        out = (char *)malloc(shlen + dirlen + llen + 1);
        memcpy(out, scheme_host, shlen);
        memcpy(out + shlen, base_path, dirlen);
        memcpy(out + shlen + dirlen, l, llen + 1);
    }
    free(scheme_host);
    free(l);
    return out;
}

static int nv2_str_ieq(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        int ca = a[i], cb = b[i];
        if (ca >= 'A' && ca <= 'Z') ca += 32;
        if (cb >= 'A' && cb <= 'Z') cb += 32;
        if (ca != cb) return 0;
    }
    return 1;
}

typedef struct { char **urls; int n; int cap; } Nv2Links;

static void nv2_links_push(Nv2Links *ls, const char *u) {
    for (int i = 0; i < ls->n; i++) {
        if (strcmp(ls->urls[i], u) == 0) return;
    }
    if (ls->n >= ls->cap) {
        ls->cap = ls->cap ? ls->cap * 2 : 8;
        ls->urls = (char **)realloc(ls->urls, sizeof(char *) * (size_t)ls->cap);
    }
    ls->urls[ls->n++] = strdup(u);
}

static Nv2Links nv2_extract_links(const char *html, const char *base) {
    Nv2Links ls;
    memset(&ls, 0, sizeof(ls));
    size_t hlen = strlen(html);
    size_t i = 0;
    while (i < hlen) {
        size_t best = (size_t)-1;
        int qlen = 0;
        for (size_t j = i; j + 5 < hlen + 1; j++) {
            if (j + 6 <= hlen && nv2_str_ieq(html + j, "href=\"", 6)) { best = j; qlen = 6; break; }
            if (j + 6 <= hlen && nv2_str_ieq(html + j, "href='", 6)) { best = j; qlen = 6; break; }
            if (j + 5 <= hlen && nv2_str_ieq(html + j, "src=\"", 5)) { best = j; qlen = 5; break; }
            if (j + 5 <= hlen && nv2_str_ieq(html + j, "src='", 5)) { best = j; qlen = 5; break; }
        }
        if (best == (size_t)-1) break;
        char q = html[best + (size_t)qlen - 1];
        size_t vstart = best + (size_t)qlen;
        const char *end = NULL;
        for (size_t j = vstart; j < hlen; j++) {
            if (html[j] == q) { end = html + j; break; }
        }
        if (!end) break;
        size_t vlen = (size_t)(end - (html + vstart));
        char *raw = (char *)malloc(vlen + 1);
        memcpy(raw, html + vstart, vlen);
        raw[vlen] = '\0';
        char *u = nv2_resolve_link(base, raw);
        free(raw);
        if (u) {
            nv2_links_push(&ls, u);
            free(u);
        }
        i = vstart + vlen + 1;
    }
    return ls;
}

static NvValue nv2_save_in(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "بحفظ_في", line, col);
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.بحفظ_في تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *path = nv2_path_arg(args, 1, "بحفظ_في", line, col);
    NvValue out = nv_clone_value(args[0]);
    nv_dict_set_raw(out.u.dict, "حفظ_في", nv_str_cstr(path));
    return out;
}

static NvValue nv2_one_json(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "أرسل_json", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_json", line, col);
    char *buf = NULL;
    size_t len = 0, cap = 0;
    nv2_json_write(args[1], &buf, &len, &cap, 0, line, col);
    NvValue req = nv2_req_new2(url, "POST");
    /* الجسم نص JSON: قيمة نصية بطول البايتات */
    nv_dict_set_raw(req.u.dict, "جسم", nv_str_new(buf ? buf : "", len));
    nv2_req_set_str(req, "نوع_المحتوى", "application/json; charset=utf-8");
    free(buf);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_one_form(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "أرسل_نموذج", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_نموذج", line, col);
    Nv2Fields fields = nv2_form_fields(args[1], "أرسل_نموذج", line, col);
    char *buf = NULL;
    size_t len = 0, cap = 0;
    for (int i = 0; i < fields.n; i++) {
        if (i > 0) nv2_buf_adds(&buf, &len, &cap, "&");
        char *k = nv2_url_encode(fields.keys[i]);
        char *v = nv2_url_encode(fields.vals[i]);
        nv2_buf_adds(&buf, &len, &cap, k);
        nv2_buf_adds(&buf, &len, &cap, "=");
        nv2_buf_adds(&buf, &len, &cap, v);
        free(k);
        free(v);
    }
    NvValue req = nv2_req_new2(url, "POST");
    nv_dict_set_raw(req.u.dict, "جسم", nv_str_new(buf ? buf : "", len));
    nv2_req_set_str(req, "نوع_المحتوى", "application/x-www-form-urlencoded");
    free(buf);
    return nv2_call_exec(req, line, col);
}

static const char *nv2_opt_ct(NvValue *args, int nargs, int i, const char *name, const char *def, int line, int col) {
    if (nargs > i) {
        if (args[i].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع نوعا نصا لكن أعطيت %s", name, nv_type_name(args[i]));
        }
        return args[i].u.str->data;
    }
    return def;
}

static NvValue nv2_one_raw(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 3, "أرسل_خام", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_خام", line, col);
    Nv2Bytes body = nv2_body_arg(args[1], "أرسل_خام", line, col);
    const char *ct = nv2_opt_ct(args, nargs, 2, "أرسل_خام", "application/octet-stream", line, col);
    NvValue req = nv2_req_new2(url, "POST");
    nv2_req_set_bytes(req, "جسم", body.data, body.len);
    nv2_req_set_str(req, "نوع_المحتوى", ct);
    free(body.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_one_binary(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "أرسل_ثنائي", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_ثنائي", line, col);
    Nv2Bytes body = nv2_body_arg(args[1], "أرسل_ثنائي", line, col);
    NvValue req = nv2_req_new2(url, "POST");
    nv2_req_set_bytes(req, "جسم", body.data, body.len);
    nv2_req_set_str(req, "نوع_المحتوى", "application/octet-stream");
    free(body.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_one_from_file(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 3, "أرسل_من_ملف", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_من_ملف", line, col);
    const char *path = nv2_path_arg(args, 1, "أرسل_من_ملف", line, col);
    Nv2Bytes bytes = nv2_read_file(path, "أرسل_من_ملف", line, col);
    const char *ct = nv2_opt_ct(args, nargs, 2, "أرسل_من_ملف", "application/octet-stream", line, col);
    NvValue req = nv2_req_new2(url, "POST");
    nv2_req_set_bytes(req, "جسم", bytes.data, bytes.len);
    nv2_req_set_str(req, "نوع_المحتوى", ct);
    free(bytes.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_one_from_stdin(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 1, "أرسل_من_المدخل", line, col);
    const char *url = nv2_url_arg(args, 0, "أرسل_من_المدخل", line, col);
    Nv2Bytes text = nv2_read_stdin(line, col);
    NvValue req = nv2_req_new2(url, "POST");
    nv_dict_set_raw(req.u.dict, "جسم", nv_str_new((char *)text.data, text.len));
    nv2_req_set_str(req, "نوع_المحتوى", "text/plain; charset=utf-8");
    free(text.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_method_body(NvValue *args, int nargs, const char *name, const char *method, int line, int col) {
    nv2_need_args(nargs, 2, 2, name, line, col);
    const char *url = nv2_url_arg(args, 0, name, line, col);
    Nv2Bytes body = nv2_body_arg(args[1], name, line, col);
    NvValue req = nv2_req_new2(url, method);
    nv2_req_set_bytes(req, "جسم", body.data, body.len);
    nv2_req_set_str(req, "نوع_المحتوى", "text/plain; charset=utf-8");
    free(body.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_m_put(NvValue *args, int nargs, int line, int col) {
    return nv2_method_body(args, nargs, "استبدل", "PUT", line, col);
}

static NvValue nv2_m_patch(NvValue *args, int nargs, int line, int col) {
    return nv2_method_body(args, nargs, "عدل", "PATCH", line, col);
}

static NvValue nv2_m_delete(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 1, "احذف_مورد", line, col);
    const char *url = nv2_url_arg(args, 0, "احذف_مورد", line, col);
    return nv2_call_exec(nv2_req_new2(url, "DELETE"), line, col);
}

static NvValue nv2_m_head(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 1, "افحص_رؤوس", line, col);
    const char *url = nv2_url_arg(args, 0, "افحص_رؤوس", line, col);
    NvValue resp = nv2_call_exec(nv2_req_new2(url, "HEAD"), line, col);
    Nv2Resp parts = nv2_resp_parts(resp);
    NvValue hdrs = nv_dict_new();
    for (int i = 0; i < parts.nhdr; i++) {
        nv_dict_set_raw(hdrs.u.dict, parts.hnames[i], nv_str_cstr(parts.hvals[i]));
    }
    return hdrs;
}

static NvValue nv2_m_options(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 1, "اطلب_خيارات", line, col);
    const char *url = nv2_url_arg(args, 0, "اطلب_خيارات", line, col);
    return nv2_call_exec(nv2_req_new2(url, "OPTIONS"), line, col);
}

static NvValue nv2_m_custom(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 3, "اطلب_مخصص", line, col);
    const char *url = nv2_url_arg(args, 0, "اطلب_مخصص", line, col);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اطلب_مخصص تتوقع طريقة نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    const char *method = args[1].u.str->data;
    if (method[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اطلب_مخصص طريقة غير صالحة", line, col);
    }
    for (const unsigned char *p = (const unsigned char *)method; *p; p++) {
        if (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r' || *p < 0x20 || *p == 0x7F) {
            nv_error(NV_ERR_TYPE, "الدالة شبكة.اطلب_مخصص طريقة غير صالحة", line, col);
        }
    }
    NvValue req = nv2_req_new2(url, method);
    if (nargs == 3) {
        Nv2Bytes body = nv2_body_arg(args[2], "اطلب_مخصص", line, col);
        nv2_req_set_bytes(req, "جسم", body.data, body.len);
        nv2_req_set_str(req, "نوع_المحتوى", "text/plain; charset=utf-8");
        free(body.data);
    }
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_upload_file(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 3, "ارفع_ملف", line, col);
    const char *url = nv2_url_arg(args, 0, "ارفع_ملف", line, col);
    const char *path = nv2_path_arg(args, 1, "ارفع_ملف", line, col);
    Nv2Bytes bytes = nv2_read_file(path, "ارفع_ملف", line, col);
    Nv2Fields fields;
    memset(&fields, 0, sizeof(fields));
    if (nargs == 3) fields = nv2_form_fields(args[2], "ارفع_ملف", line, col);
    const char *slash = strrchr(path, '/');
    const char *fname = slash ? slash + 1 : path;
    char *ct = NULL;
    Nv2Bytes body = nv2_multipart(&fields, fname, bytes.data, bytes.len, &ct);
    NvValue req = nv2_req_new2(url, "POST");
    nv2_req_set_bytes(req, "جسم", body.data, body.len);
    nv2_req_set_str(req, "نوع_المحتوى", ct);
    free(bytes.data);
    free(body.data);
    free(ct);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_upload_body_file(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "ارفع_جسم_ملف", line, col);
    const char *url = nv2_url_arg(args, 0, "ارفع_جسم_ملف", line, col);
    const char *path = nv2_path_arg(args, 1, "ارفع_جسم_ملف", line, col);
    Nv2Bytes bytes = nv2_read_file(path, "ارفع_جسم_ملف", line, col);
    NvValue req = nv2_req_new2(url, "POST");
    nv2_req_set_bytes(req, "جسم", bytes.data, bytes.len);
    nv2_req_set_str(req, "نوع_المحتوى", "application/octet-stream");
    free(bytes.data);
    return nv2_call_exec(req, line, col);
}

static NvValue nv2_dl_server_name(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 2, "نزل_باسم_الخادم", line, col);
    const char *url = nv2_url_arg(args, 0, "نزل_باسم_الخادم", line, col);
    const char *dir = "";
    if (nargs == 2) {
        if (args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نزل_باسم_الخادم تتوقع مجلدا نصا لكن أعطيت %s", nv_type_name(args[1]));
        }
        dir = args[1].u.str->data;
    }
    NvValue req = nv2_req_new2(url, "GET");
    nv2_req_set_str(req, "ترميز_الرد", "latin1");
    NvValue resp = nv2_call_exec(req, line, col);
    Nv2Resp parts = nv2_resp_parts(resp);
    Nv2Bytes bytes = nv2_latin1_back(parts.body, parts.body_len);
    char *fname = nv2_cd_filename(&parts);
    if (!fname) fname = nv2_url_basename(url);
    char *save = nv2_join_dir(dir, fname);
    nv2_write_file(save, bytes.data, bytes.len, "نزل_باسم_الخادم", line, col);
    free(fname);
    free(save);
    NvValue out = nv2_resp_rebuild(&parts, bytes.data, bytes.len);
    free(bytes.data);
    return out;
}

static const char *nv2_dir_arg(NvValue *args, int nargs, int i, const char *name, int line, int col) {
    if (i >= nargs) return "";
    if (args[i].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع مجلدا نصا لكن أعطيت %s", name, nv_type_name(args[i]));
    }
    return args[i].u.str->data;
}

static NvValue nv2_dl_list_inner(char **urls, int nurls, const char *dir, int has_opts, NvValue opts, const char *name, int line, int col) {
    NvValue out = nv_list_new();
    for (int i = 0; i < nurls; i++) {
        char *fname = nv2_url_basename(urls[i]);
        char *save = nv2_join_dir(dir, fname);
        NvValue req = nv2_req_new2(urls[i], "GET");
        nv2_req_set_str(req, "حفظ_في", save);
        if (has_opts) nv2_req_merge_opts(req, opts, name, line, col);
        NvValue resp = nv2_call_exec(req, line, col);
        nv_list_push_raw(out.u.list, resp);
        free(fname);
        free(save);
    }
    return out;
}

static char **nv2_url_list_arg(NvValue v, const char *name, int line, int col, int *out_n) {
    if (v.tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع قائمة روابط لكن أعطيت %s", name, nv_type_name(v));
    }
    NvList *l = v.u.list;
    char **out = (char **)malloc(sizeof(char *) * (size_t)(l->len ? l->len : 1));
    for (int i = 0; i < l->len; i++) {
        if (l->items[i].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع روابط نصية لكن أعطيت %s", name, nv_type_name(l->items[i]));
        }
        out[i] = strdup(l->items[i].u.str->data);
    }
    *out_n = l->len;
    return out;
}

static NvValue nv2_dl_all(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 3, "نزل_الكل", line, col);
    int nurls = 0;
    char **urls = nv2_url_list_arg(args[0], "نزل_الكل", line, col, &nurls);
    const char *dir = nv2_dir_arg(args, nargs, 1, "نزل_الكل", line, col);
    int has_opts = nargs >= 3;
    NvValue out = nv2_dl_list_inner(urls, nurls, dir, has_opts, has_opts ? args[2] : nv_nil(), "نزل_الكل", line, col);
    return out;
}

static char **nv2_split_lines(const char *text, size_t len, int *out_n) {
    char **out = NULL;
    int n = 0, cap = 0;
    size_t i = 0;
    while (i < len) {
        size_t j = i;
        while (j < len && text[j] != '\n') j++;
        size_t a = i, b = j;
        while (a < b && (text[a] == ' ' || text[a] == '\t' || text[a] == '\r')) a++;
        while (b > a && (text[b - 1] == ' ' || text[b - 1] == '\t' || text[b - 1] == '\r')) b--;
        if (b > a) {
            if (n >= cap) {
                cap = cap ? cap * 2 : 8;
                out = (char **)realloc(out, sizeof(char *) * (size_t)cap);
            }
            char *line = (char *)malloc(b - a + 1);
            memcpy(line, text + a, b - a);
            line[b - a] = '\0';
            out[n++] = line;
        }
        i = (j < len) ? j + 1 : j;
        if (j >= len) break;
    }
    *out_n = n;
    return out;
}

static NvValue nv2_dl_from_list(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 1, 3, "نزل_من_قائمة", line, col);
    const char *path = nv2_path_arg(args, 0, "نزل_من_قائمة", line, col);
    Nv2Bytes bytes = nv2_read_file(path, "نزل_من_قائمة", line, col);
    int nurls = 0;
    char **urls = nv2_split_lines((char *)bytes.data, bytes.len, &nurls);
    const char *dir = nv2_dir_arg(args, nargs, 1, "نزل_من_قائمة", line, col);
    int has_opts = nargs >= 3;
    NvValue out = nv2_dl_list_inner(urls, nurls, dir, has_opts, has_opts ? args[2] : nv_nil(), "نزل_من_قائمة", line, col);
    free(bytes.data);
    return out;
}

static NvValue nv2_dl_from_stdin(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 0, 2, "نزل_من_المدخل", line, col);
    Nv2Bytes text = nv2_read_stdin(line, col);
    int nurls = 0;
    char **urls = nv2_split_lines((char *)text.data, text.len, &nurls);
    const char *dir = nv2_dir_arg(args, nargs, 0, "نزل_من_المدخل", line, col);
    int has_opts = nargs >= 2;
    NvValue out = nv2_dl_list_inner(urls, nurls, dir, has_opts, has_opts ? args[1] : nv_nil(), "نزل_من_المدخل", line, col);
    free(text.data);
    return out;
}

static NvValue nv2_dl_resume(NvValue *args, int nargs, int line, int col) {
    nv2_need_args(nargs, 2, 2, "أكمل_التنزيل", line, col);
    const char *url = nv2_url_arg(args, 0, "أكمل_التنزيل", line, col);
    const char *path = nv2_path_arg(args, 1, "أكمل_التنزيل", line, col);
    NvValue req = nv2_req_new2(url, "GET");
    nv2_req_set_str(req, "حفظ_في", path);
    nv_dict_set_raw(req.u.dict, "استئناف", nv_bool(1));
    return nv2_call_exec(req, line, col);
}

static void nv2_dl_site_walk(const char *url, const char *dir, int has_opts, NvValue opts, long long depth, long long max_files, char ***seen, int *nseen, int *seen_cap, NvValue out, const char *name, int line, int col) {
    for (int i = 0; i < *nseen; i++) {
        if (strcmp((*seen)[i], url) == 0) return;
    }
    if (max_files > 0 && out.u.list->len >= max_files) return;
    if (*nseen >= *seen_cap) {
        *seen_cap = *seen_cap ? *seen_cap * 2 : 8;
        *seen = (char **)realloc(*seen, sizeof(char *) * (size_t)*seen_cap);
    }
    (*seen)[(*nseen)++] = strdup(url);
    NvValue req = nv2_req_new2(url, "GET");
    nv2_req_set_str(req, "ترميز_الرد", "latin1");
    if (has_opts) {
        nv2_req_merge_opts(req, opts, name, line, col);
        nv2_req_set_str(req, "ترميز_الرد", "latin1");
    }
    NvValue resp = nv2_call_exec(req, line, col);
    Nv2Resp parts = nv2_resp_parts(resp);
    Nv2Bytes bytes = nv2_latin1_back(parts.body, parts.body_len);
    char *fname = nv2_url_basename(url);
    char *save = nv2_join_dir(dir, fname);
    nv2_write_file(save, bytes.data, bytes.len, name, line, col);
    free(fname);
    free(save);
    nv_list_push_raw(out.u.list, nv2_resp_rebuild(&parts, bytes.data, bytes.len));
    if (depth > 0) {
        size_t olen = 0;
        char *text = nv_utf8_lossy(bytes.data, bytes.len, &olen);
        Nv2Links links = nv2_extract_links(text ? text : "", url);
        free(text);
        for (int i = 0; i < links.n; i++) {
            nv2_dl_site_walk(links.urls[i], dir, has_opts, opts, depth - 1, max_files, seen, nseen, seen_cap, out, name, line, col);
        }
    }
    free(bytes.data);
}

static NvValue nv2_dl_site(NvValue *args, int nargs, int line, int col) {
    /* الاسم الثاني انسخ_الموقع يستدعي الدالة نفسها؛ ورسائل
       الخطأ تحمل نزل_الموقع في الحالين (الدالة لا تعرف اسمها). */
    nv2_need_args(nargs, 1, 3, "نزل_الموقع", line, col);
    const char *url = nv2_url_arg(args, 0, "نزل_الموقع", line, col);
    const char *dir = nv2_dir_arg(args, nargs, 1, "نزل_الموقع", line, col);
    int has_opts = nargs >= 3;
    NvValue opts = has_opts ? args[2] : nv_nil();
    long long depth = 1, max_files = 0;
    if (has_opts && opts.tag == NV_DICT) {
        NvDict *od = opts.u.dict;
        int i = nv_dict_find(od, "عمق_التكرار");
        if (i >= 0 && od->entries[i].val.tag == NV_NUM) depth = (long long)od->entries[i].val.u.num;
        i = nv_dict_find(od, "حد_الملفات");
        if (i >= 0 && od->entries[i].val.tag == NV_NUM) max_files = (long long)od->entries[i].val.u.num;
    }
    char **seen = NULL;
    int nseen = 0, seen_cap = 0;
    NvValue out = nv_list_new();
    nv2_dl_site_walk(url, dir, has_opts, opts, depth, max_files, &seen, &nseen, &seen_cap, out, "نزل_الموقع", line, col);
    return out;
}


// ---------- لينكس8-طبقة3: FTP/FTPS/MQTT (اجتماع 90 §105) ----------
typedef struct { char *host; int port; char *path; char *user; char *pass; } Nv3HostUrl;

static char *nv3_strndup(const char *s, size_t n) {
    char *out = (char *)malloc(n + 1);
    memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

static Nv3HostUrl nv3_parse_host_url(const char *url, const char *scheme, int default_port, const char *display, int line, int col) {
    Nv3HostUrl out;
    memset(&out, 0, sizeof(out));
    size_t slen = strlen(scheme);
    const char *sep = strstr(url, "://");
    if (!sep || (size_t)(sep - url) != slen || strncmp(url, scheme, slen) != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع رابط %s", display, scheme);
    }
    const char *rest = sep + 3;
    const char *slash = strchr(rest, '/');
    size_t alen = slash ? (size_t)(slash - rest) : strlen(rest);
    char *authority = nv3_strndup(rest, alen);
    out.path = slash ? strdup(slash) : strdup("/");
    char *hostport = authority;
    char *at = strrchr(authority, '@');
    if (at) {
        *at = '\0';
        char *colon = strchr(authority, ':');
        if (colon) {
            *colon = '\0';
            out.user = strdup(authority);
            out.pass = strdup(colon + 1);
        } else {
            out.user = strdup(authority);
        }
        hostport = at + 1;
    }
    if (hostport[0] == '[') {
        char *close = strchr(hostport, ']');
        if (!close) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع مضيفا صحيحا", display);
        }
        out.host = nv3_strndup(hostport + 1, (size_t)(close - (hostport + 1)));
        if (close[1] == ':') {
            if (close[2]) {
                char *end = NULL;
                long p = strtol(close + 2, &end, 10);
                if (!end || *end || p < 0 || p > 65535) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع منفذا صحيحا", display);
                }
                out.port = (int)p;
            } else {
                out.port = default_port;
            }
        } else {
            out.port = default_port;
        }
    } else {
        char *colon = strrchr(hostport, ':');
        if (colon) {
            if (colon[1]) {
                char *end = NULL;
                long p = strtol(colon + 1, &end, 10);
                if (!end || *end || p < 0 || p > 65535) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع منفذا صحيحا", display);
                }
                out.port = (int)p;
            } else {
                out.port = default_port;
            }
            out.host = nv3_strndup(hostport, (size_t)(colon - hostport));
        } else {
            out.host = strdup(hostport);
            out.port = default_port;
        }
    }
    if (!out.host || !out.host[0]) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع مضيفا غير فارغ", display);
    }
    free(authority);
    return out;
}

static void nv3_require_network(int line, int col) {
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
}

static void nv3_ftp_send(int fd, const unsigned char *data, size_t len, long long deadline_ms, int line, int col) {
    size_t sent = 0;
    while (sent < len) {
        nv_sock_arm_timeout(fd, deadline_ms);
        ssize_t n = send(fd, data + sent, len - sent, 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                nv_error(NV_ERR_RUNTIME, "انتهت مهلة الإرسال إلى الخادم", line, col);
            }
            nv_error(NV_ERR_RUNTIME, "تعذر إرسال الطلب إلى الخادم", line, col);
        }
        if (n == 0) {
            nv_error(NV_ERR_RUNTIME, "تعذر إرسال الطلب إلى الخادم", line, col);
        }
        sent += (size_t)n;
    }
}

static void nv3_send_quiet(int fd, const void *data, size_t len) {
    (void)send(fd, data, len, 0);
}

static int nv3_ftp_read_line(int fd, long long deadline_ms, char *out, size_t cap, int line, int col) {
    size_t len = 0;
    while (1) {
        if (nv_net_now_ms() >= deadline_ms) {
            nv_error(NV_ERR_RUNTIME, "انتهت المهلة الإجمالية (30 ثانية)", line, col);
        }
        nv_sock_arm_timeout(fd, deadline_ms);
        char c;
        ssize_t n = recv(fd, &c, 1, 0);
        if (n == 0) {
            nv_error(NV_ERR_RUNTIME, "انقطع الاتصال بالخادم", line, col);
        }
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                nv_error(NV_ERR_RUNTIME, "انتهت مهلة القراءة من الخادم", line, col);
            }
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
        }
        if (c == '\n') break;
        if (len + 1 >= cap) {
            nv_error(NV_ERR_RUNTIME, "رد طويل جدا من الخادم", line, col);
        }
        out[len++] = c;
    }
    if (len > 0 && out[len - 1] == '\r') len--;
    out[len] = '\0';
    return (int)len;
}

static int nv3_ftp_read_reply(int fd, long long deadline_ms, char *text_out, size_t text_cap, int line, int col) {
    char first[4096];
    nv3_ftp_read_line(fd, deadline_ms, first, sizeof(first), line, col);
    if (strlen(first) < 3 || first[0] < '0' || first[0] > '9' || first[1] < '0' || first[1] > '9' || first[2] < '0' || first[2] > '9') {
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
    }
    int code = (first[0] - '0') * 100 + (first[1] - '0') * 10 + (first[2] - '0');
    snprintf(text_out, text_cap, "%s", first);
    if (first[3] == '-') {
        while (1) {
            char l[4096];
            nv3_ftp_read_line(fd, deadline_ms, l, sizeof(l), line, col);
            size_t tl = strlen(text_out);
            if (tl + 1 < text_cap) {
                text_out[tl] = '\n';
                text_out[tl + 1] = '\0';
            }
            strncat(text_out, l, text_cap - strlen(text_out) - 1);
            if (strlen(l) >= 4 && l[3] == ' ' && strncmp(l, first, 3) == 0) break;
        }
    }
    return code;
}

static int nv3_ftp_cmd(int fd, const char *cmd, long long deadline_ms, char *text_out, size_t text_cap, int line, int col) {
    size_t cl = strlen(cmd);
    char *full = (char *)malloc(cl + 3);
    memcpy(full, cmd, cl);
    full[cl] = '\r';
    full[cl + 1] = '\n';
    full[cl + 2] = '\0';
    nv3_ftp_send(fd, (unsigned char *)full, cl + 2, deadline_ms, line, col);
    free(full);
    return nv3_ftp_read_reply(fd, deadline_ms, text_out, text_cap, line, col);
}

static void nv3_ftp_expect(int code, const int *want, int nwant, int line, int col) {
    for (int i = 0; i < nwant; i++) {
        if (code == want[i]) return;
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "خادم FTP رد برمز %d", code);
}

static void nv3_ftp_parse_pasv(const char *text, char *host_out, int *port_out, int line, int col) {
    const char *open = strchr(text, '(');
    const char *close = open ? strchr(open, ')') : NULL;
    if (!open || !close) {
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
    }
    int nums[6];
    const char *p = open + 1;
    for (int i = 0; i < 6; i++) {
        while (*p == ' ') p++;
        char *end = NULL;
        long v = strtol(p, &end, 10);
        if (end == p || v < 0 || v > 65535) {
            nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
        }
        nums[i] = (int)v;
        p = end;
        if (i < 5) {
            if (*p != ',') {
                nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
            }
            p++;
        }
    }
    snprintf(host_out, 64, "%d.%d.%d.%d", nums[0], nums[1], nums[2], nums[3]);
    *port_out = nums[4] * 256 + nums[5];
}

static Nv2Bytes nv3_ftp_transfer(int ctrl, const char *user, const char *pass, const char *path, int is_retr, const unsigned char *up, size_t up_len, long long deadline_ms, int line, int col) {
    char text[8192];
    int code;
    code = nv3_ftp_read_reply(ctrl, deadline_ms, text, sizeof(text), line, col);
    { int w[] = {220}; nv3_ftp_expect(code, w, 1, line, col); }
    char cmd[1200];
    snprintf(cmd, sizeof(cmd), "USER %s", user);
    code = nv3_ftp_cmd(ctrl, cmd, deadline_ms, text, sizeof(text), line, col);
    if (code == 331) {
        snprintf(cmd, sizeof(cmd), "PASS %s", pass);
        code = nv3_ftp_cmd(ctrl, cmd, deadline_ms, text, sizeof(text), line, col);
        { int w[] = {202, 230}; nv3_ftp_expect(code, w, 2, line, col); }
    } else {
        { int w[] = {202, 230}; nv3_ftp_expect(code, w, 2, line, col); }
    }
    code = nv3_ftp_cmd(ctrl, "TYPE I", deadline_ms, text, sizeof(text), line, col);
    { int w[] = {200}; nv3_ftp_expect(code, w, 1, line, col); }
    code = nv3_ftp_cmd(ctrl, "PASV", deadline_ms, text, sizeof(text), line, col);
    { int w[] = {227}; nv3_ftp_expect(code, w, 1, line, col); }
    char dhost[64];
    int dport = 0;
    nv3_ftp_parse_pasv(text, dhost, &dport, line, col);
    struct addrinfo *res = nv_net_resolve_checked(dhost, dport, line, col);
    int data = nv_tcp_connect(res->ai_addr, res->ai_addrlen, dhost, deadline_ms, line, col);
    freeaddrinfo(res);
    nv_sock_arm_timeout(data, deadline_ms);
    snprintf(cmd, sizeof(cmd), "%s %s", is_retr ? "RETR" : "STOR", path);
    code = nv3_ftp_cmd(ctrl, cmd, deadline_ms, text, sizeof(text), line, col);
    { int w[] = {125, 150}; nv3_ftp_expect(code, w, 2, line, col); }
    Nv2Bytes out;
    out.data = NULL;
    out.len = 0;
    if (is_retr) {
        size_t cap = 8192;
        out.data = (unsigned char *)malloc(cap);
        unsigned char chunk[8192];
        while (1) {
            if (nv_net_now_ms() >= deadline_ms) {
                free(out.data);
                nv_error(NV_ERR_RUNTIME, "انتهت المهلة الإجمالية (30 ثانية)", line, col);
            }
            nv_sock_arm_timeout(data, deadline_ms);
            ssize_t n = recv(data, chunk, sizeof(chunk), 0);
            if (n == 0) break;
            if (n < 0) {
                if (errno == EINTR) continue;
                free(out.data);
                if (errno == EAGAIN || errno == EWOULDBLOCK) {
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة القراءة من الخادم", line, col);
                }
                nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
            }
            if (out.len + (size_t)n > cap) {
                cap = (out.len + (size_t)n) * 2;
                out.data = (unsigned char *)realloc(out.data, cap);
            }
            memcpy(out.data + out.len, chunk, (size_t)n);
            out.len += (size_t)n;
        }
    } else {
        nv3_ftp_send(data, up, up_len, deadline_ms, line, col);
        shutdown(data, SHUT_WR);
        out.data = (unsigned char *)malloc(1);
        out.len = 0;
    }
    close(data);
    code = nv3_ftp_read_reply(ctrl, deadline_ms, text, sizeof(text), line, col);
    { int w[] = {226, 250}; nv3_ftp_expect(code, w, 2, line, col); }
    nv3_send_quiet(ctrl, "QUIT\r\n", 6);
    return out;
}

static Nv2Bytes nv3_ftp_fetch_core(const char *url, const char *display, int line, int col) {
    Nv3HostUrl u = nv3_parse_host_url(url, "ftp", 21, display, line, col);
    long long deadline_ms = nv_net_deadline_ms();
    struct addrinfo *res = nv_net_resolve_checked(u.host, u.port, line, col);
    int ctrl = nv_tcp_connect(res->ai_addr, res->ai_addrlen, u.host, deadline_ms, line, col);
    freeaddrinfo(res);
    nv_sock_arm_timeout(ctrl, deadline_ms);
    const char *user = u.user ? u.user : "anonymous";
    const char *pass = u.pass ? u.pass : "niva@";
    Nv2Bytes out = nv3_ftp_transfer(ctrl, user, pass, u.path, 1, NULL, 0, deadline_ms, line, col);
    close(ctrl);
    return out;
}

static void nv3_ftp_upload_core(const char *url, const unsigned char *data, size_t len, const char *display, int line, int col) {
    Nv3HostUrl u = nv3_parse_host_url(url, "ftp", 21, display, line, col);
    long long deadline_ms = nv_net_deadline_ms();
    struct addrinfo *res = nv_net_resolve_checked(u.host, u.port, line, col);
    int ctrl = nv_tcp_connect(res->ai_addr, res->ai_addrlen, u.host, deadline_ms, line, col);
    freeaddrinfo(res);
    nv_sock_arm_timeout(ctrl, deadline_ms);
    const char *user = u.user ? u.user : "anonymous";
    const char *pass = u.pass ? u.pass : "niva@";
    Nv2Bytes out = nv3_ftp_transfer(ctrl, user, pass, u.path, 0, data, len, deadline_ms, line, col);
    free(out.data);
    close(ctrl);
}


static void nv3_mqtt_len(size_t n, unsigned char *out, size_t *n_out) {
    size_t i = 0;
    do {
        unsigned char b = (unsigned char)(n % 128);
        n /= 128;
        if (n > 0) b |= 0x80;
        out[i++] = b;
    } while (n > 0);
    *n_out = i;
}

static Nv2Bytes nv3_mqtt_packet(unsigned char cmd, const unsigned char *body, size_t body_len) {
    unsigned char lenbuf[4];
    size_t ln = 0;
    nv3_mqtt_len(body_len, lenbuf, &ln);
    Nv2Bytes out;
    out.data = (unsigned char *)malloc(1 + ln + body_len);
    out.data[0] = cmd;
    memcpy(out.data + 1, lenbuf, ln);
    if (body_len > 0) memcpy(out.data + 1 + ln, body, body_len);
    out.len = 1 + ln + body_len;
    return out;
}

static void nv3_mqtt_str_append(const char *s, unsigned char *buf, size_t *len) {
    size_t sl = strlen(s);
    buf[*len] = (unsigned char)((sl >> 8) & 0xFF);
    buf[*len + 1] = (unsigned char)(sl & 0xFF);
    memcpy(buf + *len + 2, s, sl);
    *len += 2 + sl;
}

static int nv3_mqtt_read_exact(int fd, unsigned char *buf, size_t len, long long deadline_ms, int line, int col) {
    size_t got = 0;
    while (got < len) {
        if (nv_net_now_ms() >= deadline_ms) {
            return -1;
        }
        nv_sock_arm_timeout(fd, deadline_ms);
        ssize_t n = recv(fd, buf + got, len - got, 0);
        if (n == 0) {
            nv_error(NV_ERR_RUNTIME, "انقطع الاتصال بالخادم", line, col);
        }
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return -1;
            }
            nv_error(NV_ERR_RUNTIME, "تعذر قراءة الرد من الخادم", line, col);
        }
        got += (size_t)n;
    }
    return 0;
}

/* 0 = حزمة مقروءة، 1 = مهلة شريحة (بلا خطأ)، وخطأ حقيقي يرفع nv_error.
   (افتراض: وسط حزمة على المضيف المحلي لا يُفترض أن ينقطع.) */
static int nv3_mqtt_read_packet(int fd, long long until_ms, unsigned char *cmd_out, unsigned char **body_out, size_t *body_len_out, int line, int col) {
    long long slice_end = until_ms;
    long long cap = nv_net_now_ms() + 30000;
    if (cap < slice_end) slice_end = cap;
    unsigned char hdr[1];
    if (nv3_mqtt_read_exact(fd, hdr, 1, slice_end, line, col) != 0) return 1;
    unsigned char cmd = hdr[0];
    size_t len = 0;
    size_t mult = 1;
    while (1) {
        unsigned char b;
        if (nv3_mqtt_read_exact(fd, &b, 1, slice_end, line, col) != 0) return 1;
        len += (size_t)(b & 0x7F) * mult;
        if ((b & 0x80) == 0) break;
        mult *= 128;
    }
    if (len > 10000000) {
        nv_error(NV_ERR_RUNTIME, "رد طويل جدا من الخادم", line, col);
    }
    unsigned char *body = (unsigned char *)malloc(len > 0 ? len : 1);
    if (len > 0 && nv3_mqtt_read_exact(fd, body, len, slice_end, line, col) != 0) {
        free(body);
        return 1;
    }
    *cmd_out = cmd;
    *body_out = body;
    *body_len_out = len;
    return 0;
}

static int nv3_mqtt_connect_stream(const char *broker, const char *display, long long *deadline_out, int line, int col) {
    Nv3HostUrl u = nv3_parse_host_url(broker, "mqtt", 1883, display, line, col);
    long long deadline_ms = nv_net_deadline_ms();
    struct addrinfo *res = nv_net_resolve_checked(u.host, u.port, line, col);
    int fd = nv_tcp_connect(res->ai_addr, res->ai_addrlen, u.host, deadline_ms, line, col);
    freeaddrinfo(res);
    unsigned char body[256];
    size_t blen = 0;
    nv3_mqtt_str_append("MQTT", body, &blen);
    body[blen++] = 0x04;
    body[blen++] = 0x02;
    body[blen++] = 0x00;
    body[blen++] = 0x3C;
    nv3_mqtt_str_append("niva", body, &blen);
    Nv2Bytes pkt = nv3_mqtt_packet(0x10, body, blen);
    nv3_ftp_send(fd, pkt.data, pkt.len, deadline_ms, line, col);
    free(pkt.data);
    unsigned char cmd = 0;
    unsigned char *rbody = NULL;
    size_t rlen = 0;
    int rc = nv3_mqtt_read_packet(fd, deadline_ms, &cmd, &rbody, &rlen, line, col);
    if (rc == 1) {
        nv_error(NV_ERR_RUNTIME, "انتهت مهلة القراءة من الخادم", line, col);
    }
    if (cmd != 0x20 || rlen != 2) {
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
    }
    if (rbody[1] != 0) {
        int code = rbody[1];
        free(rbody);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "وسيط MQTT رفض الاتصال (رمز %d)", code);
    }
    free(rbody);
    *deadline_out = deadline_ms;
    return fd;
}

static NvValue nv3_mqtt_subscribe_core(const char *broker, const char *topic, long long timeout_secs, const char *display, int line, int col) {
    long long deadline_ms = 0;
    int fd = nv3_mqtt_connect_stream(broker, display, &deadline_ms, line, col);
    unsigned char body[1200];
    size_t blen = 0;
    body[blen++] = 0x00;
    body[blen++] = 0x01;
    nv3_mqtt_str_append(topic, body, &blen);
    body[blen++] = 0x00;
    Nv2Bytes pkt = nv3_mqtt_packet(0x82, body, blen);
    nv3_ftp_send(fd, pkt.data, pkt.len, deadline_ms, line, col);
    free(pkt.data);
    unsigned char cmd = 0;
    unsigned char *rbody = NULL;
    size_t rlen = 0;
    int rc = nv3_mqtt_read_packet(fd, deadline_ms, &cmd, &rbody, &rlen, line, col);
    if (rc == 1) {
        nv_error(NV_ERR_RUNTIME, "انتهت مهلة القراءة من الخادم", line, col);
    }
    if (cmd != 0x90) {
        nv_error(NV_ERR_RUNTIME, "رد غير صالح من الخادم", line, col);
    }
    free(rbody);
    long long until_ms = nv_net_now_ms() + timeout_secs * 1000;
    NvValue out = nv_list_new();
    long long last_ping = nv_net_now_ms();
    while (nv_net_now_ms() < until_ms) {
        unsigned char pcmd = 0;
        unsigned char *pbody = NULL;
        size_t plen = 0;
        int prc = nv3_mqtt_read_packet(fd, until_ms, &pcmd, &pbody, &plen, line, col);
        if (prc == 1) {
            if (nv_net_now_ms() >= until_ms) break;
            if (nv_net_now_ms() - last_ping >= 30000) {
                unsigned char ping[] = {0xC0, 0x00};
                nv3_send_quiet(fd, ping, 2);
                last_ping = nv_net_now_ms();
            }
            continue;
        }
        if (pcmd == 0xD0) {
            free(pbody);
            continue;
        }
        if ((pcmd & 0xF0) == 0x30 && plen >= 2) {
            int tlen = (pbody[0] << 8) | pbody[1];
            if ((size_t)(2 + tlen) <= plen) {
                size_t olen = 0;
                char *text = nv_utf8_lossy((char *)pbody + 2 + tlen, plen - (size_t)(2 + tlen), &olen);
                nv_list_push_raw(out.u.list, nv_str_new(text ? text : "", olen));
                free(text);
            }
        }
        free(pbody);
    }
    unsigned char disc[] = {0xE0, 0x00};
    nv3_send_quiet(fd, disc, 2);
    close(fd);
    return out;
}

static void nv3_mqtt_publish_core(const char *broker, const char *topic, const unsigned char *msg, size_t msg_len, const char *display, int line, int col) {
    long long deadline_ms = 0;
    int fd = nv3_mqtt_connect_stream(broker, display, &deadline_ms, line, col);
    size_t tlen = strlen(topic);
    size_t blen = 2 + tlen + msg_len;
    unsigned char *body = (unsigned char *)malloc(blen);
    body[0] = (unsigned char)((tlen >> 8) & 0xFF);
    body[1] = (unsigned char)(tlen & 0xFF);
    memcpy(body + 2, topic, tlen);
    if (msg_len > 0) memcpy(body + 2 + tlen, msg, msg_len);
    Nv2Bytes pub = nv3_mqtt_packet(0x30, body, blen);
    free(body);
    nv3_ftp_send(fd, pub.data, pub.len, deadline_ms, line, col);
    free(pub.data);
    unsigned char disc[] = {0xE0, 0x00};
    nv3_send_quiet(fd, disc, 2);
    close(fd);
}


// ---------- لينكس8-طبقة3: مغلفات بروتوكولات ----------
static NvValue nv3_ftp_fetch(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اجلب_ftp تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *url = nv2_url_arg(args, 0, "اجلب_ftp", line, col);
    nv3_require_network(line, col);
    Nv2Bytes b = nv3_ftp_fetch_core(url, "شبكة.اجلب_ftp", line, col);
    size_t olen = 0;
    char *text = nv_utf8_lossy((char *)b.data, b.len, &olen);
    NvValue out = nv_str_new(text ? text : "", olen);
    free(text);
    free(b.data);
    return out;
}

static NvValue nv3_ftp_download(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نزل_ftp تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *url = nv2_url_arg(args, 0, "نزل_ftp", line, col);
    const char *path = nv2_path_arg(args, 1, "نزل_ftp", line, col);
    nv3_require_network(line, col);
    Nv2Bytes b = nv3_ftp_fetch_core(url, "شبكة.نزل_ftp", line, col);
    nv2_write_file(path, b.data, b.len, "نزل_ftp", line, col);
    double n = (double)b.len;
    free(b.data);
    return nv_num(n);
}

static NvValue nv3_ftp_upload(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.ارفع_ftp تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *url = nv2_url_arg(args, 0, "ارفع_ftp", line, col);
    const char *path = nv2_path_arg(args, 1, "ارفع_ftp", line, col);
    Nv2Bytes data = nv2_read_file(path, "ارفع_ftp", line, col);
    nv3_require_network(line, col);
    nv3_ftp_upload_core(url, data.data, data.len, "شبكة.ارفع_ftp", line, col);
    double n = (double)data.len;
    free(data.data);
    return nv_num(n);
}

static NvValue nv3_ftps_fetch(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اجلب_ftps تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *url = nv2_url_arg(args, 0, "اجلب_ftps", line, col);
    nv3_require_network(line, col);
    if (strncmp(url, "ftps://", 7) != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اجلب_ftps تتوقع رابط ftps");
    }
    nv_error(NV_ERR_RUNTIME, "FTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق للروابط المشفرة", line, col);
    return nv_nil();
}

static NvValue nv3_ftps_download(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نزل_ftps تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *url = nv2_url_arg(args, 0, "نزل_ftps", line, col);
    (void)nv2_path_arg(args, 1, "نزل_ftps", line, col);
    nv3_require_network(line, col);
    if (strncmp(url, "ftps://", 7) != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نزل_ftps تتوقع رابط ftps");
    }
    nv_error(NV_ERR_RUNTIME, "FTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق للروابط المشفرة", line, col);
    return nv_nil();
}

static NvValue nv3_mqtt_sub(NvValue *args, int nargs, int line, int col) {
    if (nargs < 2 || nargs > 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اشترك_mqtt تتوقع 2 أو 3 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt تتوقع وسيطا نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *broker = args[0].u.str->data;
    if (args[0].u.str->len == 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt تتوقع وسيطا غير فارغ");
    }
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt تتوقع موضوعا نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    const char *topic = args[1].u.str->data;
    if (args[1].u.str->len == 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt تتوقع موضوعا غير فارغ");
    }
    long long timeout = 5;
    if (nargs == 3) {
        if (args[2].tag != NV_NUM) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt تتوقع مهلة عددا لكن أعطيت %s", nv_type_name(args[2]));
        }
        double d = args[2].u.num;
        if (d != (double)(long long)d || d < 1 || d > 300) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اشترك_mqtt مهلة غير صالحة");
        }
        timeout = (long long)d;
    }
    nv3_require_network(line, col);
    return nv3_mqtt_subscribe_core(broker, topic, timeout, "شبكة.اشترك_mqtt", line, col);
}

static NvValue nv3_mqtt_pub(NvValue *args, int nargs, int line, int col) {
    if (nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.انشر_mqtt تتوقع 3 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.انشر_mqtt تتوقع وسيطا نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *broker = args[0].u.str->data;
    if (args[0].u.str->len == 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.انشر_mqtt تتوقع وسيطا غير فارغ");
    }
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.انشر_mqtt تتوقع موضوعا نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    const char *topic = args[1].u.str->data;
    if (args[1].u.str->len == 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.انشر_mqtt تتوقع موضوعا غير فارغ");
    }
    Nv2Bytes msg = nv2_body_arg(args[2], "انشر_mqtt", line, col);
    nv3_require_network(line, col);
    nv3_mqtt_publish_core(broker, topic, msg.data, msg.len, "شبكة.انشر_mqtt", line, col);
    free(msg.data);
    return nv_bool(1);
}

static NvValue nv3_sftp_fetch(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اجلب_sftp تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    nv_error(NV_ERR_RUNTIME, "SFTP غير مدعوم في هذه النسخة — يحتاج تنفيذ SSH", line, col);
    return nv_nil();
}

static NvValue nv3_sftp_download(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نزل_sftp تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    nv_error(NV_ERR_RUNTIME, "SFTP غير مدعوم في هذه النسخة — يحتاج تنفيذ SSH", line, col);
    return nv_nil();
}

static NvValue nv3_http3(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اطلب_http3 تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    nv_error(NV_ERR_RUNTIME, "HTTP/3 غير مدعوم في هذه النسخة — يحتاج QUIC", line, col);
    return nv_nil();
}

static NvValue nv3_bg_download(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نزل_في_الخلفية تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    nv_error(NV_ERR_RUNTIME, "التنزيل في الخلفية غير مدعوم بعد — يحتاج آلة المهام (§102)", line, col);
    return nv_nil();
}


// ---------- لينكس8-طبقة4: JSON (محلل + طابعة جميلة) ----------
// مرآة parse_json + b4_json_write_pretty في Rust (مسافة بادئة 2).
typedef struct { char *buf; size_t len; size_t cap; } Nv4Buf;

static void nv4_buf_addn(Nv4Buf *b, const char *s, size_t n) {
    if (b->len + n + 1 > b->cap) {
        size_t nc = b->cap ? b->cap : 64;
        while (nc < b->len + n + 1) nc *= 2;
        b->buf = (char *)realloc(b->buf, nc);
        b->cap = nc;
    }
    memcpy(b->buf + b->len, s, n);
    b->len += n;
    b->buf[b->len] = '\0';
}

static void nv4_buf_adds(Nv4Buf *b, const char *s) {
    nv4_buf_addn(b, s, strlen(s));
}

static void nv4_buf_addc(Nv4Buf *b, char c) {
    nv4_buf_addn(b, &c, 1);
}

static void nv4_utf8_encode(uint32_t cp, char *out, size_t *n) {
    if (cp < 0x80) {
        out[0] = (char)cp;
        *n = 1;
    } else if (cp < 0x800) {
        out[0] = (char)(0xC0 | (cp >> 6));
        out[1] = (char)(0x80 | (cp & 0x3F));
        *n = 2;
    } else if (cp < 0x10000) {
        out[0] = (char)(0xE0 | (cp >> 12));
        out[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[2] = (char)(0x80 | (cp & 0x3F));
        *n = 3;
    } else {
        out[0] = (char)(0xF0 | (cp >> 18));
        out[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
        out[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[3] = (char)(0x80 | (cp & 0x3F));
        *n = 4;
    }
}

typedef struct { const unsigned char *s; size_t len; size_t pos; } Nv4Json;

static void nv4_json_ws(Nv4Json *p) {
    while (p->pos < p->len) {
        unsigned char c = p->s[p->pos];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') p->pos++;
        else break;
    }
}

static int nv4_json_hex4(Nv4Json *p, uint32_t *out) {
    if (p->pos + 4 > p->len) return 0;
    uint32_t v = 0;
    for (int i = 0; i < 4; i++) {
        unsigned char c = p->s[p->pos++];
        v <<= 4;
        if (c >= '0' && c <= '9') v |= (uint32_t)(c - '0');
        else if (c >= 'a' && c <= 'f') v |= (uint32_t)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') v |= (uint32_t)(c - 'A' + 10);
        else return 0;
    }
    *out = v;
    return 1;
}

static int nv4_json_string(Nv4Json *p, char **out, size_t *out_len) {
    if (p->pos >= p->len || p->s[p->pos] != '"') return 0;
    p->pos++;
    size_t cap = 64;
    size_t len = 0;
    char *buf = (char *)malloc(cap);
    while (p->pos < p->len) {
        unsigned char c = p->s[p->pos++];
        if (c == '"') {
            buf[len] = '\0';
            *out = buf;
            *out_len = len;
            return 1;
        }
        if (c == '\\') {
            if (p->pos >= p->len) {
                free(buf);
                return 0;
            }
            unsigned char e = p->s[p->pos++];
            char simple = 0;
            int is_simple = 1;
            switch (e) {
                case '"': simple = '"'; break;
                case '\\': simple = '\\'; break;
                case '/': simple = '/'; break;
                case 'b': simple = '\b'; break;
                case 'f': simple = '\f'; break;
                case 'n': simple = '\n'; break;
                case 'r': simple = '\r'; break;
                case 't': simple = '\t'; break;
                case 'u': is_simple = 0; break;
                default:
                    free(buf);
                    return 0;
            }
            if (is_simple) {
                if (len + 1 >= cap) {
                    cap *= 2;
                    buf = (char *)realloc(buf, cap);
                }
                buf[len++] = simple;
            } else {
                uint32_t cp = 0;
                if (!nv4_json_hex4(p, &cp)) {
                    free(buf);
                    return 0;
                }
                if (cp >= 0xD800 && cp <= 0xDBFF) {
                    if (p->pos + 6 <= p->len && p->s[p->pos] == '\\' && p->s[p->pos + 1] == 'u') {
                        size_t save = p->pos;
                        p->pos += 2;
                        uint32_t lo = 0;
                        if (nv4_json_hex4(p, &lo) && lo >= 0xDC00 && lo <= 0xDFFF) {
                            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        } else {
                            p->pos = save;
                            cp = 0xFFFD;
                        }
                    } else {
                        cp = 0xFFFD;
                    }
                } else if (cp >= 0xDC00 && cp <= 0xDFFF) {
                    cp = 0xFFFD;
                }
                char enc[4];
                size_t en = 0;
                nv4_utf8_encode(cp, enc, &en);
                if (len + en >= cap) {
                    while (len + en >= cap) cap *= 2;
                    buf = (char *)realloc(buf, cap);
                }
                memcpy(buf + len, enc, en);
                len += en;
            }
        } else if (c < 0x20) {
            free(buf);
            return 0;
        } else {
            if (len + 1 >= cap) {
                cap *= 2;
                buf = (char *)realloc(buf, cap);
            }
            buf[len++] = (char)c;
        }
    }
    free(buf);
    return 0;
}

static int nv4_json_number(Nv4Json *p, double *out) {
    size_t start = p->pos;
    if (p->pos < p->len && p->s[p->pos] == '-') p->pos++;
    if (p->pos >= p->len) return 0;
    if (p->s[p->pos] == '0') {
        p->pos++;
    } else if (p->s[p->pos] >= '1' && p->s[p->pos] <= '9') {
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    } else {
        return 0;
    }
    if (p->pos < p->len && p->s[p->pos] == '.') {
        p->pos++;
        if (p->pos >= p->len || p->s[p->pos] < '0' || p->s[p->pos] > '9') return 0;
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    }
    if (p->pos < p->len && (p->s[p->pos] == 'e' || p->s[p->pos] == 'E')) {
        p->pos++;
        if (p->pos < p->len && (p->s[p->pos] == '+' || p->s[p->pos] == '-')) p->pos++;
        if (p->pos >= p->len || p->s[p->pos] < '0' || p->s[p->pos] > '9') return 0;
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    }
    size_t n = p->pos - start;
    char *tmp = (char *)malloc(n + 1);
    memcpy(tmp, p->s + start, n);
    tmp[n] = '\0';
    *out = strtod(tmp, NULL);
    free(tmp);
    return 1;
}

static int nv4_json_value(Nv4Json *p, NvValue *out, int depth) {
    if (depth > 100) return 0;
    nv4_json_ws(p);
    if (p->pos >= p->len) return 0;
    unsigned char c = p->s[p->pos];
    if (c == '{') {
        p->pos++;
        NvValue d = nv_dict_new();
        nv4_json_ws(p);
        if (p->pos < p->len && p->s[p->pos] == '}') {
            p->pos++;
            *out = d;
            return 1;
        }
        while (1) {
            nv4_json_ws(p);
            char *key = NULL;
            size_t klen = 0;
            if (!nv4_json_string(p, &key, &klen)) return 0;
            nv4_json_ws(p);
            if (p->pos >= p->len || p->s[p->pos] != ':') {
                free(key);
                return 0;
            }
            p->pos++;
            NvValue val;
            if (!nv4_json_value(p, &val, depth + 1)) {
                free(key);
                return 0;
            }
            nv_dict_set_raw(d.u.dict, key, val);
            free(key);
            nv4_json_ws(p);
            if (p->pos < p->len && p->s[p->pos] == ',') {
                p->pos++;
                continue;
            }
            if (p->pos < p->len && p->s[p->pos] == '}') {
                p->pos++;
                *out = d;
                return 1;
            }
            return 0;
        }
    }
    if (c == '[') {
        p->pos++;
        NvValue l = nv_list_new();
        nv4_json_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ']') {
            p->pos++;
            *out = l;
            return 1;
        }
        while (1) {
            NvValue val;
            if (!nv4_json_value(p, &val, depth + 1)) return 0;
            nv_list_push_raw(l.u.list, val);
            nv4_json_ws(p);
            if (p->pos < p->len && p->s[p->pos] == ',') {
                p->pos++;
                continue;
            }
            if (p->pos < p->len && p->s[p->pos] == ']') {
                p->pos++;
                *out = l;
                return 1;
            }
            return 0;
        }
    }
    if (c == '"') {
        char *s = NULL;
        size_t sl = 0;
        if (!nv4_json_string(p, &s, &sl)) return 0;
        *out = nv_str_new(s, sl);
        free(s);
        return 1;
    }
    if (c == 't') {
        if (p->pos + 4 <= p->len && memcmp(p->s + p->pos, "true", 4) == 0) {
            p->pos += 4;
            *out = nv_bool(1);
            return 1;
        }
        return 0;
    }
    if (c == 'f') {
        if (p->pos + 5 <= p->len && memcmp(p->s + p->pos, "false", 5) == 0) {
            p->pos += 5;
            *out = nv_bool(0);
            return 1;
        }
        return 0;
    }
    if (c == 'n') {
        if (p->pos + 4 <= p->len && memcmp(p->s + p->pos, "null", 4) == 0) {
            p->pos += 4;
            *out = nv_nil();
            return 1;
        }
        return 0;
    }
    if (c == '-' || (c >= '0' && c <= '9')) {
        double n = 0;
        if (!nv4_json_number(p, &n)) return 0;
        *out = nv_num(n);
        return 1;
    }
    return 0;
}

static int nv4_json_parse_doc(const char *text, NvValue *out) {
    Nv4Json p;
    p.s = (const unsigned char *)text;
    p.len = strlen(text);
    p.pos = 0;
    if (!nv4_json_value(&p, out, 0)) return 0;
    nv4_json_ws(&p);
    if (p.pos != p.len) return 0;
    return 1;
}

static void nv4_json_escape_into(Nv4Buf *out, const char *s, size_t len) {
    nv4_buf_addc(out, '"');
    size_t i = 0;
    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        if (c == '"') nv4_buf_adds(out, "\\\"");
        else if (c == '\\') nv4_buf_adds(out, "\\\\");
        else if (c == '\n') nv4_buf_adds(out, "\\n");
        else if (c == '\r') nv4_buf_adds(out, "\\r");
        else if (c == '\t') nv4_buf_adds(out, "\\t");
        else if (c < 0x20) {
            char tmp[8];
            snprintf(tmp, sizeof(tmp), "\\u%04x", c);
            nv4_buf_adds(out, tmp);
        } else {
            nv4_buf_addc(out, (char)c);
        }
        i++;
    }
    nv4_buf_addc(out, '"');
}

static void nv4_json_num(Nv4Buf *out, double n) {
    char tmp[64];
    if (n == trunc(n) && fabs(n) < 9e15) {
        snprintf(tmp, sizeof(tmp), "%lld", (long long)n);
    } else {
        snprintf(tmp, sizeof(tmp), "%.15g", n);
    }
    nv4_buf_adds(out, tmp);
}

static void nv4_json_pad(Nv4Buf *out, int level) {
    for (int i = 0; i < 2 * level; i++) nv4_buf_addc(out, ' ');
}

static int nv4_json_pretty(NvValue v, Nv4Buf *out, int level) {
    if (level > 100) return 0;
    switch (v.tag) {
        case NV_NIL:
            nv4_buf_adds(out, "null");
            return 1;
        case NV_BOOL:
            nv4_buf_adds(out, v.u.boolean ? "true" : "false");
            return 1;
        case NV_NUM:
            if (!isfinite(v.u.num)) return 0;
            nv4_json_num(out, v.u.num);
            return 1;
        case NV_STR:
            nv4_json_escape_into(out, v.u.str->data, v.u.str->len);
            return 1;
        case NV_LIST: {
            NvList *l = v.u.list;
            if (l->len == 0) {
                nv4_buf_adds(out, "[]");
                return 1;
            }
            nv4_buf_addc(out, '[');
            for (int i = 0; i < l->len; i++) {
                if (i > 0) nv4_buf_addc(out, ',');
                nv4_buf_addc(out, '\n');
                nv4_json_pad(out, level + 1);
                if (!nv4_json_pretty(l->items[i], out, level + 1)) return 0;
            }
            nv4_buf_addc(out, '\n');
            nv4_json_pad(out, level);
            nv4_buf_addc(out, ']');
            return 1;
        }
        case NV_DICT: {
            NvDict *d = v.u.dict;
            if (d->len == 0) {
                nv4_buf_adds(out, "{}");
                return 1;
            }
            nv4_buf_addc(out, '{');
            for (int i = 0; i < d->len; i++) {
                if (i > 0) nv4_buf_addc(out, ',');
                nv4_buf_addc(out, '\n');
                nv4_json_pad(out, level + 1);
                nv4_json_escape_into(out, d->entries[i].key, strlen(d->entries[i].key));
                nv4_buf_adds(out, ": ");
                if (!nv4_json_pretty(d->entries[i].val, out, level + 1)) return 0;
            }
            nv4_buf_addc(out, '\n');
            nv4_json_pad(out, level);
            nv4_buf_addc(out, '}');
            return 1;
        }
        default:
            return 0;
    }
}


// ---------- لينكس8-طبقة4: مساعدات جلسات/رد/توقيع ----------
typedef struct { char *key; char *val; } Nv4Pair;
typedef struct { Nv4Pair *items; int len; int cap; } Nv4Pairs;

static void nv4_pairs_push(Nv4Pairs *ps, const char *k, const char *v) {
    if (ps->len >= ps->cap) {
        ps->cap = ps->cap ? ps->cap * 2 : 8;
        ps->items = (Nv4Pair *)realloc(ps->items, (size_t)ps->cap * sizeof(Nv4Pair));
    }
    ps->items[ps->len].key = strdup(k);
    ps->items[ps->len].val = strdup(v);
    ps->len++;
}

static int nv4_pairs_find(Nv4Pairs *ps, const char *k) {
    for (int i = 0; i < ps->len; i++) {
        if (strcmp(ps->items[i].key, k) == 0) return i;
    }
    return -1;
}

static NvValue nv4_pairs_to_dict(Nv4Pairs *ps) {
    NvValue d = nv_dict_new();
    for (int i = 0; i < ps->len; i++) {
        nv_dict_set_raw(d.u.dict, ps->items[i].key, nv_str_cstr(ps->items[i].val));
    }
    return d;
}

static NvValue *nv4_dict_get(NvDict *d, const char *key) {
    int i = nv_dict_find(d, key);
    if (i < 0) return NULL;
    return &d->entries[i].val;
}

static void nv4_req_parts(NvValue req, const char *display, int line, int col, char **url_out, char **method_out, Nv2Bytes *body_out) {
    if (req.tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(req));
    }
    NvValue *u = nv4_dict_get(req.u.dict, "رابط");
    if (!u) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع مفتاح رابط في الطلب", display);
    }
    if (u->tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع رابطا نصا لكن أعطيت %s", display, nv_type_name(*u));
    }
    if (u->u.str->len == 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع رابطا غير فارغ", display);
    }
    *url_out = strdup(u->u.str->data);
    NvValue *m = nv4_dict_get(req.u.dict, "طريقة");
    if (m) {
        if (m->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع طريقة نصا لكن أعطيت %s", display, nv_type_name(*m));
        }
        *method_out = strdup(m->u.str->data);
    } else {
        *method_out = strdup("GET");
    }
    if ((*method_out)[0] == '\0') {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s طريقة غير صالحة", display);
    }
    for (char *p = *method_out; *p; p++) {
        if (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r' || (unsigned char)*p < 0x20) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s طريقة غير صالحة", display);
        }
    }
    body_out->data = NULL;
    body_out->len = 0;
    NvValue *b = nv4_dict_get(req.u.dict, "جسم");
    if (b) {
        if (b->tag == NV_STR) {
            body_out->len = b->u.str->len;
            body_out->data = (unsigned char *)malloc(body_out->len ? body_out->len : 1);
            memcpy(body_out->data, b->u.str->data, body_out->len);
        } else if (b->tag == NV_LIST) {
            NvList *l = b->u.list;
            body_out->data = (unsigned char *)malloc(l->len ? (size_t)l->len : 1);
            for (int i = 0; i < l->len; i++) {
                NvValue it = l->items[i];
                if (it.tag != NV_NUM || it.u.num != trunc(it.u.num) || it.u.num < 0 || it.u.num > 255) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s جسم غير صالح — القائمة بايتات 0 إلى 255", display);
                }
                body_out->data[body_out->len++] = (unsigned char)it.u.num;
            }
        } else {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع جسما نصا أو قائمة بايتات لكن أعطيت %s", display, nv_type_name(*b));
        }
    }
}

static void nv4_resp_parts(NvValue resp, const char *display, int line, int col, int *status_out, Nv4Pairs *headers_out, char **body_out) {
    if (resp.tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(resp));
    }
    *status_out = 0;
    NvValue *s = nv4_dict_get(resp.u.dict, "رمز");
    if (s) {
        if (s->tag != NV_NUM || s->u.num != trunc(s->u.num) || s->u.num < 0 || s->u.num > 65535) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع ردا فيه رمز عددا لكن أعطيت %s", display, nv_type_name(*s));
        }
        *status_out = (int)s->u.num;
    }
    headers_out->items = NULL;
    headers_out->len = 0;
    headers_out->cap = 0;
    NvValue *h = nv4_dict_get(resp.u.dict, "رؤوس");
    if (h) {
        if (h->tag != NV_DICT) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع ردا فيه رؤوس قاموسا لكن أعطيت %s", display, nv_type_name(*h));
        }
        NvDict *d = h->u.dict;
        for (int i = 0; i < d->len; i++) {
            if (d->entries[i].val.tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قيما نصية فقط لكن أعطيت %s", display, nv_type_name(d->entries[i].val));
            }
            nv4_pairs_push(headers_out, d->entries[i].key, d->entries[i].val.u.str->data);
        }
    }
    *body_out = strdup("");
    NvValue *b = nv4_dict_get(resp.u.dict, "جسم");
    if (b) {
        if (b->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع ردا فيه جسم نصا لكن أعطيت %s", display, nv_type_name(*b));
        }
        free(*body_out);
        *body_out = strdup(b->u.str->data);
    }
}

static int nv4_cmp_pair(const void *a, const void *b) {
    return strcmp(((const Nv4Pair *)a)->key, ((const Nv4Pair *)b)->key);
}

static char *nv4_headers_text(Nv4Pairs *headers) {
    Nv4Buf out;
    out.buf = NULL;
    out.len = 0;
    out.cap = 0;
    Nv4Pairs sorted;
    sorted.items = NULL;
    sorted.len = 0;
    sorted.cap = 0;
    for (int i = 0; i < headers->len; i++) {
        nv4_pairs_push(&sorted, headers->items[i].key, headers->items[i].val);
    }
    if (sorted.len > 1) qsort(sorted.items, (size_t)sorted.len, sizeof(Nv4Pair), nv4_cmp_pair);
    for (int i = 0; i < sorted.len; i++) {
        if (i > 0) nv4_buf_addc(&out, '\n');
        nv4_buf_adds(&out, sorted.items[i].key);
        nv4_buf_adds(&out, ": ");
        nv4_buf_adds(&out, sorted.items[i].val);
    }
    if (!out.buf) {
        out.buf = strdup("");
    }
    return out.buf;
}

static char *nv4_session_text(Nv4Pairs *cookies, Nv4Pairs *headers) {
    Nv4Buf out;
    out.buf = NULL;
    out.len = 0;
    out.cap = 0;
    for (int i = 0; i < cookies->len; i++) {
        nv4_buf_adds(&out, "كوكيز ");
        nv4_buf_adds(&out, cookies->items[i].key);
        nv4_buf_addc(&out, '=');
        nv4_buf_adds(&out, cookies->items[i].val);
        nv4_buf_addc(&out, '\n');
    }
    for (int i = 0; i < headers->len; i++) {
        nv4_buf_adds(&out, "رأس ");
        nv4_buf_adds(&out, headers->items[i].key);
        nv4_buf_adds(&out, ": ");
        nv4_buf_adds(&out, headers->items[i].val);
        nv4_buf_addc(&out, '\n');
    }
    if (!out.buf) {
        out.buf = strdup("");
    }
    return out.buf;
}

static void nv4_parse_session_text(const char *text, Nv4Pairs *cookies, Nv4Pairs *headers) {
    const char *p = text;
    while (*p) {
        const char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (llen > 0 && line[llen - 1] == '\r') line[llen - 1] = '\0';
        if (strncmp(line, "كوكيز ", strlen("كوكيز ")) == 0) {
            char *eq = strchr(line + strlen("كوكيز "), '=');
            if (eq) {
                *eq = '\0';
                nv4_pairs_push(cookies, line + strlen("كوكيز "), eq + 1);
            }
        } else if (strncmp(line, "رأس ", strlen("رأس ")) == 0) {
            char *co = strchr(line + strlen("رأس "), ':');
            if (co) {
                *co = '\0';
                const char *v = co + 1;
                while (*v == ' ') v++;
                nv4_pairs_push(headers, line + strlen("رأس "), v);
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
}

static unsigned char *nv4_read_opt(const char *path, size_t *len_out, int line, int col) {
    char *resolved = nv_resolve_path(path, line, col);
    FILE *f = fopen(resolved, "rb");
    free(resolved);
    if (!f) return NULL;
    size_t cap = 4096;
    size_t len = 0;
    unsigned char *buf = (unsigned char *)malloc(cap);
    while (1) {
        if (len == cap) {
            cap *= 2;
            buf = (unsigned char *)realloc(buf, cap);
        }
        size_t n = fread(buf + len, 1, cap - len, f);
        if (n == 0) break;
        len += n;
    }
    fclose(f);
    *len_out = len;
    return buf;
}

static char *nv4_b64_encode(const unsigned char *data, size_t len) {
    static const char tbl[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t outlen = ((len + 2) / 3) * 4;
    char *out = (char *)malloc(outlen + 1);
    size_t o = 0;
    for (size_t i = 0; i < len; i += 3) {
        size_t rem = len - i;
        unsigned int v = ((unsigned int)data[i]) << 16;
        if (rem > 1) v |= ((unsigned int)data[i + 1]) << 8;
        if (rem > 2) v |= data[i + 2];
        out[o++] = tbl[(v >> 18) & 63];
        out[o++] = tbl[(v >> 12) & 63];
        out[o++] = rem > 1 ? tbl[(v >> 6) & 63] : '=';
        out[o++] = rem > 2 ? tbl[v & 63] : '=';
    }
    out[o] = '\0';
    return out;
}

static void nv4_sign_amazon_core(const char *method, const char *url, const unsigned char *body, size_t body_len, const char *key, const char *secret, const char *region, const char *service, int line, int col, Nv4Pairs *out) {
    NvUrl parsed;
    nv_parse_url(url, &parsed, line, col);
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    char datestamp[16];
    char amzdate[32];
    nv_amz_datetime((long long)ts.tv_sec, datestamp, sizeof(datestamp), amzdate, sizeof(amzdate));
    char payload_hash[65];
    nv_sha256_hex(body, body_len, payload_hash);
    const char *q = strchr(parsed.path, '?');
    char *uri;
    char *query;
    if (q) {
        uri = (char *)malloc((size_t)(q - parsed.path) + 1);
        memcpy(uri, parsed.path, (size_t)(q - parsed.path));
        uri[q - parsed.path] = '\0';
        query = strdup(q + 1);
    } else {
        uri = strdup(parsed.path);
        query = strdup("");
    }
    char host_hdr[1024];
    if ((strcmp(parsed.scheme, "http") == 0 && parsed.port == 80)
        || (strcmp(parsed.scheme, "https") == 0 && parsed.port == 443)) {
        snprintf(host_hdr, sizeof(host_hdr), "%s", parsed.host);
    } else {
        snprintf(host_hdr, sizeof(host_hdr), "%s:%d", parsed.host, parsed.port);
    }
    char *method_up = strdup(method);
    for (char *p = method_up; *p; p++) {
        if (*p >= 'a' && *p <= 'z') *p = (char)(*p - 32);
    }
    size_t cr_cap = strlen(method_up) + strlen(uri) + strlen(query) + strlen(host_hdr) + strlen(payload_hash) * 2 + 128;
    char *canonical = (char *)malloc(cr_cap);
    snprintf(canonical, cr_cap, "%s\n%s\n%s\nhost:%s\nx-amz-content-sha256:%s\nx-amz-date:%s\nhost;x-amz-content-sha256;x-amz-date\n%s",
             method_up, uri, query, host_hdr, payload_hash, amzdate, payload_hash);
    char scope[256];
    snprintf(scope, sizeof(scope), "%s/%s/%s/aws4_request", datestamp, region, service);
    char canon_hash[65];
    nv_sha256_hex((const unsigned char *)canonical, strlen(canonical), canon_hash);
    size_t ts_cap = strlen(amzdate) + strlen(scope) + strlen(canon_hash) + 64;
    char *to_sign = (char *)malloc(ts_cap);
    snprintf(to_sign, ts_cap, "AWS4-HMAC-SHA256\n%s\n%s\n%s", amzdate, scope, canon_hash);
    char k_date_in[256];
    snprintf(k_date_in, sizeof(k_date_in), "AWS4%s", secret);
    unsigned char k_date[32];
    nv_hmac_sha256((const unsigned char *)k_date_in, strlen(k_date_in), (const unsigned char *)datestamp, strlen(datestamp), k_date);
    unsigned char k_region[32];
    nv_hmac_sha256(k_date, 32, (const unsigned char *)region, strlen(region), k_region);
    unsigned char k_service[32];
    nv_hmac_sha256(k_region, 32, (const unsigned char *)service, strlen(service), k_service);
    unsigned char k_signing[32];
    nv_hmac_sha256(k_service, 32, (const unsigned char *)"aws4_request", 12, k_signing);
    unsigned char sig_raw[32];
    nv_hmac_sha256(k_signing, 32, (const unsigned char *)to_sign, strlen(to_sign), sig_raw);
    char sig_hex[65];
    nv_hex_bytes(sig_raw, 32, sig_hex);
    nv4_pairs_push(out, "x-amz-date", amzdate);
    nv4_pairs_push(out, "x-amz-content-sha256", payload_hash);
    size_t au_cap = strlen(key) + strlen(scope) + strlen(sig_hex) + 128;
    char *auth = (char *)malloc(au_cap);
    snprintf(auth, au_cap, "AWS4-HMAC-SHA256 Credential=%s/%s, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=%s",
             key, scope, sig_hex);
    nv4_pairs_push(out, "Authorization", auth);
    free(auth);
    free(to_sign);
    free(canonical);
    free(method_up);
    free(uri);
    free(query);
    nv_url_free(&parsed);
}

static void nv4_sign_httpsig(const char *method, const char *url, const char *key_id, const char *key, const char *algorithm, int line, int col, char **sig_out) {
    if (strcmp(algorithm, "hmac-sha256") != 0) {
        nv_error(NV_ERR_RUNTIME, "خوارزمية التوقيع غير مدعومة — المتاح hmac-sha256 فقط", line, col);
    }
    NvUrl parsed;
    nv_parse_url(url, &parsed, line, col);
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    char date[64];
    nv_http_date_from_unix((long long)ts.tv_sec, date, sizeof(date));
    char host_hdr[1024];
    if ((strcmp(parsed.scheme, "http") == 0 && parsed.port == 80)
        || (strcmp(parsed.scheme, "https") == 0 && parsed.port == 443)) {
        snprintf(host_hdr, sizeof(host_hdr), "%s", parsed.host);
    } else {
        snprintf(host_hdr, sizeof(host_hdr), "%s:%d", parsed.host, parsed.port);
    }
    char *method_lo = strdup(method);
    for (char *p = method_lo; *p; p++) {
        if (*p >= 'A' && *p <= 'Z') *p = (char)(*p + 32);
    }
    size_t sg_cap = strlen(method_lo) + strlen(parsed.path) + strlen(host_hdr) + strlen(date) + 64;
    char *signing = (char *)malloc(sg_cap);
    snprintf(signing, sg_cap, "(request-target): %s %s\nhost: %s\ndate: %s",
             method_lo, parsed.path, host_hdr, date);
    unsigned char sig_raw[32];
    nv_hmac_sha256((const unsigned char *)key, strlen(key), (const unsigned char *)signing, strlen(signing), sig_raw);
    char *sig_b64 = nv4_b64_encode(sig_raw, 32);
    size_t au_cap = strlen(key_id) + strlen(sig_b64) + 128;
    char *auth = (char *)malloc(au_cap);
    snprintf(auth, au_cap, "Signature keyId=\"%s\",algorithm=\"hmac-sha256\",headers=\"(request-target) host date\",signature=\"%s\"",
             key_id, sig_b64);
    *sig_out = auth;
    free(sig_b64);
    free(signing);
    free(method_lo);
    nv_url_free(&parsed);
}


// ---------- لينكس8-طبقة4: مغلفات جلسات/نقية ----------
static NvValue nv4_session_new_fn(NvValue *args, int nargs, int line, int col) {
    if (nargs != 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.جلسة_جديدة تتوقع 0 معاملات لكن أعطيت %d", nargs);
    }
    (void)args;
    NvValue out = nv_dict_new();
    nv_dict_set_raw(out.u.dict, "كوكيز", nv_dict_new());
    nv_dict_set_raw(out.u.dict, "رؤوس", nv_dict_new());
    return out;
}

static NvValue nv4_session_named(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.جلسة تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.جلسة تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[0].u.str->len == 0) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.جلسة تتوقع اسما غير فارغ", line, col);
    }
    NvValue out = nv_dict_new();
    nv_dict_set_raw(out.u.dict, "اسم", nv_str_cstr(args[0].u.str->data));
    nv_dict_set_raw(out.u.dict, "كوكيز", nv_dict_new());
    nv_dict_set_raw(out.u.dict, "رؤوس", nv_dict_new());
    return out;
}

static NvValue nv4_apply_session(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.طبق_جلسة تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.طبق_جلسة تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[1].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.طبق_جلسة تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[1]));
    }
    NvValue out = nv_dict_new();
    NvDict *req = args[0].u.dict;
    for (int i = 0; i < req->len; i++) {
        nv_dict_set_raw(out.u.dict, req->entries[i].key, req->entries[i].val);
    }
    NvDict *sess = args[1].u.dict;
    const char *keys[2] = {"كوكيز", "رؤوس"};
    for (int k = 0; k < 2; k++) {
        Nv4Pairs merged;
        merged.items = NULL;
        merged.len = 0;
        merged.cap = 0;
        NvValue *existing = nv4_dict_get(out.u.dict, keys[k]);
        if (existing && existing->tag == NV_DICT) {
            NvDict *d = existing->u.dict;
            for (int i = 0; i < d->len; i++) {
                if (d->entries[i].val.tag == NV_STR) {
                    nv4_pairs_push(&merged, d->entries[i].key, d->entries[i].val.u.str->data);
                }
            }
        }
        NvValue *sv = nv4_dict_get(sess, keys[k]);
        if (sv && sv->tag == NV_DICT) {
            NvDict *d = sv->u.dict;
            for (int i = 0; i < d->len; i++) {
                if (d->entries[i].val.tag == NV_STR && nv4_pairs_find(&merged, d->entries[i].key) < 0) {
                    nv4_pairs_push(&merged, d->entries[i].key, d->entries[i].val.u.str->data);
                }
            }
        }
        nv_dict_set_raw(out.u.dict, keys[k], nv4_pairs_to_dict(&merged));
    }
    return out;
}

static NvValue nv4_save_session(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.احفظ_الجلسة تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.احفظ_الجلسة تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *path = nv2_path_arg(args, 1, "احفظ_الجلسة", line, col);
    Nv4Pairs cookies;
    cookies.items = NULL;
    cookies.len = 0;
    cookies.cap = 0;
    Nv4Pairs headers;
    headers.items = NULL;
    headers.len = 0;
    headers.cap = 0;
    NvValue *cv = nv4_dict_get(args[0].u.dict, "كوكيز");
    if (cv && cv->tag == NV_DICT) {
        NvDict *d = cv->u.dict;
        for (int i = 0; i < d->len; i++) {
            if (d->entries[i].val.tag == NV_STR) {
                nv4_pairs_push(&cookies, d->entries[i].key, d->entries[i].val.u.str->data);
            }
        }
    }
    NvValue *hv = nv4_dict_get(args[0].u.dict, "رؤوس");
    if (hv && hv->tag == NV_DICT) {
        NvDict *d = hv->u.dict;
        for (int i = 0; i < d->len; i++) {
            if (d->entries[i].val.tag == NV_STR) {
                nv4_pairs_push(&headers, d->entries[i].key, d->entries[i].val.u.str->data);
            }
        }
    }
    char *text = nv4_session_text(&cookies, &headers);
    nv2_write_file(path, (const unsigned char *)text, strlen(text), "احفظ_الجلسة", line, col);
    free(text);
    return nv_bool(1);
}

static NvValue nv4_read_session(NvValue *args, int nargs, int line, int col) {
    if (nargs < 1 || nargs > 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اقرأ_الجلسة تتوقع 1 أو 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *path = nv2_path_arg(args, 0, "اقرأ_الجلسة", line, col);
    int readonly = 0;
    if (nargs == 2) {
        if (args[1].tag != NV_BOOL) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اقرأ_الجلسة تتوقع منطقيا لكن أعطيت %s", nv_type_name(args[1]));
        }
        readonly = args[1].u.boolean;
    }
    size_t len = 0;
    unsigned char *bytes = nv4_read_opt(path, &len, line, col);
    if (!bytes) {
        if (readonly) {
            nv_error(NV_ERR_RUNTIME, "الدالة شبكة.اقرأ_الجلسة الملف غير موجود", line, col);
        }
        bytes = (unsigned char *)strdup("");
        len = 0;
    }
    char *text = (char *)malloc(len + 1);
    memcpy(text, bytes, len);
    text[len] = '\0';
    free(bytes);
    Nv4Pairs cookies;
    cookies.items = NULL;
    cookies.len = 0;
    cookies.cap = 0;
    Nv4Pairs headers;
    headers.items = NULL;
    headers.len = 0;
    headers.cap = 0;
    nv4_parse_session_text(text, &cookies, &headers);
    free(text);
    NvValue out = nv_dict_new();
    nv_dict_set_raw(out.u.dict, "طريقة", nv_str_cstr("GET"));
    nv_dict_set_raw(out.u.dict, "كوكيز", nv4_pairs_to_dict(&cookies));
    nv_dict_set_raw(out.u.dict, "رؤوس", nv4_pairs_to_dict(&headers));
    return out;
}

static NvValue nv4_read_cookies(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اقرأ_كوكيز تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *path = nv2_path_arg(args, 0, "اقرأ_كوكيز", line, col);
    Nv2Bytes b = nv2_read_file(path, "اقرأ_كوكيز", line, col);
    char *text = (char *)malloc(b.len + 1);
    memcpy(text, b.data, b.len);
    text[b.len] = '\0';
    free(b.data);
    NvValue out = nv_dict_new();
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (llen > 0 && line[llen - 1] == '\r') line[llen - 1] = '\0';
        if (line[0] != '\0' && line[0] != '#') {
            int nf = 0;
            char *fields[8];
            char *save = NULL;
            char *tok = strtok_r(line, "\t", &save);
            while (tok && nf < 8) {
                fields[nf++] = tok;
                tok = strtok_r(NULL, "\t", &save);
            }
            if (nf >= 7 && nv_dict_find(out.u.dict, fields[5]) < 0) {
                nv_dict_set_raw(out.u.dict, fields[5], nv_str_cstr(fields[6]));
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return out;
}

static NvValue nv4_save_tag(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.احفظ_الوسم تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.احفظ_الوسم", line, col, &status, &headers, &body);
    (void)status;
    free(body);
    const char *path = nv2_path_arg(args, 1, "احفظ_الوسم", line, col);
    const char *tag = NULL;
    for (int i = 0; i < headers.len; i++) {
        char *lower = strdup(headers.items[i].key);
        for (char *q = lower; *q; q++) {
            if (*q >= 'A' && *q <= 'Z') *q = (char)(*q + 32);
        }
        if (strcmp(lower, "etag") == 0) {
            tag = headers.items[i].val;
        }
        free(lower);
        if (tag) break;
    }
    if (!tag) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.احفظ_الوسم لا يوجد وسم في الرد", line, col);
    }
    nv2_write_file(path, (const unsigned char *)tag, strlen(tag), "احفظ_الوسم", line, col);
    return nv_str_cstr(tag);
}

static NvValue nv4_read_tag(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اقرأ_الوسم تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *path = nv2_path_arg(args, 0, "اقرأ_الوسم", line, col);
    size_t len = 0;
    unsigned char *bytes = nv4_read_opt(path, &len, line, col);
    if (!bytes) {
        return nv_nil();
    }
    while (len > 0 && (bytes[len - 1] == ' ' || bytes[len - 1] == '\t' || bytes[len - 1] == '\n' || bytes[len - 1] == '\r')) len--;
    size_t start = 0;
    while (start < len && (bytes[start] == ' ' || bytes[start] == '\t' || bytes[start] == '\n' || bytes[start] == '\r')) start++;
    NvValue out = nv_str_new((const char *)bytes + start, len - start);
    free(bytes);
    return out;
}

static NvValue nv4_read_credentials(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اقرأ_بيانات_الدخول تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *path = nv2_path_arg(args, 0, "اقرأ_بيانات_الدخول", line, col);
    Nv2Bytes b = nv2_read_file(path, "اقرأ_بيانات_الدخول", line, col);
    char *text = (char *)malloc(b.len + 1);
    memcpy(text, b.data, b.len);
    text[b.len] = '\0';
    free(b.data);
    NvValue out = nv_dict_new();
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (llen > 0 && line[llen - 1] == '\r') line[llen - 1] = '\0';
        char *t = line;
        while (*t == ' ' || *t == '\t') t++;
        if (*t != '\0' && *t != '#') {
            char *save = NULL;
            char *host = strtok_r(t, " \t", &save);
            char *user = host ? strtok_r(NULL, " \t", &save) : NULL;
            char *pass = user ? strtok_r(NULL, "", &save) : NULL;
            if (host && user && pass && nv_dict_find(out.u.dict, host) < 0) {
                while (*pass == ' ' || *pass == '\t') pass++;
                NvValue cred = nv_dict_new();
                nv_dict_set_raw(cred.u.dict, "مستخدم", nv_str_cstr(user));
                nv_dict_set_raw(cred.u.dict, "كلمة_سر", nv_str_cstr(pass));
                nv_dict_set_raw(out.u.dict, host, cred);
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return out;
}

static NvValue nv4_read_dl_opts(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اقرأ_إعدادات_التنزيل تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *path = nv2_path_arg(args, 0, "اقرأ_إعدادات_التنزيل", line, col);
    Nv2Bytes b = nv2_read_file(path, "اقرأ_إعدادات_التنزيل", line, col);
    char *text = (char *)malloc(b.len + 1);
    memcpy(text, b.data, b.len);
    text[b.len] = '\0';
    free(b.data);
    NvValue out = nv_dict_new();
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (llen > 0 && line[llen - 1] == '\r') line[llen - 1] = '\0';
        char *t = line;
        while (*t == ' ' || *t == '\t') t++;
        if (*t != '\0' && *t != '#') {
            char *eq = strchr(t, '=');
            if (eq) {
                *eq = '\0';
                char *k = t;
                char *kend = k + strlen(k);
                while (kend > k && (kend[-1] == ' ' || kend[-1] == '\t')) *--kend = '\0';
                char *v = eq + 1;
                while (*v == ' ' || *v == '\t') v++;
                char *vend = v + strlen(v);
                while (vend > v && (vend[-1] == ' ' || vend[-1] == '\t')) *--vend = '\0';
                if (*k != '\0' && nv_dict_find(out.u.dict, k) < 0) {
                    if (strcmp(v, "صحيح") == 0) {
                        nv_dict_set_raw(out.u.dict, k, nv_bool(1));
                    } else if (strcmp(v, "خطأ") == 0) {
                        nv_dict_set_raw(out.u.dict, k, nv_bool(0));
                    } else {
                        char *end = NULL;
                        double n = strtod(v, &end);
                        if (end && *end == '\0' && end != v && isfinite(n)) {
                            nv_dict_set_raw(out.u.dict, k, nv_num(n));
                        } else {
                            nv_dict_set_raw(out.u.dict, k, nv_str_cstr(v));
                        }
                    }
                }
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return out;
}

static NvValue nv4_write_dl_opts(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اكتب_إعدادات_التنزيل تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اكتب_إعدادات_التنزيل تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *path = nv2_path_arg(args, 1, "اكتب_إعدادات_التنزيل", line, col);
    Nv4Buf out;
    out.buf = NULL;
    out.len = 0;
    out.cap = 0;
    NvDict *d = args[0].u.dict;
    for (int i = 0; i < d->len; i++) {
        NvValue v = d->entries[i].val;
        nv4_buf_adds(&out, d->entries[i].key);
        nv4_buf_addc(&out, '=');
        if (v.tag == NV_BOOL) {
            nv4_buf_adds(&out, v.u.boolean ? "صحيح" : "خطأ");
        } else if (v.tag == NV_NUM) {
            nv4_json_num(&out, v.u.num);
        } else if (v.tag == NV_STR) {
            nv4_buf_addn(&out, v.u.str->data, v.u.str->len);
        } else {
            nv_error(NV_ERR_TYPE, "الدالة شبكة.اكتب_إعدادات_التنزيل قيمة غير صالحة", line, col);
        }
        nv4_buf_addc(&out, '\n');
    }
    if (!out.buf) {
        out.buf = strdup("");
    }
    nv2_write_file(path, (const unsigned char *)out.buf, strlen(out.buf), "اكتب_إعدادات_التنزيل", line, col);
    free(out.buf);
    return nv_bool(1);
}

static NvValue nv4_url_encode_fn(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.رمز_العنوان تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.رمز_العنوان تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    char *enc = nv2_url_encode(args[0].u.str->data);
    NvValue out = nv_str_cstr(enc);
    free(enc);
    return out;
}

static NvValue nv4_add_param(NvValue *args, int nargs, int line, int col) {
    if (nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أضف_معامل تتوقع 3 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أضف_معامل تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أضف_معامل تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[1].u.str->len == 0) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.أضف_معامل تتوقع اسما غير فارغ", line, col);
    }
    char *value = NULL;
    if (args[2].tag == NV_STR) {
        value = strdup(args[2].u.str->data);
    } else if (args[2].tag == NV_NUM) {
        Nv4Buf tmp;
        tmp.buf = NULL;
        tmp.len = 0;
        tmp.cap = 0;
        nv4_json_num(&tmp, args[2].u.num);
        value = tmp.buf ? tmp.buf : strdup("");
    } else if (args[2].tag == NV_BOOL) {
        value = strdup(args[2].u.boolean ? "true" : "false");
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أضف_معامل تتوقع قيمة نصا أو عددا أو منطقيا لكن أعطيت %s", nv_type_name(args[2]));
    }
    const char *url = args[0].u.str->data;
    const char *hash = strchr(url, '#');
    char *base = hash ? (char *)malloc((size_t)(hash - url) + 1) : strdup(url);
    if (hash) {
        memcpy(base, url, (size_t)(hash - url));
        base[hash - url] = '\0';
    }
    const char *sep = strchr(base, '?') ? "&" : "?";
    char *enc_name = nv2_url_encode(args[1].u.str->data);
    char *enc_val = nv2_url_encode(value);
    size_t cap = strlen(base) + strlen(sep) + strlen(enc_name) + strlen(enc_val) + (hash ? strlen(hash) : 0) + 2;
    char *out = (char *)malloc(cap);
    snprintf(out, cap, "%s%s%s=%s", base, sep, enc_name, enc_val);
    if (hash) {
        strcat(out, hash);
    }
    NvValue ret = nv_str_cstr(out);
    free(out);
    free(enc_name);
    free(enc_val);
    free(value);
    free(base);
    return ret;
}

static NvValue nv4_resp_headers(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.رؤوس_الرد تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.رؤوس_الرد", line, col, &status, &headers, &body);
    (void)status;
    free(body);
    return nv4_pairs_to_dict(&headers);
}

static NvValue nv4_headers_only(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.رؤوس_فقط تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.رؤوس_فقط", line, col, &status, &headers, &body);
    (void)status;
    free(body);
    char *text = nv4_headers_text(&headers);
    NvValue out = nv_str_cstr(text);
    free(text);
    return out;
}

static NvValue nv4_body_only(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.جسم_فقط تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.جسم_فقط", line, col, &status, &headers, &body);
    (void)status;
    NvValue out = nv_str_cstr(body);
    free(body);
    return out;
}

static NvValue nv4_show_resp(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اعرض_الرد تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.اعرض_الرد", line, col, &status, &headers, &body);
    (void)status;
    char *h = nv4_headers_text(&headers);
    NvValue out;
    if (h[0] == '\0') {
        out = nv_str_cstr(body);
    } else {
        size_t cap = strlen(h) + strlen(body) + 3;
        char *text = (char *)malloc(cap);
        snprintf(text, cap, "%s\n\n%s", h, body);
        out = nv_str_cstr(text);
        free(text);
    }
    free(h);
    free(body);
    return out;
}

static NvValue nv4_show_resp_color(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اعرض_ملونا تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.اعرض_ملونا", line, col, &status, &headers, &body);
    Nv4Buf out;
    out.buf = NULL;
    out.len = 0;
    out.cap = 0;
    char tmp[64];
    snprintf(tmp, sizeof(tmp), "\x1b[1;32mرمز %d\x1b[0m\n", status);
    nv4_buf_adds(&out, tmp);
    for (int i = 0; i < headers.len; i++) {
        nv4_buf_adds(&out, "\x1b[36m");
        nv4_buf_adds(&out, headers.items[i].key);
        nv4_buf_adds(&out, "\x1b[0m: ");
        nv4_buf_adds(&out, headers.items[i].val);
        nv4_buf_addc(&out, '\n');
    }
    nv4_buf_addc(&out, '\n');
    nv4_buf_adds(&out, body);
    if (!out.buf) {
        out.buf = strdup("");
    }
    NvValue ret = nv_str_cstr(out.buf);
    free(out.buf);
    free(body);
    return ret;
}

static NvValue nv4_format_json(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نسق_json تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.نسق_json", line, col, &status, &headers, &body);
    (void)status;
    NvValue parsed;
    if (!nv4_json_parse_doc(body, &parsed)) {
        free(body);
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.نسق_json جسم ليس JSON صالحا", line, col);
    }
    free(body);
    Nv4Buf out;
    out.buf = NULL;
    out.len = 0;
    out.cap = 0;
    if (!nv4_json_pretty(parsed, &out, 0)) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.نسق_json قيمة لا تقبل كتابة JSON", line, col);
    }
    if (!out.buf) {
        out.buf = strdup("");
    }
    NvValue ret = nv_str_cstr(out.buf);
    free(out.buf);
    return ret;
}

static NvValue nv4_save_headers(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.احفظ_الرؤوس تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    int status = 0;
    Nv4Pairs headers;
    char *body = NULL;
    nv4_resp_parts(args[0], "شبكة.احفظ_الرؤوس", line, col, &status, &headers, &body);
    (void)status;
    free(body);
    const char *path = nv2_path_arg(args, 1, "احفظ_الرؤوس", line, col);
    char *text = nv4_headers_text(&headers);
    double n = (double)strlen(text);
    nv2_write_file(path, (const unsigned char *)text, strlen(text), "احفظ_الرؤوس", line, col);
    free(text);
    return nv_num(n);
}

static NvValue nv4_sign_amazon(NvValue *args, int nargs, int line, int col) {
    if (nargs < 3 || nargs > 5) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.وقع_أمازون تتوقع 3 إلى 5 معاملات لكن أعطيت %d", nargs);
    }
    char *url = NULL;
    char *method = NULL;
    Nv2Bytes body;
    nv4_req_parts(args[0], "شبكة.وقع_أمازون", line, col, &url, &method, &body);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_أمازون تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[2].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_أمازون تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
    }
    const char *region = "us-east-1";
    const char *service = "s3";
    if (nargs >= 4) {
        if (args[3].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_أمازون تتوقع نصا لكن أعطيت %s", nv_type_name(args[3]));
        }
        region = args[3].u.str->data;
    }
    if (nargs >= 5) {
        if (args[4].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_أمازون تتوقع نصا لكن أعطيت %s", nv_type_name(args[4]));
        }
        service = args[4].u.str->data;
    }
    Nv4Pairs out;
    out.items = NULL;
    out.len = 0;
    out.cap = 0;
    nv4_sign_amazon_core(method, url, body.data, body.len, args[1].u.str->data, args[2].u.str->data, region, service, line, col, &out);
    free(url);
    free(method);
    free(body.data);
    return nv4_pairs_to_dict(&out);
}

static NvValue nv4_sign_request(NvValue *args, int nargs, int line, int col) {
    if (nargs < 3 || nargs > 4) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.وقع_الطلب تتوقع 3 أو 4 معاملات لكن أعطيت %d", nargs);
    }
    char *url = NULL;
    char *method = NULL;
    Nv2Bytes body;
    nv4_req_parts(args[0], "شبكة.وقع_الطلب", line, col, &url, &method, &body);
    free(body.data);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_الطلب تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[2].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_الطلب تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
    }
    const char *algorithm = "hmac-sha256";
    if (nargs >= 4) {
        if (args[3].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.وقع_الطلب تتوقع نصا لكن أعطيت %s", nv_type_name(args[3]));
        }
        algorithm = args[3].u.str->data;
    }
    char *sig = NULL;
    nv4_sign_httpsig(method, url, args[1].u.str->data, args[2].u.str->data, algorithm, line, col, &sig);
    free(url);
    free(method);
    NvValue ret = nv_str_cstr(sig);
    free(sig);
    return ret;
}

static NvValue nv4_write_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اكتب_الفحص تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اكتب_الفحص تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *path = nv2_path_arg(args, 1, "اكتب_الفحص", line, col);
    nv2_write_file(path, (const unsigned char *)args[0].u.str->data, args[0].u.str->len, "اكتب_الفحص", line, col);
    return nv_bool(1);
}


// ---------- لينكس8-طبقة5أ: معلومات محلية (اجتماع 90 §105) ----------
static char *nv5_read_file(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    size_t cap = 4096;
    size_t len = 0;
    char *buf = (char *)malloc(cap);
    while (1) {
        if (len + 1024 >= cap) {
            cap *= 2;
            buf = (char *)realloc(buf, cap);
        }
        size_t n = fread(buf + len, 1, cap - len - 1, f);
        if (n == 0) break;
        len += n;
    }
    fclose(f);
    buf[len] = '\0';
    return buf;
}

static char *nv5_sys(const char *iface, const char *file) {
    char path[512];
    snprintf(path, sizeof(path), "/sys/class/net/%s/%s", iface, file);
    char *text = nv5_read_file(path);
    if (!text) return NULL;
    size_t len = strlen(text);
    while (len > 0 && (text[len - 1] == ' ' || text[len - 1] == '\t' || text[len - 1] == '\n' || text[len - 1] == '\r')) {
        text[--len] = '\0';
    }
    char *start = text;
    while (*start == ' ' || *start == '\t') start++;
    if (start != text) {
        memmove(text, start, strlen(start) + 1);
    }
    return text;
}

static void nv5_ipv4_str(const unsigned char o[4], char *out) {
    snprintf(out, 64, "%u.%u.%u.%u", o[0], o[1], o[2], o[3]);
}

static unsigned int nv5_count_bits(const unsigned char *b, size_t n) {
    unsigned int c = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char x = b[i];
        while (x) {
            c += (unsigned int)(x & 1);
            x >>= 1;
        }
    }
    return c;
}

static void nv5_hex_ip_le(const char *hex, char *out) {
    unsigned long v = strtoul(hex, NULL, 16);
    unsigned char o[4];
    o[0] = (unsigned char)(v & 0xFF);
    o[1] = (unsigned char)((v >> 8) & 0xFF);
    o[2] = (unsigned char)((v >> 16) & 0xFF);
    o[3] = (unsigned char)((v >> 24) & 0xFF);
    nv5_ipv4_str(o, out);
}

// البث: لينكس يسميه ifa_ifu.ifu_broadaddr، وماك/BSD يسميه
// ifa_broadaddr مباشرة (الاتحاد بلا اسم هناك).
#if defined(__APPLE__) || defined(__FreeBSD__) || defined(__NetBSD__) || defined(__OpenBSD__)
#define NV5_BROADADDR(ifa) ((ifa)->ifa_broadaddr)
#else
#define NV5_BROADADDR(ifa) ((ifa)->ifa_ifu.ifu_broadaddr)
#endif
typedef struct { char addr[64]; char mask[64]; char bcast[64]; int has_bcast; int v6; } Nv5Addr;
typedef struct { char name[64]; Nv5Addr *addrs; int naddrs; int cap; int up; int loopback; } Nv5Iface;

static Nv5Iface *nv5_iface_find(Nv5Iface *list, int n, const char *name) {
    for (int i = 0; i < n; i++) {
        if (strcmp(list[i].name, name) == 0) return &list[i];
    }
    return NULL;
}

static int nv5_ifaces(Nv5Iface **out, int *n_out) {
    struct ifaddrs *ifap = NULL;
    if (getifaddrs(&ifap) != 0 || !ifap) return 0;
    Nv5Iface *list = NULL;
    int n = 0;
    int cap = 0;
    for (struct ifaddrs *ifa = ifap; ifa; ifa = ifa->ifa_next) {
        if (!ifa->ifa_name) continue;
        Nv5Iface *e = nv5_iface_find(list, n, ifa->ifa_name);
        if (!e) {
            if (n >= cap) {
                cap = cap ? cap * 2 : 8;
                list = (Nv5Iface *)realloc(list, (size_t)cap * sizeof(Nv5Iface));
            }
            e = &list[n++];
            memset(e, 0, sizeof(*e));
            snprintf(e->name, sizeof(e->name), "%s", ifa->ifa_name);
        }
        unsigned int flags = ifa->ifa_flags;
        e->up = (flags & IFF_UP) != 0;
        e->loopback = (flags & IFF_LOOPBACK) != 0;
        if (!ifa->ifa_addr) continue;
        if (ifa->ifa_addr->sa_family == AF_INET) {
            struct sockaddr_in *sin = (struct sockaddr_in *)ifa->ifa_addr;
            unsigned char oct[4];
            memcpy(oct, &sin->sin_addr.s_addr, 4);
            if (e->naddrs >= e->cap) {
                e->cap = e->cap ? e->cap * 2 : 4;
                e->addrs = (Nv5Addr *)realloc(e->addrs, (size_t)e->cap * sizeof(Nv5Addr));
            }
            Nv5Addr *a = &e->addrs[e->naddrs++];
            memset(a, 0, sizeof(*a));
            nv5_ipv4_str(oct, a->addr);
            if (ifa->ifa_netmask && ifa->ifa_netmask->sa_family == AF_INET) {
                struct sockaddr_in *msin = (struct sockaddr_in *)ifa->ifa_netmask;
                unsigned char moct[4];
                memcpy(moct, &msin->sin_addr.s_addr, 4);
                nv5_ipv4_str(moct, a->mask);
            } else {
                snprintf(a->mask, sizeof(a->mask), "255.255.255.255");
            }
            if ((flags & IFF_BROADCAST) != 0 && NV5_BROADADDR(ifa)
                && NV5_BROADADDR(ifa)->sa_family == AF_INET) {
                struct sockaddr_in *bsin = (struct sockaddr_in *)NV5_BROADADDR(ifa);
                unsigned char boct[4];
                memcpy(boct, &bsin->sin_addr.s_addr, 4);
                nv5_ipv4_str(boct, a->bcast);
                a->has_bcast = 1;
            }
            a->v6 = 0;
        } else if (ifa->ifa_addr->sa_family == AF_INET6) {
            struct sockaddr_in6 *sin6 = (struct sockaddr_in6 *)ifa->ifa_addr;
            if (e->naddrs >= e->cap) {
                e->cap = e->cap ? e->cap * 2 : 4;
                e->addrs = (Nv5Addr *)realloc(e->addrs, (size_t)e->cap * sizeof(Nv5Addr));
            }
            Nv5Addr *a = &e->addrs[e->naddrs++];
            memset(a, 0, sizeof(*a));
            inet_ntop(AF_INET6, &sin6->sin6_addr, a->addr, sizeof(a->addr));
            unsigned int prefix = 128;
            if (ifa->ifa_netmask && ifa->ifa_netmask->sa_family == AF_INET6) {
                struct sockaddr_in6 *msin6 = (struct sockaddr_in6 *)ifa->ifa_netmask;
                prefix = nv5_count_bits(msin6->sin6_addr.s6_addr, 16);
            }
            snprintf(a->mask, sizeof(a->mask), "%u", prefix);
            a->v6 = 1;
        }
    }
    freeifaddrs(ifap);
    *out = list;
    *n_out = n;
    return 1;
}

static NvValue nv5_iface_value(Nv5Iface *e, Nv5Addr *a) {
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(e->name));
    nv_dict_set_raw(d.u.dict, "عنوان", nv_str_cstr(a->addr));
    nv_dict_set_raw(d.u.dict, "قناع", nv_str_cstr(a->mask));
    if (a->has_bcast) {
        nv_dict_set_raw(d.u.dict, "بث", nv_str_cstr(a->bcast));
    } else {
        nv_dict_set_raw(d.u.dict, "بث", nv_nil());
    }
    nv_dict_set_raw(d.u.dict, "يعمل", nv_bool(e->up));
    nv_dict_set_raw(d.u.dict, "حلقي", nv_bool(e->loopback));
    return d;
}

typedef struct { char dest[64]; char gw[64]; char mask[64]; char iface[64]; } Nv5Route;

static int nv5_split(char *line, char **fields, int maxf) {
    int n = 0;
    char *save = NULL;
    char *tok = strtok_r(line, " \t", &save);
    while (tok && n < maxf) {
        fields[n++] = tok;
        tok = strtok_r(NULL, " \t", &save);
    }
    return n;
}

static int nv5_routes(Nv5Route **out, int *n_out) {
    char *text = nv5_read_file("/proc/net/route");
    if (!text) return 0;
    Nv5Route *list = NULL;
    int n = 0;
    int cap = 0;
    int first = 1;
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (first) {
            first = 0;
        } else {
            char *fields[16];
            int nf = nv5_split(line, fields, 16);
            if (nf >= 8) {
                if (n >= cap) {
                    cap = cap ? cap * 2 : 8;
                    list = (Nv5Route *)realloc(list, (size_t)cap * sizeof(Nv5Route));
                }
                Nv5Route *r = &list[n++];
                nv5_hex_ip_le(fields[1], r->dest);
                nv5_hex_ip_le(fields[2], r->gw);
                nv5_hex_ip_le(fields[7], r->mask);
                snprintf(r->iface, sizeof(r->iface), "%s", fields[0]);
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    *out = list;
    *n_out = n;
    return 1;
}

static NvValue nv5_route_value(Nv5Route *r) {
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "وجهة", nv_str_cstr(r->dest));
    nv_dict_set_raw(d.u.dict, "بوابة", nv_str_cstr(r->gw));
    nv_dict_set_raw(d.u.dict, "قناع", nv_str_cstr(r->mask));
    nv_dict_set_raw(d.u.dict, "واجهة", nv_str_cstr(r->iface));
    return d;
}

typedef struct { char name[64]; unsigned long long rx_bytes; unsigned long long rx_packets; unsigned long long rx_errs; unsigned long long rx_drop; unsigned long long tx_bytes; unsigned long long tx_packets; unsigned long long tx_errs; unsigned long long tx_drop; } Nv5Dev;

static int nv5_dev(Nv5Dev **out, int *n_out) {
    char *text = nv5_read_file("/proc/net/dev");
    if (!text) return 0;
    Nv5Dev *list = NULL;
    int n = 0;
    int cap = 0;
    int skipped = 0;
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (skipped < 2) {
            skipped++;
        } else {
            char *colon = strchr(line, ':');
            if (colon) {
                *colon = '\0';
                char *name = line;
                while (*name == ' ' || *name == '\t') name++;
                char *fields[24];
                int nf = nv5_split(colon + 1, fields, 24);
                if (nf >= 16) {
                    if (n >= cap) {
                        cap = cap ? cap * 2 : 8;
                        list = (Nv5Dev *)realloc(list, (size_t)cap * sizeof(Nv5Dev));
                    }
                    Nv5Dev *d = &list[n++];
                    memset(d, 0, sizeof(*d));
                    snprintf(d->name, sizeof(d->name), "%s", name);
                    d->rx_bytes = strtoull(fields[0], NULL, 10);
                    d->rx_packets = strtoull(fields[1], NULL, 10);
                    d->rx_errs = strtoull(fields[2], NULL, 10);
                    d->rx_drop = strtoull(fields[3], NULL, 10);
                    d->tx_bytes = strtoull(fields[8], NULL, 10);
                    d->tx_packets = strtoull(fields[9], NULL, 10);
                    d->tx_errs = strtoull(fields[10], NULL, 10);
                    d->tx_drop = strtoull(fields[11], NULL, 10);
                }
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    *out = list;
    *n_out = n;
    return 1;
}

static const char *nv5_tcp_state(const char *code, char *buf) {
    if (strcmp(code, "01") == 0) return "متصل";
    if (strcmp(code, "02") == 0) return "مزامنة-مرسل";
    if (strcmp(code, "03") == 0) return "مزامنة-مستلم";
    if (strcmp(code, "04") == 0) return "مغلق-انتظار";
    if (strcmp(code, "05") == 0) return "انتظار-إغلاق";
    if (strcmp(code, "06") == 0) return "مغلق";
    if (strcmp(code, "07") == 0) return "إغلاق";
    if (strcmp(code, "08") == 0) return "إغلاق-انتظار";
    if (strcmp(code, "09") == 0) return "إغلاق-أخير";
    if (strcmp(code, "0A") == 0) return "يستمع";
    if (strcmp(code, "0B") == 0) return "مغلق-جديد";
    snprintf(buf, 32, "حالة %s", code);
    return buf;
}

static void nv5_sock_addr(const char *hex, char *ip_out, int *port_out) {
    const char *colon = strchr(hex, ':');
    if (!colon) {
        snprintf(ip_out, 64, "0.0.0.0");
        *port_out = 0;
        return;
    }
    *port_out = (int)strtoul(colon + 1, NULL, 16);
    size_t iplen = (size_t)(colon - hex);
    if (iplen == 8) {
        char tmp[9];
        memcpy(tmp, hex, 8);
        tmp[8] = '\0';
        nv5_hex_ip_le(tmp, ip_out);
        return;
    }
    if (iplen == 32) {
        unsigned char oct[16];
        for (int w = 0; w < 4; w++) {
            for (int b = 0; b < 4; b++) {
                char tmp[3];
                tmp[0] = hex[w * 8 + b * 2];
                tmp[1] = hex[w * 8 + b * 2 + 1];
                tmp[2] = '\0';
                oct[w * 4 + (3 - b)] = (unsigned char)strtoul(tmp, NULL, 16);
            }
        }
        inet_ntop(AF_INET6, oct, ip_out, 64);
        return;
    }
    snprintf(ip_out, 64, "0.0.0.0");
}

static NvValue nv5_conns(void) {
    NvValue list = nv_list_new();
    const char *paths[4] = {"/proc/net/tcp", "/proc/net/tcp6", "/proc/net/udp", "/proc/net/udp6"};
    const char *protos[4] = {"TCP", "TCP", "UDP", "UDP"};
    for (int pi = 0; pi < 4; pi++) {
        char *text = nv5_read_file(paths[pi]);
        if (!text) continue;
        int first = 1;
        char *p = text;
        while (*p) {
            char *eol = strchr(p, '\n');
            size_t llen = eol ? (size_t)(eol - p) : strlen(p);
            char *line = (char *)malloc(llen + 1);
            memcpy(line, p, llen);
            line[llen] = '\0';
            if (first) {
                first = 0;
            } else {
                char *fields[16];
                int nf = nv5_split(line, fields, 16);
                if (nf >= 4) {
                    char lip[64];
                    char rip[64];
                    int lport = 0;
                    int rport = 0;
                    nv5_sock_addr(fields[1], lip, &lport);
                    nv5_sock_addr(fields[2], rip, &rport);
                    const char *state;
                    char statebuf[32];
                    if (strcmp(protos[pi], "UDP") == 0) {
                        state = "بلا حالة";
                    } else {
                        state = nv5_tcp_state(fields[3], statebuf);
                    }
                    char local[128];
                    char remote[128];
                    snprintf(local, sizeof(local), "%s:%d", lip, lport);
                    snprintf(remote, sizeof(remote), "%s:%d", rip, rport);
                    NvValue d = nv_dict_new();
                    nv_dict_set_raw(d.u.dict, "بروتوكول", nv_str_cstr(protos[pi]));
                    nv_dict_set_raw(d.u.dict, "محلي", nv_str_cstr(local));
                    nv_dict_set_raw(d.u.dict, "بعيد", nv_str_cstr(remote));
                    nv_dict_set_raw(d.u.dict, "حالة", nv_str_cstr(state));
                    nv_list_push_raw(list.u.list, d);
                }
            }
            free(line);
            if (!eol) break;
            p = eol + 1;
        }
        free(text);
    }
    return list;
}

static unsigned long long nv5_snmp_num(const char *section, const char *key) {
    char *text = nv5_read_file("/proc/net/snmp");
    if (!text) return 0;
    unsigned long long result = 0;
    char *save = NULL;
    char *line = strtok_r(text, "\n", &save);
    while (line) {
        if (strncmp(line, section, strlen(section)) == 0) {
            char *keys = strdup(line + strlen(section));
            char *vals_line = strtok_r(NULL, "\n", &save);
            if (!vals_line) {
                free(keys);
                break;
            }
            char *vals = strdup(vals_line + strlen(section));
            char *ksave = NULL;
            char *vsave = NULL;
            char *k = strtok_r(keys, " \t", &ksave);
            char *v = strtok_r(vals, " \t", &vsave);
            while (k && v) {
                if (strcmp(k, key) == 0) {
                    result = strtoull(v, NULL, 10);
                    break;
                }
                k = strtok_r(NULL, " \t", &ksave);
                v = strtok_r(NULL, " \t", &vsave);
            }
            free(keys);
            free(vals);
            break;
        }
        line = strtok_r(NULL, "\n", &save);
    }
    free(text);
    return result;
}


static void nv5_need_text(NvValue *args, int nargs, const char *name, const char *empty_msg, int line, int col, const char **out) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع نصا لكن أعطيت %s", name, nv_type_name(args[0]));
    }
    if (args[0].u.str->len == 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s %s", name, empty_msg);
    }
    *out = args[0].u.str->data;
}

static void nv5_no_args(NvValue *args, int nargs, const char *name, int line, int col) {
    (void)args;
    if (nargs != 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
    }
}

static NvValue nv5_my_addrs(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "عناويني", line, col);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < list[i].naddrs; j++) {
            nv_list_push_raw(out.u.list, nv5_iface_value(&list[i], &list[i].addrs[j]));
        }
    }
    return out;
}

static NvValue nv5_iface_addr(NvValue *args, int nargs, int line, int col) {
    const char *name = NULL;
    nv5_need_text(args, nargs, "عنوان_واجهة", "تتوقع اسما غير فارغ", line, col, &name);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    Nv5Iface *e = nv5_iface_find(list, n, name);
    if (!e || e->naddrs == 0) return nv_nil();
    return nv5_iface_value(e, &e->addrs[0]);
}

static NvValue nv5_my_addrs_short(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "عناويني_مختصرة", line, col);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < list[i].naddrs; j++) {
            char buf[160];
            snprintf(buf, sizeof(buf), "%s: %s", list[i].name, list[i].addrs[j].addr);
            nv_list_push_raw(out.u.list, nv_str_cstr(buf));
        }
    }
    return out;
}

static NvValue nv5_my_addrs_json(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "عناويني_json", line, col);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    NvValue arr = nv_list_new();
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < list[i].naddrs; j++) {
            nv_list_push_raw(arr.u.list, nv5_iface_value(&list[i], &list[i].addrs[j]));
        }
    }
    char *buf = NULL;
    size_t len = 0;
    size_t cap = 0;
    nv2_json_write(arr, &buf, &len, &cap, 0, line, col);
    NvValue out = nv_str_new(buf ? buf : "", buf ? len : 0);
    free(buf);
    return out;
}

static NvValue nv5_my_routes(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "مساراتي", line, col);
    Nv5Route *list = NULL;
    int n = 0;
    nv5_routes(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        nv_list_push_raw(out.u.list, nv5_route_value(&list[i]));
    }
    return out;
}

static int nv5_parse_ipv4(const char *s, unsigned char o[4]) {
    char tmp[64];
    snprintf(tmp, sizeof(tmp), "%s", s);
    char *save = NULL;
    char *tok = strtok_r(tmp, ".", &save);
    for (int i = 0; i < 4; i++) {
        if (!tok) return 0;
        o[i] = (unsigned char)strtoul(tok, NULL, 10);
        tok = strtok_r(NULL, ".", &save);
    }
    return 1;
}

static NvValue nv5_route_for_host(NvValue *args, int nargs, int line, int col) {
    const char *host = NULL;
    nv5_need_text(args, nargs, "مسار_لمضيف", "تتوقع مضيفا غير فارغ", line, col, &host);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = NULL;
    if (getaddrinfo(host, NULL, &hints, &res) != 0 || !res) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.مسار_لمضيف تعذر حل اسم المضيف %s", host);
    }
    struct addrinfo *first = res;
    NvValue out = nv_nil();
    if (first->ai_family == AF_INET) {
        struct sockaddr_in *sin = (struct sockaddr_in *)first->ai_addr;
        unsigned char target[4];
        memcpy(target, &sin->sin_addr.s_addr, 4);
        Nv5Route *list = NULL;
        int n = 0;
        nv5_routes(&list, &n);
        int best = -1;
        unsigned int best_bits = 0;
        for (int i = 0; i < n; i++) {
            unsigned char dest[4];
            unsigned char mask[4];
            if (!nv5_parse_ipv4(list[i].dest, dest) || !nv5_parse_ipv4(list[i].mask, mask)) continue;
            int ok = 1;
            for (int b = 0; b < 4; b++) {
                if ((target[b] & mask[b]) != (dest[b] & mask[b])) {
                    ok = 0;
                    break;
                }
            }
            if (!ok) continue;
            unsigned int bits = nv5_count_bits(mask, 4);
            if (best < 0 || bits > best_bits) {
                best = i;
                best_bits = bits;
            }
        }
        if (best >= 0) {
            out = nv5_route_value(&list[best]);
        }
    }
    freeaddrinfo(res);
    return out;
}

static long nv5_sys_long(const char *iface, const char *file) {
    char *text = nv5_sys(iface, file);
    if (!text) return 0;
    long v = strtol(text, NULL, 10);
    free(text);
    return v;
}

static NvValue nv5_net_status(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "حالة_الشبكة", line, col);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(list[i].name));
        nv_dict_set_raw(d.u.dict, "حالة", nv_str_cstr(list[i].up ? "يعمل" : "متوقف"));
        long speed = nv5_sys_long(list[i].name, "speed");
        if (speed < 0) speed = 0;
        nv_dict_set_raw(d.u.dict, "سرعة", nv_num((double)speed));
        const char *kind = "سلكي";
        if (list[i].loopback) {
            kind = "حلقي";
        } else {
            char wpath[512];
            snprintf(wpath, sizeof(wpath), "/sys/class/net/%s/wireless", list[i].name);
            if (access(wpath, F_OK) == 0) kind = "لاسلكي";
        }
        nv_dict_set_raw(d.u.dict, "نوع", nv_str_cstr(kind));
        nv_list_push_raw(out.u.list, d);
    }
    return out;
}

static int nv5_arp_rows(NvValue *out_list, const char *want_ip, NvValue *found) {
    char *text = nv5_read_file("/proc/net/arp");
    if (!text) return 0;
    int first = 1;
    int nfound = 0;
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line = (char *)malloc(llen + 1);
        memcpy(line, p, llen);
        line[llen] = '\0';
        if (first) {
            first = 0;
        } else {
            char *fields[8];
            int nf = nv5_split(line, fields, 8);
            if (nf >= 6) {
                long flags = strtol(fields[2], NULL, 0);
                if (want_ip) {
                    if (strcmp(fields[0], want_ip) == 0) {
                        NvValue d = nv_dict_new();
                        nv_dict_set_raw(d.u.dict, "عنوان", nv_str_cstr(fields[0]));
                        nv_dict_set_raw(d.u.dict, "ماك", nv_str_cstr(fields[3]));
                        nv_dict_set_raw(d.u.dict, "واجهة", nv_str_cstr(fields[5]));
                        *found = d;
                        nfound = 1;
                    }
                } else if (out_list) {
                    NvValue d = nv_dict_new();
                    nv_dict_set_raw(d.u.dict, "عنوان", nv_str_cstr(fields[0]));
                    nv_dict_set_raw(d.u.dict, "ماك", nv_str_cstr(fields[3]));
                    nv_dict_set_raw(d.u.dict, "واجهة", nv_str_cstr(fields[5]));
                    nv_list_push_raw(out_list->u.list, d);
                    nfound++;
                    (void)flags;
                } else {
                    if (flags == 0x2) {
                        nfound++;
                    }
                }
            }
        }
        free(line);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return nfound;
}

static NvValue nv5_neighbors(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "جيراني", line, col);
    NvValue out = nv_list_new();
    nv5_arp_rows(&out, NULL, NULL);
    return out;
}

static NvValue nv5_neighbor(NvValue *args, int nargs, int line, int col) {
    const char *ip = NULL;
    nv5_need_text(args, nargs, "جار", "تتوقع عنوانا غير فارغ", line, col, &ip);
    NvValue found = nv_nil();
    nv5_arp_rows(NULL, ip, &found);
    return found;
}

static NvValue nv5_net_stats(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "إحصاءات_الشبكة", line, col);
    Nv5Dev *list = NULL;
    int n = 0;
    nv5_dev(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(list[i].name));
        nv_dict_set_raw(d.u.dict, "مستلم", nv_num((double)list[i].rx_bytes));
        nv_dict_set_raw(d.u.dict, "مرسل", nv_num((double)list[i].tx_bytes));
        nv_dict_set_raw(d.u.dict, "أخطاء", nv_num((double)(list[i].rx_errs + list[i].tx_errs)));
        nv_list_push_raw(out.u.list, d);
    }
    return out;
}

static NvValue nv5_route_stats(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "إحصاءات_التوجيه", line, col);
    Nv5Route *list = NULL;
    int n = 0;
    nv5_routes(&list, &n);
    int gw_count = 0;
    int if_count = 0;
    for (int i = 0; i < n; i++) {
        if (strcmp(list[i].gw, "0.0.0.0") != 0) {
            int seen = 0;
            for (int j = 0; j < i; j++) {
                if (strcmp(list[j].gw, list[i].gw) == 0) seen = 1;
            }
            if (!seen) gw_count++;
        }
        int seen = 0;
        for (int j = 0; j < i; j++) {
            if (strcmp(list[j].iface, list[i].iface) == 0) seen = 1;
        }
        if (!seen) if_count++;
    }
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "وجهات", nv_num((double)n));
    nv_dict_set_raw(d.u.dict, "بوابات", nv_num((double)gw_count));
    nv_dict_set_raw(d.u.dict, "واجهات", nv_num((double)if_count));
    return d;
}

static NvValue nv5_route_rules(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "قواعد_التوجيه", line, col);
    NvValue out = nv_list_new();
    char *text = nv5_read_file("/proc/net/fib_rules");
    if (text) {
        char *p = text;
        while (*p) {
            char *eol = strchr(p, '\n');
            size_t llen = eol ? (size_t)(eol - p) : strlen(p);
            char *line2 = (char *)malloc(llen + 1);
            memcpy(line2, p, llen);
            line2[llen] = '\0';
            char *colon = strchr(line2, ':');
            char *lookup = strstr(line2, "lookup");
            if (colon && lookup) {
                long prio = strtol(line2, NULL, 10);
                char *table = lookup + 6;
                while (*table == ' ' || *table == '\t') table++;
                char *end = table;
                while (*end && *end != ' ' && *end != '\t' && *end != '\n' && *end != '\r') end++;
                *end = '\0';
                NvValue d = nv_dict_new();
                nv_dict_set_raw(d.u.dict, "أولوية", nv_num((double)prio));
                nv_dict_set_raw(d.u.dict, "جدول", nv_str_cstr(table));
                nv_list_push_raw(out.u.list, d);
            }
            free(line2);
            if (!eol) break;
            p = eol + 1;
        }
        free(text);
    }
    if (out.u.list->len == 0) {
        const char *defs[3][2] = {{"0", "local"}, {"32766", "main"}, {"32767", "default"}};
        for (int i = 0; i < 3; i++) {
            NvValue d = nv_dict_new();
            nv_dict_set_raw(d.u.dict, "أولوية", nv_num((double)strtol(defs[i][0], NULL, 10)));
            nv_dict_set_raw(d.u.dict, "جدول", nv_str_cstr(defs[i][1]));
            nv_list_push_raw(out.u.list, d);
        }
    }
    return out;
}

static NvValue nv5_broadcast_addrs(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "عناوين_البث", line, col);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < list[i].naddrs; j++) {
            if (list[i].addrs[j].has_bcast) {
                NvValue d = nv_dict_new();
                nv_dict_set_raw(d.u.dict, "واجهة", nv_str_cstr(list[i].name));
                nv_dict_set_raw(d.u.dict, "مجموعة", nv_str_cstr(list[i].addrs[j].bcast));
                nv_list_push_raw(out.u.list, d);
            }
        }
    }
    return out;
}

static NvValue nv5_firewall_rules(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "قواعد_الجدار", line, col);
    NvValue out = nv_list_new();
    char *text = nv5_read_file("/proc/net/ip_tables_names");
    if (!text) return out;
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        while (llen > 0 && (p[llen - 1] == ' ' || p[llen - 1] == '\t' || p[llen - 1] == '\r')) llen--;
        if (llen > 0) {
            char *name = (char *)malloc(llen + 1);
            memcpy(name, p, llen);
            name[llen] = '\0';
            nv_list_push_raw(out.u.list, nv_str_cstr(name));
            free(name);
        }
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return out;
}

static NvValue nv5_my_connections(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "اتصالاتي", line, col);
    return nv5_conns();
}

static NvValue nv5_listeners(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "المستمعة", line, col);
    NvValue all = nv5_conns();
    NvValue out = nv_list_new();
    for (size_t i = 0; i < all.u.list->len; i++) {
        NvValue d = all.u.list->items[i];
        int si = nv_dict_find(d.u.dict, "حالة");
        if (si >= 0) {
            NvValue st = d.u.dict->entries[si].val;
            if (st.tag == NV_STR && strcmp(st.u.str->data, "يستمع") == 0) {
                nv_list_push_raw(out.u.list, d);
            }
        }
    }
    return out;
}

static NvValue nv5_bridges(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "الجسور", line, col);
    NvValue out = nv_list_new();
    DIR *dir = opendir("/sys/class/net");
    if (!dir) return out;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (ent->d_name[0] == '.') continue;
        char bpath[512];
        snprintf(bpath, sizeof(bpath), "/sys/class/net/%s/bridge/brif", ent->d_name);
        DIR *br = opendir(bpath);
        if (!br) continue;
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(ent->d_name));
        NvValue members = nv_list_new();
        struct dirent *m;
        while ((m = readdir(br)) != NULL) {
            if (m->d_name[0] == '.') continue;
            nv_list_push_raw(members.u.list, nv_str_cstr(m->d_name));
        }
        closedir(br);
        nv_dict_set_raw(d.u.dict, "واجهات", members);
        nv_list_push_raw(out.u.list, d);
    }
    closedir(dir);
    return out;
}

static NvValue nv5_traffic_queues(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "طوابير_الحركة", line, col);
    NvValue out = nv_list_new();
    DIR *dir = opendir("/sys/class/net");
    if (!dir) return out;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (ent->d_name[0] == '.') continue;
        char qpath[512];
        snprintf(qpath, sizeof(qpath), "/sys/class/net/%s/queues", ent->d_name);
        DIR *q = opendir(qpath);
        int count = 0;
        if (q) {
            struct dirent *m;
            while ((m = readdir(q)) != NULL) {
                if (m->d_name[0] == '.') continue;
                count++;
            }
            closedir(q);
        }
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "واجهة", nv_str_cstr(ent->d_name));
        nv_dict_set_raw(d.u.dict, "طوابير", nv_num((double)count));
        nv_list_push_raw(out.u.list, d);
    }
    closedir(dir);
    return out;
}

static NvValue nv5_traffic_stats(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "إحصاءات_الحركة", line, col);
    Nv5Dev *list = NULL;
    int n = 0;
    nv5_dev(&list, &n);
    NvValue out = nv_list_new();
    for (int i = 0; i < n; i++) {
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(list[i].name));
        nv_dict_set_raw(d.u.dict, "حزم", nv_num((double)(list[i].rx_packets + list[i].tx_packets)));
        nv_dict_set_raw(d.u.dict, "مسقط", nv_num((double)(list[i].rx_drop + list[i].tx_drop)));
        nv_list_push_raw(out.u.list, d);
    }
    return out;
}

static NvValue nv5_device_info(NvValue *args, int nargs, int line, int col) {
    const char *name = NULL;
    nv5_need_text(args, nargs, "معلومات_الجهاز", "تتوقع اسما غير فارغ", line, col, &name);
    Nv5Iface *list = NULL;
    int n = 0;
    nv5_ifaces(&list, &n);
    if (!nv5_iface_find(list, n, name)) return nv_nil();
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "اسم", nv_str_cstr(name));
    char *mac = nv5_sys(name, "address");
    if (mac) {
        nv_dict_set_raw(d.u.dict, "ماك", nv_str_cstr(mac));
        free(mac);
    } else {
        nv_dict_set_raw(d.u.dict, "ماك", nv_nil());
    }
    nv_dict_set_raw(d.u.dict, "وحدة", nv_num((double)nv5_sys_long(name, "mtu")));
    long speed = nv5_sys_long(name, "speed");
    if (speed < 0) speed = 0;
    nv_dict_set_raw(d.u.dict, "سرعة", nv_num((double)speed));
    return d;
}

static NvValue nv5_proto_stats(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "إحصاءات_البروتوكول", line, col);
    NvValue out = nv_dict_new();
    NvValue tcp = nv_dict_new();
    nv_dict_set_raw(tcp.u.dict, "مستلم", nv_num((double)nv5_snmp_num("Tcp:", "InSegs")));
    nv_dict_set_raw(tcp.u.dict, "مرسل", nv_num((double)nv5_snmp_num("Tcp:", "OutSegs")));
    nv_dict_set_raw(tcp.u.dict, "أخطاء", nv_num((double)nv5_snmp_num("Tcp:", "InErrs")));
    nv_dict_set_raw(out.u.dict, "TCP", tcp);
    NvValue udp = nv_dict_new();
    nv_dict_set_raw(udp.u.dict, "مستلم", nv_num((double)nv5_snmp_num("Udp:", "InDatagrams")));
    nv_dict_set_raw(udp.u.dict, "مرسل", nv_num((double)nv5_snmp_num("Udp:", "OutDatagrams")));
    nv_dict_set_raw(udp.u.dict, "أخطاء", nv_num((double)nv5_snmp_num("Udp:", "InErrors")));
    nv_dict_set_raw(out.u.dict, "UDP", udp);
    NvValue icmp = nv_dict_new();
    nv_dict_set_raw(icmp.u.dict, "مستلم", nv_num((double)nv5_snmp_num("Icmp:", "InMsgs")));
    nv_dict_set_raw(icmp.u.dict, "مرسل", nv_num((double)nv5_snmp_num("Icmp:", "OutMsgs")));
    nv_dict_set_raw(icmp.u.dict, "أخطاء", nv_num((double)nv5_snmp_num("Icmp:", "InErrors")));
    nv_dict_set_raw(out.u.dict, "ICMP", icmp);
    return out;
}

static NvValue nv5_watch_changes(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "راقب_التغييرات", line, col);
    Nv5Dev *before = NULL;
    int nb = 0;
    nv5_dev(&before, &nb);
    nv_net_sleep_ms(1000);
    Nv5Dev *after = NULL;
    int na = 0;
    nv5_dev(&after, &na);
    NvValue out = nv_list_new();
    for (int i = 0; i < nb; i++) {
        for (int j = 0; j < na; j++) {
            if (strcmp(before[i].name, after[j].name) == 0) {
                if (before[i].rx_bytes + before[i].tx_bytes != after[j].rx_bytes + after[j].tx_bytes) {
                    nv_list_push_raw(out.u.list, nv_str_cstr(before[i].name));
                }
                break;
            }
        }
    }
    return out;
}

static NvValue nv5_traffic_rate(NvValue *args, int nargs, int line, int col) {
    const char *name = NULL;
    nv5_need_text(args, nargs, "معدل_الحركة", "تتوقع اسما غير فارغ", line, col, &name);
    Nv5Dev *before = NULL;
    int nb = 0;
    nv5_dev(&before, &nb);
    Nv5Dev *b = NULL;
    for (int i = 0; i < nb; i++) {
        if (strcmp(before[i].name, name) == 0) {
            b = &before[i];
            break;
        }
    }
    if (!b) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.معدل_الحركة واجهة غير موجودة");
    }
    unsigned long long rx0 = b->rx_bytes;
    unsigned long long tx0 = b->tx_bytes;
    nv_net_sleep_ms(1000);
    Nv5Dev *after = NULL;
    int na = 0;
    nv5_dev(&after, &na);
    unsigned long long rx1 = rx0;
    unsigned long long tx1 = tx0;
    for (int i = 0; i < na; i++) {
        if (strcmp(after[i].name, name) == 0) {
            rx1 = after[i].rx_bytes;
            tx1 = after[i].tx_bytes;
            break;
        }
    }
    NvValue d = nv_dict_new();
    nv_dict_set_raw(d.u.dict, "مستلم_في_الثانية", nv_num((double)(rx1 >= rx0 ? rx1 - rx0 : 0)));
    nv_dict_set_raw(d.u.dict, "مرسل_في_الثانية", nv_num((double)(tx1 >= tx0 ? tx1 - tx0 : 0)));
    return d;
}

static NvValue nv5_who_is_connected(NvValue *args, int nargs, int line, int col) {
    nv5_no_args(args, nargs, "من_المتصل", line, col);
    NvValue out = nv_list_new();
    char *text = nv5_read_file("/proc/net/arp");
    if (!text) return out;
    int first = 1;
    char *p = text;
    while (*p) {
        char *eol = strchr(p, '\n');
        size_t llen = eol ? (size_t)(eol - p) : strlen(p);
        char *line2 = (char *)malloc(llen + 1);
        memcpy(line2, p, llen);
        line2[llen] = '\0';
        if (first) {
            first = 0;
        } else {
            char *fields[8];
            int nf = nv5_split(line2, fields, 8);
            if (nf >= 6) {
                long flags = strtol(fields[2], NULL, 0);
                if (flags == 0x2) {
                    nv_list_push_raw(out.u.list, nv_str_cstr(fields[0]));
                }
            }
        }
        free(line2);
        if (!eol) break;
        p = eol + 1;
    }
    free(text);
    return out;
}



// إعلانات مسبقة: دوال المقابس معرّفة لاحقا في الملف.
static void nv_sock_require_net(int line, int col);
static unsigned long long nv_sock_insert2(int kind, int fd, int line, int col);
static void nv_resolve_checked(const char *host, int port, struct sockaddr_storage *out, socklen_t *out_len, int line, int col);
static void nv_sock_check_port(long long port, const char *display, int allow_zero, int line, int col);
static void nv_sock_arm_timeout(int fd, long long deadline_ms);

// ---------- لينكس8-طبقة5ب: فحص واتصال (اجتماع 90 §105) ----------
static int nv5b_tcp_probe(const char *host, int port, long long timeout_ms, int line, int col) {
    nv_sock_require_net(line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(host, port, &ss, &slen, line, col);
    int fd = nv_connect_timeout((const struct sockaddr *)&ss, slen, nv_net_now_ms() + timeout_ms);
    if (fd < 0) return 0;
    close(fd);
    return 1;
}

static const char *nv5b_udp_probe(const char *host, int port, long long timeout_ms, int line, int col) {
    nv_sock_require_net(line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(host, port, &ss, &slen, line, col);
    int fd = socket(ss.ss_family, SOCK_DGRAM, 0);
    if (fd < 0) {
        nv_error(NV_ERR_RUNTIME, "تعذر فتح مقبس UDP", line, col);
    }
    if (connect(fd, (const struct sockaddr *)&ss, slen) != 0) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "تعذر الاتصال بمقبس UDP", line, col);
    }
    struct timeval tv;
    tv.tv_sec = (time_t)(timeout_ms / 1000);
    tv.tv_usec = (suseconds_t)((timeout_ms % 1000) * 1000);
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    unsigned char one = 0;
    ssize_t sent = send(fd, &one, 1, 0);
    if (sent < 0) {
        int e = errno;
        close(fd);
        if (e == ECONNREFUSED) return "مغلق";
        return "مفلتر";
    }
    unsigned char buf[512];
    ssize_t n = recv(fd, buf, sizeof(buf), 0);
    if (n >= 0) {
        close(fd);
        return "مفتوح";
    }
    int e = errno;
    close(fd);
    if (e == ECONNREFUSED) return "مغلق";
    return "مفلتر";
}

static unsigned short nv5b_checksum(const unsigned char *data, size_t len) {
    unsigned long sum = 0;
    size_t i = 0;
    while (i + 1 < len) {
        sum += (unsigned long)((data[i] << 8) | data[i + 1]);
        i += 2;
    }
    if (i < len) {
        sum += (unsigned long)(data[i] << 8);
    }
    while ((sum >> 16) != 0) {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    return (unsigned short)(~sum);
}

static int nv5b_icmp_probe(const char *host, long long timeout_ms, int line, int col) {
#ifdef __linux__
    nv_sock_require_net(line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(host, 0, &ss, &slen, line, col);
    if (ss.ss_family != AF_INET) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.انبض_icmp النبض يدعم IPv4 فقط");
    }
    int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_ICMP);
    if (fd < 0) return 0;
    unsigned short id = (unsigned short)(getpid() & 0xFFFF);
    unsigned char pkt[64];
    memset(pkt, 0, sizeof(pkt));
    pkt[0] = 8;
    pkt[1] = 0;
    pkt[4] = (unsigned char)(id >> 8);
    pkt[5] = (unsigned char)(id & 0xFF);
    pkt[6] = 0;
    pkt[7] = 1;
    memcpy(pkt + 8, "niva", 4);
    unsigned short csum = nv5b_checksum(pkt, sizeof(pkt));
    pkt[2] = (unsigned char)(csum >> 8);
    pkt[3] = (unsigned char)(csum & 0xFF);
    struct sockaddr_in *sin = (struct sockaddr_in *)&ss;
    ssize_t sent = sendto(fd, pkt, sizeof(pkt), 0, (const struct sockaddr *)sin, sizeof(*sin));
    if (sent < 0) {
        close(fd);
        return 0;
    }
    struct pollfd pfd;
    pfd.fd = fd;
    pfd.events = POLLIN;
    pfd.revents = 0;
    int pr = poll(&pfd, 1, (int)timeout_ms);
    if (pr <= 0) {
        close(fd);
        return 0;
    }
    unsigned char buf[512];
    ssize_t n = recv(fd, buf, sizeof(buf), 0);
    close(fd);
    if (n < 20) return 0;
    return buf[0] == 0 && buf[4] == pkt[4] && buf[5] == pkt[5];
#else
    (void)host;
    (void)timeout_ms;
    nv_error(NV_ERR_RUNTIME, "شبكة.انبض_icmp غير مدعوم على هذه المنصة", line, col);
#endif
}

#ifdef __linux__
static NvValue nv5b_trace_list(const char *host, int max_hops, int line, int col) {
    nv_sock_require_net(line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(host, 33434, &ss, &slen, line, col);
    if (ss.ss_family != AF_INET) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.تتبع_المسار التتبع يدعم IPv4 فقط");
    }
    struct sockaddr_in dest0 = *(struct sockaddr_in *)&ss;
    int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (fd < 0) {
        nv_error(NV_ERR_RUNTIME, "تعذر فتح مقبس التتبع", line, col);
    }
    int on = 1;
    setsockopt(fd, IPPROTO_IP, IP_RECVERR, &on, sizeof(on));
    NvValue out = nv_list_new();
    unsigned char payload[32];
    memset(payload, 0, sizeof(payload));
    for (int ttl = 1; ttl <= max_hops; ttl++) {
        setsockopt(fd, IPPROTO_IP, IP_TTL, &ttl, sizeof(ttl));
        long long start = nv_net_now_ms();
        struct sockaddr_in dest = dest0;
        dest.sin_port = htons((unsigned short)(33434 + ttl));
        ssize_t sent = sendto(fd, payload, sizeof(payload), 0, (const struct sockaddr *)&dest, sizeof(dest));
        if (sent < 0) {
            NvValue d = nv_dict_new();
            nv_dict_set_raw(d.u.dict, "قفزة", nv_num((double)ttl));
            nv_dict_set_raw(d.u.dict, "عنوان", nv_nil());
            nv_dict_set_raw(d.u.dict, "زمن", nv_num((double)(nv_net_now_ms() - start)));
            nv_list_push_raw(out.u.list, d);
            continue;
        }
        struct pollfd pfd;
        pfd.fd = fd;
        pfd.events = POLLIN;
        pfd.revents = 0;
        int pr = poll(&pfd, 1, 1000);
        if (pr <= 0) {
            NvValue d = nv_dict_new();
            nv_dict_set_raw(d.u.dict, "قفزة", nv_num((double)ttl));
            nv_dict_set_raw(d.u.dict, "عنوان", nv_nil());
            nv_dict_set_raw(d.u.dict, "زمن", nv_num((double)(nv_net_now_ms() - start)));
            nv_list_push_raw(out.u.list, d);
            continue;
        }
        unsigned char buf[512];
        unsigned char ctrl[512];
        struct sockaddr_storage src;
        struct iovec iov;
        iov.iov_base = buf;
        iov.iov_len = sizeof(buf);
        struct msghdr msg;
        memset(&msg, 0, sizeof(msg));
        msg.msg_name = &src;
        msg.msg_namelen = sizeof(src);
        msg.msg_iov = &iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctrl;
        msg.msg_controllen = sizeof(ctrl);
        ssize_t n = recvmsg(fd, &msg, 0);
        long long elapsed = nv_net_now_ms() - start;
        if (n < 0) {
            NvValue d = nv_dict_new();
            nv_dict_set_raw(d.u.dict, "قفزة", nv_num((double)ttl));
            nv_dict_set_raw(d.u.dict, "عنوان", nv_nil());
            nv_dict_set_raw(d.u.dict, "زمن", nv_num((double)elapsed));
            nv_list_push_raw(out.u.list, d);
            continue;
        }
        NvValue d = nv_dict_new();
        nv_dict_set_raw(d.u.dict, "قفزة", nv_num((double)ttl));
        if (src.ss_family == AF_INET) {
            char ip[64];
            struct sockaddr_in *sin = (struct sockaddr_in *)&src;
            inet_ntop(AF_INET, &sin->sin_addr, ip, sizeof(ip));
            nv_dict_set_raw(d.u.dict, "عنوان", nv_str_cstr(ip));
        } else {
            nv_dict_set_raw(d.u.dict, "عنوان", nv_nil());
        }
        nv_dict_set_raw(d.u.dict, "زمن", nv_num((double)elapsed));
        int reached = 0;
        for (struct cmsghdr *cmsg = CMSG_FIRSTHDR(&msg); cmsg; cmsg = CMSG_NXTHDR(&msg, cmsg)) {
            if (cmsg->cmsg_level == IPPROTO_IP && cmsg->cmsg_type == IP_RECVERR) {
                struct sock_extended_err *ee = (struct sock_extended_err *)CMSG_DATA(cmsg);
                if (ee->ee_type == 3 && ee->ee_code == 3) reached = 1;
            }
        }
        nv_list_push_raw(out.u.list, d);
        if (reached) break;
    }
    close(fd);
    return out;
}
#endif

static const char *nv5b_host_arg(NvValue *args, int nargs, int i, const char *name, int line, int col) {
    (void)nargs;
    if (args[i].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع نصا لكن أعطيت %s", name, nv_type_name(args[i]));
    }
    if (args[i].u.str->len == 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع مضيفا غير فارغ", name);
    }
    return args[i].u.str->data;
}

static long long nv5b_int_arg(NvValue *args, int i, const char *name, int line, int col) {
    if (args[i].tag != NV_NUM || args[i].u.num != trunc(args[i].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.%s تتوقع عددا صحيحا", name);
    }
    return (long long)args[i].u.num;
}

static int nv5b_port_arg(NvValue *args, int i, const char *name, int line, int col) {
    long long p = nv5b_int_arg(args, i, name, line, col);
    if (p < 1 || p > 65535) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.%s تتوقع منفذا بين 1 و 65535", name);
    }
    return (int)p;
}

static NvValue nv5b_ping(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1 && nargs != 2) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.انبض تتوقع معاملا واحدا (مضيف) أو معاملين (مضيف، منفذ)", line, col);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "انبض", line, col);
    int port = 80;
    if (nargs == 2) port = nv5b_port_arg(args, 1, "انبض", line, col);
    return nv_bool(nv5b_tcp_probe(host, port, 2000, line, col));
}

static NvValue nv5b_ping_loop(NvValue *args, int nargs, int line, int col) {
    if (nargs < 2 || nargs > 3) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.انبض_مستمر تتوقع معاملين (مضيف، عدد) أو ثلاثة (مضيف، عدد، فاصل)", line, col);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "انبض_مستمر", line, col);
    long long count = nv5b_int_arg(args, 1, "انبض_مستمر", line, col);
    if (count < 1 || count > 100) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.انبض_مستمر عدد غير صالح", line, col);
    }
    long long gap = 1;
    if (nargs == 3) {
        gap = nv5b_int_arg(args, 2, "انبض_مستمر", line, col);
        if (gap < 0 || gap > 60) {
            nv_error(NV_ERR_RUNTIME, "الدالة شبكة.انبض_مستمر فاصل غير صالح", line, col);
        }
    }
    NvValue out = nv_list_new();
    for (long long i = 0; i < count; i++) {
        if (i > 0 && gap > 0) nv_net_sleep_ms(gap * 1000);
        nv_list_push_raw(out.u.list, nv_bool(nv5b_tcp_probe(host, 80, 2000, line, col)));
    }
    return out;
}

static NvValue nv5b_rtt(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1 && nargs != 2) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.زمن_الذهاب_والعودة تتوقع معاملا واحدا (مضيف) أو معاملين (مضيف، منفذ)", line, col);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "زمن_الذهاب_والعودة", line, col);
    int port = 80;
    if (nargs == 2) port = nv5b_port_arg(args, 1, "زمن_الذهاب_والعودة", line, col);
    long long start = nv_net_now_ms();
    int ok = nv5b_tcp_probe(host, port, 2000, line, col);
    if (!ok) return nv_num(-1);
    return nv_num((double)(nv_net_now_ms() - start));
}

static NvValue nv5b_resolve(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.حل_الاسم تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "حل_الاسم", line, col);
    nv_sock_require_net(line, col);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = NULL;
    if (getaddrinfo(host, "0", &hints, &res) != 0 || !res) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.حل_الاسم تعذر حل اسم المضيف %s", host);
    }
    NvValue out = nv_list_new();
    for (struct addrinfo *ai = res; ai; ai = ai->ai_next) {
        char ip[64];
        void *addr = NULL;
        if (ai->ai_family == AF_INET) {
            addr = &((struct sockaddr_in *)ai->ai_addr)->sin_addr;
        } else if (ai->ai_family == AF_INET6) {
            addr = &((struct sockaddr_in6 *)ai->ai_addr)->sin6_addr;
        } else {
            continue;
        }
        inet_ntop(ai->ai_family, addr, ip, sizeof(ip));
        int seen = 0;
        for (size_t i = 0; i < out.u.list->len; i++) {
            NvValue v = out.u.list->items[i];
            if (v.tag == NV_STR && strcmp(v.u.str->data, ip) == 0) seen = 1;
        }
        if (!seen) nv_list_push_raw(out.u.list, nv_str_cstr(ip));
    }
    freeaddrinfo(res);
    return out;
}

static NvValue nv5b_port_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_المنفذ تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "افحص_المنفذ", line, col);
    int port = nv5b_port_arg(args, 1, "افحص_المنفذ", line, col);
    return nv_bool(nv5b_tcp_probe(host, port, 500, line, col));
}

static NvValue nv5b_ports_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_المنافذ تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "افحص_المنافذ", line, col);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.افحص_المنافذ تتوقع قائمة لكن أعطيت %s", nv_type_name(args[1]));
    }
    NvValue out = nv_dict_new();
    for (size_t i = 0; i < args[1].u.list->len; i++) {
        NvValue item = args[1].u.list->items[i];
        if (item.tag != NV_NUM || item.u.num != trunc(item.u.num)) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.افحص_المنافذ تتوقع عددا صحيحا");
        }
        long long p = (long long)item.u.num;
        if (p < 1 || p > 65535) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_المنافذ تتوقع منفذا بين 1 و 65535");
        }
        char key[16];
        snprintf(key, sizeof(key), "%lld", p);
        nv_dict_set_raw(out.u.dict, key, nv_bool(nv5b_tcp_probe(host, (int)p, 500, line, col)));
    }
    return out;
}

static NvValue nv5b_range_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_نطاق تتوقع 3 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "افحص_نطاق", line, col);
    long long from = nv5b_int_arg(args, 1, "افحص_نطاق", line, col);
    long long to = nv5b_int_arg(args, 2, "افحص_نطاق", line, col);
    if (from < 1 || from > 65535 || to < 1 || to > 65535 || from > to) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.افحص_نطاق مدى غير صالح", line, col);
    }
    if (to - from + 1 > 1024) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.افحص_نطاق نطاق كبير جدا (الحد 1024)", line, col);
    }
    NvValue out = nv_list_new();
    for (long long p = from; p <= to; p++) {
        if (nv5b_tcp_probe(host, (int)p, 200, line, col)) {
            nv_list_push_raw(out.u.list, nv_num((double)p));
        }
    }
    return out;
}

static NvValue nv5b_fast_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_سريع تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "افحص_سريع", line, col);
    int ports[18] = {21, 22, 23, 25, 53, 80, 110, 143, 443, 445, 993, 995, 1723, 3306, 3389, 5432, 5900, 8080};
    NvValue out = nv_list_new();
    for (int i = 0; i < 18; i++) {
        if (nv5b_tcp_probe(host, ports[i], 500, line, col)) {
            nv_list_push_raw(out.u.list, nv_num((double)ports[i]));
        }
    }
    return out;
}

static NvValue nv5b_udp_check(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.افحص_udp تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "افحص_udp", line, col);
    int port = nv5b_port_arg(args, 1, "افحص_udp", line, col);
    return nv_str_cstr(nv5b_udp_probe(host, port, 1000, line, col));
}

static NvValue nv5b_icmp_ping(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.انبض_icmp تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "انبض_icmp", line, col);
    return nv_bool(nv5b_icmp_probe(host, 1000, line, col));
}

static NvValue nv5b_trace(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1 && nargs != 2) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.تتبع_المسار تتوقع معاملا واحدا (مضيف) أو معاملين (مضيف، أقصى)", line, col);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "تتبع_المسار", line, col);
    long long max = 30;
    if (nargs == 2) {
        max = nv5b_int_arg(args, 1, "تتبع_المسار", line, col);
        if (max < 1 || max > 30) {
            nv_error(NV_ERR_RUNTIME, "الدالة شبكة.تتبع_المسار أقصى غير صالح", line, col);
        }
    }
#ifdef __linux__
    return nv5b_trace_list(host, (int)max, line, col);
#else
    (void)host;
    (void)max;
    nv_error(NV_ERR_RUNTIME, "شبكة.تتبع_المسار غير مدعوم على هذه المنصة", line, col);
#endif
}

static NvValue nv5b_tls_connect(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اتصل_مشفر تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    (void)nv5b_host_arg(args, nargs, 0, "اتصل_مشفر", line, col);
    long long port = nv5b_int_arg(args, 1, "اتصل_مشفر", line, col);
    nv_sock_require_net(line, col);
    nv_sock_check_port(port, "شبكة.اتصل_مشفر", 0, line, col);
    nv_error(NV_ERR_RUNTIME, "اتصل_مشفر غير مدعوم في النسخة الأصلية — استخدم التطبيق للاتصالات المشفرة", line, col);
}

static NvValue nv5b_timeout_connect(NvValue *args, int nargs, int line, int col) {
    if (nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اتصل_بمهلة تتوقع 3 معاملات لكن أعطيت %d", nargs);
    }
    const char *host = nv5b_host_arg(args, nargs, 0, "اتصل_بمهلة", line, col);
    long long port = nv5b_int_arg(args, 1, "اتصل_بمهلة", line, col);
    long long secs = nv5b_int_arg(args, 2, "اتصل_بمهلة", line, col);
    if (secs < 1 || secs > 300) {
        nv_error(NV_ERR_RUNTIME, "الدالة شبكة.اتصل_بمهلة ثوان غير صالحة", line, col);
    }
    nv_sock_require_net(line, col);
    nv_sock_check_port(port, "شبكة.اتصل_بمهلة", 0, line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(host, (int)port, &ss, &slen, line, col);
    int fd = nv_connect_timeout((const struct sockaddr *)&ss, slen, nv_net_now_ms() + secs * 1000);
    if (fd < 0) {
        if (errno == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاتصال (30 ثانية)", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر الاتصال بالمضيف %s", host);
    }
    nv_sock_arm_timeout(fd, nv_net_deadline_ms());
    unsigned long long id = nv_sock_insert2(1, fd, line, col);
    return nv_num((double)id);
}

// نزّل: رابط، مسار → عدد البايتات.
NvValue nv_net_download(NvValue *args, int nargs, int line, int col) {
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.نزل تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نزل تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.نزل تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    const char *url = args[0].u.str->data;
    if (url[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.نزل تتوقع رابطا غير فارغ", line, col);
    }
    const char *path = args[1].u.str->data;
    if (path[0] == '\0') {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.نزل تتوقع مسارا غير فارغ", line, col);
    }
    // حل المسار قبل أي شبكة (كما اكتب_ملف).
    char *resolved = nv_resolve_path(path, line, col);
    char *display = nv_display_path(resolved);
    // الحصة: النمو الصافي فقط (المتبقي + الحالي).
    struct stat st;
    size_t existing = 0;
    if (stat(resolved, &st) == 0) existing = (size_t)st.st_size;
    size_t limit = nv_quota_remaining() + existing;
    FILE *f = fopen(resolved, "wb");
    if (!f) {
        nv_net_err_runtime(line, col, "تعذر كتابة الملف %s", display);
    }
    NvRequest req;
    memset(&req, 0, sizeof(req));
    NvSink sink;
    memset(&sink, 0, sizeof(sink));
    sink.file = f;
    sink.limit = limit;
    // عند الخطأ يُحذف الملف الجزئي: نلتقط الخطأ عبر
    // معالج مؤقت (longjmp) ثم نحذف ونعيد الرفع.
    NvHandler h;
    volatile NvCheckpoint cp = nv_checkpoint();
    h.prev = nv_handlers;
    NvHandler *volatile hp = h.prev;
    h.has_except = 0;
    nv_handlers = &h;
    if (setjmp(h.jb) == 0) {
        nv_fetch_loop(url, &req, line, col, &sink);
    } else {
        nv_restore_checkpoint(cp);
        nv_handlers = hp;
        fclose(f);
        remove(resolved);
        free(resolved);
        free(display);
        nv_reraise_current();
    }
    nv_handlers = hp;
    fclose(f);
    size_t written = sink.written;
    free(resolved);
    free(display);
    return nv_num((double)written);
}

// ================= الخادم (HTTP عادي، متزامن) =================
// المعالج يعمل على الخيط الرئيسي (قيم نيفا لا تعبر
// الخيوط)؛ والاتصالات واحدا بعد الآخر.

// ترويسة صالحة؟ (اسم رمزي، وقيمة بلا CR/LF)
static int nv_header_name_ok(const char *name) {
    if (!name || !*name) return 0;
    for (const unsigned char *p = (const unsigned char *)name; *p; p++) {
        unsigned char b = *p;
        int ok = (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9')
            || b == '!' || b == '#' || b == '$' || b == '%' || b == '&' || b == '\''
            || b == '*' || b == '+' || b == '-' || b == '.' || b == '^' || b == '_'
            || b == '`' || b == '|' || b == '~';
        if (!ok) return 0;
    }
    return 1;
}

static int nv_header_value_ok(const char *v) {
    for (const unsigned char *p = (const unsigned char *)v; *p; p++) {
        if (*p == '\r' || *p == '\n') return 0;
    }
    return 1;
}

// ابنِ رد HTTP كاملا.
static unsigned char *nv_build_response(int status, const char **hnames, const char **hvalues, int nhdr,
                                        const unsigned char *body, size_t body_len, size_t *out_len) {
    const char *reason = "OK";
    switch (status) {
        case 200: reason = "OK"; break;
        case 201: reason = "Created"; break;
        case 204: reason = "No Content"; break;
        case 301: reason = "Moved Permanently"; break;
        case 302: reason = "Found"; break;
        case 400: reason = "Bad Request"; break;
        case 404: reason = "Not Found"; break;
        case 413: reason = "Content Too Large"; break;
        case 500: reason = "Internal Server Error"; break;
        default: reason = "OK"; break;
    }
    int has_ct = 0;
    for (int i = 0; i < nhdr; i++) {
        if (strcasecmp(hnames[i], "content-type") == 0) has_ct = 1;
    }
    size_t cap = 512 + body_len;
    for (int i = 0; i < nhdr; i++) cap += strlen(hnames[i]) + strlen(hvalues[i]) + 8;
    unsigned char *out = (unsigned char *)malloc(cap + 64);
    int n = snprintf((char *)out, cap + 64, "HTTP/1.1 %d %s\r\n", status, reason);
    for (int i = 0; i < nhdr; i++) {
        if (strcasecmp(hnames[i], "content-length") == 0 || strcasecmp(hnames[i], "connection") == 0) continue;
        n += snprintf((char *)out + n, cap + 64 - (size_t)n, "%s: %s\r\n", hnames[i], hvalues[i]);
    }
    if (!has_ct) {
        n += snprintf((char *)out + n, cap + 64 - (size_t)n, "Content-Type: text/plain; charset=utf-8\r\n");
    }
    n += snprintf((char *)out + n, cap + 64 - (size_t)n, "Content-Length: %zu\r\nConnection: close\r\n\r\n", body_len);
    memcpy(out + n, body, body_len);
    *out_len = (size_t)n + body_len;
    return out;
}

// شغّل المعالج على طلب (يلتقط خطأه → 500).
static void nv_run_handler(NvValue handler, NvValue req_dict, int line, int col,
                           int *status_out, const char ***hnames_out, const char ***hvalues_out,
                           int *nhdr_out, unsigned char **body_out, size_t *body_len_out) {
    // معالج مؤقت يلتقط خطأ المعالج.
    NvHandler h;
    volatile NvCheckpoint cp = nv_checkpoint();
    h.prev = nv_handlers;
    NvHandler *volatile hp = h.prev;
    h.has_except = 0;
    nv_handlers = &h;
    NvValue result;
    int failed = 0;
    if (setjmp(h.jb) == 0) {
        NvValue argv[1];
        argv[0] = req_dict;
        size_t ri = nv_root_push_idx();
        *nv_root_slot(ri) = req_dict;
        result = nv_call(handler, argv, 1, line, col);
        nv_root_pop_n(1);
        (void)ri;
    } else {
        nv_restore_checkpoint(cp);
        failed = 1;
    }
    nv_handlers = hp;
    if (failed) {
        NvError *e = nv_current_error();
        if (e) {
            fprintf(stderr, "خطأ في معالج الطلب: خطأ %s: %s (السطر %d، العمود %d)\n",
                    nv_err_kind_ar(e->kind), e->message, e->line, e->col);
        } else {
            fprintf(stderr, "خطأ في معالج الطلب\n");
        }
        nv_clear_error();
        *status_out = 500;
        *nhdr_out = 0;
        *hnames_out = NULL;
        *hvalues_out = NULL;
        const char *msg = "خطأ في معالج الطلب";
        *body_out = (unsigned char *)strdup(msg);
        *body_len_out = strlen(msg);
        return;
    }
    int status = 200;
    const char **hnames = NULL;
    const char **hvalues = NULL;
    int nhdr = 0;
    unsigned char *body = NULL;
    size_t body_len = 0;
    if (result.tag == NV_STR) {
        body = (unsigned char *)strdup(result.u.str->data);
        body_len = result.u.str->len;
    } else if (result.tag == NV_DICT) {
        NvDict *d = result.u.dict;
        int idx = nv_dict_find(d, "حالة");
        if (idx >= 0) {
            NvValue sv = d->entries[idx].val;
            if (sv.tag == NV_NUM && sv.u.num >= 100.0 && sv.u.num <= 599.0 && sv.u.num == trunc(sv.u.num)) {
                status = (int)sv.u.num;
            } else {
                goto bad_handler;
            }
        }
        idx = nv_dict_find(d, "نص");
        if (idx >= 0) {
            NvValue tv = d->entries[idx].val;
            if (tv.tag != NV_STR) goto bad_handler;
            body = (unsigned char *)strdup(tv.u.str->data);
            body_len = tv.u.str->len;
        } else {
            body = (unsigned char *)strdup("");
            body_len = 0;
        }
        idx = nv_dict_find(d, "ترويسات");
        if (idx >= 0) {
            NvValue hv = d->entries[idx].val;
            if (hv.tag != NV_DICT) goto bad_handler;
            NvDict *hd = hv.u.dict;
            nhdr = (int)hd->len;
            hnames = (const char **)malloc((size_t)(nhdr ? nhdr : 1) * sizeof(char *));
            hvalues = (const char **)malloc((size_t)(nhdr ? nhdr : 1) * sizeof(char *));
            for (size_t i = 0; i < hd->len; i++) {
                if (hd->entries[i].val.tag != NV_STR) {
                    free(hnames);
                    free(hvalues);
                    free(body);
                    goto bad_handler;
                }
                hnames[i] = hd->entries[i].key;
                hvalues[i] = hd->entries[i].val.u.str->data;
            }
            for (int i = 0; i < nhdr; i++) {
                if (!nv_header_name_ok(hnames[i]) || !nv_header_value_ok(hvalues[i])) {
                    free(hnames);
                    free(hvalues);
                    free(body);
                    goto bad_handler;
                }
            }
        }
    } else {
        goto bad_handler;
    }
    *status_out = status;
    *hnames_out = hnames;
    *hvalues_out = hvalues;
    *nhdr_out = nhdr;
    *body_out = body;
    *body_len_out = body_len;
    return;
bad_handler:;
    *status_out = 500;
    *nhdr_out = 0;
    *hnames_out = NULL;
    *hvalues_out = NULL;
    const char *msg = "خطأ في معالج الطلب";
    *body_out = (unsigned char *)strdup(msg);
    *body_len_out = strlen(msg);
}

// اخدم: منفذ، شهادة، مفتاح، معالج، [مضيف].
NvValue nv_net_serve(NvValue handler, NvValue *args, int nargs, int line, int col) {
    if (!nv_server_allowed) {
        nv_error(NV_ERR_RUNTIME, "الخادم مغلق افتراضيا — استخدم --اسمح-بالخادم للسماح بالخادم، أو وافق عند أول استخدام في الكونسول", line, col);
    }
    if (nargs != 4 && nargs != 5) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اخدم تتوقع 4 معاملات (منفذ شهادة مفتاح معالج) أو 5 (مع مضيف)", line, col);
    }
    if (args[0].tag != NV_NUM || args[0].u.num != trunc(args[0].u.num) || args[0].u.num < 1 || args[0].u.num > 65535) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اخدم تتوقع منفذا بين 1 و 65535", line, col);
    }
    int port = (int)args[0].u.num;
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اخدم تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[2].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اخدم تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
    }
    const char *cert = args[1].u.str->data;
    const char *key = args[2].u.str->data;
    if (cert[0] != '\0' || key[0] != '\0') {
        // شهادة/مفتاح = طلب HTTPS صريح — والخلفية بلا TLS.
        nv_error(NV_ERR_RUNTIME, "الخادم يحتاج HTTPS — هذه النسخة بُنيت بلا دعم TLS", line, col);
    }
    if (handler.tag != NV_FUNC) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اخدم تتوقع معالجا دالة نيفا", line, col);
    }
    const char *host = "127.0.0.1";
    if (nargs == 5) {
        if (args[4].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اخدم تتوقع نصا لكن أعطيت %s", nv_type_name(args[4]));
        }
        host = args[4].u.str->data;
    }
    while (*host == ' ' || *host == '\t') host++;
    if (strcmp(host, "127.0.0.1") != 0 && strcmp(host, "0.0.0.0") != 0) {
        nv_error(NV_ERR_RUNTIME, "الخادم يربط محليا فقط: المضيف 127.0.0.1 أو 0.0.0.0", line, col);
    }
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    if (listener < 0) {
        nv_error(NV_ERR_RUNTIME, "تعذر تهيئة الخادم", line, col);
    }
    int one = 1;
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (strcmp(host, "0.0.0.0") == 0) {
        addr.sin_addr.s_addr = INADDR_ANY;
    } else {
        inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
    }
    if (bind(listener, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        int e = errno;
        close(listener);
        if (e == EACCES) {
            nv_net_err_runtime(line, col, "تعذر ربط الخادم على %s:%d — المنفذ يحتاج صلاحيات مرتفعة", host, port);
        }
        if (e == EADDRINUSE) {
            nv_net_err_runtime(line, col, "تعذر ربط الخادم على %s:%d — المنفذ مستخدم بالفعل", host, port);
        }
        nv_net_err_runtime(line, col, "تعذر ربط الخادم على %s:%d", host, port);
    }
    struct sockaddr_in bound;
    socklen_t blen = sizeof(bound);
    int bound_port = port;
    if (getsockname(listener, (struct sockaddr *)&bound, &blen) == 0) {
        bound_port = ntohs(bound.sin_port);
    }
    if (listen(listener, 64) != 0) {
        close(listener);
        nv_error(NV_ERR_RUNTIME, "تعذر تهيئة الخادم", line, col);
    }
    printf("الخادم يعمل على http://%s:%d\n", host, bound_port);
    fflush(stdout);
    // حلقة قبول متزامنة (المعالج على الخيط الرئيسي).
    for (;;) {
        struct sockaddr_in peer;
        socklen_t plen = sizeof(peer);
        int conn = accept(listener, (struct sockaddr *)&peer, &plen);
        if (conn < 0) continue;
        long long deadline_ms = nv_net_deadline_ms();
        // اقرأ الطلب (ترويسات حتى 64KB + جسم حتى 5MB).
        char *head = (char *)malloc(NV_NET_HEADER_CAP + 1);
        size_t hlen = 0;
        int found = 0;
        while (hlen < NV_NET_HEADER_CAP) {
            nv_sock_arm_timeout(conn, deadline_ms);
            char c = 0;
            ssize_t r = recv(conn, &c, 1, 0);
            if (r <= 0) break;
            head[hlen++] = c;
            if (hlen >= 4 && memcmp(head + hlen - 4, "\r\n\r\n", 4) == 0) {
                found = 1;
                break;
            }
        }
        if (!found) {
            free(head);
            close(conn);
            continue;
        }
        head[hlen] = '\0';
        char *save = NULL;
        char *req_line = strtok_r(head, "\r\n", &save);
        char method[32] = "";
        char target[4096] = "";
        if (req_line) {
            sscanf(req_line, "%31s %4095s", method, target);
        }
        if (method[0] == '\0' || target[0] == '\0') {
            free(head);
            close(conn);
            continue;
        }
        // المسار والاستعلام.
        char *path = target;
        char *query = "";
        char *q = strchr(target, '?');
        if (q) {
            *q = '\0';
            query = q + 1;
        }
        // الترويسات + طول الجسم.
        NvValue headers_dict = nv_dict_new();
        size_t hri = nv_root_push_idx();
        *nv_root_slot(hri) = headers_dict;
        long content_length = 0;
        char *hline = NULL;
        while ((hline = strtok_r(NULL, "\r\n", &save)) != NULL) {
            char *colon = strchr(hline, ':');
            if (!colon) continue;
            *colon = '\0';
            char *hname = hline;
            char *hval = colon + 1;
            while (*hval == ' ' || *hval == '\t') hval++;
            // دمج المكرر بـ «, ».
            int idx = nv_dict_find(headers_dict.u.dict, hname);
            if (idx >= 0) {
                NvValue old = headers_dict.u.dict->entries[idx].val;
                size_t nlen = old.u.str->len + 2 + strlen(hval);
                char *joined = (char *)malloc(nlen + 1);
                memcpy(joined, old.u.str->data, old.u.str->len);
                memcpy(joined + old.u.str->len, ", ", 2);
                memcpy(joined + old.u.str->len + 2, hval, strlen(hval));
                joined[nlen] = '\0';
                nv_dict_set_raw(headers_dict.u.dict, hname, nv_str_take(joined, nlen));
            } else {
                nv_dict_set_raw(headers_dict.u.dict, hname, nv_str_cstr(hval));
            }
            if (strcasecmp(hname, "content-length") == 0) {
                content_length = atol(hval);
            }
        }
        free(head);
        // الجسم.
        unsigned char *rbody = NULL;
        size_t rbody_len = 0;
        if (content_length > 0) {
            if (content_length > NV_NET_MAX_RESPONSE) {
                const char *msg = "الطلب أكبر من الحد";
                size_t blen2 = 0;
                unsigned char *resp = nv_build_response(413, NULL, NULL, 0, (const unsigned char *)msg, strlen(msg), &blen2);
                nv_write_all(conn, resp, blen2, deadline_ms);
                free(resp);
                nv_root_pop_n(1);
                close(conn);
                continue;
            }
            rbody = (unsigned char *)malloc((size_t)content_length + 1);
            size_t got = 0;
            while (got < (size_t)content_length) {
                nv_sock_arm_timeout(conn, deadline_ms);
                ssize_t r = recv(conn, rbody + got, (size_t)content_length - got, 0);
                if (r <= 0) break;
                got += (size_t)r;
            }
            rbody_len = got;
            rbody[rbody_len] = '\0';
        }
        // قاموس الطلب.
        NvValue req_dict = nv_dict_new();
        size_t rri = nv_root_push_idx();
        *nv_root_slot(rri) = req_dict;
        nv_dict_set_raw(req_dict.u.dict, "طريقة", nv_str_cstr(method));
        nv_dict_set_raw(req_dict.u.dict, "مسار", nv_str_cstr(path));
        nv_dict_set_raw(req_dict.u.dict, "استعلام", nv_str_cstr(query));
        nv_dict_set_raw(req_dict.u.dict, "ترويسات", headers_dict);
        nv_dict_set_raw(req_dict.u.dict, "جسم", nv_str_new(rbody ? (const char *)rbody : "", rbody_len));
        free(rbody);
        nv_root_pop_n(1); // headers_dict (في قاموس الطلب الآن)
        int status = 200;
        const char **hnames = NULL;
        const char **hvalues = NULL;
        int nhdr = 0;
        unsigned char *resp_body = NULL;
        size_t resp_len = 0;
        nv_run_handler(handler, req_dict, line, col, &status, &hnames, &hvalues, &nhdr, &resp_body, &resp_len);
        nv_root_pop_n(1); // req_dict
        size_t resp_total = 0;
        unsigned char *resp = nv_build_response(status, hnames, hvalues, nhdr, resp_body, resp_len, &resp_total);
        nv_write_all(conn, resp, resp_total, deadline_ms);
        free(resp);
        free(resp_body);
        free(hnames);
        free(hvalues);
        close(conn);
    }
}

// ================= المقابس =================
// سجل المقابس الحية (معرف رقمي → fd/نوع).
#define NV_MAX_SOCKETS 64
typedef struct {
    int used;
    int kind; // 0=TCP غير متصل، 1=TCP متصل، 2=مستمع، 3=UDP
    int fd;
} NvSocketEntry;

static NvSocketEntry nv_sockets[NV_MAX_SOCKETS];
static unsigned long long nv_next_socket_id = 1;

static void nv_sock_require_net(int line, int col) {
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
}

static unsigned long long nv_sock_ids[NV_MAX_SOCKETS];

static NvSocketEntry *nv_sock_lookup(unsigned long long id, int line, int col) {
    for (int i = 0; i < NV_MAX_SOCKETS; i++) {
        if (nv_sockets[i].used && nv_sock_ids[i] == id) return &nv_sockets[i];
    }
    nv_net_err_runtime(line, col, "مقبس غير معروف: %llu", id);
}

static unsigned long long nv_sock_insert2(int kind, int fd, int line, int col) {
    int count = 0;
    for (int i = 0; i < NV_MAX_SOCKETS; i++) if (nv_sockets[i].used) count++;
    if (count >= NV_MAX_SOCKETS) {
        close(fd);
        nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل المقبس الجديد", line, col);
    }
    for (int i = 0; i < NV_MAX_SOCKETS; i++) {
        if (!nv_sockets[i].used) {
            nv_sockets[i].used = 1;
            nv_sockets[i].kind = kind;
            nv_sockets[i].fd = fd;
            nv_sock_ids[i] = nv_next_socket_id;
            return nv_next_socket_id++;
        }
    }
    close(fd);
    nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل المقبس الجديد", line, col);
}

static void nv_sock_close_entry(NvSocketEntry *e) {
    if (e->fd >= 0) close(e->fd);
    e->used = 0;
    e->fd = -1;
}

// حل مضيف + فحص IP (كما في الجلب).
static void nv_resolve_checked(const char *host, int port, struct sockaddr_storage *out, socklen_t *out_len, int line, int col) {
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", port);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = NULL;
    int gai = getaddrinfo(host, port_str, &hints, &res);
    if (gai != 0 || !res) {
        if (res) freeaddrinfo(res);
        nv_net_err_runtime(line, col, "تعذر حل اسم المضيف %s", host);
    }
    const char *cls = nv_ip_class(res->ai_addr);
    if (cls && !nv_local_host_allowed(host)) {
        char ipbuf[64];
        if (res->ai_family == AF_INET) {
            inet_ntop(AF_INET, &((struct sockaddr_in *)res->ai_addr)->sin_addr, ipbuf, sizeof(ipbuf));
        } else {
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)res->ai_addr)->sin6_addr, ipbuf, sizeof(ipbuf));
        }
        freeaddrinfo(res);
        nv_net_err_runtime(line, col, "الوصول إلى العنوان %s مرفوض (عنوان %s)", ipbuf, cls);
    }
    memcpy(out, res->ai_addr, res->ai_addrlen);
    *out_len = res->ai_addrlen;
    freeaddrinfo(res);
}

static long long nv_sock_need_id(NvValue *args, int at, const char *display, int line, int col) {
    double x = 0;
    if (args[at].tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا لكن أعطيت %s", display, nv_type_name(args[at]));
    }
    x = args[at].u.num;
    if (x != trunc(x) || x < -9.0e15 || x >= 9.0e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا", display);
    }
    long long id = (long long)x;
    if (id < 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع معرف مقبس غير سالب", display);
    }
    return id;
}

static void nv_sock_check_port(long long port, const char *display, int allow_zero, int line, int col) {
    long low = allow_zero ? 0 : 1;
    if (port < low || port > 65535) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع منفذا بين %lld و 65535", display, low);
    }
}

static NvValue nv_sock_connect_new(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 2) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اتصل تتوقع معاملين (مضيف، منفذ) أو ثلاثة (معرف، مضيف، منفذ)", line, col);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اتصل تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    if (args[1].tag != NV_NUM || args[1].u.num != trunc(args[1].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اتصل تتوقع عددا صحيحا", line, col);
    }
    long long port = (long long)args[1].u.num;
    nv_sock_check_port(port, "شبكة.اتصل", 0, line, col);
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(args[0].u.str->data, (int)port, &ss, &slen, line, col);
    long long deadline_ms = nv_net_deadline_ms();
    int fd = nv_connect_timeout((const struct sockaddr *)&ss, slen, deadline_ms);
    if (fd < 0) {
        if (errno == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاتصال (30 ثانية)", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر الاتصال بالمضيف %s", args[0].u.str->data);
    }
    nv_sock_arm_timeout(fd, deadline_ms);
    unsigned long long id = nv_sock_insert2(1, fd, line, col);
    return nv_num((double)id);
}

static NvValue nv_sock_connect_existing(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 3) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.اتصل تتوقع معاملين (مضيف، منفذ) أو ثلاثة (معرف، مضيف، منفذ)", line, col);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.اتصل", line, col);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اتصل تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[2].tag != NV_NUM || args[2].u.num != trunc(args[2].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اتصل تتوقع عددا صحيحا", line, col);
    }
    long long port = (long long)args[2].u.num;
    nv_sock_check_port(port, "شبكة.اتصل", 0, line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind == 1) {
        nv_error(NV_ERR_RUNTIME, "المقبس متصل بالفعل", line, col);
    }
    if (e->kind == 2) {
        nv_error(NV_ERR_RUNTIME, "المستمع لا يتصل — اقبل اتصالا أولا", line, col);
    }
    if (e->kind == 3) {
        nv_error(NV_ERR_RUNTIME, "مقبس UDP لا يتصل — استخدم أرسل_إلى", line, col);
    }
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(args[1].u.str->data, (int)port, &ss, &slen, line, col);
    long long deadline_ms = nv_net_deadline_ms();
    int fd = nv_connect_timeout((const struct sockaddr *)&ss, slen, deadline_ms);
    if (fd < 0) {
        if (errno == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاتصال (30 ثانية)", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر الاتصال بالمضيف %s", args[1].u.str->data);
    }
    close(e->fd);
    e->fd = fd;
    e->kind = 1;
    nv_sock_arm_timeout(fd, deadline_ms);
    return nv_num((double)id);
}

static NvValue nv_sock_create(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.مقبس تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.مقبس تتوقع نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *kind = args[0].u.str->data;
    if (strcmp(kind, "tcp") == 0) {
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل المقبس الجديد", line, col);
        unsigned long long id = nv_sock_insert2(0, fd, line, col);
        return nv_num((double)id);
    }
    if (strcmp(kind, "udp") == 0) {
        int fd = socket(AF_INET, SOCK_DGRAM, 0);
        if (fd < 0) nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل المقبس الجديد", line, col);
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        addr.sin_port = 0;
        if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
            close(fd);
            nv_error(NV_ERR_RUNTIME, "تعذر ربط المقبس على 127.0.0.1:0", line, col);
        }
        long long deadline_ms = nv_net_deadline_ms();
        nv_sock_arm_timeout(fd, deadline_ms);
        unsigned long long id = nv_sock_insert2(3, fd, line, col);
        return nv_num((double)id);
    }
    nv_error(NV_ERR_RUNTIME, "نوع المقبس يجب أن يكون tcp أو udp", line, col);
}

static NvValue nv_sock_listen(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs < 2 || nargs > 3) {
        nv_error(NV_ERR_TYPE, "الدالة شبكة.استمع تتوقع معاملين (معرف، منفذ) أو ثلاثة (مع مضيف)", line, col);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.استمع", line, col);
    if (args[1].tag != NV_NUM || args[1].u.num != trunc(args[1].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.استمع تتوقع عددا صحيحا", line, col);
    }
    long long port = (long long)args[1].u.num;
    nv_sock_check_port(port, "شبكة.استمع", 1, line, col);
    const char *host = "127.0.0.1";
    if (nargs == 3) {
        if (args[2].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.استمع تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
        }
        host = args[2].u.str->data;
    }
    if (strcmp(host, "127.0.0.1") != 0 && strcmp(host, "0.0.0.0") != 0) {
        nv_error(NV_ERR_RUNTIME, "المستمع يربط محليا فقط: المضيف 127.0.0.1 أو 0.0.0.0", line, col);
    }
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind != 0) {
        nv_error(NV_ERR_RUNTIME, "استمع يعمل على مقبس TCP غير متصل فقط", line, col);
    }
    int one = 1;
    setsockopt(e->fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (strcmp(host, "0.0.0.0") == 0) addr.sin_addr.s_addr = INADDR_ANY;
    else inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
    if (bind(e->fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        int er = errno;
        if (er == EACCES) {
            nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld — المنفذ يحتاج صلاحيات مرتفعة", "المستمع", host, port);
        }
        if (er == EADDRINUSE) {
            nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld — المنفذ مستخدم بالفعل", "المستمع", host, port);
        }
        nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld", "المستمع", host, port);
    }
    if (listen(e->fd, 64) != 0) {
        nv_error(NV_ERR_RUNTIME, "تعذر ربط المستمع", line, col);
    }
    e->kind = 2;
    return nv_num((double)id);
}

static NvValue nv_sock_accept(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أقبل تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.أقبل", line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind == 3) {
        nv_error(NV_ERR_RUNTIME, "اقبل يعمل على مستمع TCP فقط — مقبس UDP لا يقبل اتصالات", line, col);
    }
    if (e->kind != 2) {
        nv_error(NV_ERR_RUNTIME, "اقبل يعمل على مستمع TCP فقط", line, col);
    }
    long long deadline_ms = nv_net_deadline_ms();
    // انتظار بمهلة (poll كل 50ms).
    for (;;) {
        struct pollfd pfd;
        pfd.fd = e->fd;
        pfd.events = POLLIN;
        long long left = deadline_ms - nv_net_now_ms();
        if (left <= 0) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة القبول (30 ثانية) — لم يصل أي عميل", line, col);
        }
        int pr = poll(&pfd, 1, left > 50 ? 50 : (int)left);
        if (pr < 0) {
            if (errno == EINTR) continue;
            nv_error(NV_ERR_RUNTIME, "تعذر قبول الاتصال", line, col);
        }
        if (pr == 0) continue;
        struct sockaddr_in peer;
        socklen_t plen = sizeof(peer);
        int cfd = accept(e->fd, (struct sockaddr *)&peer, &plen);
        if (cfd < 0) {
            if (errno == EINTR || errno == EAGAIN) continue;
            nv_error(NV_ERR_RUNTIME, "تعذر قبول الاتصال", line, col);
        }
        nv_sock_arm_timeout(cfd, deadline_ms);
        unsigned long long nid = nv_sock_insert2(1, cfd, line, col);
        return nv_num((double)nid);
    }
}

static NvValue nv_sock_bind_udp(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.اربط تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.اربط", line, col);
    if (args[1].tag != NV_NUM || args[1].u.num != trunc(args[1].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.اربط تتوقع عددا صحيحا", line, col);
    }
    long long port = (long long)args[1].u.num;
    nv_sock_check_port(port, "شبكة.اربط", 1, line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind != 3) {
        nv_error(NV_ERR_RUNTIME, "اربط يعمل على مقبس UDP فقط", line, col);
    }
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons((uint16_t)port);
    if (bind(e->fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        int er = errno;
        if (er == EACCES) {
            nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld — المنفذ يحتاج صلاحيات مرتفعة", "المقبس", "127.0.0.1", port);
        }
        if (er == EADDRINUSE) {
            nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld — المنفذ مستخدم بالفعل", "المقبس", "127.0.0.1", port);
        }
        nv_net_err_runtime(line, col, "تعذر ربط %s على %s:%lld", "المقبس", "127.0.0.1", port);
    }
    return nv_nil();
}

static NvValue nv_sock_send(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أرسل تتوقع 2 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.أرسل", line, col);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind == 0) {
        nv_error(NV_ERR_RUNTIME, "المقبس غير متصل — اتصل أولا", line, col);
    }
    if (e->kind == 3) {
        nv_error(NV_ERR_RUNTIME, "مقبس UDP لا يستخدم أرسل واستقبل — استخدم أرسل_إلى واستقبل_من", line, col);
    }
    if (e->kind == 2) {
        nv_error(NV_ERR_RUNTIME, "المستمع لا يرسل — استخدم اقبل", line, col);
    }
    const char *text = args[1].u.str->data;
    size_t len = args[1].u.str->len;
    long long deadline_ms = nv_net_deadline_ms();
    size_t sent = 0;
    while (sent < len) {
        nv_sock_arm_timeout(e->fd, deadline_ms);
        ssize_t n = send(e->fd, text + sent, len - sent, 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة الإرسال (30 ثانية)", line, col);
                }
                continue;
            }
            nv_error(NV_ERR_RUNTIME, "تعذر الإرسال على المقبس", line, col);
        }
        sent += (size_t)n;
    }
    return nv_num((double)sent);
}

static NvValue nv_sock_recv(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.استقبل تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.استقبل", line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind == 0) {
        nv_error(NV_ERR_RUNTIME, "المقبس غير متصل — اتصل أولا", line, col);
    }
    if (e->kind == 3) {
        nv_error(NV_ERR_RUNTIME, "مقبس UDP لا يستخدم أرسل واستقبل — استخدم أرسل_إلى واستقبل_من", line, col);
    }
    if (e->kind == 2) {
        nv_error(NV_ERR_RUNTIME, "المستمع لا يستقبل — استخدم اقبل", line, col);
    }
    unsigned char *buf = (unsigned char *)malloc(64 * 1024);
    long long deadline_ms = nv_net_deadline_ms();
    for (;;) {
        nv_sock_arm_timeout(e->fd, deadline_ms);
        ssize_t n = recv(e->fd, buf, 64 * 1024, 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    free(buf);
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاستقبال (30 ثانية)", line, col);
                }
                continue;
            }
            free(buf);
            nv_error(NV_ERR_RUNTIME, "تعذر الاستقبال من المقبس", line, col);
        }
        NvValue out = nv_str_new((const char *)buf, (size_t)n);
        free(buf);
        return out;
    }
}

static NvValue nv_sock_send_to(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 4) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أرسل_إلى تتوقع 4 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.أرسل_إلى", line, col);
    if (args[1].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل_إلى تتوقع نصا لكن أعطيت %s", nv_type_name(args[1]));
    }
    if (args[2].tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل_إلى تتوقع نصا لكن أعطيت %s", nv_type_name(args[2]));
    }
    if (args[3].tag != NV_NUM || args[3].u.num != trunc(args[3].u.num)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة شبكة.أرسل_إلى تتوقع عددا صحيحا", line, col);
    }
    long long port = (long long)args[3].u.num;
    nv_sock_check_port(port, "شبكة.أرسل_إلى", 0, line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind != 3) {
        nv_error(NV_ERR_RUNTIME, "مقبس TCP لا يستخدم أرسل_إلى واستقبل_من — استخدم أرسل واستقبل", line, col);
    }
    struct sockaddr_storage ss;
    socklen_t slen = 0;
    nv_resolve_checked(args[2].u.str->data, (int)port, &ss, &slen, line, col);
    const char *text = args[1].u.str->data;
    size_t len = args[1].u.str->len;
    long long deadline_ms = nv_net_deadline_ms();
    size_t sent = 0;
    while (sent < len) {
        nv_sock_arm_timeout(e->fd, deadline_ms);
        ssize_t n = sendto(e->fd, text + sent, len - sent, 0, (const struct sockaddr *)&ss, slen);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة الإرسال (30 ثانية)", line, col);
                }
                continue;
            }
            nv_error(NV_ERR_RUNTIME, "تعذر الإرسال على المقبس", line, col);
        }
        sent += (size_t)n;
    }
    return nv_num((double)sent);
}

static NvValue nv_sock_recv_from(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.استقبل_من تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.استقبل_من", line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    if (e->kind != 3) {
        nv_error(NV_ERR_RUNTIME, "مقبس TCP لا يستخدم أرسل_إلى واستقبل_من — استخدم أرسل واستقبل", line, col);
    }
    unsigned char *buf = (unsigned char *)malloc(64 * 1024);
    long long deadline_ms = nv_net_deadline_ms();
    for (;;) {
        nv_sock_arm_timeout(e->fd, deadline_ms);
        struct sockaddr_storage peer;
        socklen_t plen = sizeof(peer);
        ssize_t n = recvfrom(e->fd, buf, 64 * 1024, 0, (struct sockaddr *)&peer, &plen);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                if (nv_net_now_ms() >= deadline_ms) {
                    free(buf);
                    nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاستقبال (30 ثانية)", line, col);
                }
                continue;
            }
            free(buf);
            nv_error(NV_ERR_RUNTIME, "تعذر الاستقبال من المقبس", line, col);
        }
        char ipbuf[64];
        int pport = 0;
        if (peer.ss_family == AF_INET) {
            inet_ntop(AF_INET, &((struct sockaddr_in *)&peer)->sin_addr, ipbuf, sizeof(ipbuf));
            pport = ntohs(((struct sockaddr_in *)&peer)->sin_port);
        } else {
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)&peer)->sin6_addr, ipbuf, sizeof(ipbuf));
            pport = ntohs(((struct sockaddr_in6 *)&peer)->sin6_port);
        }
        NvValue out = nv_list_new();
        size_t ri = nv_root_push_idx();
        *nv_root_slot(ri) = out;
        nv_list_push_raw(out.u.list, nv_str_new((const char *)buf, (size_t)n));
        nv_list_push_raw(out.u.list, nv_str_cstr(ipbuf));
        nv_list_push_raw(out.u.list, nv_num((double)pport));
        nv_root_pop_n(1);
        free(buf);
        return out;
    }
}

static NvValue nv_sock_close(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.أغلق تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.أغلق", line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    nv_sock_close_entry(e);
    return nv_nil();
}

static NvValue nv_sock_local_port(NvValue *args, int nargs, int line, int col) {
    nv_sock_require_net(line, col);
    if (nargs != 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة شبكة.المنفذ تتوقع 1 معاملات لكن أعطيت %d", nargs);
    }
    long long id = nv_sock_need_id(args, 0, "شبكة.المنفذ", line, col);
    NvSocketEntry *e = nv_sock_lookup((unsigned long long)id, line, col);
    struct sockaddr_storage ss;
    socklen_t slen = sizeof(ss);
    if (getsockname(e->fd, (struct sockaddr *)&ss, &slen) != 0) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة منفذ المقبس", line, col);
    }
    int p = 0;
    if (ss.ss_family == AF_INET) p = ntohs(((struct sockaddr_in *)&ss)->sin_port);
    else p = ntohs(((struct sockaddr_in6 *)&ss)->sin6_port);
    return nv_num((double)p);
}

// ---------- توزيع شبكة.اتصل (2 أو 3 وسائط) ----------
static NvValue nv_net_connect_dispatch(NvValue *args, int nargs, int line, int col) {
    if (nargs == 2) return nv_sock_connect_new(args, nargs, line, col);
    if (nargs == 3) return nv_sock_connect_existing(args, nargs, line, col);
    nv_error(NV_ERR_TYPE, "الدالة شبكة.اتصل تتوقع معاملين (مضيف، منفذ) أو ثلاثة (معرف، مضيف، منفذ)", line, col);
}

// موزّع أرسل (اجتماع 29): نص أولا يعني رابطا فيُرسل
// POST؛ وغير النص يعني معرف مقبس (المهمة 13).
static NvValue nv_net_send_dispatch(NvValue *args, int nargs, int line, int col) {
    if (nargs >= 1 && args[0].tag == NV_STR) {
        return nv_net_post(args, nargs, line, col);
    }
    return nv_sock_send(args, nargs, line, col);
}

// غلاف اخدم الأصلي: المعالج وسيط رابع.
static NvValue nv_net_serve_native(NvValue *args, int nargs, int line, int col) {
    return nv_net_serve(args[3], args, nargs, line, col);
}


// ----- لينكس8-طبقة1-مرآة: مساعدات الضوابط -----

static void nv_setter_bad(const char *display, const char *key, int line, int col) {
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s %s غير صالح", display, key);
}

static double nv_setter_number(const char *display, NvValue *v, int line, int col) {
    if (v->tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا لكن أعطيت %s", display, nv_type_name(*v));
    }
    return v->u.num;
}

static long long nv_setter_integer(const char *display, NvValue *v, int line, int col) {
    double x = nv_setter_number(display, v, line, col);
    if (x != floor(x) || fabs(x) >= 9e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا", display);
    }
    return (long long)x;
}

static const char *nv_setter_string(const char *display, NvValue *v, int line, int col) {
    if (v->tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصا لكن أعطيت %s", display, nv_type_name(*v));
    }
    return v->u.str->data;
}

static int nv_setter_bool(const char *display, NvValue *v, int line, int col) {
    if (v->tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع منطقيا لكن أعطيت %s", display, nv_type_name(*v));
    }
    return v->u.boolean;
}

static NvValue nv_setter_head(NvValue *args, int nargs, const char *display, int line, int col, int expected) {
    if (nargs != expected) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع %d معاملات لكن أعطيت %d", display, expected, nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(args[0]));
    }
    return nv_clone_value(args[0]);
}

static NvDict *nv_setter_nested(NvValue *out, const char *key) {
    int i = nv_dict_find(out->u.dict, key);
    if (i >= 0 && out->u.dict->entries[i].val.tag == NV_DICT) {
        return out->u.dict->entries[i].val.u.dict;
    }
    NvValue nd = nv_dict_new();
    nv_dict_set_raw(out->u.dict, key, nd);
    i = nv_dict_find(out->u.dict, key);
    return out->u.dict->entries[i].val.u.dict;
}

static NvList *nv_setter_list(NvValue *out, const char *key) {
    int i = nv_dict_find(out->u.dict, key);
    if (i >= 0 && out->u.dict->entries[i].val.tag == NV_LIST) {
        return out->u.dict->entries[i].val.u.list;
    }
    NvValue nl = nv_list_new();
    nv_dict_set_raw(out->u.dict, key, nl);
    i = nv_dict_find(out->u.dict, key);
    return out->u.dict->entries[i].val.u.list;
}

static NvValue nv_net_setter_0(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.خلال_ثوان";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    double v = nv_setter_number(display, &args[1], line, col);
    if (!(v > 0.0) || !isfinite(v)) nv_setter_bad(display, "مهلة", line, col);
    nv_dict_set_raw(out.u.dict, "مهلة", nv_num(v));
    return out;
}

static NvValue nv_net_setter_1(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بسرعة_قصوى";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v <= 0) nv_setter_bad(display, "حد_سرعة_التنزيل", line, col);
    nv_dict_set_raw(out.u.dict, "حد_سرعة_التنزيل", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_2(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بحد_سرعة_الرفع";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v <= 0) nv_setter_bad(display, "حد_سرعة_الرفع", line, col);
    nv_dict_set_raw(out.u.dict, "حد_سرعة_الرفع", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_3(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بحد_الحجم";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v <= 0) nv_setter_bad(display, "حد_الحجم", line, col);
    nv_dict_set_raw(out.u.dict, "حد_الحجم", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_4(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.أعد_عند_الفشل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v < 1 || v > 100) nv_setter_bad(display, "محاولات", line, col);
    nv_dict_set_raw(out.u.dict, "محاولات", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_5(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتأخير_المحاولات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    double v = nv_setter_number(display, &args[1], line, col);
    if (v < 0.0 || !isfinite(v)) nv_setter_bad(display, "تأخير", line, col);
    nv_dict_set_raw(out.u.dict, "تأخير", nv_num(v));
    return out;
}

static NvValue nv_net_setter_6(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأقصى_زمن_المحاولات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    double v = nv_setter_number(display, &args[1], line, col);
    if (!(v > 0.0) || !isfinite(v)) nv_setter_bad(display, "أقصى_زمن_المحاولات", line, col);
    nv_dict_set_raw(out.u.dict, "أقصى_زمن_المحاولات", nv_num(v));
    return out;
}

static NvValue nv_net_setter_7(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأعد_عند_الرفض";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أعد_عند_الرفض", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_8(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأعد_بلا_نهاية";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أعد_بلا_نهاية", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_9(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بصامت";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "صامت", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_10(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأخطاء_فقط";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أخطاء_فقط", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_11(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.أظهر_التقدم";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أظهر_التقدم", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_12(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمفصل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "مفصل", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_13(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باتباع_التوجيه";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "اتباع_التوجيه", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_14(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأقصى_توجيه";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v < 0) nv_setter_bad(display, "أقصى_التوجيه", line, col);
    nv_dict_set_raw(out.u.dict, "أقصى_التوجيه", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_15(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتوجيه_لموثوق_فقط";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "توجيه_لموثوق_فقط", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_16(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.ببروتوكولات_التوجيه";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(args[1]));
    }
    NvValue list = nv_list_new();
    for (int i = 0; i < args[1].u.list->len; i++) {
        NvValue *it = &args[1].u.list->items[i];
        if (it->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عناصر نصية فقط لكن أعطيت %s", display, nv_type_name(*it));
        }
        nv_list_push_raw(list.u.list, nv_str_cstr(it->u.str->data));
    }
    nv_dict_set_raw(out.u.dict, "بروتوكولات_التوجيه", list);
    return out;
}

static NvValue nv_net_setter_17(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.عبر_وكيل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (v[0] == '\0') nv_setter_bad(display, "وكيل", line, col);
    nv_dict_set_raw(out.u.dict, "وكيل", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_18(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.عبر_وكيل_socks5";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (v[0] == '\0') nv_setter_bad(display, "وكيل_socks5", line, col);
    nv_dict_set_raw(out.u.dict, "وكيل_socks5", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_19(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بلا_وكيل_ل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(args[1]));
    }
    NvValue list = nv_list_new();
    for (int i = 0; i < args[1].u.list->len; i++) {
        NvValue *it = &args[1].u.list->items[i];
        if (it->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عناصر نصية فقط لكن أعطيت %s", display, nv_type_name(*it));
        }
        nv_list_push_raw(list.u.list, nv_str_cstr(it->u.str->data));
    }
    nv_dict_set_raw(out.u.dict, "بلا_وكيل_ل", list);
    return out;
}

static NvValue nv_net_setter_20(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بنفق_الوكيل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "نفق_الوكيل", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_21(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمصادقة_الوكيل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *user = nv_setter_string(display, &args[1], line, col);
    const char *pass = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "مستخدم", nv_str_cstr(user));
    nv_dict_set_raw(auth.u.dict, "كلمة_سر", nv_str_cstr(pass));
    nv_dict_set_raw(out.u.dict, "مصادقة_الوكيل", auth);
    return out;
}

static NvValue nv_net_setter_22(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتحقق_الشهادة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    if (!v) {
        nv_error(NV_ERR_RUNTIME, "تجاهل التحقق من الشهادة مرفوض — الشهادة تُتحقق دائما", line, col);
    }
    nv_dict_set_raw(out.u.dict, "تحقق_الشهادة", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_23(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بملف_شهادات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "ملف_شهادات", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_24(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمجلد_شهادات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "مجلد_شهادات", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_25(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بشهادة_عميل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *cert = nv_setter_string(display, &args[1], line, col);
    const char *skey = nv_setter_string(display, &args[2], line, col);
    NvValue cc = nv_dict_new();
    nv_dict_set_raw(cc.u.dict, "شهادة", nv_str_cstr(cert));
    nv_dict_set_raw(cc.u.dict, "مفتاح", nv_str_cstr(skey));
    nv_dict_set_raw(out.u.dict, "شهادة_عميل", cc);
    return out;
}

static NvValue nv_net_setter_26(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باسم_متصفح";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "وكيل_المستخدم", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_27(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بإحالة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "إحالة", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_28(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باللغة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "لغة", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_29(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.مع_رأس";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *hname = nv_setter_string(display, &args[1], line, col);
    const char *hval = nv_setter_string(display, &args[2], line, col);
    if (!nv_std_header_name_ok(hname)) nv_setter_bad(display, "رؤوس", line, col);
    if (!nv_header_value_ok(hval)) {
        nv_error(NV_ERR_RUNTIME, "قيمة ترويسة غير صالحة — لا يُسمح بأسطر جديدة في الترويسات", line, col);
    }
    NvDict *hdrs = nv_setter_nested(&out, "رؤوس");
    nv_dict_set_raw(hdrs, hname, nv_str_cstr(hval));
    return out;
}

static NvValue nv_net_setter_30(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بلا_رأس";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (!nv_std_header_name_ok(v)) nv_setter_bad(display, "رؤوس_محذوفة", line, col);
    NvList *list = nv_setter_list(&out, "رؤوس_محذوفة");
    int found = 0;
    for (int i = 0; i < list->len; i++) {
        if (list->items[i].tag == NV_STR && strcmp(list->items[i].u.str->data, v) == 0) { found = 1; break; }
    }
    if (!found) nv_list_push_raw(list, nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_31(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.مع_كوكيز";
    if (nargs != 2 && nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 2 أو 3 معاملات لكن أعطيت %d", display, nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(args[0]));
    }
    NvValue out = nv_clone_value(args[0]);
    NvDict *cks = nv_setter_nested(&out, "كوكيز");
    if (nargs == 2) {
        const char **keys = NULL, **vals = NULL;
        int n = 0;
        nv_net_text_dict(&args[1], display, line, col, &keys, &vals, &n);
        for (int i = 0; i < n; i++) nv_dict_set_raw(cks, keys[i], nv_str_cstr(vals[i]));
    } else {
        const char *cname = nv_setter_string(display, &args[1], line, col);
        const char *cval = nv_setter_string(display, &args[2], line, col);
        nv_dict_set_raw(cks, cname, nv_str_cstr(cval));
    }
    return out;
}

static NvValue nv_net_setter_32(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتجاهل_كوكيز_الجلسة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تجاهل_كوكيز_الجلسة", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_33(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باسم_وكلمة_سر";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *user = nv_setter_string(display, &args[1], line, col);
    const char *pass = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("أساسية"));
    nv_dict_set_raw(auth.u.dict, "مستخدم", nv_str_cstr(user));
    nv_dict_set_raw(auth.u.dict, "كلمة_سر", nv_str_cstr(pass));
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_34(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمصادقة_هضمية";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *user = nv_setter_string(display, &args[1], line, col);
    const char *pass = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("هضمية"));
    nv_dict_set_raw(auth.u.dict, "مستخدم", nv_str_cstr(user));
    nv_dict_set_raw(auth.u.dict, "كلمة_سر", nv_str_cstr(pass));
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_35(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأي_مصادقة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 3);
    const char *user = nv_setter_string(display, &args[1], line, col);
    const char *pass = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("تفاوضية"));
    nv_dict_set_raw(auth.u.dict, "مستخدم", nv_str_cstr(user));
    nv_dict_set_raw(auth.u.dict, "كلمة_سر", nv_str_cstr(pass));
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_36(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.برمز";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *token = nv_setter_string(display, &args[1], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("حامل"));
    nv_dict_set_raw(auth.u.dict, "رمز", nv_str_cstr(token));
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_37(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتوقيع_أمازون";
    if (nargs < 3 || nargs > 5) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 3 إلى 5 معاملات لكن أعطيت %d", display, nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(args[0]));
    }
    NvValue out = nv_clone_value(args[0]);
    const char *akey = nv_setter_string(display, &args[1], line, col);
    const char *secret = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("أمازون"));
    nv_dict_set_raw(auth.u.dict, "مفتاح", nv_str_cstr(akey));
    nv_dict_set_raw(auth.u.dict, "سر", nv_str_cstr(secret));
    if (nargs >= 4) {
        const char *region = nv_setter_string(display, &args[3], line, col);
        nv_dict_set_raw(auth.u.dict, "منطقة", nv_str_cstr(region));
    }
    if (nargs >= 5) {
        const char *service = nv_setter_string(display, &args[4], line, col);
        nv_dict_set_raw(auth.u.dict, "خدمة", nv_str_cstr(service));
    }
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_38(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتوقيع_طلب";
    if (nargs < 3 || nargs > 4) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 3 إلى 4 معاملات لكن أعطيت %d", display, nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(args[0]));
    }
    NvValue out = nv_clone_value(args[0]);
    const char *key_id = nv_setter_string(display, &args[1], line, col);
    const char *skey = nv_setter_string(display, &args[2], line, col);
    NvValue auth = nv_dict_new();
    nv_dict_set_raw(auth.u.dict, "نوع", nv_str_cstr("توقيع"));
    nv_dict_set_raw(auth.u.dict, "معرف", nv_str_cstr(key_id));
    nv_dict_set_raw(auth.u.dict, "مفتاح", nv_str_cstr(skey));
    if (nargs >= 4) {
        const char *algo = nv_setter_string(display, &args[3], line, col);
        nv_dict_set_raw(auth.u.dict, "خوارزمية", nv_str_cstr(algo));
    }
    nv_dict_set_raw(out.u.dict, "مصادقة", auth);
    return out;
}

static NvValue nv_net_setter_39(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمنع_بيانات_الدخول";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "منع_بيانات_الدخول", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_40(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمهلة_النقل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    double v = nv_setter_number(display, &args[1], line, col);
    if (!(v > 0.0) || !isfinite(v)) nv_setter_bad(display, "مهلة_النقل", line, col);
    nv_dict_set_raw(out.u.dict, "مهلة_النقل", nv_num(v));
    return out;
}

static NvValue nv_net_setter_41(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتتبع";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "تتبع_في", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_42(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتتبع_نصي";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تتبع_نصي", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_43(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بضغط";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "ضغط", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_44(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بترميز_الرد";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (strcmp(v, "utf-8") != 0 && strcmp(v, "latin1") != 0) nv_setter_bad(display, "ترميز_الرد", line, col);
    nv_dict_set_raw(out.u.dict, "ترميز_الرد", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_45(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتحقق_الحالة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تحقق_من_الحالة", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_46(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بإصدار_http";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (strcmp(v, "1.0") != 0 && strcmp(v, "1.1") != 0) nv_setter_bad(display, "إصدار_http", line, col);
    nv_dict_set_raw(out.u.dict, "إصدار_http", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_47(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بإرسال_متقطع";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "إرسال_متقطع", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_48(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأبق_الجسم_بعد_التوجيه";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أبق_الجسم_بعد_التوجيه", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_49(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمدى";
    if (nargs != 2 && nargs != 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 2 أو 3 معاملات لكن أعطيت %d", display, nargs);
    }
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(args[0]));
    }
    NvValue out = nv_clone_value(args[0]);
    long long from = nv_setter_integer(display, &args[1], line, col);
    if (from < 0) nv_setter_bad(display, "مدى", line, col);
    NvValue range = nv_dict_new();
    nv_dict_set_raw(range.u.dict, "من", nv_num((double)from));
    if (nargs == 3) {
        long long to = nv_setter_integer(display, &args[2], line, col);
        if (to < from) nv_setter_bad(display, "مدى", line, col);
        nv_dict_set_raw(range.u.dict, "إلى", nv_num((double)to));
    }
    nv_dict_set_raw(out.u.dict, "مدى", range);
    return out;
}

static NvValue nv_net_setter_50(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بوسم_الكيان";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "وسم_الكيان", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_51(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بشرط_زمني";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);
    if (v[0] == '\0') nv_setter_bad(display, "شرط_زمني", line, col);
    nv_dict_set_raw(out.u.dict, "شرط_زمني", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_52(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بحفظ_وقت_التعديل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "حفظ_وقت_التعديل", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_53(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بمنع_الكتابة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "منع_الكتابة", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_54(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتخط_الموجود";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تخط_الموجود", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_55(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأنشئ_المجلدات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أنشئ_المجلدات", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_56(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بسجل_التنزيل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "سجل_التنزيل", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_57(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بإلحاق_السجل";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "إلحاق_السجل", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_58(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بإلى_مجلد";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    const char *v = nv_setter_string(display, &args[1], line, col);

    nv_dict_set_raw(out.u.dict, "مجلد_التنزيل", nv_str_cstr(v));
    return out;
}

static NvValue nv_net_setter_59(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بعمق_التكرار";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v < 0) nv_setter_bad(display, "عمق_التكرار", line, col);
    nv_dict_set_raw(out.u.dict, "عمق_التكرار", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_60(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتحويل_الروابط";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "حوّل_الروابط", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_61(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باحترام_الروبوتات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "احترام_الروبوتات", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_62(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بالبقاء_في_المضيف";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "البقاء_في_المضيف", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_63(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بالبقاء_في_المجلد";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "البقاء_في_المجلد", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_64(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأنواع_مقبولة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(args[1]));
    }
    NvValue list = nv_list_new();
    for (int i = 0; i < args[1].u.list->len; i++) {
        NvValue *it = &args[1].u.list->items[i];
        if (it->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عناصر نصية فقط لكن أعطيت %s", display, nv_type_name(*it));
        }
        nv_list_push_raw(list.u.list, nv_str_cstr(it->u.str->data));
    }
    nv_dict_set_raw(out.u.dict, "أنواع_مقبولة", list);
    return out;
}

static NvValue nv_net_setter_65(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأنواع_مرفوضة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(args[1]));
    }
    NvValue list = nv_list_new();
    for (int i = 0; i < args[1].u.list->len; i++) {
        NvValue *it = &args[1].u.list->items[i];
        if (it->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عناصر نصية فقط لكن أعطيت %s", display, nv_type_name(*it));
        }
        nv_list_push_raw(list.u.list, nv_str_cstr(it->u.str->data));
    }
    nv_dict_set_raw(out.u.dict, "أنواع_مرفوضة", list);
    return out;
}

static NvValue nv_net_setter_66(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأنماط_مقبولة";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    if (args[1].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(args[1]));
    }
    NvValue list = nv_list_new();
    for (int i = 0; i < args[1].u.list->len; i++) {
        NvValue *it = &args[1].u.list->items[i];
        if (it->tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عناصر نصية فقط لكن أعطيت %s", display, nv_type_name(*it));
        }
        nv_list_push_raw(list.u.list, nv_str_cstr(it->u.str->data));
    }
    nv_dict_set_raw(out.u.dict, "أنماط_مقبولة", list);
    return out;
}

static NvValue nv_net_setter_67(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتتبع_الصور";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تتبع_الصور", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_68(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتتبع_css";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "تتبع_css", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_69(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بتسطيح_المجلدات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "سطّح_المجلدات", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_70(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بحد_الملفات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    long long v = nv_setter_integer(display, &args[1], line, col);
    if (v < 0) nv_setter_bad(display, "حد_الملفات", line, col);
    nv_dict_set_raw(out.u.dict, "حد_الملفات", nv_num((double)v));
    return out;
}

static NvValue nv_net_setter_71(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.باسمح_http";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "اسمح_http", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_72(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.ببنية_المجلدات";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "بنية_المجلدات", nv_bool(v));
    return out;
}

static NvValue nv_net_setter_73(NvValue *args, int nargs, int line, int col) {
    const char *display = "شبكة.بأنشئ_مجلدات_ftp";
    NvValue out = nv_setter_head(args, nargs, display, line, col, 2);
    int v = nv_setter_bool(display, &args[1], line, col);
    nv_dict_set_raw(out.u.dict, "أنشئ_مجلدات_ftp", nv_bool(v));
    return out;
}

#else // _WIN32

// ---------- ويندوز: شبكة مرفوضة بالعربي ----------
// اجتماع 34 §49 والاجتماع 35 §50: لا مقابس ولا
// HTTP في البناء الأصلي على ويندوز حتى نقل winsock
// الكامل (مرشح لاحق بقرار مستقل)؛ فكل دالة شبكة
// ترفض بالرسالة نفسها المستعملة لباقي الوحدات
// المقيّدة بالمنصة («… غير مدعوم على هذه المنصة»).
static void nv_net_win32_refuse(const char *display, int line, int col) __attribute__((noreturn));
static void nv_net_win32_refuse(const char *display, int line, int col) {
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "%s غير مدعوم على هذه المنصة", display);
}

NvValue nv_net_fetch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اجلب", line, col);
    return nv_nil();
}

NvValue nv_net_post(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل", line, col);
    return nv_nil();
}

NvValue nv_net_download(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل", line, col);
    return nv_nil();
}

NvValue nv_net_serve(NvValue handler, NvValue *args, int nargs, int line, int col) {
    (void)handler; (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اخدم", line, col);
    return nv_nil();
}

static NvValue nv_net_connect_dispatch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اتصل", line, col);
    return nv_nil();
}

static NvValue nv_net_send_dispatch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل", line, col);
    return nv_nil();
}

static NvValue nv_net_serve_native(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اخدم", line, col);
    return nv_nil();
}

static NvValue nv_sock_create(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.مقبس", line, col);
    return nv_nil();
}

static NvValue nv_sock_listen(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.استمع", line, col);
    return nv_nil();
}

static NvValue nv_sock_accept(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أقبل", line, col);
    return nv_nil();
}

static NvValue nv_sock_bind_udp(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اربط", line, col);
    return nv_nil();
}

static NvValue nv_sock_recv(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.استقبل", line, col);
    return nv_nil();
}

static NvValue nv_sock_send_to(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_إلى", line, col);
    return nv_nil();
}

static NvValue nv_sock_recv_from(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.استقبل_من", line, col);
    return nv_nil();
}

static NvValue nv_sock_close(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أغلق", line, col);
    return nv_nil();
}

static NvValue nv_sock_local_port(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.المنفذ", line, col);
    return nv_nil();
}

NvValue nv_net_execute(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نفذ", line, col);
    return nv_nil();
}

NvValue nv_net_req_new(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.طلب_جديد", line, col);
    return nv_nil();
}

NvValue nv_net_req_show(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اعرض_الطلب", line, col);
    return nv_nil();
}

static NvValue nv2_save_in(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بحفظ_في", line, col);
    return nv_nil();
}
static NvValue nv2_one_json(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_json", line, col);
    return nv_nil();
}
static NvValue nv2_one_form(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_نموذج", line, col);
    return nv_nil();
}
static NvValue nv2_one_raw(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_خام", line, col);
    return nv_nil();
}
static NvValue nv2_one_binary(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_ثنائي", line, col);
    return nv_nil();
}
static NvValue nv2_one_from_file(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_من_ملف", line, col);
    return nv_nil();
}
static NvValue nv2_one_from_stdin(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أرسل_من_المدخل", line, col);
    return nv_nil();
}
static NvValue nv2_m_put(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.استبدل", line, col);
    return nv_nil();
}
static NvValue nv2_m_patch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عدل", line, col);
    return nv_nil();
}
static NvValue nv2_m_delete(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.احذف_مورد", line, col);
    return nv_nil();
}
static NvValue nv2_m_head(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_رؤوس", line, col);
    return nv_nil();
}
static NvValue nv2_m_options(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اطلب_خيارات", line, col);
    return nv_nil();
}
static NvValue nv2_m_custom(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اطلب_مخصص", line, col);
    return nv_nil();
}
static NvValue nv2_upload_file(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.ارفع_ملف", line, col);
    return nv_nil();
}
static NvValue nv2_upload_body_file(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.ارفع_جسم_ملف", line, col);
    return nv_nil();
}
static NvValue nv2_dl_server_name(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_باسم_الخادم", line, col);
    return nv_nil();
}
static NvValue nv2_dl_all(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_الكل", line, col);
    return nv_nil();
}
static NvValue nv2_dl_from_list(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_من_قائمة", line, col);
    return nv_nil();
}
static NvValue nv2_dl_from_stdin(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_من_المدخل", line, col);
    return nv_nil();
}
static NvValue nv2_dl_resume(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أكمل_التنزيل", line, col);
    return nv_nil();
}
static NvValue nv2_dl_site(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_الموقع", line, col);
    return nv_nil();
}
static NvValue nv3_ftp_fetch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اجلب_ftp", line, col);
    return nv_nil();
}
static NvValue nv3_ftp_download(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_ftp", line, col);
    return nv_nil();
}
static NvValue nv3_ftp_upload(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.ارفع_ftp", line, col);
    return nv_nil();
}
static NvValue nv3_ftps_fetch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اجلب_ftps", line, col);
    return nv_nil();
}
static NvValue nv3_ftps_download(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_ftps", line, col);
    return nv_nil();
}
static NvValue nv3_mqtt_sub(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اشترك_mqtt", line, col);
    return nv_nil();
}
static NvValue nv3_mqtt_pub(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.انشر_mqtt", line, col);
    return nv_nil();
}
static NvValue nv3_sftp_fetch(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اجلب_sftp", line, col);
    return nv_nil();
}
static NvValue nv3_sftp_download(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_sftp", line, col);
    return nv_nil();
}
static NvValue nv3_http3(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اطلب_http3", line, col);
    return nv_nil();
}
static NvValue nv3_bg_download(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نزل_في_الخلفية", line, col);
    return nv_nil();
}
static NvValue nv4_session_new_fn(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.جلسة_جديدة", line, col);
    return nv_nil();
}
static NvValue nv4_session_named(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.جلسة", line, col);
    return nv_nil();
}
static NvValue nv4_apply_session(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.طبق_جلسة", line, col);
    return nv_nil();
}
static NvValue nv4_save_session(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.احفظ_الجلسة", line, col);
    return nv_nil();
}
static NvValue nv4_read_session(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اقرأ_الجلسة", line, col);
    return nv_nil();
}
static NvValue nv4_read_cookies(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اقرأ_كوكيز", line, col);
    return nv_nil();
}
static NvValue nv4_save_tag(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.احفظ_الوسم", line, col);
    return nv_nil();
}
static NvValue nv4_read_tag(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اقرأ_الوسم", line, col);
    return nv_nil();
}
static NvValue nv4_read_credentials(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اقرأ_بيانات_الدخول", line, col);
    return nv_nil();
}
static NvValue nv4_read_dl_opts(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اقرأ_إعدادات_التنزيل", line, col);
    return nv_nil();
}
static NvValue nv4_write_dl_opts(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اكتب_إعدادات_التنزيل", line, col);
    return nv_nil();
}
static NvValue nv4_url_encode_fn(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.رمز_العنوان", line, col);
    return nv_nil();
}
static NvValue nv4_add_param(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أضف_معامل", line, col);
    return nv_nil();
}
static NvValue nv4_resp_headers(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.رؤوس_الرد", line, col);
    return nv_nil();
}
static NvValue nv4_headers_only(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.رؤوس_فقط", line, col);
    return nv_nil();
}
static NvValue nv4_body_only(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.جسم_فقط", line, col);
    return nv_nil();
}
static NvValue nv4_show_resp(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اعرض_الرد", line, col);
    return nv_nil();
}
static NvValue nv4_show_resp_color(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اعرض_ملونا", line, col);
    return nv_nil();
}
static NvValue nv4_format_json(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.نسق_json", line, col);
    return nv_nil();
}
static NvValue nv4_save_headers(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.احفظ_الرؤوس", line, col);
    return nv_nil();
}
static NvValue nv4_sign_amazon(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.وقع_أمازون", line, col);
    return nv_nil();
}
static NvValue nv4_sign_request(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.وقع_الطلب", line, col);
    return nv_nil();
}
static NvValue nv4_write_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اكتب_الفحص", line, col);
    return nv_nil();
}
static NvValue nv5_my_addrs(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عناويني", line, col);
    return nv_nil();
}
static NvValue nv5_iface_addr(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عنوان_واجهة", line, col);
    return nv_nil();
}
static NvValue nv5_my_addrs_short(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عناويني_مختصرة", line, col);
    return nv_nil();
}
static NvValue nv5_my_addrs_json(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عناويني_json", line, col);
    return nv_nil();
}
static NvValue nv5_my_routes(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.مساراتي", line, col);
    return nv_nil();
}
static NvValue nv5_route_for_host(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.مسار_لمضيف", line, col);
    return nv_nil();
}
static NvValue nv5_net_status(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.حالة_الشبكة", line, col);
    return nv_nil();
}
static NvValue nv5_neighbors(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.جيراني", line, col);
    return nv_nil();
}
static NvValue nv5_neighbor(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.جار", line, col);
    return nv_nil();
}
static NvValue nv5_net_stats(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.إحصاءات_الشبكة", line, col);
    return nv_nil();
}
static NvValue nv5_route_stats(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.إحصاءات_التوجيه", line, col);
    return nv_nil();
}
static NvValue nv5_route_rules(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.قواعد_التوجيه", line, col);
    return nv_nil();
}
static NvValue nv5_broadcast_addrs(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عناوين_البث", line, col);
    return nv_nil();
}
static NvValue nv5_firewall_rules(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.قواعد_الجدار", line, col);
    return nv_nil();
}
static NvValue nv5_my_connections(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اتصالاتي", line, col);
    return nv_nil();
}
static NvValue nv5_listeners(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.المستمعة", line, col);
    return nv_nil();
}
static NvValue nv5_bridges(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.الجسور", line, col);
    return nv_nil();
}
static NvValue nv5_traffic_queues(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.طوابير_الحركة", line, col);
    return nv_nil();
}
static NvValue nv5_traffic_stats(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.إحصاءات_الحركة", line, col);
    return nv_nil();
}
static NvValue nv5_device_info(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.معلومات_الجهاز", line, col);
    return nv_nil();
}
static NvValue nv5_proto_stats(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.إحصاءات_البروتوكول", line, col);
    return nv_nil();
}
static NvValue nv5_watch_changes(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.راقب_التغييرات", line, col);
    return nv_nil();
}
static NvValue nv5_traffic_rate(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.معدل_الحركة", line, col);
    return nv_nil();
}
static NvValue nv5_who_is_connected(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.من_المتصل", line, col);
    return nv_nil();
}
static NvValue nv5b_ping(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.انبض", line, col);
    return nv_nil();
}
static NvValue nv5b_ping_loop(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.انبض_مستمر", line, col);
    return nv_nil();
}
static NvValue nv5b_rtt(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.زمن_الذهاب_والعودة", line, col);
    return nv_nil();
}
static NvValue nv5b_resolve(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.حل_الاسم", line, col);
    return nv_nil();
}
static NvValue nv5b_port_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_المنفذ", line, col);
    return nv_nil();
}
static NvValue nv5b_ports_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_المنافذ", line, col);
    return nv_nil();
}
static NvValue nv5b_range_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_نطاق", line, col);
    return nv_nil();
}
static NvValue nv5b_fast_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_سريع", line, col);
    return nv_nil();
}
static NvValue nv5b_udp_check(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.افحص_udp", line, col);
    return nv_nil();
}
static NvValue nv5b_icmp_ping(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.انبض_icmp", line, col);
    return nv_nil();
}
static NvValue nv5b_trace(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.تتبع_المسار", line, col);
    return nv_nil();
}
static NvValue nv5b_tls_connect(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اتصل_مشفر", line, col);
    return nv_nil();
}
static NvValue nv5b_timeout_connect(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.اتصل_بمهلة", line, col);
    return nv_nil();
}
/* انسخ_الموقع يشارك الدالة نفسها (بلا تعريف مكرر)؛
   ورفض Win32 يسمي نزل_الموقع في الحالين. */

static NvValue nv_net_setter_0(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.خلال_ثوان", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_1(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بسرعة_قصوى", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_2(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بحد_سرعة_الرفع", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_3(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بحد_الحجم", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_4(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أعد_عند_الفشل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_5(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتأخير_المحاولات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_6(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأقصى_زمن_المحاولات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_7(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأعد_عند_الرفض", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_8(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأعد_بلا_نهاية", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_9(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بصامت", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_10(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأخطاء_فقط", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_11(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.أظهر_التقدم", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_12(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمفصل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_13(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باتباع_التوجيه", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_14(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأقصى_توجيه", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_15(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتوجيه_لموثوق_فقط", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_16(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.ببروتوكولات_التوجيه", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_17(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عبر_وكيل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_18(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.عبر_وكيل_socks5", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_19(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بلا_وكيل_ل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_20(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بنفق_الوكيل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_21(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمصادقة_الوكيل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_22(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتحقق_الشهادة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_23(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بملف_شهادات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_24(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمجلد_شهادات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_25(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بشهادة_عميل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_26(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باسم_متصفح", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_27(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بإحالة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_28(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باللغة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_29(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.مع_رأس", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_30(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بلا_رأس", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_31(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.مع_كوكيز", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_32(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتجاهل_كوكيز_الجلسة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_33(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باسم_وكلمة_سر", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_34(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمصادقة_هضمية", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_35(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأي_مصادقة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_36(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.برمز", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_37(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتوقيع_أمازون", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_38(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتوقيع_طلب", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_39(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمنع_بيانات_الدخول", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_40(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمهلة_النقل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_41(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتتبع", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_42(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتتبع_نصي", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_43(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بضغط", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_44(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بترميز_الرد", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_45(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتحقق_الحالة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_46(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بإصدار_http", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_47(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بإرسال_متقطع", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_48(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأبق_الجسم_بعد_التوجيه", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_49(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمدى", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_50(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بوسم_الكيان", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_51(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بشرط_زمني", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_52(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بحفظ_وقت_التعديل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_53(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بمنع_الكتابة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_54(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتخط_الموجود", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_55(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأنشئ_المجلدات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_56(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بسجل_التنزيل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_57(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بإلحاق_السجل", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_58(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بإلى_مجلد", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_59(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بعمق_التكرار", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_60(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتحويل_الروابط", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_61(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باحترام_الروبوتات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_62(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بالبقاء_في_المضيف", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_63(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بالبقاء_في_المجلد", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_64(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأنواع_مقبولة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_65(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأنواع_مرفوضة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_66(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأنماط_مقبولة", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_67(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتتبع_الصور", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_68(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتتبع_css", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_69(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بتسطيح_المجلدات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_70(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بحد_الملفات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_71(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.باسمح_http", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_72(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.ببنية_المجلدات", line, col);
    return nv_nil();
}

static NvValue nv_net_setter_73(NvValue *args, int nargs, int line, int col) {
    (void)args; (void)nargs;
    nv_net_win32_refuse("شبكة.بأنشئ_مجلدات_ftp", line, col);
    return nv_nil();
}
#endif // _WIN32

// ---------- تعبئة وحدة شبكة ----------
void nv_std_network_fill(NvDict *d) {
    nv_dict_set_raw(d, "اجلب", nv_native_new("اجلب", nv_net_fetch));
    nv_dict_set_raw(d, "نزل", nv_native_new("نزل", nv_net_download));
    // اخدم: معالجها دالة نيفا — تُستدعى عبر nv_net_serve
    // بوساطة nv_call (المعالج وسيط رابع لا أول).
    nv_dict_set_raw(d, "اخدم", nv_native_new("اخدم", nv_net_serve_native));
    nv_dict_set_raw(d, "اتصل", nv_native_new("اتصل", nv_net_connect_dispatch));
    nv_dict_set_raw(d, "مقبس", nv_native_new("مقبس", nv_sock_create));
    nv_dict_set_raw(d, "استمع", nv_native_new("استمع", nv_sock_listen));
    nv_dict_set_raw(d, "أقبل", nv_native_new("أقبل", nv_sock_accept));
    nv_dict_set_raw(d, "اربط", nv_native_new("اربط", nv_sock_bind_udp));
    nv_dict_set_raw(d, "أرسل", nv_native_new("أرسل", nv_net_send_dispatch));
    nv_dict_set_raw(d, "استقبل", nv_native_new("استقبل", nv_sock_recv));
    nv_dict_set_raw(d, "أرسل_إلى", nv_native_new("أرسل_إلى", nv_sock_send_to));
    nv_dict_set_raw(d, "استقبل_من", nv_native_new("استقبل_من", nv_sock_recv_from));
    nv_dict_set_raw(d, "أغلق", nv_native_new("أغلق", nv_sock_close));
    nv_dict_set_raw(d, "المنفذ", nv_native_new("المنفذ", nv_sock_local_port));
    nv_dict_set_raw(d, "طلب_جديد", nv_native_new("طلب_جديد", nv_net_req_new));
    nv_dict_set_raw(d, "نفذ", nv_native_new("نفذ", nv_net_execute));
    nv_dict_set_raw(d, "اعرض_الطلب", nv_native_new("اعرض_الطلب", nv_net_req_show));
    nv_dict_set_raw(d, "خلال_ثوان", nv_native_new("خلال_ثوان", nv_net_setter_0));
    nv_dict_set_raw(d, "بسرعة_قصوى", nv_native_new("بسرعة_قصوى", nv_net_setter_1));
    nv_dict_set_raw(d, "بحد_سرعة_الرفع", nv_native_new("بحد_سرعة_الرفع", nv_net_setter_2));
    nv_dict_set_raw(d, "بحد_الحجم", nv_native_new("بحد_الحجم", nv_net_setter_3));
    nv_dict_set_raw(d, "أعد_عند_الفشل", nv_native_new("أعد_عند_الفشل", nv_net_setter_4));
    nv_dict_set_raw(d, "بتأخير_المحاولات", nv_native_new("بتأخير_المحاولات", nv_net_setter_5));
    nv_dict_set_raw(d, "بأقصى_زمن_المحاولات", nv_native_new("بأقصى_زمن_المحاولات", nv_net_setter_6));
    nv_dict_set_raw(d, "بأعد_عند_الرفض", nv_native_new("بأعد_عند_الرفض", nv_net_setter_7));
    nv_dict_set_raw(d, "بأعد_بلا_نهاية", nv_native_new("بأعد_بلا_نهاية", nv_net_setter_8));
    nv_dict_set_raw(d, "بصامت", nv_native_new("بصامت", nv_net_setter_9));
    nv_dict_set_raw(d, "بأخطاء_فقط", nv_native_new("بأخطاء_فقط", nv_net_setter_10));
    nv_dict_set_raw(d, "أظهر_التقدم", nv_native_new("أظهر_التقدم", nv_net_setter_11));
    nv_dict_set_raw(d, "بمفصل", nv_native_new("بمفصل", nv_net_setter_12));
    nv_dict_set_raw(d, "باتباع_التوجيه", nv_native_new("باتباع_التوجيه", nv_net_setter_13));
    nv_dict_set_raw(d, "بأقصى_توجيه", nv_native_new("بأقصى_توجيه", nv_net_setter_14));
    nv_dict_set_raw(d, "بتوجيه_لموثوق_فقط", nv_native_new("بتوجيه_لموثوق_فقط", nv_net_setter_15));
    nv_dict_set_raw(d, "ببروتوكولات_التوجيه", nv_native_new("ببروتوكولات_التوجيه", nv_net_setter_16));
    nv_dict_set_raw(d, "عبر_وكيل", nv_native_new("عبر_وكيل", nv_net_setter_17));
    nv_dict_set_raw(d, "عبر_وكيل_socks5", nv_native_new("عبر_وكيل_socks5", nv_net_setter_18));
    nv_dict_set_raw(d, "بلا_وكيل_ل", nv_native_new("بلا_وكيل_ل", nv_net_setter_19));
    nv_dict_set_raw(d, "بنفق_الوكيل", nv_native_new("بنفق_الوكيل", nv_net_setter_20));
    nv_dict_set_raw(d, "بمصادقة_الوكيل", nv_native_new("بمصادقة_الوكيل", nv_net_setter_21));
    nv_dict_set_raw(d, "بتحقق_الشهادة", nv_native_new("بتحقق_الشهادة", nv_net_setter_22));
    nv_dict_set_raw(d, "بملف_شهادات", nv_native_new("بملف_شهادات", nv_net_setter_23));
    nv_dict_set_raw(d, "بمجلد_شهادات", nv_native_new("بمجلد_شهادات", nv_net_setter_24));
    nv_dict_set_raw(d, "بشهادة_عميل", nv_native_new("بشهادة_عميل", nv_net_setter_25));
    nv_dict_set_raw(d, "باسم_متصفح", nv_native_new("باسم_متصفح", nv_net_setter_26));
    nv_dict_set_raw(d, "بإحالة", nv_native_new("بإحالة", nv_net_setter_27));
    nv_dict_set_raw(d, "باللغة", nv_native_new("باللغة", nv_net_setter_28));
    nv_dict_set_raw(d, "مع_رأس", nv_native_new("مع_رأس", nv_net_setter_29));
    nv_dict_set_raw(d, "بلا_رأس", nv_native_new("بلا_رأس", nv_net_setter_30));
    nv_dict_set_raw(d, "مع_كوكيز", nv_native_new("مع_كوكيز", nv_net_setter_31));
    nv_dict_set_raw(d, "بتجاهل_كوكيز_الجلسة", nv_native_new("بتجاهل_كوكيز_الجلسة", nv_net_setter_32));
    nv_dict_set_raw(d, "باسم_وكلمة_سر", nv_native_new("باسم_وكلمة_سر", nv_net_setter_33));
    nv_dict_set_raw(d, "بمصادقة_هضمية", nv_native_new("بمصادقة_هضمية", nv_net_setter_34));
    nv_dict_set_raw(d, "بأي_مصادقة", nv_native_new("بأي_مصادقة", nv_net_setter_35));
    nv_dict_set_raw(d, "برمز", nv_native_new("برمز", nv_net_setter_36));
    nv_dict_set_raw(d, "بتوقيع_أمازون", nv_native_new("بتوقيع_أمازون", nv_net_setter_37));
    nv_dict_set_raw(d, "بتوقيع_طلب", nv_native_new("بتوقيع_طلب", nv_net_setter_38));
    nv_dict_set_raw(d, "بمنع_بيانات_الدخول", nv_native_new("بمنع_بيانات_الدخول", nv_net_setter_39));
    nv_dict_set_raw(d, "بمهلة_النقل", nv_native_new("بمهلة_النقل", nv_net_setter_40));
    nv_dict_set_raw(d, "بتتبع", nv_native_new("بتتبع", nv_net_setter_41));
    nv_dict_set_raw(d, "بتتبع_نصي", nv_native_new("بتتبع_نصي", nv_net_setter_42));
    nv_dict_set_raw(d, "بضغط", nv_native_new("بضغط", nv_net_setter_43));
    nv_dict_set_raw(d, "بترميز_الرد", nv_native_new("بترميز_الرد", nv_net_setter_44));
    nv_dict_set_raw(d, "بتحقق_الحالة", nv_native_new("بتحقق_الحالة", nv_net_setter_45));
    nv_dict_set_raw(d, "بإصدار_http", nv_native_new("بإصدار_http", nv_net_setter_46));
    nv_dict_set_raw(d, "بإرسال_متقطع", nv_native_new("بإرسال_متقطع", nv_net_setter_47));
    nv_dict_set_raw(d, "بأبق_الجسم_بعد_التوجيه", nv_native_new("بأبق_الجسم_بعد_التوجيه", nv_net_setter_48));
    nv_dict_set_raw(d, "بمدى", nv_native_new("بمدى", nv_net_setter_49));
    nv_dict_set_raw(d, "بوسم_الكيان", nv_native_new("بوسم_الكيان", nv_net_setter_50));
    nv_dict_set_raw(d, "بشرط_زمني", nv_native_new("بشرط_زمني", nv_net_setter_51));
    nv_dict_set_raw(d, "بحفظ_وقت_التعديل", nv_native_new("بحفظ_وقت_التعديل", nv_net_setter_52));
    nv_dict_set_raw(d, "بمنع_الكتابة", nv_native_new("بمنع_الكتابة", nv_net_setter_53));
    nv_dict_set_raw(d, "بتخط_الموجود", nv_native_new("بتخط_الموجود", nv_net_setter_54));
    nv_dict_set_raw(d, "بأنشئ_المجلدات", nv_native_new("بأنشئ_المجلدات", nv_net_setter_55));
    nv_dict_set_raw(d, "بسجل_التنزيل", nv_native_new("بسجل_التنزيل", nv_net_setter_56));
    nv_dict_set_raw(d, "بإلحاق_السجل", nv_native_new("بإلحاق_السجل", nv_net_setter_57));
    nv_dict_set_raw(d, "بإلى_مجلد", nv_native_new("بإلى_مجلد", nv_net_setter_58));
    nv_dict_set_raw(d, "بعمق_التكرار", nv_native_new("بعمق_التكرار", nv_net_setter_59));
    nv_dict_set_raw(d, "بتحويل_الروابط", nv_native_new("بتحويل_الروابط", nv_net_setter_60));
    nv_dict_set_raw(d, "باحترام_الروبوتات", nv_native_new("باحترام_الروبوتات", nv_net_setter_61));
    nv_dict_set_raw(d, "بالبقاء_في_المضيف", nv_native_new("بالبقاء_في_المضيف", nv_net_setter_62));
    nv_dict_set_raw(d, "بالبقاء_في_المجلد", nv_native_new("بالبقاء_في_المجلد", nv_net_setter_63));
    nv_dict_set_raw(d, "بأنواع_مقبولة", nv_native_new("بأنواع_مقبولة", nv_net_setter_64));
    nv_dict_set_raw(d, "بأنواع_مرفوضة", nv_native_new("بأنواع_مرفوضة", nv_net_setter_65));
    nv_dict_set_raw(d, "بأنماط_مقبولة", nv_native_new("بأنماط_مقبولة", nv_net_setter_66));
    nv_dict_set_raw(d, "بتتبع_الصور", nv_native_new("بتتبع_الصور", nv_net_setter_67));
    nv_dict_set_raw(d, "بتتبع_css", nv_native_new("بتتبع_css", nv_net_setter_68));
    nv_dict_set_raw(d, "بتسطيح_المجلدات", nv_native_new("بتسطيح_المجلدات", nv_net_setter_69));
    nv_dict_set_raw(d, "بحد_الملفات", nv_native_new("بحد_الملفات", nv_net_setter_70));
    nv_dict_set_raw(d, "باسمح_http", nv_native_new("باسمح_http", nv_net_setter_71));
    nv_dict_set_raw(d, "ببنية_المجلدات", nv_native_new("ببنية_المجلدات", nv_net_setter_72));
    nv_dict_set_raw(d, "بأنشئ_مجلدات_ftp", nv_native_new("بأنشئ_مجلدات_ftp", nv_net_setter_73));
    nv_dict_set_raw(d, "بحفظ_في", nv_native_new("بحفظ_في", nv2_save_in));
    nv_dict_set_raw(d, "أرسل_json", nv_native_new("أرسل_json", nv2_one_json));
    nv_dict_set_raw(d, "أرسل_نموذج", nv_native_new("أرسل_نموذج", nv2_one_form));
    nv_dict_set_raw(d, "أرسل_خام", nv_native_new("أرسل_خام", nv2_one_raw));
    nv_dict_set_raw(d, "أرسل_ثنائي", nv_native_new("أرسل_ثنائي", nv2_one_binary));
    nv_dict_set_raw(d, "أرسل_من_ملف", nv_native_new("أرسل_من_ملف", nv2_one_from_file));
    nv_dict_set_raw(d, "أرسل_من_المدخل", nv_native_new("أرسل_من_المدخل", nv2_one_from_stdin));
    nv_dict_set_raw(d, "استبدل", nv_native_new("استبدل", nv2_m_put));
    nv_dict_set_raw(d, "عدل", nv_native_new("عدل", nv2_m_patch));
    nv_dict_set_raw(d, "احذف_مورد", nv_native_new("احذف_مورد", nv2_m_delete));
    nv_dict_set_raw(d, "افحص_رؤوس", nv_native_new("افحص_رؤوس", nv2_m_head));
    nv_dict_set_raw(d, "اطلب_خيارات", nv_native_new("اطلب_خيارات", nv2_m_options));
    nv_dict_set_raw(d, "اطلب_مخصص", nv_native_new("اطلب_مخصص", nv2_m_custom));
    nv_dict_set_raw(d, "ارفع_ملف", nv_native_new("ارفع_ملف", nv2_upload_file));
    nv_dict_set_raw(d, "ارفع_جسم_ملف", nv_native_new("ارفع_جسم_ملف", nv2_upload_body_file));
    nv_dict_set_raw(d, "نزل_باسم_الخادم", nv_native_new("نزل_باسم_الخادم", nv2_dl_server_name));
    nv_dict_set_raw(d, "نزل_الكل", nv_native_new("نزل_الكل", nv2_dl_all));
    nv_dict_set_raw(d, "نزل_من_قائمة", nv_native_new("نزل_من_قائمة", nv2_dl_from_list));
    nv_dict_set_raw(d, "نزل_من_المدخل", nv_native_new("نزل_من_المدخل", nv2_dl_from_stdin));
    nv_dict_set_raw(d, "أكمل_التنزيل", nv_native_new("أكمل_التنزيل", nv2_dl_resume));
    nv_dict_set_raw(d, "نزل_الموقع", nv_native_new("نزل_الموقع", nv2_dl_site));
    nv_dict_set_raw(d, "انسخ_الموقع", nv_native_new("انسخ_الموقع", nv2_dl_site));
    nv_dict_set_raw(d, "اجلب_ftp", nv_native_new("اجلب_ftp", nv3_ftp_fetch));
    nv_dict_set_raw(d, "نزل_ftp", nv_native_new("نزل_ftp", nv3_ftp_download));
    nv_dict_set_raw(d, "ارفع_ftp", nv_native_new("ارفع_ftp", nv3_ftp_upload));
    nv_dict_set_raw(d, "اجلب_ftps", nv_native_new("اجلب_ftps", nv3_ftps_fetch));
    nv_dict_set_raw(d, "نزل_ftps", nv_native_new("نزل_ftps", nv3_ftps_download));
    nv_dict_set_raw(d, "اشترك_mqtt", nv_native_new("اشترك_mqtt", nv3_mqtt_sub));
    nv_dict_set_raw(d, "انشر_mqtt", nv_native_new("انشر_mqtt", nv3_mqtt_pub));
    nv_dict_set_raw(d, "اجلب_sftp", nv_native_new("اجلب_sftp", nv3_sftp_fetch));
    nv_dict_set_raw(d, "نزل_sftp", nv_native_new("نزل_sftp", nv3_sftp_download));
    nv_dict_set_raw(d, "اطلب_http3", nv_native_new("اطلب_http3", nv3_http3));
    nv_dict_set_raw(d, "نزل_في_الخلفية", nv_native_new("نزل_في_الخلفية", nv3_bg_download));
    nv_dict_set_raw(d, "جلسة_جديدة", nv_native_new("جلسة_جديدة", nv4_session_new_fn));
    nv_dict_set_raw(d, "جلسة", nv_native_new("جلسة", nv4_session_named));
    nv_dict_set_raw(d, "طبق_جلسة", nv_native_new("طبق_جلسة", nv4_apply_session));
    nv_dict_set_raw(d, "احفظ_الجلسة", nv_native_new("احفظ_الجلسة", nv4_save_session));
    nv_dict_set_raw(d, "اقرأ_الجلسة", nv_native_new("اقرأ_الجلسة", nv4_read_session));
    nv_dict_set_raw(d, "اقرأ_كوكيز", nv_native_new("اقرأ_كوكيز", nv4_read_cookies));
    nv_dict_set_raw(d, "احفظ_الوسم", nv_native_new("احفظ_الوسم", nv4_save_tag));
    nv_dict_set_raw(d, "اقرأ_الوسم", nv_native_new("اقرأ_الوسم", nv4_read_tag));
    nv_dict_set_raw(d, "اقرأ_بيانات_الدخول", nv_native_new("اقرأ_بيانات_الدخول", nv4_read_credentials));
    nv_dict_set_raw(d, "اقرأ_إعدادات_التنزيل", nv_native_new("اقرأ_إعدادات_التنزيل", nv4_read_dl_opts));
    nv_dict_set_raw(d, "اكتب_إعدادات_التنزيل", nv_native_new("اكتب_إعدادات_التنزيل", nv4_write_dl_opts));
    nv_dict_set_raw(d, "رمز_العنوان", nv_native_new("رمز_العنوان", nv4_url_encode_fn));
    nv_dict_set_raw(d, "أضف_معامل", nv_native_new("أضف_معامل", nv4_add_param));
    nv_dict_set_raw(d, "رؤوس_الرد", nv_native_new("رؤوس_الرد", nv4_resp_headers));
    nv_dict_set_raw(d, "رؤوس_فقط", nv_native_new("رؤوس_فقط", nv4_headers_only));
    nv_dict_set_raw(d, "جسم_فقط", nv_native_new("جسم_فقط", nv4_body_only));
    nv_dict_set_raw(d, "اعرض_الرد", nv_native_new("اعرض_الرد", nv4_show_resp));
    nv_dict_set_raw(d, "اعرض_ملونا", nv_native_new("اعرض_ملونا", nv4_show_resp_color));
    nv_dict_set_raw(d, "نسق_json", nv_native_new("نسق_json", nv4_format_json));
    nv_dict_set_raw(d, "احفظ_الرؤوس", nv_native_new("احفظ_الرؤوس", nv4_save_headers));
    nv_dict_set_raw(d, "وقع_أمازون", nv_native_new("وقع_أمازون", nv4_sign_amazon));
    nv_dict_set_raw(d, "وقع_الطلب", nv_native_new("وقع_الطلب", nv4_sign_request));
    nv_dict_set_raw(d, "اكتب_الفحص", nv_native_new("اكتب_الفحص", nv4_write_check));
    nv_dict_set_raw(d, "عناويني", nv_native_new("عناويني", nv5_my_addrs));
    nv_dict_set_raw(d, "عنوان_واجهة", nv_native_new("عنوان_واجهة", nv5_iface_addr));
    nv_dict_set_raw(d, "عناويني_مختصرة", nv_native_new("عناويني_مختصرة", nv5_my_addrs_short));
    nv_dict_set_raw(d, "عناويني_json", nv_native_new("عناويني_json", nv5_my_addrs_json));
    nv_dict_set_raw(d, "مساراتي", nv_native_new("مساراتي", nv5_my_routes));
    nv_dict_set_raw(d, "مسار_لمضيف", nv_native_new("مسار_لمضيف", nv5_route_for_host));
    nv_dict_set_raw(d, "حالة_الشبكة", nv_native_new("حالة_الشبكة", nv5_net_status));
    nv_dict_set_raw(d, "جيراني", nv_native_new("جيراني", nv5_neighbors));
    nv_dict_set_raw(d, "جار", nv_native_new("جار", nv5_neighbor));
    nv_dict_set_raw(d, "إحصاءات_الشبكة", nv_native_new("إحصاءات_الشبكة", nv5_net_stats));
    nv_dict_set_raw(d, "إحصاءات_التوجيه", nv_native_new("إحصاءات_التوجيه", nv5_route_stats));
    nv_dict_set_raw(d, "قواعد_التوجيه", nv_native_new("قواعد_التوجيه", nv5_route_rules));
    nv_dict_set_raw(d, "عناوين_البث", nv_native_new("عناوين_البث", nv5_broadcast_addrs));
    nv_dict_set_raw(d, "قواعد_الجدار", nv_native_new("قواعد_الجدار", nv5_firewall_rules));
    nv_dict_set_raw(d, "اتصالاتي", nv_native_new("اتصالاتي", nv5_my_connections));
    nv_dict_set_raw(d, "المستمعة", nv_native_new("المستمعة", nv5_listeners));
    nv_dict_set_raw(d, "الجسور", nv_native_new("الجسور", nv5_bridges));
    nv_dict_set_raw(d, "طوابير_الحركة", nv_native_new("طوابير_الحركة", nv5_traffic_queues));
    nv_dict_set_raw(d, "إحصاءات_الحركة", nv_native_new("إحصاءات_الحركة", nv5_traffic_stats));
    nv_dict_set_raw(d, "معلومات_الجهاز", nv_native_new("معلومات_الجهاز", nv5_device_info));
    nv_dict_set_raw(d, "إحصاءات_البروتوكول", nv_native_new("إحصاءات_البروتوكول", nv5_proto_stats));
    nv_dict_set_raw(d, "راقب_التغييرات", nv_native_new("راقب_التغييرات", nv5_watch_changes));
    nv_dict_set_raw(d, "معدل_الحركة", nv_native_new("معدل_الحركة", nv5_traffic_rate));
    nv_dict_set_raw(d, "من_المتصل", nv_native_new("من_المتصل", nv5_who_is_connected));
    nv_dict_set_raw(d, "انبض", nv_native_new("انبض", nv5b_ping));
    nv_dict_set_raw(d, "انبض_مستمر", nv_native_new("انبض_مستمر", nv5b_ping_loop));
    nv_dict_set_raw(d, "زمن_الذهاب_والعودة", nv_native_new("زمن_الذهاب_والعودة", nv5b_rtt));
    nv_dict_set_raw(d, "حل_الاسم", nv_native_new("حل_الاسم", nv5b_resolve));
    nv_dict_set_raw(d, "افحص_المنفذ", nv_native_new("افحص_المنفذ", nv5b_port_check));
    nv_dict_set_raw(d, "افحص_المنافذ", nv_native_new("افحص_المنافذ", nv5b_ports_check));
    nv_dict_set_raw(d, "افحص_نطاق", nv_native_new("افحص_نطاق", nv5b_range_check));
    nv_dict_set_raw(d, "افحص_سريع", nv_native_new("افحص_سريع", nv5b_fast_check));
    nv_dict_set_raw(d, "افحص_udp", nv_native_new("افحص_udp", nv5b_udp_check));
    nv_dict_set_raw(d, "انبض_icmp", nv_native_new("انبض_icmp", nv5b_icmp_ping));
    nv_dict_set_raw(d, "تتبع_المسار", nv_native_new("تتبع_المسار", nv5b_trace));
    nv_dict_set_raw(d, "اتصل_مشفر", nv_native_new("اتصل_مشفر", nv5b_tls_connect));
    nv_dict_set_raw(d, "اتصل_بمهلة", nv_native_new("اتصل_بمهلة", nv5b_timeout_connect));
}
