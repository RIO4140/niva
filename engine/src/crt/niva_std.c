// مكتبة الوحدات الأصلية — خلفية C (مهمة 29).
// 13 وحدة: رياضيات، عشوائي، وقت، شبكة، عمليات، ذاكرة،
// ملفات، نظام، تواصل، تشفير، أمن، غير_متزامن، جهاز.
// كل رسائل الأخطاء عربية كما في المحرك (stdlib.rs).
// شبكة.اجلب/نزّل/اخدم والمقابس في niva_net.c.
#include "niva_rt.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>

// ---------- مساعدات فحص الوسائط (كما need_* في stdlib.rs) ----------
static void nv_std_need_args(const char *display, NvValue *args, int nargs, int expected, int line, int col) {
    (void)args;
    if (nargs != expected) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع %d معاملات لكن أعطيت %d", display, expected, nargs);
    }
}

static double nv_std_need_number(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.num;
}

static long long nv_std_need_integer(const char *display, NvValue v, int line, int col) {
    double x = nv_std_need_number(display, v, line, col);
    if (x != trunc(x) || x < -9.0e15 || x >= 9.0e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا", display);
    }
    return (long long)x;
}

static const char *nv_std_need_string(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصا لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.str->data;
}

static long long nv_std_need_id(const char *display, NvValue *args, int at, const char *kind_word, int line, int col) {
    long long id = nv_std_need_integer(display, args[at], line, col);
    if (id < 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع معرف %s غير سالب", display, kind_word);
    }
    return id;
}

static void nv_std_add_fn(NvDict *d, const char *name, NvNativeFn fn) {
    nv_dict_set_raw(d, name, nv_native_new(name, fn));
}

static void nv_std_add_num(NvDict *d, const char *name, double n) {
    nv_dict_set_raw(d, name, nv_num(n));
}

// ---------- حمولات المقابض (أنواعها من niva_rt.h) ----------
// kind 1=ملف 2=عملية 3=قناة 4=حلقة 5=كتلة-ذاكرة 6=مقبس.
// الجامعة تستدعي nv_handle_finalize قبل free(ptr).
struct NvStdFile { FILE *fp; };
struct NvStdProc { long pid; int waited; };
struct NvStdChannel { int read_fd; int write_fd; long pid; };
struct NvStdMemBlock { void *ptr; size_t size; };
struct NvStdSocket { int fd; };
struct NvStdLoop { void *state; };

static void nv_std_handle_finalize(int kind, void *ptr) {
    if (!ptr) return;
    switch (kind) {
        case 1: {
            struct NvStdFile *f = (struct NvStdFile *)ptr;
            if (f->fp) fclose(f->fp);
            break;
        }
        case 3: {
            struct NvStdChannel *c = (struct NvStdChannel *)ptr;
            if (c->read_fd >= 0) close(c->read_fd);
            if (c->write_fd >= 0) close(c->write_fd);
            break;
        }
        case 5: {
            struct NvStdMemBlock *b = (struct NvStdMemBlock *)ptr;
            free(b->ptr);
            break;
        }
        case 6: {
            struct NvStdSocket *s = (struct NvStdSocket *)ptr;
            if (s->fd >= 0) close(s->fd);
            break;
        }
        default:
            break;
    }
}

// ---------- تعبئة الوحدات (تعريف كل قسم في ملفه) ----------
void nv_std_math_fill(NvDict *d);
void nv_std_random_fill(NvDict *d);
void nv_std_time_fill(NvDict *d);
void nv_std_network_fill(NvDict *d);
void nv_std_processes_fill(NvDict *d);
void nv_std_memory_fill(NvDict *d);
void nv_std_files_fill(NvDict *d);
void nv_std_system_fill(NvDict *d);
void nv_std_comm_fill(NvDict *d);
void nv_std_crypto_fill(NvDict *d);
void nv_std_security_fill(NvDict *d);
void nv_std_async_fill(NvDict *d);
void nv_std_device_fill(NvDict *d);

// ---------- السجل ----------
// وحدة أصلية بالاسم أو NULL (مؤشر إلى قيمة ساكنة مخزنة).
static NvValue nv_std_modules[13];
static const char *nv_std_names[13] = {
    "رياضيات", "عشوائي", "وقت", "شبكة", "عمليات", "ذاكرة",
    "ملفات", "نظام", "تواصل", "تشفير", "أمن", "غير_متزامن", "جهاز",
};
static int nv_std_built = 0;

static void nv_std_build(void) {
    if (nv_std_built) return;
    nv_std_built = 1;
    nv_handle_set_finalizer(nv_std_handle_finalize);
    for (int i = 0; i < 13; i++) {
        nv_std_modules[i] = nv_module_new(nv_std_names[i], NULL);
        NvDict *exports = nv_std_modules[i].u.module->exports;
        if (!exports) {
            exports = nv_dict_new().u.dict;
            nv_std_modules[i].u.module->exports = exports;
        }
        switch (i) {
            case 0: nv_std_math_fill(exports); break;
            case 1: nv_std_random_fill(exports); break;
            case 2: nv_std_time_fill(exports); break;
            case 3: nv_std_network_fill(exports); break;
            case 4: nv_std_processes_fill(exports); break;
            case 5: nv_std_memory_fill(exports); break;
            case 6: nv_std_files_fill(exports); break;
            case 7: nv_std_system_fill(exports); break;
            case 8: nv_std_comm_fill(exports); break;
            case 9: nv_std_crypto_fill(exports); break;
            case 10: nv_std_security_fill(exports); break;
            case 11: nv_std_async_fill(exports); break;
            case 12: nv_std_device_fill(exports); break;
        }
        nv_add_global_root(nv_std_modules[i]);
    }
}

NvValue *nv_native_module(const char *name) {
    nv_std_build();
    for (int i = 0; i < 13; i++) {
        if (strcmp(name, nv_std_names[i]) == 0) return &nv_std_modules[i];
    }
    return NULL;
}

int nv_is_native_module(const char *name) {
    return nv_native_module(name) != NULL;
}

NvValue nv_call_native(const char *module, const char *name, NvValue *args, int nargs, int line, int col) {
    NvValue *mod = nv_native_module(module);
    if (!mod) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "وحدة غير موجودة: %s", module);
    }
    NvValue fn = nv_module_export(*mod, name, line, col);
    return nv_call(fn, args, nargs, line, col);
}
// قطعة من مكتبة الوحدات الأصلية — خلفية لغة نيفا (مهمة 29).
// الأقسام: رياضيات، عشوائي، تشفير، أمن.
// قطعة فقط لا وحدة ترجمة مستقلة: بلا أي تضمين وبلا دالة رئيسية.
// تُدمج بعد مقدمة مكتبة الوحدات (المساعدات) ووقت التشغيل.
// كل رسائل الأخطاء والنصوص الظاهرة عربية حرفيا كما في المحرك.
// المعرّفات الداخلية لاتينية فقط.

// ================= رياضيات =================

static NvValue nv_std_math_sqrt(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جذر", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.جذر", args[0], line, col);
    if (x < 0.0) {
        nv_error(NV_ERR_RUNTIME, "جذر عدد سالب غير معرف", line, col);
    }
    return nv_num(sqrt(x));
}

static NvValue nv_std_math_pow(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.قوة", args, nargs, 2, line, col);
    double base = nv_std_need_number("رياضيات.قوة", args[0], line, col);
    double exp = nv_std_need_number("رياضيات.قوة", args[1], line, col);
    return nv_num(pow(base, exp));
}

static NvValue nv_std_math_sin(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جيب", args, nargs, 1, line, col);
    return nv_num(sin(nv_std_need_number("رياضيات.جيب", args[0], line, col)));
}

static NvValue nv_std_math_cos(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جيب_تمام", args, nargs, 1, line, col);
    return nv_num(cos(nv_std_need_number("رياضيات.جيب_تمام", args[0], line, col)));
}

static NvValue nv_std_math_tan(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.ظل", args, nargs, 1, line, col);
    return nv_num(tan(nv_std_need_number("رياضيات.ظل", args[0], line, col)));
}

static NvValue nv_std_math_ln(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.لوغاريتم", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.لوغاريتم", args[0], line, col);
    if (x <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "لوغاريتم عدد غير موجب غير معرف", line, col);
    }
    return nv_num(log(x));
}

static NvValue nv_std_math_exp(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.أس_طبيعي", args, nargs, 1, line, col);
    return nv_num(exp(nv_std_need_number("رياضيات.أس_طبيعي", args[0], line, col)));
}

static NvValue nv_std_math_floor(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.أرضية", args, nargs, 1, line, col);
    return nv_num(floor(nv_std_need_number("رياضيات.أرضية", args[0], line, col)));
}

static NvValue nv_std_math_ceil(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.سقف", args, nargs, 1, line, col);
    return nv_num(ceil(nv_std_need_number("رياضيات.سقف", args[0], line, col)));
}

void nv_std_math_fill(NvDict *d) {
    nv_std_add_fn(d, "جذر", nv_std_math_sqrt);
    nv_std_add_fn(d, "قوة", nv_std_math_pow);
    nv_std_add_fn(d, "جيب", nv_std_math_sin);
    nv_std_add_fn(d, "جيب_تمام", nv_std_math_cos);
    nv_std_add_fn(d, "ظل", nv_std_math_tan);
    nv_std_add_fn(d, "لوغاريتم", nv_std_math_ln);
    nv_std_add_fn(d, "أس_طبيعي", nv_std_math_exp);
    nv_std_add_fn(d, "أرضية", nv_std_math_floor);
    nv_std_add_fn(d, "سقف", nv_std_math_ceil);
    nv_std_add_num(d, "باي", 3.14159265358979323846);
    nv_std_add_num(d, "ه", 2.71828182845904523536);
}

// ================= عشوائي =================
// مولد الحالة الواحدة: يبدأ صفرا بمعنى لم يُبذر بعد، ويُبذر
// مرة واحدة من الساعة (نانوثانية) ومعرف العملية، ثم يتقدم
// بإضافة الثابت الذهبي في كل سحب ومزج الناتج — كما في المحرك.

static uint64_t nv_std_random_state = 0;

static uint64_t nv_std_random_next(void) {
    uint64_t state = nv_std_random_state;
    if (state == 0) {
        uint64_t nanos = 0x9E3779B97F4A7C15ULL;
        struct timespec ts;
        if (clock_gettime(CLOCK_REALTIME, &ts) == 0) {
            nanos = (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
        }
        uint64_t pid_part = (uint64_t)(unsigned int)getpid() * 0x9E3779B97F4A7C15ULL;
        state = nanos ^ pid_part ^ 0x2545F4914F6CDD1DULL;
        if (state == 0) {
            state = 0x2545F4914F6CDD1DULL;
        }
        nv_std_random_state = state;
    }
    uint64_t z = state + 0x9E3779B97F4A7C15ULL;
    nv_std_random_state = z;
    uint64_t x = z;
    x = (x ^ (x >> 30)) * 0xBF58476D1CE4E5B9ULL;
    x = (x ^ (x >> 27)) * 0x94D049BB133111EBULL;
    return x ^ (x >> 31);
}

static NvValue nv_std_random_float(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عشوائي.عدد_عشوائي", args, nargs, 0, line, col);
    return nv_num((double)(nv_std_random_next() >> 11) / 9007199254740992.0);
}

static NvValue nv_std_random_int(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عشوائي.صحيح_عشوائي", args, nargs, 2, line, col);
    long long min = nv_std_need_integer("عشوائي.صحيح_عشوائي", args[0], line, col);
    long long max = nv_std_need_integer("عشوائي.صحيح_عشوائي", args[1], line, col);
    if (min > max) {
        nv_error(NV_ERR_RUNTIME, "صحيح_عشوائي: الحد الأدنى أكبر من الحد الأقصى", line, col);
    }
    uint64_t range = (uint64_t)(max - min) + 1;
    long long picked = min + (long long)(nv_std_random_next() % range);
    return nv_num((double)picked);
}

static NvValue nv_std_random_choice(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عشوائي.اختيار", args, nargs, 1, line, col);
    if (args[0].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "الدالة عشوائي.اختيار تتوقع قائمة لكن أعطيت %s",
                     nv_type_name(args[0]));
    }
    if (args[0].u.list->len == 0) {
        nv_error(NV_ERR_RUNTIME, "لا يمكن الاختيار من قائمة فارغة", line, col);
    }
    return nv_clone_value(args[0].u.list->items[nv_std_random_next() % args[0].u.list->len]);
}

void nv_std_random_fill(NvDict *d) {
    nv_std_add_fn(d, "عدد_عشوائي", nv_std_random_float);
    nv_std_add_fn(d, "صحيح_عشوائي", nv_std_random_int);
    nv_std_add_fn(d, "اختيار", nv_std_random_choice);
}

// ================= تشفير =================
// تجزئة نص إلى بصمة سداسية عشرية صغيرة (64 محرفا)،
// وعشوائية آمنة من النظام. العشوائية الآمنة تعيد قائمة
// أعداد (بايتات) كما في المحرك — لا نصا سداسيا ولا بايتات خاما.

static uint32_t nv_std_crypto_sha256_rotr(uint32_t x, unsigned n) {
    return (x >> n) | (x << (32u - n));
}

// ثوابت جولات التجزئة (64 ثابتا).
static const uint32_t nv_std_crypto_sha256_k[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
    0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
    0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
    0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
    0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
    0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
    0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
    0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
};

// القيم الأولية للتجميع (8 قيم).
static const uint32_t nv_std_crypto_sha256_h0[8] = {
    0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
    0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u,
};

// معالجة كتلة واحدة (64 بايتا) وتحديث التجميع.
static void nv_std_crypto_sha256_block(uint32_t h[8], const unsigned char *p) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) {
        w[i] = ((uint32_t)p[4 * i] << 24) | ((uint32_t)p[4 * i + 1] << 16) |
               ((uint32_t)p[4 * i + 2] << 8) | (uint32_t)p[4 * i + 3];
    }
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = nv_std_crypto_sha256_rotr(w[i - 15], 7) ^
                      nv_std_crypto_sha256_rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t s1 = nv_std_crypto_sha256_rotr(w[i - 2], 17) ^
                      nv_std_crypto_sha256_rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = h[0], b = h[1], c = h[2], d = h[3];
    uint32_t e = h[4], f = h[5], g = h[6], hh = h[7];
    for (int i = 0; i < 64; i++) {
        uint32_t s1 = nv_std_crypto_sha256_rotr(e, 6) ^
                      nv_std_crypto_sha256_rotr(e, 11) ^
                      nv_std_crypto_sha256_rotr(e, 25);
        uint32_t ch = (e & f) ^ ((~e) & g);
        uint32_t temp1 = hh + s1 + ch + nv_std_crypto_sha256_k[i] + w[i];
        uint32_t s0 = nv_std_crypto_sha256_rotr(a, 2) ^
                      nv_std_crypto_sha256_rotr(a, 13) ^
                      nv_std_crypto_sha256_rotr(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t temp2 = s0 + maj;
        hh = g;
        g = f;
        f = e;
        e = d + temp1;
        d = c;
        c = b;
        b = a;
        a = temp1 + temp2;
    }
    h[0] += a;
    h[1] += b;
    h[2] += c;
    h[3] += d;
    h[4] += e;
    h[5] += f;
    h[6] += g;
    h[7] += hh;
}

// حالة التجزئة التدفقية: تغذية بلا تخصيص كبير.
struct NvStdCryptoSha256 {
    uint32_t h[8];
    uint64_t total;
    unsigned char buf[64];
    size_t buflen;
};

static void nv_std_crypto_sha256_init(struct NvStdCryptoSha256 *s) {
    for (int i = 0; i < 8; i++) {
        s->h[i] = nv_std_crypto_sha256_h0[i];
    }
    s->total = 0;
    s->buflen = 0;
}

static void nv_std_crypto_sha256_update(struct NvStdCryptoSha256 *s,
                                        const unsigned char *data, size_t len) {
    s->total += (uint64_t)len;
    while (len > 0) {
        size_t take = 64 - s->buflen;
        if (take > len) {
            take = len;
        }
        memcpy(s->buf + s->buflen, data, take);
        s->buflen += take;
        data += take;
        len -= take;
        if (s->buflen == 64) {
            nv_std_crypto_sha256_block(s->h, s->buf);
            s->buflen = 0;
        }
    }
}

static void nv_std_crypto_sha256_final(struct NvStdCryptoSha256 *s,
                                       unsigned char out[32]) {
    uint64_t bits = s->total * 8;
    unsigned char pad = 0x80;
    unsigned char zero = 0;
    nv_std_crypto_sha256_update(s, &pad, 1);
    while (s->buflen != 56) {
        nv_std_crypto_sha256_update(s, &zero, 1);
    }
    unsigned char lenbuf[8];
    for (int i = 0; i < 8; i++) {
        lenbuf[i] = (unsigned char)(bits >> (56 - 8 * i));
    }
    nv_std_crypto_sha256_update(s, lenbuf, 8);
    for (int i = 0; i < 8; i++) {
        out[4 * i] = (unsigned char)(s->h[i] >> 24);
        out[4 * i + 1] = (unsigned char)(s->h[i] >> 16);
        out[4 * i + 2] = (unsigned char)(s->h[i] >> 8);
        out[4 * i + 3] = (unsigned char)s->h[i];
    }
}

static NvValue nv_std_crypto_hash(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تشفير.جزّئ", args, nargs, 1, line, col);
    nv_std_need_string("تشفير.جزّئ", args[0], line, col);
    struct NvStdCryptoSha256 st;
    nv_std_crypto_sha256_init(&st);
    nv_std_crypto_sha256_update(&st, (const unsigned char *)args[0].u.str->data,
                                args[0].u.str->len);
    unsigned char digest[32];
    nv_std_crypto_sha256_final(&st, digest);
    static const char hexd[] = "0123456789abcdef";
    char hex[65];
    for (int i = 0; i < 32; i++) {
        hex[2 * i] = hexd[digest[i] >> 4];
        hex[2 * i + 1] = hexd[digest[i] & 0x0f];
    }
    hex[64] = '\0';
    return nv_str_cstr(hex);
}

// الحد الأعلى للعشوائية الآمنة في الاستدعاء الواحد: 1 ميبيبايت.
#define NV_STD_CRYPTO_MAX_SECURE 1048576

static NvValue nv_std_crypto_secure_random(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تشفير.عشوائي_آمن", args, nargs, 1, line, col);
    long long n = nv_std_need_integer("تشفير.عشوائي_آمن", args[0], line, col);
    if (n < 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "الدالة تشفير.عشوائي_آمن تتوقع عددا غير سالب");
    }
    if (n > NV_STD_CRYPTO_MAX_SECURE) {
        nv_error(NV_ERR_RUNTIME, "تشفير.عشوائي_آمن: العدد تجاوز الحد (١ ميبيبايت)",
                 line, col);
    }
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة العشوائية من النظام", line, col);
    }
    unsigned char *buf = NULL;
    if (n > 0) {
        buf = (unsigned char *)malloc((size_t)n);
        if (!buf) {
            close(fd);
            nv_error(NV_ERR_RUNTIME, "تعذّر قراءة العشوائية من النظام", line, col);
        }
    }
    size_t got = 0;
    while (got < (size_t)n) {
        ssize_t r = read(fd, buf + got, (size_t)n - got);
        if (r < 0) {
            if (errno == EINTR) {
                continue;
            }
            free(buf);
            close(fd);
            nv_error(NV_ERR_RUNTIME, "تعذّر قراءة العشوائية من النظام", line, col);
        }
        if (r == 0) {
            free(buf);
            close(fd);
            nv_error(NV_ERR_RUNTIME, "تعذّر قراءة العشوائية من النظام", line, col);
        }
        got += (size_t)r;
    }
    close(fd);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    for (long long i = 0; i < n; i++) {
        nv_list_push_raw(out.u.list, nv_num((double)buf[i]));
    }
    free(buf);
    nv_root_pop();
    return out;
}

void nv_std_crypto_fill(NvDict *d) {
    nv_std_add_fn(d, "جزئ", nv_std_crypto_hash);
    nv_std_add_fn(d, "عشوائي_آمن", nv_std_crypto_secure_random);
}

// ================= أمن =================
// قراءة فقط من لينكس: مجموعات قدرات العملية من
// ‎/proc/self/status، ووحدات الأمان النشطة من ‎/sys،
// وفحص صلاحيات الوصول إلى ملف داخل مجلد البرنامج.
// أسماء القدرات معرّفات نواة لاتينية كما في المحرك.

#if defined(__linux__) || defined(__ANDROID__)
static const char *nv_std_sec_cap_names[41] = {
    "CHOWN", "DAC_OVERRIDE", "DAC_READ_SEARCH", "FOWNER", "FSETID",
    "KILL", "SETGID", "SETUID", "SETPCAP", "LINUX_IMMUTABLE",
    "NET_BIND_SERVICE", "NET_BROADCAST", "NET_ADMIN", "NET_RAW",
    "IPC_LOCK", "IPC_OWNER", "SYS_MODULE", "SYS_RAWIO", "SYS_CHROOT",
    "SYS_PTRACE", "SYS_PACCT", "SYS_ADMIN", "SYS_BOOT", "SYS_NICE",
    "SYS_RESOURCE", "SYS_TIME", "SYS_TTY_CONFIG", "MKNOD", "LEASE",
    "AUDIT_WRITE", "AUDIT_CONTROL", "SETFCAP", "MAC_OVERRIDE",
    "MAC_ADMIN", "SYSLOG", "WAKE_ALARM", "BLOCK_SUSPEND",
    "AUDIT_READ", "PERFMON", "BPF", "CHECKPOINT_RESTORE",
};

// فك قناع قدرات إلى قائمة أسماء بترتيب البتات؛
// وبت فوق 40 يدرج برقمه العشري بلا اسم مختلق.
static NvValue nv_std_sec_cap_list(uint64_t mask) {
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    for (int bit = 0; bit < 64; bit++) {
        if (((mask >> bit) & 1u) != 0) {
            if (bit < 41) {
                nv_list_push_raw(out.u.list, nv_str_cstr(nv_std_sec_cap_names[bit]));
            } else {
                char num[24];
                snprintf(num, sizeof(num), "%d", bit);
                nv_list_push_raw(out.u.list, nv_str_cstr(num));
            }
        }
    }
    nv_root_pop();
    return out;
}

// ضع قائمة في قاموس مع تجذيرها أثناء النسخ.
static void nv_std_sec_dict_set_list(NvDict *d, const char *key, NvValue list) {
    size_t idx = nv_root_push_idx();
    *nv_root_slot(idx) = list;
    nv_dict_set_raw(d, key, list);
    nv_root_pop();
}

// تحليل قيمة سداسية عشرية بعد بادئة سطر (أو 0 عند الغياب).
static int nv_std_sec_parse_hex(const char *line, const char *prefix, uint64_t *out) {
    size_t n = strlen(prefix);
    if (strncmp(line, prefix, n) != 0) {
        return 0;
    }
    const char *p = line + n;
    while (*p == ' ' || *p == '\t') {
        p++;
    }
    if (!((*p >= '0' && *p <= '9') || (*p >= 'a' && *p <= 'f') ||
          (*p >= 'A' && *p <= 'F'))) {
        return 0;
    }
    char *end = NULL;
    unsigned long long v = strtoull(p, &end, 16);
    if (end == p) {
        return 0;
    }
    while (*end == ' ' || *end == '\t' || *end == '\n' || *end == '\r') {
        end++;
    }
    if (*end != '\0') {
        return 0;
    }
    *out = (uint64_t)v;
    return 1;
}

// الأقنعة الأربعة من ‎/proc/self/status.
static void nv_std_sec_read_caps(uint64_t *eff, uint64_t *prm, uint64_t *bnd,
                                 uint64_t *inh, int line, int col) {
    FILE *f = fopen("/proc/self/status", "r");
    if (!f) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات الأمن", line, col);
    }
    int have_eff = 0, have_prm = 0, have_bnd = 0, have_inh = 0;
    uint64_t e = 0, p = 0, b = 0, i = 0;
    char buf[256];
    while (fgets(buf, sizeof(buf), f) != NULL) {
        if (!have_eff && nv_std_sec_parse_hex(buf, "CapEff:", &e)) {
            have_eff = 1;
        } else if (!have_prm && nv_std_sec_parse_hex(buf, "CapPrm:", &p)) {
            have_prm = 1;
        } else if (!have_bnd && nv_std_sec_parse_hex(buf, "CapBnd:", &b)) {
            have_bnd = 1;
        } else if (!have_inh && nv_std_sec_parse_hex(buf, "CapInh:", &i)) {
            have_inh = 1;
        }
    }
    fclose(f);
    if (!have_eff || !have_prm || !have_bnd || !have_inh) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات الأمن", line, col);
    }
    *eff = e;
    *prm = p;
    *bnd = b;
    *inh = i;
}

// أول عدد بعد بادئة سطر (أو 0 عند الغياب).
static int nv_std_sec_parse_first_num(const char *line, const char *prefix,
                                      uint32_t *out) {
    size_t n = strlen(prefix);
    if (strncmp(line, prefix, n) != 0) {
        return 0;
    }
    const char *p = line + n;
    while (*p == ' ' || *p == '\t') {
        p++;
    }
    char *end = NULL;
    unsigned long v = strtoul(p, &end, 10);
    if (end == p) {
        return 0;
    }
    *out = (uint32_t)v;
    return 1;
}

// معرفات العملية من ‎/proc/self/status: الحقيقيان
// والمجموعات التكميلية.
static void nv_std_sec_read_ids(uint32_t *uid_out, uint32_t *gid_out,
                                uint32_t **groups_out, size_t *ngroups_out,
                                int line, int col) {
    FILE *f = fopen("/proc/self/status", "r");
    if (!f) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات الأمن", line, col);
    }
    int have_uid = 0, have_gid = 0;
    uint32_t uid = 0, gid = 0;
    uint32_t *groups = NULL;
    size_t ngroups = 0, cap = 0;
    char buf[256];
    while (fgets(buf, sizeof(buf), f) != NULL) {
        if (!have_uid && nv_std_sec_parse_first_num(buf, "Uid:", &uid)) {
            have_uid = 1;
        } else if (!have_gid && nv_std_sec_parse_first_num(buf, "Gid:", &gid)) {
            have_gid = 1;
        } else if (strncmp(buf, "Groups:", 7) == 0) {
            const char *p = buf + 7;
            for (;;) {
                while (*p == ' ' || *p == '\t') {
                    p++;
                }
                if (*p == '\n' || *p == '\r' || *p == '\0') {
                    break;
                }
                char *end = NULL;
                unsigned long v = strtoul(p, &end, 10);
                if (end == p) {
                    break;
                }
                p = end;
                if (ngroups == cap) {
                    size_t ncap = cap ? cap * 2 : 16;
                    uint32_t *ng = (uint32_t *)realloc(groups, ncap * sizeof(uint32_t));
                    if (!ng) {
                        free(groups);
                        fclose(f);
                        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات الأمن",
                                 line, col);
                    }
                    groups = ng;
                    cap = ncap;
                }
                groups[ngroups++] = (uint32_t)v;
            }
        }
    }
    fclose(f);
    if (!have_uid || !have_gid) {
        free(groups);
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات الأمن", line, col);
    }
    *uid_out = uid;
    *gid_out = gid;
    *groups_out = groups;
    *ngroups_out = ngroups;
}

// منطق بتات الوصول (على نمط استدعاء النظام): فئة المالك
// إن طابق المعرف، وإلا فئة المجموعة إن طابقت، وإلا فئة
// الآخرين. الجذر يقرأ ويكتب أي شيء، والتنفيذ يحتاج
// بت تنفيذ واحدا في أي فئة.
static void nv_std_sec_access_bits(uint32_t mode, uint32_t file_uid, uint32_t file_gid,
                                   uint32_t uid, uint32_t gid,
                                   const uint32_t *groups, size_t ngroups,
                                   int *r, int *w, int *x) {
    if (uid == 0) {
        *r = 1;
        *w = 1;
        *x = (mode & 0111u) != 0;
        return;
    }
    uint32_t bits;
    if (file_uid == uid) {
        bits = (mode >> 6) & 7u;
    } else if (file_gid == gid) {
        bits = (mode >> 3) & 7u;
    } else {
        int in_groups = 0;
        for (size_t i = 0; i < ngroups; i++) {
            if (groups[i] == file_gid) {
                in_groups = 1;
                break;
            }
        }
        bits = in_groups ? (mode >> 3) & 7u : mode & 7u;
    }
    *r = (bits & 4u) != 0;
    *w = (bits & 2u) != 0;
    *x = (bits & 1u) != 0;
}

// قائمة وحدات الأمان النشطة: ملف القائمة أولا،
// وإن غاب أو فرغ فالمؤشرات المنفردة.
static NvValue nv_std_sec_mode_list(void) {
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    FILE *f = fopen("/sys/kernel/security/lsm", "r");
    if (f != NULL) {
        char *text = NULL;
        size_t len = 0, cap = 0;
        for (;;) {
            if (len == cap) {
                size_t ncap = cap ? cap * 2 : 256;
                char *nt = (char *)realloc(text, ncap);
                if (!nt) {
                    break;
                }
                text = nt;
                cap = ncap;
            }
            size_t r = fread(text + len, 1, cap - len, f);
            if (r == 0) {
                break;
            }
            len += r;
        }
        fclose(f);
        size_t start = 0;
        for (size_t i = 0; i <= len; i++) {
            if (i == len || text[i] == ',') {
                size_t a = start, b = i;
                while (a < b && (text[a] == ' ' || text[a] == '\t' ||
                                 text[a] == '\n' || text[a] == '\r')) {
                    a++;
                }
                while (b > a && (text[b - 1] == ' ' || text[b - 1] == '\t' ||
                                 text[b - 1] == '\n' || text[b - 1] == '\r')) {
                    b--;
                }
                if (b > a) {
                    char *name = (char *)malloc(b - a + 1);
                    if (name != NULL) {
                        memcpy(name, text + a, b - a);
                        name[b - a] = '\0';
                        nv_list_push_raw(out.u.list, nv_str_take(name, b - a));
                    }
                }
                start = i + 1;
            }
        }
        free(text);
    }
    if (out.u.list->len == 0) {
        static const char *paths[4] = {
            "/sys/fs/selinux/enforce",
            "/sys/kernel/security/apparmor/profiles",
            "/sys/kernel/security/tomoyo",
            "/sys/kernel/security/smackfs",
        };
        static const char *names[4] = { "selinux", "apparmor", "tomoyo", "smack" };
        for (int i = 0; i < 4; i++) {
            struct stat st;
            if (stat(paths[i], &st) == 0) {
                nv_list_push_raw(out.u.list, nv_str_cstr(names[i]));
            }
        }
    }
    nv_root_pop();
    return out;
}

// عرض مسار كما يعرضه المحرك للمسار المكتوب: المطلق
// عبر عرض المسار، والنسبي والمبدوء بعلامة البيت كما هو.
static const char *nv_std_sec_display(const char *path, char **owned) {
    if (path[0] == '/' || path[0] == '~') {
        *owned = nv_display_path(path);
        return *owned;
    }
    *owned = NULL;
    return path;
}

#endif // defined(__linux__) || defined(__ANDROID__)

static NvValue nv_std_sec_caps(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أمن.قدرات_العملية", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    uint64_t eff = 0, prm = 0, bnd = 0, inh = 0;
    nv_std_sec_read_caps(&eff, &prm, &bnd, &inh, line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_dict_new();
    *nv_root_slot(ridx) = out;
    nv_std_sec_dict_set_list(out.u.dict, "فعالة", nv_std_sec_cap_list(eff));
    nv_std_sec_dict_set_list(out.u.dict, "مسموحة", nv_std_sec_cap_list(prm));
    nv_std_sec_dict_set_list(out.u.dict, "مقيدة", nv_std_sec_cap_list(bnd));
    nv_std_sec_dict_set_list(out.u.dict, "موروثة", nv_std_sec_cap_list(inh));
    nv_root_pop();
    return out;
#else
    nv_error(NV_ERR_RUNTIME, "أمن.قدرات_العملية غير مدعوم على هذه المنصة",
             line, col);
#endif
    return nv_nil();
}

static NvValue nv_std_sec_mode(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أمن.الوضع", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    return nv_std_sec_mode_list();
#else
    nv_error(NV_ERR_RUNTIME, "أمن.الوضع غير مدعوم على هذه المنصة", line, col);
#endif
    return nv_nil();
}

static NvValue nv_std_sec_check(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أمن.تحقق", args, nargs, 1, line, col);
    const char *path = nv_std_need_string("أمن.تحقق", args[0], line, col);
#if defined(__linux__) || defined(__ANDROID__)
    char *resolved = nv_resolve_path(path, line, col);
    char *disp_owned = NULL;
    const char *disp = nv_std_sec_display(path, &disp_owned);
    struct stat st;
    if (stat(resolved, &st) != 0) {
        int e = errno;
        free(resolved);
        if (e == ENOENT) {
            nv_error_fmt(NV_ERR_IO, line, col, "الملف غير موجود: %s", disp);
        }
        nv_error_fmt(NV_ERR_IO, line, col, "تعذّر الوصول إلى الملف %s", disp);
    }
    uint32_t uid = 0, gid = 0;
    uint32_t *groups = NULL;
    size_t ngroups = 0;
    nv_std_sec_read_ids(&uid, &gid, &groups, &ngroups, line, col);
    int r = 0, w = 0, x = 0;
    nv_std_sec_access_bits((uint32_t)st.st_mode, (uint32_t)st.st_uid,
                           (uint32_t)st.st_gid, uid, gid, groups, ngroups,
                           &r, &w, &x);
    free(groups);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_dict_new();
    *nv_root_slot(ridx) = out;
    nv_dict_set_raw(out.u.dict, "قراءة", nv_bool(r));
    nv_dict_set_raw(out.u.dict, "كتابة", nv_bool(w));
    nv_dict_set_raw(out.u.dict, "تنفيذ", nv_bool(x));
    nv_root_pop();
    free(resolved);
    free(disp_owned);
    return out;
#else
    (void)path;
    nv_error(NV_ERR_RUNTIME, "أمن.تحقق غير مدعوم على هذه المنصة", line, col);
#endif
    return nv_nil();
}

void nv_std_security_fill(NvDict *d) {
    nv_std_add_fn(d, "قدرات_العملية", nv_std_sec_caps);
    nv_std_add_fn(d, "الوضع", nv_std_sec_mode);
    nv_std_add_fn(d, "تحقق", nv_std_sec_check);
}
/* قسم الوحدات: وقت + نظام — خلفية C (مهمة 29).
   قطعة تُدمج مع المقدمة (niva_std.c)؛ بلا أي سطر تضمين وبلا دالة رئيسية.
   كل رسائل الأخطاء والنصوص الظاهرة عربية حرفيا من المحرك (time.rs وsystem.rs
   وstdlib.rs)؛ والمعرّفات الداخلية لاتينية فقط.
   ملاحظة صادقة عن الأرقام: time.rs ينسّق نصوص التاريخ بأرقام مشرقية
   (٢٠٢٦-١١-٠٦) عبر eastern()، أما هذه القطعة فتعيد الأرقام غربية كما طلب
   أمر المهمة صراحة («الأرقام في النصوص المعادة غربية») — فهي تختلف هنا
   عن time.rs قصدا بأمر المهمة، لا سهوا. */

/* ---------- وقت: رياضيات التقويم ---------- */

typedef struct {
    long long year;
    long month;
    long day;
    long hour;
    long minute;
    long second;
    long weekday; /* 0 = الأحد .. 6 = السبت */
    long long epoch;
    int utc;
} NvStdTimeDt;

static long long nv_std_time_div_euclid(long long a, long long b) {
    long long q = a / b;
    long long r = a % b;
    if (r < 0) {
        q -= 1;
    }
    return q;
}

static long long nv_std_time_rem_euclid(long long a, long long b) {
    long long r = a % b;
    if (r < 0) {
        r += b;
    }
    return r;
}

static void nv_std_time_civil_from_days(long long days, long long *year, long *month, long *day) {
    long long z = days + 719468;
    long long era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned long long doe = (unsigned long long)(z - era * 146097);
    unsigned long long yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long long y = (long long)yoe + era * 400;
    unsigned long long doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    unsigned long long mp = (5 * doy + 2) / 153;
    unsigned long long d = doy - (153 * mp + 2) / 5 + 1;
    unsigned long long mo = mp < 10 ? mp + 3 : mp - 9;
    if (mo <= 2) {
        y += 1;
    }
    *year = y;
    *month = (long)mo;
    *day = (long)d;
}

static long long nv_std_time_days_from_civil(long long y, unsigned long m, unsigned long d) {
    long long yy = m <= 2 ? y - 1 : y;
    long long era = (yy >= 0 ? yy : yy - 399) / 400;
    unsigned long long yoe = (unsigned long long)(yy - era * 400);
    unsigned long long mp = m > 2 ? (unsigned long long)(m - 3) : (unsigned long long)(m + 9);
    unsigned long long doy = (153 * mp + 2) / 5 + (unsigned long long)d - 1;
    unsigned long long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (long long)doe - 719468;
}

static long nv_std_time_weekday_from_days(long long days) {
    return (long)nv_std_time_rem_euclid(days + 4, 7);
}

static int nv_std_time_is_leap(long long y) {
    return (y % 4 == 0) && (y % 100 != 0 || y % 400 == 0);
}

static long nv_std_time_days_in_month(long long y, long m) {
    static const long days[12] = { 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 };
    if (m == 2 && nv_std_time_is_leap(y)) {
        return 29;
    }
    return days[m - 1];
}

/* ---------- وقت: المنطقة الزمنية المحلية (ملف TZif) ---------- */

#if defined(__linux__) || defined(__ANDROID__)
static unsigned int nv_std_time_be_u32(const unsigned char *d, size_t off, size_t len) {
    if (off + 4 > len) {
        return 0;
    }
    return ((unsigned int)d[off] << 24) | ((unsigned int)d[off + 1] << 16) |
           ((unsigned int)d[off + 2] << 8) | (unsigned int)d[off + 3];
}

static int nv_std_time_be_i32(const unsigned char *d, size_t off, size_t len) {
    return (int)nv_std_time_be_u32(d, off, len);
}

static long long nv_std_time_be_i64(const unsigned char *d, size_t off, size_t len) {
    long long v = 0;
    size_t i;
    if (off + 8 > len) {
        return 0;
    }
    for (i = 0; i < 8; i++) {
        v = (v << 8) | (long long)d[off + i];
    }
    return v;
}

/* إزاحة UTC (بالثواني) السارية عند secs من ملف TZif (إصدار 1/2/3/4).
   تطلق خطأ وقت التشغيل العربي عند فشل القراءة أو فساد الملف. */
static long long nv_std_time_offset_in_file(const char *path, long long secs, int line, int col) {
    unsigned char *data = NULL;
    size_t len = 0;
    size_t hdr = 0;
    unsigned long isutcnt = 0;
    unsigned long isstdcnt = 0;
    unsigned long leapcnt = 0;
    unsigned long timecnt = 0;
    unsigned long typecnt = 0;
    unsigned long charcnt = 0;
    unsigned long v1_len = 0;
    unsigned char version = 0;
    long long *trans = NULL;
    unsigned char *idx = NULL;
    int *ttinfo = NULL;
    size_t ntrans = 0;
    size_t ntypes = 0;
    size_t chosen = 0;
    long long result = 0;
    size_t i = 0;

    data = nv_read_file_bytes(path, &len);
    if (!data) {
        nv_error(NV_ERR_RUNTIME, "تعذر قراءة ملف المنطقة الزمنية", line, col);
    }
    if (len < 44 || memcmp(data, "TZif", 4) != 0) {
        free(data);
        nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
    }
    isutcnt = nv_std_time_be_u32(data, hdr + 20, len);
    isstdcnt = nv_std_time_be_u32(data, hdr + 24, len);
    leapcnt = nv_std_time_be_u32(data, hdr + 28, len);
    timecnt = nv_std_time_be_u32(data, hdr + 32, len);
    typecnt = nv_std_time_be_u32(data, hdr + 36, len);
    charcnt = nv_std_time_be_u32(data, hdr + 40, len);
    v1_len = 4 * timecnt + timecnt + 6 * typecnt + charcnt + 8 * leapcnt + isstdcnt + isutcnt;
    version = data[4];
    if (version == '2' || version == '3' || version == '4') {
        size_t hdr2 = 44 + v1_len;
        size_t base = 0;
        size_t need = 0;
        if (len < hdr2 + 44 || memcmp(data + hdr2, "TZif", 4) != 0) {
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
        isutcnt = nv_std_time_be_u32(data, hdr2 + 20, len);
        isstdcnt = nv_std_time_be_u32(data, hdr2 + 24, len);
        leapcnt = nv_std_time_be_u32(data, hdr2 + 28, len);
        timecnt = nv_std_time_be_u32(data, hdr2 + 32, len);
        typecnt = nv_std_time_be_u32(data, hdr2 + 36, len);
        charcnt = nv_std_time_be_u32(data, hdr2 + 40, len);
        base = hdr2 + 44;
        need = 8 * timecnt + timecnt + 6 * typecnt + charcnt + 12 * leapcnt + isstdcnt + isutcnt;
        if (len < base + need) {
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
        ntrans = timecnt;
        ntypes = typecnt;
        trans = (long long *)malloc((ntrans == 0 ? 1 : ntrans) * sizeof(long long));
        idx = (unsigned char *)malloc((ntrans == 0 ? 1 : ntrans) * sizeof(unsigned char));
        ttinfo = (int *)malloc((ntypes == 0 ? 1 : ntypes) * sizeof(int));
        if (!trans || !idx || !ttinfo) {
            free(trans);
            free(idx);
            free(ttinfo);
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
        for (i = 0; i < ntrans; i++) {
            trans[i] = nv_std_time_be_i64(data, base + 8 * i, len);
        }
        for (i = 0; i < ntrans; i++) {
            idx[i] = data[base + 8 * ntrans + i];
        }
        for (i = 0; i < ntypes; i++) {
            ttinfo[i] = nv_std_time_be_i32(data, base + 8 * ntrans + ntrans + 6 * i, len);
        }
    } else if (version == 0) {
        size_t base = 44;
        size_t need = 4 * timecnt + timecnt + 6 * typecnt + charcnt + 8 * leapcnt + isstdcnt + isutcnt;
        if (len < base + need) {
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
        ntrans = timecnt;
        ntypes = typecnt;
        trans = (long long *)malloc((ntrans == 0 ? 1 : ntrans) * sizeof(long long));
        idx = (unsigned char *)malloc((ntrans == 0 ? 1 : ntrans) * sizeof(unsigned char));
        ttinfo = (int *)malloc((ntypes == 0 ? 1 : ntypes) * sizeof(int));
        if (!trans || !idx || !ttinfo) {
            free(trans);
            free(idx);
            free(ttinfo);
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
        for (i = 0; i < ntrans; i++) {
            trans[i] = (long long)nv_std_time_be_i32(data, base + 4 * i, len);
        }
        for (i = 0; i < ntrans; i++) {
            idx[i] = data[base + 4 * ntrans + i];
        }
        for (i = 0; i < ntypes; i++) {
            ttinfo[i] = nv_std_time_be_i32(data, base + 4 * ntrans + ntrans + 6 * i, len);
        }
    } else {
        free(data);
        nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
    }
    if (ntypes == 0) {
        free(trans);
        free(idx);
        free(ttinfo);
        free(data);
        nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
    }
    for (i = 0; i < ntrans; i++) {
        if ((size_t)idx[i] >= ntypes) {
            free(trans);
            free(idx);
            free(ttinfo);
            free(data);
            nv_error(NV_ERR_RUNTIME, "ملف المنطقة الزمنية تالف", line, col);
        }
    }
    if (ntrans == 0) {
        chosen = 0;
    } else {
        size_t lo = 0;
        size_t hi = ntrans;
        size_t found = ntrans;
        int exact = 0;
        while (lo < hi) {
            size_t mid = lo + (hi - lo) / 2;
            if (trans[mid] == secs) {
                found = mid;
                exact = 1;
                break;
            } else if (trans[mid] < secs) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if (exact) {
            chosen = (size_t)idx[found];
        } else if (lo == 0) {
            chosen = (size_t)idx[0];
        } else {
            chosen = (size_t)idx[lo - 1];
        }
    }
    result = (long long)ttinfo[chosen];
    free(trans);
    free(idx);
    free(ttinfo);
    free(data);
    return result;
}

/* مسار ملف المنطقة المحلية (يُحرَّر بـ free) أو NULL عند UTC.
   TZ اسم منطقة (مسار مطلق أو اسم تحت مجلدات zoneinfo)؛
   وTZ الفارغ يعني UTC؛ وبلا TZ يُستخدم /etc/localtime.
   سلسلة قواعد POSIX لا تُفسَّر في الإصدار 1 — يُستخدم ملف النظام بدلا منها. */
static char *nv_std_time_local_zone_path(void) {
    const char *tz = getenv("TZ");
    char buf[1024];
    static const char *dirs[3] = {
        "/usr/share/zoneinfo",
        "/system/usr/share/zoneinfo",
        "/data/misc/zoneinfo"
    };
    int i = 0;
    if (tz) {
        if (tz[0] == ':') {
            tz++;
        }
        if (tz[0] == '\0') {
            return NULL;
        }
        if (tz[0] == '/' && access(tz, F_OK) == 0) {
            return strdup(tz);
        }
        for (i = 0; i < 3; i++) {
            snprintf(buf, sizeof(buf), "%s/%s", dirs[i], tz);
            if (access(buf, F_OK) == 0) {
                return strdup(buf);
            }
        }
    }
    return strdup("/etc/localtime");
}

#endif // defined(__linux__) || defined(__ANDROID__)

/* الإزاحة المحلية (بالثواني) عند secs. خارج لينكس/أندرويد تُرفض بصدق. */
static long long nv_std_time_local_offset_at(long long secs, const char *display, int line, int col) {
#if defined(__linux__) || defined(__ANDROID__)
    char *path = NULL;
    long long off = 0;
    (void)display;
    path = nv_std_time_local_zone_path();
    if (!path) {
        return 0;
    }
    off = nv_std_time_offset_in_file(path, secs, line, col);
    free(path);
    return off;
#else
    (void)secs;
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "%s غير مدعوم على هذه المنصة", display);
    return 0;
#endif
}

/* ---------- وقت: بناء التاريخ والتحويلات ---------- */

static NvStdTimeDt nv_std_time_from_epoch(long long secs, int utc, const char *display, int line, int col) {
    long long offset = 0;
    long long local = 0;
    long long days = 0;
    long long sod = 0;
    NvStdTimeDt dt;
    offset = utc ? 0 : nv_std_time_local_offset_at(secs, display, line, col);
    local = secs + offset;
    days = nv_std_time_div_euclid(local, 86400);
    sod = nv_std_time_rem_euclid(local, 86400);
    nv_std_time_civil_from_days(days, &dt.year, &dt.month, &dt.day);
    dt.hour = (long)(sod / 3600);
    dt.minute = (long)((sod % 3600) / 60);
    dt.second = (long)(sod % 60);
    dt.weekday = nv_std_time_weekday_from_days(days);
    dt.epoch = secs;
    dt.utc = utc;
    return dt;
}

static NvStdTimeDt nv_std_time_from_components(long long year, long long month, long long day,
                                               long long hour, long long minute, long long second,
                                               int utc, const char *display, int line, int col) {
    long dim = 0;
    long long days = 0;
    long long naive = 0;
    long long epoch = 0;
    if (year < 1 || year > 9999) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع سنة بين 1 و 9999", display);
    }
    if (month < 1 || month > 12) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع شهرا بين 1 و 12", display);
    }
    dim = nv_std_time_days_in_month(year, (long)month);
    if (day < 1 || day > (long long)dim) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع يوما بين 1 و %ld في هذا الشهر", display, dim);
    }
    if (hour < 0 || hour > 23) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع ساعة بين 0 و 23", display);
    }
    if (minute < 0 || minute > 59) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع دقيقة بين 0 و 59", display);
    }
    if (second < 0 || second > 59) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع ثانية بين 0 و 59", display);
    }
    days = nv_std_time_days_from_civil(year, (unsigned long)month, (unsigned long)day);
    naive = days * 86400 + hour * 3600 + minute * 60 + second;
    if (utc) {
        epoch = naive;
    } else {
        long long off1 = nv_std_time_local_offset_at(naive, display, line, col);
        long long e1 = naive - off1;
        long long off2 = nv_std_time_local_offset_at(e1, display, line, col);
        epoch = off2 == off1 ? e1 : naive - off2;
    }
    return nv_std_time_from_epoch(epoch, utc, display, line, col);
}

static NvStdTimeDt nv_std_time_shift(const NvStdTimeDt *dt, long long delta, const char *display, int line, int col) {
    long long epoch = 0;
    if ((delta > 0 && dt->epoch > LLONG_MAX - delta) ||
        (delta < 0 && dt->epoch < LLONG_MIN - delta)) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تجاوزت نطاق الثواني", display);
    }
    epoch = dt->epoch + delta;
    return nv_std_time_from_epoch(epoch, dt->utc, display, line, col);
}

static long long nv_std_time_diff_secs(const NvStdTimeDt *a, const NvStdTimeDt *b, const char *display, int line, int col) {
    if ((b->epoch > 0 && a->epoch < LLONG_MIN + b->epoch) ||
        (b->epoch < 0 && a->epoch > LLONG_MAX + b->epoch)) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تجاوز الفرق نطاق الثواني", display);
    }
    return a->epoch - b->epoch;
}

/* ---------- وقت: التنسيق والأسماء (أرقام غربية بأمر المهمة) ---------- */

static void nv_std_time_format_datetime(const NvStdTimeDt *dt, char *buf, size_t n) {
    snprintf(buf, n, "%04lld-%02ld-%02ld %02ld:%02ld:%02ld",
             dt->year, dt->month, dt->day, dt->hour, dt->minute, dt->second);
}

static void nv_std_time_format_date(const NvStdTimeDt *dt, char *buf, size_t n) {
    snprintf(buf, n, "%04lld-%02ld-%02ld", dt->year, dt->month, dt->day);
}

static void nv_std_time_format_time(const NvStdTimeDt *dt, char *buf, size_t n) {
    snprintf(buf, n, "%02ld:%02ld:%02ld", dt->hour, dt->minute, dt->second);
}

static const char *nv_std_time_weekday_name_str(long weekday) {
    static const char *names[7] = {
        "الأحد", "الاثنين", "الثلاثاء", "الأربعاء", "الخميس", "الجمعة", "السبت"
    };
    return names[weekday];
}

static const char *nv_std_time_month_name_str(long month) {
    static const char *names[12] = {
        "يناير", "فبراير", "مارس", "أبريل", "مايو", "يونيو", "يوليو", "أغسطس", "سبتمبر",
        "أكتوبر", "نوفمبر", "ديسمبر"
    };
    return names[month - 1];
}

/* ---------- وقت: قاموس التاريخ ---------- */

static NvValue nv_std_time_to_dict(const NvStdTimeDt *dt) {
    NvValue v = nv_dict_new();
    NvDict *d = v.u.dict;
    nv_dict_set_raw(d, "سنة", nv_num((double)dt->year));
    nv_dict_set_raw(d, "شهر", nv_num((double)dt->month));
    nv_dict_set_raw(d, "يوم", nv_num((double)dt->day));
    nv_dict_set_raw(d, "ساعة", nv_num((double)dt->hour));
    nv_dict_set_raw(d, "دقيقة", nv_num((double)dt->minute));
    nv_dict_set_raw(d, "ثانية", nv_num((double)dt->second));
    nv_dict_set_raw(d, "يوم_الأسبوع", nv_num((double)dt->weekday));
    nv_dict_set_raw(d, "ثواني", nv_num((double)dt->epoch));
    nv_dict_set_raw(d, "عالمي", nv_bool(dt->utc));
    return v;
}

static const NvValue *nv_std_time_dict_find(const NvDict *d, const char *key) {
    size_t i = 0;
    for (i = 0; i < d->len; i++) {
        if (strcmp(d->entries[i].key, key) == 0) {
            return &d->entries[i].val;
        }
    }
    return NULL;
}

static long long nv_std_time_dict_number(const NvDict *d, const char *key, const char *display, int line, int col) {
    const NvValue *v = nv_std_time_dict_find(d, key);
    if (v && v->tag == NV_NUM && v->u.num == trunc(v->u.num) && fabs(v->u.num) < 9.0e15) {
        return (long long)v->u.num;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموس تاريخ فيه %s عددا صحيحا", display, key);
    return 0;
}

static int nv_std_time_dict_bool(const NvDict *d, const char *key, const char *display, int line, int col) {
    const NvValue *v = nv_std_time_dict_find(d, key);
    if (v && v->tag == NV_BOOL) {
        return v->u.boolean;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموس تاريخ فيه %s منطقيا", display, key);
    return 0;
}

/* قراءة قاموس تاريخ: «ثواني» مصدر الحقيقة عند وجودها
   (تُعاد الحقول حسابا منها)؛ وبدونها تُستخدم المكونات
   (وتُتحقق)؛ و«عالمي» محلي عند غيابه. */
static NvStdTimeDt nv_std_time_from_dict(NvValue v, const char *display, int line, int col) {
    const NvDict *d = NULL;
    int utc = 0;
    if (v.tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموس تاريخ لكن أعطيت %s", display, nv_type_name(v));
    }
    d = v.u.dict;
    utc = nv_std_time_dict_find(d, "عالمي") ? nv_std_time_dict_bool(d, "عالمي", display, line, col) : 0;
    if (nv_std_time_dict_find(d, "ثواني")) {
        long long epoch = nv_std_time_dict_number(d, "ثواني", display, line, col);
        return nv_std_time_from_epoch(epoch, utc, display, line, col);
    }
    {
        long long year = nv_std_time_dict_number(d, "سنة", display, line, col);
        long long month = nv_std_time_dict_number(d, "شهر", display, line, col);
        long long day = nv_std_time_dict_number(d, "يوم", display, line, col);
        long long hour = nv_std_time_dict_number(d, "ساعة", display, line, col);
        long long minute = nv_std_time_dict_number(d, "دقيقة", display, line, col);
        long long second = nv_std_time_dict_number(d, "ثانية", display, line, col);
        return nv_std_time_from_components(year, month, day, hour, minute, second, utc, display, line, col);
    }
}

static long long nv_std_time_now_epoch(void) {
    time_t t = time(NULL);
    if (t == (time_t)-1) {
        return 0;
    }
    return (long long)t;
}

/* عدد ثواني يونكس من وسيط عددي (يقبل الكسر — فيُهمل الجزء الكسري). */
static long long nv_std_time_need_epoch(const char *display, NvValue v, int line, int col) {
    double x = nv_std_need_number(display, v, line, col);
    if (!isfinite(x) || fabs(x) >= 9.0e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا للثواني", display);
    }
    return (long long)floor(x);
}

/* ---------- وقت: الدوال الأصلية ---------- */

static NvValue nv_std_time_now(NvValue *args, int nargs, int line, int col) {
    struct timespec ts;
    double secs = 0.0;
    nv_std_need_args("وقت.الآن", args, nargs, 0, line, col);
    if (clock_gettime(CLOCK_REALTIME, &ts) == 0) {
        secs = (double)ts.tv_sec + (double)ts.tv_nsec / 1000000000.0;
    } else {
        secs = (double)nv_std_time_now_epoch();
    }
    return nv_num(secs);
}

static NvValue nv_std_time_sleep(NvValue *args, int nargs, int line, int col) {
    double secs = 0.0;
    struct timespec ts;
    nv_std_need_args("وقت.نوم", args, nargs, 1, line, col);
    secs = nv_std_need_number("وقت.نوم", args[0], line, col);
    if (secs < 0.0) {
        nv_error(NV_ERR_RUNTIME, "نوم مدة سالبة غير مسموح", line, col);
    }
    if (!isfinite(secs)) {
        nv_error(NV_ERR_RUNTIME, "نوم مدة غير صالحة", line, col);
    }
    if (secs > 60.0) {
        nv_error(NV_ERR_RUNTIME, "نوم أكثر من 60 ثانية غير مسموح", line, col);
    }
    ts.tv_sec = (time_t)floor(secs);
    ts.tv_nsec = (long)((secs - floor(secs)) * 1000000000.0);
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
    return nv_nil();
}

static NvValue nv_std_time_date(NvValue *args, int nargs, int line, int col) {
    long long epoch = 0;
    NvStdTimeDt dt;
    if (nargs == 0) {
        epoch = nv_std_time_now_epoch();
    } else if (nargs == 1) {
        epoch = nv_std_time_need_epoch("وقت.التاريخ", args[0], line, col);
    } else {
        nv_error(NV_ERR_RUNTIME, "الدالة وقت.التاريخ تتوقع صفر معاملات أو معاملا واحدا (ثواني)", line, col);
    }
    dt = nv_std_time_from_epoch(epoch, 0, "وقت.التاريخ", line, col);
    return nv_std_time_to_dict(&dt);
}

static NvValue nv_std_time_date_utc(NvValue *args, int nargs, int line, int col) {
    long long epoch = 0;
    NvStdTimeDt dt;
    if (nargs == 0) {
        epoch = nv_std_time_now_epoch();
    } else if (nargs == 1) {
        epoch = nv_std_time_need_epoch("وقت.التاريخ_العالمي", args[0], line, col);
    } else {
        nv_error(NV_ERR_RUNTIME, "الدالة وقت.التاريخ_العالمي تتوقع صفر معاملات أو معاملا واحدا (ثواني)", line, col);
    }
    dt = nv_std_time_from_epoch(epoch, 1, "وقت.التاريخ_العالمي", line, col);
    return nv_std_time_to_dict(&dt);
}

static NvValue nv_std_time_from_date_impl(NvValue *args, int nargs, int utc, const char *display, int line, int col) {
    long long y = 0;
    long long mo = 0;
    long long d = 0;
    long long h = 0;
    long long mi = 0;
    long long s = 0;
    NvStdTimeDt dt;
    nv_std_need_args(display, args, nargs, 6, line, col);
    y = nv_std_need_integer(display, args[0], line, col);
    mo = nv_std_need_integer(display, args[1], line, col);
    d = nv_std_need_integer(display, args[2], line, col);
    h = nv_std_need_integer(display, args[3], line, col);
    mi = nv_std_need_integer(display, args[4], line, col);
    s = nv_std_need_integer(display, args[5], line, col);
    dt = nv_std_time_from_components(y, mo, d, h, mi, s, utc, display, line, col);
    return nv_std_time_to_dict(&dt);
}

static NvValue nv_std_time_from_date(NvValue *args, int nargs, int line, int col) {
    return nv_std_time_from_date_impl(args, nargs, 0, "وقت.من_التاريخ", line, col);
}

static NvValue nv_std_time_from_date_utc(NvValue *args, int nargs, int line, int col) {
    return nv_std_time_from_date_impl(args, nargs, 1, "وقت.من_التاريخ_العالمي", line, col);
}

static NvValue nv_std_time_text(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    char buf[64];
    nv_std_need_args("وقت.نص", args, nargs, 1, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.نص", line, col);
    nv_std_time_format_datetime(&dt, buf, sizeof(buf));
    return nv_str_cstr(buf);
}

static NvValue nv_std_time_text_date(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    char buf[64];
    nv_std_need_args("وقت.نص_التاريخ", args, nargs, 1, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.نص_التاريخ", line, col);
    nv_std_time_format_date(&dt, buf, sizeof(buf));
    return nv_str_cstr(buf);
}

static NvValue nv_std_time_text_time(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    char buf[64];
    nv_std_need_args("وقت.نص_الوقت", args, nargs, 1, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.نص_الوقت", line, col);
    nv_std_time_format_time(&dt, buf, sizeof(buf));
    return nv_str_cstr(buf);
}

static NvValue nv_std_time_weekday_name(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    nv_std_need_args("وقت.اسم_اليوم", args, nargs, 1, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.اسم_اليوم", line, col);
    return nv_str_cstr(nv_std_time_weekday_name_str(dt.weekday));
}

static NvValue nv_std_time_month_name(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    nv_std_need_args("وقت.اسم_الشهر", args, nargs, 1, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.اسم_الشهر", line, col);
    return nv_str_cstr(nv_std_time_month_name_str(dt.month));
}

static NvValue nv_std_time_diff(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt a;
    NvStdTimeDt b;
    nv_std_need_args("وقت.فرق", args, nargs, 2, line, col);
    a = nv_std_time_from_dict(args[0], "وقت.فرق", line, col);
    b = nv_std_time_from_dict(args[1], "وقت.فرق", line, col);
    return nv_num((double)nv_std_time_diff_secs(&a, &b, "وقت.فرق", line, col));
}

static NvValue nv_std_time_add(NvValue *args, int nargs, int line, int col) {
    NvStdTimeDt dt;
    NvStdTimeDt out;
    long long delta = 0;
    nv_std_need_args("وقت.أضف", args, nargs, 2, line, col);
    dt = nv_std_time_from_dict(args[0], "وقت.أضف", line, col);
    delta = nv_std_need_integer("وقت.أضف", args[1], line, col);
    out = nv_std_time_shift(&dt, delta, "وقت.أضف", line, col);
    return nv_std_time_to_dict(&out);
}

void nv_std_time_fill(NvDict *d) {
    nv_std_add_fn(d, "الآن", nv_std_time_now);
    nv_std_add_fn(d, "نوم", nv_std_time_sleep);
    nv_std_add_fn(d, "التاريخ", nv_std_time_date);
    nv_std_add_fn(d, "التاريخ_العالمي", nv_std_time_date_utc);
    nv_std_add_fn(d, "من_التاريخ", nv_std_time_from_date);
    nv_std_add_fn(d, "من_التاريخ_العالمي", nv_std_time_from_date_utc);
    nv_std_add_fn(d, "نص", nv_std_time_text);
    nv_std_add_fn(d, "نص_التاريخ", nv_std_time_text_date);
    nv_std_add_fn(d, "نص_الوقت", nv_std_time_text_time);
    nv_std_add_fn(d, "اسم_اليوم", nv_std_time_weekday_name);
    nv_std_add_fn(d, "اسم_الشهر", nv_std_time_month_name);
    nv_std_add_fn(d, "فرق", nv_std_time_diff);
    nv_std_add_fn(d, "أضف", nv_std_time_add);
}
/* جدول النداءات — مولّد آليا من system.rs (لا يُكتب يدويا). */
typedef struct { unsigned int num; const char *name; } NvStdSysEntry;

#if (defined(__x86_64__) || defined(_M_X64)) && (defined(__linux__) || defined(__ANDROID__))
static const NvStdSysEntry nv_std_sys_table[] = {
    { 0u, "read" },
    { 1u, "write" },
    { 2u, "open" },
    { 3u, "close" },
    { 4u, "stat" },
    { 5u, "fstat" },
    { 6u, "lstat" },
    { 7u, "poll" },
    { 8u, "lseek" },
    { 9u, "mmap" },
    { 10u, "mprotect" },
    { 11u, "munmap" },
    { 12u, "brk" },
    { 13u, "rt_sigaction" },
    { 14u, "rt_sigprocmask" },
    { 15u, "rt_sigreturn" },
    { 16u, "ioctl" },
    { 17u, "pread64" },
    { 18u, "pwrite64" },
    { 19u, "readv" },
    { 20u, "writev" },
    { 21u, "access" },
    { 22u, "pipe" },
    { 23u, "select" },
    { 24u, "sched_yield" },
    { 25u, "mremap" },
    { 26u, "msync" },
    { 27u, "mincore" },
    { 28u, "madvise" },
    { 29u, "shmget" },
    { 30u, "shmat" },
    { 31u, "shmctl" },
    { 32u, "dup" },
    { 33u, "dup2" },
    { 34u, "pause" },
    { 35u, "nanosleep" },
    { 36u, "getitimer" },
    { 37u, "alarm" },
    { 38u, "setitimer" },
    { 39u, "getpid" },
    { 40u, "sendfile" },
    { 41u, "socket" },
    { 42u, "connect" },
    { 43u, "accept" },
    { 44u, "sendto" },
    { 45u, "recvfrom" },
    { 46u, "sendmsg" },
    { 47u, "recvmsg" },
    { 48u, "shutdown" },
    { 49u, "bind" },
    { 50u, "listen" },
    { 51u, "getsockname" },
    { 52u, "getpeername" },
    { 53u, "socketpair" },
    { 54u, "setsockopt" },
    { 55u, "getsockopt" },
    { 56u, "clone" },
    { 57u, "fork" },
    { 58u, "vfork" },
    { 59u, "execve" },
    { 60u, "exit" },
    { 61u, "wait4" },
    { 62u, "kill" },
    { 63u, "uname" },
    { 64u, "semget" },
    { 65u, "semop" },
    { 66u, "semctl" },
    { 67u, "shmdt" },
    { 68u, "msgget" },
    { 69u, "msgsnd" },
    { 70u, "msgrcv" },
    { 71u, "msgctl" },
    { 72u, "fcntl" },
    { 73u, "flock" },
    { 74u, "fsync" },
    { 75u, "fdatasync" },
    { 76u, "truncate" },
    { 77u, "ftruncate" },
    { 78u, "getdents" },
    { 79u, "getcwd" },
    { 80u, "chdir" },
    { 81u, "fchdir" },
    { 82u, "rename" },
    { 83u, "mkdir" },
    { 84u, "rmdir" },
    { 85u, "creat" },
    { 86u, "link" },
    { 87u, "unlink" },
    { 88u, "symlink" },
    { 89u, "readlink" },
    { 90u, "chmod" },
    { 91u, "fchmod" },
    { 92u, "chown" },
    { 93u, "fchown" },
    { 94u, "lchown" },
    { 95u, "umask" },
    { 96u, "gettimeofday" },
    { 97u, "getrlimit" },
    { 98u, "getrusage" },
    { 99u, "sysinfo" },
    { 100u, "times" },
    { 101u, "ptrace" },
    { 102u, "getuid" },
    { 103u, "syslog" },
    { 104u, "getgid" },
    { 105u, "setuid" },
    { 106u, "setgid" },
    { 107u, "geteuid" },
    { 108u, "getegid" },
    { 109u, "setpgid" },
    { 110u, "getppid" },
    { 111u, "getpgrp" },
    { 112u, "setsid" },
    { 113u, "setreuid" },
    { 114u, "setregid" },
    { 115u, "getgroups" },
    { 116u, "setgroups" },
    { 117u, "setresuid" },
    { 118u, "getresuid" },
    { 119u, "setresgid" },
    { 120u, "getresgid" },
    { 121u, "getpgid" },
    { 122u, "setfsuid" },
    { 123u, "setfsgid" },
    { 124u, "getsid" },
    { 125u, "capget" },
    { 126u, "capset" },
    { 127u, "rt_sigpending" },
    { 128u, "rt_sigtimedwait" },
    { 129u, "rt_sigqueueinfo" },
    { 130u, "rt_sigsuspend" },
    { 131u, "sigaltstack" },
    { 132u, "utime" },
    { 133u, "mknod" },
    { 134u, "uselib" },
    { 135u, "personality" },
    { 136u, "ustat" },
    { 137u, "statfs" },
    { 138u, "fstatfs" },
    { 139u, "sysfs" },
    { 140u, "getpriority" },
    { 141u, "setpriority" },
    { 142u, "sched_setparam" },
    { 143u, "sched_getparam" },
    { 144u, "sched_setscheduler" },
    { 145u, "sched_getscheduler" },
    { 146u, "sched_get_priority_max" },
    { 147u, "sched_get_priority_min" },
    { 148u, "sched_rr_get_interval" },
    { 149u, "mlock" },
    { 150u, "munlock" },
    { 151u, "mlockall" },
    { 152u, "munlockall" },
    { 153u, "vhangup" },
    { 154u, "modify_ldt" },
    { 155u, "pivot_root" },
    { 156u, "_sysctl" },
    { 157u, "prctl" },
    { 158u, "arch_prctl" },
    { 159u, "adjtimex" },
    { 160u, "setrlimit" },
    { 161u, "chroot" },
    { 162u, "sync" },
    { 163u, "acct" },
    { 164u, "settimeofday" },
    { 165u, "mount" },
    { 166u, "umount2" },
    { 167u, "swapon" },
    { 168u, "swapoff" },
    { 169u, "reboot" },
    { 170u, "sethostname" },
    { 171u, "setdomainname" },
    { 172u, "iopl" },
    { 173u, "ioperm" },
    { 174u, "create_module" },
    { 175u, "init_module" },
    { 176u, "delete_module" },
    { 177u, "get_kernel_syms" },
    { 178u, "query_module" },
    { 179u, "quotactl" },
    { 180u, "nfsservctl" },
    { 181u, "getpmsg" },
    { 182u, "putpmsg" },
    { 183u, "afs_syscall" },
    { 184u, "tuxcall" },
    { 185u, "security" },
    { 186u, "gettid" },
    { 187u, "readahead" },
    { 188u, "setxattr" },
    { 189u, "lsetxattr" },
    { 190u, "fsetxattr" },
    { 191u, "getxattr" },
    { 192u, "lgetxattr" },
    { 193u, "fgetxattr" },
    { 194u, "listxattr" },
    { 195u, "llistxattr" },
    { 196u, "flistxattr" },
    { 197u, "removexattr" },
    { 198u, "lremovexattr" },
    { 199u, "fremovexattr" },
    { 200u, "tkill" },
    { 201u, "time" },
    { 202u, "futex" },
    { 203u, "sched_setaffinity" },
    { 204u, "sched_getaffinity" },
    { 205u, "set_thread_area" },
    { 206u, "io_setup" },
    { 207u, "io_destroy" },
    { 208u, "io_getevents" },
    { 209u, "io_submit" },
    { 210u, "io_cancel" },
    { 211u, "get_thread_area" },
    { 212u, "lookup_dcookie" },
    { 213u, "epoll_create" },
    { 214u, "epoll_ctl_old" },
    { 215u, "epoll_wait_old" },
    { 216u, "remap_file_pages" },
    { 217u, "getdents64" },
    { 218u, "set_tid_address" },
    { 219u, "restart_syscall" },
    { 220u, "semtimedop" },
    { 221u, "fadvise64" },
    { 222u, "timer_create" },
    { 223u, "timer_settime" },
    { 224u, "timer_gettime" },
    { 225u, "timer_getoverrun" },
    { 226u, "timer_delete" },
    { 227u, "clock_settime" },
    { 228u, "clock_gettime" },
    { 229u, "clock_getres" },
    { 230u, "clock_nanosleep" },
    { 231u, "exit_group" },
    { 232u, "epoll_wait" },
    { 233u, "epoll_ctl" },
    { 234u, "tgkill" },
    { 235u, "utimes" },
    { 236u, "vserver" },
    { 237u, "mbind" },
    { 238u, "set_mempolicy" },
    { 239u, "get_mempolicy" },
    { 240u, "mq_open" },
    { 241u, "mq_unlink" },
    { 242u, "mq_timedsend" },
    { 243u, "mq_timedreceive" },
    { 244u, "mq_notify" },
    { 245u, "mq_getsetattr" },
    { 246u, "kexec_load" },
    { 247u, "waitid" },
    { 248u, "add_key" },
    { 249u, "request_key" },
    { 250u, "keyctl" },
    { 251u, "ioprio_set" },
    { 252u, "ioprio_get" },
    { 253u, "inotify_init" },
    { 254u, "inotify_add_watch" },
    { 255u, "inotify_rm_watch" },
    { 256u, "migrate_pages" },
    { 257u, "openat" },
    { 258u, "mkdirat" },
    { 259u, "mknodat" },
    { 260u, "fchownat" },
    { 261u, "futimesat" },
    { 262u, "newfstatat" },
    { 263u, "unlinkat" },
    { 264u, "renameat" },
    { 265u, "linkat" },
    { 266u, "symlinkat" },
    { 267u, "readlinkat" },
    { 268u, "fchmodat" },
    { 269u, "faccessat" },
    { 270u, "pselect6" },
    { 271u, "ppoll" },
    { 272u, "unshare" },
    { 273u, "set_robust_list" },
    { 274u, "get_robust_list" },
    { 275u, "splice" },
    { 276u, "tee" },
    { 277u, "sync_file_range" },
    { 278u, "vmsplice" },
    { 279u, "move_pages" },
    { 280u, "utimensat" },
    { 281u, "epoll_pwait" },
    { 282u, "signalfd" },
    { 283u, "timerfd_create" },
    { 284u, "eventfd" },
    { 285u, "fallocate" },
    { 286u, "timerfd_settime" },
    { 287u, "timerfd_gettime" },
    { 288u, "accept4" },
    { 289u, "signalfd4" },
    { 290u, "eventfd2" },
    { 291u, "epoll_create1" },
    { 292u, "dup3" },
    { 293u, "pipe2" },
    { 294u, "inotify_init1" },
    { 295u, "preadv" },
    { 296u, "pwritev" },
    { 297u, "rt_tgsigqueueinfo" },
    { 298u, "perf_event_open" },
    { 299u, "recvmmsg" },
    { 300u, "fanotify_init" },
    { 301u, "fanotify_mark" },
    { 302u, "prlimit64" },
    { 303u, "name_to_handle_at" },
    { 304u, "open_by_handle_at" },
    { 305u, "clock_adjtime" },
    { 306u, "syncfs" },
    { 307u, "sendmmsg" },
    { 308u, "setns" },
    { 309u, "getcpu" },
    { 310u, "process_vm_readv" },
    { 311u, "process_vm_writev" },
    { 312u, "kcmp" },
    { 313u, "finit_module" },
    { 314u, "sched_setattr" },
    { 315u, "sched_getattr" },
    { 316u, "renameat2" },
    { 317u, "seccomp" },
    { 318u, "getrandom" },
    { 319u, "memfd_create" },
    { 320u, "kexec_file_load" },
    { 321u, "bpf" },
    { 322u, "execveat" },
    { 323u, "userfaultfd" },
    { 324u, "membarrier" },
    { 325u, "mlock2" },
    { 326u, "copy_file_range" },
    { 327u, "preadv2" },
    { 328u, "pwritev2" },
    { 329u, "pkey_mprotect" },
    { 330u, "pkey_alloc" },
    { 331u, "pkey_free" },
    { 332u, "statx" },
    { 333u, "io_pgetevents" },
    { 334u, "rseq" },
    { 335u, "uretprobe" },
    { 336u, "uprobe" },
    { 424u, "pidfd_send_signal" },
    { 425u, "io_uring_setup" },
    { 426u, "io_uring_enter" },
    { 427u, "io_uring_register" },
    { 428u, "open_tree" },
    { 429u, "move_mount" },
    { 430u, "fsopen" },
    { 431u, "fsconfig" },
    { 432u, "fsmount" },
    { 433u, "fspick" },
    { 434u, "pidfd_open" },
    { 435u, "clone3" },
    { 436u, "close_range" },
    { 437u, "openat2" },
    { 438u, "pidfd_getfd" },
    { 439u, "faccessat2" },
    { 440u, "process_madvise" },
    { 441u, "epoll_pwait2" },
    { 442u, "mount_setattr" },
    { 443u, "quotactl_fd" },
    { 444u, "landlock_create_ruleset" },
    { 445u, "landlock_add_rule" },
    { 446u, "landlock_restrict_self" },
    { 447u, "memfd_secret" },
    { 448u, "process_mrelease" },
    { 449u, "futex_waitv" },
    { 450u, "set_mempolicy_home_node" },
    { 451u, "cachestat" },
    { 452u, "fchmodat2" },
    { 453u, "map_shadow_stack" },
    { 454u, "futex_wake" },
    { 455u, "futex_wait" },
    { 456u, "futex_requeue" },
    { 457u, "statmount" },
    { 458u, "listmount" },
    { 459u, "lsm_get_self_attr" },
    { 460u, "lsm_set_self_attr" },
    { 461u, "lsm_list_modules" },
    { 462u, "mseal" },
    { 463u, "setxattrat" },
    { 464u, "getxattrat" },
    { 465u, "listxattrat" },
    { 466u, "removexattrat" },
    { 467u, "open_tree_attr" },
    { 468u, "file_getattr" },
    { 469u, "file_setattr" },
    { 470u, "listns" },
    { 471u, "rseq_slice_yield" },
    { 472u, "fchroot" },
};
#elif (defined(__aarch64__) || defined(_M_ARM64)) && (defined(__linux__) || defined(__ANDROID__))
static const NvStdSysEntry nv_std_sys_table[] = {
    { 0u, "io_setup" },
    { 1u, "io_destroy" },
    { 2u, "io_submit" },
    { 3u, "io_cancel" },
    { 4u, "io_getevents" },
    { 5u, "setxattr" },
    { 6u, "lsetxattr" },
    { 7u, "fsetxattr" },
    { 8u, "getxattr" },
    { 9u, "lgetxattr" },
    { 10u, "fgetxattr" },
    { 11u, "listxattr" },
    { 12u, "llistxattr" },
    { 13u, "flistxattr" },
    { 14u, "removexattr" },
    { 15u, "lremovexattr" },
    { 16u, "fremovexattr" },
    { 17u, "getcwd" },
    { 18u, "lookup_dcookie" },
    { 19u, "eventfd2" },
    { 20u, "epoll_create1" },
    { 21u, "epoll_ctl" },
    { 22u, "epoll_pwait" },
    { 23u, "dup" },
    { 24u, "dup3" },
    { 25u, "fcntl" },
    { 26u, "inotify_init1" },
    { 27u, "inotify_add_watch" },
    { 28u, "inotify_rm_watch" },
    { 29u, "ioctl" },
    { 30u, "ioprio_set" },
    { 31u, "ioprio_get" },
    { 32u, "flock" },
    { 33u, "mknodat" },
    { 34u, "mkdirat" },
    { 35u, "unlinkat" },
    { 36u, "symlinkat" },
    { 37u, "linkat" },
    { 39u, "umount2" },
    { 40u, "mount" },
    { 41u, "pivot_root" },
    { 42u, "nfsservctl" },
    { 43u, "statfs" },
    { 44u, "fstatfs" },
    { 45u, "truncate" },
    { 46u, "ftruncate" },
    { 47u, "fallocate" },
    { 48u, "faccessat" },
    { 49u, "chdir" },
    { 50u, "fchdir" },
    { 51u, "chroot" },
    { 52u, "fchmod" },
    { 53u, "fchmodat" },
    { 54u, "fchownat" },
    { 55u, "fchown" },
    { 56u, "openat" },
    { 57u, "close" },
    { 58u, "vhangup" },
    { 59u, "pipe2" },
    { 60u, "quotactl" },
    { 61u, "getdents64" },
    { 62u, "lseek" },
    { 63u, "read" },
    { 64u, "write" },
    { 65u, "readv" },
    { 66u, "writev" },
    { 67u, "pread64" },
    { 68u, "pwrite64" },
    { 69u, "preadv" },
    { 70u, "pwritev" },
    { 71u, "sendfile" },
    { 72u, "pselect6" },
    { 73u, "ppoll" },
    { 74u, "signalfd4" },
    { 75u, "vmsplice" },
    { 76u, "splice" },
    { 77u, "tee" },
    { 78u, "readlinkat" },
    { 79u, "newfstatat" },
    { 80u, "fstat" },
    { 81u, "sync" },
    { 82u, "fsync" },
    { 83u, "fdatasync" },
    { 84u, "sync_file_range" },
    { 85u, "timerfd_create" },
    { 86u, "timerfd_settime" },
    { 87u, "timerfd_gettime" },
    { 88u, "utimensat" },
    { 89u, "acct" },
    { 90u, "capget" },
    { 91u, "capset" },
    { 92u, "personality" },
    { 93u, "exit" },
    { 94u, "exit_group" },
    { 95u, "waitid" },
    { 96u, "set_tid_address" },
    { 97u, "unshare" },
    { 98u, "futex" },
    { 99u, "set_robust_list" },
    { 100u, "get_robust_list" },
    { 101u, "nanosleep" },
    { 102u, "getitimer" },
    { 103u, "setitimer" },
    { 104u, "kexec_load" },
    { 105u, "init_module" },
    { 106u, "delete_module" },
    { 107u, "timer_create" },
    { 108u, "timer_gettime" },
    { 109u, "timer_getoverrun" },
    { 110u, "timer_settime" },
    { 111u, "timer_delete" },
    { 112u, "clock_settime" },
    { 113u, "clock_gettime" },
    { 114u, "clock_getres" },
    { 115u, "clock_nanosleep" },
    { 116u, "syslog" },
    { 117u, "ptrace" },
    { 118u, "sched_setparam" },
    { 119u, "sched_setscheduler" },
    { 120u, "sched_getscheduler" },
    { 121u, "sched_getparam" },
    { 122u, "sched_setaffinity" },
    { 123u, "sched_getaffinity" },
    { 124u, "sched_yield" },
    { 125u, "sched_get_priority_max" },
    { 126u, "sched_get_priority_min" },
    { 127u, "sched_rr_get_interval" },
    { 128u, "restart_syscall" },
    { 129u, "kill" },
    { 130u, "tkill" },
    { 131u, "tgkill" },
    { 132u, "sigaltstack" },
    { 133u, "rt_sigsuspend" },
    { 134u, "rt_sigaction" },
    { 135u, "rt_sigprocmask" },
    { 136u, "rt_sigpending" },
    { 137u, "rt_sigtimedwait" },
    { 138u, "rt_sigqueueinfo" },
    { 139u, "rt_sigreturn" },
    { 140u, "setpriority" },
    { 141u, "getpriority" },
    { 142u, "reboot" },
    { 143u, "setregid" },
    { 144u, "setgid" },
    { 145u, "setreuid" },
    { 146u, "setuid" },
    { 147u, "setresuid" },
    { 148u, "getresuid" },
    { 149u, "setresgid" },
    { 150u, "getresgid" },
    { 151u, "setfsuid" },
    { 152u, "setfsgid" },
    { 153u, "times" },
    { 154u, "setpgid" },
    { 155u, "getpgid" },
    { 156u, "getsid" },
    { 157u, "setsid" },
    { 158u, "getgroups" },
    { 159u, "setgroups" },
    { 160u, "uname" },
    { 161u, "sethostname" },
    { 162u, "setdomainname" },
    { 165u, "getrusage" },
    { 166u, "umask" },
    { 167u, "prctl" },
    { 168u, "getcpu" },
    { 169u, "gettimeofday" },
    { 170u, "settimeofday" },
    { 171u, "adjtimex" },
    { 172u, "getpid" },
    { 173u, "getppid" },
    { 174u, "getuid" },
    { 175u, "geteuid" },
    { 176u, "getgid" },
    { 177u, "getegid" },
    { 178u, "gettid" },
    { 179u, "sysinfo" },
    { 180u, "mq_open" },
    { 181u, "mq_unlink" },
    { 182u, "mq_timedsend" },
    { 183u, "mq_timedreceive" },
    { 184u, "mq_notify" },
    { 185u, "mq_getsetattr" },
    { 186u, "msgget" },
    { 187u, "msgctl" },
    { 188u, "msgrcv" },
    { 189u, "msgsnd" },
    { 190u, "semget" },
    { 191u, "semctl" },
    { 192u, "semtimedop" },
    { 193u, "semop" },
    { 194u, "shmget" },
    { 195u, "shmctl" },
    { 196u, "shmat" },
    { 197u, "shmdt" },
    { 198u, "socket" },
    { 199u, "socketpair" },
    { 200u, "bind" },
    { 201u, "listen" },
    { 202u, "accept" },
    { 203u, "connect" },
    { 204u, "getsockname" },
    { 205u, "getpeername" },
    { 206u, "sendto" },
    { 207u, "recvfrom" },
    { 208u, "setsockopt" },
    { 209u, "getsockopt" },
    { 210u, "shutdown" },
    { 211u, "sendmsg" },
    { 212u, "recvmsg" },
    { 213u, "readahead" },
    { 214u, "brk" },
    { 215u, "munmap" },
    { 216u, "mremap" },
    { 217u, "add_key" },
    { 218u, "request_key" },
    { 219u, "keyctl" },
    { 220u, "clone" },
    { 221u, "execve" },
    { 222u, "mmap" },
    { 223u, "fadvise64" },
    { 224u, "swapon" },
    { 225u, "swapoff" },
    { 226u, "mprotect" },
    { 227u, "msync" },
    { 228u, "mlock" },
    { 229u, "munlock" },
    { 230u, "mlockall" },
    { 231u, "munlockall" },
    { 232u, "mincore" },
    { 233u, "madvise" },
    { 234u, "remap_file_pages" },
    { 235u, "mbind" },
    { 236u, "get_mempolicy" },
    { 237u, "set_mempolicy" },
    { 238u, "migrate_pages" },
    { 239u, "move_pages" },
    { 240u, "rt_tgsigqueueinfo" },
    { 241u, "perf_event_open" },
    { 242u, "accept4" },
    { 243u, "recvmmsg" },
    { 260u, "wait4" },
    { 261u, "prlimit64" },
    { 262u, "fanotify_init" },
    { 263u, "fanotify_mark" },
    { 264u, "name_to_handle_at" },
    { 265u, "open_by_handle_at" },
    { 266u, "clock_adjtime" },
    { 267u, "syncfs" },
    { 268u, "setns" },
    { 269u, "sendmmsg" },
    { 270u, "process_vm_readv" },
    { 271u, "process_vm_writev" },
    { 272u, "kcmp" },
    { 273u, "finit_module" },
    { 274u, "sched_setattr" },
    { 275u, "sched_getattr" },
    { 276u, "renameat2" },
    { 277u, "seccomp" },
    { 278u, "getrandom" },
    { 279u, "memfd_create" },
    { 280u, "bpf" },
    { 281u, "execveat" },
    { 282u, "userfaultfd" },
    { 283u, "membarrier" },
    { 284u, "mlock2" },
    { 285u, "copy_file_range" },
    { 286u, "preadv2" },
    { 287u, "pwritev2" },
    { 288u, "pkey_mprotect" },
    { 289u, "pkey_alloc" },
    { 290u, "pkey_free" },
    { 291u, "statx" },
    { 292u, "io_pgetevents" },
    { 293u, "rseq" },
    { 294u, "kexec_file_load" },
    { 424u, "pidfd_send_signal" },
    { 425u, "io_uring_setup" },
    { 426u, "io_uring_enter" },
    { 427u, "io_uring_register" },
    { 428u, "open_tree" },
    { 429u, "move_mount" },
    { 430u, "fsopen" },
    { 431u, "fsconfig" },
    { 432u, "fsmount" },
    { 433u, "fspick" },
    { 434u, "pidfd_open" },
    { 435u, "clone3" },
    { 436u, "close_range" },
    { 437u, "openat2" },
    { 438u, "pidfd_getfd" },
    { 439u, "faccessat2" },
    { 440u, "process_madvise" },
    { 441u, "epoll_pwait2" },
    { 442u, "mount_setattr" },
    { 443u, "quotactl_fd" },
    { 444u, "landlock_create_ruleset" },
    { 445u, "landlock_add_rule" },
    { 446u, "landlock_restrict_self" },
    { 448u, "process_mrelease" },
    { 449u, "futex_waitv" },
    { 450u, "set_mempolicy_home_node" },
    { 451u, "cachestat" },
    { 452u, "fchmodat2" },
    { 453u, "map_shadow_stack" },
    { 454u, "futex_wake" },
    { 455u, "futex_wait" },
    { 456u, "futex_requeue" },
    { 457u, "statmount" },
    { 458u, "listmount" },
    { 459u, "lsm_get_self_attr" },
    { 460u, "lsm_set_self_attr" },
    { 461u, "lsm_list_modules" },
    { 462u, "mseal" },
    { 463u, "setxattrat" },
    { 464u, "getxattrat" },
    { 465u, "listxattrat" },
    { 466u, "removexattrat" },
    { 467u, "open_tree_attr" },
    { 468u, "file_getattr" },
    { 469u, "file_setattr" },
    { 470u, "listns" },
    { 471u, "rseq_slice_yield" },
    { 472u, "fchroot" },
};
#endif

#if (defined(__x86_64__) || defined(__aarch64__) || defined(_M_X64) || defined(_M_ARM64)) && (defined(__linux__) || defined(__ANDROID__))
static const size_t nv_std_sys_table_len = sizeof(nv_std_sys_table) / sizeof(nv_std_sys_table[0]);
#endif

/* ---------- نظام: قراءات /proc ---------- */

#if defined(__linux__) || defined(__ANDROID__)
/* قيمة واحدة من /proc/sys/kernel/<name> بلا فراغات حولها.
   تطلق «تعذّر قراءة معلومات النظام» عند فشل القراءة.
   ملاحظة: ملفات /proc حجمها 0 عند ftell، فلا يصح
   nv_read_file_bytes هنا — القراءة على دفعات حتى EOF. */
static char *nv_std_sys_proc_kernel(const char *name, int line, int col) {
    char path[128];
    FILE *fp = NULL;
    char *text = NULL;
    size_t len = 0;
    size_t cap = 0;
    size_t start = 0;
    size_t end = 0;
    snprintf(path, sizeof(path), "/proc/sys/kernel/%s", name);
    fp = fopen(path, "rb");
    if (!fp) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات النظام", line, col);
    }
    for (;;) {
        char chunk[256];
        size_t n = fread(chunk, 1, sizeof(chunk), fp);
        if (n == 0) {
            break;
        }
        if (len + n + 1 > cap) {
            size_t ncap = cap == 0 ? 512 : cap * 2;
            char *grown = NULL;
            while (ncap < len + n + 1) {
                ncap *= 2;
            }
            grown = (char *)realloc(text, ncap);
            if (!grown) {
                free(text);
                fclose(fp);
                nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات النظام", line, col);
            }
            text = grown;
            cap = ncap;
        }
        memcpy(text + len, chunk, n);
        len += n;
    }
    fclose(fp);
    if (!text) {
        text = (char *)malloc(1);
        if (!text) {
            nv_error(NV_ERR_RUNTIME, "تعذّر قراءة معلومات النظام", line, col);
        }
    }
    text[len] = '\0';
    while (start < len && isspace((unsigned char)text[start])) {
        start++;
    }
    end = len;
    while (end > start && isspace((unsigned char)text[end - 1])) {
        end--;
    }
    text[end] = '\0';
    if (start > 0) {
        memmove(text, text + start, end - start + 1);
    }
    return text;
}

#endif // defined(__linux__) || defined(__ANDROID__)

/* ---------- نظام: الدوال الأصلية ---------- */

static NvValue nv_std_sys_name(NvValue *args, int nargs, int line, int col) {
    long long num = 0;
    nv_std_need_args("نظام.اسم_النداء", args, nargs, 1, line, col);
    num = nv_std_need_integer("نظام.اسم_النداء", args[0], line, col);
#if defined(__linux__) || defined(__ANDROID__)
#if defined(__x86_64__) || defined(__aarch64__) || defined(_M_X64) || defined(_M_ARM64)
    if (num < 0 || num > 4294967295LL) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "نداء غير معروف: %lld", num);
    }
    {
        size_t lo = 0;
        size_t hi = nv_std_sys_table_len;
        while (lo < hi) {
            size_t mid = lo + (hi - lo) / 2;
            if (nv_std_sys_table[mid].num == (unsigned int)num) {
                return nv_str_cstr(nv_std_sys_table[mid].name);
            } else if (nv_std_sys_table[mid].num < (unsigned int)num) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "نداء غير معروف: %lld", num);
    return nv_nil();
#else
    (void)num;
    nv_error(NV_ERR_RUNTIME, "نظام.اسم_النداء غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
#else
    (void)num;
    nv_error(NV_ERR_RUNTIME, "نظام.اسم_النداء غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_sys_count(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نظام.عدد_النداءات", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
#if defined(__x86_64__) || defined(__aarch64__) || defined(_M_X64) || defined(_M_ARM64)
    return nv_num((double)nv_std_sys_table_len);
#else
    nv_error(NV_ERR_RUNTIME, "نظام.عدد_النداءات غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
#else
    nv_error(NV_ERR_RUNTIME, "نظام.عدد_النداءات غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_sys_kernel(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نظام.نسخة_النواة", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    char *v = NULL;
    NvValue out;
    v = nv_std_sys_proc_kernel("osrelease", line, col);
    out = nv_str_cstr(v);
    free(v);
    return out;
#else
    nv_error(NV_ERR_RUNTIME, "نظام.نسخة_النواة غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_sys_os(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نظام.اسم_النظام", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    char *v = NULL;
    NvValue out;
    v = nv_std_sys_proc_kernel("ostype", line, col);
    out = nv_str_cstr(v);
    free(v);
    return out;
#else
    nv_error(NV_ERR_RUNTIME, "نظام.اسم_النظام غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_sys_uid(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نظام.معرف_المستخدم", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    return nv_num((double)getuid());
#else
    nv_error(NV_ERR_RUNTIME, "نظام.معرف_المستخدم غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_sys_gid(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نظام.معرف_المجموعة", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    return nv_num((double)getgid());
#else
    nv_error(NV_ERR_RUNTIME, "نظام.معرف_المجموعة غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

void nv_std_system_fill(NvDict *d) {
    nv_std_add_fn(d, "اسم_النداء", nv_std_sys_name);
    nv_std_add_fn(d, "عدد_النداءات", nv_std_sys_count);
    nv_std_add_fn(d, "نسخة_النواة", nv_std_sys_kernel);
    nv_std_add_fn(d, "اسم_النظام", nv_std_sys_os);
    nv_std_add_fn(d, "معرف_المستخدم", nv_std_sys_uid);
    nv_std_add_fn(d, "معرف_المجموعة", nv_std_sys_gid);
}
// قطعة قسم عمليات وذاكرة وملفات — خلفية C (مهمة 29).
// قطعة لا وحدة ترجمة مستقلة: بلا تضمين وبلا دالة رئيسية.
// تُدمج بعد المقدمة ووقت التشغيل فترى مساعدات المقدمة
// (فحص الوسائط والإضافة) وبنى حمولات المقابض والمنظف
// الموحد. كل الرسائل الظاهرة عربية حرفيا كما في المحرك.
// التصميم: المقابض هي المعرفات — انشي واحجز وافتح تعيد
// مقبضا (نوع 2 عملية، نوع 5 كتلة، نوع 1 ملف) يحمل رقما
// متسلسلا يبدأ من 1؛ والانتظار والتحرير يستهلكان المقبض
// (انتظار أو تحرير ثان يرفض برسالة «غير معروف» كما في
// المحرك). مسار العرض للملفات يُحفظ في جدول جانبي لأن
// حمولة الملف تحمل المؤشر وحده.

#define NV_STD_PROC_MAX_LIVE 256
#define NV_STD_MEM_MAX_SINGLE (64ull * 1024 * 1024)
#define NV_STD_MEM_MAX_TOTAL (256ull * 1024 * 1024)
#define NV_STD_FILE_MAX_READ (5u * 1024 * 1024)

// ---------- عمليات ----------

// كل تنفيذ هذا القسم POSIX خالص (fork/waitpid/
// pipe2) — يُستبعد كاملا على ويندوز (فرع #else
// أدناه: رفض عربي)؛ والحالية والتعبئة بعده باقيتان.
#ifndef _WIN32

// pipe2 غير موجود على ماك أصلا: غلاف بأنبوب +
// fcntl على Apple فقط؛ وفي غيرها تمرير إلى pipe2.
static int nv_std_pipe2(int pipefd[2], int flags) {
#ifdef __APPLE__
    if (pipe(pipefd) != 0) return -1;
    if (flags & O_CLOEXEC) {
        if (fcntl(pipefd[0], F_SETFD, FD_CLOEXEC) != 0) return -1;
        if (fcntl(pipefd[1], F_SETFD, FD_CLOEXEC) != 0) return -1;
    }
    return 0;
#else
    return pipe2(pipefd, flags);
#endif
}

static int nv_std_proc_next_id = 1;
static int nv_std_proc_live = 0;

static NvHandle *nv_std_proc_need_handle(const char *display, NvValue v, int kind, const char *kind_word, int line, int col) {
    if (v.tag != NV_HANDLE || !v.u.handle || v.u.handle->kind != kind) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع %s لكن أعطيت %s", display, kind_word, nv_type_name(v));
    }
    return v.u.handle;
}

static int nv_std_proc_is_path_token(const char *token) {
    return strchr(token, '/') != NULL || strchr(token, '\\') != NULL;
}

static void nv_std_proc_spawn_failed(int e, const char *display, int line, int col) __attribute__((noreturn));

static void nv_std_proc_spawn_failed(int e, const char *display, int line, int col) {
    if (e == ENOENT) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "البرنامج غير موجود: %s", display);
    }
    if (e == EACCES) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "لا يمكن تشغيل البرنامج (إذن مرفوض): %s", display);
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر تشغيل البرنامج: %s", display);
}

static void nv_std_proc_check_executable(const char *path, const char *display, int line, int col) {
    struct stat st;
    if (!path) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "البرنامج غير موجود: %s", display);
    }
    if (stat(path, &st) != 0) {
        int e = errno;
        if (e == ENOENT) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "البرنامج غير موجود: %s", display);
        }
        nv_std_proc_spawn_failed(e, display, line, col);
    }
    if ((st.st_mode & 0111) == 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الملف ليس برنامجا قابلا للتشغيل: %s", display);
    }
}

static char *nv_std_proc_canonical(const char *path) {
    char *canon = realpath(path, NULL);
    if (canon) {
        return canon;
    }
    return strdup(path);
}

static int nv_std_proc_is_space(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f';
}

static char **nv_std_proc_split(const char *cmd, int *out_n) {
    int cap = 8;
    int n = 0;
    char **argv = (char **)malloc((size_t)cap * sizeof(char *));
    const char *p = cmd;
    if (!argv) {
        *out_n = 0;
        return NULL;
    }
    while (*p) {
        while (*p && nv_std_proc_is_space(*p)) {
            p++;
        }
        if (!*p) {
            break;
        }
        const char *start = p;
        while (*p && !nv_std_proc_is_space(*p)) {
            p++;
        }
        size_t len = (size_t)(p - start);
        char *tok = (char *)malloc(len + 1);
        if (!tok) {
            for (int i = 0; i < n; i++) {
                free(argv[i]);
            }
            free(argv);
            *out_n = 0;
            return NULL;
        }
        memcpy(tok, start, len);
        tok[len] = '\0';
        if (n + 1 >= cap) {
            cap *= 2;
            char **grown = (char **)realloc(argv, (size_t)cap * sizeof(char *));
            if (!grown) {
                free(tok);
                for (int i = 0; i < n; i++) {
                    free(argv[i]);
                }
                free(argv);
                *out_n = 0;
                return NULL;
            }
            argv = grown;
        }
        argv[n++] = tok;
    }
    argv[n] = NULL;
    *out_n = n;
    return argv;
}

static void nv_std_proc_free_argv(char **argv, int n) {
    if (!argv) {
        return;
    }
    for (int i = 0; i < n; i++) {
        free(argv[i]);
    }
    free(argv);
}

// يبني وسائط التنفيذ: الأول والثاني مملوكان هنا، والباقي
// يُنسخ من وسائط المستخدم. يعيد NULL عند نفاد الذاكرة.
static char **nv_std_proc_build_exec_argv(char *first, char *second, char **user_args, int nuser) {
    int n = 1 + (second ? 1 : 0) + nuser;
    char **out = (char **)malloc((size_t)(n + 1) * sizeof(char *));
    if (!out) {
        return NULL;
    }
    out[0] = first;
    int k = 1;
    if (second) {
        out[k++] = second;
    }
    for (int i = 0; i < nuser; i++) {
        out[k] = strdup(user_args[i]);
        if (!out[k]) {
            for (int j = 0; j < k; j++) {
                free(out[j]);
            }
            free(out);
            return NULL;
        }
        k++;
    }
    out[k] = NULL;
    return out;
}

static NvValue nv_std_proc_spawn(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.أنشئ", args, nargs, 1, line, col);
    const char *cmd = nv_std_need_string("عمليات.أنشئ", args[0], line, col);
    int ntok = 0;
    char **argv = nv_std_proc_split(cmd, &ntok);
    if (!argv || ntok == 0) {
        nv_std_proc_free_argv(argv, ntok);
        nv_error(NV_ERR_RUNTIME, "الأمر فارغ — اكتب اسم برنامج", line, col);
    }
    if (nv_std_proc_live >= NV_STD_PROC_MAX_LIVE) {
        nv_std_proc_free_argv(argv, ntok);
        nv_error(NV_ERR_RUNTIME, "عدد العمليات المفتوحة تجاوز الحد (٢٥٦) — انتظر بعضها أو أوقفها", line, col);
    }
    if (!nv_program_dir || nv_program_dir[0] == '\0') {
        nv_std_proc_free_argv(argv, ntok);
        nv_error(NV_ERR_RUNTIME, "لا يمكن تشغيل برامج بلا مجلد برنامج", line, col);
    }
    const char *program = argv[0];
    char *display = nv_display_path(program);
    char *exec_first = NULL;
    char *exec_second = NULL;
    int use_path_search = 0;
    size_t prog_len = strlen(program);
    if (prog_len >= 9 && strcmp(program + prog_len - 9, ".نيفا") == 0) {
        // برنامج نيفا فرعي: يُحلّ المسار بقواعد الملفات ثم
        // يُشغَّل بثنائي نيفا الحالي نفسه.
        char *resolved = nv_resolve_path(program, line, col);
        char exe[4096];
        ssize_t elen = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
        if (elen <= 0) {
            nv_std_proc_free_argv(argv, ntok);
            free(display);
            free(resolved);
            nv_error(NV_ERR_RUNTIME, "تعذّر تحديد مسار ثنائي نيفا الحالي", line, col);
        }
        exe[elen] = '\0';
        const char *base = strrchr(exe, '/');
        base = base ? base + 1 : exe;
        char stem[4096];
        size_t blen = strlen(base);
        if (blen >= sizeof(stem)) {
            blen = sizeof(stem) - 1;
        }
        memcpy(stem, base, blen);
        stem[blen] = '\0';
        char *dot = strrchr(stem, '.');
        if (dot && dot != stem) {
            *dot = '\0';
        }
        if (strcmp(stem, "niva") == 0 || strcmp(stem, "نيفا") == 0) {
            exec_first = strdup(exe);
        } else {
            // ثنائي أصلي (اسمه اسم البرنامج لا نيفا): لا
            // يستطيع تشغيل مصدر نيفا بنفسه — ابحث عن
            // محرك نيفا المثبت وشغّل به البرنامج الفرعي.
            // الترتيب: ~/.local/bin/نيفا ثم نيفا/niva في PATH.
            exec_first = NULL;
            const char *home = getenv("HOME");
            if (home && home[0]) {
                char cand[4096];
                snprintf(cand, sizeof(cand), "%s/.local/bin/نيفا", home);
                if (access(cand, X_OK) == 0) exec_first = strdup(cand);
            }
            if (!exec_first) {
                const char *path = getenv("PATH");
                const char *names[2] = {"نيفا", "niva"};
                if (path) {
                    for (int ni = 0; ni < 2 && !exec_first; ni++) {
                        const char *cur = path;
                        while (*cur && !exec_first) {
                            const char *colon = strchr(cur, ':');
                            size_t dlen = colon ? (size_t)(colon - cur) : strlen(cur);
                            char cand[4096];
                            if (dlen == 0) {
                                snprintf(cand, sizeof(cand), "%s", names[ni]);
                            } else {
                                snprintf(cand, sizeof(cand), "%.*s/%s", (int)dlen, cur, names[ni]);
                            }
                            if (access(cand, X_OK) == 0) exec_first = strdup(cand);
                            cur = colon ? colon + 1 : cur + strlen(cur);
                        }
                    }
                }
            }
            if (!exec_first) {
                nv_std_proc_free_argv(argv, ntok);
                free(display);
                free(resolved);
                nv_error(NV_ERR_RUNTIME, "تشغيل برنامج نيفا فرعي غير مدعوم من هذا الثنائي", line, col);
            }
        }
        exec_second = resolved;
        if (!exec_first) {
            nv_std_proc_free_argv(argv, ntok);
            free(display);
            free(resolved);
            nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل العملية الجديدة", line, col);
        }
    } else if (nv_std_proc_is_path_token(program)) {
        // مسار: يُحلّ بقواعد الملفات ثم يُفحص التنفيذ.
        char *resolved = nv_resolve_path(program, line, col);
        exec_first = nv_std_proc_canonical(resolved);
        free(resolved);
        nv_std_proc_check_executable(exec_first, display, line, col);
    } else if (nv_trusted) {
        // اسم مجرد في الوضع الموثوق: بحث في المسار.
        exec_first = strdup(program);
        use_path_search = 1;
        if (!exec_first) {
            nv_std_proc_free_argv(argv, ntok);
            free(display);
            nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل العملية الجديدة", line, col);
        }
    } else {
        // اسم مجرد في الوضع المقيد: ملف تنفيذي في مجلد
        // الجلسة (داخل مجلد البرنامج).
        char *resolved = nv_resolve_path(program, line, col);
        exec_first = nv_std_proc_canonical(resolved);
        free(resolved);
        nv_std_proc_check_executable(exec_first, display, line, col);
    }
    char **exec_argv = nv_std_proc_build_exec_argv(exec_first, exec_second, argv + 1, ntok - 1);
    nv_std_proc_free_argv(argv, ntok);
    if (!exec_argv) {
        free(display);
        nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل العملية الجديدة", line, col);
    }
    int pipefd[2];
    if (nv_std_pipe2(pipefd, O_CLOEXEC) != 0) {
        int e = errno;
        nv_std_proc_free_argv(exec_argv, 1 + (exec_second ? 1 : 0) + (ntok - 1));
        nv_std_proc_spawn_failed(e, display, line, col);
    }
    pid_t pid = fork();
    if (pid < 0) {
        int e = errno;
        close(pipefd[0]);
        close(pipefd[1]);
        nv_std_proc_free_argv(exec_argv, 1 + (exec_second ? 1 : 0) + (ntok - 1));
        nv_std_proc_spawn_failed(e, display, line, col);
    }
    if (pid == 0) {
        close(pipefd[0]);
        int devnull = open("/dev/null", O_RDONLY);
        if (devnull >= 0) {
            if (dup2(devnull, 0) < 0) {
                // إدخال فارغ تعذّر — أكمل بالإدخال الموروث.
            }
            if (devnull > 0) {
                close(devnull);
            }
        }
        if (use_path_search) {
            execvp(exec_argv[0], exec_argv);
        } else {
            execv(exec_argv[0], exec_argv);
        }
        int e = errno;
        ssize_t w = write(pipefd[1], &e, sizeof(e));
        (void)w;
        _exit(127);
    }
    close(pipefd[1]);
    int child_err = 0;
    ssize_t got = 0;
    char *dst = (char *)&child_err;
    while (got < (ssize_t)sizeof(child_err)) {
        ssize_t r = read(pipefd[0], dst + got, sizeof(child_err) - (size_t)got);
        if (r <= 0) {
            break;
        }
        got += r;
    }
    close(pipefd[0]);
    if (got == (ssize_t)sizeof(child_err)) {
        // فشل التنفيذ في الابن: اجمعه ثم ارفع الخطأ.
        int status = 0;
        (void)waitpid(pid, &status, 0);
        int e = child_err;
        nv_std_proc_free_argv(exec_argv, 1 + (exec_second ? 1 : 0) + (ntok - 1));
        nv_std_proc_spawn_failed(e, display, line, col);
    }
    struct NvStdProc *p = (struct NvStdProc *)malloc(sizeof(*p));
    if (!p) {
        (void)kill(pid, SIGKILL);
        int status = 0;
        (void)waitpid(pid, &status, 0);
        nv_std_proc_free_argv(exec_argv, 1 + (exec_second ? 1 : 0) + (ntok - 1));
        free(display);
        nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل العملية الجديدة", line, col);
    }
    p->pid = (long)pid;
    p->waited = 0;
    int id = nv_std_proc_next_id++;
    nv_std_proc_live++;
    nv_std_proc_free_argv(exec_argv, 1 + (exec_second ? 1 : 0) + (ntok - 1));
    free(display);
    return nv_handle_new(2, id, p);
}

static NvValue nv_std_proc_wait(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.انتظر", args, nargs, 1, line, col);
    NvHandle *h = nv_std_proc_need_handle("عمليات.انتظر", args[0], 2, "معرف عملية", line, col);
    struct NvStdProc *p = (struct NvStdProc *)h->ptr;
    if (!p || p->waited) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "عملية غير معروفة: %d", h->id);
    }
    int status = 0;
    pid_t r = waitpid((pid_t)p->pid, &status, 0);
    if (r < 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر انتظار العملية: %d", h->id);
    }
    p->waited = 1;
    nv_std_proc_live--;
    long code = -1;
    if (WIFEXITED(status)) {
        code = (long)WEXITSTATUS(status);
    }
    return nv_num((double)code);
}

static NvValue nv_std_proc_kill(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.أوقف", args, nargs, 1, line, col);
    NvHandle *h = nv_std_proc_need_handle("عمليات.أوقف", args[0], 2, "معرف عملية", line, col);
    struct NvStdProc *p = (struct NvStdProc *)h->ptr;
    if (!p || p->waited) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "عملية غير معروفة: %d", h->id);
    }
    int status = 0;
    pid_t r = waitpid((pid_t)p->pid, &status, WNOHANG);
    if (r < 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر إيقاف العملية: %d", h->id);
    }
    if (r == 0) {
        if (kill((pid_t)p->pid, SIGKILL) != 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر إيقاف العملية: %d", h->id);
        }
        // اجمع حتى لا يبقى زومبي.
        (void)waitpid((pid_t)p->pid, &status, 0);
    }
    p->waited = 1;
    nv_std_proc_live--;
    return nv_nil();
}

#else // _WIN32

// ---------- ويندوز: عمليات مرفوضة بالعربي ----------
// اجتماع 35 §50: fork/waitpid/pipe2 غير موجودة في
// MinGW؛ فالإنشاء والانتظار والإيقاف ترفض بالرسالة
// نفسها المستعملة لباقي الوحدات المقيّدة بالمنصة.
// والحالية أدناه تبقى عاملة (getpid موجود في MinGW).
static NvValue nv_std_proc_spawn(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.أنشئ", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "عمليات.أنشئ غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_proc_wait(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.انتظر", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "عمليات.انتظر غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_proc_kill(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.أوقف", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "عمليات.أوقف غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

#endif // _WIN32

static NvValue nv_std_proc_current(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("عمليات.الحالية", args, nargs, 0, line, col);
    return nv_num((double)getpid());
}

void nv_std_processes_fill(NvDict *d) {
    nv_std_add_fn(d, "انشي", nv_std_proc_spawn);
    nv_std_add_fn(d, "انتظر", nv_std_proc_wait);
    nv_std_add_fn(d, "أوقف", nv_std_proc_kill);
    nv_std_add_fn(d, "الحالية", nv_std_proc_current);
}

// ---------- ذاكرة ----------

static int nv_std_mem_next_id = 1;
static unsigned long long nv_std_mem_live_total = 0;

static NvHandle *nv_std_mem_need_handle(const char *display, NvValue v, int kind, const char *kind_word, int line, int col) {
    if (v.tag != NV_HANDLE || !v.u.handle || v.u.handle->kind != kind) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع %s لكن أعطيت %s", display, kind_word, nv_type_name(v));
    }
    return v.u.handle;
}

static NvValue nv_std_mem_alloc(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ذاكرة.احجز", args, nargs, 1, line, col);
    long long size = nv_std_need_integer("ذاكرة.احجز", args[0], line, col);
    if (size <= 0) {
        nv_error(NV_ERR_TYPE, "الدالة ذاكرة.احجز تتوقع حجما موجبا", line, col);
    }
    if ((unsigned long long)size > NV_STD_MEM_MAX_SINGLE) {
        nv_error(NV_ERR_RUNTIME, "حجم الكتلة تجاوز الحد (٦٤ ميبيبايت) — احجز كتلا أصغر", line, col);
    }
    if (nv_std_mem_live_total + (unsigned long long)size > NV_STD_MEM_MAX_TOTAL) {
        nv_error(NV_ERR_RUNTIME, "مجموع الذاكرة المحجوزة تجاوز الحد (٢٥٦ ميبيبايت) — حرّر بعض الكتل", line, col);
    }
    void *ptr = calloc(1, (size_t)size);
    if (!ptr) {
        nv_error(NV_ERR_RUNTIME, "تعذّر حجز الذاكرة", line, col);
    }
    struct NvStdMemBlock *b = (struct NvStdMemBlock *)malloc(sizeof(*b));
    if (!b) {
        free(ptr);
        nv_error(NV_ERR_RUNTIME, "تعذّر حجز الذاكرة", line, col);
    }
    b->ptr = ptr;
    b->size = (size_t)size;
    int id = nv_std_mem_next_id++;
    nv_std_mem_live_total += (unsigned long long)size;
    return nv_handle_new(5, id, b);
}

static NvValue nv_std_mem_free(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ذاكرة.حرّر", args, nargs, 1, line, col);
    NvHandle *h = nv_std_mem_need_handle("ذاكرة.حرّر", args[0], 5, "معرف كتلة", line, col);
    struct NvStdMemBlock *b = (struct NvStdMemBlock *)h->ptr;
    if (!b || !b->ptr) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "كتلة غير معروفة: %d", h->id);
    }
    size_t size = b->size;
    free(b->ptr);
    b->ptr = NULL;
    b->size = 0;
    nv_std_mem_live_total -= (unsigned long long)size;
    return nv_nil();
}

static NvValue nv_std_mem_available(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ذاكرة.المتاح", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    FILE *f = fopen("/proc/meminfo", "r");
    if (!f) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة ذاكرة النظام", line, col);
    }
    char linebuf[256];
    unsigned long long kb = 0;
    int found = 0;
    while (fgets(linebuf, sizeof(linebuf), f)) {
        if (strncmp(linebuf, "MemAvailable:", 13) == 0) {
            const char *p = linebuf + 13;
            unsigned long long v = 0;
            int digits = 0;
            while (*p) {
                if (*p >= '0' && *p <= '9') {
                    v = v * 10 + (unsigned long long)(*p - '0');
                    digits++;
                } else if (digits) {
                    break;
                }
                p++;
            }
            if (digits) {
                kb = v;
                found = 1;
            }
            break;
        }
    }
    fclose(f);
    if (!found) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة ذاكرة النظام", line, col);
    }
    return nv_num((double)(kb * 1024ull));
#else
    // المتاح لينكس/أندرويد فقط في المحرك — الرفض بنصه الحرفي
    // (بلا قيمة مختلقة)؛ وmacOS/ويندوز يصلان هنا.
    nv_error(NV_ERR_RUNTIME, "ذاكرة.المتاح غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

static NvValue nv_std_mem_page_size(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ذاكرة.حجم_الصفحة", args, nargs, 0, line, col);
#if defined(__linux__) || defined(__ANDROID__)
    long ps = sysconf(_SC_PAGESIZE);
    if (ps <= 0) {
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة ذاكرة النظام", line, col);
    }
    return nv_num((double)ps);
#else
    // حجم_الصفحة لينكس/أندرويد فقط في المحرك — الرفض بنصه
    // الحرفي؛ وعلى ماك كان sysconf ينجح بقيمة بينما المحرك
    // يرفض، فكانت حالة «ذاكرة-استعلامات» ستفشل هناك.
    nv_error(NV_ERR_RUNTIME, "ذاكرة.حجم_الصفحة غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
#endif
}

void nv_std_memory_fill(NvDict *d) {
    nv_std_add_fn(d, "احجز", nv_std_mem_alloc);
    nv_std_add_fn(d, "حرر", nv_std_mem_free);
    nv_std_add_fn(d, "المتاح", nv_std_mem_available);
    nv_std_add_fn(d, "حجم_الصفحة", nv_std_mem_page_size);
}

// ---------- ملفات ----------

static int nv_std_file_next_id = 1;

// سجل جانبي لصيغة عرض كل ملف مفتوح (لرسائل الأخطاء):
// حمولة الملف تحمل مؤشر الملف وحده، فالعرض يُحفظ هنا.
// بلا دالة إغلاق في النسخة الأولى، فالسجلات تعيش حتى
// نهاية العملية.
struct nv_std_file_rec {
    int id;
    char *display;
};
static struct nv_std_file_rec *nv_std_file_recs = NULL;
static size_t nv_std_file_recs_len = 0;
static size_t nv_std_file_recs_cap = 0;

static NvHandle *nv_std_file_need_handle(const char *display, NvValue v, int kind, const char *kind_word, int line, int col) {
    if (v.tag != NV_HANDLE || !v.u.handle || v.u.handle->kind != kind) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع %s لكن أعطيت %s", display, kind_word, nv_type_name(v));
    }
    return v.u.handle;
}

static int nv_std_file_rec_add(int id, const char *display) {
    if (nv_std_file_recs_len == nv_std_file_recs_cap) {
        size_t ncap = nv_std_file_recs_cap ? nv_std_file_recs_cap * 2 : 8;
        struct nv_std_file_rec *grown = (struct nv_std_file_rec *)realloc(nv_std_file_recs, ncap * sizeof(*grown));
        if (!grown) {
            return 0;
        }
        nv_std_file_recs = grown;
        nv_std_file_recs_cap = ncap;
    }
    char *copy = strdup(display);
    if (!copy) {
        return 0;
    }
    nv_std_file_recs[nv_std_file_recs_len].id = id;
    nv_std_file_recs[nv_std_file_recs_len].display = copy;
    nv_std_file_recs_len++;
    return 1;
}

static const char *nv_std_file_rec_display(int id) {
    for (size_t i = 0; i < nv_std_file_recs_len; i++) {
        if (nv_std_file_recs[i].id == id) {
            return nv_std_file_recs[i].display;
        }
    }
    return "";
}

static int nv_std_file_valid_utf8(const unsigned char *s, size_t n) {
    size_t i = 0;
    while (i < n) {
        unsigned char c = s[i];
        size_t adv = 1;
        if ((c & 0x80) == 0) {
            adv = 1;
        } else if ((c & 0xE0) == 0xC0) {
            adv = 2;
        } else if ((c & 0xF0) == 0xE0) {
            adv = 3;
        } else if ((c & 0xF8) == 0xF0) {
            adv = 4;
        } else {
            return 0;
        }
        if (i + adv > n) {
            return 0;
        }
        for (size_t k = 1; k < adv; k++) {
            if ((s[i + k] & 0xC0) != 0x80) {
                return 0;
            }
        }
        i += adv;
    }
    return 1;
}

static NvValue nv_std_file_open(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.افتح", args, nargs, 1, line, col);
    const char *path = nv_std_need_string("ملفات.افتح", args[0], line, col);
    char *resolved = nv_resolve_path(path, line, col);
    char *display = nv_display_path(path);
    struct stat st;
    if (stat(resolved, &st) == 0 && S_ISDIR(st.st_mode)) {
        free(resolved);
        nv_error_fmt(NV_ERR_IO, line, col, "ليس ملفا: %s", display);
    }
    int fd = open(resolved, O_RDWR | O_CREAT, 0666);
    if (fd < 0) {
        free(resolved);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر فتح الملف %s", display);
    }
    FILE *fp = fdopen(fd, "r+b");
    free(resolved);
    if (!fp) {
        close(fd);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر فتح الملف %s", display);
    }
    struct NvStdFile *f = (struct NvStdFile *)malloc(sizeof(*f));
    if (!f) {
        fclose(fp);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر فتح الملف %s", display);
    }
    f->fp = fp;
    int id = nv_std_file_next_id++;
    if (!nv_std_file_rec_add(id, display)) {
        free(f);
        fclose(fp);
        free(display);
        nv_error(NV_ERR_RUNTIME, "تعذّر تسجيل الملف الجديد", line, col);
    }
    free(display);
    return nv_handle_new(1, id, f);
}

static NvValue nv_std_file_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.اقرأ", args, nargs, 1, line, col);
    NvHandle *h = nv_std_file_need_handle("ملفات.اقرأ", args[0], 1, "معرف ملف", line, col);
    struct NvStdFile *f = (struct NvStdFile *)h->ptr;
    const char *display = nv_std_file_rec_display(h->id);
    if (!f || !f->fp) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "ملف غير معروف: %d", h->id);
    }
    if (fseek(f->fp, 0, SEEK_SET) != 0) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر قراءة الملف %s", display);
    }
    unsigned char *buf = (unsigned char *)malloc(NV_STD_FILE_MAX_READ + 2);
    if (!buf) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر قراءة الملف %s", display);
    }
    size_t n = 0;
    while (n <= NV_STD_FILE_MAX_READ) {
        size_t r = fread(buf + n, 1, (size_t)(NV_STD_FILE_MAX_READ + 1) - n, f->fp);
        n += r;
        if (r == 0) {
            break;
        }
    }
    if (ferror(f->fp)) {
        free(buf);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر قراءة الملف %s", display);
    }
    if (n > NV_STD_FILE_MAX_READ) {
        free(buf);
        nv_error_fmt(NV_ERR_IO, line, col, "الملف %s أكبر من الحد الأقصى (5 ميغابايت)", display);
    }
    if (!nv_std_file_valid_utf8(buf, n)) {
        free(buf);
        nv_error_fmt(NV_ERR_IO, line, col, "الملف %s ليس نصا صالحا (UTF-8)", display);
    }
    NvValue out = nv_str_new((const char *)buf, n);
    free(buf);
    return out;
}

static NvValue nv_std_file_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.اكتب", args, nargs, 2, line, col);
    NvHandle *h = nv_std_file_need_handle("ملفات.اكتب", args[0], 1, "معرف ملف", line, col);
    const char *content = nv_std_need_string("ملفات.اكتب", args[1], line, col);
    size_t len = args[1].u.str->len;
    struct NvStdFile *f = (struct NvStdFile *)h->ptr;
    const char *display = nv_std_file_rec_display(h->id);
    if (!f || !f->fp) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "ملف غير معروف: %d", h->id);
    }
    struct stat st;
    unsigned long long existing = 0;
    if (fstat(fileno(f->fp), &st) == 0 && S_ISREG(st.st_mode)) {
        existing = (unsigned long long)st.st_size;
    }
    unsigned long long extra = len > existing ? (unsigned long long)(len - existing) : 0;
    nv_check_quota((size_t)extra, line, col);
    if (fseek(f->fp, 0, SEEK_SET) != 0) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر كتابة الملف %s", display);
    }
    if (ftruncate(fileno(f->fp), 0) != 0) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر كتابة الملف %s", display);
    }
    size_t off = 0;
    while (off < len) {
        size_t w = fwrite((const unsigned char *)content + off, 1, len - off, f->fp);
        if (w == 0) {
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر كتابة الملف %s", display);
        }
        off += w;
    }
    if (fflush(f->fp) != 0) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر كتابة الملف %s", display);
    }
    return nv_nil();
}

// ---------- عمليات الملفات الممنوحة ----------
// الفحص أولا (عدد ونوع)، ثم رفض المنصة: هذا البناء
// أصلي للينكس لا أندرويد، فالرفض دائم هنا.

static long long nv_std_file_saf_tree_id(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا لمعرف الشجرة لكن أعطيت %s", display, nv_type_name(v));
    }
    double x = v.u.num;
    if (x != trunc(x) || x < 1.0 || x >= 9.0e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا موجبا لمعرف الشجرة", display);
    }
    return (long long)x;
}

static const char *nv_std_file_saf_path(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصا للمسار لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.str->data;
}

static const char *nv_std_file_saf_text(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصا للنص لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.str->data;
}

static void nv_std_file_saf_refuse(const char *display, int line, int col) __attribute__((noreturn));

static void nv_std_file_saf_refuse(const char *display, int line, int col) {
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "%s غير مدعوم على هذه المنصة", display);
}

static NvValue nv_std_file_saf_grant(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.امنح_مجلد", args, nargs, 0, line, col);
    nv_std_file_saf_refuse("ملفات.امنح_مجلد", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_list(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.سرد_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.سرد_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.سرد_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.سرد_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.اقرأ_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.اقرأ_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.اقرأ_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.اقرأ_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.اكتب_ممنوح", args, nargs, 3, line, col);
    nv_std_file_saf_tree_id("ملفات.اكتب_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.اكتب_ممنوح", args[1], line, col);
    nv_std_file_saf_text("ملفات.اكتب_ممنوح", args[2], line, col);
    nv_std_file_saf_refuse("ملفات.اكتب_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_create_file(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.أنشئ_ملف_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.أنشئ_ملف_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.أنشئ_ملف_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.أنشئ_ملف_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_create_dir(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.أنشئ_مجلد_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.أنشئ_مجلد_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.أنشئ_مجلد_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.أنشئ_مجلد_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_stat(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.تفاصيل_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.تفاصيل_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.تفاصيل_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.تفاصيل_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_delete(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.احذف_ممنوح", args, nargs, 2, line, col);
    nv_std_file_saf_tree_id("ملفات.احذف_ممنوح", args[0], line, col);
    nv_std_file_saf_path("ملفات.احذف_ممنوح", args[1], line, col);
    nv_std_file_saf_refuse("ملفات.احذف_ممنوح", line, col);
    return nv_nil();
}

static NvValue nv_std_file_saf_list_trees(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ملفات.الممنوح", args, nargs, 0, line, col);
    nv_std_file_saf_refuse("ملفات.الممنوح", line, col);
    return nv_nil();
}

void nv_std_files_fill(NvDict *d) {
    nv_std_add_fn(d, "افتح", nv_std_file_open);
    nv_std_add_fn(d, "اقرأ", nv_std_file_read);
    nv_std_add_fn(d, "اكتب", nv_std_file_write);
    nv_std_add_fn(d, "امنح_مجلد", nv_std_file_saf_grant);
    nv_std_add_fn(d, "سرد_ممنوح", nv_std_file_saf_list);
    nv_std_add_fn(d, "اقرأ_ممنوح", nv_std_file_saf_read);
    nv_std_add_fn(d, "اكتب_ممنوح", nv_std_file_saf_write);
    nv_std_add_fn(d, "أنشئ_ملف_ممنوح", nv_std_file_saf_create_file);
    nv_std_add_fn(d, "أنشئ_مجلد_ممنوح", nv_std_file_saf_create_dir);
    nv_std_add_fn(d, "تفاصيل_ممنوح", nv_std_file_saf_stat);
    nv_std_add_fn(d, "احذف_ممنوح", nv_std_file_saf_delete);
    nv_std_add_fn(d, "الممنوح", nv_std_file_saf_list_trees);
}
// قطعة قسم: تواصل + غير_متزامن + جهاز — خلفية C (مهمة 29).
// قطعة لا وحدة ترجمة مستقلة: بلا أي سطر تضمين وبلا دالة رئيسية.
// تُدمج بعد مقدمة مكتبة الوحدات الأصلية فترى مساعداتها
// (فحص الوسائط والإضافة إلى القاموس) وبنى حمولات المقابض
// (قناة للنوع 3 وحلقة للنوع 4) والمنظف الموحد للمقابض.
// كل رسائل الأخطاء والنصوص الظاهرة عربية حرفيا كما في
// المصدر (الترجمة التنفيذية): انسخها حرفا حرفا ولا تخترع
// رسالة. والمعرفات الداخلية لاتينية فقط.
// الدوال الأصلية كلها ساكنة بأسماء تبدأ بإحدى البادئات
// الثلاث، فلا تتصادم مع أقسام أخرى.
// ملاحظة عن المنصة: هذا البناء لينكس لا أندرويد؛ فكل
// دوال جهاز تتحقق من وسائطها أولا ثم ترفض برسالة
// «جهاز.… غير مدعوم على هذه المنصة» كما في المصدر.

 // ================= تواصل =================
// أنبوب + عملية فرعية بلا صدفة أبدا (تقسيم على الفراغات
// فقط): رمز أول ينتهي بـ .نيفا يعمل برنامجا فرعيا من
// نيفا عبر الثنائي الحالي؛ ورمز فيه شرطة مائلة أو شرطة
// عكسية مسار يُحل بقواعد الملفات ويُفحص تنفيذه؛ واسم
// مجرد بحث في المسار عند الثقة وإلا ملف في مجلد الجلسة.
// قناة واحدة رسالة واحدة سطر واحد بلا أسطر جديدة وبحد
// 64 كيبيبايت؛ واستقبال بمهلة 30 ثانية؛ وأغلق يغلق دخل
// العملية (نهاية الملف) وينتظر خروجها 30 ثانية ثم يقتل؛
// وأوقف يقتل فورا ويجمعها فلا يبقى يتيم. والمعرفات من 1
// ولا تتكرر في التشغيل الواحد، وبحد 64 قناة حية.

// كل تنفيذ هذا القسم POSIX خالص (fork/pipe/
// waitpid/kill) — يُستبعد كاملا على ويندوز (فرع
// #else أدناه: رفض عربي)؛ والتعبئة بعده باقية.
#ifndef _WIN32

struct NvStdIpcQueue {
    char **lines; // سطور بانتظار القراءة (بترتيب الوصول)
    size_t head;
    size_t tail;
    size_t len;
    size_t cap; // 64 سطرا
    int eof; // قارئ الخيط رأى نهاية مخرجات العملية
    int abandoned; // أزيلت القناة من السجل (أغلق/أوقف)
    pthread_mutex_t mu;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
};

struct NvStdIpcEntry {
    unsigned long long id;
    struct NvStdChannel ch; // read_fd مخرجات العملية، write_fd دخلها، pid
    struct NvStdIpcQueue q;
    pthread_mutex_t send_mu; // يسرّع الإرسال (كتابة واحدة في كل مرة)
    pthread_t reader;
    int refs; // السجل + خيط القارئ + مستخدمون عابرون
    struct NvStdIpcEntry *next;
};

static pthread_mutex_t nv_std_ipc_reg_mu = PTHREAD_MUTEX_INITIALIZER;
static struct NvStdIpcEntry *nv_std_ipc_reg = NULL;
static unsigned long long nv_std_ipc_next_id = 1;
static int nv_std_ipc_live = 0;

static char *nv_std_ipc_strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *out = (char *)malloc(n);
    memcpy(out, s, n);
    return out;
}

// عرض مسار كما يعرضه المحرك (كما في display_str
// بالمصدر): ~ كما هي، والمطلق عبر nv_display_path،
// والنسبي كما هو.
static char *nv_std_ipc_display_str(const char *s) {
    if (strcmp(s, "~") == 0 || strncmp(s, "~/", 2) == 0) return nv_std_ipc_strdup(s);
    if (s[0] == '/') return nv_display_path(s);
    return nv_std_ipc_strdup(s);
}

static void nv_std_ipc_unknown(unsigned long long id, int line, int col) {
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "قناة غير معروفة: %llu", id);
}

static void nv_std_ipc_unref(struct NvStdIpcEntry *e) {
    int dead = 0;
    pthread_mutex_lock(&nv_std_ipc_reg_mu);
    if (--e->refs == 0) dead = 1;
    pthread_mutex_unlock(&nv_std_ipc_reg_mu);
    if (!dead) return;
    for (size_t i = 0; i < e->q.len; i++) {
        free(e->q.lines[(e->q.head + i) % e->q.cap]);
    }
    free(e->q.lines);
    pthread_mutex_destroy(&e->q.mu);
    pthread_cond_destroy(&e->q.not_empty);
    pthread_cond_destroy(&e->q.not_full);
    pthread_mutex_destroy(&e->send_mu);
    free(e);
}

static struct NvStdIpcEntry *nv_std_ipc_lookup(unsigned long long id) {
    pthread_mutex_lock(&nv_std_ipc_reg_mu);
    struct NvStdIpcEntry *e = nv_std_ipc_reg;
    while (e && e->id != id) e = e->next;
    if (e) e->refs++;
    pthread_mutex_unlock(&nv_std_ipc_reg_mu);
    return e;
}

static struct NvStdIpcEntry *nv_std_ipc_take(unsigned long long id) {
    pthread_mutex_lock(&nv_std_ipc_reg_mu);
    struct NvStdIpcEntry **pp = &nv_std_ipc_reg;
    while (*pp && (*pp)->id != id) pp = &(*pp)->next;
    struct NvStdIpcEntry *e = *pp;
    if (e) {
        *pp = e->next;
        nv_std_ipc_live--;
    }
    pthread_mutex_unlock(&nv_std_ipc_reg_mu);
    return e; // يملك المتصل مرجع السجل
}

static void nv_std_ipc_kill_wait(pid_t pid) {
    kill(pid, SIGKILL);
    int st = 0;
    pid_t w;
    do {
        w = waitpid(pid, &st, 0);
    } while (w < 0 && errno == EINTR);
}

// حرر مدخلا لم يُنشر أبدا (فشل تسجيل أو فشل خيط قارئ):
// يغلق طرفي الأنبوب ويدمر القفلين. لا يُستدعى أبدا بعد
// بدء خيط القارئ.
static void nv_std_ipc_drop_unpublished(struct NvStdIpcEntry *e) {
    if (e->ch.write_fd >= 0) close(e->ch.write_fd);
    if (e->ch.read_fd >= 0) close(e->ch.read_fd);
    free(e->q.lines);
    pthread_mutex_destroy(&e->q.mu);
    pthread_cond_destroy(&e->q.not_empty);
    pthread_cond_destroy(&e->q.not_full);
    pthread_mutex_destroy(&e->send_mu);
    free(e);
}

// أدخل سطرا في طابور القناة (يحجب عند الامتلاء كما في
// قناة المصدر المحدودة). يعيد 0 إن هُجر الطابور.
static int nv_std_ipc_queue_push(struct NvStdIpcEntry *e, char *line) {
    struct NvStdIpcQueue *q = &e->q;
    pthread_mutex_lock(&q->mu);
    while (q->len >= q->cap && !q->abandoned) {
        pthread_cond_wait(&q->not_full, &q->mu);
    }
    if (q->abandoned) {
        pthread_mutex_unlock(&q->mu);
        return 0;
    }
    q->lines[q->tail] = line;
    q->tail = (q->tail + 1) % q->cap;
    q->len++;
    pthread_cond_signal(&q->not_empty);
    pthread_mutex_unlock(&q->mu);
    return 1;
}

// خيط قارئ قناة واحدة: سطور مخرجات العملية إلى الطابور
// (بلا سطر جديد، ومع حذف \r قبلها). سطر أخير بلا سطر
// جديد يُسلَّم أيضا ثم نهاية الملف. يملك طرف القراءة
// فيغلقه عند خروجه، ويضع علامة النهاية ويُسقط مرجعه.
static void *nv_std_ipc_reader(void *arg) {
    struct NvStdIpcEntry *e = (struct NvStdIpcEntry *)arg;
    int fd = e->ch.read_fd;
    char *buf = NULL;
    size_t len = 0;
    size_t cap = 0;
    char chunk[4096];
    int done = 0;
    while (!done) {
        ssize_t r = read(fd, chunk, sizeof chunk);
        if (r < 0) {
            if (errno == EINTR) continue;
            r = 0; // خطأ قراءة يُعامل نهاية ملف
        }
        if (r == 0) {
            if (len > 0) {
                char *line = buf;
                buf = NULL;
                len = 0;
                cap = 0;
                if (!nv_std_ipc_queue_push(e, line)) free(line);
            }
            done = 1;
            break;
        }
        if (len + (size_t)r + 1 > cap) {
            size_t ncap = cap ? cap : 1024;
            while (ncap < len + (size_t)r + 1) ncap *= 2;
            char *nb = (char *)realloc(buf, ncap);
            if (!nb) {
                done = 1;
                break;
            }
            buf = nb;
            cap = ncap;
        }
        memcpy(buf + len, chunk, (size_t)r);
        len += (size_t)r;
        buf[len] = '\0';
        size_t scan = 0;
        while (scan < len) {
            char *nl = (char *)memchr(buf + scan, '\n', len - scan);
            if (!nl) break;
            size_t linelen = (size_t)(nl - buf);
            if (linelen > 0 && buf[linelen - 1] == '\r') linelen--;
            char *line = (char *)malloc(linelen + 1);
            if (!line) {
                done = 1;
                break;
            }
            memcpy(line, buf, linelen);
            line[linelen] = '\0';
            size_t consumed = (size_t)(nl - buf) + 1;
            memmove(buf, buf + consumed, len - consumed);
            len -= consumed;
            scan = 0;
            if (!nv_std_ipc_queue_push(e, line)) {
                free(line);
                done = 1;
                break;
            }
        }
    }
    free(buf);
    close(fd);
    pthread_mutex_lock(&e->q.mu);
    e->q.eof = 1;
    pthread_cond_broadcast(&e->q.not_empty);
    pthread_cond_broadcast(&e->q.not_full);
    pthread_mutex_unlock(&e->q.mu);
    nv_std_ipc_unref(e);
    return NULL;
}

// اسحب سطرا من الطابور بمهلة 30 ثانية. يستهلك مرجع
// البحث الذي أخذه المتصل، في كل المخارج.
static char *nv_std_ipc_queue_pop(struct NvStdIpcEntry *e, unsigned long long id, int line, int col) {
    struct NvStdIpcQueue *q = &e->q;
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += 30;
    if (pthread_mutex_lock(&q->mu) != 0) {
        nv_std_ipc_unref(e);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر الوصول إلى القناة: %llu", id);
    }
    for (;;) {
        if (q->len > 0) {
            char *out = q->lines[q->head];
            q->head = (q->head + 1) % q->cap;
            q->len--;
            pthread_cond_signal(&q->not_full);
            pthread_mutex_unlock(&q->mu);
            nv_std_ipc_unref(e);
            return out;
        }
        if (q->eof) {
            pthread_mutex_unlock(&q->mu);
            nv_std_ipc_unref(e);
            nv_error(NV_ERR_RUNTIME, "أغلق الطرف الآخر الاتصال", line, col);
        }
        int rc = pthread_cond_timedwait(&q->not_empty, &q->mu, &deadline);
        if (rc == ETIMEDOUT && q->len == 0 && !q->eof) {
            pthread_mutex_unlock(&q->mu);
            nv_std_ipc_unref(e);
            nv_error(NV_ERR_RUNTIME, "انتهت مهلة الاستقبال (30 ثانية) — لم تصل رسالة", line, col);
        }
    }
}

// قسّم أمرا على الفراغات (بلا صدفة وبلا علامات تنصيص).
// تعيد مصفوفة وسائط تشير إلى نسخة يملكها المتصل.
static char **nv_std_ipc_split(const char *command, char **copy_out, int *count_out) {
    char *copy = nv_std_ipc_strdup(command);
    int n = 0;
    char *p = copy;
    while (*p) {
        while (*p && isspace((unsigned char)*p)) p++;
        if (!*p) break;
        n++;
        while (*p && !isspace((unsigned char)*p)) p++;
    }
    char **argv = (char **)malloc((size_t)(n + 1) * sizeof(char *));
    int i = 0;
    p = copy;
    while (*p) {
        while (*p && isspace((unsigned char)*p)) {
            *p = '\0';
            p++;
        }
        if (!*p) break;
        argv[i++] = p;
        while (*p && !isspace((unsigned char)*p)) p++;
    }
    argv[i] = NULL;
    *copy_out = copy;
    *count_out = n;
    return argv;
}

// ابنِ أمر التشغيل المشترك مع عمليات.أنشئ ثم افرع
// العملية بأنبوبي دخل ومخرجات، وسجّل القناة وابدأ
// قارئها. الأخطاء عربية كما في المصدر، والرسالة
// الواحدة تُجمع في emsg فيُحرَّر كل شيء قبل إطلاقها.
static struct NvStdIpcEntry *nv_std_ipc_spawn(const char *command, int line, int col) {
    char emsg[2048];
    char *copy = NULL;
    char **argv = NULL;
    int argc = 0;
    char *display = NULL;
    char *resolved = NULL;
    char *exec_path = NULL;
    char **exec_argv = NULL;
    int use_path = 0;
    int pin[2] = { -1, -1 };
    int pout[2] = { -1, -1 };
    int perr[2] = { -1, -1 };
    pid_t pid = -1;
    int child_started = 0;

    argv = nv_std_ipc_split(command, &copy, &argc);
    if (argc == 0) {
        snprintf(emsg, sizeof emsg, "الأمر فارغ — اكتب اسم برنامج");
        goto fail;
    }
    display = nv_std_ipc_display_str(argv[0]);
    pthread_mutex_lock(&nv_std_ipc_reg_mu);
    int live = nv_std_ipc_live;
    pthread_mutex_unlock(&nv_std_ipc_reg_mu);
    if (live >= 64) {
        snprintf(emsg, sizeof emsg, "عدد القنوات المفتوحة تجاوز الحد (٦٤) — أغلق بعضها");
        goto fail;
    }
    if (!nv_program_dir || nv_program_dir[0] == '\0') {
        snprintf(emsg, sizeof emsg, "لا يمكن تشغيل برامج بلا مجلد برنامج");
        goto fail;
    }
    {
        const char *program = argv[0];
        size_t plen = strlen(program);
        const char *suf = ".نيفا";
        size_t slen = strlen(suf);
        int is_niva = (plen >= slen && strcmp(program + plen - slen, suf) == 0);
        int is_path = (strchr(program, '/') != NULL || strchr(program, '\\') != NULL);
        if (is_niva) {
            resolved = nv_resolve_path(program, line, col);
            char exe_buf[4096];
            ssize_t exelen = readlink("/proc/self/exe", exe_buf, sizeof exe_buf - 1);
            if (exelen < 0) {
                snprintf(emsg, sizeof emsg, "تعذّر تحديد مسار ثنائي نيفا الحالي");
                goto fail;
            }
            exe_buf[exelen] = '\0';
            char *base = strrchr(exe_buf, '/');
            base = base ? base + 1 : exe_buf;
            char *dot = strrchr(base, '.');
            size_t stemlen = dot ? (size_t)(dot - base) : strlen(base);
            if (!((stemlen == 4 && memcmp(base, "niva", 4) == 0) ||
                  (stemlen == 8 && memcmp(base, "نيفا", 8) == 0))) {
                snprintf(emsg, sizeof emsg, "تشغيل برنامج نيفا فرعي غير مدعوم من هذا الثنائي");
                goto fail;
            }
            exec_path = nv_std_ipc_strdup(exe_buf);
            use_path = 0;
        } else if (is_path || !nv_trusted) {
            resolved = nv_resolve_path(program, line, col);
            char *canon = realpath(resolved, NULL);
            if (!canon) canon = nv_std_ipc_strdup(resolved);
            struct stat st;
            if (stat(canon, &st) != 0) {
                if (errno == ENOENT) {
                    snprintf(emsg, sizeof emsg, "البرنامج غير موجود: %s", display);
                } else {
                    snprintf(emsg, sizeof emsg, "تعذّر تشغيل البرنامج: %s", display);
                }
                free(canon);
                goto fail;
            }
            if ((st.st_mode & 0111) == 0) {
                snprintf(emsg, sizeof emsg, "الملف ليس برنامجا قابلا للتشغيل: %s", display);
                free(canon);
                goto fail;
            }
            exec_path = canon;
            use_path = 0;
        } else {
            exec_path = nv_std_ipc_strdup(program);
            use_path = 1;
        }
        int eargc = argc + (is_niva ? 1 : 0);
        exec_argv = (char **)malloc((size_t)(eargc + 1) * sizeof(char *));
        if (!exec_argv) {
            snprintf(emsg, sizeof emsg, "تعذّر فتح قناة للبرنامج");
            goto fail;
        }
        int j = 0;
        exec_argv[j++] = exec_path;
        if (is_niva) exec_argv[j++] = resolved;
        for (int i = 1; i < argc; i++) exec_argv[j++] = argv[i];
        exec_argv[j] = NULL;
    }
    if (pipe(pin) != 0 || pipe(pout) != 0 || pipe(perr) != 0) {
        snprintf(emsg, sizeof emsg, "تعذّر فتح قناة للبرنامج");
        goto fail;
    }
    fcntl(perr[1], F_SETFD, FD_CLOEXEC);
    pid = fork();
    if (pid < 0) {
        snprintf(emsg, sizeof emsg, "تعذّر تشغيل البرنامج: %s", display);
        goto fail;
    }
    child_started = 1;
    if (pid == 0) {
        close(perr[0]);
        close(pin[1]);
        close(pout[0]);
        if (dup2(pin[0], 0) < 0) _exit(127);
        if (dup2(pout[1], 1) < 0) _exit(127);
        close(pin[0]);
        close(pout[1]);
        if (use_path) execvp(exec_path, exec_argv);
        else execv(exec_path, exec_argv);
        int e = errno;
        (void)!write(perr[1], &e, sizeof e);
        _exit(127);
    }
    close(pin[0]);
    pin[0] = -1;
    close(pout[1]);
    pout[1] = -1;
    close(perr[1]);
    perr[1] = -1;
    {
        int exec_errno = 0;
        ssize_t got = 0;
        char *dst = (char *)&exec_errno;
        size_t left = sizeof exec_errno;
        while (left > 0) {
            ssize_t r = read(perr[0], dst, left);
            if (r < 0) {
                if (errno == EINTR) continue;
                break;
            }
            if (r == 0) break;
            dst += r;
            left -= (size_t)r;
            got += r;
        }
        close(perr[0]);
        perr[0] = -1;
        if (got > 0) {
            if (got == (ssize_t)sizeof exec_errno && exec_errno == ENOENT) {
                snprintf(emsg, sizeof emsg, "البرنامج غير موجود: %s", display);
            } else if (got == (ssize_t)sizeof exec_errno && exec_errno == EACCES) {
                snprintf(emsg, sizeof emsg, "لا يمكن تشغيل البرنامج (إذن مرفوض): %s", display);
            } else {
                snprintf(emsg, sizeof emsg, "تعذّر تشغيل البرنامج: %s", display);
            }
            goto fail;
        }
    }
    {
        struct NvStdIpcEntry *e = (struct NvStdIpcEntry *)calloc(1, sizeof *e);
        if (!e) {
            snprintf(emsg, sizeof emsg, "تعذّر فتح قناة للبرنامج");
            nv_std_ipc_kill_wait(pid);
            child_started = 0;
            goto fail;
        }
        e->ch.read_fd = pout[0];
        e->ch.write_fd = pin[1];
        e->ch.pid = (long)pid;
        e->q.cap = 64;
        e->q.lines = (char **)calloc(e->q.cap, sizeof(char *));
        if (!e->q.lines) {
            free(e);
            snprintf(emsg, sizeof emsg, "تعذّر فتح قناة للبرنامج");
            nv_std_ipc_kill_wait(pid);
            child_started = 0;
            goto fail;
        }
        pthread_mutex_init(&e->q.mu, NULL);
        pthread_cond_init(&e->q.not_empty, NULL);
        pthread_cond_init(&e->q.not_full, NULL);
        pthread_mutex_init(&e->send_mu, NULL);
        e->refs = 2; // السجل + خيط القارئ
        pthread_mutex_lock(&nv_std_ipc_reg_mu);
        if (nv_std_ipc_live >= 64) {
            pthread_mutex_unlock(&nv_std_ipc_reg_mu);
            nv_std_ipc_drop_unpublished(e);
            pin[1] = -1;
            pout[0] = -1;
            snprintf(emsg, sizeof emsg, "عدد القنوات المفتوحة تجاوز الحد (٦٤) — أغلق بعضها");
            nv_std_ipc_kill_wait(pid);
            child_started = 0;
            goto fail;
        }
        e->id = nv_std_ipc_next_id++;
        e->next = nv_std_ipc_reg;
        nv_std_ipc_reg = e;
        nv_std_ipc_live++;
        pthread_mutex_unlock(&nv_std_ipc_reg_mu);
        if (pthread_create(&e->reader, NULL, nv_std_ipc_reader, e) != 0) {
            pthread_mutex_lock(&nv_std_ipc_reg_mu);
            struct NvStdIpcEntry **pp = &nv_std_ipc_reg;
            while (*pp && *pp != e) pp = &(*pp)->next;
            if (*pp) {
                *pp = e->next;
                nv_std_ipc_live--;
            }
            pthread_mutex_unlock(&nv_std_ipc_reg_mu);
            nv_std_ipc_drop_unpublished(e);
            pin[1] = -1;
            pout[0] = -1;
            snprintf(emsg, sizeof emsg, "تعذّر فتح قناة للبرنامج");
            nv_std_ipc_kill_wait(pid);
            child_started = 0;
            goto fail;
        }
        pthread_detach(e->reader);
        free(exec_argv);
        free(exec_path);
        free(resolved);
        free(display);
        free(argv);
        free(copy);
        return e;
    }

fail:
    if (child_started) {
        int st = 0;
        pid_t w;
        do {
            w = waitpid(pid, &st, 0);
        } while (w < 0 && errno == EINTR);
    }
    if (pin[0] >= 0) close(pin[0]);
    if (pin[1] >= 0) close(pin[1]);
    if (pout[0] >= 0) close(pout[0]);
    if (pout[1] >= 0) close(pout[1]);
    if (perr[0] >= 0) close(perr[0]);
    if (perr[1] >= 0) close(perr[1]);
    free(exec_argv);
    free(exec_path);
    free(resolved);
    free(display);
    free(argv);
    free(copy);
    nv_error(NV_ERR_RUNTIME, emsg, line, col);
}

static NvValue nv_std_ipc_open(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أنبوب", args, nargs, 1, line, col);
    const char *cmd = nv_std_need_string("تواصل.أنبوب", args[0], line, col);
    struct NvStdIpcEntry *e = nv_std_ipc_spawn(cmd, line, col);
    return nv_num((double)e->id);
}

static NvValue nv_std_ipc_send(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أرسل", args, nargs, 2, line, col);
    long long id = nv_std_need_id("تواصل.أرسل", args, 0, "قناة", line, col);
    const char *text = nv_std_need_string("تواصل.أرسل", args[1], line, col);
    size_t tlen = args[1].u.str->len;
    if (memchr(text, '\n', tlen) != NULL || memchr(text, '\r', tlen) != NULL) {
        nv_error(NV_ERR_RUNTIME, "الرسالة يجب أن تكون سطرا واحدا بلا أسطر جديدة", line, col);
    }
    if (tlen > 64u * 1024u) {
        nv_error(NV_ERR_RUNTIME, "الرسالة أكبر من الحد (64 كيبيبايت)", line, col);
    }
    struct NvStdIpcEntry *e = nv_std_ipc_lookup((unsigned long long)id);
    if (!e) nv_std_ipc_unknown((unsigned long long)id, line, col);
    if (pthread_mutex_lock(&e->send_mu) != 0) {
        nv_std_ipc_unref(e);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر الوصول إلى القناة: %lld", id);
    }
    int fd = e->ch.write_fd;
    int failed = 0;
    int failed_no = 0;
    if (fd < 0) {
        pthread_mutex_unlock(&e->send_mu);
        nv_std_ipc_unref(e);
        nv_std_ipc_unknown((unsigned long long)id, line, col);
    }
    size_t off = 0;
    while (off < tlen) {
        ssize_t w = write(fd, text + off, tlen - off);
        if (w < 0) {
            if (errno == EINTR) continue;
            failed = 1;
            failed_no = errno;
            break;
        }
        off += (size_t)w;
    }
    if (!failed) {
        ssize_t w;
        do {
            w = write(fd, "\n", 1);
        } while (w < 0 && errno == EINTR);
        if (w < 0) {
            failed = 1;
            failed_no = errno;
        }
    }
    pthread_mutex_unlock(&e->send_mu);
    if (failed) {
        nv_std_ipc_unref(e);
        if (failed_no == EPIPE) {
            nv_error(NV_ERR_RUNTIME, "أغلق الطرف الآخر الاتصال", line, col);
        }
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر الإرسال إلى القناة: %lld", id);
    }
    nv_std_ipc_unref(e);
    return nv_num((double)(tlen + 1));
}

static NvValue nv_std_ipc_recv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.استقبل", args, nargs, 1, line, col);
    long long id = nv_std_need_id("تواصل.استقبل", args, 0, "قناة", line, col);
    struct NvStdIpcEntry *e = nv_std_ipc_lookup((unsigned long long)id);
    if (!e) nv_std_ipc_unknown((unsigned long long)id, line, col);
    char *line_text = nv_std_ipc_queue_pop(e, (unsigned long long)id, line, col);
    NvValue v = nv_str_new(line_text, strlen(line_text));
    free(line_text);
    return v;
}

static NvValue nv_std_ipc_close(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أغلق", args, nargs, 1, line, col);
    long long id = nv_std_need_id("تواصل.أغلق", args, 0, "قناة", line, col);
    struct NvStdIpcEntry *e = nv_std_ipc_take((unsigned long long)id);
    if (!e) nv_std_ipc_unknown((unsigned long long)id, line, col);
    if (pthread_mutex_lock(&e->send_mu) == 0) {
        if (e->ch.write_fd >= 0) {
            close(e->ch.write_fd);
            e->ch.write_fd = -1;
        }
        pthread_mutex_unlock(&e->send_mu);
    }
    pthread_mutex_lock(&e->q.mu);
    e->q.abandoned = 1;
    pthread_cond_broadcast(&e->q.not_full);
    pthread_mutex_unlock(&e->q.mu);
    pid_t pid = (pid_t)e->ch.pid;
    long code = -1;
    int done = 0;
    struct timespec start;
    clock_gettime(CLOCK_MONOTONIC, &start);
    for (;;) {
        int status = 0;
        pid_t w = waitpid(pid, &status, WNOHANG);
        if (w == pid) {
            if (WIFEXITED(status)) code = (long)WEXITSTATUS(status);
            else code = -1;
            done = 1;
            break;
        }
        if (w < 0) {
            if (errno == EINTR) continue;
            nv_std_ipc_unref(e);
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر انتظار القناة: %lld", id);
        }
        struct timespec now;
        clock_gettime(CLOCK_MONOTONIC, &now);
        double elapsed = (double)(now.tv_sec - start.tv_sec) +
            (double)(now.tv_nsec - start.tv_nsec) / 1000000000.0;
        if (elapsed >= 30.0) break;
        struct timespec nap = { 0, 50 * 1000 * 1000 };
        nanosleep(&nap, NULL);
    }
    if (!done) {
        kill(pid, SIGKILL);
        int status = 0;
        pid_t w;
        do {
            w = waitpid(pid, &status, 0);
        } while (w < 0 && errno == EINTR);
        code = -1;
    }
    nv_std_ipc_unref(e);
    return nv_num((double)code);
}

static NvValue nv_std_ipc_stop(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أوقف", args, nargs, 1, line, col);
    long long id = nv_std_need_id("تواصل.أوقف", args, 0, "قناة", line, col);
    struct NvStdIpcEntry *e = nv_std_ipc_take((unsigned long long)id);
    if (!e) nv_std_ipc_unknown((unsigned long long)id, line, col);
    if (pthread_mutex_lock(&e->send_mu) == 0) {
        if (e->ch.write_fd >= 0) {
            close(e->ch.write_fd);
            e->ch.write_fd = -1;
        }
        pthread_mutex_unlock(&e->send_mu);
    }
    pthread_mutex_lock(&e->q.mu);
    e->q.abandoned = 1;
    pthread_cond_broadcast(&e->q.not_full);
    pthread_mutex_unlock(&e->q.mu);
    pid_t pid = (pid_t)e->ch.pid;
    int status = 0;
    pid_t w;
    do {
        w = waitpid(pid, &status, WNOHANG);
    } while (w < 0 && errno == EINTR);
    if (w == 0) {
        if (kill(pid, SIGKILL) != 0) {
            int st2 = 0;
            pid_t w2;
            do {
                w2 = waitpid(pid, &st2, WNOHANG);
            } while (w2 < 0 && errno == EINTR);
            nv_std_ipc_unref(e);
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر إيقاف القناة: %lld", id);
        }
        pid_t w3;
        do {
            w3 = waitpid(pid, &status, 0);
        } while (w3 < 0 && errno == EINTR);
        if (w3 < 0) {
            nv_std_ipc_unref(e);
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر إيقاف القناة: %lld", id);
        }
    } else if (w < 0) {
        nv_std_ipc_unref(e);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذّر إيقاف القناة: %lld", id);
    }
    nv_std_ipc_unref(e);
    return nv_nil();
}

#else // _WIN32

// ---------- ويندوز: تواصل مرفوض بالعربي ----------
// اجتماع 35 §50: fork/pipe/waitpid/kill غير موجودة
// في MinGW؛ ففتح القناة وكل عملياتها ترفض بالرسالة
// نفسها المستعملة لباقي الوحدات المقيّدة بالمنصة.
static NvValue nv_std_ipc_open(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أنبوب", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "تواصل.أنبوب غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_ipc_send(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أرسل", args, nargs, 2, line, col);
    nv_error(NV_ERR_RUNTIME, "تواصل.أرسل غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_ipc_recv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.استقبل", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "تواصل.استقبل غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_ipc_close(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أغلق", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "تواصل.أغلق غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

static NvValue nv_std_ipc_stop(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تواصل.أوقف", args, nargs, 1, line, col);
    nv_error(NV_ERR_RUNTIME, "تواصل.أوقف غير مدعوم على هذه المنصة", line, col);
    return nv_nil();
}

#endif // _WIN32

void nv_std_comm_fill(NvDict *d) {
#ifndef _WIN32
    // SIGPIPE غير موجود في MinGW (ويندوز).
    signal(SIGPIPE, SIG_IGN);
#endif
    nv_std_add_fn(d, "أنبوب", nv_std_ipc_open);
    nv_std_add_fn(d, "أرسل", nv_std_ipc_send);
    nv_std_add_fn(d, "استقبل", nv_std_ipc_recv);
    nv_std_add_fn(d, "أغلق", nv_std_ipc_close);
    nv_std_add_fn(d, "أوقف", nv_std_ipc_stop);
}

 // ================= غير_متزامن =================
// كما في المصدر: خيط عامل واحد لكل عملية مقدمة (بلا
// حلقة أحداث وبلا مكتبة خارجية). الحلقة سجل عمليات
// معلقة + قناة نتائج محدودة (64). التقديم يعود فورا
// بمعرف العملية؛ والفشل أثناء العمل يصل قاموس نتيجة
// فيه «خطأ» ولا يُطلق أبدا. وانتظر_النتائج ينتظر 30
// ثانية لأول نتيجة ثم يفرغ الجاهز، وقائمة فارغة عند
// انقضاء المهلة بلا نتائج ليست خطأ.
// نتيجة النجاح قاموس {المعرف، النوع، المحتوى}: نص
// للقراءة والاستقبال، وعدد بايتات للكتابة والإرسال.
// والأنواع حرفيا: قراءة، كتابة، إرسال، استقبال.
// حدود: 64 حلقة حية، و64 عملية معلقة لكل حلقة.
// مسارات الملفات تُحل بسياسة الجلسة عند التقديم،
// فيرفض خارج مجلد البرنامج فورا وبنفس رسالة المصدر.
// عمليات المقابس: البوابة أولا، ثم نسخ المقبس، ثم
// الحجز. ونسخ المقبس يعيش في قسم الشبكة (سجله ساكن
// في وحدة ترجمته، بلا واجهة مشتركة بعد) — فالدالة
// المساعدة أدناه ترفض اليوم برسالة المصدر نفسها
// «مقبس غير معروف: …» لأي معرف؛ وعندما يكشف قسم
// الشبكة واجهة نسخ مشتركة تُستبدل هذه الدالة وحدها
// ويعمل الإرسال والاستقبال بلا أي تغيير آخر (عامِلا
// المقبس مكتوبان وجاهزان ويعملان على واصف منسوخ).
// مهلة المقبس 30 ثانية من نص المصدر: العامل يعتمد
// مهلة المقبس نفسه كما في نسخ المقبس بالمصدر، وخطأ
// المهلة (EAGAIN) يتحول إلى رسالتي المهلة العربيتين.

struct NvStdAsyncResult {
    unsigned long long op;
    const char *kind; // قراءة، كتابة، إرسال، استقبال
    int is_text; // 1 نص، 0 عدد بايتات
    char *text; // مملوك عند النجاح النصي
    size_t text_len;
    unsigned long long count; // عدد البايتات عند النجاح العددي
    char *error; // مملوك عند الفشل، أو NULL
};

struct NvStdAsyncLoopState {
    pthread_mutex_t mu;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
    struct NvStdAsyncResult items[64];
    size_t head;
    size_t tail;
    size_t len;
    size_t pending; // مقدمة ولم تُجمع بعد
};

struct NvStdAsyncLoopEntry {
    unsigned long long id;
    struct NvStdLoop handle; // state يشير إلى الحالة
    struct NvStdAsyncLoopEntry *next;
};

struct NvStdAsyncOpArg {
    unsigned long long op;
    const char *kind;
    struct NvStdAsyncLoopState *loop; // لا يملكه العامل
    char *path; // محلول مطلق مملوك، أو NULL
    char *display; // عرض المسار للمستخدم مملوك، أو NULL
    char *text; // نص الكتابة/الإرسال مملوك، أو NULL
    size_t text_len;
    unsigned long long offset; // موضع القراءة
    int fd; // مقبس منسوخ، أو -1
};

static pthread_mutex_t nv_std_async_reg_mu = PTHREAD_MUTEX_INITIALIZER;
static struct NvStdAsyncLoopEntry *nv_std_async_loops = NULL;
static unsigned long long nv_std_async_next_loop = 1;
static unsigned long long nv_std_async_next_op = 1;
static int nv_std_async_live_loops = 0;
static pthread_mutex_t nv_std_async_write_mu = PTHREAD_MUTEX_INITIALIZER;

static char *nv_std_async_strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *out = (char *)malloc(n);
    memcpy(out, s, n);
    return out;
}

static char *nv_std_async_format(const char *fmt, const char *arg) {
    size_t n = strlen(fmt) + strlen(arg) + 1;
    char *out = (char *)malloc(n);
    snprintf(out, n, fmt, arg);
    return out;
}

// عرض مسار كما يعرضه المحرك (كما في display_str
// بالمصدر): ~ كما هي، والمطلق عبر nv_display_path،
// والنسبي كما هو.
static char *nv_std_async_display_str(const char *s) {
    if (strcmp(s, "~") == 0 || strncmp(s, "~/", 2) == 0) return nv_std_async_strdup(s);
    if (s[0] == '/') return nv_display_path(s);
    return nv_std_async_strdup(s);
}

static unsigned long long nv_std_async_need_loop_id(const char *display, NvValue *args, int at, int line, int col) {
    long long id = nv_std_need_id(display, args, at, "حلقة", line, col);
    return (unsigned long long)id;
}

static unsigned long long nv_std_async_need_socket_id(const char *display, NvValue *args, int at, int line, int col) {
    long long id = nv_std_need_id(display, args, at, "مقبس", line, col);
    return (unsigned long long)id;
}

// فحص UTF-8 صارم (كما في المصدر): يرفض المتتابعات
// الناقصة والزائدة الطول والبديلة وما فوق U+10FFFF.
static int nv_std_async_valid_utf8(const unsigned char *s, size_t n) {
    size_t i = 0;
    while (i < n) {
        unsigned char c = s[i];
        if (c < 0x80) {
            i++;
            continue;
        }
        size_t extra;
        unsigned int cp;
        if ((c & 0xE0) == 0xC0) {
            extra = 1;
            cp = c & 0x1F;
            if (cp < 2) return 0; // C0 وC1 زائدتا الطول
        } else if ((c & 0xF0) == 0xE0) {
            extra = 2;
            cp = c & 0x0F;
        } else if ((c & 0xF8) == 0xF0) {
            extra = 3;
            cp = c & 0x07;
            if (cp > 4) return 0; // فوق U+10FFFF
        } else {
            return 0;
        }
        if (i + extra >= n) return 0;
        for (size_t k = 1; k <= extra; k++) {
            unsigned char cc = s[i + k];
            if ((cc & 0xC0) != 0x80) return 0;
            cp = (cp << 6) | (cc & 0x3F);
        }
        if (extra == 1 && cp < 0x80) return 0;
        if (extra == 2) {
            if (cp < 0x800) return 0;
            if (cp >= 0xD800 && cp <= 0xDFFF) return 0;
        }
        if (extra == 3 && (cp < 0x10000 || cp > 0x10FFFF)) return 0;
        i += extra + 1;
    }
    return 1;
}

// حجم مجلد تكراري بميزانية مدخلات (كما في فحص الحصة
// في وقت التشغيل): 10000 مدخل، ويتتبع الروابط لا
// المجلدات (lstat).
static unsigned long long nv_std_async_dir_size(const char *path, int *budget) {
    if (!path || *budget <= 0) return 0;
    (*budget)--;
    struct stat st;
    if (nv_lstat(path, &st) != 0) return 0;
    if (S_ISDIR(st.st_mode)) {
        unsigned long long total = 0;
        DIR *d = opendir(path);
        if (!d) return 0;
        struct dirent *e;
        while ((e = readdir(d)) != NULL) {
            if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
            size_t n = strlen(path) + 1 + strlen(e->d_name) + 1;
            char *child = (char *)malloc(n);
            snprintf(child, n, "%s/%s", path, e->d_name);
            total += nv_std_async_dir_size(child, budget);
            free(child);
            if (*budget <= 0) break;
        }
        closedir(d);
        return total;
    }
    return (unsigned long long)st.st_size;
}

static void nv_std_async_push_result(struct NvStdAsyncLoopState *st, struct NvStdAsyncResult *r) {
    pthread_mutex_lock(&st->mu);
    while (st->len >= 64) pthread_cond_wait(&st->not_full, &st->mu);
    st->items[st->tail] = *r;
    st->tail = (st->tail + 1) % 64;
    st->len++;
    pthread_cond_signal(&st->not_empty);
    pthread_mutex_unlock(&st->mu);
}

static struct NvStdAsyncLoopState *nv_std_async_find_loop(unsigned long long loop_id, int line, int col) {
    if (pthread_mutex_lock(&nv_std_async_reg_mu) != 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حلقة غير معروفة: %llu", loop_id);
    }
    struct NvStdAsyncLoopEntry *e = nv_std_async_loops;
    while (e && e->id != loop_id) e = e->next;
    if (!e) {
        pthread_mutex_unlock(&nv_std_async_reg_mu);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حلقة غير معروفة: %llu", loop_id);
    }
    struct NvStdAsyncLoopState *st = (struct NvStdAsyncLoopState *)e->handle.state;
    pthread_mutex_unlock(&nv_std_async_reg_mu);
    return st;
}

// احجز خانة معلقة واحدة وسلم معرف العملية. الحجز
// والحساب تحت القفلين معا فلا يتجاوز السقف أبدا.
static struct NvStdAsyncLoopState *nv_std_async_reserve(unsigned long long loop_id, unsigned long long *op_out, int line, int col) {
    pthread_mutex_lock(&nv_std_async_reg_mu);
    struct NvStdAsyncLoopEntry *e = nv_std_async_loops;
    while (e && e->id != loop_id) e = e->next;
    if (!e) {
        pthread_mutex_unlock(&nv_std_async_reg_mu);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حلقة غير معروفة: %llu", loop_id);
    }
    struct NvStdAsyncLoopState *st = (struct NvStdAsyncLoopState *)e->handle.state;
    pthread_mutex_lock(&st->mu);
    if (st->pending >= 64) {
        pthread_mutex_unlock(&st->mu);
        pthread_mutex_unlock(&nv_std_async_reg_mu);
        nv_error(NV_ERR_RUNTIME, "الحلقة ممتلئة (64 عملية معلقة)", line, col);
    }
    st->pending++;
    *op_out = nv_std_async_next_op++;
    pthread_mutex_unlock(&st->mu);
    pthread_mutex_unlock(&nv_std_async_reg_mu);
    return st;
}

static void nv_std_async_release(unsigned long long loop_id) {
    pthread_mutex_lock(&nv_std_async_reg_mu);
    struct NvStdAsyncLoopEntry *e = nv_std_async_loops;
    while (e && e->id != loop_id) e = e->next;
    if (e) {
        struct NvStdAsyncLoopState *st = (struct NvStdAsyncLoopState *)e->handle.state;
        pthread_mutex_lock(&st->mu);
        if (st->pending > 0) st->pending--;
        pthread_mutex_unlock(&st->mu);
    }
    pthread_mutex_unlock(&nv_std_async_reg_mu);
}

static void nv_std_async_spawn_or_release(unsigned long long loop_id, struct NvStdAsyncOpArg *arg, void *(*worker)(void *), int line, int col) {
    pthread_t th;
    if (pthread_create(&th, NULL, worker, arg) != 0) {
        free(arg->path);
        free(arg->display);
        free(arg->text);
        if (arg->fd >= 0) close(arg->fd);
        free(arg);
        nv_std_async_release(loop_id);
        nv_error(NV_ERR_RUNTIME, "تعذّر بدء العملية غير المتزامنة", line, col);
    }
    pthread_detach(th);
}

static void *nv_std_async_read_worker(void *raw) {
    struct NvStdAsyncOpArg *arg = (struct NvStdAsyncOpArg *)raw;
    struct NvStdAsyncResult r;
    r.op = arg->op;
    r.kind = arg->kind;
    r.is_text = 0;
    r.text = NULL;
    r.text_len = 0;
    r.count = 0;
    r.error = NULL;
    FILE *f = fopen(arg->path, "rb");
    if (!f) {
        r.error = nv_std_async_format("تعذر قراءة الملف %s", arg->display);
    } else if (fseeko(f, (off_t)arg->offset, SEEK_SET) != 0) {
        fclose(f);
        r.error = nv_std_async_format("تعذر قراءة الملف %s", arg->display);
    } else {
        size_t cap = 5u * 1024u * 1024u + 1u;
        unsigned char *buf = (unsigned char *)malloc(cap);
        size_t n = 0;
        int bad = 0;
        if (!buf) {
            bad = 1;
        } else {
            while (n < cap) {
                size_t want = cap - n;
                size_t got = fread(buf + n, 1, want, f);
                n += got;
                if (got < want) {
                    if (ferror(f)) bad = 1;
                    break;
                }
            }
        }
        fclose(f);
        if (bad) {
            free(buf);
            r.error = nv_std_async_format("تعذر قراءة الملف %s", arg->display);
        } else if (n > 5u * 1024u * 1024u) {
            free(buf);
            r.error = nv_std_async_format("الملف %s أكبر من الحد الأقصى (5 ميغابايت)", arg->display);
        } else if (!nv_std_async_valid_utf8(buf, n)) {
            free(buf);
            r.error = nv_std_async_format("الملف %s ليس نصا صالحا (UTF-8)", arg->display);
        } else {
            r.is_text = 1;
            r.text = (char *)buf;
            r.text_len = n;
        }
    }
    nv_std_async_push_result(arg->loop, &r);
    free(arg->path);
    free(arg->display);
    free(arg->text);
    free(arg);
    return NULL;
}

static void *nv_std_async_write_worker(void *raw) {
    struct NvStdAsyncOpArg *arg = (struct NvStdAsyncOpArg *)raw;
    struct NvStdAsyncResult r;
    r.op = arg->op;
    r.kind = arg->kind;
    r.is_text = 0;
    r.text = NULL;
    r.text_len = 0;
    r.count = 0;
    r.error = NULL;
    struct stat st;
    unsigned long long existing = 0;
    if (stat(arg->path, &st) == 0) existing = (unsigned long long)st.st_size;
    unsigned long long extra = arg->text_len > (size_t)existing
        ? (unsigned long long)(arg->text_len - (size_t)existing) : 0;
    pthread_mutex_lock(&nv_std_async_write_mu);
    int budget = 10000;
    unsigned long long used = nv_std_async_dir_size(nv_program_dir, &budget);
    if (used + extra > 100ull * 1024ull * 1024ull) {
        pthread_mutex_unlock(&nv_std_async_write_mu);
        r.error = nv_std_async_strdup("تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة");
    } else {
        FILE *f = fopen(arg->path, "wb");
        if (!f) {
            pthread_mutex_unlock(&nv_std_async_write_mu);
            r.error = nv_std_async_format("تعذر كتابة الملف %s", arg->display);
        } else {
            int bad = 0;
            size_t off = 0;
            while (off < arg->text_len) {
                size_t w = fwrite(arg->text + off, 1, arg->text_len - off, f);
                if (w == 0) {
                    bad = 1;
                    break;
                }
                off += w;
            }
            if (fclose(f) != 0) bad = 1;
            pthread_mutex_unlock(&nv_std_async_write_mu);
            if (bad) {
                r.error = nv_std_async_format("تعذر كتابة الملف %s", arg->display);
            } else {
                r.count = (unsigned long long)arg->text_len;
            }
        }
    }
    nv_std_async_push_result(arg->loop, &r);
    free(arg->path);
    free(arg->display);
    free(arg->text);
    free(arg);
    return NULL;
}

static void *nv_std_async_send_worker(void *raw) {
    struct NvStdAsyncOpArg *arg = (struct NvStdAsyncOpArg *)raw;
    struct NvStdAsyncResult r;
    r.op = arg->op;
    r.kind = arg->kind;
    r.is_text = 0;
    r.text = NULL;
    r.text_len = 0;
    r.count = 0;
    r.error = NULL;
    int fd = arg->fd;
    size_t off = 0;
    int bad = 0;
    int bad_no = 0;
    while (off < arg->text_len) {
        ssize_t w = write(fd, arg->text + off, arg->text_len - off);
        if (w < 0) {
            if (errno == EINTR) continue;
            bad = 1;
            bad_no = errno;
            break;
        }
        off += (size_t)w;
    }
    close(fd);
    if (bad) {
        if (bad_no == EAGAIN || bad_no == EWOULDBLOCK) {
            r.error = nv_std_async_strdup("انتهت مهلة الإرسال (30 ثانية)");
        } else {
            r.error = nv_std_async_strdup("تعذر الإرسال على المقبس");
        }
    } else {
        r.count = (unsigned long long)arg->text_len;
    }
    nv_std_async_push_result(arg->loop, &r);
    free(arg->path);
    free(arg->display);
    free(arg->text);
    free(arg);
    return NULL;
}

static void *nv_std_async_recv_worker(void *raw) {
    struct NvStdAsyncOpArg *arg = (struct NvStdAsyncOpArg *)raw;
    struct NvStdAsyncResult r;
    r.op = arg->op;
    r.kind = arg->kind;
    r.is_text = 0;
    r.text = NULL;
    r.text_len = 0;
    r.count = 0;
    r.error = NULL;
    int fd = arg->fd;
    unsigned char *buf = (unsigned char *)malloc(64u * 1024u);
    ssize_t n;
    do {
        n = read(fd, buf, 64u * 1024u);
    } while (n < 0 && errno == EINTR);
    close(fd);
    if (n < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            r.error = nv_std_async_strdup("انتهت مهلة الاستقبال (30 ثانية)");
        } else {
            r.error = nv_std_async_strdup("تعذر الاستقبال من المقبس");
        }
        free(buf);
    } else if (n == 0) {
        r.error = nv_std_async_strdup("أغلق الطرف الآخر الاتصال");
        free(buf);
    } else if (!nv_std_async_valid_utf8(buf, (size_t)n)) {
        r.error = nv_std_async_strdup("البيانات المستقبلة ليست نصا صالحا (UTF-8)");
        free(buf);
    } else {
        r.is_text = 1;
        r.text = (char *)buf;
        r.text_len = (size_t)n;
    }
    nv_std_async_push_result(arg->loop, &r);
    free(arg->path);
    free(arg->display);
    free(arg->text);
    free(arg);
    return NULL;
}

// انسخ مقابس شبكة.اتصل حيا: الواصف المنسوخ يخص
// العامل وحده فيغلقه عند انتهائه. انظر المقدمة:
// السجل في قسم الشبكة بلا واجهة مشتركة بعد.
static int nv_std_async_clone_socket(unsigned long long id, int line, int col) {
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "مقبس غير معروف: %llu", id);
    return -1;
}

static struct NvStdAsyncOpArg *nv_std_async_new_op(unsigned long long op, const char *kind, struct NvStdAsyncLoopState *st, unsigned long long loop_id, int line, int col) {
    struct NvStdAsyncOpArg *arg = (struct NvStdAsyncOpArg *)calloc(1, sizeof *arg);
    if (!arg) {
        nv_std_async_release(loop_id);
        nv_error(NV_ERR_RUNTIME, "تعذّر بدء العملية غير المتزامنة", line, col);
    }
    arg->op = op;
    arg->kind = kind;
    arg->loop = st;
    arg->fd = -1;
    return arg;
}

static NvValue nv_std_async_create_loop(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.أنشئ_حلقة", args, nargs, 0, line, col);
    if (pthread_mutex_lock(&nv_std_async_reg_mu) != 0) {
        nv_error(NV_ERR_RUNTIME, "تعذّر إنشاء الحلقة", line, col);
    }
    if (nv_std_async_live_loops >= 64) {
        pthread_mutex_unlock(&nv_std_async_reg_mu);
        nv_error(NV_ERR_RUNTIME, "بلغ عدد الحلقات الحد الأقصى (64 حلقة)", line, col);
    }
    struct NvStdAsyncLoopState *st = (struct NvStdAsyncLoopState *)calloc(1, sizeof *st);
    struct NvStdAsyncLoopEntry *e = (struct NvStdAsyncLoopEntry *)malloc(sizeof *e);
    if (!st || !e) {
        free(st);
        free(e);
        pthread_mutex_unlock(&nv_std_async_reg_mu);
        nv_error(NV_ERR_RUNTIME, "تعذّر إنشاء الحلقة", line, col);
    }
    pthread_mutex_init(&st->mu, NULL);
    pthread_cond_init(&st->not_empty, NULL);
    pthread_cond_init(&st->not_full, NULL);
    e->id = nv_std_async_next_loop++;
    e->handle.state = st;
    e->next = nv_std_async_loops;
    nv_std_async_loops = e;
    nv_std_async_live_loops++;
    unsigned long long id = e->id;
    pthread_mutex_unlock(&nv_std_async_reg_mu);
    return nv_num((double)id);
}

static NvValue nv_std_async_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.اقرأ", args, nargs, 3, line, col);
    long long loop_id = (long long)nv_std_async_need_loop_id("غير_متزامن.اقرأ", args, 0, line, col);
    const char *path = nv_std_need_string("غير_متزامن.اقرأ", args[1], line, col);
    long long offset = nv_std_need_integer("غير_متزامن.اقرأ", args[2], line, col);
    if (offset < 0) {
        nv_error(NV_ERR_TYPE, "الدالة غير_متزامن.اقرأ تتوقع موضعا غير سالب", line, col);
    }
    char *resolved = nv_resolve_path(path, line, col);
    char *display = nv_std_async_display_str(path);
    unsigned long long op = 0;
    struct NvStdAsyncLoopState *st = nv_std_async_reserve((unsigned long long)loop_id, &op, line, col);
    struct NvStdAsyncOpArg *arg = nv_std_async_new_op(op, "قراءة", st, (unsigned long long)loop_id, line, col);
    arg->path = resolved;
    arg->display = display;
    arg->offset = (unsigned long long)offset;
    nv_std_async_spawn_or_release((unsigned long long)loop_id, arg, nv_std_async_read_worker, line, col);
    return nv_num((double)op);
}

static NvValue nv_std_async_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.اكتب", args, nargs, 3, line, col);
    long long loop_id = (long long)nv_std_async_need_loop_id("غير_متزامن.اكتب", args, 0, line, col);
    const char *path = nv_std_need_string("غير_متزامن.اكتب", args[1], line, col);
    const char *text = nv_std_need_string("غير_متزامن.اكتب", args[2], line, col);
    size_t tlen = args[2].u.str->len;
    char *resolved = nv_resolve_path(path, line, col);
    char *display = nv_std_async_display_str(path);
    unsigned long long op = 0;
    struct NvStdAsyncLoopState *st = nv_std_async_reserve((unsigned long long)loop_id, &op, line, col);
    struct NvStdAsyncOpArg *arg = nv_std_async_new_op(op, "كتابة", st, (unsigned long long)loop_id, line, col);
    arg->path = resolved;
    arg->display = display;
    arg->text = (char *)malloc(tlen ? tlen : 1);
    memcpy(arg->text, text, tlen);
    arg->text_len = tlen;
    nv_std_async_spawn_or_release((unsigned long long)loop_id, arg, nv_std_async_write_worker, line, col);
    return nv_num((double)op);
}

static NvValue nv_std_async_send(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.أرسل", args, nargs, 3, line, col);
    long long loop_id = (long long)nv_std_async_need_loop_id("غير_متزامن.أرسل", args, 0, line, col);
    long long socket_id = (long long)nv_std_async_need_socket_id("غير_متزامن.أرسل", args, 1, line, col);
    const char *text = nv_std_need_string("غير_متزامن.أرسل", args[2], line, col);
    size_t tlen = args[2].u.str->len;
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
    int fd = nv_std_async_clone_socket((unsigned long long)socket_id, line, col);
    unsigned long long op = 0;
    struct NvStdAsyncLoopState *st = nv_std_async_reserve((unsigned long long)loop_id, &op, line, col);
    struct NvStdAsyncOpArg *arg = nv_std_async_new_op(op, "إرسال", st, (unsigned long long)loop_id, line, col);
    arg->fd = fd;
    arg->text = (char *)malloc(tlen ? tlen : 1);
    memcpy(arg->text, text, tlen);
    arg->text_len = tlen;
    nv_std_async_spawn_or_release((unsigned long long)loop_id, arg, nv_std_async_send_worker, line, col);
    return nv_num((double)op);
}

static NvValue nv_std_async_recv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.استقبل", args, nargs, 2, line, col);
    long long loop_id = (long long)nv_std_async_need_loop_id("غير_متزامن.استقبل", args, 0, line, col);
    long long socket_id = (long long)nv_std_async_need_socket_id("غير_متزامن.استقبل", args, 1, line, col);
    if (!nv_network_allowed) {
        nv_error(NV_ERR_RUNTIME, "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول", line, col);
    }
    int fd = nv_std_async_clone_socket((unsigned long long)socket_id, line, col);
    unsigned long long op = 0;
    struct NvStdAsyncLoopState *st = nv_std_async_reserve((unsigned long long)loop_id, &op, line, col);
    struct NvStdAsyncOpArg *arg = nv_std_async_new_op(op, "استقبال", st, (unsigned long long)loop_id, line, col);
    arg->fd = fd;
    nv_std_async_spawn_or_release((unsigned long long)loop_id, arg, nv_std_async_recv_worker, line, col);
    return nv_num((double)op);
}

static NvValue nv_std_async_wait(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("غير_متزامن.انتظر_النتائج", args, nargs, 1, line, col);
    long long loop_id = (long long)nv_std_async_need_loop_id("غير_متزامن.انتظر_النتائج", args, 0, line, col);
    struct NvStdAsyncLoopState *st = nv_std_async_find_loop((unsigned long long)loop_id, line, col);
    struct NvStdAsyncResult collected[64];
    size_t n = 0;
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += 30;
    pthread_mutex_lock(&st->mu);
    for (;;) {
        if (st->len > 0) break;
        int rc = pthread_cond_timedwait(&st->not_empty, &st->mu, &deadline);
        if (rc == ETIMEDOUT && st->len == 0) break;
    }
    while (st->len > 0 && n < 64) {
        collected[n++] = st->items[st->head];
        st->head = (st->head + 1) % 64;
        st->len--;
    }
    if (n > 0) {
        st->pending = st->pending > n ? st->pending - n : 0;
        pthread_cond_broadcast(&st->not_full);
    }
    pthread_mutex_unlock(&st->mu);
    // جذّر القائمة وكل قاموس نتيجة أثناء ملئه:
    // nv_dict_new وnv_str_new قد يطلقان الجامعة.
    size_t _ro = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(_ro) = out;
    for (size_t i = 0; i < n; i++) {
        size_t _rd = nv_root_push_idx();
        NvValue dv = nv_dict_new();
        *nv_root_slot(_rd) = dv;
        nv_dict_set_raw(dv.u.dict, "المعرف", nv_num((double)collected[i].op));
        nv_dict_set_raw(dv.u.dict, "النوع", nv_str_cstr(collected[i].kind));
        if (collected[i].error) {
            nv_dict_set_raw(dv.u.dict, "خطأ", nv_str_cstr(collected[i].error));
            free(collected[i].error);
        } else if (collected[i].is_text) {
            nv_dict_set_raw(dv.u.dict, "المحتوى", nv_str_new(collected[i].text, collected[i].text_len));
            free(collected[i].text);
        } else {
            nv_dict_set_raw(dv.u.dict, "المحتوى", nv_num((double)collected[i].count));
        }
        nv_list_push_raw(out.u.list, dv);
        nv_root_pop_n(1);
    }
    nv_root_pop_n(1);
    return out;
}

void nv_std_async_fill(NvDict *d) {
#ifndef _WIN32
    // SIGPIPE غير موجود في MinGW (ويندوز).
    signal(SIGPIPE, SIG_IGN);
#endif
    nv_std_add_fn(d, "أنشئ_حلقة", nv_std_async_create_loop);
    nv_std_add_fn(d, "اقرأ", nv_std_async_read);
    nv_std_add_fn(d, "اكتب", nv_std_async_write);
    nv_std_add_fn(d, "أرسل", nv_std_async_send);
    nv_std_add_fn(d, "استقبل", nv_std_async_recv);
    nv_std_add_fn(d, "انتظر_النتائج", nv_std_async_wait);
}

 // ================= جهاز =================
// قدرات أندرويد فقط في المصدر (حوار، تنبيه، نطق،
 // صوت، حساس، استماع، بطارية). التحقق من الوسائط
// خالص ويعمل على كل المنصات، فيجري أولا بترتيبه
// ورسائله حرفيا؛ وبعده بوابة المنصة: على لينكس
// (ليست أندرويد) ترفض كل دالة برسالتها الخاصة
// «جهاز.… غير مدعوم على هذه المنصة». لا يوجد هنا
// أي وصول إلى جهاز ولا أي رسالة مخترعة.

static void nv_std_dev_arity(const char *display, int nargs, int expected, int line, int col) {
    if (nargs != expected) {
        if (expected == 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 1 معامل لكن أعطيت %d", display, nargs);
        } else {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع %d معاملات لكن أعطيت %d", display, expected, nargs);
        }
    }
}

static const char *nv_std_dev_need_str(const char *display, NvValue v, const char *what, int line, int col) {
    if (v.tag != NV_STR) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s %s لكن أعطيت %s", display, what, nv_type_name(v));
    }
    return v.u.str->data;
}

static void nv_std_dev_refuse(const char *name, int line, int col) {
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "جهاز.%s غير مدعوم على هذه المنصة", name);
}

static NvValue nv_std_dev_dialog(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.حوار";
    if (nargs < 2 || nargs > 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 2 أو 3 معاملات لكن أعطيت %d", display, nargs);
    }
    const char *kind = nv_std_dev_need_str(display, args[0], "تتوقع نصا لنوع الحوار", line, col);
    const char *title = nv_std_dev_need_str(display, args[1], "تتوقع نصا لعنوان الحوار", line, col);
    if (title[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "عنوان الحوار فارغ", line, col);
    }
    if (strcmp(kind, "تأكيد") == 0 || strcmp(kind, "نص") == 0 || strcmp(kind, "رقم") == 0 ||
        strcmp(kind, "تاريخ") == 0 || strcmp(kind, "وقت") == 0) {
        if (nargs != 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 2 معاملات لنوع %s لكن أعطيت %d", display, kind, nargs);
        }
    } else if (strcmp(kind, "صوتي") == 0) {
        nv_error(NV_ERR_RUNTIME, "الحوار الصوتي غير متاح بعد — يحتاج إذن ميكروفون وتعرفا صوتيا", line, col);
    } else if (strcmp(kind, "اختيار واحد") == 0 || strcmp(kind, "اختيارات") == 0 || strcmp(kind, "قائمة") == 0) {
        if (nargs != 3) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 3 معاملات لنوع %s لكن أعطيت %d", display, kind, nargs);
        }
        if (args[2].tag != NV_LIST) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة خيارات لكن أعطيت %s", display, nv_type_name(args[2]));
        }
        NvList *opts = args[2].u.list;
        if (opts->len == 0) {
            nv_error(NV_ERR_RUNTIME, "قائمة الخيارات فارغة", line, col);
        }
        for (size_t i = 0; i < opts->len; i++) {
            if (opts->items[i].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصوصا في قائمة الخيارات لكن أعطيت %s", display, nv_type_name(opts->items[i]));
            }
        }
    } else if (strcmp(kind, "عداد") == 0) {
        if (nargs != 3) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع 3 معاملات لنوع عداد لكن أعطيت %d", display, nargs);
        }
        if (args[2].tag != NV_DICT) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموس حدود للعداد لكن أعطيت %s", display, nv_type_name(args[2]));
        }
        NvDict *d = args[2].u.dict;
        int have_min = 0;
        int have_max = 0;
        int have_def = 0;
        long long min_v = 0;
        long long max_v = 0;
        long long def_v = 0;
        for (size_t i = 0; i < d->len; i++) {
            NvValue v = d->entries[i].val;
            if (v.tag != NV_NUM || trunc(v.u.num) != v.u.num || fabs(v.u.num) >= 9.0e15) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا لقيمة «%s» في قاموس العداد", display, d->entries[i].key);
            }
            long long n = (long long)v.u.num;
            const char *key = d->entries[i].key;
            if (strcmp(key, "أدنى") == 0) {
                min_v = n;
                have_min = 1;
            } else if (strcmp(key, "أقصى") == 0) {
                max_v = n;
                have_max = 1;
            } else if (strcmp(key, "افتراضي") == 0) {
                def_v = n;
                have_def = 1;
            } else {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "مفتاح غير معروف في قاموس العداد: %s", key);
            }
        }
        if (!have_min) nv_error(NV_ERR_RUNTIME, "قاموس العداد يفتقد «أدنى»", line, col);
        if (!have_max) nv_error(NV_ERR_RUNTIME, "قاموس العداد يفتقد «أقصى»", line, col);
        if (min_v > max_v) nv_error(NV_ERR_RUNTIME, "الحد الأدنى أكبر من الحد الأقصى", line, col);
        if (have_def && (def_v < min_v || def_v > max_v)) nv_error(NV_ERR_RUNTIME, "القيمة الافتراضية خارج النطاق", line, col);
    } else {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "نوع حوار غير معروف: %s", kind);
    }
    nv_std_dev_refuse("حوار", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_notify(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.نبه";
    nv_std_dev_arity(display, nargs, 2, line, col);
    const char *title = nv_std_dev_need_str(display, args[0], "تتوقع نصا لعنوان التنبيه", line, col);
    const char *text = nv_std_dev_need_str(display, args[1], "تتوقع نصا لنص التنبيه", line, col);
    if (title[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "عنوان التنبيه فارغ", line, col);
    }
    if (text[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "نص التنبيه فارغ", line, col);
    }
    nv_std_dev_refuse("نبه", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_remove_notify(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.أزل_تنبيه";
    nv_std_dev_arity(display, nargs, 1, line, col);
    NvValue v = args[0];
    if (v.tag == NV_NUM) {
        if (trunc(v.u.num) != v.u.num || v.u.num < 1.0 || fabs(v.u.num) >= 9.0e15) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا موجبا لمعرف التنبيه", display);
        }
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا لمعرف التنبيه لكن أعطيت %s", display, nv_type_name(v));
    }
    nv_std_dev_refuse("أزل_تنبيه", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_speak(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.انطق";
    nv_std_dev_arity(display, nargs, 1, line, col);
    const char *text = nv_std_dev_need_str(display, args[0], "تتوقع نصا لنص النطق", line, col);
    if (text[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "نص النطق فارغ", line, col);
    }
    nv_std_dev_refuse("انطق", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_list_engines(NvValue *args, int nargs, int line, int col) {
    nv_std_dev_arity("جهاز.محركات_النطق", nargs, 0, line, col);
    (void)args;
    nv_std_dev_refuse("محركات_النطق", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_play_audio(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.شغل_صوت";
    nv_std_dev_arity(display, nargs, 1, line, col);
    const char *path = nv_std_dev_need_str(display, args[0], "تتوقع نصا لمسار الصوت", line, col);
    if (path[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "مسار الصوت فارغ", line, col);
    }
    nv_std_dev_refuse("شغل_صوت", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_stop_audio(NvValue *args, int nargs, int line, int col) {
    nv_std_dev_arity("جهاز.أوقف_صوت", nargs, 0, line, col);
    (void)args;
    nv_std_dev_refuse("أوقف_صوت", line, col);
    return nv_nil();
}

static const char *nv_std_dev_sensor_names[8] = {
    "تسارع", "جيروسكوب", "ضوء", "قرب", "مغناطيسي", "ضغط", "رطوبة", "حرارة",
};

static NvValue nv_std_dev_sensor(NvValue *args, int nargs, int line, int col) {
    const char *display = "جهاز.حساس";
    nv_std_dev_arity(display, nargs, 1, line, col);
    const char *name = nv_std_dev_need_str(display, args[0], "تتوقع نصا لاسم الحساس", line, col);
    if (name[0] == '\0') {
        nv_error(NV_ERR_RUNTIME, "اسم الحساس فارغ", line, col);
    }
    int known = 0;
    for (int i = 0; i < 8; i++) {
        if (strcmp(name, nv_std_dev_sensor_names[i]) == 0) {
            known = 1;
            break;
        }
    }
    if (!known) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حساس غير معروف: %s", name);
    }
    nv_std_dev_refuse("حساس", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_listen(NvValue *args, int nargs, int line, int col) {
    nv_std_dev_arity("جهاز.اسمع", nargs, 0, line, col);
    (void)args;
    nv_std_dev_refuse("اسمع", line, col);
    return nv_nil();
}

static NvValue nv_std_dev_battery(NvValue *args, int nargs, int line, int col) {
    nv_std_dev_arity("جهاز.بطارية", nargs, 0, line, col);
    (void)args;
    nv_std_dev_refuse("بطارية", line, col);
    return nv_nil();
}

void nv_std_device_fill(NvDict *d) {
    nv_std_add_fn(d, "حوار", nv_std_dev_dialog);
    nv_std_add_fn(d, "نبه", nv_std_dev_notify);
    nv_std_add_fn(d, "أزل_تنبيه", nv_std_dev_remove_notify);
    nv_std_add_fn(d, "انطق", nv_std_dev_speak);
    nv_std_add_fn(d, "محركات_النطق", nv_std_dev_list_engines);
    nv_std_add_fn(d, "شغل_صوت", nv_std_dev_play_audio);
    nv_std_add_fn(d, "أوقف_صوت", nv_std_dev_stop_audio);
    nv_std_add_fn(d, "حساس", nv_std_dev_sensor);
    nv_std_add_fn(d, "اسمع", nv_std_dev_listen);
    nv_std_add_fn(d, "بطارية", nv_std_dev_battery);
}
