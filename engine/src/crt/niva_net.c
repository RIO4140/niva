// الشبكة الأصلية — خلفية C (مهمة 29).
// HTTP/1.1 عادي بمقابس POSIX؛ وHTTPS يُرفض برسالة
// عربية (يحتاج قرار TLS منفصلا). قواعد التحويل كما
// في الاجتماعات 26 و28 و29 و30 (network.rs).
#include "niva_rt.h"
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <poll.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#define NV_NET_TIMEOUT_SECS 30
#define NV_NET_MAX_RESPONSE (5 * 1024 * 1024)
#define NV_NET_MAX_REQUEST (5 * 1024 * 1024)
#define NV_NET_MAX_REDIRECTS 5
#define NV_NET_HEADER_CAP (64 * 1024)

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
} NvSink;

static void nv_sink_push(NvSink *s, const unsigned char *chunk, size_t n, int line, int col) {
    if (s->file) {
        if (s->written + n > s->limit) {
            nv_error(NV_ERR_RUNTIME, "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة", line, col);
        }
        if (fwrite(chunk, 1, n, s->file) != n) {
            nv_error(NV_ERR_RUNTIME, "تعذر كتابة الملف", line, col);
        }
        s->written += n;
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
                            NvSink *sink, int *status_out, char **location_out) {
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
    char *hline = NULL;
    while ((hline = strtok_r(NULL, "\r\n", &save)) != NULL) {
        char *colon = strchr(hline, ':');
        if (!colon) continue;
        *colon = '\0';
        char *name = hline;
        char *value = colon + 1;
        while (*value == ' ' || *value == '\t') value++;
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
    if (status >= 300 && status < 400) {
        if (location) return 1;
        nv_net_err_runtime(line, col, "الخادم رد برمز الحالة %d بلا عنوان إعادة توجيه", status);
    }
    if (status < 200 || status >= 300) {
        nv_net_err_runtime(line, col, "الخادم رد برمز الحالة %d", status);
    }
    return 0;
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
} NvRequest;

static void nv_fetch_one(const NvUrl *url, const NvRequest *req, long long deadline_ms,
                         int line, int col, NvSink *sink,
                         int *status_out, char **location_out) {
    if (strcmp(url->scheme, "https") == 0) {
        nv_error(NV_ERR_RUNTIME, "HTTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق أو المتصفح للروابط المشفرة", line, col);
    }
    // DNS مرة واحدة ثم فحص IP ثم الاتصال بنفس العنوان.
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", url->port);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = NULL;
    int gai = getaddrinfo(url->host, port_str, &hints, &res);
    if (gai != 0 || !res) {
        if (res) freeaddrinfo(res);
        nv_net_err_runtime(line, col, "تعذر حل اسم المضيف %s", url->host);
    }
    struct addrinfo *ai = res;
    const char *cls = nv_ip_class(ai->ai_addr);
    if (cls && !nv_local_host_allowed(url->host)) {
        char ipbuf[64];
        if (ai->ai_family == AF_INET) {
            inet_ntop(AF_INET, &((struct sockaddr_in *)ai->ai_addr)->sin_addr, ipbuf, sizeof(ipbuf));
        } else {
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)ai->ai_addr)->sin6_addr, ipbuf, sizeof(ipbuf));
        }
        freeaddrinfo(res);
        nv_net_err_runtime(line, col, "الوصول إلى العنوان %s مرفوض (عنوان %s)", ipbuf, cls);
    }
    int fd = nv_connect_timeout(ai->ai_addr, ai->ai_addrlen, deadline_ms);
    if (fd < 0) {
        int e = errno;
        freeaddrinfo(res);
        if (e == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاتصال (30 ثانية)", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر الاتصال بالمضيف %s", url->host);
    }
    freeaddrinfo(res);
    // بناء الطلب.
    char *target = nv_encode_target(url->path);
    char host_hdr[1024];
    if (url->port == 80) {
        snprintf(host_hdr, sizeof(host_hdr), "%s", url->host);
    } else {
        snprintf(host_hdr, sizeof(host_hdr), "%s:%d", url->host, url->port);
    }
    size_t head_cap = strlen(target) + strlen(host_hdr) + 4096;
    for (int i = 0; i < req->nhdr; i++) head_cap += strlen(req->hdr_names[i]) + strlen(req->hdr_values[i]) + 8;
    char *head = (char *)malloc(head_cap);
    int hn = snprintf(head, head_cap, "%s %s HTTP/1.1\r\nHost: %s\r\nUser-Agent: Niva/1.0\r\nAccept: */*\r\n",
                      req->is_post ? "POST" : "GET", target, host_hdr);
    for (int i = 0; i < req->nhdr; i++) {
        const char *nm = req->hdr_names[i];
        if (strcasecmp(nm, "host") == 0 || strcasecmp(nm, "content-length") == 0
            || strcasecmp(nm, "content-type") == 0 || strcasecmp(nm, "connection") == 0) {
            continue;
        }
        hn += snprintf(head + hn, head_cap - (size_t)hn, "%s: %s\r\n", nm, req->hdr_values[i]);
    }
    if (req->is_post) {
        hn += snprintf(head + hn, head_cap - (size_t)hn, "Content-Type: %s\r\nContent-Length: %zu\r\n",
                       req->content_type, req->body_len);
    }
    hn += snprintf(head + hn, head_cap - (size_t)hn, "Connection: close\r\n\r\n");
    if (nv_write_all(fd, (const unsigned char *)head, (size_t)hn, deadline_ms) != 0) {
        int e = errno;
        free(head);
        free(target);
        close(fd);
        if (e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT) {
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة إرسال الطلب", line, col);
        }
        nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", url->host);
    }
    free(head);
    if (req->is_post && req->body_len > 0) {
        if (nv_write_all(fd, req->body, req->body_len, deadline_ms) != 0) {
            int e = errno;
            free(target);
            close(fd);
            if (e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT) {
                nv_error(NV_ERR_RUNTIME, "انتهت مهلة إرسال الطلب", line, col);
            }
            nv_net_err_runtime(line, col, "تعذر إرسال الطلب إلى %s", url->host);
        }
    }
    free(target);
    int st = 0;
    char *loc = NULL;
    int rc = nv_read_response(fd, deadline_ms, line, col, sink, &st, &loc);
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
    long long deadline_ms = nv_net_deadline_ms();
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
        if (redirects >= NV_NET_MAX_REDIRECTS) {
            free(location);
            nv_url_free(&current);
            nv_error(NV_ERR_RUNTIME, "عدد إعادات التوجيه تجاوز الحد (5)", line, col);
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
        if (req->is_post && (status == 307 || status == 308)) {
            if (cross) {
                nv_net_err_runtime(line, col, "إعادة التوجيه إلى مضيف مختلف (%s) مرفوضة", next.host);
            }
        } else {
            if (req->is_post) {
                req->is_post = 0;
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
            if (ftruncate(fileno(sink->file), 0) != 0) {
                // تجاهل صامت: الملف يُستبدل لاحقا بالجسم الكامل.
            }
            fseek(sink->file, 0, SEEK_SET);
            sink->written = 0;
        } else {
            sink->len = 0;
        }
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
    NvCheckpoint cp = nv_checkpoint();
    h.prev = nv_handlers;
    h.has_except = 0;
    nv_handlers = &h;
    if (setjmp(h.jb) == 0) {
        nv_fetch_loop(url, &req, line, col, &sink);
    } else {
        nv_restore_checkpoint(cp);
        nv_handlers = h.prev;
        fclose(f);
        remove(resolved);
        free(resolved);
        free(display);
        nv_reraise_current();
    }
    nv_handlers = h.prev;
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
    NvCheckpoint cp = nv_checkpoint();
    h.prev = nv_handlers;
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
    nv_handlers = h.prev;
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
}
