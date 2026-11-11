// مكتبة الوحدات الأصلية — خلفية C (مهمة 29).
// 37 وحدة: رياضيات، عشوائي، وقت، شبكة، عمليات، ذاكرة،
// ملفات، نظام، تواصل، تشفير، أمن، غير_متزامن، جهاز،
// نمط، بحث، جسون، اكسمل، يامال، قوالب، تحقق،
// نص، أسس، وحدات، أرشيف، كلمة_مرور، وهمي، تقويم، ألوان، إيموجي،
// قوائم، علمي، تمويل، اكتواري، إحصاء، توزيعات، دوال، تنفيذ.
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
#ifdef __APPLE__
#include <mach-o/dyld.h> // _NSGetExecutablePath — التضمين
// في niva_rt.c وحده لا يصل هذه الوحدة (ترجمة مستقلة).
#endif

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
void nv_std_regex_fill(NvDict *d);
void nv_std_search_fill(NvDict *d);
void nv_std_json_fill(NvDict *d);
void nv_std_xml_fill(NvDict *d);
void nv_std_yaml_fill(NvDict *d);
void nv_std_template_fill(NvDict *d);
void nv_std_validate_fill(NvDict *d);
void nv_std_text_fill(NvDict *d);
void nv_std_bases_fill(NvDict *d);
void nv_std_units_fill(NvDict *d);
void nv_std_archive_fill(NvDict *d);
void nv_std_password_fill(NvDict *d);
void nv_std_fake_fill(NvDict *d);
void nv_std_calendar_fill(NvDict *d);
void nv_std_colors_fill(NvDict *d);
void nv_std_emoji_fill(NvDict *d);
void nv_std_lists_fill(NvDict *d);
void nv_std_science_fill(NvDict *d);
void nv_std_finance_fill(NvDict *d);
void nv_std_actuarial_fill(NvDict *d);
void nv_std_stats_fill(NvDict *d);
void nv_std_distrib_fill(NvDict *d);
void nv_std_funcs_fill(NvDict *d);
void nv_std_exec_fill(NvDict *d);
// J3: إعلانات مسبقة — تعريف هذه الدوال في آخر الملف،
// لكن تعبئة قوائم ونص (أسبق في الملف) تسجلها.
static NvValue nv_std_lists_table(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_group_by(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_prefixes(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_suffixes(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_cumulative(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_windows(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_windows_outside(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_sort(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_sort_desc(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_chunk(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_lists_split_sep(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_text_split_words_all(NvValue *args, int nargs, int line, int col);

// ---------- السجل ----------
// وحدة أصلية بالاسم أو NULL (مؤشر إلى قيمة ساكنة مخزنة).
static NvValue nv_std_modules[37];
static const char *nv_std_names[37] = {
    "رياضيات", "عشوائي", "وقت", "شبكة", "عمليات", "ذاكرة",
    "ملفات", "نظام", "تواصل", "تشفير", "أمن", "غير_متزامن", "جهاز",
    "نمط", "جسون", "اكسمل", "يامال", "قوالب", "تحقق",
    "نص", "أسس", "وحدات", "أرشيف", "كلمة_مرور", "وهمي", "تقويم", "ألوان", "إيموجي",
    "قوائم", "علمي", "تمويل", "اكتواري", "إحصاء", "توزيعات",
    "دوال", "تنفيذ", "بحث",
};
static int nv_std_built = 0;

static void nv_std_build(void) {
    if (nv_std_built) return;
    nv_std_built = 1;
    nv_handle_set_finalizer(nv_std_handle_finalize);
    for (int i = 0; i < 37; i++) {
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
            case 13: nv_std_regex_fill(exports); break;
            case 14: nv_std_json_fill(exports); break;
            case 15: nv_std_xml_fill(exports); break;
            case 16: nv_std_yaml_fill(exports); break;
            case 17: nv_std_template_fill(exports); break;
            case 18: nv_std_validate_fill(exports); break;
            case 19: nv_std_text_fill(exports); break;
            case 20: nv_std_bases_fill(exports); break;
            case 21: nv_std_units_fill(exports); break;
            case 22: nv_std_archive_fill(exports); break;
            case 23: nv_std_password_fill(exports); break;
            case 24: nv_std_fake_fill(exports); break;
            case 25: nv_std_calendar_fill(exports); break;
            case 26: nv_std_colors_fill(exports); break;
            case 27: nv_std_emoji_fill(exports); break;
            case 28: nv_std_lists_fill(exports); break;
            case 29: nv_std_science_fill(exports); break;
            case 30: nv_std_finance_fill(exports); break;
            case 31: nv_std_actuarial_fill(exports); break;
            case 32: nv_std_stats_fill(exports); break;
            case 33: nv_std_distrib_fill(exports); break;
            case 34: nv_std_funcs_fill(exports); break;
            case 35: nv_std_exec_fill(exports); break;
            case 36: nv_std_search_fill(exports); break;
        }
        nv_add_global_root(nv_std_modules[i]);
    }
}

NvValue *nv_native_module(const char *name) {
    nv_std_build();
    for (int i = 0; i < 37; i++) {
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

// --- J1: الأفعال الحسابية والمنطقية (البنود 2–11 و14 و19 و20 و22) ---

static NvValue nv_std_math_nth_root(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جذر_نوني", args, nargs, 2, line, col);
    double x = nv_std_need_number("رياضيات.جذر_نوني", args[0], line, col);
    long long n = nv_std_need_integer("رياضيات.جذر_نوني", args[1], line, col);
    if (n == 0) {
        nv_error(NV_ERR_RUNTIME, "جذر من الدرجة صفر غير معرف", line, col);
    }
    if (x < 0.0 && n % 2 == 0) {
        nv_error(NV_ERR_RUNTIME, "جذر من درجة زوجية لعدد سالب غير معرف", line, col);
    }
    double inv = 1.0 / (double)n;
    if (x < 0.0) {
        return nv_num(-pow(-x, inv));
    }
    return nv_num(pow(x, inv));
}

static NvValue nv_std_math_rem(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.باقي", args, nargs, 2, line, col);
    double a = nv_std_need_number("رياضيات.باقي", args[0], line, col);
    double b = nv_std_need_number("رياضيات.باقي", args[1], line, col);
    if (b == 0.0) {
        nv_error(NV_ERR_RUNTIME, "باقي القسمة على صفر غير معرف", line, col);
    }
    return nv_num(fmod(a, b));
}

static NvValue nv_std_math_factorial(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.عاملي", args, nargs, 1, line, col);
    long long n = nv_std_need_integer("رياضيات.عاملي", args[0], line, col);
    if (n < 0) {
        nv_error(NV_ERR_RUNTIME, "عاملي عدد سالب غير معرف", line, col);
    }
    if (n > 170) {
        nv_error(NV_ERR_RUNTIME, "عاملي عدد أكبر من 170 غير معرف", line, col);
    }
    double result = 1.0;
    for (long long i = 2; i <= n; i++) {
        result *= (double)i;
    }
    return nv_num(result);
}

static NvValue nv_std_math_choose(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.توافيق", args, nargs, 2, line, col);
    long long n = nv_std_need_integer("رياضيات.توافيق", args[0], line, col);
    long long k = nv_std_need_integer("رياضيات.توافيق", args[1], line, col);
    if (n < 0 || k < 0 || k > n) {
        nv_error(NV_ERR_RUNTIME, "توافيق تتطلب عددين صحيحين حيث 0 ≤ ك ≤ ن", line, col);
    }
    if (k > n - k) k = n - k;
    if (k > 1000) {
        nv_error(NV_ERR_RUNTIME, "توافيق بعدد كبير غير مدعومة", line, col);
    }
    double result = 1.0;
    for (long long i = 1; i <= k; i++) {
        result = result * (double)(n - k + i) / (double)i;
    }
    return nv_num(result);
}

static NvValue nv_std_math_min2(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.اصغر", args, nargs, 2, line, col);
    double a = nv_std_need_number("رياضيات.اصغر", args[0], line, col);
    double b = nv_std_need_number("رياضيات.اصغر", args[1], line, col);
    return nv_num(fmin(a, b));
}

static NvValue nv_std_math_max2(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.اكبر", args, nargs, 2, line, col);
    double a = nv_std_need_number("رياضيات.اكبر", args[0], line, col);
    double b = nv_std_need_number("رياضيات.اكبر", args[1], line, col);
    return nv_num(fmax(a, b));
}

static NvValue nv_std_math_inc(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.زد", args, nargs, 1, line, col);
    return nv_num(nv_std_need_number("رياضيات.زد", args[0], line, col) + 1.0);
}

static NvValue nv_std_math_dec(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.نقص", args, nargs, 1, line, col);
    return nv_num(nv_std_need_number("رياضيات.نقص", args[0], line, col) - 1.0);
}

static NvValue nv_std_math_half(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.نصف", args, nargs, 1, line, col);
    return nv_num(nv_std_need_number("رياضيات.نصف", args[0], line, col) / 2.0);
}

static NvValue nv_std_math_double(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.ضعف", args, nargs, 1, line, col);
    return nv_num(nv_std_need_number("رياضيات.ضعف", args[0], line, col) * 2.0);
}

static unsigned long long nv_std_gcd_ull(unsigned long long a, unsigned long long b) {
    while (b != 0) {
        unsigned long long t = b;
        b = a % b;
        a = t;
    }
    return a;
}

static NvValue nv_std_math_gcd(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.قاسم_مشترك_أكبر", args, nargs, 2, line, col);
    long long a = nv_std_need_integer("رياضيات.قاسم_مشترك_أكبر", args[0], line, col);
    long long b = nv_std_need_integer("رياضيات.قاسم_مشترك_أكبر", args[1], line, col);
    unsigned long long g = nv_std_gcd_ull(
        (unsigned long long)(a < 0 ? -a : a),
        (unsigned long long)(b < 0 ? -b : b));
    if (g == 0) {
        nv_error(NV_ERR_RUNTIME, "القاسم المشترك الأكبر لصفر وصفر غير معرف", line, col);
    }
    return nv_num((double)g);
}

static NvValue nv_std_math_lcm(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.مضاعف_مشترك_أصغر", args, nargs, 2, line, col);
    long long a = nv_std_need_integer("رياضيات.مضاعف_مشترك_أصغر", args[0], line, col);
    long long b = nv_std_need_integer("رياضيات.مضاعف_مشترك_أصغر", args[1], line, col);
    unsigned long long ua = (unsigned long long)(a < 0 ? -a : a);
    unsigned long long ub = (unsigned long long)(b < 0 ? -b : b);
    if (ua == 0 || ub == 0) {
        return nv_num(0.0);
    }
    unsigned long long g = nv_std_gcd_ull(ua, ub);
    return nv_num((double)ua / (double)g * (double)ub);
}

static NvValue nv_std_math_set_diff(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.فرق_المجموعات", args, nargs, 2, line, col);
    if (args[0].tag != NV_LIST || args[1].tag != NV_LIST) {
        nv_error(NV_ERR_TYPE, "الدالة رياضيات.فرق_المجموعات تتوقع قائمتين", line, col);
    }
    NvList *a = args[0].u.list;
    NvList *b = args[1].u.list;
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    for (size_t i = 0; i < a->len; i++) {
        int found = 0;
        for (size_t j = 0; j < b->len; j++) {
            if (nv_eq(a->items[i], b->items[j])) {
                found = 1;
                break;
            }
        }
        if (!found) {
            nv_list_push_raw(out.u.list, nv_clone_value(a->items[i]));
        }
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_math_match(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.تطابق", args, nargs, 2, line, col);
    return nv_bool(nv_eq(args[0], args[1]));
}

static NvValue nv_std_math_asin(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جيب_عكسي", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.جيب_عكسي", args[0], line, col);
    if (x < -1.0 || x > 1.0) {
        nv_error(NV_ERR_RUNTIME, "جيب عكسي لعدد خارج [-1، 1] غير معرف", line, col);
    }
    return nv_num(asin(x));
}

static NvValue nv_std_math_acos(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جيب_تمام_عكسي", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.جيب_تمام_عكسي", args[0], line, col);
    if (x < -1.0 || x > 1.0) {
        nv_error(NV_ERR_RUNTIME, "جيب تمام عكسي لعدد خارج [-1، 1] غير معرف", line, col);
    }
    return nv_num(acos(x));
}

static NvValue nv_std_math_atan(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.ظل_عكسي", args, nargs, 1, line, col);
    return nv_num(atan(nv_std_need_number("رياضيات.ظل_عكسي", args[0], line, col)));
}

static NvValue nv_std_math_polar(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.قطبي", args, nargs, 2, line, col);
    double x = nv_std_need_number("رياضيات.قطبي", args[0], line, col);
    double y = nv_std_need_number("رياضيات.قطبي", args[1], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    nv_list_push_raw(out.u.list, nv_num(hypot(x, y)));
    nv_list_push_raw(out.u.list, nv_num(atan2(y, x)));
    nv_root_pop();
    return out;
}

static NvValue nv_std_math_cartesian(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.ديكارتي", args, nargs, 2, line, col);
    double r = nv_std_need_number("رياضيات.ديكارتي", args[0], line, col);
    double theta = nv_std_need_number("رياضيات.ديكارتي", args[1], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    nv_list_push_raw(out.u.list, nv_num(r * cos(theta)));
    nv_list_push_raw(out.u.list, nv_num(r * sin(theta)));
    nv_root_pop();
    return out;
}

static int nv_std_is_prime_ll(long long n) {
    if (n < 2) return 0;
    if (n == 2) return 1;
    if (n % 2 == 0) return 0;
    for (long long i = 3; i <= n / i; i += 2) {
        if (n % i == 0) return 0;
    }
    return 1;
}

static NvValue nv_std_math_is_prime(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.أولي", args, nargs, 1, line, col);
    long long n = nv_std_need_integer("رياضيات.أولي", args[0], line, col);
    return nv_bool(nv_std_is_prime_ll(n));
}

static NvValue nv_std_math_factorize(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.حلل_إلى_عوامل", args, nargs, 1, line, col);
    long long n = nv_std_need_integer("رياضيات.حلل_إلى_عوامل", args[0], line, col);
    if (n < 2) {
        nv_error(NV_ERR_RUNTIME, "تحليل عدد أصغر من 2 غير معرف", line, col);
    }
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    long long d = 2;
    while (d <= n / d) {
        while (n % d == 0) {
            nv_list_push_raw(out.u.list, nv_num((double)d));
            n /= d;
        }
        d += (d == 2 ? 1 : 2);
    }
    if (n > 1) {
        nv_list_push_raw(out.u.list, nv_num((double)n));
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_math_sign(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.إشارة", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.إشارة", args[0], line, col);
    double s = (x > 0.0) ? 1.0 : (x < 0.0 ? -1.0 : 0.0);
    return nv_num(s);
}

static NvValue nv_std_math_recip(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.مقلوب", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.مقلوب", args[0], line, col);
    if (x == 0.0) {
        nv_error(NV_ERR_RUNTIME, "مقلوب صفر غير معرف", line, col);
    }
    return nv_num(1.0 / x);
}

static NvValue nv_std_math_square(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.مربع", args, nargs, 1, line, col);
    double x = nv_std_need_number("رياضيات.مربع", args[0], line, col);
    return nv_num(x * x);
}

// ---------- 157: الحساب النمطي ----------
// كما في stdlib.rs حرفيا: ضرب نمطي بالمضاعفة
// الثنائية (بلا فيض)، وأس بالتربيع المتكرر، ومعكوس
// بأقليدس الموسع مع اختزال المعاملات بباقي القسمة.

static unsigned long long nv_std_mod_mul_ull(unsigned long long a, unsigned long long b, unsigned long long m) {
    unsigned long long result = 0;
    unsigned long long x = a % m;
    unsigned long long y = b;
    while (y > 0) {
        if (y & 1) {
            result = (result + x) % m;
        }
        x = (x * 2) % m;
        y >>= 1;
    }
    return result;
}

static unsigned long long nv_std_mod_pow_ull(unsigned long long base, unsigned long long exp, unsigned long long m) {
    unsigned long long result = 1 % m;
    base %= m;
    while (exp > 0) {
        if (exp & 1) {
            result = nv_std_mod_mul_ull(result, base, m);
        }
        base = nv_std_mod_mul_ull(base, base, m);
        exp >>= 1;
    }
    return result;
}

// معكوس نمطي: 1 إن وجد، و0 مع علم الغياب.
// a هنا مطبّع إلى [0، m) قبل الاستدعاء.
static unsigned long long nv_std_mod_inv_ull(unsigned long long a, unsigned long long m, int *found) {
    long long old_r = (long long)a;
    long long r = (long long)m;
    long long old_s = 1;
    long long s = 0;
    long long mi = (long long)m;
    while (r != 0) {
        long long q = old_r / r;
        long long next_r = old_r - q * r;
        unsigned long long qs = nv_std_mod_mul_ull((unsigned long long)(q % mi), (unsigned long long)s, m);
        long long next_s = old_s - (long long)qs;
        if (next_s < 0) {
            next_s += mi;
        }
        old_r = r;
        r = next_r;
        old_s = s;
        s = next_s;
    }
    if (old_r != 1) {
        *found = 0;
        return 0;
    }
    *found = 1;
    return (unsigned long long)old_s;
}

static long long nv_std_mod_norm_ll(long long a, long long m) {
    long long r = a % m;
    if (r < 0) {
        r += m;
    }
    return r;
}

static void nv_std_mod_need_pos(const char *display, long long m, int line, int col) {
    (void)display;
    if (m <= 0) {
        nv_error(NV_ERR_RUNTIME, "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا", line, col);
    }
}

static NvValue nv_std_math_mod_rem(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.باقي_نمطي", args, nargs, 2, line, col);
    long long a = nv_std_need_integer("رياضيات.باقي_نمطي", args[0], line, col);
    long long m = nv_std_need_integer("رياضيات.باقي_نمطي", args[1], line, col);
    nv_std_mod_need_pos("رياضيات.باقي_نمطي", m, line, col);
    return nv_num((double)nv_std_mod_norm_ll(a, m));
}

static NvValue nv_std_math_mod_pow(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.أس_نمطي", args, nargs, 3, line, col);
    long long base = nv_std_need_integer("رياضيات.أس_نمطي", args[0], line, col);
    long long exp = nv_std_need_integer("رياضيات.أس_نمطي", args[1], line, col);
    long long m = nv_std_need_integer("رياضيات.أس_نمطي", args[2], line, col);
    nv_std_mod_need_pos("رياضيات.أس_نمطي", m, line, col);
    if (exp < 0) {
        nv_error(NV_ERR_RUNTIME, "الأس النمطي يجب أن يكون عددا صحيحا غير سالب", line, col);
    }
    unsigned long long r = nv_std_mod_pow_ull(
        (unsigned long long)nv_std_mod_norm_ll(base, m),
        (unsigned long long)exp,
        (unsigned long long)m);
    return nv_num((double)r);
}

static NvValue nv_std_math_mod_inv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.معكوس_نمطي", args, nargs, 2, line, col);
    long long a = nv_std_need_integer("رياضيات.معكوس_نمطي", args[0], line, col);
    long long m = nv_std_need_integer("رياضيات.معكوس_نمطي", args[1], line, col);
    nv_std_mod_need_pos("رياضيات.معكوس_نمطي", m, line, col);
    int found = 0;
    unsigned long long r = nv_std_mod_inv_ull(
        (unsigned long long)nv_std_mod_norm_ll(a, m),
        (unsigned long long)m,
        &found);
    if (!found) {
        nv_error(NV_ERR_RUNTIME, "لا معكوس نمطي لهذا العدد بهذا المقسوم عليه", line, col);
    }
    return nv_num((double)r);
}

static NvValue nv_std_math_mod_add(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.جمع_نمطي", args, nargs, 3, line, col);
    long long a = nv_std_need_integer("رياضيات.جمع_نمطي", args[0], line, col);
    long long b = nv_std_need_integer("رياضيات.جمع_نمطي", args[1], line, col);
    long long m = nv_std_need_integer("رياضيات.جمع_نمطي", args[2], line, col);
    nv_std_mod_need_pos("رياضيات.جمع_نمطي", m, line, col);
    long long r = (nv_std_mod_norm_ll(a, m) + nv_std_mod_norm_ll(b, m)) % m;
    return nv_num((double)r);
}

static NvValue nv_std_math_mod_mul(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.ضرب_نمطي", args, nargs, 3, line, col);
    long long a = nv_std_need_integer("رياضيات.ضرب_نمطي", args[0], line, col);
    long long b = nv_std_need_integer("رياضيات.ضرب_نمطي", args[1], line, col);
    long long m = nv_std_need_integer("رياضيات.ضرب_نمطي", args[2], line, col);
    nv_std_mod_need_pos("رياضيات.ضرب_نمطي", m, line, col);
    unsigned long long r = nv_std_mod_mul_ull(
        (unsigned long long)nv_std_mod_norm_ll(a, m),
        (unsigned long long)nv_std_mod_norm_ll(b, m),
        (unsigned long long)m);
    return nv_num((double)r);
}

static NvValue nv_std_math_mod_sub(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("رياضيات.طرح_نمطي", args, nargs, 3, line, col);
    long long a = nv_std_need_integer("رياضيات.طرح_نمطي", args[0], line, col);
    long long b = nv_std_need_integer("رياضيات.طرح_نمطي", args[1], line, col);
    long long m = nv_std_need_integer("رياضيات.طرح_نمطي", args[2], line, col);
    nv_std_mod_need_pos("رياضيات.طرح_نمطي", m, line, col);
    long long r = nv_std_mod_norm_ll(nv_std_mod_norm_ll(a, m) - nv_std_mod_norm_ll(b, m), m);
    return nv_num((double)r);
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
    nv_std_add_fn(d, "جذر_نوني", nv_std_math_nth_root);
    nv_std_add_fn(d, "باقي", nv_std_math_rem);
    nv_std_add_fn(d, "عاملي", nv_std_math_factorial);
    nv_std_add_fn(d, "توافيق", nv_std_math_choose);
    // أصغر/أكبر كلمتا مقارنة محجوزتان: المفتاح المخزن
    // تهجئتهما المطبّعة "اصغر"/"اكبر" (كما "انشي" في
    // عمليات) — فيصل إليهما رياضيات.أصغر ورياضيات.اصغر معا.
    nv_std_add_fn(d, "اصغر", nv_std_math_min2);
    nv_std_add_fn(d, "اكبر", nv_std_math_max2);
    nv_std_add_fn(d, "زد", nv_std_math_inc);
    nv_std_add_fn(d, "نقص", nv_std_math_dec);
    nv_std_add_fn(d, "نصف", nv_std_math_half);
    nv_std_add_fn(d, "ضعف", nv_std_math_double);
    nv_std_add_fn(d, "قاسم_مشترك_أكبر", nv_std_math_gcd);
    nv_std_add_fn(d, "مضاعف_مشترك_أصغر", nv_std_math_lcm);
    nv_std_add_fn(d, "فرق_المجموعات", nv_std_math_set_diff);
    nv_std_add_fn(d, "تطابق", nv_std_math_match);
    nv_std_add_fn(d, "جيب_عكسي", nv_std_math_asin);
    nv_std_add_fn(d, "جيب_تمام_عكسي", nv_std_math_acos);
    nv_std_add_fn(d, "ظل_عكسي", nv_std_math_atan);
    nv_std_add_fn(d, "قطبي", nv_std_math_polar);
    nv_std_add_fn(d, "ديكارتي", nv_std_math_cartesian);
    nv_std_add_fn(d, "أولي", nv_std_math_is_prime);
    nv_std_add_fn(d, "حلل_إلى_عوامل", nv_std_math_factorize);
    nv_std_add_fn(d, "إشارة", nv_std_math_sign);
    nv_std_add_fn(d, "مقلوب", nv_std_math_recip);
    nv_std_add_fn(d, "مربع", nv_std_math_square);
    // J10-157: الحساب النمطي.
    nv_std_add_fn(d, "باقي_نمطي", nv_std_math_mod_rem);
    nv_std_add_fn(d, "أس_نمطي", nv_std_math_mod_pow);
    nv_std_add_fn(d, "معكوس_نمطي", nv_std_math_mod_inv);
    nv_std_add_fn(d, "جمع_نمطي", nv_std_math_mod_add);
    nv_std_add_fn(d, "ضرب_نمطي", nv_std_math_mod_mul);
    nv_std_add_fn(d, "طرح_نمطي", nv_std_math_mod_sub);
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

static NvValue nv_std_crypto_encrypt(NvValue *args, int nargs, int line, int col);
static NvValue nv_std_crypto_decrypt(NvValue *args, int nargs, int line, int col);

void nv_std_crypto_fill(NvDict *d) {
    nv_std_add_fn(d, "جزئ", nv_std_crypto_hash);
    nv_std_add_fn(d, "عشوائي_آمن", nv_std_crypto_secure_random);
    nv_std_add_fn(d, "شفر", nv_std_crypto_encrypt);
    nv_std_add_fn(d, "فك_التشفير", nv_std_crypto_decrypt);
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

#if defined(__linux__) || defined(__ANDROID__) || defined(__APPLE__)
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

#endif // defined(__linux__) || defined(__ANDROID__) || defined(__APPLE__)

/* الإزاحة المحلية (بالثواني) عند secs. خارج لينكس/أندرويد/ماك تُرفض بصدق. */
static long long nv_std_time_local_offset_at(long long secs, const char *display, int line, int col) {
#if defined(__linux__) || defined(__ANDROID__) || defined(__APPLE__)
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

static NvValue nv_std_time_format_custom(NvValue *args, int nargs, int line, int col);

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
    nv_std_add_fn(d, "نسق", nv_std_time_format_custom);
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

// مسار الثنائي الحالي (لتشغيل برامج .نيفا الفرعية بمحرك
// نيفا نفسه): على لينكس /proc/self/exe؛ وعلى ماك
// _NSGetExecutablePath + realpath (لا /proc هناك) —
// على نمط nv_exe_dir في niva_rt.c (حجم المخزن +
// realpath حرفيا). يعيد الطول أو -1 عند الفشل.
static ssize_t nv_std_exe_path(char *buf, size_t cap) {
    if (!buf || cap == 0) {
        return -1;
    }
#ifdef __APPLE__
    uint32_t sz = (uint32_t)cap;
    if (_NSGetExecutablePath(buf, &sz) != 0) {
        return -1;
    }
    char *real = realpath(buf, NULL);
    if (!real) {
        // realpath تقنين تجميلي فقط — فشله لا يلغي
        // المسار الخام: يصلح لفحص الجذع وللتشغيل.
        // (فشل متقطع هنا كان يُسقط إنشاء العملية كله
        // رغم وجود محرك مثبت في PATH.)
        return (ssize_t)strlen(buf);
    }
    size_t rl = strlen(real);
    if (rl >= cap) {
        rl = cap - 1;
    }
    memcpy(buf, real, rl);
    buf[rl] = '\0';
    free(real);
    return (ssize_t)rl;
#else
    return readlink("/proc/self/exe", buf, cap - 1);
#endif
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
        // يُشغَّل بثنائي نيفا الحالي نفسه. (اجتماع 49 §63)
        // الرمز النسبي: مجلد البرنامج أولا ثم البيت.
        char *resolved = nv_resolve_subprogram(program, line, col);
        char exe[4096];
        ssize_t elen = nv_std_exe_path(exe, sizeof(exe));
        if (elen <= 0) {
            // مسار الثنائي الحالي مجهول — لا خطأ فوري:
            // الجذع الفارغ ليس نيفا فيسلك فرع المحرك
            // المثبت (البديل المصمم) أدناه، والرفض فقط
            // إن لم يوجد محرك إطلاقا.
            exe[0] = '\0';
        } else {
            exe[elen] = '\0';
        }
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
    if (strcmp(s, "$نيفا") == 0 || strncmp(s, "$نيفا/", 10) == 0) return nv_std_ipc_strdup(s);
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
            // (اجتماع 49 §63) الرمز النسبي: مجلد البرنامج
            // أولا ثم البيت — مرآة `شغّل`.
            resolved = nv_resolve_subprogram(program, line, col);
            char exe_buf[4096];
            ssize_t exelen = nv_std_exe_path(exe_buf, sizeof exe_buf);
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
    if (strcmp(s, "$نيفا") == 0 || strncmp(s, "$نيفا/", 10) == 0) return nv_std_async_strdup(s);
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

// ================= J14 الأساسي (البنود 220 و221 و222 و223 و224 و213) =================
// مرآة C لوحدات stdlib.rs — المنطق نفسه حرفيا والرسائل نفسها.

// ---------- مساعدات عامة ----------

static char *j14_strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *d = (char *)malloc(n);
    memcpy(d, s, n);
    return d;
}

static char *j14_strndup(const char *s, size_t n) {
    char *d = (char *)malloc(n + 1);
    memcpy(d, s, n);
    d[n] = '\0';
    return d;
}

typedef struct {
    char *data;
    size_t len, cap;
} J14Buf;

static void j14_buf_init(J14Buf *b) {
    b->cap = 64;
    b->len = 0;
    b->data = (char *)malloc(b->cap);
    b->data[0] = '\0';
}

static void j14_buf_reserve(J14Buf *b, size_t extra) {
    if (b->len + extra + 1 > b->cap) {
        while (b->len + extra + 1 > b->cap) b->cap *= 2;
        b->data = (char *)realloc(b->data, b->cap);
    }
}

static void j14_buf_addn(J14Buf *b, const char *s, size_t n) {
    j14_buf_reserve(b, n);
    memcpy(b->data + b->len, s, n);
    b->len += n;
    b->data[b->len] = '\0';
}

static void j14_buf_add(J14Buf *b, const char *s) {
    j14_buf_addn(b, s, strlen(s));
}

static void j14_buf_addc(J14Buf *b, char c) {
    j14_buf_addn(b, &c, 1);
}

static size_t j14_utf8_encode(uint32_t cp, char out[4]) {
    if (cp < 0x80) {
        out[0] = (char)cp;
        return 1;
    }
    if (cp < 0x800) {
        out[0] = (char)(0xC0 | (cp >> 6));
        out[1] = (char)(0x80 | (cp & 0x3F));
        return 2;
    }
    if (cp < 0x10000) {
        out[0] = (char)(0xE0 | (cp >> 12));
        out[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[2] = (char)(0x80 | (cp & 0x3F));
        return 3;
    }
    out[0] = (char)(0xF0 | (cp >> 18));
    out[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
    out[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
    out[3] = (char)(0x80 | (cp & 0x3F));
    return 4;
}

static void j14_buf_add_cp(J14Buf *b, uint32_t cp) {
    char tmp[4];
    size_t n = j14_utf8_encode(cp, tmp);
    j14_buf_addn(b, tmp, n);
}

static void j14_buf_free(J14Buf *b) {
    free(b->data);
    b->data = NULL;
    b->len = b->cap = 0;
}

/// فك محرف UTF-8 واحد؛ ويعيد البايتات المستهلكة
/// (بايت غير صالح يُقرأ كنقطة شفرة لاتينية).
static size_t j14_utf8_decode(const unsigned char *s, size_t len, uint32_t *cp) {
    if (len == 0) {
        *cp = 0;
        return 0;
    }
    unsigned char c = s[0];
    if (c < 0x80) {
        *cp = c;
        return 1;
    }
    if ((c & 0xE0) == 0xC0 && len >= 2 && (s[1] & 0xC0) == 0x80) {
        *cp = ((uint32_t)(c & 0x1F) << 6) | (uint32_t)(s[1] & 0x3F);
        return 2;
    }
    if ((c & 0xF0) == 0xE0 && len >= 3 && (s[1] & 0xC0) == 0x80 && (s[2] & 0xC0) == 0x80) {
        *cp = ((uint32_t)(c & 0x0F) << 12) | ((uint32_t)(s[1] & 0x3F) << 6) |
              (uint32_t)(s[2] & 0x3F);
        return 3;
    }
    if ((c & 0xF8) == 0xF0 && len >= 4 && (s[1] & 0xC0) == 0x80 && (s[2] & 0xC0) == 0x80 &&
        (s[3] & 0xC0) == 0x80) {
        *cp = ((uint32_t)(c & 0x07) << 18) | ((uint32_t)(s[1] & 0x3F) << 12) |
              ((uint32_t)(s[2] & 0x3F) << 6) | (uint32_t)(s[3] & 0x3F);
        return 4;
    }
    *cp = c;
    return 1;
}

/// فك نص كاملا إلى نقاط شفرة؛ وoffs[i] إزاحة بايت
/// المحرف i، وoffs[n] طول البايتات.
static uint32_t *j14_decode_all(const char *s, size_t len, size_t *n_out, size_t **offs_out) {
    size_t cap = len + 1;
    uint32_t *out = (uint32_t *)malloc(cap * sizeof(uint32_t));
    size_t *offs = (size_t *)malloc((cap + 1) * sizeof(size_t));
    size_t n = 0, i = 0;
    while (i < len) {
        uint32_t cp = 0;
        size_t adv = j14_utf8_decode((const unsigned char *)s + i, len - i, &cp);
        if (adv == 0) adv = 1;
        offs[n] = i;
        out[n++] = cp;
        i += adv;
    }
    offs[n] = len;
    *n_out = n;
    *offs_out = offs;
    return out;
}

// ---------- 220: التعابير النمطية (مرآة Rust حرفيا) ----------

#define RX_MAX_PATTERN 1000
#define RX_MAX_TEXT 1000000
#define RX_MAX_PROG 10000
#define RX_MAX_STEPS 1000000
#define RX_MAX_GROUPS 32
#define RX_MAX_REPEAT 1000

typedef struct {
    uint32_t lo, hi;
} RxRange;

typedef enum {
    RX_CHR, RX_ANY, RX_CLS, RX_DIGIT, RX_WORD, RX_SPACE,
    RX_SPLIT, RX_JMP, RX_SAVE, RX_BOL, RX_EOL, RX_WB, RX_MATCH
} RxKind;

typedef struct {
    RxKind kind;
    uint32_t ch;
    RxRange *ranges;
    int nranges;
    int neg;
    int a, b;
    int target;
    int slot;
    int want;
} RxInst;

typedef struct {
    RxInst *prog;
    int len, cap;
    uint32_t *chars;
    int clen, cpos;
    int ngroups;
} RxComp;

typedef struct {
    int idx, slot;
} RxOut;

typedef struct {
    int start, end;
    RxOut *outs;
    int nouts, capouts;
} RxFrag;

typedef struct {
    RxInst *prog;
    int len;
    int ngroups;
    int fold; // طيّ الحالة (وحدة بحث): 1 يطابق بلا حساسية حالة
} RxProg;

static int rx_is_word_cp(uint32_t c) {
    if ((c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_')
        return 1;
    if (c >= 0x0600 && c <= 0x06FF) return 1;
    if (c >= 0x0750 && c <= 0x077F) return 1;
    if (c >= 0x08A0 && c <= 0x08FF) return 1;
    if (c >= 0xFB50 && c <= 0xFDFF) return 1;
    if (c >= 0xFE70 && c <= 0xFEFF) return 1;
    return 0;
}

static int rx_is_space_cp(uint32_t c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == 0x0B || c == 0x0C;
}

static int rx_class_matches(RxRange *r, int n, int neg, uint32_t c) {
    int inside = 0;
    for (int i = 0; i < n; i++) {
        if (r[i].lo <= c && c <= r[i].hi) {
            inside = 1;
            break;
        }
    }
    return inside != neg;
}

/// طيّ محرف إلى صغيره (ASCII + لاتينية-1) لمطابقة
/// بلا حساسية حالة؛ والعربية بلا حالة فلا أثر لها.
static uint32_t rx_fold_cp(uint32_t c) {
    if (c >= 'A' && c <= 'Z') return c + 32;
    if (c >= 0x00C0 && c <= 0x00DE && c != 0x00D7) return c + 32;
    return c;
}

static int rx_class_matches_fold(RxRange *r, int n, int neg, uint32_t c) {
    uint32_t fc = rx_fold_cp(c);
    int inside = 0;
    for (int i = 0; i < n; i++) {
        uint32_t lo = rx_fold_cp(r[i].lo);
        uint32_t hi = rx_fold_cp(r[i].hi);
        if (lo <= fc && fc <= hi) {
            inside = 1;
            break;
        }
    }
    return inside != neg;
}

static int rx_consumes(RxInst *in, uint32_t c, int fold) {
    switch (in->kind) {
        case RX_CHR: return fold ? rx_fold_cp(in->ch) == rx_fold_cp(c) : in->ch == c;
        case RX_ANY: return c != '\n';
        case RX_CLS: return fold ? rx_class_matches_fold(in->ranges, in->nranges, in->neg, c)
                                 : rx_class_matches(in->ranges, in->nranges, in->neg, c);
        case RX_DIGIT: return (c >= '0' && c <= '9') != in->neg;
        case RX_WORD: return rx_is_word_cp(c) != in->neg;
        case RX_SPACE: return rx_is_space_cp(c) != in->neg;
        default: return 0;
    }
}

static void rx_prog_free(RxProg *p) {
    if (!p->prog) return;
    for (int i = 0; i < p->len; i++) free(p->prog[i].ranges);
    free(p->prog);
    p->prog = NULL;
    p->len = 0;
}

static RxInst rx_inst_new(RxKind kind) {
    RxInst in;
    memset(&in, 0, sizeof(in));
    in.kind = kind;
    return in;
}

static int rx_emit(RxComp *c, RxInst in, int line, int col) {
    if (c->len >= RX_MAX_PROG) {
        nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
    }
    if (c->len == c->cap) {
        c->cap = c->cap ? c->cap * 2 : 64;
        c->prog = (RxInst *)realloc(c->prog, c->cap * sizeof(RxInst));
    }
    c->prog[c->len] = in;
    return c->len++;
}

static void rx_frag_push_out(RxFrag *f, int idx, int slot) {
    if (f->nouts == f->capouts) {
        f->capouts = f->capouts ? f->capouts * 2 : 8;
        f->outs = (RxOut *)realloc(f->outs, f->capouts * sizeof(RxOut));
    }
    f->outs[f->nouts].idx = idx;
    f->outs[f->nouts].slot = slot;
    f->nouts++;
}

static void rx_patch(RxComp *c, RxFrag *f, int target) {
    for (int i = 0; i < f->nouts; i++) {
        RxInst *in = &c->prog[f->outs[i].idx];
        if (in->kind == RX_SPLIT) {
            if (f->outs[i].slot == 0) in->a = target;
            else in->b = target;
        } else if (in->kind == RX_JMP) {
            in->target = target;
        }
    }
}

static RxFrag rx_frag_one(RxComp *c, RxInst in, int line, int col) {
    int start = rx_emit(c, in, line, col);
    RxInst jmp = rx_inst_new(RX_JMP);
    jmp.target = INT_MAX;
    int j = rx_emit(c, jmp, line, col);
    RxFrag f;
    memset(&f, 0, sizeof(f));
    f.start = start;
    f.end = c->len;
    rx_frag_push_out(&f, j, 0);
    return f;
}

static RxFrag rx_chain(RxComp *c, RxFrag a, RxFrag b) {
    rx_patch(c, &a, b.start);
    free(a.outs);
    RxFrag r;
    r.start = a.start;
    r.end = b.end;
    r.outs = b.outs;
    r.nouts = b.nouts;
    r.capouts = b.capouts;
    return r;
}

static RxFrag rx_seq(RxComp *c, RxFrag *frags, int n, int line, int col) {
    if (n == 0) {
        RxInst jmp = rx_inst_new(RX_JMP);
        jmp.target = INT_MAX;
        int j = rx_emit(c, jmp, line, col);
        RxFrag f;
        memset(&f, 0, sizeof(f));
        f.start = j;
        f.end = c->len;
        rx_frag_push_out(&f, j, 0);
        return f;
    }
    RxFrag cur = frags[0];
    for (int i = 1; i < n; i++) cur = rx_chain(c, cur, frags[i]);
    return cur;
}

static RxFrag rx_star(RxComp *c, RxFrag body, int lazy, int line, int col) {
    RxInst s = rx_inst_new(RX_SPLIT);
    if (lazy) {
        s.a = INT_MAX;
        s.b = body.start;
    } else {
        s.a = body.start;
        s.b = INT_MAX;
    }
    int si = rx_emit(c, s, line, col);
    rx_patch(c, &body, si);
    free(body.outs);
    RxFrag f;
    memset(&f, 0, sizeof(f));
    f.start = si;
    f.end = c->len;
    rx_frag_push_out(&f, si, lazy ? 0 : 1);
    return f;
}

static RxFrag rx_opt(RxComp *c, RxFrag body, int lazy, int line, int col) {
    RxInst s = rx_inst_new(RX_SPLIT);
    if (lazy) {
        s.a = INT_MAX;
        s.b = body.start;
    } else {
        s.a = body.start;
        s.b = INT_MAX;
    }
    int si = rx_emit(c, s, line, col);
    RxFrag f;
    memset(&f, 0, sizeof(f));
    f.start = si;
    f.end = c->len;
    for (int i = 0; i < body.nouts; i++) rx_frag_push_out(&f, body.outs[i].idx, body.outs[i].slot);
    rx_frag_push_out(&f, si, lazy ? 0 : 1);
    free(body.outs);
    return f;
}

static RxFrag rx_dup(RxComp *c, RxFrag *frag, int line, int col) {
    int start = frag->start;
    int end = frag->end;
    int base = c->len;
    for (int i = start; i < end; i++) {
        RxInst in = c->prog[i];
        if (in.kind == RX_CLS && in.ranges) {
            RxRange *copy = (RxRange *)malloc(in.nranges * sizeof(RxRange));
            memcpy(copy, in.ranges, in.nranges * sizeof(RxRange));
            in.ranges = copy;
        }
        rx_emit(c, in, line, col);
    }
    for (int i = base; i < base + (end - start); i++) {
        RxInst *in = &c->prog[i];
        if (in->kind == RX_SPLIT) {
            if (in->a >= start && in->a < end) in->a = in->a - start + base;
            if (in->b >= start && in->b < end) in->b = in->b - start + base;
        } else if (in->kind == RX_JMP) {
            if (in->target >= start && in->target < end)
                in->target = in->target - start + base;
        }
    }
    RxFrag f;
    memset(&f, 0, sizeof(f));
    f.start = base;
    f.end = base + (end - start);
    for (int i = 0; i < frag->nouts; i++)
        rx_frag_push_out(&f, frag->outs[i].idx - start + base, frag->outs[i].slot);
    return f;
}

static int rx_comp_peek(RxComp *c) {
    if (c->cpos >= c->clen) return -1;
    return (int)c->chars[c->cpos];
}

static int rx_comp_next(RxComp *c) {
    int v = rx_comp_peek(c);
    if (v >= 0) c->cpos++;
    return v;
}

static RxFrag rx_parse_alt(RxComp *c, int line, int col);
static RxFrag rx_parse_concat(RxComp *c, int line, int col);
static RxFrag rx_parse_repeat(RxComp *c, int line, int col);
static RxFrag rx_parse_atom(RxComp *c, int line, int col);

static RxFrag rx_parse_alt(RxComp *c, int line, int col) {
    RxFrag left = rx_parse_concat(c, line, col);
    if (rx_comp_peek(c) != '|') return left;
    c->cpos++;
    RxFrag right = rx_parse_alt(c, line, col);
    RxInst s = rx_inst_new(RX_SPLIT);
    s.a = left.start;
    s.b = right.start;
    int si = rx_emit(c, s, line, col);
    RxFrag f;
    memset(&f, 0, sizeof(f));
    f.start = si;
    f.end = c->len;
    for (int i = 0; i < left.nouts; i++)
        rx_frag_push_out(&f, left.outs[i].idx, left.outs[i].slot);
    for (int i = 0; i < right.nouts; i++)
        rx_frag_push_out(&f, right.outs[i].idx, right.outs[i].slot);
    free(left.outs);
    free(right.outs);
    return f;
}

static RxFrag rx_parse_concat(RxComp *c, int line, int col) {
    RxFrag frags[1024];
    int n = 0;
    int peek;
    while ((peek = rx_comp_peek(c)) >= 0 && peek != '|' && peek != ')') {
        if (n >= 1024) nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
        frags[n++] = rx_parse_repeat(c, line, col);
    }
    return rx_seq(c, frags, n, line, col);
}

static int rx_eat_lazy(RxComp *c) {
    if (rx_comp_peek(c) == '?') {
        c->cpos++;
        return 1;
    }
    return 0;
}

/// نسخة سطحية من قطعة: التعليمات نفسها، ومصفوفة مخارج مستقلة.
/// (بدونها تشارك القطعة الأولى في الأقواس مصفوفة atom فيحررها rx_chain.)
static RxFrag rx_frag_copy_outs(RxFrag *frag) {
    RxFrag f = *frag;
    if (frag->nouts > 0) {
        f.outs = (RxOut *)malloc(frag->nouts * sizeof(RxOut));
        memcpy(f.outs, frag->outs, frag->nouts * sizeof(RxOut));
        f.capouts = frag->nouts;
    } else {
        f.outs = NULL;
        f.capouts = 0;
    }
    return f;
}

static RxFrag rx_parse_braces(RxComp *c, RxFrag atom, int line, int col) {
    c->cpos++; // '{'
    long n = 0;
    int ndigits = 0;
    while (rx_comp_peek(c) >= '0' && rx_comp_peek(c) <= '9') {
        n = n * 10 + (rx_comp_next(c) - '0');
        ndigits++;
        if (n > RX_MAX_REPEAT) nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
    }
    if (ndigits == 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    long m = n;
    int unbounded = 0;
    if (rx_comp_peek(c) == ',') {
        c->cpos++;
        if (rx_comp_peek(c) == '}') {
            unbounded = 1;
        } else {
            m = 0;
            ndigits = 0;
            while (rx_comp_peek(c) >= '0' && rx_comp_peek(c) <= '9') {
                m = m * 10 + (rx_comp_next(c) - '0');
                ndigits++;
                if (m > RX_MAX_REPEAT) nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
            }
            if (ndigits == 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
        }
    }
    if (rx_comp_peek(c) != '}') nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    c->cpos++;
    if (!unbounded && m < n) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    int lazy = rx_eat_lazy(c);
    if (n == 0 && !unbounded && m == 0) {
        RxInst jmp = rx_inst_new(RX_JMP);
        jmp.target = INT_MAX;
        int j = rx_emit(c, jmp, line, col);
        RxFrag f;
        memset(&f, 0, sizeof(f));
        f.start = j;
        f.end = c->len;
        rx_frag_push_out(&f, j, 0);
        free(atom.outs);
        return f;
    }
    int have = 0;
    RxFrag result;
    memset(&result, 0, sizeof(result));
    for (long i = 0; i < n; i++) {
        RxFrag piece;
        if (i == 0) piece = rx_frag_copy_outs(&atom);
        else piece = rx_dup(c, &atom, line, col);
        if (!have) {
            result = piece;
            have = 1;
        } else {
            result = rx_chain(c, result, piece);
        }
    }
    if (unbounded) {
        RxFrag extra = rx_dup(c, &atom, line, col);
        RxFrag starred = rx_star(c, extra, lazy, line, col);
        if (!have) result = starred;
        else result = rx_chain(c, result, starred);
    } else {
        for (long i = n; i < m; i++) {
            RxFrag extra = rx_dup(c, &atom, line, col);
            RxFrag optional = rx_opt(c, extra, lazy, line, col);
            if (!have) {
                result = optional;
                have = 1;
            } else {
                result = rx_chain(c, result, optional);
            }
        }
    }
    free(atom.outs);
    return result;
}

static RxFrag rx_parse_repeat(RxComp *c, int line, int col) {
    RxFrag atom = rx_parse_atom(c, line, col);
    for (;;) {
        int peek = rx_comp_peek(c);
        if (peek == '*') {
            c->cpos++;
            int lazy = rx_eat_lazy(c);
            atom = rx_star(c, atom, lazy, line, col);
        } else if (peek == '+') {
            c->cpos++;
            int lazy = rx_eat_lazy(c);
            RxInst s = rx_inst_new(RX_SPLIT);
            if (lazy) {
                s.a = INT_MAX;
                s.b = atom.start;
            } else {
                s.a = atom.start;
                s.b = INT_MAX;
            }
            int si = rx_emit(c, s, line, col);
            rx_patch(c, &atom, si);
            free(atom.outs);
            RxFrag f;
            memset(&f, 0, sizeof(f));
            f.start = atom.start;
            f.end = c->len;
            rx_frag_push_out(&f, si, lazy ? 0 : 1);
            atom = f;
        } else if (peek == '?') {
            c->cpos++;
            int lazy = rx_eat_lazy(c);
            atom = rx_opt(c, atom, lazy, line, col);
        } else if (peek == '{') {
            atom = rx_parse_braces(c, atom, line, col);
        } else {
            break;
        }
    }
    return atom;
}

/// عنصر فئة: محرف مفرد (is_ranges=0) أو نطاقات هروب.
typedef struct {
    int is_ranges;
    uint32_t ch;
    RxRange *ranges;
    int nranges;
} RxClassItem;

static void rx_word_ranges(RxRange **out, int *n_out) {
    static const RxRange ranges[] = {
        {'0', '9'}, {'A', 'Z'}, {'_', '_'}, {'a', 'z'},
        {0x0600, 0x06FF}, {0x0750, 0x077F}, {0x08A0, 0x08FF},
        {0xFB50, 0xFDFF}, {0xFE70, 0xFEFF},
    };
    int n = (int)(sizeof(ranges) / sizeof(ranges[0]));
    RxRange *copy = (RxRange *)malloc(n * sizeof(RxRange));
    memcpy(copy, ranges, n * sizeof(RxRange));
    *out = copy;
    *n_out = n;
}

static RxClassItem rx_class_item(RxComp *c, int line, int col) {
    RxClassItem item;
    memset(&item, 0, sizeof(item));
    if (rx_comp_peek(c) == '\\') {
        c->cpos++;
        int e = rx_comp_next(c);
        if (e < 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
        switch (e) {
            case 'd':
                item.is_ranges = 1;
                item.ranges = (RxRange *)malloc(sizeof(RxRange));
                item.ranges[0].lo = '0';
                item.ranges[0].hi = '9';
                item.nranges = 1;
                return item;
            case 'w':
                item.is_ranges = 1;
                rx_word_ranges(&item.ranges, &item.nranges);
                return item;
            case 's': {
                item.is_ranges = 1;
                item.nranges = 6;
                item.ranges = (RxRange *)malloc(6 * sizeof(RxRange));
                item.ranges[0].lo = item.ranges[0].hi = ' ';
                item.ranges[1].lo = item.ranges[1].hi = '\t';
                item.ranges[2].lo = item.ranges[2].hi = '\n';
                item.ranges[3].lo = item.ranges[3].hi = '\r';
                item.ranges[4].lo = item.ranges[4].hi = 0x0B;
                item.ranges[5].lo = item.ranges[5].hi = 0x0C;
                return item;
            }
            case 'D': case 'W': case 'S':
                nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
            case 'n': item.ch = '\n'; return item;
            case 'r': item.ch = '\r'; return item;
            case 't': item.ch = '\t'; return item;
            case 'f': item.ch = 0x0C; return item;
            case 'v': item.ch = 0x0B; return item;
            default:
                if (e >= '0' && e <= '9') nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
                if ((e >= 'A' && e <= 'Z') || (e >= 'a' && e <= 'z'))
                    nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
                item.ch = (uint32_t)e;
                return item;
        }
    }
    int ch = rx_comp_next(c);
    if (ch < 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    item.ch = (uint32_t)ch;
    return item;
}

static RxInst rx_parse_class(RxComp *c, int line, int col) {
    c->cpos++; // '['
    int neg = 0;
    if (rx_comp_peek(c) == '^') {
        c->cpos++;
        neg = 1;
    }
    RxRange *ranges = NULL;
    int nranges = 0, capranges = 0;
    int first = 1;
    for (;;) {
        int peek = rx_comp_peek(c);
        if (peek < 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
        if (peek == ']' && !first) {
            c->cpos++;
            break;
        }
        first = 0;
        RxClassItem item = rx_class_item(c, line, col);
        if (item.is_ranges) {
            for (int i = 0; i < item.nranges; i++) {
                if (nranges == capranges) {
                    capranges = capranges ? capranges * 2 : 8;
                    ranges = (RxRange *)realloc(ranges, capranges * sizeof(RxRange));
                }
                ranges[nranges++] = item.ranges[i];
            }
            free(item.ranges);
        } else {
            uint32_t lo = item.ch;
            int next = rx_comp_peek(c);
            int after = (c->cpos + 1 < c->clen) ? (int)c->chars[c->cpos + 1] : -1;
            if (next == '-' && after != ']' && after >= 0) {
                c->cpos++;
                RxClassItem hi_item = rx_class_item(c, line, col);
                if (hi_item.is_ranges) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
                uint32_t hi = hi_item.ch;
                if (hi < lo) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
                if (nranges == capranges) {
                    capranges = capranges ? capranges * 2 : 8;
                    ranges = (RxRange *)realloc(ranges, capranges * sizeof(RxRange));
                }
                ranges[nranges].lo = lo;
                ranges[nranges].hi = hi;
                nranges++;
            } else {
                if (nranges == capranges) {
                    capranges = capranges ? capranges * 2 : 8;
                    ranges = (RxRange *)realloc(ranges, capranges * sizeof(RxRange));
                }
                ranges[nranges].lo = lo;
                ranges[nranges].hi = lo;
                nranges++;
            }
        }
    }
    RxInst in = rx_inst_new(RX_CLS);
    in.ranges = ranges;
    in.nranges = nranges;
    in.neg = neg;
    return in;
}

static RxInst rx_parse_escape(RxComp *c, int in_class, int line, int col) {
    int e = rx_comp_next(c);
    if (e < 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    RxInst in;
    switch (e) {
        case 'd': in = rx_inst_new(RX_DIGIT); in.neg = 0; return in;
        case 'D': in = rx_inst_new(RX_DIGIT); in.neg = 1; return in;
        case 'w': in = rx_inst_new(RX_WORD); in.neg = 0; return in;
        case 'W': in = rx_inst_new(RX_WORD); in.neg = 1; return in;
        case 's': in = rx_inst_new(RX_SPACE); in.neg = 0; return in;
        case 'S': in = rx_inst_new(RX_SPACE); in.neg = 1; return in;
        case 'b':
            if (!in_class) {
                in = rx_inst_new(RX_WB);
                in.want = 1;
                return in;
            }
            break;
        case 'B':
            if (!in_class) {
                in = rx_inst_new(RX_WB);
                in.want = 0;
                return in;
            }
            break;
        case 'n': in = rx_inst_new(RX_CHR); in.ch = '\n'; return in;
        case 'r': in = rx_inst_new(RX_CHR); in.ch = '\r'; return in;
        case 't': in = rx_inst_new(RX_CHR); in.ch = '\t'; return in;
        case 'f': in = rx_inst_new(RX_CHR); in.ch = 0x0C; return in;
        case 'v': in = rx_inst_new(RX_CHR); in.ch = 0x0B; return in;
        default: break;
    }
    if ((e >= '0' && e <= '9') || (e >= 'A' && e <= 'Z') || (e >= 'a' && e <= 'z'))
        nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    in = rx_inst_new(RX_CHR);
    in.ch = (uint32_t)e;
    return in;
}

static RxFrag rx_parse_atom(RxComp *c, int line, int col) {
    int peek = rx_comp_peek(c);
    if (peek < 0) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    if (peek == '(') {
        c->cpos++;
        int capturing = 1;
        if (rx_comp_peek(c) == '?' && c->cpos + 1 < c->clen && c->chars[c->cpos + 1] == ':') {
            c->cpos += 2;
            capturing = 0;
        }
        int group = 0;
        if (capturing) {
            c->ngroups++;
            if (c->ngroups > RX_MAX_GROUPS)
                nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
            group = c->ngroups;
        }
        RxFrag save1;
        memset(&save1, 0, sizeof(save1));
        if (capturing) {
            RxInst s = rx_inst_new(RX_SAVE);
            s.slot = 2 * group;
            save1 = rx_frag_one(c, s, line, col);
        }
        RxFrag inner = rx_parse_alt(c, line, col);
        if (rx_comp_next(c) != ')') nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
        if (!capturing) return inner;
        RxInst s2 = rx_inst_new(RX_SAVE);
        s2.slot = 2 * group + 1;
        RxFrag save2 = rx_frag_one(c, s2, line, col);
        RxFrag chained = rx_chain(c, save1, inner);
        return rx_chain(c, chained, save2);
    }
    if (peek == '[') {
        RxInst in = rx_parse_class(c, line, col);
        return rx_frag_one(c, in, line, col);
    }
    if (peek == '.') {
        c->cpos++;
        return rx_frag_one(c, rx_inst_new(RX_ANY), line, col);
    }
    if (peek == '^') {
        c->cpos++;
        return rx_frag_one(c, rx_inst_new(RX_BOL), line, col);
    }
    if (peek == '$') {
        c->cpos++;
        return rx_frag_one(c, rx_inst_new(RX_EOL), line, col);
    }
    if (peek == '\\') {
        c->cpos++;
        RxInst in = rx_parse_escape(c, 0, line, col);
        return rx_frag_one(c, in, line, col);
    }
    if (peek == ')' || peek == '|') nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    c->cpos++;
    RxInst in = rx_inst_new(RX_CHR);
    in.ch = (uint32_t)peek;
    return rx_frag_one(c, in, line, col);
}

static RxProg rx_compile_fold_c(const char *pattern, int full, int fold, int line, int col);

static RxProg rx_compile_c(const char *pattern, int full, int line, int col) {
    return rx_compile_fold_c(pattern, full, 0, line, col);
}

/// تجميع النمط مع طيّ الحالة اختيارا (وحدة بحث).
static RxProg rx_compile_fold_c(const char *pattern, int full, int fold, int line, int col) {
    size_t plen = strlen(pattern);
    size_t nchars = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(pattern, plen, &nchars, &offs);
    if (nchars > RX_MAX_PATTERN) {
        free(chars);
        free(offs);
        nv_error(NV_ERR_RUNTIME, "النمط كبير جدا", line, col);
    }
    RxComp c;
    memset(&c, 0, sizeof(c));
    c.chars = chars;
    c.clen = (int)nchars;
    RxInst save0in = rx_inst_new(RX_SAVE);
    save0in.slot = 0;
    RxFrag frag = rx_frag_one(&c, save0in, line, col);
    if (full) {
        RxFrag bol = rx_frag_one(&c, rx_inst_new(RX_BOL), line, col);
        frag = rx_chain(&c, frag, bol);
    }
    RxFrag body = rx_parse_alt(&c, line, col);
    if (c.cpos != c.clen) nv_error(NV_ERR_RUNTIME, "نمط غير صالح", line, col);
    frag = rx_chain(&c, frag, body);
    RxInst save1in = rx_inst_new(RX_SAVE);
    save1in.slot = 1;
    RxFrag save1 = rx_frag_one(&c, save1in, line, col);
    frag = rx_chain(&c, frag, save1);
    if (full) {
        RxFrag eol = rx_frag_one(&c, rx_inst_new(RX_EOL), line, col);
        frag = rx_chain(&c, frag, eol);
    }
    int midx = rx_emit(&c, rx_inst_new(RX_MATCH), line, col);
    rx_patch(&c, &frag, midx);
    free(frag.outs);
    free(chars);
    free(offs);
    RxProg p;
    p.prog = c.prog;
    p.len = c.len;
    p.ngroups = c.ngroups;
    p.fold = fold;
    return p;
}

typedef struct {
    int pc;
    int *saved;
} RxThread;

/// أضف خيطا مع تتبع تعليمات إبسيلون (بأولوية الترتيب).
/// يعيد 1 إن تبنّى خيط مصفوفة saved (فلا يحررها المتصل)
/// و0 إن لم يتبنها أحد (فيحررها المتصل).
static int rx_add_thread(
    RxProg *p, RxThread **list, int *nlist, int *caplist,
    unsigned char *visited, int pc, int pos,
    uint32_t *text, int tlen, int *saved, int nslots,
    int *steps, int line, int col) {
    for (;;) {
        (*steps)++;
        if (*steps > RX_MAX_STEPS) {
            nv_error(NV_ERR_RUNTIME, "تجاوز النمط حد الخطوات", line, col);
        }
        if (pc < 0 || pc >= p->len || visited[pc]) return 0;
        RxInst *in = &p->prog[pc];
        switch (in->kind) {
            case RX_SPLIT: {
                visited[pc] = 1;
                int a = in->a, b = in->b;
                int *copy = (int *)malloc(nslots * sizeof(int));
                memcpy(copy, saved, nslots * sizeof(int));
                int adopted_a = rx_add_thread(p, list, nlist, caplist, visited, a, pos, text, tlen, copy, nslots, steps, line, col);
                if (!adopted_a) free(copy);
                return rx_add_thread(p, list, nlist, caplist, visited, b, pos, text, tlen, saved, nslots, steps, line, col);
            }
            case RX_JMP:
                visited[pc] = 1;
                pc = in->target;
                break;
            case RX_SAVE:
                visited[pc] = 1;
                saved[in->slot] = pos;
                pc++;
                break;
            case RX_BOL:
                visited[pc] = 1;
                if (pos != 0) return 0;
                pc++;
                break;
            case RX_EOL:
                visited[pc] = 1;
                if (pos != tlen) return 0;
                pc++;
                break;
            case RX_WB: {
                visited[pc] = 1;
                int before = (pos == 0) ? 0 : rx_is_word_cp(text[pos - 1]);
                int after = (pos >= tlen) ? 0 : rx_is_word_cp(text[pos]);
                if ((before != after) != in->want) return 0;
                pc++;
                break;
            }
            default: {
                visited[pc] = 1;
                if (*nlist == *caplist) {
                    *caplist = *caplist ? *caplist * 2 : 16;
                    *list = (RxThread *)realloc(*list, *caplist * sizeof(RxThread));
                }
                (*list)[*nlist].pc = pc;
                (*list)[*nlist].saved = saved;
                (*nlist)++;
                return 1;
            }
        }
    }
}

static void rx_free_threads(RxThread *list, int n) {
    for (int i = 0; i < n; i++) free(list[i].saved);
    free(list);
}

/// مطابقة تبدأ عند start؛ 1 مع خانات (يملكها المتصل) أو 0.
static int rx_match_at(
    RxProg *p, uint32_t *text, int tlen, int start,
    int **saved_out, int line, int col) {
    int nslots = 2 * (p->ngroups + 1);
    int steps = 0;
    RxThread *clist = NULL;
    int nclist = 0, capclist = 0;
    unsigned char *visited = (unsigned char *)calloc(p->len > 0 ? p->len : 1, 1);
    int *saved0 = (int *)malloc(nslots * sizeof(int));
    for (int i = 0; i < nslots; i++) saved0[i] = -1;
    int adopted0 = rx_add_thread(p, &clist, &nclist, &capclist, visited, 0, start, text, tlen, saved0, nslots, &steps, line, col);
    if (!adopted0) free(saved0);
    free(visited);
    int pos = start;
    for (;;) {
        int *best = NULL;
        int alive = 0;
        for (int i = 0; i < nclist; i++) {
            RxInst *in = &p->prog[clist[i].pc];
            if (in->kind == RX_MATCH) {
                best = clist[i].saved;
                break;
            }
            if (pos < tlen && rx_consumes(in, text[pos], p->fold)) {
                alive = 1;
                break;
            }
        }
        if (best) {
            *saved_out = best;
            for (int i = 0; i < nclist; i++) {
                if (clist[i].saved != best) free(clist[i].saved);
            }
            free(clist);
            return 1;
        }
        if (!alive || pos >= tlen) {
            rx_free_threads(clist, nclist);
            return 0;
        }
        RxThread *nlist = NULL;
        int nnlist = 0, capnlist = 0;
        unsigned char *nvisited = (unsigned char *)calloc(p->len > 0 ? p->len : 1, 1);
        for (int i = 0; i < nclist; i++) {
            steps++;
            if (steps > RX_MAX_STEPS) {
                free(nvisited);
                rx_free_threads(clist, nclist);
                rx_free_threads(nlist, nnlist);
                nv_error(NV_ERR_RUNTIME, "تجاوز النمط حد الخطوات", line, col);
            }
            RxInst *in = &p->prog[clist[i].pc];
            if (rx_consumes(in, text[pos], p->fold)) {
                int adopted = rx_add_thread(p, &nlist, &nnlist, &capnlist, nvisited, clist[i].pc + 1, pos + 1,
                              text, tlen, clist[i].saved, nslots, &steps, line, col);
                if (!adopted) free(clist[i].saved);
            } else {
                free(clist[i].saved);
            }
        }
        free(nvisited);
        free(clist);
        clist = nlist;
        nclist = nnlist;
        capclist = capnlist;
        pos++;
        if (nclist == 0) return 0;
    }
}

/// أول مطابقة عند from أو بعده؛ 1 مع (s, e, خانات) أو 0.
static int rx_find_from(
    RxProg *p, uint32_t *text, int tlen, int from,
    int *s_out, int *e_out, int **saved_out, int line, int col) {
    for (int start = from; start <= tlen; start++) {
        int *saved = NULL;
        if (rx_match_at(p, text, tlen, start, &saved, line, col)) {
            *s_out = saved[0] >= 0 ? saved[0] : start;
            *e_out = saved[1] >= 0 ? saved[1] : start;
            *saved_out = saved;
            return 1;
        }
    }
    return 0;
}

/// مطابقة كاملة (مع ^ و$ ضمنيين).
static int rx_full_match_c(const char *pattern, const char *text, int line, int col) {
    size_t tlen = strlen(text);
    size_t nchars = nv_utf8_chars(text, tlen);
    if (nchars > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_c(pattern, 1, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    int *saved = NULL;
    int found = rx_match_at(&prog, chars, (int)n, 0, &saved, line, col);
    free(saved);
    free(chars);
    free(offs);
    rx_prog_free(&prog);
    return found;
}

/// نص مقطع [s, e) بالبايتات من إزاحات المحارف.
static NvValue rx_slice_str(const char *text, size_t *offs, int s, int e) {
    return nv_str_new(text + offs[s], offs[e] - offs[s]);
}

static const char *rx_group_slice(
    const char *text, size_t *offs, int *saved, int group, size_t *len_out) {
    int s = saved[2 * group];
    int e = saved[2 * group + 1];
    if (s < 0 || e < 0) {
        *len_out = 0;
        return NULL;
    }
    *len_out = offs[e] - offs[s];
    return text + offs[s];
}

// --- وحدة نمط: دوال أصلية ---

static NvValue nv_std_regex_match(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.طابق", args, nargs, 2, line, col);
    const char *pattern = nv_std_need_string("نمط.طابق", args[0], line, col);
    const char *text = nv_std_need_string("نمط.طابق", args[1], line, col);
    return nv_bool(rx_full_match_c(pattern, text, line, col));
}

static NvValue nv_std_regex_search(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.ابحث", args, nargs, 2, line, col);
    const char *pattern = nv_std_need_string("نمط.ابحث", args[0], line, col);
    const char *text = nv_std_need_string("نمط.ابحث", args[1], line, col);
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_c(pattern, 0, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    int s = 0, e = 0;
    int *saved = NULL;
    int found = rx_find_from(&prog, chars, (int)n, 0, &s, &e, &saved, line, col);
    NvValue out = nv_nil();
    if (found) out = rx_slice_str(text, offs, s, e);
    free(saved);
    free(chars);
    free(offs);
    rx_prog_free(&prog);
    return out;
}

static NvValue nv_std_regex_extract(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.استخرج", args, nargs, 2, line, col);
    const char *pattern = nv_std_need_string("نمط.استخرج", args[0], line, col);
    const char *text = nv_std_need_string("نمط.استخرج", args[1], line, col);
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_c(pattern, 0, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    int pos = 0;
    while (pos <= (int)n) {
        int s = 0, e = 0;
        int *saved = NULL;
        if (!rx_find_from(&prog, chars, (int)n, pos, &s, &e, &saved, line, col)) break;
        nv_list_push_raw(out.u.list, rx_slice_str(text, offs, s, e));
        free(saved);
        if (e == s) {
            if (e >= (int)n) break;
            pos = e + 1;
        } else {
            pos = e;
        }
    }
    nv_root_pop();
    free(chars);
    free(offs);
    rx_prog_free(&prog);
    return out;
}

static NvValue regex_replace_fold_c(const char *pattern, const char *text, const char *repl, int fold, int line, int col);

static NvValue nv_std_regex_replace(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.استبدل", args, nargs, 3, line, col);
    const char *pattern = nv_std_need_string("نمط.استبدل", args[0], line, col);
    const char *text = nv_std_need_string("نمط.استبدل", args[1], line, col);
    const char *repl = nv_std_need_string("نمط.استبدل", args[2], line, col);
    return regex_replace_fold_c(pattern, text, repl, 0, line, col);
}

/// محرك نمط.استبدل نفسه مع طيّ حالة اختياري — تعيد
/// بحث.استبدل_معاينة استعماله بلا محرك جديد.
static NvValue regex_replace_fold_c(const char *pattern, const char *text, const char *repl, int fold, int line, int col) {
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_fold_c(pattern, 0, fold, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    size_t rn = 0;
    size_t *roffs = NULL;
    uint32_t *rchars = j14_decode_all(repl, strlen(repl), &rn, &roffs);
    J14Buf out;
    j14_buf_init(&out);
    int pos = 0;
    while (pos <= (int)n) {
        int s = 0, e = 0;
        int *saved = NULL;
        if (!rx_find_from(&prog, chars, (int)n, pos, &s, &e, &saved, line, col)) break;
        j14_buf_addn(&out, text + offs[pos], offs[s] - offs[pos]);
        // توسعة البديل: $0 و$1..$9 و$$.
        for (size_t i = 0; i < rn; i++) {
            uint32_t rc = rchars[i];
            if (rc == '$' && i + 1 < rn) {
                uint32_t nx = rchars[i + 1];
                if (nx == '$') {
                    j14_buf_addc(&out, '$');
                    i++;
                    continue;
                }
                if (nx >= '0' && nx <= '9') {
                    int g = (int)(nx - '0');
                    if (g == 0) {
                        j14_buf_addn(&out, text + offs[s], offs[e] - offs[s]);
                    } else if (g <= prog.ngroups) {
                        size_t glen = 0;
                        const char *gs = rx_group_slice(text, offs, saved, g, &glen);
                        if (gs) j14_buf_addn(&out, gs, glen);
                    }
                    i++;
                    continue;
                }
            }
            j14_buf_add_cp(&out, rc);
        }
        free(saved);
        if (nv_utf8_chars(out.data, out.len) > RX_MAX_TEXT) {
            j14_buf_free(&out);
            free(chars);
            free(offs);
            free(rchars);
            free(roffs);
            rx_prog_free(&prog);
            nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
        }
        if (e == s) {
            if (e >= (int)n) {
                pos = e + 1;
                break;
            }
            j14_buf_addn(&out, text + offs[e], offs[e + 1] - offs[e]);
            pos = e + 1;
        } else {
            pos = e;
        }
    }
    if (pos <= (int)n) j14_buf_addn(&out, text + offs[pos], offs[n] - offs[pos]);
    NvValue result = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    free(chars);
    free(offs);
    free(rchars);
    free(roffs);
    rx_prog_free(&prog);
    return result;
}

static NvValue nv_std_regex_split(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.قسم", args, nargs, 2, line, col);
    const char *pattern = nv_std_need_string("نمط.قسم", args[0], line, col);
    const char *text = nv_std_need_string("نمط.قسم", args[1], line, col);
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_c(pattern, 0, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    int last = 0;
    int pos = 0;
    while (pos <= (int)n) {
        int s = 0, e = 0;
        int *saved = NULL;
        if (!rx_find_from(&prog, chars, (int)n, pos, &s, &e, &saved, line, col)) break;
        free(saved);
        if (s == e) {
            if (e >= (int)n) break;
            pos = e + 1;
        } else {
            nv_list_push_raw(out.u.list, rx_slice_str(text, offs, last, s));
            last = e;
            pos = e;
        }
    }
    nv_list_push_raw(out.u.list, rx_slice_str(text, offs, last, (int)n));
    nv_root_pop();
    free(chars);
    free(offs);
    rx_prog_free(&prog);
    return out;
}

static NvValue nv_std_regex_groups(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نمط.مجموعات", args, nargs, 2, line, col);
    const char *pattern = nv_std_need_string("نمط.مجموعات", args[0], line, col);
    const char *text = nv_std_need_string("نمط.مجموعات", args[1], line, col);
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    RxProg prog = rx_compile_c(pattern, 0, line, col);
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    int s = 0, e = 0;
    int *saved = NULL;
    int found = rx_find_from(&prog, chars, (int)n, 0, &s, &e, &saved, line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    if (found) {
        for (int g = 1; g <= prog.ngroups; g++) {
            size_t glen = 0;
            const char *gs = rx_group_slice(text, offs, saved, g, &glen);
            if (gs) nv_list_push_raw(out.u.list, nv_str_new(gs, glen));
            else nv_list_push_raw(out.u.list, nv_nil());
        }
    }
    nv_root_pop();
    free(saved);
    free(chars);
    free(offs);
    rx_prog_free(&prog);
    return out;
}

void nv_std_regex_fill(NvDict *d) {
    nv_std_add_fn(d, "طابق", nv_std_regex_match);
    nv_std_add_fn(d, "ابحث", nv_std_regex_search);
    nv_std_add_fn(d, "استخرج", nv_std_regex_extract);
    nv_std_add_fn(d, "استبدل", nv_std_regex_replace);
    nv_std_add_fn(d, "قسم", nv_std_regex_split);
    nv_std_add_fn(d, "مجموعات", nv_std_regex_groups);
}

// ---------- بحث: دفعة لينكس الأولى، الموجة الأولى §1 ----------
// البنود 1 و2 و3 و4 و5 و6 و7 و8 و18 و19 و20 و23 و27 و28.
// مرآة search_entries في stdlib.rs حرفيا (الشكل
// والسلوك والرسائل)؛ وتستعمل محرك التعابير هنا
// (rx_compile_fold_c وrx_find_from) بلا محرك جديد.
// العمود محرفي 1-based؛ وسجل واحد لكل سطر مطابق
// بالمطابقة الأولى المؤهلة فيه؛ ومع اعكس_المطابقة
// يحمل السجل عمود 1 ونص_مطابق فارغا؛ وقلم_النتائج
// يقلم السطر المعروض وحده (العمود على السطر الأصلي).
// مفاتيح مخزنة بلا شدة ولا تطويل (قاعدة نبّه).
// ---------- الموجة الأولى §2 (البنود 30 و36–42 و44
// و45 و48 و50 و51 و55 و57 و68): امتداد على الشكل
// نفسه — سياق قبل/بعد وحقول قبل وبعد في السجلات؛
// ورقم_السطر صيغة سجلات بلا عمود؛ وجسون عبر
// json_write_value_c القائم؛ ولوّن بالأحمر ANSI
// القياسي (\x1b[31m ... \x1b[0m) إذ لا دعم ANSI
// قائم في المحرك؛ واستبدل_معاينة بإعادة استعمال
// regex_replace_fold_c؛ والبحث في الملفات باجتياز
// يحترم أقصى_عمق ويتخطى الثنائي (بايت صفر أو ليس
// UTF-8 صالحا) وغير المقروء والروابط الرمزية.

typedef struct {
    int ignore_case;
    int case_sensitive;
    int smart_case;
    int whole_word;
    int whole_line;
    int invert;
    long long max_count; // -1 بلا حد
    int column;
    int trim;
    int quiet;
    long long before; // -1 بلا تحديد
    long long after; // -1 بلا تحديد
    long long context; // -1 بلا تحديد
    int line_number;
    int show_filename;
    int json;
    int color;
    long long max_depth; // -1 بلا حد
} SearchOptsC;

static int search_need_bool_c(const char *display, const char *key, NvValue v, int line, int col) {
    if (v.tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قيمة منطقية للخيار %s لكن أعطيت %s", display, key, nv_type_name(v));
    }
    return v.u.boolean;
}

static long long search_need_count_c(const char *display, const char *key, NvValue v, int line, int col) {
    double x = nv_std_need_number(display, v, line, col);
    if (x != trunc(x) || x < -9.0e15 || x >= 9.0e15) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا", display);
    }
    long long n = (long long)x;
    if (n < 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s الخيار %s لا يقبل عددا سالبا", display, key);
    }
    return n;
}

static void search_parse_opts_dict_c(const char *display, NvValue opts, SearchOptsC *o, int line, int col) {
    if (opts.tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s", display, nv_type_name(opts));
    }
    NvDict *d = opts.u.dict;
    for (size_t i = 0; i < d->len; i++) {
        const char *key = d->entries[i].key;
        NvValue v = d->entries[i].val;
        if (strcmp(key, "تجاهل_الحالة") == 0) o->ignore_case = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "حساسية_الحالة") == 0) o->case_sensitive = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "حالة_ذكية") == 0) o->smart_case = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "كلمة_كاملة") == 0) o->whole_word = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "سطر_كامل") == 0) o->whole_line = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "اعكس_المطابقة") == 0) o->invert = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "عمود_المطابقة") == 0) o->column = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "قلم_النتائج") == 0) o->trim = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "صامت") == 0) o->quiet = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "أقصى_مطابقات") == 0) o->max_count = search_need_count_c(display, key, v, line, col);
        else if (strcmp(key, "سياق_قبل") == 0) o->before = search_need_count_c(display, key, v, line, col);
        else if (strcmp(key, "سياق_بعد") == 0) o->after = search_need_count_c(display, key, v, line, col);
        else if (strcmp(key, "سياق") == 0) o->context = search_need_count_c(display, key, v, line, col);
        else if (strcmp(key, "رقم_السطر") == 0) o->line_number = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "أظهر_اسم_الملف") == 0) o->show_filename = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "جسون") == 0) o->json = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "لوّن") == 0) o->color = search_need_bool_c(display, key, v, line, col);
        else if (strcmp(key, "أقصى_عمق") == 0) o->max_depth = search_need_count_c(display, key, v, line, col);
        else {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s خيار غير معروف: %s", display, key);
        }
    }
    int ncase = (o->ignore_case ? 1 : 0) + (o->case_sensitive ? 1 : 0) + (o->smart_case ? 1 : 0);
    if (ncase > 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s خيارات حالة متضاربة: اختر واحدا فقط من تجاهل_الحالة وحساسية_الحالة وحالة_ذكية", display);
    }
}

static void search_parse_opts_c(const char *display, NvValue *args, int nargs, int line, int col, SearchOptsC *o) {
    memset(o, 0, sizeof(*o));
    o->max_count = -1;
    o->before = -1;
    o->after = -1;
    o->context = -1;
    o->max_depth = -1;
    if (nargs < 2 || nargs > 3) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع معاملين أو ثلاثة لكن أعطيت %d", display, nargs);
    }
    if (nargs == 3) {
        search_parse_opts_dict_c(display, args[2], o, line, col);
    }
}

static void search_parse_opts_4_c(const char *display, NvValue *args, int nargs, int line, int col, SearchOptsC *o) {
    memset(o, 0, sizeof(*o));
    o->max_count = -1;
    o->before = -1;
    o->after = -1;
    o->context = -1;
    o->max_depth = -1;
    if (nargs < 3 || nargs > 4) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع ثلاثة معاملات أو أربعة لكن أعطيت %d", display, nargs);
    }
    if (nargs == 4) {
        search_parse_opts_dict_c(display, args[3], o, line, col);
    }
}

/// السياق الفعلي (قبل، بعد): المنفرد يغلب المجمع.
static void search_eff_context_c(SearchOptsC *o, long long *b, long long *a) {
    *b = o->before >= 0 ? o->before : (o->context >= 0 ? o->context : 0);
    *a = o->after >= 0 ? o->after : (o->context >= 0 ? o->context : 0);
}

static int search_has_context_c(SearchOptsC *o) {
    long long b = 0, a = 0;
    search_eff_context_c(o, &b, &a);
    return b > 0 || a > 0;
}

static int search_record_form_c(SearchOptsC *o) {
    return o->column || o->line_number || search_has_context_c(o);
}

static int search_has_upper_c(uint32_t *pat, int plen) {
    for (int i = 0; i < plen; i++) {
        uint32_t c = pat[i];
        if ((c >= 'A' && c <= 'Z') || (c >= 0x00C0 && c <= 0x00DE && c != 0x00D7)) return 1;
    }
    return 0;
}

static int search_fold_c(uint32_t *pat, int plen, SearchOptsC *o) {
    if (o->ignore_case) return 1;
    if (o->smart_case) return !search_has_upper_c(pat, plen);
    return 0;
}

/// هل المدى [s, e) كلمة مستقلة (أو — للمطابقة الفارغة
/// — حد كلمة عند الموضع)؟ `len` طول السطر بالمحارف.
static int search_word_ok_c(uint32_t *chars, int s, int e, int len) {
    if (e > s) {
        int before_ok = s == 0 || !rx_is_word_cp(chars[s - 1]);
        int after_ok = e >= len || !rx_is_word_cp(chars[e]);
        return before_ok && after_ok;
    }
    int before = s > 0 && rx_is_word_cp(chars[s - 1]);
    int after = s < len && rx_is_word_cp(chars[s]);
    return before != after;
}

static int json_write_value_c(NvValue v, J14Buf *out, int depth);

typedef struct {
    uint32_t *chars;
    size_t *offs;
    size_t n;
    uint32_t *pchars;
    size_t *poffs;
    size_t pn;
    RxProg prog;
    int have_prog;
    int fold;
} SearchCompiledC;

static void search_compile_c(const char *text, int is_regex, const char *pattern, SearchOptsC *o, int line, int col, SearchCompiledC *sc) {
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    sc->chars = j14_decode_all(text, tlen, &sc->n, &sc->offs);
    size_t plen = strlen(pattern);
    sc->pchars = j14_decode_all(pattern, plen, &sc->pn, &sc->poffs);
    sc->fold = search_fold_c(sc->pchars, (int)sc->pn, o);
    sc->have_prog = 0;
    if (is_regex) {
        sc->prog = rx_compile_fold_c(pattern, o->whole_line, sc->fold, line, col);
        sc->have_prog = 1;
    }
}

static void search_compiled_free_c(SearchCompiledC *sc) {
    free(sc->chars);
    free(sc->offs);
    free(sc->pchars);
    free(sc->poffs);
    if (sc->have_prog) rx_prog_free(&sc->prog);
}

static void search_push_match_c(int **ss, int **ee, int *count, int *cap, int s, int e, int line, int col) {
    if (*count >= *cap) {
        int ncap = *cap == 0 ? 4 : *cap * 2;
        int *nss = (int *)realloc(*ss, (size_t)ncap * sizeof(int));
        int *nee = (int *)realloc(*ee, (size_t)ncap * sizeof(int));
        if (!nss || !nee) {
            free(*ss);
            free(*ee);
            nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
        }
        *ss = nss;
        *ee = nee;
        *cap = ncap;
    }
    (*ss)[*count] = s;
    (*ee)[*count] = e;
    (*count)++;
}

/// كل المطابقات المؤهلة في سطر (بداية، نهاية) — بلا
/// تداخل؛ والمطابقة الفارغة تتقدم محرفا واحدا؛ ومع
/// سطر_كامل مطابقة واحدة على الأكثر هي السطر كله.
/// يعيد العدد ويملأ مصفوفتين يحررهما المتصل.
static int search_line_all_c(SearchCompiledC *sc, SearchOptsC *o, int is_regex, uint32_t *chars, int len, int **ss_out, int **ee_out, int line, int col) {
    int *ss = NULL, *ee = NULL;
    int count = 0, cap = 0;
    if (o->whole_line) {
        if (is_regex) {
            int s = 0, e = 0;
            int *saved = NULL;
            int found = rx_find_from(&sc->prog, chars, len, 0, &s, &e, &saved, line, col);
            free(saved);
            if (found && s == 0 && e == len) {
                search_push_match_c(&ss, &ee, &count, &cap, s, e, line, col);
            }
        } else {
            int eq = (len == (int)sc->pn);
            for (int i = 0; eq && i < len; i++) {
                uint32_t a = chars[i], b = sc->pchars[i];
                if (sc->fold ? rx_fold_cp(a) != rx_fold_cp(b) : a != b) eq = 0;
            }
            if (eq && (!o->whole_word || search_word_ok_c(chars, 0, len, len))) {
                search_push_match_c(&ss, &ee, &count, &cap, 0, len, line, col);
            }
        }
        *ss_out = ss;
        *ee_out = ee;
        return count;
    }
    if (is_regex) {
        int pos = 0;
        while (pos <= len) {
            int s = 0, e = 0;
            int *saved = NULL;
            if (!rx_find_from(&sc->prog, chars, len, pos, &s, &e, &saved, line, col)) break;
            free(saved);
            if (!o->whole_word || search_word_ok_c(chars, s, e, len)) {
                search_push_match_c(&ss, &ee, &count, &cap, s, e, line, col);
            }
            if (e == s) {
                if (e >= len) break;
                pos = e + 1;
            } else {
                pos = e;
            }
        }
    } else {
        int litlen = (int)sc->pn;
        if (litlen == 0) {
            for (int p = 0; p <= len; p++) {
                if (!o->whole_word || search_word_ok_c(chars, p, p, len)) {
                    search_push_match_c(&ss, &ee, &count, &cap, p, p, line, col);
                }
            }
        } else if (litlen <= len) {
            int p = 0;
            while (p + litlen <= len) {
                int ok = 1;
                for (int i = 0; i < litlen; i++) {
                    uint32_t a = chars[p + i], b = sc->pchars[i];
                    if (sc->fold ? rx_fold_cp(a) != rx_fold_cp(b) : a != b) {
                        ok = 0;
                        break;
                    }
                }
                if (ok && (!o->whole_word || search_word_ok_c(chars, p, p + litlen, len))) {
                    search_push_match_c(&ss, &ee, &count, &cap, p, p + litlen, line, col);
                    p += litlen;
                } else {
                    p += 1;
                }
            }
        }
    }
    *ss_out = ss;
    *ee_out = ee;
    return count;
}

/// تقليم المسافات من طرفي المدى [ls, le) في مصفوفة
/// المحارف الكاملة؛ يعيد المدى المقلَّم.
static void search_trim_span_c(uint32_t *chars, int ls, int le, int *ds, int *de) {
    int s = ls, e = le;
    while (s < e && (chars[s] == ' ' || chars[s] == '\t')) s++;
    while (e > s && (chars[e - 1] == ' ' || chars[e - 1] == '\t')) e--;
    *ds = s;
    *de = e;
}

/// سطر معروض مع تغليف المطابقة بالأحمر (ANSI
/// القياسي)؛ والمدى (s, e) نسبي إلى بداية السطر
/// المعروض ds.
static NvValue search_color_line_c(const char *text, size_t *offs, int ds, int de, int s, int e) {
    J14Buf out;
    j14_buf_init(&out);
    j14_buf_addn(&out, text + offs[ds], offs[ds + s] - offs[ds]);
    j14_buf_add(&out, "\x1b[31m");
    j14_buf_addn(&out, text + offs[ds + s], offs[ds + e] - offs[ds + s]);
    j14_buf_add(&out, "\x1b[0m");
    j14_buf_addn(&out, text + offs[ds + e], offs[de] - offs[ds + e]);
    NvValue r = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return r;
}

static NvValue search_maybe_json_c(NvValue value, SearchOptsC *o, const char *display, int line, int col) {
    if (!o->json) return value;
    J14Buf out;
    j14_buf_init(&out);
    if (!json_write_value_c(value, &out, 0)) {
        j14_buf_free(&out);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s قيمة لا تقبل كتابة JSON", display);
    }
    NvValue r = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return r;
}

static NvValue search_run_c(const char *display, const char *text, int is_regex, const char *pattern, SearchOptsC *o, int line, int col) {
    SearchCompiledC sc;
    search_compile_c(text, is_regex, pattern, o, line, col, &sc);
    uint32_t *chars = sc.chars;
    size_t *offs = sc.offs;
    int total = (int)sc.n;
    // جدول الأسطر (بداية، نهاية) — النهاية بلا \r.
    int *ls_arr = NULL, *le_arr = NULL;
    int nlines = 0, lcap = 0;
    {
        int pos = 0;
        while (pos < total) {
            int ls = pos;
            int le = total;
            int next = total + 1;
            for (int i = pos; i < total; i++) {
                if (chars[i] == '\n') {
                    le = i;
                    next = i + 1;
                    break;
                }
            }
            if (le > ls && chars[le - 1] == '\r') le--;
            if (nlines >= lcap) {
                int ncap = lcap == 0 ? 16 : lcap * 2;
                int *nls = (int *)realloc(ls_arr, (size_t)ncap * sizeof(int));
                int *nle = (int *)realloc(le_arr, (size_t)ncap * sizeof(int));
                if (!nls || !nle) {
                    free(ls_arr);
                    free(le_arr);
                    search_compiled_free_c(&sc);
                    nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
                }
                ls_arr = nls;
                le_arr = nle;
                lcap = ncap;
            }
            ls_arr[nlines] = ls;
            le_arr[nlines] = le;
            nlines++;
            if (next > total) break;
            pos = next;
        }
    }
    long long ctx_b = 0, ctx_a = 0;
    search_eff_context_c(o, &ctx_b, &ctx_a);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    long long count = 0;
    for (int idx = 0; idx < nlines; idx++) {
        if (o->max_count >= 0 && count >= o->max_count) break;
        int ls = ls_arr[idx], le = le_arr[idx];
        int llen = le - ls;
        int *ss = NULL, *ee = NULL;
        int nmatch = search_line_all_c(&sc, o, is_regex, chars + ls, llen, &ss, &ee, line, col);
        int has = nmatch > 0;
        if (has != o->invert) {
            count++;
            int ds = ls, de = le;
            if (o->trim) search_trim_span_c(chars, ls, le, &ds, &de);
            int s = 0, e = 0;
            int has_span = 0;
            if (!o->invert && nmatch > 0) {
                s = ss[0];
                e = ee[0];
                has_span = 1;
            }
            // المطابقة الأولى في السطر المعروض (للتلوين).
            int cs = 0, ce = 0;
            int has_cspan = 0;
            if (o->color && !o->invert && nmatch > 0) {
                int *css = NULL, *cee = NULL;
                int cn = search_line_all_c(&sc, o, is_regex, chars + ds, de - ds, &css, &cee, line, col);
                if (cn > 0) {
                    cs = css[0];
                    ce = cee[0];
                    has_cspan = 1;
                }
                free(css);
                free(cee);
            }
            NvValue line_val;
            if (o->color && has_cspan) {
                line_val = search_color_line_c(text, offs, ds, de, cs, ce);
            } else {
                line_val = rx_slice_str(text, offs, ds, de);
            }
            if (search_record_form_c(o)) {
                size_t didx = nv_root_push_idx();
                NvValue rec = nv_dict_new();
                *nv_root_slot(didx) = rec;
                nv_dict_set_raw(rec.u.dict, "سطر", line_val);
                nv_dict_set_raw(rec.u.dict, "رقم_السطر", nv_num((double)(idx + 1)));
                if (o->column) {
                    nv_dict_set_raw(rec.u.dict, "عمود", nv_num((double)(has_span ? s + 1 : 1)));
                }
                if (o->invert) {
                    nv_dict_set_raw(rec.u.dict, "نص_مطابق", nv_str_cstr(""));
                } else if (has_span) {
                    nv_dict_set_raw(rec.u.dict, "نص_مطابق", rx_slice_str(text, offs, ls + s, ls + e));
                } else {
                    nv_dict_set_raw(rec.u.dict, "نص_مطابق", nv_str_cstr(""));
                }
                if (search_has_context_c(o)) {
                    size_t bidx = nv_root_push_idx();
                    NvValue before = nv_list_new();
                    *nv_root_slot(bidx) = before;
                    int bfrom = idx - (int)ctx_b;
                    if (bfrom < 0) bfrom = 0;
                    for (int j = bfrom; j < idx; j++) {
                        int cds = ls_arr[j], cde = le_arr[j];
                        if (o->trim) search_trim_span_c(chars, ls_arr[j], le_arr[j], &cds, &cde);
                        nv_list_push_raw(before.u.list, rx_slice_str(text, offs, cds, cde));
                    }
                    nv_dict_set_raw(rec.u.dict, "قبل", before);
                    nv_root_pop();
                    size_t aidx = nv_root_push_idx();
                    NvValue after = nv_list_new();
                    *nv_root_slot(aidx) = after;
                    int ato = idx + 1 + (int)ctx_a;
                    if (ato > nlines) ato = nlines;
                    for (int j = idx + 1; j < ato; j++) {
                        int cds = ls_arr[j], cde = le_arr[j];
                        if (o->trim) search_trim_span_c(chars, ls_arr[j], le_arr[j], &cds, &cde);
                        nv_list_push_raw(after.u.list, rx_slice_str(text, offs, cds, cde));
                    }
                    nv_dict_set_raw(rec.u.dict, "بعد", after);
                    nv_root_pop();
                }
                nv_list_push_raw(out.u.list, rec);
                nv_root_pop();
            } else {
                nv_list_push_raw(out.u.list, line_val);
            }
        }
        free(ss);
        free(ee);
    }
    free(ls_arr);
    free(le_arr);
    // التسلسل إلى JSON — إن وُجد — يتم والقائمة ما
    // زالت مجذرة، ثم يُحرر الجذر وتُرجع النتيجة.
    NvValue result;
    if (o->quiet) {
        result = search_maybe_json_c(nv_bool(count > 0), o, display, line, col);
    } else {
        result = search_maybe_json_c(out, o, display, line, col);
    }
    search_compiled_free_c(&sc);
    nv_root_pop();
    return result;
}

/// العد: كل الأسطر أو كل المطابقات — مع العكس
/// (السطر غير المطابق يُحسب مرة واحدة في عد الكل)
/// والحد الأقصى على الأسطر المؤهلة.
static NvValue search_count_c(const char *display, const char *text, int is_regex, const char *pattern, SearchOptsC *o, int all_matches, int line, int col) {
    SearchCompiledC sc;
    search_compile_c(text, is_regex, pattern, o, line, col, &sc);
    uint32_t *chars = sc.chars;
    int total = (int)sc.n;
    long long total_count = 0;
    long long count = 0;
    int pos = 0;
    while (pos < total) {
        if (o->max_count >= 0 && count >= o->max_count) break;
        int ls = pos;
        int le = total;
        int next = total + 1;
        for (int i = pos; i < total; i++) {
            if (chars[i] == '\n') {
                le = i;
                next = i + 1;
                break;
            }
        }
        if (le > ls && chars[le - 1] == '\r') le--;
        int llen = le - ls;
        int *ss = NULL, *ee = NULL;
        int nmatch = search_line_all_c(&sc, o, is_regex, chars + ls, llen, &ss, &ee, line, col);
        free(ss);
        free(ee);
        int has = nmatch > 0;
        if (has != o->invert) {
            count++;
            if (all_matches) {
                total_count += o->invert ? 1 : nmatch;
            } else {
                total_count += 1;
            }
        }
        if (next > total) break;
        pos = next;
    }
    search_compiled_free_c(&sc);
    if (o->quiet) {
        return search_maybe_json_c(nv_bool(total_count > 0), o, display, line, col);
    }
    return search_maybe_json_c(nv_num((double)total_count), o, display, line, col);
}

/// كل النصوص المطابقة وحدها — بترتيب الأسطر ثم
/// مواضعها؛ ومع العكس لا مطابقة فالقائمة فارغة.
static NvValue search_only_c(const char *display, const char *text, int is_regex, const char *pattern, SearchOptsC *o, int line, int col) {
    SearchCompiledC sc;
    search_compile_c(text, is_regex, pattern, o, line, col, &sc);
    uint32_t *chars = sc.chars;
    size_t *offs = sc.offs;
    int total = (int)sc.n;
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    long long count = 0;
    int pos = 0;
    while (pos < total) {
        if (o->max_count >= 0 && count >= o->max_count) break;
        int ls = pos;
        int le = total;
        int next = total + 1;
        for (int i = pos; i < total; i++) {
            if (chars[i] == '\n') {
                le = i;
                next = i + 1;
                break;
            }
        }
        if (le > ls && chars[le - 1] == '\r') le--;
        int llen = le - ls;
        int *ss = NULL, *ee = NULL;
        int nmatch = search_line_all_c(&sc, o, is_regex, chars + ls, llen, &ss, &ee, line, col);
        int has = nmatch > 0;
        if (has != o->invert) {
            count++;
            if (!o->invert) {
                for (int k = 0; k < nmatch; k++) {
                    if (o->color) {
                        J14Buf cb;
                        j14_buf_init(&cb);
                        j14_buf_add(&cb, "\x1b[31m");
                        // إلحاق نص المطابقة بايت بايت من النص الأصلي.
                        j14_buf_addn(&cb, text + offs[ls + ss[k]], offs[ls + ee[k]] - offs[ls + ss[k]]);
                        j14_buf_add(&cb, "\x1b[0m");
                        NvValue cm = nv_str_new(cb.data, cb.len);
                        j14_buf_free(&cb);
                        nv_list_push_raw(out.u.list, cm);
                    } else {
                        nv_list_push_raw(out.u.list, rx_slice_str(text, offs, ls + ss[k], ls + ee[k]));
                    }
                }
            }
        }
        free(ss);
        free(ee);
        if (next > total) break;
        pos = next;
    }
    int found_any = out.u.list->len > 0;
    NvValue result;
    if (o->quiet) {
        result = search_maybe_json_c(nv_bool(found_any), o, display, line, col);
    } else {
        result = search_maybe_json_c(out, o, display, line, col);
    }
    search_compiled_free_c(&sc);
    nv_root_pop();
    return result;
}

static NvValue nv_std_search_regex(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c("بحث.ابحث_بنمط", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.ابحث_بنمط", args[0], line, col);
    const char *pattern = nv_std_need_string("بحث.ابحث_بنمط", args[1], line, col);
    return search_run_c("بحث.ابحث_بنمط", text, 1, pattern, &o, line, col);
}

static NvValue nv_std_search_literal(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c("بحث.ابحث_بنص_ثابت", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.ابحث_بنص_ثابت", args[0], line, col);
    const char *literal = nv_std_need_string("بحث.ابحث_بنص_ثابت", args[1], line, col);
    return search_run_c("بحث.ابحث_بنص_ثابت", text, 0, literal, &o, line, col);
}

static NvValue nv_std_search_patterns_from_file(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("بحث.أنماط_من_ملف", args, nargs, 1, line, col);
    const char *path = nv_std_need_string("بحث.أنماط_من_ملف", args[0], line, col);
    // المسار النسبي يُحل مقابل مجلد الجلسة (كما في
    // ملفات.افتح عبر nv_resolve_path) لا مقابل مجلد
    // العملية — فالجلسة تبدأ في البيت (home/).
    char *resolved = nv_resolve_path(path, line, col);
    FILE *fp = fopen(resolved, "rb");
    if (!fp) {
        free(resolved);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذر قراءة الملف %s", path);
    }
    free(resolved);
    char *buf = NULL;
    size_t len = 0, cap = 0;
    for (;;) {
        char chunk[4096];
        size_t m = fread(chunk, 1, sizeof(chunk), fp);
        if (m == 0) break;
        if (len + m + 1 > cap) {
            size_t ncap = cap == 0 ? 8192 : cap * 2;
            while (ncap < len + m + 1) ncap *= 2;
            char *grown = (char *)realloc(buf, ncap);
            if (!grown) {
                free(buf);
                fclose(fp);
                nv_error(NV_ERR_RUNTIME, "تعذر قراءة الملف", line, col);
            }
            buf = grown;
            cap = ncap;
        }
        memcpy(buf + len, chunk, m);
        len += m;
    }
    fclose(fp);
    if (!buf) {
        buf = strdup("");
        len = 0;
    }
    buf[len] = '\0';
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t start = 0;
    for (size_t i = 0; i <= len; i++) {
        if (i == len || buf[i] == '\n') {
            size_t e = i;
            if (e > start && buf[e - 1] == '\r') e--;
            if (e > start) {
                nv_list_push_raw(out.u.list, nv_str_new(buf + start, e - start));
            }
            start = i + 1;
        }
    }
    free(buf);
    nv_root_pop();
    return out;
}

static NvValue nv_std_search_count_lines(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c("بحث.عد_المطابقات", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.عد_المطابقات", args[0], line, col);
    const char *pattern = nv_std_need_string("بحث.عد_المطابقات", args[1], line, col);
    return search_count_c("بحث.عد_المطابقات", text, 1, pattern, &o, 0, line, col);
}

static NvValue nv_std_search_count_all(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c("بحث.عد_كل_المطابقات", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.عد_كل_المطابقات", args[0], line, col);
    const char *pattern = nv_std_need_string("بحث.عد_كل_المطابقات", args[1], line, col);
    return search_count_c("بحث.عد_كل_المطابقات", text, 1, pattern, &o, 1, line, col);
}

static NvValue nv_std_search_only(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c("بحث.المطابق_فقط", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.المطابق_فقط", args[0], line, col);
    const char *pattern = nv_std_need_string("بحث.المطابق_فقط", args[1], line, col);
    return search_only_c("بحث.المطابق_فقط", text, 1, pattern, &o, line, col);
}

static NvValue nv_std_search_replace_preview(NvValue *args, int nargs, int line, int col) {
    SearchOptsC o;
    search_parse_opts_4_c("بحث.استبدل_معاينة", args, nargs, line, col, &o);
    const char *text = nv_std_need_string("بحث.استبدل_معاينة", args[0], line, col);
    const char *pattern = nv_std_need_string("بحث.استبدل_معاينة", args[1], line, col);
    const char *repl = nv_std_need_string("بحث.استبدل_معاينة", args[2], line, col);
    size_t pn = 0;
    size_t *poffs = NULL;
    uint32_t *pchars = j14_decode_all(pattern, strlen(pattern), &pn, &poffs);
    int fold = search_fold_c(pchars, (int)pn, &o);
    free(pchars);
    free(poffs);
    // إعادة استعمال محرك نمط.استبدل نفسه مع طيّ
    // الحالة من الخيارات؛ وباقي الخيارات مقبولة
    // بلا أثر هنا (كما في مرآة Rust).
    return regex_replace_fold_c(pattern, text, repl, fold, line, col);
}

typedef struct {
    char **fs;
    char **disp;
    size_t len, cap;
} SearchFileListC;

static void search_files_push_c(SearchFileListC *l, const char *fs, const char *disp, int line, int col) {
    if (l->len >= l->cap) {
        size_t ncap = l->cap == 0 ? 16 : l->cap * 2;
        char **nfs = (char **)realloc(l->fs, ncap * sizeof(char *));
        char **nd = (char **)realloc(l->disp, ncap * sizeof(char *));
        if (!nfs || !nd) {
            free(l->fs);
            free(l->disp);
            nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
        }
        l->fs = nfs;
        l->disp = nd;
        l->cap = ncap;
    }
    l->fs[l->len] = strdup(fs);
    l->disp[l->len] = strdup(disp);
    if (!l->fs[l->len] || !l->disp[l->len]) {
        nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
    }
    l->len++;
}

static int search_name_cmp_c(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}

/// اجتياز مجلد تكراريا: كل ملف عادي؛ والروابط
/// الرمزية تُتخطى؛ والمجلد الفرعي غير المقروء
/// يُتخطى بصمت؛ والعمق 0 يعني ملفات المجلد الأعلى
/// وحدها؛ والعمق السالب بلا حد.
static void search_walk_dir_c(const char *dir_fs, const char *dir_display, long long depth, long long max_depth, SearchFileListC *out, int line, int col) {
    DIR *d = opendir(dir_fs);
    if (!d) return;
    char **names = NULL;
    size_t n = 0, cap = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        if (n >= cap) {
            size_t ncap = cap == 0 ? 16 : cap * 2;
            char **grown = (char **)realloc(names, ncap * sizeof(char *));
            if (!grown) {
                for (size_t i = 0; i < n; i++) free(names[i]);
                free(names);
                closedir(d);
                nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
            }
            names = grown;
            cap = ncap;
        }
        names[n] = strdup(e->d_name);
        if (!names[n]) {
            for (size_t i = 0; i < n; i++) free(names[i]);
            free(names);
            closedir(d);
            nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
        }
        n++;
    }
    closedir(d);
    qsort(names, n, sizeof(char *), search_name_cmp_c);
    for (size_t i = 0; i < n; i++) {
        size_t flen = strlen(dir_fs) + 1 + strlen(names[i]) + 1;
        char *child_fs = (char *)malloc(flen);
        snprintf(child_fs, flen, "%s/%s", dir_fs, names[i]);
        size_t dlen = dir_display[0] ? strlen(dir_display) + 1 + strlen(names[i]) + 1 : strlen(names[i]) + 1;
        char *child_disp = (char *)malloc(dlen);
        if (dir_display[0]) snprintf(child_disp, dlen, "%s/%s", dir_display, names[i]);
        else snprintf(child_disp, dlen, "%s", names[i]);
        struct stat st;
        int is_link = 0;
        if (nv_lstat(child_fs, &st) == 0) {
#ifdef S_ISLNK
            is_link = S_ISLNK(st.st_mode);
#endif
            if (!is_link) {
                if (S_ISDIR(st.st_mode)) {
                    if (max_depth < 0 || depth < max_depth) {
                        search_walk_dir_c(child_fs, child_disp, depth + 1, max_depth, out, line, col);
                    }
                } else if (S_ISREG(st.st_mode)) {
                    search_files_push_c(out, child_fs, child_disp, line, col);
                }
            }
        }
        free(child_fs);
        free(child_disp);
        free(names[i]);
    }
    free(names);
}

/// قراءة ملف كاملا؛ NULL عند تعذر القراءة.
static char *search_read_file_c(const char *fs_path, size_t *len_out) {
    FILE *fp = fopen(fs_path, "rb");
    if (!fp) return NULL;
    char *buf = NULL;
    size_t len = 0, cap = 0;
    for (;;) {
        char chunk[4096];
        size_t m = fread(chunk, 1, sizeof(chunk), fp);
        if (m == 0) break;
        if (len + m + 1 > cap) {
            size_t ncap = cap == 0 ? 8192 : cap * 2;
            while (ncap < len + m + 1) ncap *= 2;
            char *grown = (char *)realloc(buf, ncap);
            if (!grown) {
                free(buf);
                fclose(fp);
                return NULL;
            }
            buf = grown;
            cap = ncap;
        }
        memcpy(buf + len, chunk, m);
        len += m;
    }
    fclose(fp);
    if (!buf) {
        buf = strdup("");
        len = 0;
    }
    buf[len] = '\0';
    *len_out = len;
    return buf;
}

/// هل في النص مطابقة؟ (فحص ملف واحد؛ والحد الأقصى
/// صفر يعني لا مطابقة.)
static int search_text_has_match_c(const char *text, const char *pattern, SearchOptsC *o, int line, int col) {
    if (o->max_count == 0) return 0;
    SearchCompiledC sc;
    search_compile_c(text, 1, pattern, o, line, col, &sc);
    uint32_t *chars = sc.chars;
    int total = (int)sc.n;
    int found = 0;
    int pos = 0;
    while (pos < total) {
        int ls = pos;
        int le = total;
        int next = total + 1;
        for (int i = pos; i < total; i++) {
            if (chars[i] == '\n') {
                le = i;
                next = i + 1;
                break;
            }
        }
        if (le > ls && chars[le - 1] == '\r') le--;
        int *ss = NULL, *ee = NULL;
        int nmatch = search_line_all_c(&sc, o, 1, chars + ls, le - ls, &ss, &ee, line, col);
        free(ss);
        free(ee);
        if ((nmatch > 0) != o->invert) {
            found = 1;
            break;
        }
        if (next > total) break;
        pos = next;
    }
    search_compiled_free_c(&sc);
    return found;
}

static NvValue search_files_impl_c(NvValue *args, int nargs, int want_match, const char *display, int line, int col) {
    SearchOptsC o;
    search_parse_opts_c(display, args, nargs, line, col, &o);
    const char *path = nv_std_need_string(display, args[0], line, col);
    const char *pattern = nv_std_need_string(display, args[1], line, col);
    // التحقق من النمط أولا — فلو كانت كل الملفات
    // ثنائية أو غير مقروءة لما ظهر خطأ النمط أبدا.
    (void)search_text_has_match_c("", pattern, &o, line, col);
    char *resolved = nv_resolve_path(path, line, col);
    struct stat st;
    if (nv_lstat(resolved, &st) != 0) {
        free(resolved);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "المسار غير موجود: %s", path);
    }
    // قاعدة العرض: «.» بلا بادئة، وغيرها بلا شرطة أخيرة.
    char *base = strdup(path);
    size_t plen = strlen(base);
    while (plen > 0 && base[plen - 1] == '/') base[--plen] = '\0';
    if (strcmp(base, ".") == 0) base[0] = '\0';
    SearchFileListC list;
    memset(&list, 0, sizeof(list));
    if (S_ISREG(st.st_mode)) {
        search_files_push_c(&list, resolved, path, line, col);
    } else if (S_ISDIR(st.st_mode)) {
        search_walk_dir_c(resolved, base, 0, o.max_depth, &list, line, col);
    } else {
        free(resolved);
        free(base);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "المسار غير موجود: %s", path);
    }
    free(resolved);
    free(base);
    // فرز النتائج بالمسار المعروض (إدخال مستقر).
    for (size_t i = 1; i < list.len; i++) {
        char *kfs = list.fs[i];
        char *kd = list.disp[i];
        size_t j = i;
        while (j > 0 && strcmp(list.disp[j - 1], kd) > 0) {
            list.fs[j] = list.fs[j - 1];
            list.disp[j] = list.disp[j - 1];
            j--;
        }
        list.fs[j] = kfs;
        list.disp[j] = kd;
    }
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    for (size_t i = 0; i < list.len; i++) {
        size_t flen = 0;
        char *content = search_read_file_c(list.fs[i], &flen);
        int has = 0;
        int skip = 1;
        if (content) {
            // الثنائي (بايت صفر أو ليس UTF-8 صالحا)
            // وغير المقروء يُتخطى من القائمتين معا.
            if (!memchr(content, '\0', flen) && nv_std_file_valid_utf8((const unsigned char *)content, flen)) {
                has = search_text_has_match_c(content, pattern, &o, line, col);
                skip = 0;
            }
            free(content);
        }
        if (!skip && has == want_match) {
            nv_list_push_raw(out.u.list, nv_str_cstr(list.disp[i]));
        }
        free(list.fs[i]);
        free(list.disp[i]);
    }
    free(list.fs);
    free(list.disp);
    int found_any = out.u.list->len > 0;
    NvValue result;
    if (o.quiet) {
        result = search_maybe_json_c(nv_bool(found_any), &o, display, line, col);
    } else {
        result = search_maybe_json_c(out, &o, display, line, col);
    }
    nv_root_pop();
    return result;
}

static NvValue nv_std_search_files_with(NvValue *args, int nargs, int line, int col) {
    return search_files_impl_c(args, nargs, 1, "بحث.ملفات_فيها", line, col);
}

static NvValue nv_std_search_files_without(NvValue *args, int nargs, int line, int col) {
    return search_files_impl_c(args, nargs, 0, "بحث.ملفات_بلا_مطابقة", line, col);
}

typedef struct {
    NvValue v;
    char *file;
    double num;
} SearchSortItemC;

static NvValue nv_std_search_sort(NvValue *args, int nargs, int line, int col) {
    if (nargs < 1 || nargs > 2) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة بحث.رتب_النتائج تتوقع معاملا أو معاملين لكن أعطيت %d", nargs);
    }
    if (args[0].tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة بحث.رتب_النتائج تتوقع قائمة لكن أعطيت %s", nv_type_name(args[0]));
    }
    int descending = 0;
    if (nargs == 2) {
        if (args[1].tag != NV_STR) {
            nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع اتجاها «تصاعدي» أو «تنازلي»", line, col);
        }
        const char *dir = args[1].u.str->data;
        if (strcmp(dir, "تصاعدي") == 0) descending = 0;
        else if (strcmp(dir, "تنازلي") == 0) descending = 1;
        else nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع اتجاها «تصاعدي» أو «تنازلي»", line, col);
    }
    NvList *list = args[0].u.list;
    size_t n = list->len;
    if (n == 0) {
        return nv_list_new();
    }
    int first_is_dict = list->items[0].tag == NV_DICT;
    int first_is_str = list->items[0].tag == NV_STR;
    if (!first_is_dict && !first_is_str) {
        nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع قائمة سجلات أو قائمة نصوص", line, col);
    }
    SearchSortItemC *items = (SearchSortItemC *)malloc(n * sizeof(SearchSortItemC));
    if (!items) {
        nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
    }
    for (size_t i = 0; i < n; i++) {
        NvValue v = list->items[i];
        if (first_is_dict && v.tag != NV_DICT) {
            free(items);
            nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع عناصر من نوع واحد", line, col);
        }
        if (first_is_str && v.tag != NV_STR) {
            free(items);
            nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع عناصر من نوع واحد", line, col);
        }
        items[i].v = v;
        items[i].file = strdup("");
        items[i].num = 0.0;
        if (!items[i].file) {
            for (size_t j = 0; j < i; j++) free(items[j].file);
            free(items);
            nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
        }
        if (v.tag == NV_DICT) {
            NvDict *d = v.u.dict;
            int fi = nv_dict_find(d, "ملف");
            if (fi >= 0) {
                NvValue fv = d->entries[fi].val;
                if (fv.tag != NV_STR) {
                    for (size_t j = 0; j <= i; j++) free(items[j].file);
                    free(items);
                    nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع «ملف» نصا و«رقم_السطر» عددا في السجلات", line, col);
                }
                free(items[i].file);
                items[i].file = strdup(fv.u.str->data);
                if (!items[i].file) {
                    for (size_t j = 0; j < i; j++) free(items[j].file);
                    free(items);
                    nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
                }
            }
            int ni = nv_dict_find(d, "رقم_السطر");
            if (ni >= 0) {
                NvValue nv = d->entries[ni].val;
                if (nv.tag != NV_NUM) {
                    for (size_t j = 0; j <= i; j++) free(items[j].file);
                    free(items);
                    nv_error(NV_ERR_RUNTIME, "الدالة بحث.رتب_النتائج تتوقع «ملف» نصا و«رقم_السطر» عددا في السجلات", line, col);
                }
                items[i].num = nv.u.num;
            }
        } else {
            free(items[i].file);
            items[i].file = strdup(v.u.str->data);
            if (!items[i].file) {
                for (size_t j = 0; j < i; j++) free(items[j].file);
                free(items);
                nv_error(NV_ERR_RUNTIME, "تعذر تخصيص الذاكرة", line, col);
            }
        }
    }
    // فرز إدخال مستقر بالمفتاح (ملف ثم رقم_السطر).
    for (size_t i = 1; i < n; i++) {
        SearchSortItemC key = items[i];
        size_t j = i;
        while (j > 0) {
            int c = strcmp(items[j - 1].file, key.file);
            if (c == 0) {
                if (items[j - 1].num < key.num) c = -1;
                else if (items[j - 1].num > key.num) c = 1;
                else c = 0;
            }
            if (descending) c = -c;
            if (c <= 0) break;
            items[j] = items[j - 1];
            j--;
        }
        items[j] = key;
    }
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    for (size_t i = 0; i < n; i++) {
        nv_list_push_raw(out.u.list, items[i].v);
        free(items[i].file);
    }
    free(items);
    nv_root_pop();
    return out;
}

static const char SEARCH_HELP_C[] = "بحث — البحث في النصوص والملفات.\nالدوال: ابحث_بنمط(نص، نمط، خيارات؟)؛ ابحث_بنص_ثابت(نص، نص_ثابت، خيارات؟)؛ عد_المطابقات(نص، نمط، خيارات؟)؛ عد_كل_المطابقات(نص، نمط، خيارات؟)؛ المطابق_فقط(نص، نمط، خيارات؟)؛ استبدل_معاينة(نص، نمط، بديل، خيارات؟)؛ ملفات_فيها(مسار، نمط، خيارات؟)؛ ملفات_بلا_مطابقة(مسار، نمط، خيارات؟)؛ رتب_النتائج(نتائج، اتجاه؟)؛ أنماط_من_ملف(مسار)؛ مساعدة().\nالخيارات: تجاهل_الحالة وحساسية_الحالة وحالة_ذكية وكلمة_كاملة وسطر_كامل واعكس_المطابقة وأقصى_مطابقات وعمود_المطابقة ورقم_السطر وسياق_قبل وسياق_بعد وسياق وقلم_النتائج وصامت وأظهر_اسم_الملف وجسون ولوّن وأقصى_عمق.\nاتجاه رتب_النتائج: «تصاعدي» أو «تنازلي» والافتراضي «تصاعدي».\nأقصى_عمق 0 يعني المجلد الأعلى وحده؛ ولوّن يغلّف المطابق بالأحمر؛ والملفات الثنائية أو غير المقروءة تُتخطى.";

static NvValue nv_std_search_help(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("بحث.مساعدة", args, nargs, 0, line, col);
    return nv_str_cstr(SEARCH_HELP_C);
}

void nv_std_search_fill(NvDict *d) {
    nv_std_add_fn(d, "ابحث_بنمط", nv_std_search_regex);
    nv_std_add_fn(d, "ابحث_بنص_ثابت", nv_std_search_literal);
    nv_std_add_fn(d, "أنماط_من_ملف", nv_std_search_patterns_from_file);
    nv_std_add_fn(d, "عد_المطابقات", nv_std_search_count_lines);
    nv_std_add_fn(d, "عد_كل_المطابقات", nv_std_search_count_all);
    nv_std_add_fn(d, "المطابق_فقط", nv_std_search_only);
    nv_std_add_fn(d, "استبدل_معاينة", nv_std_search_replace_preview);
    nv_std_add_fn(d, "ملفات_فيها", nv_std_search_files_with);
    nv_std_add_fn(d, "ملفات_بلا_مطابقة", nv_std_search_files_without);
    nv_std_add_fn(d, "رتب_النتائج", nv_std_search_sort);
    nv_std_add_fn(d, "مساعدة", nv_std_search_help);
}

// ---------- 221: JSON (مرآة device.rs: محلل، بلا اعتماديات) ----------
// محلل JSON نفسه حرفيا (قبولا ورفضا)؛ وnull تصير لاشيء.
// أخطاء التحليل كلها «نص JSON غير صالح» (بلا تفاصيل).

typedef struct {
    const unsigned char *s;
    size_t len, pos;
} JsonParserC;

static void json_skip_ws(JsonParserC *p) {
    while (p->pos < p->len &&
           (p->s[p->pos] == ' ' || p->s[p->pos] == '\t' || p->s[p->pos] == '\n' ||
            p->s[p->pos] == '\r')) {
        p->pos++;
    }
}

static int json_expect(JsonParserC *p, unsigned char c) {
    if (p->pos < p->len && p->s[p->pos] == c) {
        p->pos++;
        return 1;
    }
    return 0;
}

static int json_hex4(JsonParserC *p, uint32_t *out) {
    uint32_t v = 0;
    for (int i = 0; i < 4; i++) {
        if (p->pos >= p->len) return 0;
        unsigned char c = p->s[p->pos++];
        uint32_t d;
        if (c >= '0' && c <= '9') d = (uint32_t)(c - '0');
        else if (c >= 'a' && c <= 'f') d = (uint32_t)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') d = (uint32_t)(c - 'A' + 10);
        else return 0;
        v = v * 16 + d;
    }
    *out = v;
    return 1;
}

static int json_parse_string_raw(JsonParserC *p, J14Buf *out) {
    if (!json_expect(p, '"')) return 0;
    for (;;) {
        if (p->pos >= p->len) return 0;
        unsigned char c = p->s[p->pos++];
        if (c == '"') return 1;
        if (c == '\\') {
            if (p->pos >= p->len) return 0;
            unsigned char e = p->s[p->pos++];
            switch (e) {
                case '"': j14_buf_addc(out, '"'); break;
                case '\\': j14_buf_addc(out, '\\'); break;
                case '/': j14_buf_addc(out, '/'); break;
                case 'b': j14_buf_addc(out, '\b'); break;
                case 'f': j14_buf_addc(out, '\f'); break;
                case 'n': j14_buf_addc(out, '\n'); break;
                case 'r': j14_buf_addc(out, '\r'); break;
                case 't': j14_buf_addc(out, '\t'); break;
                case 'u': {
                    uint32_t first = 0;
                    if (!json_hex4(p, &first)) return 0;
                    if (first >= 0xD800 && first < 0xDC00) {
                        if (p->pos + 1 >= p->len || p->s[p->pos] != '\\' ||
                            p->s[p->pos + 1] != 'u') {
                            return 0;
                        }
                        p->pos += 2;
                        uint32_t second = 0;
                        if (!json_hex4(p, &second)) return 0;
                        if (second < 0xDC00 || second >= 0xE000) return 0;
                        uint32_t code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        j14_buf_add_cp(out, code);
                    } else if (first >= 0xDC00 && first < 0xE000) {
                        return 0;
                    } else {
                        j14_buf_add_cp(out, first);
                    }
                    break;
                }
                default: return 0;
            }
        } else if (c < 0x20) {
            return 0;
        } else {
            j14_buf_addc(out, (char)c);
        }
    }
}

static int json_parse_value(JsonParserC *p, NvValue *out);

static int json_parse_number(JsonParserC *p, double *out) {
    size_t start = p->pos;
    if (p->pos < p->len && p->s[p->pos] == '-') p->pos++;
    if (p->pos < p->len && p->s[p->pos] == '0') {
        p->pos++;
    } else if (p->pos < p->len && p->s[p->pos] >= '1' && p->s[p->pos] <= '9') {
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
    char *tmp = j14_strndup((const char *)p->s + start, n);
    char *endp = NULL;
    double v = strtod(tmp, &endp);
    int ok = endp != NULL && *endp == '\0' && isfinite(v);
    free(tmp);
    if (!ok) return 0;
    *out = v;
    return 1;
}

static int json_literal(JsonParserC *p, const char *word) {
    size_t n = strlen(word);
    if (p->pos + n > p->len) return 0;
    if (memcmp(p->s + p->pos, word, n) != 0) return 0;
    p->pos += n;
    return 1;
}

static int json_parse_value(JsonParserC *p, NvValue *out) {
    json_skip_ws(p);
    if (p->pos >= p->len) return 0;
    unsigned char c = p->s[p->pos];
    if (c == '{') {
        p->pos++;
        size_t ridx = nv_root_push_idx();
        NvValue dict = nv_dict_new();
        *nv_root_slot(ridx) = dict;
        json_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == '}') {
            p->pos++;
            nv_root_pop();
            *out = dict;
            return 1;
        }
        for (;;) {
            json_skip_ws(p);
            if (p->pos >= p->len || p->s[p->pos] != '"') {
                nv_root_pop();
                return 0;
            }
            J14Buf key;
            j14_buf_init(&key);
            if (!json_parse_string_raw(p, &key)) {
                j14_buf_free(&key);
                nv_root_pop();
                return 0;
            }
            json_skip_ws(p);
            if (!json_expect(p, ':')) {
                j14_buf_free(&key);
                nv_root_pop();
                return 0;
            }
            NvValue val;
            if (!json_parse_value(p, &val)) {
                j14_buf_free(&key);
                nv_root_pop();
                return 0;
            }
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = val;
            nv_dict_set_raw(dict.u.dict, key.data, val);
            nv_root_pop();
            j14_buf_free(&key);
            json_skip_ws(p);
            if (p->pos >= p->len) {
                nv_root_pop();
                return 0;
            }
            unsigned char d = p->s[p->pos++];
            if (d == ',') continue;
            if (d == '}') break;
            nv_root_pop();
            return 0;
        }
        nv_root_pop();
        *out = dict;
        return 1;
    }
    if (c == '[') {
        p->pos++;
        size_t ridx = nv_root_push_idx();
        NvValue list = nv_list_new();
        *nv_root_slot(ridx) = list;
        json_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ']') {
            p->pos++;
            nv_root_pop();
            *out = list;
            return 1;
        }
        for (;;) {
            NvValue val;
            if (!json_parse_value(p, &val)) {
                nv_root_pop();
                return 0;
            }
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = val;
            nv_list_push_raw(list.u.list, val);
            nv_root_pop();
            json_skip_ws(p);
            if (p->pos >= p->len) {
                nv_root_pop();
                return 0;
            }
            unsigned char d = p->s[p->pos++];
            if (d == ',') continue;
            if (d == ']') break;
            nv_root_pop();
            return 0;
        }
        nv_root_pop();
        *out = list;
        return 1;
    }
    if (c == '"') {
        J14Buf buf;
        j14_buf_init(&buf);
        if (!json_parse_string_raw(p, &buf)) {
            j14_buf_free(&buf);
            return 0;
        }
        *out = nv_str_new(buf.data, buf.len);
        j14_buf_free(&buf);
        return 1;
    }
    if (c == 't') {
        if (!json_literal(p, "true")) return 0;
        *out = nv_bool(1);
        return 1;
    }
    if (c == 'f') {
        if (!json_literal(p, "false")) return 0;
        *out = nv_bool(0);
        return 1;
    }
    if (c == 'n') {
        if (!json_literal(p, "null")) return 0;
        *out = nv_nil();
        return 1;
    }
    if (c == '-' || (c >= '0' && c <= '9')) {
        double v = 0;
        if (!json_parse_number(p, &v)) return 0;
        *out = nv_num(v);
        return 1;
    }
    return 0;
}

static int json_parse_doc(const char *text, NvValue *out) {
    JsonParserC p;
    p.s = (const unsigned char *)text;
    p.len = strlen(text);
    p.pos = 0;
    if (!json_parse_value(&p, out)) return 0;
    json_skip_ws(&p);
    if (p.pos != p.len) return 0;
    return 1;
}

/// تهريب نص إلى سلسلة JSON (كما json_escape في Rust).
static void json_escape_into(J14Buf *out, const char *s, size_t len) {
    j14_buf_addc(out, '"');
    size_t i = 0;
    while (i < len) {
        uint32_t cp = 0;
        size_t adv = j14_utf8_decode((const unsigned char *)s + i, len - i, &cp);
        if (adv == 0) adv = 1;
        switch (cp) {
            case '"': j14_buf_add(out, "\\\""); break;
            case '\\': j14_buf_add(out, "\\\\"); break;
            case '\n': j14_buf_add(out, "\\n"); break;
            case '\r': j14_buf_add(out, "\\r"); break;
            case '\t': j14_buf_add(out, "\\t"); break;
            default:
                if (cp < 0x20) {
                    char tmp[8];
                    snprintf(tmp, sizeof(tmp), "\\u%04x", cp);
                    j14_buf_add(out, tmp);
                } else {
                    j14_buf_addn(out, s + i, adv);
                }
                break;
        }
        i += adv;
    }
    j14_buf_addc(out, '"');
}

/// تنسيق عدد بصيغة JSON الغربية الأقصر (كما Rust Display).
static void json_fmt_num_c(double n, J14Buf *out) {
    if (n == trunc(n) && fabs(n) < 9.0e15) {
        char tmp[32];
        snprintf(tmp, sizeof(tmp), "%lld", (long long)n);
        j14_buf_add(out, tmp);
        return;
    }
    char cand[64];
    char *use = NULL;
    for (int prec = 15; prec <= 17; prec++) {
        snprintf(cand, sizeof(cand), "%.*g", prec, n);
        if (strtod(cand, NULL) == n) {
            use = cand;
            break;
        }
    }
    if (!use) {
        snprintf(cand, sizeof(cand), "%.17g", n);
        use = cand;
    }
    // التدوين العلمي ← تدوين عشري عادي (كما Rust Display).
    char *epos = strchr(use, 'e');
    if (!epos) {
        j14_buf_add(out, use);
        return;
    }
    int expv = atoi(epos + 1);
    int neg = (use[0] == '-');
    const char *digits = use + (neg ? 1 : 0);
    size_t dlen = (size_t)(epos - digits);
    J14Buf mant;
    j14_buf_init(&mant);
    for (size_t i = 0; i < dlen; i++) {
        if (digits[i] != '.') j14_buf_addc(&mant, digits[i]);
    }
    // موضع النقطة العشرية في الأرقام.
    int dot = 0;
    for (size_t i = 0; i < dlen; i++) {
        if (digits[i] == '.') {
            dot = (int)i;
            break;
        }
        dot = (int)i + 1;
    }
    int point = dot + expv; // عدد الأرقام قبل النقطة
    if (neg) j14_buf_addc(out, '-');
    if (point <= 0) {
        j14_buf_add(out, "0.");
        for (int i = 0; i < -point; i++) j14_buf_addc(out, '0');
        j14_buf_addn(out, mant.data, mant.len);
    } else if ((size_t)point >= mant.len) {
        j14_buf_addn(out, mant.data, mant.len);
        for (size_t i = mant.len; i < (size_t)point; i++) j14_buf_addc(out, '0');
    } else {
        j14_buf_addn(out, mant.data, (size_t)point);
        j14_buf_addc(out, '.');
        j14_buf_addn(out, mant.data + point, mant.len - (size_t)point);
    }
    j14_buf_free(&mant);
}

static int json_write_value_c(NvValue v, J14Buf *out, int depth) {
    if (depth > 100) return 0;
    switch (v.tag) {
        case NV_NIL:
            j14_buf_add(out, "null");
            return 1;
        case NV_BOOL:
            j14_buf_add(out, v.u.boolean ? "true" : "false");
            return 1;
        case NV_NUM:
            if (!isfinite(v.u.num)) return 0;
            json_fmt_num_c(v.u.num, out);
            return 1;
        case NV_STR:
            json_escape_into(out, v.u.str->data, v.u.str->len);
            return 1;
        case NV_LIST: {
            j14_buf_addc(out, '[');
            for (size_t i = 0; i < v.u.list->len; i++) {
                if (i > 0) j14_buf_addc(out, ',');
                if (!json_write_value_c(v.u.list->items[i], out, depth + 1)) return 0;
            }
            j14_buf_addc(out, ']');
            return 1;
        }
        case NV_DICT: {
            j14_buf_addc(out, '{');
            for (size_t i = 0; i < v.u.dict->len; i++) {
                if (i > 0) j14_buf_addc(out, ',');
                json_escape_into(out, v.u.dict->entries[i].key, strlen(v.u.dict->entries[i].key));
                j14_buf_addc(out, ':');
                if (!json_write_value_c(v.u.dict->entries[i].val, out, depth + 1)) return 0;
            }
            j14_buf_addc(out, '}');
            return 1;
        }
        default:
            return 0;
    }
}

static NvValue nv_std_json_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("جسون.اقرأ", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("جسون.اقرأ", args[0], line, col);
    if (nv_utf8_chars(text, strlen(text)) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    NvValue out;
    if (!json_parse_doc(text, &out)) {
        nv_error(NV_ERR_RUNTIME, "نص JSON غير صالح", line, col);
    }
    return out;
}

static NvValue nv_std_json_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("جسون.اكتب", args, nargs, 1, line, col);
    J14Buf out;
    j14_buf_init(&out);
    if (!json_write_value_c(args[0], &out, 0)) {
        j14_buf_free(&out);
        nv_error(NV_ERR_RUNTIME, "قيمة لا تقبل كتابة JSON", line, col);
    }
    NvValue result = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return result;
}

static NvValue nv_std_json_valid(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("جسون.صالح", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("جسون.صالح", args[0], line, col);
    if (nv_utf8_chars(text, strlen(text)) > RX_MAX_TEXT) {
        return nv_bool(0);
    }
    NvValue tmp;
    int ok = json_parse_doc(text, &tmp);
    return nv_bool(ok);
}

void nv_std_json_fill(NvDict *d) {
    nv_std_add_fn(d, "اقرأ", nv_std_json_read);
    nv_std_add_fn(d, "اكتب", nv_std_json_write);
    nv_std_add_fn(d, "صالح", nv_std_json_valid);
}

// ---------- 222: XML (مرآة Rust حرفيا) ----------

typedef struct XmlNodeC XmlNodeC;
struct XmlNodeC {
    char *name;
    char **akeys;
    char **avals;
    int nattrs;
    XmlNodeC **children;
    int nchildren;
    J14Buf text;
};

typedef struct {
    uint32_t *chars;
    int len, pos;
} XmlParserC;

static int xml_name_start_cp(uint32_t c) {
    if (c == '_' || c == ':') return 1;
    if (c < 0x80) return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
    // عربي ولاتيني موسّع ونحوه: أي محرف أبجدي تقريبا.
    if (c >= 0x0600 && c <= 0x06FF) return 1;
    if (c >= 0x0750 && c <= 0x077F) return 1;
    if (c >= 0x08A0 && c <= 0x08FF) return 1;
    if (c >= 0x00C0 && c <= 0x02FF) return 1;
    if (c >= 0x0370 && c <= 0x03FF) return 1;
    if (c >= 0x0400 && c <= 0x04FF) return 1;
    if (c >= 0x4E00 && c <= 0x9FFF) return 1;
    if (c >= 0x3040 && c <= 0x30FF) return 1;
    if (c >= 0xAC00 && c <= 0xD7AF) return 1;
    return 0;
}

static int xml_name_char_cp(uint32_t c) {
    if (xml_name_start_cp(c)) return 1;
    if (c >= '0' && c <= '9') return 1;
    if (c >= 0x0660 && c <= 0x0669) return 1;
    if (c >= 0x06F0 && c <= 0x06F9) return 1;
    return c == '-' || c == '.';
}

static int xml_peek(XmlParserC *p) {
    if (p->pos >= p->len) return -1;
    return (int)p->chars[p->pos];
}

static int xml_starts_with(XmlParserC *p, const char *s) {
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *want = j14_decode_all(s, strlen(s), &n, &offs);
    int ok = 1;
    if (p->pos + (int)n > p->len) ok = 0;
    else {
        for (size_t i = 0; i < n; i++) {
            if (p->chars[p->pos + (int)i] != want[i]) {
                ok = 0;
                break;
            }
        }
    }
    free(want);
    free(offs);
    return ok;
}

static int xml_eat(XmlParserC *p, const char *s) {
    if (!xml_starts_with(p, s)) return 0;
    p->pos += (int)nv_utf8_chars(s, strlen(s));
    return 1;
}

static void xml_skip_ws(XmlParserC *p) {
    while (p->pos < p->len &&
           (p->chars[p->pos] == ' ' || p->chars[p->pos] == '\t' || p->chars[p->pos] == '\n' ||
            p->chars[p->pos] == '\r')) {
        p->pos++;
    }
}

static int xml_expect(XmlParserC *p, uint32_t c) {
    if (p->pos < p->len && p->chars[p->pos] == c) {
        p->pos++;
        return 1;
    }
    return 0;
}

static void xml_skip_comment(XmlParserC *p, int line, int col) {
    for (;;) {
        if (xml_starts_with(p, "-->")) {
            p->pos += 3;
            return;
        }
        if (p->pos >= p->len) nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        p->pos++;
    }
}

static void xml_skip_pi(XmlParserC *p, int line, int col) {
    for (;;) {
        if (p->pos >= p->len) nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        if (p->chars[p->pos] == '?' && p->pos + 1 < p->len && p->chars[p->pos + 1] == '>') {
            p->pos += 2;
            return;
        }
        p->pos++;
    }
}

static void xml_skip_doctype(XmlParserC *p, int line, int col) {
    p->pos += 9; // "<!DOCTYPE"
    int depth = 0;
    while (p->pos < p->len) {
        uint32_t c = p->chars[p->pos++];
        if (c == '[') depth++;
        else if (c == ']') {
            if (depth > 0) depth--;
        } else if (c == '>' && depth == 0) {
            return;
        }
    }
    nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
}

static char *xml_parse_name(XmlParserC *p, int line, int col) {
    int start = p->pos;
    if (p->pos >= p->len || !xml_name_start_cp(p->chars[p->pos])) {
        nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
    }
    p->pos++;
    while (p->pos < p->len && xml_name_char_cp(p->chars[p->pos])) p->pos++;
    J14Buf out;
    j14_buf_init(&out);
    for (int i = start; i < p->pos; i++) j14_buf_add_cp(&out, p->chars[i]);
    char *name = j14_strdup(out.data);
    j14_buf_free(&out);
    return name;
}

/// فك كيان عند '&' (وقد استُهلك)؛ يعيد النص المفكوك.
static void xml_decode_entity(XmlParserC *p, J14Buf *out, int line, int col) {
    J14Buf body;
    j14_buf_init(&body);
    for (;;) {
        if (p->pos >= p->len) {
            j14_buf_free(&body);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        uint32_t c = p->chars[p->pos++];
        if (c == ';') break;
        j14_buf_add_cp(&body, c);
        if (body.len > 12) {
            j14_buf_free(&body);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
    }
    if (strcmp(body.data, "amp") == 0) j14_buf_addc(out, '&');
    else if (strcmp(body.data, "lt") == 0) j14_buf_addc(out, '<');
    else if (strcmp(body.data, "gt") == 0) j14_buf_addc(out, '>');
    else if (strcmp(body.data, "quot") == 0) j14_buf_addc(out, '"');
    else if (strcmp(body.data, "apos") == 0) j14_buf_addc(out, '\'');
    else if (body.data[0] == '#' && (body.data[1] == 'x' || body.data[1] == 'X')) {
        char *endp = NULL;
        long v = strtol(body.data + 2, &endp, 16);
        if (endp == NULL || *endp != '\0' || v <= 0 || v > 0x10FFFF ||
            (v >= 0xD800 && v < 0xE000)) {
            j14_buf_free(&body);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        j14_buf_add_cp(out, (uint32_t)v);
    } else if (body.data[0] == '#') {
        char *endp = NULL;
        long v = strtol(body.data + 1, &endp, 10);
        if (endp == NULL || *endp != '\0' || v <= 0 || v > 0x10FFFF ||
            (v >= 0xD800 && v < 0xE000)) {
            j14_buf_free(&body);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        j14_buf_add_cp(out, (uint32_t)v);
    } else {
        j14_buf_free(&body);
        nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
    }
    j14_buf_free(&body);
}

static void xml_read_text(XmlParserC *p, J14Buf *out, int line, int col) {
    while (p->pos < p->len && p->chars[p->pos] != '<') {
        uint32_t c = p->chars[p->pos++];
        if (c == '&') xml_decode_entity(p, out, line, col);
        else j14_buf_add_cp(out, c);
    }
}

static XmlNodeC *xml_parse_element(XmlParserC *p, int depth, int line, int col);

static XmlNodeC *xml_node_new(void) {
    XmlNodeC *n = (XmlNodeC *)calloc(1, sizeof(XmlNodeC));
    j14_buf_init(&n->text);
    return n;
}

static void xml_node_free(XmlNodeC *n) {
    if (!n) return;
    free(n->name);
    for (int i = 0; i < n->nattrs; i++) {
        free(n->akeys[i]);
        free(n->avals[i]);
    }
    free(n->akeys);
    free(n->avals);
    for (int i = 0; i < n->nchildren; i++) xml_node_free(n->children[i]);
    free(n->children);
    j14_buf_free(&n->text);
    free(n);
}

static XmlNodeC *xml_parse_element(XmlParserC *p, int depth, int line, int col) {
    if (depth > 100) nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
    if (!xml_eat(p, "<")) nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
    XmlNodeC *node = xml_node_new();
    node->name = xml_parse_name(p, line, col);
    for (;;) {
        xml_skip_ws(p);
        int peek = xml_peek(p);
        if (peek == '>') {
            p->pos++;
            break;
        }
        if (peek == '/') {
            if (xml_eat(p, "/>")) return node;
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        if (peek < 0) {
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        char *aname = xml_parse_name(p, line, col);
        for (int i = 0; i < node->nattrs; i++) {
            if (strcmp(node->akeys[i], aname) == 0) {
                free(aname);
                xml_node_free(node);
                nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
            }
        }
        xml_skip_ws(p);
        if (!xml_expect(p, '=')) {
            free(aname);
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        xml_skip_ws(p);
        int q = xml_peek(p);
        if (q != '"' && q != '\'') {
            free(aname);
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        p->pos++;
        J14Buf value;
        j14_buf_init(&value);
        int closed = 0;
        while (p->pos < p->len) {
            uint32_t c = p->chars[p->pos++];
            if ((int)c == q) {
                closed = 1;
                break;
            }
            if (c == '&') xml_decode_entity(p, &value, line, col);
            else if (c == '<') {
                j14_buf_free(&value);
                free(aname);
                xml_node_free(node);
                nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
            } else {
                j14_buf_add_cp(&value, c);
            }
        }
        if (!closed) {
            j14_buf_free(&value);
            free(aname);
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        node->akeys = (char **)realloc(node->akeys, (node->nattrs + 1) * sizeof(char *));
        node->avals = (char **)realloc(node->avals, (node->nattrs + 1) * sizeof(char *));
        node->akeys[node->nattrs] = aname;
        node->avals[node->nattrs] = j14_strdup(value.data);
        node->nattrs++;
        j14_buf_free(&value);
        if (node->nattrs > 100) {
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
    }
    for (;;) {
        if (p->pos >= p->len) {
            xml_node_free(node);
            nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
        }
        if (xml_starts_with(p, "</")) {
            p->pos += 2;
            char *end = xml_parse_name(p, line, col);
            if (strcmp(end, node->name) != 0) {
                free(end);
                xml_node_free(node);
                nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
            }
            free(end);
            xml_skip_ws(p);
            if (!xml_expect(p, '>')) {
                xml_node_free(node);
                nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
            }
            break;
        }
        if (xml_eat(p, "<!--")) {
            xml_skip_comment(p, line, col);
            continue;
        }
        if (xml_starts_with(p, "<![CDATA[")) {
            p->pos += 9;
            for (;;) {
                if (xml_starts_with(p, "]]>")) {
                    p->pos += 3;
                    break;
                }
                if (p->pos >= p->len) {
                    xml_node_free(node);
                    nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
                }
                j14_buf_add_cp(&node->text, p->chars[p->pos++]);
            }
            continue;
        }
        if (xml_eat(p, "<?")) {
            xml_skip_pi(p, line, col);
            continue;
        }
        if (xml_peek(p) == '<') {
            XmlNodeC *child = xml_parse_element(p, depth + 1, line, col);
            node->children = (XmlNodeC **)realloc(
                node->children, (node->nchildren + 1) * sizeof(XmlNodeC *));
            node->children[node->nchildren++] = child;
            continue;
        }
        xml_read_text(p, &node->text, line, col);
    }
    return node;
}

static NvValue xml_node_to_value(XmlNodeC *n) {
    size_t ridx = nv_root_push_idx();
    NvValue dict = nv_dict_new();
    *nv_root_slot(ridx) = dict;
    nv_dict_set_raw(dict.u.dict, "وسم", nv_str_new(n->name, strlen(n->name)));
    size_t aridx = nv_root_push_idx();
    NvValue attrs = nv_dict_new();
    *nv_root_slot(aridx) = attrs;
    for (int i = 0; i < n->nattrs; i++) {
        nv_dict_set_raw(attrs.u.dict, n->akeys[i], nv_str_new(n->avals[i], strlen(n->avals[i])));
    }
    nv_dict_set_raw(dict.u.dict, "سمات", attrs);
    nv_root_pop();
    size_t cridx = nv_root_push_idx();
    NvValue kids = nv_list_new();
    *nv_root_slot(cridx) = kids;
    for (int i = 0; i < n->nchildren; i++) {
        NvValue child = xml_node_to_value(n->children[i]);
        size_t vridx = nv_root_push_idx();
        *nv_root_slot(vridx) = child;
        nv_list_push_raw(kids.u.list, child);
        nv_root_pop();
    }
    nv_dict_set_raw(dict.u.dict, "أبناء", kids);
    nv_root_pop();
    nv_dict_set_raw(dict.u.dict, "نص", nv_str_new(n->text.data, n->text.len));
    nv_root_pop();
    return dict;
}

static NvValue nv_std_xml_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكسمل.اقرأ", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("اكسمل.اقرأ", args[0], line, col);
    size_t tlen = strlen(text);
    if (nv_utf8_chars(text, tlen) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(text, tlen, &n, &offs);
    XmlParserC p;
    p.chars = chars;
    p.len = (int)n;
    p.pos = 0;
    for (;;) {
        xml_skip_ws(&p);
        if (xml_eat(&p, "<!--")) xml_skip_comment(&p, line, col);
        else if (xml_eat(&p, "<?")) xml_skip_pi(&p, line, col);
        else if (xml_starts_with(&p, "<!DOCTYPE")) xml_skip_doctype(&p, line, col);
        else break;
    }
    XmlNodeC *node = xml_parse_element(&p, 0, line, col);
    for (;;) {
        xml_skip_ws(&p);
        if (xml_eat(&p, "<!--")) xml_skip_comment(&p, line, col);
        else if (xml_eat(&p, "<?")) xml_skip_pi(&p, line, col);
        else break;
    }
    if (p.pos != p.len) {
        xml_node_free(node);
        free(chars);
        free(offs);
        nv_error(NV_ERR_RUNTIME, "نص XML غير صالح", line, col);
    }
    NvValue out = xml_node_to_value(node);
    xml_node_free(node);
    free(chars);
    free(offs);
    return out;
}

static void xml_escape_text_c(const char *s, J14Buf *out) {
    for (const char *p = s; *p; p++) {
        if (*p == '&') j14_buf_add(out, "&amp;");
        else if (*p == '<') j14_buf_add(out, "&lt;");
        else if (*p == '>') j14_buf_add(out, "&gt;");
        else j14_buf_addc(out, *p);
    }
}

static void xml_escape_attr_c(const char *s, J14Buf *out) {
    for (const char *p = s; *p; p++) {
        if (*p == '&') j14_buf_add(out, "&amp;");
        else if (*p == '<') j14_buf_add(out, "&lt;");
        else if (*p == '>') j14_buf_add(out, "&gt;");
        else if (*p == '"') j14_buf_add(out, "&quot;");
        else j14_buf_addc(out, *p);
    }
}

static int xml_valid_name_c(const char *name) {
    size_t n = 0;
    size_t *offs = NULL;
    uint32_t *chars = j14_decode_all(name, strlen(name), &n, &offs);
    int ok = 0;
    if (n > 0 && xml_name_start_cp(chars[0])) {
        ok = 1;
        for (size_t i = 1; i < n; i++) {
            if (!xml_name_char_cp(chars[i])) {
                ok = 0;
                break;
            }
        }
    }
    free(chars);
    free(offs);
    return ok;
}

static int xml_write_value_c(NvValue v, J14Buf *out, int depth) {
    if (depth > 100) return 0;
    if (v.tag != NV_DICT) return 0;
    NvDict *d = v.u.dict;
    int name_idx = nv_dict_find(d, "وسم");
    if (name_idx < 0) return 0;
    NvValue name_v = d->entries[name_idx].val;
    if (name_v.tag != NV_STR || !xml_valid_name_c(name_v.u.str->data)) return 0;
    const char *name = name_v.u.str->data;
    NvDict *attrs = NULL;
    int attrs_idx = nv_dict_find(d, "سمات");
    if (attrs_idx >= 0) {
        if (d->entries[attrs_idx].val.tag != NV_DICT) return 0;
        attrs = d->entries[attrs_idx].val.u.dict;
    }
    NvList *kids = NULL;
    int kids_idx = nv_dict_find(d, "أبناء");
    if (kids_idx >= 0) {
        if (d->entries[kids_idx].val.tag != NV_LIST) return 0;
        kids = d->entries[kids_idx].val.u.list;
    }
    const char *text = "";
    int text_idx = nv_dict_find(d, "نص");
    if (text_idx >= 0) {
        if (d->entries[text_idx].val.tag != NV_STR) return 0;
        text = d->entries[text_idx].val.u.str->data;
    }
    if (attrs) {
        for (size_t i = 0; i < attrs->len; i++) {
            if (!xml_valid_name_c(attrs->entries[i].key)) return 0;
            if (attrs->entries[i].val.tag != NV_STR) return 0;
        }
    }
    j14_buf_addc(out, '<');
    j14_buf_add(out, name);
    if (attrs) {
        for (size_t i = 0; i < attrs->len; i++) {
            j14_buf_addc(out, ' ');
            j14_buf_add(out, attrs->entries[i].key);
            j14_buf_add(out, "=\"");
            xml_escape_attr_c(attrs->entries[i].val.u.str->data, out);
            j14_buf_addc(out, '"');
        }
    }
    size_t nkids = kids ? kids->len : 0;
    if (nkids == 0 && text[0] == '\0') {
        j14_buf_add(out, "/>");
        return 1;
    }
    j14_buf_addc(out, '>');
    for (size_t i = 0; i < nkids; i++) {
        if (!xml_write_value_c(kids->items[i], out, depth + 1)) return 0;
    }
    xml_escape_text_c(text, out);
    j14_buf_add(out, "</");
    j14_buf_add(out, name);
    j14_buf_addc(out, '>');
    return 1;
}

static NvValue nv_std_xml_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكسمل.اكتب", args, nargs, 1, line, col);
    J14Buf out;
    j14_buf_init(&out);
    if (!xml_write_value_c(args[0], &out, 0)) {
        j14_buf_free(&out);
        nv_error(NV_ERR_RUNTIME, "قيمة XML غير صالحة", line, col);
    }
    NvValue result = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return result;
}

void nv_std_xml_fill(NvDict *d) {
    nv_std_add_fn(d, "اقرأ", nv_std_xml_read);
    nv_std_add_fn(d, "اكتب", nv_std_xml_write);
}

// ---------- 223: YAML (مرآة Rust حرفيا؛ بايتات — كل الفواصل ASCII) ----------

typedef struct {
    int indent;
    char *text;
    int blank;
} YamlLineC;

typedef struct {
    YamlLineC *lines;
    int len;
    int pos;
} YamlParserC;

static char *yaml_trim_end_c(const char *s) {
    size_t n = strlen(s);
    while (n > 0 && (s[n - 1] == ' ' || s[n - 1] == '\t' || s[n - 1] == '\r')) n--;
    return j14_strndup(s, n);
}

static char *yaml_strip_comment_c(const char *s) {
    size_t n = strlen(s);
    int in_single = 0, in_double = 0;
    size_t i = 0;
    while (i < n) {
        char c = s[i];
        if (in_single) {
            if (c == '\'') {
                if (i + 1 < n && s[i + 1] == '\'') {
                    i += 2;
                    continue;
                }
                in_single = 0;
            }
        } else if (in_double) {
            if (c == '\\') {
                i += 2;
                continue;
            }
            if (c == '"') in_double = 0;
        } else if (c == '\'') {
            in_single = 1;
        } else if (c == '"') {
            in_double = 1;
        } else if (c == '#' && (i == 0 || s[i - 1] == ' ' || s[i - 1] == '\t')) {
            char *cut = j14_strndup(s, i);
            char *trimmed = yaml_trim_end_c(cut);
            free(cut);
            return trimmed;
        }
        i++;
    }
    return yaml_trim_end_c(s);
}

static YamlLineC *yaml_split_lines_c(const char *src, int *n_out, int line, int col) {
    YamlLineC *lines = NULL;
    int n = 0, cap = 0;
    const char *p = src;
    for (;;) {
        const char *nl = strchr(p, '\n');
        size_t rawlen = nl ? (size_t)(nl - p) : strlen(p);
        char *raw = j14_strndup(p, rawlen);
        if (rawlen > 0 && raw[rawlen - 1] == '\r') raw[rawlen - 1] = '\0';
        int indent = 0;
        int has_tab = 0;
        while (raw[indent] == ' ') indent++;
        if (raw[indent] == '\t') has_tab = 1;
        if (has_tab) {
            free(raw);
            for (int i = 0; i < n; i++) free(lines[i].text);
            free(lines);
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        const char *content = raw + indent;
        int blank = 1;
        for (const char *q = content; *q; q++) {
            if (*q != ' ' && *q != '\t' && *q != '\r') {
                blank = 0;
                break;
            }
        }
        if (n == cap) {
            cap = cap ? cap * 2 : 16;
            lines = (YamlLineC *)realloc(lines, cap * sizeof(YamlLineC));
        }
        if (blank) {
            lines[n].indent = 0;
            lines[n].text = j14_strdup("");
            lines[n].blank = 1;
        } else {
            char *text = yaml_strip_comment_c(content);
            if (text[0] == '\0') {
                free(text);
                lines[n].indent = 0;
                lines[n].text = j14_strdup("");
                lines[n].blank = 1;
            } else {
                lines[n].indent = indent;
                lines[n].text = text;
                lines[n].blank = 0;
            }
        }
        n++;
        free(raw);
        if (!nl) break;
        p = nl + 1;
    }
    *n_out = n;
    return lines;
}

static void yaml_lines_free(YamlLineC *lines, int n) {
    for (int i = 0; i < n; i++) free(lines[i].text);
    free(lines);
}

static int yaml_is_dash_c(const char *text) {
    return strcmp(text, "-") == 0 || strncmp(text, "- ", 2) == 0;
}

static const char *yaml_dash_rest_c(const char *text) {
    if (strcmp(text, "-") == 0) return "";
    return text + 2;
}

static int yaml_is_block_header_c(const char *s) {
    return strcmp(s, "|") == 0 || strcmp(s, "|-") == 0 || strcmp(s, "|+") == 0 ||
           strcmp(s, ">") == 0 || strcmp(s, ">-") == 0 || strcmp(s, ">+") == 0;
}

/// أول ':' فاصل (خارج التنصيص والتدفق، متبوعا بمسافة أو نهاية).
static int yaml_split_key_c(const char *text, char **key_out, char **rest_out) {
    size_t n = strlen(text);
    int in_single = 0, in_double = 0;
    int depth = 0;
    for (size_t i = 0; i < n; i++) {
        char c = text[i];
        if (in_single) {
            if (c == '\'') {
                if (i + 1 < n && text[i + 1] == '\'') {
                    i++;
                    continue;
                }
                in_single = 0;
            }
        } else if (in_double) {
            if (c == '\\') {
                i++;
                continue;
            }
            if (c == '"') in_double = 0;
        } else if (c == '\'') {
            in_single = 1;
        } else if (c == '"') {
            in_double = 1;
        } else if (c == '[' || c == '{') {
            depth++;
        } else if (c == ']' || c == '}') {
            if (depth > 0) depth--;
        } else if (c == ':' && depth == 0 && (i + 1 >= n || text[i + 1] == ' ')) {
            char *key = j14_strndup(text, i);
            // تقليم طرفي المفتاح.
            while (*key == ' ' || *key == '\t') {
                memmove(key, key + 1, strlen(key));
            }
            size_t kl = strlen(key);
            while (kl > 0 && (key[kl - 1] == ' ' || key[kl - 1] == '\t')) key[--kl] = '\0';
            const char *rest = text + i + 1;
            while (*rest == ' ' || *rest == '\t') rest++;
            char *rest_copy = j14_strdup(rest);
            size_t rl = strlen(rest_copy);
            while (rl > 0 && (rest_copy[rl - 1] == ' ' || rest_copy[rl - 1] == '\t'))
                rest_copy[--rl] = '\0';
            *key_out = key;
            *rest_out = rest_copy;
            return 1;
        }
    }
    return 0;
}

static int yaml_is_map_line_c(const char *text) {
    char *k = NULL, *r = NULL;
    int ok = yaml_split_key_c(text, &k, &r);
    if (ok) {
        free(k);
        free(r);
    }
    return ok;
}

static char *yaml_unquote_single_c(const char *s, int line, int col) {
    size_t n = strlen(s);
    if (n < 2 || s[0] != '\'' || s[n - 1] != '\'') {
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    J14Buf out;
    j14_buf_init(&out);
    for (size_t i = 1; i + 1 < n; i++) {
        if (s[i] == '\'' && s[i + 1] == '\'') {
            j14_buf_addc(&out, '\'');
            i++;
        } else {
            j14_buf_addc(&out, s[i]);
        }
    }
    char *r = j14_strdup(out.data);
    j14_buf_free(&out);
    return r;
}

static int yaml_hex_digit_c(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static char *yaml_unquote_double_c(const char *s, int line, int col) {
    size_t n = strlen(s);
    if (n < 2 || s[0] != '"' || s[n - 1] != '"') {
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    J14Buf out;
    j14_buf_init(&out);
    size_t i = 1;
    while (i < n - 1) {
        char c = s[i];
        if (c == '\\') {
            i++;
            if (i >= n - 1) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            char e = s[i];
            switch (e) {
                case '"': j14_buf_addc(&out, '"'); break;
                case '\\': j14_buf_addc(&out, '\\'); break;
                case '/': j14_buf_addc(&out, '/'); break;
                case 'n': j14_buf_addc(&out, '\n'); break;
                case 'r': j14_buf_addc(&out, '\r'); break;
                case 't': j14_buf_addc(&out, '\t'); break;
                case 'b': j14_buf_addc(&out, '\b'); break;
                case 'f': j14_buf_addc(&out, '\f'); break;
                case 'u': {
                    uint32_t code = 0;
                    for (int k = 0; k < 4; k++) {
                        i++;
                        if (i >= n - 1) {
                            j14_buf_free(&out);
                            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                        }
                        int d = yaml_hex_digit_c(s[i]);
                        if (d < 0) {
                            j14_buf_free(&out);
                            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                        }
                        code = code * 16 + (uint32_t)d;
                    }
                    if (code >= 0xD800 && code < 0xE000) {
                        j14_buf_free(&out);
                        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                    }
                    j14_buf_add_cp(&out, code);
                    break;
                }
                default:
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            i++;
        } else {
            j14_buf_addc(&out, c);
            i++;
        }
    }
    char *r = j14_strdup(out.data);
    j14_buf_free(&out);
    return r;
}

static char *yaml_trim_c(const char *s) {
    while (*s == ' ' || *s == '\t') s++;
    size_t n = strlen(s);
    while (n > 0 && (s[n - 1] == ' ' || s[n - 1] == '\t')) n--;
    return j14_strndup(s, n);
}

static char *yaml_parse_key_c(const char *s, int line, int col) {
    char *t = yaml_trim_c(s);
    if (t[0] == '\0') {
        free(t);
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    char *r = NULL;
    if (t[0] == '\'') r = yaml_unquote_single_c(t, line, col);
    else if (t[0] == '"') r = yaml_unquote_double_c(t, line, col);
    else r = j14_strdup(t);
    free(t);
    return r;
}

static int yaml_is_integer_c(const char *s) {
    if (*s == '-') s++;
    if (*s == '\0') return 0;
    for (const char *p = s; *p; p++) {
        if (*p < '0' || *p > '9') return 0;
    }
    return 1;
}

static int yaml_is_float_c(const char *s) {
    if (*s == '-') s++;
    if (*s == '\0') return 0;
    const char *epos = strchr(s, 'e');
    if (!epos) epos = strchr(s, 'E');
    const char *mant_end = epos ? epos : s + strlen(s);
    if (epos) {
        const char *e = epos + 1;
        if (*e == '+' || *e == '-') e++;
        if (*e == '\0') return 0;
        for (const char *p = e; *p; p++) {
            if (*p < '0' || *p > '9') return 0;
        }
    }
    int has_dot = 0;
    int ndigits = 0;
    for (const char *p = s; p < mant_end; p++) {
        if (*p == '.') {
            if (has_dot) return 0;
            has_dot = 1;
        } else if (*p >= '0' && *p <= '9') {
            ndigits++;
        } else {
            return 0;
        }
    }
    if (ndigits == 0) return 0;
    if (!has_dot && !epos) return 0;
    return 1;
}

static NvValue yaml_parse_scalar_c(const char *s_in, int line, int col) {
    char *s = yaml_trim_c(s_in);
    NvValue out;
    if (strcmp(s, "لاشيء") == 0 || strcmp(s, "null") == 0 || strcmp(s, "Null") == 0 ||
        strcmp(s, "NULL") == 0 || strcmp(s, "~") == 0) {
        free(s);
        return nv_nil();
    }
    if (strcmp(s, "صحيح") == 0 || strcmp(s, "true") == 0 || strcmp(s, "True") == 0 ||
        strcmp(s, "TRUE") == 0) {
        free(s);
        return nv_bool(1);
    }
    if (strcmp(s, "خطأ") == 0 || strcmp(s, "false") == 0 || strcmp(s, "False") == 0 ||
        strcmp(s, "FALSE") == 0) {
        free(s);
        return nv_bool(0);
    }
    if (s[0] == '\'') {
        char *u = yaml_unquote_single_c(s, line, col);
        out = nv_str_new(u, strlen(u));
        free(u);
        free(s);
        return out;
    }
    if (s[0] == '"') {
        char *u = yaml_unquote_double_c(s, line, col);
        out = nv_str_new(u, strlen(u));
        free(u);
        free(s);
        return out;
    }
    if (yaml_is_integer_c(s)) {
        double v = strtod(s, NULL);
        if (!isfinite(v)) {
            free(s);
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        free(s);
        return nv_num(v);
    }
    if (yaml_is_float_c(s)) {
        double v = strtod(s, NULL);
        if (!isfinite(v)) {
            free(s);
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        free(s);
        return nv_num(v);
    }
    int bad = 0;
    if (s[0] == '\0') bad = 1;
    if (strchr("!&*?|>%@`[]{},", s[0])) bad = 1;
    if (strstr(s, ": ")) bad = 1;
    if (strstr(s, " #")) bad = 1;
    size_t sl = strlen(s);
    if (sl > 0 && s[sl - 1] == ':') bad = 1;
    for (size_t i = 0; i < sl; i++) {
        if ((unsigned char)s[i] < 0x20) bad = 1;
    }
    if (bad) {
        free(s);
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    out = nv_str_new(s, sl);
    free(s);
    return out;
}

/// محلل تدفق [..] و{..}.
typedef struct {
    const char *s;
    int len, pos;
} YamlFlowC;

static void yaml_flow_skip_ws(YamlFlowC *f) {
    while (f->pos < f->len && (f->s[f->pos] == ' ' || f->s[f->pos] == '\t')) f->pos++;
}

static NvValue yaml_flow_value(YamlFlowC *f, int line, int col);

static char *yaml_flow_quoted(YamlFlowC *f, int line, int col) {
    char q = f->s[f->pos++];
    J14Buf out;
    j14_buf_init(&out);
    if (q == '\'') {
        for (;;) {
            if (f->pos >= f->len) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            char c = f->s[f->pos++];
            if (c == '\'') {
                if (f->pos < f->len && f->s[f->pos] == '\'') {
                    f->pos++;
                    j14_buf_addc(&out, '\'');
                } else {
                    break;
                }
            } else {
                j14_buf_addc(&out, c);
            }
        }
    } else {
        for (;;) {
            if (f->pos >= f->len) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            char c = f->s[f->pos++];
            if (c == '"') break;
            if (c == '\\') {
                if (f->pos >= f->len) {
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                }
                char e = f->s[f->pos++];
                switch (e) {
                    case '"': j14_buf_addc(&out, '"'); break;
                    case '\\': j14_buf_addc(&out, '\\'); break;
                    case '/': j14_buf_addc(&out, '/'); break;
                    case 'n': j14_buf_addc(&out, '\n'); break;
                    case 'r': j14_buf_addc(&out, '\r'); break;
                    case 't': j14_buf_addc(&out, '\t'); break;
                    case 'b': j14_buf_addc(&out, '\b'); break;
                    case 'f': j14_buf_addc(&out, '\f'); break;
                    case 'u': {
                        uint32_t code = 0;
                        for (int k = 0; k < 4; k++) {
                            if (f->pos >= f->len) {
                                j14_buf_free(&out);
                                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                            }
                            int d = yaml_hex_digit_c(f->s[f->pos++]);
                            if (d < 0) {
                                j14_buf_free(&out);
                                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                            }
                            code = code * 16 + (uint32_t)d;
                        }
                        if (code >= 0xD800 && code < 0xE000) {
                            j14_buf_free(&out);
                            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                        }
                        j14_buf_add_cp(&out, code);
                        break;
                    }
                    default:
                        j14_buf_free(&out);
                        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
                }
            } else {
                j14_buf_addc(&out, c);
            }
        }
    }
    char *r = j14_strdup(out.data);
    j14_buf_free(&out);
    return r;
}

static char *yaml_flow_key(YamlFlowC *f, int line, int col) {
    yaml_flow_skip_ws(f);
    if (f->pos < f->len && (f->s[f->pos] == '"' || f->s[f->pos] == '\'')) {
        return yaml_flow_quoted(f, line, col);
    }
    int start = f->pos;
    while (f->pos < f->len && f->s[f->pos] != ':' && f->s[f->pos] != ',' && f->s[f->pos] != '}') {
        f->pos++;
    }
    char *raw = j14_strndup(f->s + start, (size_t)(f->pos - start));
    char *key = yaml_parse_key_c(raw, line, col);
    free(raw);
    return key;
}

static NvValue yaml_flow_value(YamlFlowC *f, int line, int col) {
    yaml_flow_skip_ws(f);
    if (f->pos >= f->len) nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    char c = f->s[f->pos];
    if (c == '[') {
        f->pos++;
        size_t ridx = nv_root_push_idx();
        NvValue list = nv_list_new();
        *nv_root_slot(ridx) = list;
        yaml_flow_skip_ws(f);
        if (f->pos < f->len && f->s[f->pos] == ']') {
            f->pos++;
            nv_root_pop();
            return list;
        }
        for (;;) {
            NvValue val = yaml_flow_value(f, line, col);
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = val;
            nv_list_push_raw(list.u.list, val);
            nv_root_pop();
            yaml_flow_skip_ws(f);
            if (f->pos >= f->len) {
                nv_root_pop();
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            char d = f->s[f->pos++];
            if (d == ',') continue;
            if (d == ']') break;
            nv_root_pop();
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        nv_root_pop();
        return list;
    }
    if (c == '{') {
        f->pos++;
        size_t ridx = nv_root_push_idx();
        NvValue dict = nv_dict_new();
        *nv_root_slot(ridx) = dict;
        yaml_flow_skip_ws(f);
        if (f->pos < f->len && f->s[f->pos] == '}') {
            f->pos++;
            nv_root_pop();
            return dict;
        }
        for (;;) {
            yaml_flow_skip_ws(f);
            char *key = yaml_flow_key(f, line, col);
            yaml_flow_skip_ws(f);
            if (f->pos >= f->len || f->s[f->pos] != ':') {
                free(key);
                nv_root_pop();
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            f->pos++;
            NvValue val = yaml_flow_value(f, line, col);
            if (nv_dict_find(dict.u.dict, key) >= 0) {
                free(key);
                nv_root_pop();
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = val;
            nv_dict_set_raw(dict.u.dict, key, val);
            nv_root_pop();
            free(key);
            yaml_flow_skip_ws(f);
            if (f->pos >= f->len) {
                nv_root_pop();
                nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
            }
            char d = f->s[f->pos++];
            if (d == ',') continue;
            if (d == '}') break;
            nv_root_pop();
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        nv_root_pop();
        return dict;
    }
    if (c == '"' || c == '\'') {
        char *q = yaml_flow_quoted(f, line, col);
        NvValue v = nv_str_new(q, strlen(q));
        free(q);
        return v;
    }
    int start = f->pos;
    while (f->pos < f->len && f->s[f->pos] != ',' && f->s[f->pos] != ']' && f->s[f->pos] != '}') {
        f->pos++;
    }
    char *raw = j14_strndup(f->s + start, (size_t)(f->pos - start));
    char *trimmed = yaml_trim_c(raw);
    free(raw);
    if (trimmed[0] == '\0') {
        free(trimmed);
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    NvValue v = yaml_parse_scalar_c(trimmed, line, col);
    free(trimmed);
    return v;
}

static NvValue yaml_parse_inline_c(const char *s, int line, int col) {
    char *t = yaml_trim_c(s);
    NvValue v;
    if (t[0] == '[' || t[0] == '{') {
        YamlFlowC f;
        f.s = t;
        f.len = (int)strlen(t);
        f.pos = 0;
        v = yaml_flow_value(&f, line, col);
        yaml_flow_skip_ws(&f);
        if (f.pos != f.len) {
            free(t);
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
    } else {
        v = yaml_parse_scalar_c(t, line, col);
    }
    free(t);
    return v;
}

static char *yaml_chomp_c(const char *s, char chomp) {
    if (chomp == '-') {
        size_t n = strlen(s);
        while (n > 0 && s[n - 1] == '\n') n--;
        return j14_strndup(s, n);
    }
    if (chomp == '+') return j14_strdup(s);
    size_t n = strlen(s);
    while (n > 0 && s[n - 1] == '\n') n--;
    if (n == 0) return j14_strdup("");
    char *r = j14_strndup(s, n);
    size_t rn = strlen(r);
    char *out = (char *)malloc(rn + 2);
    memcpy(out, r, rn);
    out[rn] = '\n';
    out[rn + 1] = '\0';
    free(r);
    return out;
}

static YamlLineC *yaml_cur(YamlParserC *p) {
    int i = p->pos;
    while (i < p->len) {
        if (!p->lines[i].blank) return &p->lines[i];
        i++;
    }
    return NULL;
}

static void yaml_skip_blanks(YamlParserC *p) {
    while (p->pos < p->len && p->lines[p->pos].blank) p->pos++;
}

static NvValue yaml_parse_block(YamlParserC *p, int indent, int depth, int line, int col);
static NvValue yaml_parse_seq(YamlParserC *p, int indent, int depth, int line, int col);
static NvValue yaml_parse_map(YamlParserC *p, int indent, int depth, int line, int col);

static NvValue yaml_parse_block_scalar(
    YamlParserC *p, const char *header, int parent, int line, int col) {
    int folded = header[0] == '>';
    char chomp = ' ';
    if (strchr(header, '-')) chomp = '-';
    else if (strchr(header, '+')) chomp = '+';
    char **raw = NULL;
    int nraw = 0, capraw = 0;
    int content_indent = -1;
    while (p->pos < p->len) {
        YamlLineC *l = &p->lines[p->pos];
        if (l->blank) {
            if (nraw == capraw) {
                capraw = capraw ? capraw * 2 : 8;
                raw = (char **)realloc(raw, capraw * sizeof(char *));
            }
            raw[nraw++] = j14_strdup("");
            p->pos++;
            continue;
        }
        if (l->indent <= parent) break;
        if (content_indent < 0) content_indent = l->indent;
        if (l->indent < content_indent) break;
        if (nraw == capraw) {
            capraw = capraw ? capraw * 2 : 8;
            raw = (char **)realloc(raw, capraw * sizeof(char *));
        }
        J14Buf b;
        j14_buf_init(&b);
        for (int i = 0; i < l->indent - content_indent; i++) j14_buf_addc(&b, ' ');
        j14_buf_add(&b, l->text);
        raw[nraw++] = j14_strdup(b.data);
        j14_buf_free(&b);
        p->pos++;
    }
    J14Buf body;
    j14_buf_init(&body);
    if (folded) {
        for (int i = 0; i < nraw; i++) {
            if (raw[i][0] == '\0') {
                j14_buf_addc(&body, '\n');
                continue;
            }
            j14_buf_add(&body, raw[i]);
            if (i + 1 >= nraw) j14_buf_addc(&body, '\n');
            else if (raw[i + 1][0] == '\0') j14_buf_addc(&body, '\n');
            else j14_buf_addc(&body, ' ');
        }
    } else {
        for (int i = 0; i < nraw; i++) {
            j14_buf_add(&body, raw[i]);
            j14_buf_addc(&body, '\n');
        }
    }
    for (int i = 0; i < nraw; i++) free(raw[i]);
    free(raw);
    char *chomped = yaml_chomp_c(body.data, chomp);
    j14_buf_free(&body);
    NvValue v = nv_str_new(chomped, strlen(chomped));
    free(chomped);
    return v;
}

static NvValue yaml_parse_block(YamlParserC *p, int indent, int depth, int line, int col) {
    if (depth > 50) nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    YamlLineC *l = yaml_cur(p);
    if (!l) return nv_nil();
    if (l->indent < indent) return nv_nil();
    if (l->indent > indent) nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    if (yaml_is_dash_c(l->text)) return yaml_parse_seq(p, indent, depth, line, col);
    if (yaml_is_map_line_c(l->text)) return yaml_parse_map(p, indent, depth, line, col);
    char *text = j14_strdup(l->text);
    yaml_skip_blanks(p);
    p->pos++;
    NvValue v = yaml_parse_inline_c(text, line, col);
    free(text);
    return v;
}

static NvValue yaml_parse_seq(YamlParserC *p, int indent, int depth, int line, int col) {
    size_t ridx = nv_root_push_idx();
    NvValue list = nv_list_new();
    *nv_root_slot(ridx) = list;
    for (;;) {
        yaml_skip_blanks(p);
        YamlLineC *l = yaml_cur(p);
        if (!l) break;
        if (l->indent != indent || !yaml_is_dash_c(l->text)) break;
        if (strcmp(l->text, "...") == 0) break;
        char *rest = yaml_trim_c(yaml_dash_rest_c(l->text));
        if (rest[0] == '\0') {
            p->pos++;
            yaml_skip_blanks(p);
            NvValue v;
            YamlLineC *nl = yaml_cur(p);
            if (nl && nl->indent > indent) v = yaml_parse_block(p, nl->indent, depth + 1, line, col);
            else v = nv_nil();
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = v;
            nv_list_push_raw(list.u.list, v);
            nv_root_pop();
        } else if (yaml_is_block_header_c(rest)) {
            p->pos++;
            NvValue v = yaml_parse_block_scalar(p, rest, indent, line, col);
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = v;
            nv_list_push_raw(list.u.list, v);
            nv_root_pop();
        } else {
            free(p->lines[p->pos].text);
            p->lines[p->pos].indent = indent + 2;
            p->lines[p->pos].text = rest;
            NvValue v = yaml_parse_block(p, indent + 2, depth + 1, line, col);
            size_t vridx = nv_root_push_idx();
            *nv_root_slot(vridx) = v;
            nv_list_push_raw(list.u.list, v);
            nv_root_pop();
            continue;
        }
        free(rest);
    }
    nv_root_pop();
    return list;
}

static NvValue yaml_parse_map(YamlParserC *p, int indent, int depth, int line, int col) {
    size_t ridx = nv_root_push_idx();
    NvValue dict = nv_dict_new();
    *nv_root_slot(ridx) = dict;
    for (;;) {
        yaml_skip_blanks(p);
        YamlLineC *l = yaml_cur(p);
        if (!l) break;
        if (l->indent != indent || yaml_is_dash_c(l->text)) break;
        if (strcmp(l->text, "...") == 0) break;
        char *key = NULL, *rest = NULL;
        if (!yaml_split_key_c(l->text, &key, &rest)) {
            nv_root_pop();
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        char *k = yaml_parse_key_c(key, line, col);
        free(key);
        if (nv_dict_find(dict.u.dict, k) >= 0) {
            free(k);
            free(rest);
            nv_root_pop();
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
        NvValue v;
        if (rest[0] == '\0') {
            p->pos++;
            yaml_skip_blanks(p);
            YamlLineC *nl = yaml_cur(p);
            if (nl && nl->indent > indent) v = yaml_parse_block(p, nl->indent, depth + 1, line, col);
            else v = nv_nil();
        } else if (yaml_is_block_header_c(rest)) {
            p->pos++;
            v = yaml_parse_block_scalar(p, rest, indent, line, col);
        } else {
            p->pos++;
            v = yaml_parse_inline_c(rest, line, col);
        }
        size_t vridx = nv_root_push_idx();
        *nv_root_slot(vridx) = v;
        nv_dict_set_raw(dict.u.dict, k, v);
        nv_root_pop();
        free(k);
        free(rest);
    }
    nv_root_pop();
    return dict;
}

static NvValue nv_std_yaml_read(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("يامال.اقرأ", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("يامال.اقرأ", args[0], line, col);
    if (nv_utf8_chars(text, strlen(text)) > RX_MAX_TEXT) {
        nv_error(NV_ERR_RUNTIME, "النص كبير جدا", line, col);
    }
    int nlines = 0;
    YamlLineC *lines = yaml_split_lines_c(text, &nlines, line, col);
    YamlParserC p;
    p.lines = lines;
    p.len = nlines;
    p.pos = 0;
    yaml_skip_blanks(&p);
    NvValue out;
    if (p.pos >= p.len) {
        yaml_lines_free(lines, nlines);
        return nv_nil();
    }
    if (strcmp(p.lines[p.pos].text, "---") == 0) {
        p.pos++;
        yaml_skip_blanks(&p);
        if (p.pos >= p.len) {
            yaml_lines_free(lines, nlines);
            return nv_nil();
        }
    } else if (strncmp(p.lines[p.pos].text, "--- ", 4) == 0) {
        yaml_lines_free(lines, nlines);
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    YamlLineC *first = yaml_cur(&p);
    if (first && first->indent != 0) {
        yaml_lines_free(lines, nlines);
        nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
    }
    out = yaml_parse_block(&p, 0, 0, line, col);
    yaml_skip_blanks(&p);
    if (p.pos < p.len) {
        if (strcmp(p.lines[p.pos].text, "...") == 0) {
            p.pos++;
            yaml_skip_blanks(&p);
        }
        if (p.pos < p.len) {
            yaml_lines_free(lines, nlines);
            nv_error(NV_ERR_RUNTIME, "نص YAML غير صالح", line, col);
        }
    }
    yaml_lines_free(lines, nlines);
    return out;
}

/// هل النص آمن كنص عادي في YAML (بلا تنصيص)؟
static int yaml_plain_safe_c(const char *s) {
    if (s[0] == '\0') return 0;
    if (strcmp(s, "-") == 0 || strncmp(s, "- ", 2) == 0) return 0;
    if (strchr("!&*?|>%@`\"'#[]{},", s[0])) return 0;
    if (strstr(s, ": ")) return 0;
    if (strstr(s, " #")) return 0;
    size_t n = strlen(s);
    if (s[n - 1] == ':') return 0;
    if (s[0] == ' ' || s[n - 1] == ' ') return 0;
    for (size_t i = 0; i < n; i++) {
        if ((unsigned char)s[i] < 0x20) return 0;
    }
    if (strcmp(s, "لاشيء") == 0 || strcmp(s, "null") == 0 || strcmp(s, "Null") == 0 ||
        strcmp(s, "NULL") == 0 || strcmp(s, "~") == 0) {
        return 0;
    }
    if (strcmp(s, "صحيح") == 0 || strcmp(s, "خطأ") == 0 || strcmp(s, "true") == 0 ||
        strcmp(s, "True") == 0 || strcmp(s, "TRUE") == 0 || strcmp(s, "false") == 0 ||
        strcmp(s, "False") == 0 || strcmp(s, "FALSE") == 0) {
        return 0;
    }
    if (yaml_is_integer_c(s) || yaml_is_float_c(s)) return 0;
    return 1;
}

static void yaml_double_quote_c(const char *s, J14Buf *out) {
    j14_buf_addc(out, '"');
    size_t i = 0;
    size_t n = strlen(s);
    while (i < n) {
        uint32_t cp = 0;
        size_t adv = j14_utf8_decode((const unsigned char *)s + i, n - i, &cp);
        if (adv == 0) adv = 1;
        switch (cp) {
            case '"': j14_buf_add(out, "\\\""); break;
            case '\\': j14_buf_add(out, "\\\\"); break;
            case '\n': j14_buf_add(out, "\\n"); break;
            case '\r': j14_buf_add(out, "\\r"); break;
            case '\t': j14_buf_add(out, "\\t"); break;
            case 0x08: j14_buf_add(out, "\\b"); break;
            case 0x0C: j14_buf_add(out, "\\f"); break;
            default:
                if (cp < 0x20) {
                    char tmp[8];
                    snprintf(tmp, sizeof(tmp), "\\u%04x", cp);
                    j14_buf_add(out, tmp);
                } else {
                    j14_buf_addn(out, s + i, adv);
                }
                break;
        }
        i += adv;
    }
    j14_buf_addc(out, '"');
}

static int yaml_inline_c(NvValue v, J14Buf *out) {
    switch (v.tag) {
        case NV_NIL:
            j14_buf_add(out, "لاشيء");
            return 1;
        case NV_BOOL:
            j14_buf_add(out, v.u.boolean ? "صحيح" : "خطأ");
            return 1;
        case NV_NUM:
            if (!isfinite(v.u.num)) return 0;
            json_fmt_num_c(v.u.num, out);
            return 1;
        case NV_STR:
            if (yaml_plain_safe_c(v.u.str->data)) j14_buf_add(out, v.u.str->data);
            else yaml_double_quote_c(v.u.str->data, out);
            return 1;
        case NV_LIST:
            if (v.u.list->len == 0) {
                j14_buf_add(out, "[]");
                return 1;
            }
            return 0;
        case NV_DICT:
            if (v.u.dict->len == 0) {
                j14_buf_add(out, "{}");
                return 1;
            }
            return 0;
        default:
            return 0;
    }
}

static int yaml_is_scalar_c(NvValue v) {
    return v.tag == NV_NIL || v.tag == NV_BOOL || v.tag == NV_NUM || v.tag == NV_STR;
}

static int yaml_block_c(NvValue v, int indent, int depth, J14Buf *out) {
    if (depth > 50) return 0;
    if (v.tag == NV_LIST) {
        if (v.u.list->len == 0) {
            j14_buf_add(out, "[]");
            return 1;
        }
        for (size_t i = 0; i < v.u.list->len; i++) {
            NvValue item = v.u.list->items[i];
            if (i > 0) j14_buf_addc(out, '\n');
            for (int k = 0; k < indent; k++) j14_buf_addc(out, ' ');
            int scalarish = yaml_is_scalar_c(item) ||
                            (item.tag == NV_LIST && item.u.list->len == 0) ||
                            (item.tag == NV_DICT && item.u.dict->len == 0);
            if (scalarish) {
                j14_buf_add(out, "- ");
                if (!yaml_inline_c(item, out)) return 0;
            } else {
                J14Buf inner;
                j14_buf_init(&inner);
                if (!yaml_block_c(item, indent + 2, depth + 1, &inner)) {
                    j14_buf_free(&inner);
                    return 0;
                }
                // أول سطر بعد "- " بلا مسافته البادئة.
                char *nl = strchr(inner.data, '\n');
                size_t first_len = nl ? (size_t)(nl - inner.data) : inner.len;
                size_t skip = 0;
                while (skip < first_len && skip < (size_t)(indent + 2) && inner.data[skip] == ' ') {
                    skip++;
                }
                j14_buf_add(out, "- ");
                j14_buf_addn(out, inner.data + skip, first_len - skip);
                if (nl) {
                    j14_buf_addc(out, '\n');
                    j14_buf_add(out, nl + 1);
                }
                j14_buf_free(&inner);
            }
        }
        return 1;
    }
    if (v.tag == NV_DICT) {
        if (v.u.dict->len == 0) {
            j14_buf_add(out, "{}");
            return 1;
        }
        for (size_t i = 0; i < v.u.dict->len; i++) {
            const char *key = v.u.dict->entries[i].key;
            NvValue val = v.u.dict->entries[i].val;
            if (i > 0) j14_buf_addc(out, '\n');
            for (int k = 0; k < indent; k++) j14_buf_addc(out, ' ');
            if (yaml_plain_safe_c(key) && !strstr(key, ": ")) j14_buf_add(out, key);
            else yaml_double_quote_c(key, out);
            int scalarish = yaml_is_scalar_c(val) ||
                            (val.tag == NV_LIST && val.u.list->len == 0) ||
                            (val.tag == NV_DICT && val.u.dict->len == 0);
            if (scalarish) {
                j14_buf_add(out, ": ");
                if (!yaml_inline_c(val, out)) return 0;
            } else {
                j14_buf_addc(out, ':');
                j14_buf_addc(out, '\n');
                if (!yaml_block_c(val, indent + 2, depth + 1, out)) return 0;
            }
        }
        return 1;
    }
    return yaml_inline_c(v, out);
}

static NvValue nv_std_yaml_write(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("يامال.اكتب", args, nargs, 1, line, col);
    J14Buf out;
    j14_buf_init(&out);
    if (!yaml_block_c(args[0], 0, 0, &out)) {
        j14_buf_free(&out);
        nv_error(NV_ERR_RUNTIME, "قيمة YAML غير صالحة", line, col);
    }
    j14_buf_addc(&out, '\n');
    NvValue result = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return result;
}

void nv_std_yaml_fill(NvDict *d) {
    nv_std_add_fn(d, "اقرأ", nv_std_yaml_read);
    nv_std_add_fn(d, "اكتب", nv_std_yaml_write);
}

// ---------- 224: القوالب (مرآة Rust حرفيا) ----------

#define TPL_MAX_CHARS 100000
#define TPL_MAX_OUT 1000000
#define TPL_MAX_DEPTH 20

typedef struct {
    int is_tag;
    char *text;
} TplTokC;

typedef struct {
    char *name;
    NvValue val;
} TplBind;

typedef struct {
    TplBind *binds;
    int nbinds;
} TplScope;

static char *tpl_trim_c(const char *s) {
    return yaml_trim_c(s);
}

static void tpl_push_c(J14Buf *out, const char *s, int line, int col) {
    if (out->len + strlen(s) > TPL_MAX_OUT) {
        nv_error(NV_ERR_RUNTIME, "القالب كبير جدا", line, col);
    }
    j14_buf_add(out, s);
}

static int tpl_lookup_path_c(
    const char *path, TplScope *scopes, int nscopes, NvValue *out) {
    // قسّم على '.'.
    const char *dot = strchr(path, '.');
    size_t first_len = dot ? (size_t)(dot - path) : strlen(path);
    char *first = j14_strndup(path, first_len);
    NvValue cur;
    int found = 0;
    for (int si = nscopes - 1; si >= 0 && !found; si--) {
        for (int i = 0; i < scopes[si].nbinds; i++) {
            if (strcmp(scopes[si].binds[i].name, first) == 0) {
                cur = scopes[si].binds[i].val;
                found = 1;
                break;
            }
        }
    }
    free(first);
    if (!found) return 0;
    const char *rest = dot ? dot + 1 : NULL;
    while (rest) {
        if (cur.tag != NV_DICT) return 0;
        const char *next_dot = strchr(rest, '.');
        size_t seg_len = next_dot ? (size_t)(next_dot - rest) : strlen(rest);
        char *seg = j14_strndup(rest, seg_len);
        int idx = nv_dict_find(cur.u.dict, seg);
        free(seg);
        if (idx < 0) return 0;
        cur = cur.u.dict->entries[idx].val;
        rest = next_dot ? next_dot + 1 : NULL;
    }
    *out = cur;
    return 1;
}

static int tpl_truthy_c(NvValue v) {
    switch (v.tag) {
        case NV_NIL: return 0;
        case NV_BOOL: return v.u.boolean;
        case NV_NUM: return v.u.num != 0.0;
        case NV_STR: return v.u.str->len > 0;
        case NV_LIST: return v.u.list->len > 0;
        case NV_DICT: return v.u.dict->len > 0;
        default: return 1;
    }
}

static int tpl_eval_cond_c(const char *cond, TplScope *scopes, int nscopes) {
    char *t = tpl_trim_c(cond);
    int result;
    if (strncmp(t, "ليس ", 7) == 0) {
        char *rest = tpl_trim_c(t + 7);
        NvValue v;
        int found = tpl_lookup_path_c(rest, scopes, nscopes, &v);
        result = !found || !tpl_truthy_c(v);
        free(rest);
    } else {
        NvValue v;
        int found = tpl_lookup_path_c(t, scopes, nscopes, &v);
        result = found && tpl_truthy_c(v);
    }
    free(t);
    return result;
}

static int tpl_find_end_c(TplTokC *toks, int i, int end, const char *kind, int line, int col) {
    char open[32];
    char close[32];
    snprintf(open, sizeof(open), "%s ", kind);
    snprintf(close, sizeof(close), "/%s", kind);
    int depth = 0;
    while (i < end) {
        if (toks[i].is_tag) {
            char *tag = tpl_trim_c(toks[i].text);
            if (strcmp(tag, kind) == 0 || strncmp(tag, open, strlen(open)) == 0) {
                depth++;
            } else if (strcmp(tag, close) == 0) {
                if (depth == 0) {
                    free(tag);
                    return i;
                }
                depth--;
            }
            free(tag);
        }
        i++;
    }
    nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
    return -1;
}

/// اعرض [i, end)؛ وتوقف عند وسم إنهاء فأعده.
/// يعيد (الفهرس التالي، وسم الإنهاء أو NULL).
static int tpl_render_range_c(
    TplTokC *toks, int i, int end, TplScope *scopes, int nscopes,
    J14Buf *out, int depth, int line, int col, char **term_out) {
    if (depth > TPL_MAX_DEPTH) {
        nv_error(NV_ERR_RUNTIME, "القالب كبير جدا", line, col);
    }
    *term_out = NULL;
    while (i < end) {
        if (!toks[i].is_tag) {
            tpl_push_c(out, toks[i].text, line, col);
            i++;
            continue;
        }
        char *tag = tpl_trim_c(toks[i].text);
        if (strcmp(tag, "وإلا") == 0 || strcmp(tag, "/إذا") == 0 || strcmp(tag, "/لكل") == 0) {
            *term_out = tag;
            return i;
        }
        if (strcmp(tag, "إذا") == 0 || strncmp(tag, "إذا ", 7) == 0) {
            char *cond = tpl_trim_c(tag + 6);
            if (cond[0] == '\0') {
                free(cond);
                free(tag);
                nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
            }
            int val = tpl_eval_cond_c(cond, scopes, nscopes);
            free(cond);
            i++;
            char *term = NULL;
            int ni = tpl_render_range_c(toks, i, end, scopes, nscopes, out, depth + 1, line, col, &term);
            i = ni;
            if (term && strcmp(term, "وإلا") == 0) {
                free(term);
                if (val) {
                    i = tpl_find_end_c(toks, i + 1, end, "إذا", line, col) + 1;
                } else {
                    char *term2 = NULL;
                    int ni2 = tpl_render_range_c(toks, i + 1, end, scopes, nscopes, out, depth + 1, line, col, &term2);
                    if (!term2 || strcmp(term2, "/إذا") != 0) {
                        free(term2);
                        free(tag);
                        nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
                    }
                    free(term2);
                    i = ni2 + 1;
                }
            } else if (term && strcmp(term, "/إذا") == 0) {
                free(term);
                i++;
            } else {
                free(term);
                free(tag);
                nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
            }
            free(tag);
            continue;
        }
        if (strcmp(tag, "لكل") == 0 || strncmp(tag, "لكل ", 7) == 0) {
            char *rest = tpl_trim_c(tag + 6);
            // قسّم على المسافات: <متغير> في <مسار>.
            char *toks3[8];
            int nt = 0;
            char *save = NULL;
            char *tok = strtok_r(rest, " \t", &save);
            while (tok && nt < 8) {
                toks3[nt++] = tok;
                tok = strtok_r(NULL, " \t", &save);
            }
            if (nt != 3 || strcmp(toks3[1], "في") != 0) {
                free(rest);
                free(tag);
                nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
            }
            char *var = j14_strdup(toks3[0]);
            char *path = j14_strdup(toks3[2]);
            free(rest);
            NvValue list;
            if (!tpl_lookup_path_c(path, scopes, nscopes, &list)) {
                char *msg = (char *)malloc(strlen(path) + 64);
                snprintf(msg, strlen(path) + 64, "متغير قالب غير موجود: %s", path);
                free(var);
                free(path);
                free(tag);
                nv_error(NV_ERR_RUNTIME, msg, line, col);
            }
            if (list.tag != NV_LIST) {
                free(var);
                free(path);
                free(tag);
                nv_error(NV_ERR_RUNTIME, "قيمة قالب غير صالحة", line, col);
            }
            int body_end = tpl_find_end_c(toks, i + 1, end, "لكل", line, col);
            for (size_t idx = 0; idx < list.u.list->len; idx++) {
                TplBind binds[2];
                binds[0].name = var;
                binds[0].val = list.u.list->items[idx];
                binds[1].name = "مؤشر";
                binds[1].val = nv_num((double)idx);
                scopes[nscopes].binds = binds;
                scopes[nscopes].nbinds = 2;
                char *term = NULL;
                tpl_render_range_c(toks, i + 1, body_end, scopes, nscopes + 1, out, depth + 1, line, col, &term);
                if (term) {
                    free(term);
                    free(var);
                    free(path);
                    free(tag);
                    nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
                }
            }
            free(var);
            free(path);
            free(tag);
            i = body_end + 1;
            continue;
        }
        if (tag[0] == '/') {
            free(tag);
            nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
        }
        NvValue v;
        if (!tpl_lookup_path_c(tag, scopes, nscopes, &v)) {
            char *msg = (char *)malloc(strlen(tag) + 64);
            snprintf(msg, strlen(tag) + 64, "متغير قالب غير موجود: %s", tag);
            free(tag);
            nv_error(NV_ERR_RUNTIME, msg, line, col);
        }
        char *s = nv_fmt_value(v);
        tpl_push_c(out, s, line, col);
        free(s);
        free(tag);
        i++;
    }
    return i;
}

static NvValue nv_std_template_apply(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوالب.طبق", args, nargs, 2, line, col);
    const char *tpl = nv_std_need_string("قوالب.طبق", args[0], line, col);
    if (args[1].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قاموسا لكن أعطيت %s",
                     "قوالب.طبق", nv_type_name(args[1]));
    }
    NvDict *pairs = args[1].u.dict;
    if (nv_utf8_chars(tpl, strlen(tpl)) > TPL_MAX_CHARS) {
        nv_error(NV_ERR_RUNTIME, "القالب كبير جدا", line, col);
    }
    // ترميز الرموز.
    TplTokC *toks = NULL;
    int ntoks = 0, captoks = 0;
    J14Buf text;
    j14_buf_init(&text);
    size_t tlen = strlen(tpl);
    size_t i = 0;
    while (i < tlen) {
        if (tpl[i] == '\\' && i + 2 < tlen && tpl[i + 1] == '{' && tpl[i + 2] == '{') {
            j14_buf_add(&text, "{{");
            i += 3;
            continue;
        }
        if (tpl[i] == '{' && i + 1 < tlen && tpl[i + 1] == '{') {
            size_t j = i + 2;
            int found = 0;
            while (j + 1 < tlen) {
                if (tpl[j] == '}' && tpl[j + 1] == '}') {
                    found = 1;
                    break;
                }
                j++;
            }
            if (!found) {
                j14_buf_free(&text);
                for (int k = 0; k < ntoks; k++) free(toks[k].text);
                free(toks);
                nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
            }
            if (text.len > 0) {
                if (ntoks == captoks) {
                    captoks = captoks ? captoks * 2 : 16;
                    toks = (TplTokC *)realloc(toks, captoks * sizeof(TplTokC));
                }
                toks[ntoks].is_tag = 0;
                toks[ntoks].text = j14_strdup(text.data);
                ntoks++;
                text.len = 0;
                text.data[0] = '\0';
            }
            char *raw = j14_strndup(tpl + i + 2, j - (i + 2));
            char *tag = tpl_trim_c(raw);
            free(raw);
            if (tag[0] == '\0') {
                free(tag);
                j14_buf_free(&text);
                for (int k = 0; k < ntoks; k++) free(toks[k].text);
                free(toks);
                nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
            }
            if (ntoks == captoks) {
                captoks = captoks ? captoks * 2 : 16;
                toks = (TplTokC *)realloc(toks, captoks * sizeof(TplTokC));
            }
            toks[ntoks].is_tag = 1;
            toks[ntoks].text = tag;
            ntoks++;
            i = j + 2;
            continue;
        }
        j14_buf_addc(&text, tpl[i]);
        i++;
    }
    if (text.len > 0) {
        if (ntoks == captoks) {
            captoks = captoks ? captoks * 2 : 16;
            toks = (TplTokC *)realloc(toks, captoks * sizeof(TplTokC));
        }
        toks[ntoks].is_tag = 0;
        toks[ntoks].text = j14_strdup(text.data);
        ntoks++;
    }
    j14_buf_free(&text);
    TplScope scopes[32];
    TplBind *root_binds = (TplBind *)malloc(
        (pairs->len > 0 ? pairs->len : 1) * sizeof(TplBind));
    for (size_t k = 0; k < pairs->len; k++) {
        root_binds[k].name = pairs->entries[k].key;
        root_binds[k].val = pairs->entries[k].val;
    }
    scopes[0].binds = root_binds;
    scopes[0].nbinds = (int)pairs->len;
    J14Buf out;
    j14_buf_init(&out);
    char *term = NULL;
    int ni = tpl_render_range_c(toks, 0, ntoks, scopes, 1, &out, 0, line, col, &term);
    if (term || ni != ntoks) {
        free(term);
        j14_buf_free(&out);
        free(root_binds);
        for (int k = 0; k < ntoks; k++) free(toks[k].text);
        free(toks);
        nv_error(NV_ERR_RUNTIME, "قالب غير صالح", line, col);
    }
    NvValue result = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    free(root_binds);
    for (int k = 0; k < ntoks; k++) free(toks[k].text);
    free(toks);
    return result;
}

void nv_std_template_fill(NvDict *d) {
    nv_std_add_fn(d, "طبق", nv_std_template_apply);
}

// ---------- 213: التحقق (مرآة Rust حرفيا — على محرك النمط) ----------

static NvValue nv_std_validate_email(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تحقق.بريد", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("تحقق.بريد", args[0], line, col);
    return nv_bool(rx_full_match_c(
        "^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}$", s, line, col));
}

static NvValue nv_std_validate_url(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تحقق.رابط", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("تحقق.رابط", args[0], line, col);
    return nv_bool(rx_full_match_c(
        "^https?://[A-Za-z0-9.-]+(:[0-9]+)?(/[^ ]*)?$", s, line, col));
}

static NvValue nv_std_validate_phone(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تحقق.هاتف", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("تحقق.هاتف", args[0], line, col);
    int shape = rx_full_match_c("^\\+?[0-9][0-9 ()-]{6,19}$", s, line, col);
    int digits = 0;
    for (const char *p = s; *p; p++) {
        if (*p >= '0' && *p <= '9') digits++;
    }
    return nv_bool(shape && digits >= 7);
}

static NvValue nv_std_validate_number(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تحقق.عدد", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("تحقق.عدد", args[0], line, col);
    return nv_bool(rx_full_match_c("^-?[0-9]+(\\.[0-9]+)?$", s, line, col));
}

static NvValue nv_std_validate_date(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تحقق.تاريخ", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("تحقق.تاريخ", args[0], line, col);
    int shape = rx_full_match_c("^[0-9]{4}-[0-9]{2}-[0-9]{2}$", s, line, col);
    if (!shape) return nv_bool(0);
    int y = (s[0] - '0') * 1000 + (s[1] - '0') * 100 + (s[2] - '0') * 10 + (s[3] - '0');
    int m = (s[5] - '0') * 10 + (s[6] - '0');
    int d = (s[8] - '0') * 10 + (s[9] - '0');
    if (m < 1 || m > 12 || d < 1) return nv_bool(0);
    int leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    int dim;
    if (m == 1 || m == 3 || m == 5 || m == 7 || m == 8 || m == 10 || m == 12) dim = 31;
    else if (m == 4 || m == 6 || m == 9 || m == 11) dim = 30;
    else dim = leap ? 29 : 28;
    return nv_bool(d <= dim);
}

void nv_std_validate_fill(NvDict *d) {
    nv_std_add_fn(d, "بريد", nv_std_validate_email);
    nv_std_add_fn(d, "رابط", nv_std_validate_url);
    nv_std_add_fn(d, "هاتف", nv_std_validate_phone);
    nv_std_add_fn(d, "عدد", nv_std_validate_number);
    nv_std_add_fn(d, "تاريخ", nv_std_validate_date);
}

// ============================================================
// باقي J14 (الدفعة الثالثة من برنامج الـ579، الاجتماع 73 §88):
// البنود 205–211 و214–217 — مرآة Rust حرفيا.
// المفاتيح المخزنة بلا شدة دائما: نسق، شفر، ولد، حول_وحدات.
// ============================================================

// ---------- مساعدات مشتركة لباقي J14 ----------

static const NvValue *j14r_dict_find(const NvDict *d, const char *key) {
    size_t i = 0;
    for (i = 0; i < d->len; i++) {
        if (strcmp(d->entries[i].key, key) == 0) return &d->entries[i].val;
    }
    return NULL;
}

static NvList *j14r_need_list(const char *display, NvValue v, int line, int col) {
    if (v.tag != NV_LIST) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.list;
}

// نسخة بايتات: نص (بايتات UTF-8) أو قائمة أعداد 0..255.
static unsigned char *j14r_need_bytes(const char *display, NvValue v, size_t *len_out, int line, int col) {
    unsigned char *out = NULL;
    if (v.tag == NV_STR) {
        size_t n = v.u.str->len;
        out = (unsigned char *)malloc(n > 0 ? n : 1);
        memcpy(out, v.u.str->data, n);
        *len_out = n;
        return out;
    }
    if (v.tag == NV_LIST) {
        size_t n = v.u.list->len;
        size_t i = 0;
        out = (unsigned char *)malloc(n > 0 ? n : 1);
        for (i = 0; i < n; i++) {
            NvValue item = v.u.list->items[i];
            if (item.tag != NV_NUM || item.u.num != trunc(item.u.num) || item.u.num < 0 || item.u.num > 255) {
                free(out);
                nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة بايتات (أعداد 0..255)", display);
            }
            out[i] = (unsigned char)item.u.num;
        }
        *len_out = n;
        return out;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نصا أو قائمة بايتات لكن أعطيت %s", display, nv_type_name(v));
    return NULL;
}

static NvValue j14r_list_of_bytes(const unsigned char *bytes, size_t n) {
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < n; i++) nv_list_push_raw(out.u.list, nv_num((double)bytes[i]));
    nv_root_pop();
    return out;
}

static NvValue j14r_list_of_nums(const double *nums, size_t n) {
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < n; i++) nv_list_push_raw(out.u.list, nv_num(nums[i]));
    nv_root_pop();
    return out;
}

static NvValue j14r_list_of_strs(const char **strs, size_t n) {
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < n; i++) nv_list_push_raw(out.u.list, nv_str_cstr(strs[i]));
    nv_root_pop();
    return out;
}

// تعريب عدد غربي: أرقام → مشرقية، '.' → ٫، ',' → ٬.
static char *j14r_arabicize(const char *s) {
    static const char *DIG[10] = {"٠", "١", "٢", "٣", "٤", "٥", "٦", "٧", "٨", "٩"};
    J14Buf out;
    j14_buf_init(&out);
    const unsigned char *p = (const unsigned char *)s;
    while (*p) {
        if (*p >= '0' && *p <= '9') j14_buf_add(&out, DIG[*p - '0']);
        else if (*p == '.') j14_buf_add(&out, "٫");
        else if (*p == ',') j14_buf_add(&out, "٬");
        else j14_buf_addc(&out, (char)*p);
        p++;
    }
    char *res = out.data;
    return res;
}

// عدد غربي قصير (كما json_fmt_num_c: صحيح %lld وإلا
// أقصر تمثيل يعود إلى العدد نفسه).
static void j14r_plain_num(double n, J14Buf *out) {
    json_fmt_num_c(n, out);
}

static uint64_t j14r_splitmix_next(uint64_t *state) {
    *state += 0x9E3779B97F4A7C15ULL;
    uint64_t z = *state;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

// ---------- 205: تنسيق التاريخ/الوقت (إكمال وحدة وقت) ----------

static NvValue nv_std_time_format_custom(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("وقت.نسق", args, nargs, 2, line, col);
    NvStdTimeDt dt;
    if (args[0].tag == NV_NUM) {
        double x = nv_std_need_number("وقت.نسق", args[0], line, col);
        if (x != trunc(x) || x < -9.0e15 || x >= 9.0e15) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع عددا صحيحا للثواني", "وقت.نسق");
        }
        long long epoch = (long long)x;
        dt = nv_std_time_from_epoch(epoch, 0, "وقت.نسق", line, col);
    } else if (args[0].tag == NV_DICT) {
        dt = nv_std_time_from_dict(args[0], "وقت.نسق", line, col);
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة وقت.نسق تتوقع قاموس تاريخ أو ثواني لكن أعطيت %s", nv_type_name(args[0]));
    }
    const char *fmt = nv_std_need_string("وقت.نسق", args[1], line, col);
    if (nv_utf8_chars(fmt, strlen(fmt)) > 1000) {
        nv_error(NV_ERR_RUNTIME, "صيغة التاريخ كبيرة جدا", line, col);
    }
    J14Buf out;
    j14_buf_init(&out);
    const char *p = fmt;
    while (*p) {
        if (*p != '%') {
            j14_buf_addc(&out, *p);
            p++;
            continue;
        }
        p++;
        if (!*p) {
            j14_buf_free(&out);
            nv_error(NV_ERR_RUNTIME, "صيغة تاريخ غير صالحة: % في النهاية", line, col);
        }
        char tok = *p++;
        char tmp[64];
        switch (tok) {
            case 'Y': snprintf(tmp, sizeof(tmp), "%04lld", dt.year); j14_buf_add(&out, tmp); break;
            case 'm': snprintf(tmp, sizeof(tmp), "%02ld", dt.month); j14_buf_add(&out, tmp); break;
            case 'd': snprintf(tmp, sizeof(tmp), "%02ld", dt.day); j14_buf_add(&out, tmp); break;
            case 'H': snprintf(tmp, sizeof(tmp), "%02ld", dt.hour); j14_buf_add(&out, tmp); break;
            case 'M': snprintf(tmp, sizeof(tmp), "%02ld", dt.minute); j14_buf_add(&out, tmp); break;
            case 'S': snprintf(tmp, sizeof(tmp), "%02ld", dt.second); j14_buf_add(&out, tmp); break;
            case 'e': snprintf(tmp, sizeof(tmp), "%ld", dt.day); j14_buf_add(&out, tmp); break;
            case 'k': snprintf(tmp, sizeof(tmp), "%ld", dt.hour); j14_buf_add(&out, tmp); break;
            case 'A': j14_buf_add(&out, nv_std_time_weekday_name_str(dt.weekday)); break;
            case 'B': j14_buf_add(&out, nv_std_time_month_name_str(dt.month)); break;
            case '%': j14_buf_addc(&out, '%'); break;
            default:
                j14_buf_free(&out);
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "صيغة تاريخ غير صالحة: %%%c", tok);
        }
    }
    NvValue res = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return res;
}

// ---------- 206: تنسيق النصوص (وحدة نص) ----------

static int j14r_text_spec_num(double n, const char *spec, J14Buf *out) {
    const char *rest = spec;
    int grouping = 0;
    long zero_width = -1;
    long precision = -1;
    if (*rest == ',') { grouping = 1; rest++; }
    if (*rest == '0') {
        rest++;
        if (!(*rest >= '0' && *rest <= '9')) return 0;
        zero_width = 0;
        while (*rest >= '0' && *rest <= '9') { zero_width = zero_width * 10 + (*rest - '0'); rest++; }
    }
    if (*rest == '.') {
        rest++;
        if (!(*rest >= '0' && *rest <= '9')) return 0;
        precision = 0;
        while (*rest >= '0' && *rest <= '9') { precision = precision * 10 + (*rest - '0'); rest++; }
    }
    if (*rest != '\0') return 0;
    char numbuf[128];
    if (precision >= 0) {
        snprintf(numbuf, sizeof(numbuf), "%.*f", (int)precision, n);
    } else {
        J14Buf tmp;
        j14_buf_init(&tmp);
        j14r_plain_num(n, &tmp);
        snprintf(numbuf, sizeof(numbuf), "%s", tmp.data);
        j14_buf_free(&tmp);
    }
    if (grouping) {
        const char *sign = "";
        const char *body = numbuf;
        if (numbuf[0] == '-') { sign = "-"; body = numbuf + 1; }
        const char *dot = strchr(body, '.');
        size_t intlen = dot ? (size_t)(dot - body) : strlen(body);
        J14Buf g;
        j14_buf_init(&g);
        j14_buf_add(&g, sign);
        size_t i = 0;
        for (i = 0; i < intlen; i++) {
            if (i > 0 && (intlen - i) % 3 == 0) j14_buf_addc(&g, ',');
            j14_buf_addc(&g, body[i]);
        }
        if (dot) j14_buf_add(&g, dot);
        char *arab = j14r_arabicize(g.data);
        j14_buf_add(out, arab);
        free(arab);
        j14_buf_free(&g);
    } else if (zero_width >= 0) {
        const char *sign = "";
        const char *body = numbuf;
        if (numbuf[0] == '-') { sign = "-"; body = numbuf + 1; }
        const char *dot = strchr(body, '.');
        size_t intlen = dot ? (size_t)(dot - body) : strlen(body);
        J14Buf g;
        j14_buf_init(&g);
        j14_buf_add(&g, sign);
        while ((long)intlen < zero_width) { j14_buf_addc(&g, '0'); intlen++; }
        j14_buf_add(&g, body);
        char *arab = j14r_arabicize(g.data);
        j14_buf_add(out, arab);
        free(arab);
        j14_buf_free(&g);
    } else {
        char *arab = j14r_arabicize(numbuf);
        j14_buf_add(out, arab);
        free(arab);
    }
    return 1;
}

static NvValue nv_std_text_format(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.نسق", args, nargs, 2, line, col);
    const char *tpl = nv_std_need_string("نص.نسق", args[0], line, col);
    if (nv_utf8_chars(tpl, strlen(tpl)) > 100000) {
        nv_error(NV_ERR_RUNTIME, "قالب التنسيق كبير جدا", line, col);
    }
    int is_list = args[1].tag == NV_LIST;
    int is_dict = args[1].tag == NV_DICT;
    if (!is_list && !is_dict) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة نص.نسق تتوقع قائمة أو قاموسا لكن أعطيت %s", nv_type_name(args[1]));
    }
    J14Buf out;
    j14_buf_init(&out);
    size_t tlen = strlen(tpl);
    size_t i = 0;
    while (i < tlen) {
        char c = tpl[i];
        if (c == '{') {
            if (i + 1 < tlen && tpl[i + 1] == '{') { j14_buf_addc(&out, '{'); i += 2; continue; }
            size_t j = i + 1;
            while (j < tlen && tpl[j] != '}') j++;
            if (j >= tlen) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: قوس غير مغلق", line, col);
            }
            if (j == i + 1) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: موضع فارغ", line, col);
            }
            char inner[512];
            size_t ilen = j - (i + 1);
            if (ilen >= sizeof(inner)) {
                j14_buf_free(&out);
                nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: موضع طويل جدا", line, col);
            }
            memcpy(inner, tpl + i + 1, ilen);
            inner[ilen] = '\0';
            char *colon = strchr(inner, ':');
            char *spec = NULL;
            if (colon) { *colon = '\0'; spec = colon + 1; }
            int all_digits = 1;
            for (char *q = inner; *q; q++) if (*q < '0' || *q > '9') all_digits = 0;
            NvValue val;
            int found = 0;
            if (all_digits) {
                if (!is_list) {
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: موضع رقمي بلا قائمة", line, col);
                }
                long idx = atol(inner);
                if (idx < 0 || (size_t)idx >= args[1].u.list->len) {
                    j14_buf_free(&out);
                    nv_error_fmt(NV_ERR_RUNTIME, line, col, "موضع تنسيق غير موجود: %ld", idx);
                }
                val = args[1].u.list->items[idx];
                found = 1;
            } else {
                if (!is_dict) {
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: اسم بلا قاموس", line, col);
                }
                const NvValue *v = j14r_dict_find(args[1].u.dict, inner);
                if (!v) {
                    j14_buf_free(&out);
                    nv_error_fmt(NV_ERR_RUNTIME, line, col, "اسم تنسيق غير موجود: %s", inner);
                }
                val = *v;
                found = 1;
            }
            (void)found;
            if (!spec) {
                char *s = nv_fmt_value(val);
                j14_buf_add(&out, s);
                free(s);
            } else {
                if (val.tag != NV_NUM) {
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "مواصفة تنسيق لغير عدد", line, col);
                }
                if (!j14r_text_spec_num(val.u.num, spec, &out)) {
                    j14_buf_free(&out);
                    nv_error(NV_ERR_RUNTIME, "مواصفة تنسيق غير صالحة", line, col);
                }
            }
            i = j + 1;
            continue;
        }
        if (c == '}') {
            if (i + 1 < tlen && tpl[i + 1] == '}') { j14_buf_addc(&out, '}'); i += 2; continue; }
            j14_buf_free(&out);
            nv_error(NV_ERR_RUNTIME, "قالب تنسيق غير صالح: قوس منفرد", line, col);
        }
        j14_buf_addc(&out, c);
        i++;
    }
    if (nv_utf8_chars(out.data, out.len) > 1000000) {
        j14_buf_free(&out);
        nv_error(NV_ERR_RUNTIME, "ناتج التنسيق كبير جدا", line, col);
    }
    NvValue res = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return res;
}

/// يبدأ_ب وينتهي_ب وقسم_كلمات وعدد_مسافات_بادئة
/// ومن_الفهرس وقبل_الفهرس واسم_الملف — مرايا دوال
/// وحدة نص الملفية (حزم/نص.نيفا) بنفس الدلالة حرفيا.
/// المقارنة بالمحارف عبر nv_utf8_chars؛ وشرط البادئة
/// بالبايتات صحيح لأن البادئة محاذية لحد محرف.

static NvValue nv_std_text_starts_with(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.يبدأ_ب", args, nargs, 2, line, col);
    const char *text = nv_std_need_string("نص.يبدأ_ب", args[0], line, col);
    const char *prefix = nv_std_need_string("نص.يبدأ_ب", args[1], line, col);
    size_t tchars = nv_utf8_chars(text, strlen(text));
    size_t pchars = nv_utf8_chars(prefix, strlen(prefix));
    if (pchars > tchars) {
        return nv_bool(0);
    }
    return nv_bool(strncmp(text, prefix, strlen(prefix)) == 0);
}

static NvValue nv_std_text_ends_with(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.ينتهي_ب", args, nargs, 2, line, col);
    const char *text = nv_std_need_string("نص.ينتهي_ب", args[0], line, col);
    const char *suffix = nv_std_need_string("نص.ينتهي_ب", args[1], line, col);
    size_t tchars = nv_utf8_chars(text, strlen(text));
    size_t schars = nv_utf8_chars(suffix, strlen(suffix));
    if (schars > tchars) {
        return nv_bool(0);
    }
    size_t tlen = strlen(text);
    size_t slen = strlen(suffix);
    return nv_bool(memcmp(text + (tlen - slen), suffix, slen) == 0);
}

static NvValue nv_std_text_split_words(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.قسم_كلمات", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("نص.قسم_كلمات", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    const char *p = text;
    while (*p) {
        while (*p == ' ') {
            p++;
        }
        if (!*p) {
            break;
        }
        const char *start = p;
        while (*p && *p != ' ') {
            p++;
        }
        nv_list_push_raw(out.u.list, nv_str_new(start, (size_t)(p - start)));
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_text_leading_spaces(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.عدد_مسافات_بادئة", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("نص.عدد_مسافات_بادئة", args[0], line, col);
    size_t n = 0;
    while (text[n] == ' ') {
        n++;
    }
    return nv_num((double)n);
}

static NvValue nv_std_text_from_index(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.من_الفهرس", args, nargs, 2, line, col);
    const char *text = nv_std_need_string("نص.من_الفهرس", args[0], line, col);
    long long idx = nv_std_need_integer("نص.من_الفهرس", args[1], line, col);
    size_t tlen = strlen(text);
    size_t nchars = nv_utf8_chars(text, tlen);
    if (idx < 0 || (size_t)idx > nchars) {
        nv_error(NV_ERR_RUNTIME, "الفهرس خارج حدود النص في نص.من_الفهرس", line, col);
    }
    if ((size_t)idx == nchars) {
        return nv_str_new("", 0);
    }
    size_t clen = 0;
    const char *start = nv_utf8_char_at(text, tlen, (size_t)idx, &clen);
    return nv_str_new(start, tlen - (size_t)(start - text));
}

static NvValue nv_std_text_before_index(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.قبل_الفهرس", args, nargs, 2, line, col);
    const char *text = nv_std_need_string("نص.قبل_الفهرس", args[0], line, col);
    long long idx = nv_std_need_integer("نص.قبل_الفهرس", args[1], line, col);
    size_t tlen = strlen(text);
    size_t nchars = nv_utf8_chars(text, tlen);
    if (idx < 0 || (size_t)idx > nchars) {
        nv_error(NV_ERR_RUNTIME, "الفهرس خارج حدود النص في نص.قبل_الفهرس", line, col);
    }
    if (idx == 0) {
        return nv_str_new("", 0);
    }
    size_t clen = 0;
    const char *end = nv_utf8_char_at(text, tlen, (size_t)idx, &clen);
    return nv_str_new(text, (size_t)(end - text));
}

static NvValue nv_std_text_file_name(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.اسم_الملف", args, nargs, 1, line, col);
    const char *path = nv_std_need_string("نص.اسم_الملف", args[0], line, col);
    const char *slash = strrchr(path, '/');
    const char *name = slash ? slash + 1 : path;
    return nv_str_new(name, strlen(name));
}

void nv_std_text_fill(NvDict *d) {
    nv_std_add_fn(d, "نسق", nv_std_text_format);
    nv_std_add_fn(d, "يبدأ_ب", nv_std_text_starts_with);
    nv_std_add_fn(d, "ينتهي_ب", nv_std_text_ends_with);
    nv_std_add_fn(d, "قسم_كلمات", nv_std_text_split_words);
    // J3-74: قسّم_إلى_كلمات — أي مسافة بيضاء ASCII.
    nv_std_add_fn(d, "قسم_إلى_كلمات", nv_std_text_split_words_all);
    nv_std_add_fn(d, "عدد_مسافات_بادئة", nv_std_text_leading_spaces);
    nv_std_add_fn(d, "من_الفهرس", nv_std_text_from_index);
    nv_std_add_fn(d, "قبل_الفهرس", nv_std_text_before_index);
    nv_std_add_fn(d, "اسم_الملف", nv_std_text_file_name);
}

// ---------- 207: تحويل القواعد العددية (وحدة أسس) ----------

static const char *J14R_BASE_DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz";

static NvValue nv_std_bases_to_base(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أسس.إلى_أساس", args, nargs, 2, line, col);
    long long n = nv_std_need_integer("أسس.إلى_أساس", args[0], line, col);
    long long base = nv_std_need_integer("أسس.إلى_أساس", args[1], line, col);
    if (base < 2 || base > 36) {
        nv_error(NV_ERR_TYPE, "الدالة أسس.إلى_أساس تتوقع أساسا بين 2 و 36", line, col);
    }
    if (n == 0) return nv_str_cstr("0");
    int neg = n < 0;
    unsigned long long v = neg ? (unsigned long long)(-(n + 1)) + 1 : (unsigned long long)n;
    char rev[70];
    int k = 0;
    while (v > 0) {
        rev[k++] = J14R_BASE_DIGITS[v % (unsigned long long)base];
        v /= (unsigned long long)base;
    }
    if (neg) rev[k++] = '-';
    char out[70];
    int i = 0;
    while (k > 0) out[i++] = rev[--k];
    out[i] = '\0';
    return nv_str_new(out, (size_t)i);
}

static NvValue nv_std_bases_from_base(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أسس.من_أساس", args, nargs, 2, line, col);
    const char *text = nv_std_need_string("أسس.من_أساس", args[0], line, col);
    long long base = nv_std_need_integer("أسس.من_أساس", args[1], line, col);
    if (base < 2 || base > 36) {
        nv_error(NV_ERR_TYPE, "الدالة أسس.من_أساس تتوقع أساسا بين 2 و 36", line, col);
    }
    while (*text == ' ' || *text == '\t' || *text == '\n' || *text == '\r') text++;
    size_t tlen = strlen(text);
    while (tlen > 0 && (text[tlen - 1] == ' ' || text[tlen - 1] == '\t' || text[tlen - 1] == '\n' || text[tlen - 1] == '\r')) tlen--;
    if (tlen == 0) nv_error(NV_ERR_RUNTIME, "نص أساس فارغ", line, col);
    int neg = 0;
    size_t pos = 0;
    if (text[0] == '-') { neg = 1; pos = 1; }
    if (pos >= tlen) nv_error(NV_ERR_RUNTIME, "نص أساس غير صالح", line, col);
    long long value = 0;
    while (pos < tlen) {
        uint32_t cp = 0;
        size_t adv = j14_utf8_decode((const unsigned char *)text + pos, tlen - pos, &cp);
        if (adv == 0) adv = 1;
        long long d = -1;
        if (cp >= '0' && cp <= '9') d = (long long)(cp - '0');
        else if (cp >= 'a' && cp <= 'z') d = (long long)(cp - 'a' + 10);
        else if (cp >= 'A' && cp <= 'Z') d = (long long)(cp - 'A' + 10);
        else if (cp >= 0x660 && cp <= 0x669) d = (long long)(cp - 0x660);
        if (d < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "محرف غير صالح في نص الأساس");
        }
        if (d >= base) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "محرف خارج الأساس %lld", base);
        }
        if (value > (9223372036854775807LL - d) / base) {
            nv_error(NV_ERR_RUNTIME, "عدد كبير جدا", line, col);
        }
        value = value * base + d;
        pos += adv;
    }
    if (neg) value = -value;
    return nv_num((double)value);
}

void nv_std_bases_fill(NvDict *d) {
    nv_std_add_fn(d, "إلى_أساس", nv_std_bases_to_base);
    nv_std_add_fn(d, "من_أساس", nv_std_bases_from_base);
}

// ---------- 208: تحويل الوحدات (وحدة وحدات) ----------

static int j14r_units_linear(const char *name, const char **cat, double *factor) {
    typedef struct { const char *name; const char *cat; double f; } UEnt;
    static const UEnt T[] = {
        {"متر", "طول", 1.0}, {"كيلومتر", "طول", 1000.0}, {"سنتيمتر", "طول", 0.01},
        {"مليمتر", "طول", 0.001}, {"ميل", "طول", 1609.344}, {"ياردة", "طول", 0.9144},
        {"قدم", "طول", 0.3048}, {"بوصة", "طول", 0.0254}, {"ميل_بحري", "طول", 1852.0},
        {"كيلوغرام", "وزن", 1.0}, {"غرام", "وزن", 0.001}, {"طن", "وزن", 1000.0},
        {"رطل", "وزن", 0.45359237}, {"أونصة", "وزن", 0.028349523125},
        {"لتر", "حجم", 1.0}, {"مليلتر", "حجم", 0.001}, {"متر_مكعب", "حجم", 1000.0},
        {"غالون", "حجم", 3.785411784}, {"كوب", "حجم", 0.2365882365},
        {"متر_مربع", "مساحة", 1.0}, {"كيلومتر_مربع", "مساحة", 1000000.0},
        {"هكتار", "مساحة", 10000.0}, {"فدان", "مساحة", 4200.833},
        {"قدم_مربع", "مساحة", 0.09290304},
    };
    size_t i = 0;
    for (i = 0; i < sizeof(T) / sizeof(T[0]); i++) {
        if (strcmp(name, T[i].name) == 0) { *cat = T[i].cat; *factor = T[i].f; return 1; }
    }
    return 0;
}

static int j14r_units_is_temp(const char *name) {
    return strcmp(name, "مئوية") == 0 || strcmp(name, "فهرنهايت") == 0 || strcmp(name, "كلفن") == 0;
}

static NvValue nv_std_units_convert(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("وحدات.حول_وحدات", args, nargs, 3, line, col);
    double value = nv_std_need_number("وحدات.حول_وحدات", args[0], line, col);
    const char *from = nv_std_need_string("وحدات.حول_وحدات", args[1], line, col);
    const char *to = nv_std_need_string("وحدات.حول_وحدات", args[2], line, col);
    if (j14r_units_is_temp(from) || j14r_units_is_temp(to)) {
        if (!j14r_units_is_temp(from) || !j14r_units_is_temp(to)) {
            nv_error(NV_ERR_RUNTIME, "وحدتا حرارة وغير حرارة لا تتحول إحداهما إلى الأخرى", line, col);
        }
        double c;
        if (strcmp(from, "مئوية") == 0) c = value;
        else if (strcmp(from, "فهرنهايت") == 0) c = (value - 32.0) * 5.0 / 9.0;
        else c = value - 273.15;
        double out;
        if (strcmp(to, "مئوية") == 0) out = c;
        else if (strcmp(to, "فهرنهايت") == 0) out = c * 9.0 / 5.0 + 32.0;
        else out = c + 273.15;
        return nv_num(out);
    }
    const char *cat_from = NULL, *cat_to = NULL;
    double f_from = 0, f_to = 0;
    if (!j14r_units_linear(from, &cat_from, &f_from)) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "وحدة غير معروفة: %s", from);
    }
    if (!j14r_units_linear(to, &cat_to, &f_to)) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "وحدة غير معروفة: %s", to);
    }
    if (strcmp(cat_from, cat_to) != 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "وحدتا %s و%s من فئتين مختلفتين", from, to);
    }
    return nv_num(value * f_from / f_to);
}

void nv_std_units_fill(NvDict *d) {
    nv_std_add_fn(d, "حول_وحدات", nv_std_units_convert);
}

// ---------- 209: تشفير ChaCha20 (إكمال وحدة تشفير) ----------

static void j14r_chacha_quarter(uint32_t w[16], int a, int b, int c, int d) {
    w[a] += w[b]; w[d] ^= w[a]; w[d] = (w[d] << 16) | (w[d] >> 16);
    w[c] += w[d]; w[b] ^= w[c]; w[b] = (w[b] << 12) | (w[b] >> 20);
    w[a] += w[b]; w[d] ^= w[a]; w[d] = (w[d] << 8) | (w[d] >> 24);
    w[c] += w[d]; w[b] ^= w[c]; w[b] = (w[b] << 7) | (w[b] >> 25);
}

static void j14r_chacha_block(const unsigned char key[32], uint32_t counter, const unsigned char nonce[12], unsigned char out[64]) {
    uint32_t s[16];
    s[0] = 0x61707865; s[1] = 0x3320646e; s[2] = 0x79622d32; s[3] = 0x6b206574;
    int i = 0;
    for (i = 0; i < 8; i++) {
        s[4 + i] = (uint32_t)key[4 * i] | ((uint32_t)key[4 * i + 1] << 8) |
                   ((uint32_t)key[4 * i + 2] << 16) | ((uint32_t)key[4 * i + 3] << 24);
    }
    s[12] = counter;
    for (i = 0; i < 3; i++) {
        s[13 + i] = (uint32_t)nonce[4 * i] | ((uint32_t)nonce[4 * i + 1] << 8) |
                    ((uint32_t)nonce[4 * i + 2] << 16) | ((uint32_t)nonce[4 * i + 3] << 24);
    }
    uint32_t w[16];
    for (i = 0; i < 16; i++) w[i] = s[i];
    int r = 0;
    for (r = 0; r < 10; r++) {
        j14r_chacha_quarter(w, 0, 4, 8, 12);
        j14r_chacha_quarter(w, 1, 5, 9, 13);
        j14r_chacha_quarter(w, 2, 6, 10, 14);
        j14r_chacha_quarter(w, 3, 7, 11, 15);
        j14r_chacha_quarter(w, 0, 5, 10, 15);
        j14r_chacha_quarter(w, 1, 6, 11, 12);
        j14r_chacha_quarter(w, 2, 7, 8, 13);
        j14r_chacha_quarter(w, 3, 4, 9, 14);
    }
    for (i = 0; i < 16; i++) {
        uint32_t v = w[i] + s[i];
        out[4 * i] = (unsigned char)(v & 0xFF);
        out[4 * i + 1] = (unsigned char)((v >> 8) & 0xFF);
        out[4 * i + 2] = (unsigned char)((v >> 16) & 0xFF);
        out[4 * i + 3] = (unsigned char)((v >> 24) & 0xFF);
    }
}

static unsigned char *j14r_chacha_xor(const unsigned char *data, size_t len, const unsigned char key[32], const unsigned char nonce[12]) {
    unsigned char *out = (unsigned char *)malloc(len > 0 ? len : 1);
    uint32_t counter = 1;
    size_t off = 0;
    while (off < len) {
        unsigned char ks[64];
        j14r_chacha_block(key, counter, nonce, ks);
        counter++;
        size_t chunk = len - off < 64 ? len - off : 64;
        size_t i = 0;
        for (i = 0; i < chunk; i++) out[off + i] = (unsigned char)(data[off + i] ^ ks[i]);
        off += chunk;
    }
    return out;
}

static void j14r_crypto_key_nonce(NvValue *args, const char *display, unsigned char key[32], unsigned char nonce[12], int line, int col) {
    size_t klen = 0, nlen = 0;
    unsigned char *k = j14r_need_bytes(display, args[1], &klen, line, col);
    unsigned char *n = j14r_need_bytes(display, args[2], &nlen, line, col);
    if (klen != 32) {
        free(k); free(n);
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع مفتاح من 32 بايتا لكن أعطيت %d", display, (int)klen);
    }
    if (nlen != 12) {
        free(k); free(n);
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع نون من 12 بايتا لكن أعطيت %d", display, (int)nlen);
    }
    memcpy(key, k, 32);
    memcpy(nonce, n, 12);
    free(k);
    free(n);
}

static NvValue nv_std_crypto_encrypt(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تشفير.شفر", args, nargs, 3, line, col);
    size_t dlen = 0;
    unsigned char *data = j14r_need_bytes("تشفير.شفر", args[0], &dlen, line, col);
    if (dlen > 10 * 1024 * 1024) {
        free(data);
        nv_error(NV_ERR_RUNTIME, "بيانات التشفير كبيرة جدا", line, col);
    }
    unsigned char key[32], nonce[12];
    j14r_crypto_key_nonce(args, "تشفير.شفر", key, nonce, line, col);
    unsigned char *out = j14r_chacha_xor(data, dlen, key, nonce);
    free(data);
    NvValue res = j14r_list_of_bytes(out, dlen);
    free(out);
    return res;
}

static NvValue nv_std_crypto_decrypt(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تشفير.فك_التشفير", args, nargs, 3, line, col);
    size_t dlen = 0;
    unsigned char *data = j14r_need_bytes("تشفير.فك_التشفير", args[0], &dlen, line, col);
    if (dlen > 10 * 1024 * 1024) {
        free(data);
        nv_error(NV_ERR_RUNTIME, "بيانات التشفير كبيرة جدا", line, col);
    }
    unsigned char key[32], nonce[12];
    j14r_crypto_key_nonce(args, "تشفير.فك_التشفير", key, nonce, line, col);
    unsigned char *out = j14r_chacha_xor(data, dlen, key, nonce);
    free(data);
    // التحقق من UTF-8 صالح (كما from_utf8 في Rust).
    size_t i = 0;
    int valid = 1;
    while (i < dlen) {
        unsigned char c = out[i];
        size_t need = 0;
        uint32_t cp = 0;
        if (c < 0x80) need = 1;
        else if ((c & 0xE0) == 0xC0) { need = 2; cp = c & 0x1F; }
        else if ((c & 0xF0) == 0xE0) { need = 3; cp = c & 0x0F; }
        else if ((c & 0xF8) == 0xF0) { need = 4; cp = c & 0x07; }
        else { valid = 0; break; }
        if (i + need > dlen) { valid = 0; break; }
        size_t k = 1;
        for (k = 1; k < need; k++) {
            if ((out[i + k] & 0xC0) != 0x80) { valid = 0; break; }
            cp = (cp << 6) | (out[i + k] & 0x3F);
        }
        if (!valid) break;
        if ((need == 2 && cp < 0x80) || (need == 3 && cp < 0x800) ||
            (need == 4 && cp < 0x10000) || cp > 0x10FFFF ||
            (cp >= 0xD800 && cp <= 0xDFFF)) { valid = 0; break; }
        i += need;
    }
    if (!valid) {
        free(out);
        nv_error(NV_ERR_RUNTIME, "نص غير صالح بعد فك التشفير", line, col);
    }
    NvValue res = nv_str_new((const char *)out, dlen);
    free(out);
    return res;
}

// ---------- 210: أرشفة ZIP (وحدة أرشيف) ----------

static uint32_t j14r_crc32(const unsigned char *data, size_t len) {
    static uint32_t table[256];
    static int built = 0;
    if (!built) {
        int i = 0, k = 0;
        for (i = 0; i < 256; i++) {
            uint32_t c = (uint32_t)i;
            for (k = 0; k < 8; k++) c = (c & 1) ? (0xEDB88320u ^ (c >> 1)) : (c >> 1);
            table[i] = c;
        }
        built = 1;
    }
    uint32_t c = 0xFFFFFFFFu;
    size_t i = 0;
    for (i = 0; i < len; i++) c = table[(c ^ data[i]) & 0xFF] ^ (c >> 8);
    return c ^ 0xFFFFFFFFu;
}

static void j14r_zip_u16(J14Buf *b, unsigned v) {
    j14_buf_addc(b, (char)(v & 0xFF));
    j14_buf_addc(b, (char)((v >> 8) & 0xFF));
}

static void j14r_zip_u32(J14Buf *b, uint32_t v) {
    j14_buf_addc(b, (char)(v & 0xFF));
    j14_buf_addc(b, (char)((v >> 8) & 0xFF));
    j14_buf_addc(b, (char)((v >> 16) & 0xFF));
    j14_buf_addc(b, (char)((v >> 24) & 0xFF));
}

static unsigned j14r_zip_ru16(const unsigned char *d, size_t at) {
    return (unsigned)d[at] | ((unsigned)d[at + 1] << 8);
}

static uint32_t j14r_zip_ru32(const unsigned char *d, size_t at) {
    return (uint32_t)d[at] | ((uint32_t)d[at + 1] << 8) |
           ((uint32_t)d[at + 2] << 16) | ((uint32_t)d[at + 3] << 24);
}

typedef struct { char *name; unsigned char *content; size_t clen; } J14rZipFile;

static int j14r_zip_parse(const unsigned char *data, size_t len, J14rZipFile **files_out, size_t *n_out, const char *display, int line, int col) {
    if (len == 22 && memcmp(data, "PK\x05\x06", 4) == 0) {
        *files_out = NULL;
        *n_out = 0;
        return 1;
    }
    size_t cap = 8, n = 0;
    J14rZipFile *files = (J14rZipFile *)malloc(cap * sizeof(J14rZipFile));
    size_t pos = 0;
    while (pos < len) {
        if (len - pos < 4 || memcmp(data + pos, "PK\x03\x04", 4) != 0) break;
        if (len - pos < 30) {
            free(files);
            nv_error(NV_ERR_RUNTIME, "أرشيف تالف: ترويسة ناقصة", line, col);
        }
        unsigned method = j14r_zip_ru16(data, pos + 8);
        if (method != 0) {
            free(files);
            nv_error(NV_ERR_RUNTIME, "طريقة ضغط غير مدعومة — التخزين فقط", line, col);
        }
        uint32_t crc = j14r_zip_ru32(data, pos + 14);
        uint32_t size = j14r_zip_ru32(data, pos + 22);
        size_t name_len = j14r_zip_ru16(data, pos + 26);
        size_t extra_len = j14r_zip_ru16(data, pos + 28);
        size_t name_at = pos + 30;
        size_t content_at = name_at + name_len + extra_len;
        if (content_at > len || (size_t)size > len - content_at) {
            free(files);
            nv_error(NV_ERR_RUNTIME, "أرشيف تالف: محتوى ناقص", line, col);
        }
        char *name = (char *)malloc(name_len + 1);
        memcpy(name, data + name_at, name_len);
        name[name_len] = '\0';
        unsigned char *content = (unsigned char *)malloc(size > 0 ? size : 1);
        memcpy(content, data + content_at, size);
        if (j14r_crc32(content, size) != crc) {
            free(name); free(content); free(files);
            nv_error(NV_ERR_RUNTIME, "أرشيف تالف: بصمة CRC غير مطابقة", line, col);
        }
        if (n >= cap) {
            cap *= 2;
            files = (J14rZipFile *)realloc(files, cap * sizeof(J14rZipFile));
        }
        files[n].name = name;
        files[n].content = content;
        files[n].clen = size;
        n++;
        if (n > 1000) {
            nv_error(NV_ERR_RUNTIME, "أرشيف كبير جدا", line, col);
        }
        pos = content_at + size;
    }
    if (n == 0) {
        free(files);
        nv_error(NV_ERR_RUNTIME, "أرشيف تالف: لا ملفات", line, col);
    }
    *files_out = files;
    *n_out = n;
    (void)display;
    return 1;
}

static void j14r_zip_free_files(J14rZipFile *files, size_t n) {
    size_t i = 0;
    for (i = 0; i < n; i++) { free(files[i].name); free(files[i].content); }
    free(files);
}

// نص بفقدان (كما from_utf8_lossy): بايت غير صالح ← U+FFFD.
static NvValue j14r_lossy_str(const unsigned char *data, size_t len) {
    J14Buf out;
    j14_buf_init(&out);
    size_t i = 0;
    while (i < len) {
        unsigned char c = data[i];
        size_t need = 0;
        if (c < 0x80) need = 1;
        else if ((c & 0xE0) == 0xC0) need = 2;
        else if ((c & 0xF0) == 0xE0) need = 3;
        else if ((c & 0xF8) == 0xF0) need = 4;
        else need = 0;
        int ok = 0;
        if (need > 1 && i + need <= len) {
            ok = 1;
            size_t k = 1;
            for (k = 1; k < need; k++) if ((data[i + k] & 0xC0) != 0x80) ok = 0;
        } else if (need == 1) ok = 1;
        if (ok) {
            j14_buf_addn(&out, (const char *)data + i, need);
            i += need;
        } else {
            j14_buf_add(&out, "\xEF\xBF\xBD");
            i += 1;
        }
    }
    NvValue res = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return res;
}

static NvValue nv_std_archive_compress(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أرشيف.اضغط", args, nargs, 1, line, col);
    NvList *items = j14r_need_list("أرشيف.اضغط", args[0], line, col);
    if (items->len > 1000) {
        nv_error(NV_ERR_RUNTIME, "قائمة ملفات كبيرة جدا", line, col);
    }
    J14Buf out;
    j14_buf_init(&out);
    J14Buf central;
    j14_buf_init(&central);
    size_t total = 0;
    size_t i = 0;
    for (i = 0; i < items->len; i++) {
        NvValue item = items->items[i];
        if (item.tag != NV_DICT) {
            j14_buf_free(&out); j14_buf_free(&central);
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة ملفات", "أرشيف.اضغط");
        }
        const NvValue *nv = j14r_dict_find(item.u.dict, "اسم");
        const NvValue *cv = j14r_dict_find(item.u.dict, "محتوى");
        if (!nv || nv->tag != NV_STR) {
            j14_buf_free(&out); j14_buf_free(&central);
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع ملفا فيه اسم نصا", "أرشيف.اضغط");
        }
        if (!cv || (cv->tag != NV_STR && cv->tag != NV_LIST)) {
            j14_buf_free(&out); j14_buf_free(&central);
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع ملفا فيه محتوى نصا أو بايتات", "أرشيف.اضغط");
        }
        const char *name = nv->u.str->data;
        size_t name_len = nv->u.str->len;
        size_t clen = 0;
        unsigned char *content = j14r_need_bytes("أرشيف.اضغط", *cv, &clen, line, col);
        if (name_len == 0 || name_len > 1000) {
            free(content); j14_buf_free(&out); j14_buf_free(&central);
            nv_error(NV_ERR_RUNTIME, "اسم ملف غير صالح في الأرشيف", line, col);
        }
        total += clen;
        if (total > 10 * 1024 * 1024) {
            free(content); j14_buf_free(&out); j14_buf_free(&central);
            nv_error(NV_ERR_RUNTIME, "أرشيف كبير جدا", line, col);
        }
        uint32_t crc = j14r_crc32(content, clen);
        uint32_t offset = (uint32_t)out.len;
        j14_buf_add(&out, "PK\x03\x04");
        j14r_zip_u16(&out, 20);
        j14r_zip_u16(&out, 0x0800);
        j14r_zip_u16(&out, 0);
        j14r_zip_u16(&out, 0);
        j14r_zip_u16(&out, 0x21);
        j14r_zip_u32(&out, crc);
        j14r_zip_u32(&out, (uint32_t)clen);
        j14r_zip_u32(&out, (uint32_t)clen);
        j14r_zip_u16(&out, (unsigned)name_len);
        j14r_zip_u16(&out, 0);
        j14_buf_addn(&out, name, name_len);
        j14_buf_addn(&out, (const char *)content, clen);
        j14_buf_add(&central, "PK\x01\x02");
        j14r_zip_u16(&central, 20);
        j14r_zip_u16(&central, 20);
        j14r_zip_u16(&central, 0x0800);
        j14r_zip_u16(&central, 0);
        j14r_zip_u16(&central, 0);
        j14r_zip_u16(&central, 0x21);
        j14r_zip_u32(&central, crc);
        j14r_zip_u32(&central, (uint32_t)clen);
        j14r_zip_u32(&central, (uint32_t)clen);
        j14r_zip_u16(&central, (unsigned)name_len);
        j14r_zip_u16(&central, 0);
        j14r_zip_u16(&central, 0);
        j14r_zip_u16(&central, 0);
        j14r_zip_u16(&central, 0);
        j14r_zip_u32(&central, 0);
        j14r_zip_u32(&central, offset);
        j14_buf_addn(&central, name, name_len);
        free(content);
    }
    uint32_t cd_offset = (uint32_t)out.len;
    uint32_t cd_size = (uint32_t)central.len;
    j14_buf_addn(&out, central.data, central.len);
    j14_buf_free(&central);
    j14_buf_add(&out, "PK\x05\x06");
    j14r_zip_u16(&out, 0);
    j14r_zip_u16(&out, 0);
    j14r_zip_u16(&out, (unsigned)items->len);
    j14r_zip_u16(&out, (unsigned)items->len);
    j14r_zip_u32(&out, cd_size);
    j14r_zip_u32(&out, cd_offset);
    j14r_zip_u16(&out, 0);
    NvValue res = j14r_list_of_bytes((const unsigned char *)out.data, out.len);
    j14_buf_free(&out);
    return res;
}

static NvValue nv_std_archive_decompress(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أرشيف.فك_ضغط", args, nargs, 1, line, col);
    size_t dlen = 0;
    unsigned char *data = j14r_need_bytes("أرشيف.فك_ضغط", args[0], &dlen, line, col);
    if (dlen > 10 * 1024 * 1024) {
        free(data);
        nv_error(NV_ERR_RUNTIME, "أرشيف كبير جدا", line, col);
    }
    J14rZipFile *files = NULL;
    size_t n = 0;
    j14r_zip_parse(data, dlen, &files, &n, "أرشيف.فك_ضغط", line, col);
    free(data);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < n; i++) {
        size_t didx = nv_root_push_idx();
        NvValue dict = nv_dict_new();
        *nv_root_slot(didx) = dict;
        nv_dict_set_raw(dict.u.dict, "اسم", nv_str_cstr(files[i].name));
        nv_dict_set_raw(dict.u.dict, "محتوى", j14r_lossy_str(files[i].content, files[i].clen));
        nv_root_pop();
        nv_list_push_raw(out.u.list, dict);
    }
    nv_root_pop();
    j14r_zip_free_files(files, n);
    return out;
}

static NvValue nv_std_archive_list(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("أرشيف.قائمة", args, nargs, 1, line, col);
    size_t dlen = 0;
    unsigned char *data = j14r_need_bytes("أرشيف.قائمة", args[0], &dlen, line, col);
    if (dlen > 10 * 1024 * 1024) {
        free(data);
        nv_error(NV_ERR_RUNTIME, "أرشيف كبير جدا", line, col);
    }
    J14rZipFile *files = NULL;
    size_t n = 0;
    j14r_zip_parse(data, dlen, &files, &n, "أرشيف.قائمة", line, col);
    free(data);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < n; i++) nv_list_push_raw(out.u.list, nv_str_cstr(files[i].name));
    nv_root_pop();
    j14r_zip_free_files(files, n);
    return out;
}

void nv_std_archive_fill(NvDict *d) {
    nv_std_add_fn(d, "اضغط", nv_std_archive_compress);
    nv_std_add_fn(d, "فك_ضغط", nv_std_archive_decompress);
    nv_std_add_fn(d, "قائمة", nv_std_archive_list);
}

// ---------- 211: كلمات المرور (وحدة كلمة_مرور) ----------

static NvValue nv_std_password_generate(NvValue *args, int nargs, int line, int col) {
    if (nargs != 1 && nargs != 2) {
        nv_error(NV_ERR_RUNTIME, "الدالة كلمة_مرور.ولّد تتوقع معاملا واحدا أو معاملين (طول ومجموعة)", line, col);
    }
    long long len = nv_std_need_integer("كلمة_مرور.ولّد", args[0], line, col);
    if (len < 1 || len > 128) {
        nv_error(NV_ERR_TYPE, "الدالة كلمة_مرور.ولّد تتوقع طولا بين 1 و 128", line, col);
    }
    const char *def = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*";
    const char *charset = def;
    if (nargs == 2) charset = nv_std_need_string("كلمة_مرور.ولّد", args[1], line, col);
    size_t nchars = 0;
    uint32_t *cps = NULL;
    size_t *offs = NULL;
    cps = j14_decode_all(charset, strlen(charset), &nchars, &offs);
    if (nchars == 0 || nchars > 256) {
        free(cps); free(offs);
        nv_error(NV_ERR_TYPE, "الدالة كلمة_مرور.ولّد تتوقع مجموعة غير فارغة", line, col);
    }
    size_t limit = 256 - (256 % nchars);
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0) {
        free(cps); free(offs);
        nv_error(NV_ERR_RUNTIME, "تعذّر قراءة العشوائية من النظام", line, col);
    }
    J14Buf out;
    j14_buf_init(&out);
    size_t made = 0;
    size_t fetched = 0;
    int failed = 0;
    while (made < (size_t)len) {
        if (fetched >= 4096) { failed = 1; break; }
        unsigned char buf[64];
        size_t got = 0;
        while (got < 64) {
            ssize_t r = read(fd, buf + got, 64 - got);
            if (r < 0) {
                if (errno == EINTR) continue;
                failed = 1;
                break;
            }
            if (r == 0) { failed = 1; break; }
            got += (size_t)r;
        }
        if (failed) break;
        fetched += 64;
        size_t i = 0;
        for (i = 0; i < 64 && made < (size_t)len; i++) {
            if ((size_t)buf[i] < limit) {
                uint32_t cp = cps[buf[i] % nchars];
                j14_buf_add_cp(&out, cp);
                made++;
            }
        }
    }
    close(fd);
    free(cps);
    free(offs);
    if (failed) {
        j14_buf_free(&out);
        nv_error(NV_ERR_RUNTIME, "تعذّر توليد كلمة مرور", line, col);
    }
    NvValue res = nv_str_new(out.data, out.len);
    j14_buf_free(&out);
    return res;
}

static NvValue nv_std_password_strength(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("كلمة_مرور.قوة", args, nargs, 1, line, col);
    const char *s = nv_std_need_string("كلمة_مرور.قوة", args[0], line, col);
    size_t nchars = 0;
    uint32_t *cps = NULL;
    size_t *offs = NULL;
    cps = j14_decode_all(s, strlen(s), &nchars, &offs);
    int lower = 0, upper = 0, digit = 0, symbol = 0;
    size_t i = 0;
    for (i = 0; i < nchars; i++) {
        uint32_t cp = cps[i];
        if ((cp >= 'a' && cp <= 'z') || (cp >= 0xDF && cp <= 0xFF && cp != 0xF7)) lower = 1;
        else if ((cp >= 'A' && cp <= 'Z') || (cp >= 0xC0 && cp <= 0xDE && cp != 0xD7)) upper = 1;
        else if (cp >= '0' && cp <= '9') digit = 1;
        else symbol = 1;
    }
    free(cps);
    free(offs);
    long score = (long)(nchars < 20 ? nchars : 20) * 2;
    if (lower) score += 15;
    if (upper) score += 15;
    if (digit) score += 15;
    if (symbol) score += 15;
    if (score > 100) score = 100;
    return nv_num((double)score);
}

void nv_std_password_fill(NvDict *d) {
    nv_std_add_fn(d, "ولد", nv_std_password_generate);
    nv_std_add_fn(d, "قوة", nv_std_password_strength);
}

// ---------- 214: بيانات وهمية (وحدة وهمي) ----------

static const char *J14R_FAKE_FIRST[20] = {
    "أحمد", "محمد", "سارة", "منى", "علي", "فاطمة", "حسن", "نور", "كريم", "ليلى", "عمر",
    "هدى", "يوسف", "مريم", "خالد", "آية", "إبراهيم", "سميرة", "طارق", "رشا",
};
static const char *J14R_FAKE_LAST[20] = {
    "علي", "حسن", "محمد", "إبراهيم", "أحمد", "سليمان", "عبد الله", "مصطفى", "حسين",
    "عبد الرحمن", "السيد", "فرج", "نصر", "عبد العزيز", "الشريف", "حمدي", "رمضان",
    "جابر", "عبد النبي", "توفيق",
};
static const char *J14R_FAKE_FIRST_LATIN[20] = {
    "ahmed", "mohamed", "sara", "mona", "ali", "fatima", "hassan", "nour", "karim",
    "layla", "omar", "huda", "youssef", "mariam", "khaled", "aya", "ibrahim", "samira",
    "tarek", "rasha",
};
static const char *J14R_FAKE_LAST_LATIN[20] = {
    "ali", "hassan", "mohamed", "ibrahim", "ahmed", "soliman", "abdullah", "mostafa",
    "hussein", "abdelrahman", "elsayed", "farag", "nasr", "abdelaziz", "elsherif",
    "hamdi", "ramadan", "gaber", "abdelnabi", "tawfik",
};
static const char *J14R_FAKE_CITIES[20] = {
    "القاهرة", "الجيزة", "الإسكندرية", "أسيوط", "المنصورة", "طنطا", "الزقازيق",
    "المنيا", "سوهاج", "قنا", "الأقصر", "أسوان", "بورسعيد", "السويس", "الإسماعيلية",
    "بني سويف", "الفيوم", "دمياط", "كفر الشيخ", "البحيرة",
};
static const char *J14R_FAKE_STREETS[20] = {
    "شارع التحرير", "شارع الجمهورية", "شارع النيل", "شارع الجامعة", "شارع بورسعيد",
    "شارع السودان", "شارع فيصل", "شارع الهرم", "شارع رمسيس", "شارع القصر العيني",
    "شارع الأزهر", "شارع محمد علي", "شارع طلعت حرب", "شارع قصر النيل", "شارع المنيل",
    "شارع عباس العقاد", "شارع مكرم عبيد", "شارع جسر السويس", "شارع أحمد عرابي",
    "شارع البحر الأعظم",
};

static uint64_t j14r_fake_seed(NvValue *args, int nargs, const char *display, uint64_t salt, int line, int col) {
    nv_std_need_args(display, args, nargs, 1, line, col);
    long long seed = nv_std_need_integer(display, args[0], line, col);
    return (uint64_t)seed ^ salt;
}

static NvValue nv_std_fake_name(NvValue *args, int nargs, int line, int col) {
    uint64_t st = j14r_fake_seed(args, nargs, "وهمي.اسم", 0, line, col);
    const char *first = J14R_FAKE_FIRST[j14r_splitmix_next(&st) % 20];
    const char *last = J14R_FAKE_LAST[j14r_splitmix_next(&st) % 20];
    char buf[256];
    snprintf(buf, sizeof(buf), "%s %s", first, last);
    return nv_str_cstr(buf);
}

static NvValue nv_std_fake_email(NvValue *args, int nargs, int line, int col) {
    uint64_t st = j14r_fake_seed(args, nargs, "وهمي.بريد", 0xA0761D6478BD642FULL, line, col);
    const char *first = J14R_FAKE_FIRST_LATIN[j14r_splitmix_next(&st) % 20];
    const char *last = J14R_FAKE_LAST_LATIN[j14r_splitmix_next(&st) % 20];
    unsigned long long num = (unsigned long long)(j14r_splitmix_next(&st) % 90 + 10);
    char buf[256];
    snprintf(buf, sizeof(buf), "%s.%s%llu@example.com", first, last, num);
    return nv_str_cstr(buf);
}

static NvValue nv_std_fake_phone(NvValue *args, int nargs, int line, int col) {
    uint64_t st = j14r_fake_seed(args, nargs, "وهمي.هاتف", 0xE7037ED1A0B428DBULL, line, col);
    static const char *prefixes[4] = {"010", "011", "012", "015"};
    const char *prefix = prefixes[j14r_splitmix_next(&st) % 4];
    char digits[9];
    int i = 0;
    for (i = 0; i < 8; i++) digits[i] = (char)('0' + (j14r_splitmix_next(&st) % 10));
    digits[8] = '\0';
    char buf[64];
    snprintf(buf, sizeof(buf), "+20 %s%s", prefix, digits);
    return nv_str_cstr(buf);
}

static NvValue nv_std_fake_address(NvValue *args, int nargs, int line, int col) {
    uint64_t st = j14r_fake_seed(args, nargs, "وهمي.عنوان", 0x8EBC6AF09C88C6E3ULL, line, col);
    const char *street = J14R_FAKE_STREETS[j14r_splitmix_next(&st) % 20];
    unsigned long long num = (unsigned long long)(j14r_splitmix_next(&st) % 200 + 1);
    const char *city = J14R_FAKE_CITIES[j14r_splitmix_next(&st) % 20];
    char buf[512];
    snprintf(buf, sizeof(buf), "%s %llu، %s", street, num, city);
    return nv_str_cstr(buf);
}

static NvValue nv_std_fake_city(NvValue *args, int nargs, int line, int col) {
    uint64_t st = j14r_fake_seed(args, nargs, "وهمي.مدينة", 0x589965CC75374CC3ULL, line, col);
    return nv_str_cstr(J14R_FAKE_CITIES[j14r_splitmix_next(&st) % 20]);
}

void nv_std_fake_fill(NvDict *d) {
    nv_std_add_fn(d, "اسم", nv_std_fake_name);
    nv_std_add_fn(d, "بريد", nv_std_fake_email);
    nv_std_add_fn(d, "هاتف", nv_std_fake_phone);
    nv_std_add_fn(d, "عنوان", nv_std_fake_address);
    nv_std_add_fn(d, "مدينة", nv_std_fake_city);
}

// ---------- 215: التقويم الهجري/الميلادي (وحدة تقويم) ----------

static int j14r_hijri_is_leap(long long y) {
    return (11 * y + 14) % 30 < 11;
}

static long long j14r_hijri_month_days(long long y, long long m) {
    if (m == 12 && j14r_hijri_is_leap(y)) return 30;
    if (m % 2 == 1) return 30;
    return 29;
}

static long long j14r_hijri_to_jd(long long y, long long m, long long d) {
    return (d - 1) + (m - 1) * 29 + (m - 1) / 2 + (y - 1) * 354 + (3 + 11 * y) / 30 + 1948440;
}

static void j14r_jd_to_hijri(long long jd, long long *y, long long *m, long long *d) {
    long long days = jd - 1948440;
    long long year = (30 * days + 10646) / 10631;
    long long start = (year - 1) * 354 + (3 + 11 * year) / 30;
    long long day = days - start + 1;
    long long month = 1;
    for (;;) {
        long long dim = j14r_hijri_month_days(year, month);
        if (day <= dim) break;
        day -= dim;
        month++;
    }
    *y = year;
    *m = month;
    *d = day;
}

static long long j14r_greg_to_jd(long long y, long m, long d) {
    return nv_std_time_days_from_civil(y, (unsigned long)m, (unsigned long)d) + 2440588;
}

static void j14r_jd_to_greg(long long jd, long long *y, long *m, long *d) {
    nv_std_time_civil_from_days(jd - 2440588, y, m, d);
}

static NvValue nv_std_calendar_hijri_to_greg(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تقويم.هجري_إلى_ميلادي", args, nargs, 3, line, col);
    long long y = nv_std_need_integer("تقويم.هجري_إلى_ميلادي", args[0], line, col);
    long long m = nv_std_need_integer("تقويم.هجري_إلى_ميلادي", args[1], line, col);
    long long d = nv_std_need_integer("تقويم.هجري_إلى_ميلادي", args[2], line, col);
    if (y < 1 || y > 3000 || m < 1 || m > 12) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.هجري_إلى_ميلادي تتوقع سنة هجرية بين 1 و 3000 وشهرا بين 1 و 12", line, col);
    }
    if (d < 1 || d > j14r_hijri_month_days(y, m)) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.هجري_إلى_ميلادي تتوقع يوما صالحا في هذا الشهر الهجري", line, col);
    }
    long long gy = 0;
    long gm = 0, gd = 0;
    j14r_jd_to_greg(j14r_hijri_to_jd(y, m, d), &gy, &gm, &gd);
    double nums[3] = {(double)gy, (double)gm, (double)gd};
    return j14r_list_of_nums(nums, 3);
}

static NvValue nv_std_calendar_greg_to_hijri(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تقويم.ميلادي_إلى_هجري", args, nargs, 3, line, col);
    long long y = nv_std_need_integer("تقويم.ميلادي_إلى_هجري", args[0], line, col);
    long long m = nv_std_need_integer("تقويم.ميلادي_إلى_هجري", args[1], line, col);
    long long d = nv_std_need_integer("تقويم.ميلادي_إلى_هجري", args[2], line, col);
    if (y < 1 || y > 9999 || m < 1 || m > 12 || d < 1 || d > 31) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.ميلادي_إلى_هجري تتوقع تاريخا ميلاديا صالحا", line, col);
    }
    long long ry = 0;
    long rm = 0, rd = 0;
    nv_std_time_civil_from_days(nv_std_time_days_from_civil(y, (unsigned long)m, (unsigned long)d), &ry, &rm, &rd);
    if (ry != y || rm != m || rd != d) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.ميلادي_إلى_هجري تتوقع يوما صالحا في هذا الشهر الميلادي", line, col);
    }
    long long hy = 0, hm = 0, hd = 0;
    j14r_jd_to_hijri(j14r_greg_to_jd(y, m, d), &hy, &hm, &hd);
    double nums[3] = {(double)hy, (double)hm, (double)hd};
    return j14r_list_of_nums(nums, 3);
}

static const char *J14R_HIJRI_MONTHS[12] = {
    "محرم", "صفر", "ربيع الأول", "ربيع الآخر", "جمادى الأولى", "جمادى الآخرة", "رجب",
    "شعبان", "رمضان", "شوال", "ذو القعدة", "ذو الحجة",
};

static NvValue nv_std_calendar_hijri_month_name(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تقويم.اسم_شهر_هجري", args, nargs, 1, line, col);
    long long m = nv_std_need_integer("تقويم.اسم_شهر_هجري", args[0], line, col);
    if (m < 1 || m > 12) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.اسم_شهر_هجري تتوقع شهرا بين 1 و 12", line, col);
    }
    return nv_str_cstr(J14R_HIJRI_MONTHS[m - 1]);
}

static NvValue nv_std_calendar_hijri_leap(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تقويم.كبيسة_هجرية", args, nargs, 1, line, col);
    long long y = nv_std_need_integer("تقويم.كبيسة_هجرية", args[0], line, col);
    if (y < 1) {
        nv_error(NV_ERR_TYPE, "الدالة تقويم.كبيسة_هجرية تتوقع سنة هجرية موجبة", line, col);
    }
    return nv_bool(j14r_hijri_is_leap(y));
}

void nv_std_calendar_fill(NvDict *d) {
    nv_std_add_fn(d, "هجري_إلى_ميلادي", nv_std_calendar_hijri_to_greg);
    nv_std_add_fn(d, "ميلادي_إلى_هجري", nv_std_calendar_greg_to_hijri);
    nv_std_add_fn(d, "اسم_شهر_هجري", nv_std_calendar_hijri_month_name);
    nv_std_add_fn(d, "كبيسة_هجرية", nv_std_calendar_hijri_leap);
}

// ---------- 216: الألوان (وحدة ألوان) ----------

static int j14r_colors_rgb_list(const char *display, NvValue v, double rgb[3], int line, int col) {
    NvList *l = j14r_need_list(display, v, line, col);
    if (l->len != 3) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع لونا [ر، غ، ب]", display);
    }
    int i = 0;
    for (i = 0; i < 3; i++) {
        double n = nv_std_need_number(display, l->items[i], line, col);
        if (n < 0 || n > 255) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع كل قناة لون بين 0 و 255", display);
        }
        rgb[i] = n;
    }
    return 1;
}

static int j14r_colors_rgb_ints(const char *display, NvValue *args, long long rgb[3], int line, int col) {
    int i = 0;
    for (i = 0; i < 3; i++) {
        long long n = nv_std_need_integer(display, args[i], line, col);
        if (n < 0 || n > 255) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع كل قناة لون بين 0 و 255", display);
        }
        rgb[i] = n;
    }
    return 1;
}

static NvValue nv_std_colors_to_hsv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ألوان.إلى_صبغة", args, nargs, 3, line, col);
    long long rgb[3];
    j14r_colors_rgb_ints("ألوان.إلى_صبغة", args, rgb, line, col);
    double r = rgb[0] / 255.0, g = rgb[1] / 255.0, b = rgb[2] / 255.0;
    double max = r > g ? (r > b ? r : b) : (g > b ? g : b);
    double min = r < g ? (r < b ? r : b) : (g < b ? g : b);
    double delta = max - min;
    double hue = 0;
    if (delta != 0) {
        if (max == r) hue = 60.0 * fmod((g - b) / delta, 6.0);
        else if (max == g) hue = 60.0 * (((b - r) / delta) + 2.0);
        else hue = 60.0 * (((r - g) / delta) + 4.0);
    }
    if (hue < 0) hue += 360.0;
    double sat = max == 0 ? 0 : delta / max;
    double nums[3] = {hue, sat, max};
    return j14r_list_of_nums(nums, 3);
}

static NvValue nv_std_colors_from_hsv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ألوان.من_صبغة", args, nargs, 3, line, col);
    double h = nv_std_need_number("ألوان.من_صبغة", args[0], line, col);
    double s = nv_std_need_number("ألوان.من_صبغة", args[1], line, col);
    double v = nv_std_need_number("ألوان.من_صبغة", args[2], line, col);
    if (!((h >= 0 && h < 360) || h == 360.0)) {
        nv_error(NV_ERR_TYPE, "الدالة ألوان.من_صبغة تتوقع صبغة بين 0 و 360", line, col);
    }
    if (s < 0 || s > 1 || v < 0 || v > 1) {
        nv_error(NV_ERR_TYPE, "الدالة ألوان.من_صبغة تتوقع تشبعا وقيمة بين 0 و 1", line, col);
    }
    if (h == 360.0) h = 0;
    double c = v * s;
    double x = c * (1.0 - fabs(fmod(h / 60.0, 2.0) - 1.0));
    double m = v - c;
    double r = 0, g = 0, b = 0;
    if (h < 60) { r = c; g = x; b = 0; }
    else if (h < 120) { r = x; g = c; b = 0; }
    else if (h < 180) { r = 0; g = c; b = x; }
    else if (h < 240) { r = 0; g = x; b = c; }
    else if (h < 300) { r = x; g = 0; b = c; }
    else { r = c; g = 0; b = x; }
    double nums[3] = {round((r + m) * 255.0), round((g + m) * 255.0), round((b + m) * 255.0)};
    return j14r_list_of_nums(nums, 3);
}

static NvValue nv_std_colors_to_hex(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ألوان.إلى_سداسي", args, nargs, 3, line, col);
    long long rgb[3];
    j14r_colors_rgb_ints("ألوان.إلى_سداسي", args, rgb, line, col);
    char buf[8];
    snprintf(buf, sizeof(buf), "#%02llx%02llx%02llx", rgb[0], rgb[1], rgb[2]);
    return nv_str_cstr(buf);
}

static NvValue nv_std_colors_from_hex(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ألوان.من_سداسي", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("ألوان.من_سداسي", args[0], line, col);
    const char *hex = text[0] == '#' ? text + 1 : text;
    if (strlen(hex) != 6) {
        nv_error(NV_ERR_RUNTIME, "لون سداسي غير صالح: توقع #rrggbb", line, col);
    }
    int i = 0;
    for (i = 0; i < 6; i++) {
        char ch = hex[i];
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f') || (ch >= 'A' && ch <= 'F'))) {
            nv_error(NV_ERR_RUNTIME, "لون سداسي غير صالح: توقع #rrggbb", line, col);
        }
    }
    long vals[3];
    for (i = 0; i < 3; i++) {
        char pair[3] = {hex[2 * i], hex[2 * i + 1], '\0'};
        vals[i] = strtol(pair, NULL, 16);
    }
    double nums[3] = {(double)vals[0], (double)vals[1], (double)vals[2]};
    return j14r_list_of_nums(nums, 3);
}

static NvValue nv_std_colors_mix(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("ألوان.مزج", args, nargs, 3, line, col);
    double c1[3], c2[3];
    j14r_colors_rgb_list("ألوان.مزج", args[0], c1, line, col);
    j14r_colors_rgb_list("ألوان.مزج", args[1], c2, line, col);
    double t = nv_std_need_number("ألوان.مزج", args[2], line, col);
    if (t < 0 || t > 1) {
        nv_error(NV_ERR_TYPE, "الدالة ألوان.مزج تتوقع نسبة بين 0 و 1", line, col);
    }
    double nums[3] = {
        round(c1[0] * (1.0 - t) + c2[0] * t),
        round(c1[1] * (1.0 - t) + c2[1] * t),
        round(c1[2] * (1.0 - t) + c2[2] * t),
    };
    return j14r_list_of_nums(nums, 3);
}

void nv_std_colors_fill(NvDict *d) {
    nv_std_add_fn(d, "إلى_صبغة", nv_std_colors_to_hsv);
    nv_std_add_fn(d, "من_صبغة", nv_std_colors_from_hsv);
    nv_std_add_fn(d, "إلى_سداسي", nv_std_colors_to_hex);
    nv_std_add_fn(d, "من_سداسي", nv_std_colors_from_hex);
    nv_std_add_fn(d, "مزج", nv_std_colors_mix);
}

// ---------- 217: جدول الإيموجي (وحدة إيموجي) ----------

static const char *J14R_EMOJI_NAMES[40] = {
    "وجه مبتسم", "وجه ضاحك", "وجه حزين", "وجه غاضب", "وجه نائم", "وجه مريض", "وجه عبقري",
    "قلب", "نار", "شمس", "قمر", "نجمة", "وردة", "زهرة", "شجرة", "قطة", "كلب", "طائر",
    "سمكة", "تفاحة", "قهوة", "كتاب", "سيارة", "بيت", "موسيقى", "كرة", "هدية", "تاج",
    "كاميرا", "هاتف", "حاسوب", "مصباح", "مفتاح", "ساعة", "تقويم", "علم", "طائرة",
    "سفينة", "جبل", "بحر",
};
static const char *J14R_EMOJI_SYMS[40] = {
    "😊", "😂", "😢", "😠", "😴", "🤒", "🤓",
    "❤", "🔥", "☀", "🌙", "⭐", "🌹", "🌸", "🌳", "🐱", "🐶", "🐦",
    "🐟", "🍎", "☕", "📖", "🚗", "🏠", "🎵", "⚽", "🎁", "👑",
    "📷", "📱", "💻", "💡", "🔑", "⏰", "📅", "🚩", "✈",
    "🚢", "⛰", "🌊",
};

static NvValue nv_std_emoji_symbol(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إيموجي.رمز", args, nargs, 1, line, col);
    const char *name = nv_std_need_string("إيموجي.رمز", args[0], line, col);
    int i = 0;
    for (i = 0; i < 40; i++) {
        if (strcmp(name, J14R_EMOJI_NAMES[i]) == 0) return nv_str_cstr(J14R_EMOJI_SYMS[i]);
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "اسم إيموجي غير موجود: %s", name);
    return nv_nil();
}

static NvValue nv_std_emoji_name(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إيموجي.اسم", args, nargs, 1, line, col);
    const char *sym = nv_std_need_string("إيموجي.اسم", args[0], line, col);
    int i = 0;
    for (i = 0; i < 40; i++) {
        if (strcmp(sym, J14R_EMOJI_SYMS[i]) == 0) return nv_str_cstr(J14R_EMOJI_NAMES[i]);
    }
    return nv_nil();
}

static NvValue nv_std_emoji_list(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إيموجي.قائمة", args, nargs, 0, line, col);
    return j14r_list_of_strs(J14R_EMOJI_NAMES, 40);
}

static NvValue nv_std_emoji_search(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إيموجي.ابحث", args, nargs, 1, line, col);
    const char *q = nv_std_need_string("إيموجي.ابحث", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    int i = 0;
    for (i = 0; i < 40; i++) {
        if (strstr(J14R_EMOJI_NAMES[i], q)) nv_list_push_raw(out.u.list, nv_str_cstr(J14R_EMOJI_NAMES[i]));
    }
    nv_root_pop();
    return out;
}

void nv_std_emoji_fill(NvDict *d) {
    nv_std_add_fn(d, "رمز", nv_std_emoji_symbol);
    nv_std_add_fn(d, "اسم", nv_std_emoji_name);
    nv_std_add_fn(d, "قائمة", nv_std_emoji_list);
    nv_std_add_fn(d, "ابحث", nv_std_emoji_search);
}

// ---------- J10 الأساسي (الدفعة الرابعة من برنامج الـ579، الاجتماع 73 §88):
// البنود 157 (في رياضيات أعلاه) و159–165 — مرآة C لكل ما
// في stdlib.rs، بالصيغ نفسها حرفيا ليتطابق الناتج بتّا بتّا.

// ---------- 159: الطي (وحدة قوائم جديدة) ----------
// اطوِ_بـ(قائمة، ابتدائي، دالة/عملية) — كما في stdlib.rs:
// دالة التجميع تستلم قائمة [المتراكم، العنصر] بمعامل
// واحد (nv_call بمعامل واحد)؛ أو نص عملية مدمجة.
// المفتاح المخزن «اطو_ب» (تجريد الكسرة والتطويل).

static NvValue nv_std_lists_fold(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.اطو_ب", args, nargs, 3, line, col);
    NvList *items = j14r_need_list("قوائم.اطو_ب", args[0], line, col);
    NvValue acc = args[1];
    if (args[2].tag == NV_STR) {
        const char *op = args[2].u.str->data;
        int known = strcmp(op, "جمع") == 0 || strcmp(op, "ضرب") == 0 ||
                    strcmp(op, "أكبر") == 0 || strcmp(op, "أصغر") == 0;
        if (!known) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "عملية طي غير معروفة: %s — المتاح: جمع، ضرب، أكبر، أصغر", op);
        }
        size_t i = 0;
        for (i = 0; i < items->len; i++) {
            double x = nv_std_need_number("قوائم.اطو_ب", acc, line, col);
            double y = nv_std_need_number("قوائم.اطو_ب", items->items[i], line, col);
            double r = 0.0;
            if (strcmp(op, "جمع") == 0) {
                r = x + y;
            } else if (strcmp(op, "ضرب") == 0) {
                r = x * y;
            } else if (strcmp(op, "أكبر") == 0) {
                r = fmax(x, y);
            } else {
                r = fmin(x, y);
            }
            acc = nv_num(r);
        }
        return acc;
    }
    NvValue func = args[2];
    size_t acc_idx = nv_root_push_idx();
    size_t pair_idx = nv_root_push_idx();
    *nv_root_slot(acc_idx) = acc;
    size_t i = 0;
    for (i = 0; i < items->len; i++) {
        NvValue pair = nv_list_new();
        *nv_root_slot(pair_idx) = pair;
        nv_list_push_raw(pair.u.list, nv_clone_value(acc));
        nv_list_push_raw(pair.u.list, nv_clone_value(items->items[i]));
        NvValue argv[1];
        argv[0] = pair;
        acc = nv_call(func, argv, 1, line, col);
        *nv_root_slot(acc_idx) = acc;
    }
    nv_root_pop_n(2);
    return acc;
}

void nv_std_lists_fill(NvDict *d) {
    nv_std_add_fn(d, "اطو_ب", nv_std_lists_fold);
    // J3 — إكمال وحدة قوائم: 56 و57 و58 و59 و60 و73.
    nv_std_add_fn(d, "جدول_ب", nv_std_lists_table);
    nv_std_add_fn(d, "جمع_حسب", nv_std_lists_group_by);
    nv_std_add_fn(d, "بادئات", nv_std_lists_prefixes);
    nv_std_add_fn(d, "لاحقات", nv_std_lists_suffixes);
    nv_std_add_fn(d, "مجاميع_تراكمية", nv_std_lists_cumulative);
    nv_std_add_fn(d, "نوافذ", nv_std_lists_windows);
    nv_std_add_fn(d, "خارج_النوافذ", nv_std_lists_windows_outside);
    nv_std_add_fn(d, "رتب", nv_std_lists_sort);
    nv_std_add_fn(d, "رتب_تنازليا", nv_std_lists_sort_desc);
    nv_std_add_fn(d, "قطع", nv_std_lists_chunk);
    nv_std_add_fn(d, "قطع_بفاصل", nv_std_lists_split_sep);
}

// ---------- 160: الآلة الحاسبة العلمية (وحدة علمي جديدة) ----------
// 11 ثابتا بقيم SI (CODATA 2018) + خطاف وحدة_الثابت —
// كما في stdlib.rs تماما.

static NvValue nv_std_science_unit(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("علمي.وحدة_الثابت", args, nargs, 1, line, col);
    const char *name = nv_std_need_string("علمي.وحدة_الثابت", args[0], line, col);
    const char *unit = NULL;
    if (strcmp(name, "سرعة_الضوء") == 0) {
        unit = "متر/ثانية";
    } else if (strcmp(name, "ثابت_الجاذبية") == 0) {
        unit = "متر مكعب/كيلوغرام/ثانية مربعة";
    } else if (strcmp(name, "ثابت_بلانك") == 0) {
        unit = "جول ثانية";
    } else if (strcmp(name, "شحنة_العنصر") == 0) {
        unit = "كولوم";
    } else if (strcmp(name, "ثابت_أفوجادرو") == 0) {
        unit = "1/مول";
    } else if (strcmp(name, "ثابت_بولتزمان") == 0) {
        unit = "جول/كلفن";
    } else if (strcmp(name, "كتلة_الإلكترون") == 0) {
        unit = "كيلوغرام";
    } else if (strcmp(name, "كتلة_البروتون") == 0) {
        unit = "كيلوغرام";
    } else if (strcmp(name, "ثابت_الغاز") == 0) {
        unit = "جول/مول/كلفن";
    } else if (strcmp(name, "تسارع_الجاذبية") == 0) {
        unit = "متر/ثانية مربعة";
    } else if (strcmp(name, "ثابت_ستيفان_بولتزمان") == 0) {
        unit = "واط/متر مربع/كلفن أس 4";
    } else {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "ثابت علمي غير موجود: %s", name);
    }
    return nv_str_cstr(unit);
}

void nv_std_science_fill(NvDict *d) {
    nv_std_add_num(d, "سرعة_الضوء", 299792458.0);
    nv_std_add_num(d, "ثابت_الجاذبية", 6.67430e-11);
    nv_std_add_num(d, "ثابت_بلانك", 6.62607015e-34);
    nv_std_add_num(d, "شحنة_العنصر", 1.602176634e-19);
    nv_std_add_num(d, "ثابت_أفوجادرو", 6.02214076e23);
    nv_std_add_num(d, "ثابت_بولتزمان", 1.380649e-23);
    nv_std_add_num(d, "كتلة_الإلكترون", 9.1093837015e-31);
    nv_std_add_num(d, "كتلة_البروتون", 1.67262192369e-27);
    nv_std_add_num(d, "ثابت_الغاز", 8.314462618);
    nv_std_add_num(d, "تسارع_الجاذبية", 9.80665);
    nv_std_add_num(d, "ثابت_ستيفان_بولتزمان", 5.670374419e-8);
    nv_std_add_fn(d, "وحدة_الثابت", nv_std_science_unit);
}

// ---------- 162: الفائدة المركبة (وحدة تمويل جديدة) ----------

static void nv_std_finance_check_rate(double rate, long long times, int line, int col) {
    if (times < 1) {
        nv_error(NV_ERR_RUNTIME, "مرات التركيب في السنة يجب أن تكون عددا صحيحا ≥ 1", line, col);
    }
    if (1.0 + rate / (double)times <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "المعدل يجعل أساس الفائدة غير موجب", line, col);
    }
}

static NvValue nv_std_finance_compound(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تمويل.فائدة_مركبة", args, nargs, 4, line, col);
    double principal = nv_std_need_number("تمويل.فائدة_مركبة", args[0], line, col);
    double rate = nv_std_need_number("تمويل.فائدة_مركبة", args[1], line, col);
    double years = nv_std_need_number("تمويل.فائدة_مركبة", args[2], line, col);
    long long times = nv_std_need_integer("تمويل.فائدة_مركبة", args[3], line, col);
    if (years < 0.0) {
        nv_error(NV_ERR_RUNTIME, "السنوات يجب أن تكون ≥ 0", line, col);
    }
    nv_std_finance_check_rate(rate, times, line, col);
    double n = (double)times;
    return nv_num(principal * pow(1.0 + rate / n, n * years));
}

static NvValue nv_std_finance_continuous(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تمويل.فائدة_مركبة_مستمرة", args, nargs, 3, line, col);
    double principal = nv_std_need_number("تمويل.فائدة_مركبة_مستمرة", args[0], line, col);
    double rate = nv_std_need_number("تمويل.فائدة_مركبة_مستمرة", args[1], line, col);
    double years = nv_std_need_number("تمويل.فائدة_مركبة_مستمرة", args[2], line, col);
    if (years < 0.0) {
        nv_error(NV_ERR_RUNTIME, "السنوات يجب أن تكون ≥ 0", line, col);
    }
    return nv_num(principal * exp(rate * years));
}

static NvValue nv_std_finance_present(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تمويل.قيمة_حالية", args, nargs, 4, line, col);
    double future = nv_std_need_number("تمويل.قيمة_حالية", args[0], line, col);
    double rate = nv_std_need_number("تمويل.قيمة_حالية", args[1], line, col);
    double years = nv_std_need_number("تمويل.قيمة_حالية", args[2], line, col);
    long long times = nv_std_need_integer("تمويل.قيمة_حالية", args[3], line, col);
    if (years < 0.0) {
        nv_error(NV_ERR_RUNTIME, "السنوات يجب أن تكون ≥ 0", line, col);
    }
    nv_std_finance_check_rate(rate, times, line, col);
    double n = (double)times;
    return nv_num(future / pow(1.0 + rate / n, n * years));
}

static NvValue nv_std_finance_effective(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تمويل.معدل_فعلي_سنوي", args, nargs, 2, line, col);
    double rate = nv_std_need_number("تمويل.معدل_فعلي_سنوي", args[0], line, col);
    long long times = nv_std_need_integer("تمويل.معدل_فعلي_سنوي", args[1], line, col);
    nv_std_finance_check_rate(rate, times, line, col);
    double n = (double)times;
    return nv_num(pow(1.0 + rate / n, n) - 1.0);
}

static NvValue nv_std_finance_payment(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("تمويل.قسط_شهري", args, nargs, 3, line, col);
    double principal = nv_std_need_number("تمويل.قسط_شهري", args[0], line, col);
    double rate = nv_std_need_number("تمويل.قسط_شهري", args[1], line, col);
    long long months = nv_std_need_integer("تمويل.قسط_شهري", args[2], line, col);
    if (months < 1) {
        nv_error(NV_ERR_RUNTIME, "الشهور يجب أن تكون عددا صحيحا ≥ 1", line, col);
    }
    double i = rate / 12.0;
    if (i == 0.0) {
        return nv_num(principal / (double)months);
    }
    if (1.0 + i <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "المعدل يجعل أساس القسط غير موجب", line, col);
    }
    return nv_num(principal * i / (1.0 - pow(1.0 + i, -(double)months)));
}

void nv_std_finance_fill(NvDict *d) {
    nv_std_add_fn(d, "فائدة_مركبة", nv_std_finance_compound);
    nv_std_add_fn(d, "فائدة_مركبة_مستمرة", nv_std_finance_continuous);
    nv_std_add_fn(d, "قيمة_حالية", nv_std_finance_present);
    nv_std_add_fn(d, "معدل_فعلي_سنوي", nv_std_finance_effective);
    nv_std_add_fn(d, "قسط_شهري", nv_std_finance_payment);
}

// ---------- 163: أساسيات اكتوارية (وحدة اكتواري جديدة) ----------
// القيمة الزمنية للنقود للدفعات الدورية المتساوية فقط.

static void nv_std_actuarial_check(double rate, long long periods, int line, int col) {
    if (periods < 1) {
        nv_error(NV_ERR_RUNTIME, "الفترات يجب أن تكون عددا صحيحا ≥ 1", line, col);
    }
    if (1.0 + rate <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "المعدل يجب أن يكون > -1", line, col);
    }
}

static NvValue nv_std_actuarial_pv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكتواري.قيمة_حالية_للدفعات", args, nargs, 3, line, col);
    double pmt = nv_std_need_number("اكتواري.قيمة_حالية_للدفعات", args[0], line, col);
    double rate = nv_std_need_number("اكتواري.قيمة_حالية_للدفعات", args[1], line, col);
    long long periods = nv_std_need_integer("اكتواري.قيمة_حالية_للدفعات", args[2], line, col);
    nv_std_actuarial_check(rate, periods, line, col);
    double n = (double)periods;
    if (rate == 0.0) {
        return nv_num(pmt * n);
    }
    return nv_num(pmt * (1.0 - pow(1.0 + rate, -n)) / rate);
}

static NvValue nv_std_actuarial_fv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكتواري.قيمة_مستقبلية_للدفعات", args, nargs, 3, line, col);
    double pmt = nv_std_need_number("اكتواري.قيمة_مستقبلية_للدفعات", args[0], line, col);
    double rate = nv_std_need_number("اكتواري.قيمة_مستقبلية_للدفعات", args[1], line, col);
    long long periods = nv_std_need_integer("اكتواري.قيمة_مستقبلية_للدفعات", args[2], line, col);
    nv_std_actuarial_check(rate, periods, line, col);
    double n = (double)periods;
    if (rate == 0.0) {
        return nv_num(pmt * n);
    }
    return nv_num(pmt * (pow(1.0 + rate, n) - 1.0) / rate);
}

static NvValue nv_std_actuarial_pmt_pv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكتواري.قسط_من_قيمة_حالية", args, nargs, 3, line, col);
    double pv = nv_std_need_number("اكتواري.قسط_من_قيمة_حالية", args[0], line, col);
    double rate = nv_std_need_number("اكتواري.قسط_من_قيمة_حالية", args[1], line, col);
    long long periods = nv_std_need_integer("اكتواري.قسط_من_قيمة_حالية", args[2], line, col);
    nv_std_actuarial_check(rate, periods, line, col);
    double n = (double)periods;
    if (rate == 0.0) {
        return nv_num(pv / n);
    }
    return nv_num(pv * rate / (1.0 - pow(1.0 + rate, -n)));
}

static NvValue nv_std_actuarial_pmt_fv(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("اكتواري.قسط_من_قيمة_مستقبلية", args, nargs, 3, line, col);
    double fv = nv_std_need_number("اكتواري.قسط_من_قيمة_مستقبلية", args[0], line, col);
    double rate = nv_std_need_number("اكتواري.قسط_من_قيمة_مستقبلية", args[1], line, col);
    long long periods = nv_std_need_integer("اكتواري.قسط_من_قيمة_مستقبلية", args[2], line, col);
    nv_std_actuarial_check(rate, periods, line, col);
    double n = (double)periods;
    if (rate == 0.0) {
        return nv_num(fv / n);
    }
    return nv_num(fv * rate / (pow(1.0 + rate, n) - 1.0));
}

void nv_std_actuarial_fill(NvDict *d) {
    nv_std_add_fn(d, "قيمة_حالية_للدفعات", nv_std_actuarial_pv);
    nv_std_add_fn(d, "قيمة_مستقبلية_للدفعات", nv_std_actuarial_fv);
    nv_std_add_fn(d, "قسط_من_قيمة_حالية", nv_std_actuarial_pmt_pv);
    nv_std_add_fn(d, "قسط_من_قيمة_مستقبلية", nv_std_actuarial_pmt_fv);
}

// ---------- 164: الإحصاء الأساسي (وحدة إحصاء جديدة) ----------

static int nv_std_stats_cmp_double(const void *a, const void *b) {
    double x = *(const double *)a;
    double y = *(const double *)b;
    if (x < y) return -1;
    if (x > y) return 1;
    return 0;
}

// قائمة أعداد غير فارغة ومنتهية؛ يعيد مصفوفة malloc يجب تحريرها.
static double *nv_std_stats_numbers(const char *display, NvValue v, size_t *len_out, int line, int col) {
    NvList *items = j14r_need_list(display, v, line, col);
    if (items->len == 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع قائمة غير فارغة", display);
    }
    double *out = (double *)malloc(items->len * sizeof(double));
    size_t i = 0;
    for (i = 0; i < items->len; i++) {
        double x = nv_std_need_number(display, items->items[i], line, col);
        if (!isfinite(x)) {
            free(out);
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة %s تتوقع أعدادا منتهية", display);
        }
        out[i] = x;
    }
    *len_out = items->len;
    return out;
}

static double nv_std_stats_mean_of(const double *xs, size_t n) {
    double sum = 0.0;
    size_t i = 0;
    for (i = 0; i < n; i++) {
        sum += xs[i];
    }
    return sum / (double)n;
}

static double nv_std_stats_var_of(const double *xs, size_t n, double mean, int sample) {
    double acc = 0.0;
    size_t i = 0;
    for (i = 0; i < n; i++) {
        double d = xs[i] - mean;
        acc += d * d;
    }
    if (sample) {
        return acc / ((double)n - 1.0);
    }
    return acc / (double)n;
}

static NvValue nv_std_stats_mean(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.متوسط", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.متوسط", args[0], &n, line, col);
    double r = nv_std_stats_mean_of(xs, n);
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_median(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.وسيط", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.وسيط", args[0], &n, line, col);
    qsort(xs, n, sizeof(double), nv_std_stats_cmp_double);
    double r = 0.0;
    if (n % 2 == 1) {
        r = xs[n / 2];
    } else {
        r = (xs[n / 2 - 1] + xs[n / 2]) / 2.0;
    }
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_mode(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.منوال", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.منوال", args[0], &n, line, col);
    qsort(xs, n, sizeof(double), nv_std_stats_cmp_double);
    double best = xs[0];
    size_t best_count = 1;
    double cur = xs[0];
    size_t cur_count = 1;
    size_t i = 0;
    for (i = 1; i < n; i++) {
        if (xs[i] == cur) {
            cur_count++;
        } else {
            cur = xs[i];
            cur_count = 1;
        }
        if (cur_count > best_count) {
            best = cur;
            best_count = cur_count;
        }
    }
    free(xs);
    return nv_num(best);
}

static NvValue nv_std_stats_stddev(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.انحراف_معياري", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.انحراف_معياري", args[0], &n, line, col);
    double mean = nv_std_stats_mean_of(xs, n);
    double r = sqrt(nv_std_stats_var_of(xs, n, mean, 0));
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_sample_stddev(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.انحراف_معياري_للعينة", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.انحراف_معياري_للعينة", args[0], &n, line, col);
    if (n < 2) {
        free(xs);
        nv_error(NV_ERR_RUNTIME, "انحراف العينة المعياري يحتاج عنصرين على الأقل", line, col);
    }
    double mean = nv_std_stats_mean_of(xs, n);
    double r = sqrt(nv_std_stats_var_of(xs, n, mean, 1));
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_variance(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.تباين", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.تباين", args[0], &n, line, col);
    double mean = nv_std_stats_mean_of(xs, n);
    double r = nv_std_stats_var_of(xs, n, mean, 0);
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_sample_variance(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.تباين_العينة", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.تباين_العينة", args[0], &n, line, col);
    if (n < 2) {
        free(xs);
        nv_error(NV_ERR_RUNTIME, "تباين العينة يحتاج عنصرين على الأقل", line, col);
    }
    double mean = nv_std_stats_mean_of(xs, n);
    double r = nv_std_stats_var_of(xs, n, mean, 1);
    free(xs);
    return nv_num(r);
}

static NvValue nv_std_stats_sum(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.مجموع", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.مجموع", args[0], &n, line, col);
    double sum = 0.0;
    size_t i = 0;
    for (i = 0; i < n; i++) {
        sum += xs[i];
    }
    free(xs);
    return nv_num(sum);
}

static NvValue nv_std_stats_range(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("إحصاء.مدى", args, nargs, 1, line, col);
    size_t n = 0;
    double *xs = nv_std_stats_numbers("إحصاء.مدى", args[0], &n, line, col);
    double lo = xs[0];
    double hi = xs[0];
    size_t i = 0;
    for (i = 1; i < n; i++) {
        if (xs[i] < lo) {
            lo = xs[i];
        }
        if (xs[i] > hi) {
            hi = xs[i];
        }
    }
    free(xs);
    return nv_num(hi - lo);
}

void nv_std_stats_fill(NvDict *d) {
    nv_std_add_fn(d, "متوسط", nv_std_stats_mean);
    nv_std_add_fn(d, "وسيط", nv_std_stats_median);
    nv_std_add_fn(d, "منوال", nv_std_stats_mode);
    nv_std_add_fn(d, "انحراف_معياري", nv_std_stats_stddev);
    nv_std_add_fn(d, "انحراف_معياري_للعينة", nv_std_stats_sample_stddev);
    nv_std_add_fn(d, "تباين", nv_std_stats_variance);
    nv_std_add_fn(d, "تباين_العينة", nv_std_stats_sample_variance);
    nv_std_add_fn(d, "مجموع", nv_std_stats_sum);
    nv_std_add_fn(d, "مدى", nv_std_stats_range);
}

// ---------- 165: التوزيعات الإحصائية (وحدة توزيعات جديدة) ----------
// دالة الخطأ بتقريب أبراموفيتز-ستيغن 7.1.26 — بالصيغة
// نفسها حرفيا كما في stdlib.rs؛ والكمّي بتنصيف 100 خطوة.

static double nv_std_distrib_erf(double x) {
    if (x == 0.0) {
        return 0.0;
    }
    double sign = x < 0.0 ? -1.0 : 1.0;
    double ax = fabs(x);
    double t = 1.0 / (1.0 + 0.3275911 * ax);
    double poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t;
    double y = 1.0 - poly * exp(-ax * ax);
    return sign * y;
}

static double nv_std_distrib_norm_cdf_value(double x, double mu, double sigma) {
    return 0.5 * (1.0 + nv_std_distrib_erf((x - mu) / (sigma * sqrt(2.0))));
}

static NvValue nv_std_distrib_norm_pdf(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.طبيعي_كثافة", args, nargs, 3, line, col);
    double x = nv_std_need_number("توزيعات.طبيعي_كثافة", args[0], line, col);
    double mu = nv_std_need_number("توزيعات.طبيعي_كثافة", args[1], line, col);
    double sigma = nv_std_need_number("توزيعات.طبيعي_كثافة", args[2], line, col);
    if (sigma <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col);
    }
    double z = (x - mu) / sigma;
    double pdf = exp(-0.5 * z * z) / (sigma * sqrt(2.0 * 3.14159265358979323846));
    return nv_num(pdf);
}

static NvValue nv_std_distrib_norm_cdf(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.طبيعي_تراكمي", args, nargs, 3, line, col);
    double x = nv_std_need_number("توزيعات.طبيعي_تراكمي", args[0], line, col);
    double mu = nv_std_need_number("توزيعات.طبيعي_تراكمي", args[1], line, col);
    double sigma = nv_std_need_number("توزيعات.طبيعي_تراكمي", args[2], line, col);
    if (sigma <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col);
    }
    return nv_num(nv_std_distrib_norm_cdf_value(x, mu, sigma));
}

static NvValue nv_std_distrib_norm_quantile(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.طبيعي_كمي", args, nargs, 3, line, col);
    double p = nv_std_need_number("توزيعات.طبيعي_كمي", args[0], line, col);
    double mu = nv_std_need_number("توزيعات.طبيعي_كمي", args[1], line, col);
    double sigma = nv_std_need_number("توزيعات.طبيعي_كمي", args[2], line, col);
    if (sigma <= 0.0) {
        nv_error(NV_ERR_RUNTIME, "الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col);
    }
    if (p <= 0.0 || p >= 1.0) {
        nv_error(NV_ERR_RUNTIME, "الاحتمال للكمّي الطبيعي يجب أن يكون بين 0 و1 حصرا", line, col);
    }
    double lo = mu - 10.0 * sigma;
    double hi = mu + 10.0 * sigma;
    int k = 0;
    for (k = 0; k < 100; k++) {
        double mid = (lo + hi) / 2.0;
        if (nv_std_distrib_norm_cdf_value(mid, mu, sigma) < p) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    return nv_num((lo + hi) / 2.0);
}

static NvValue nv_std_distrib_unif_pdf(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.منتظم_كثافة", args, nargs, 3, line, col);
    double x = nv_std_need_number("توزيعات.منتظم_كثافة", args[0], line, col);
    double lo = nv_std_need_number("توزيعات.منتظم_كثافة", args[1], line, col);
    double hi = nv_std_need_number("توزيعات.منتظم_كثافة", args[2], line, col);
    if (hi <= lo) {
        nv_error(NV_ERR_RUNTIME, "الحد الأقصى للتوزيع المنتظم يجب أن يكون أكبر من الأدنى", line, col);
    }
    if (x < lo || x > hi) {
        return nv_num(0.0);
    }
    return nv_num(1.0 / (hi - lo));
}

static NvValue nv_std_distrib_unif_cdf(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.منتظم_تراكمي", args, nargs, 3, line, col);
    double x = nv_std_need_number("توزيعات.منتظم_تراكمي", args[0], line, col);
    double lo = nv_std_need_number("توزيعات.منتظم_تراكمي", args[1], line, col);
    double hi = nv_std_need_number("توزيعات.منتظم_تراكمي", args[2], line, col);
    if (hi <= lo) {
        nv_error(NV_ERR_RUNTIME, "الحد الأقصى للتوزيع المنتظم يجب أن يكون أكبر من الأدنى", line, col);
    }
    if (x <= lo) {
        return nv_num(0.0);
    }
    if (x >= hi) {
        return nv_num(1.0);
    }
    return nv_num((x - lo) / (hi - lo));
}

static NvValue nv_std_distrib_binom(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.ثنائي_احتمال", args, nargs, 3, line, col);
    long long k = nv_std_need_integer("توزيعات.ثنائي_احتمال", args[0], line, col);
    long long n = nv_std_need_integer("توزيعات.ثنائي_احتمال", args[1], line, col);
    double p = nv_std_need_number("توزيعات.ثنائي_احتمال", args[2], line, col);
    if (n < 0 || k < 0 || k > n) {
        nv_error(NV_ERR_RUNTIME, "النجاحات والمحاولات للتوزيع الثنائي يجب أن تحقق 0 ≤ نجاحات ≤ محاولات", line, col);
    }
    if (p < 0.0 || p > 1.0) {
        nv_error(NV_ERR_RUNTIME, "الاحتمال للتوزيع الثنائي يجب أن يكون بين 0 و1", line, col);
    }
    long long kk = k < n - k ? k : n - k;
    double choose = 1.0;
    long long i = 0;
    for (i = 1; i <= kk; i++) {
        choose = choose * (double)(n - kk + i) / (double)i;
    }
    return nv_num(choose * pow(p, (double)k) * pow(1.0 - p, (double)(n - k)));
}

static NvValue nv_std_distrib_poisson(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("توزيعات.بواسون_احتمال", args, nargs, 2, line, col);
    long long k = nv_std_need_integer("توزيعات.بواسون_احتمال", args[0], line, col);
    double lambda = nv_std_need_number("توزيعات.بواسون_احتمال", args[1], line, col);
    if (k < 0) {
        nv_error(NV_ERR_RUNTIME, "الأحداث لتوزيع بواسون يجب أن تكون عددا صحيحا ≥ 0", line, col);
    }
    if (lambda < 0.0) {
        nv_error(NV_ERR_RUNTIME, "المتوسط لتوزيع بواسون يجب أن يكون ≥ 0", line, col);
    }
    double fact = 1.0;
    long long i = 0;
    for (i = 2; i <= k; i++) {
        fact *= (double)i;
    }
    return nv_num(exp(-lambda) * pow(lambda, (double)k) / fact);
}

void nv_std_distrib_fill(NvDict *d) {
    nv_std_add_fn(d, "طبيعي_كثافة", nv_std_distrib_norm_pdf);
    nv_std_add_fn(d, "طبيعي_تراكمي", nv_std_distrib_norm_cdf);
    nv_std_add_fn(d, "طبيعي_كمي", nv_std_distrib_norm_quantile);
    nv_std_add_fn(d, "منتظم_كثافة", nv_std_distrib_unif_pdf);
    nv_std_add_fn(d, "منتظم_تراكمي", nv_std_distrib_unif_cdf);
    nv_std_add_fn(d, "ثنائي_احتمال", nv_std_distrib_binom);
    nv_std_add_fn(d, "بواسون_احتمال", nv_std_distrib_poisson);
}

// ---------- J3 — الظروف والروابط (الدفعة الخامسة والأخيرة من
// الدفعة الأولى من برنامج الـ579، الاجتماع 73 §88) ----------
// مرآة C لـ stdlib.rs حرفيا: الرسائل العربية نفسها،
// والمفاتيح المطبّعة نفسها، وصيغة التطبيق الفوري نفسها
// (دالة بمعامل واحد؛ والثنائية بزوج [أ، ب]).

// ---------- مساعدات J3 ----------
static NvValue j3_call_single(NvValue func, NvValue arg, int line, int col) {
    size_t ridx = nv_root_push_idx();
    NvValue argv[1];
    argv[0] = arg;
    NvValue res = nv_call(func, argv, 1, line, col);
    *nv_root_slot(ridx) = res;
    nv_root_pop();
    return res;
}

static NvValue j3_call_pair(NvValue func, NvValue a, NvValue b, int line, int col) {
    size_t pidx = nv_root_push_idx();
    NvValue pair = nv_list_new();
    *nv_root_slot(pidx) = pair;
    nv_list_push_raw(pair.u.list, nv_clone_value(a));
    nv_list_push_raw(pair.u.list, nv_clone_value(b));
    NvValue argv[1];
    argv[0] = pair;
    NvValue res = nv_call(func, argv, 1, line, col);
    *nv_root_slot(pidx) = res;
    nv_root_pop();
    return res;
}

static int j3_call_bool(NvValue func, NvValue arg, const char *display, int line, int col) {
    NvValue v = j3_call_single(func, arg, line, col);
    if (v.tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع أن يعيد الشرط منطقيا لكن أعطيت %s", display, nv_type_name(v));
    }
    return v.u.boolean;
}

static double j3_op(const char *op, double x, double y, const char *kind, int line, int col) {
    if (strcmp(op, "جمع") == 0) return x + y;
    if (strcmp(op, "ضرب") == 0) return x * y;
    if (strcmp(op, "أكبر") == 0) return fmax(x, y);
    if (strcmp(op, "أصغر") == 0) return fmin(x, y);
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "عملية %s غير معروفة: %s — المتاح: جمع، ضرب، أكبر، أصغر", kind, op);
    return 0.0;
}

static int j3_is_func(NvValue v) {
    return v.tag == NV_FUNC || v.tag == NV_NATIVE || v.tag == NV_BOUND;
}

static void j3_need_func(const char *display, NvValue v, int line, int col) {
    if (!j3_is_func(v)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع دالة لكن أعطيت %s", display, nv_type_name(v));
    }
}

// ---------- 56: الجدول ----------
static NvValue nv_std_lists_table(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.جدول_ب", args, nargs, 3, line, col);
    NvList *a = j14r_need_list("قوائم.جدول_ب", args[0], line, col);
    NvList *b = j14r_need_list("قوائم.جدول_ب", args[1], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    if (args[2].tag == NV_STR) {
        const char *op = args[2].u.str->data;
        size_t i = 0, j = 0;
        for (i = 0; i < a->len; i++) {
            size_t widx = nv_root_push_idx();
            NvValue row = nv_list_new();
            *nv_root_slot(widx) = row;
            for (j = 0; j < b->len; j++) {
                double x = nv_std_need_number("قوائم.جدول_ب", a->items[i], line, col);
                double y = nv_std_need_number("قوائم.جدول_ب", b->items[j], line, col);
                nv_list_push_raw(row.u.list, nv_num(j3_op(op, x, y, "جدول", line, col)));
            }
            nv_list_push_raw(out.u.list, row);
            nv_root_pop();
        }
    } else {
        NvValue func = args[2];
        size_t i = 0, j = 0;
        for (i = 0; i < a->len; i++) {
            size_t widx = nv_root_push_idx();
            NvValue row = nv_list_new();
            *nv_root_slot(widx) = row;
            for (j = 0; j < b->len; j++) {
                NvValue cell = j3_call_pair(func, a->items[i], b->items[j], line, col);
                nv_list_push_raw(row.u.list, cell);
            }
            nv_list_push_raw(out.u.list, row);
            nv_root_pop();
        }
    }
    nv_root_pop();
    return out;
}

// ---------- 57: التجميع حسب ----------
static NvValue nv_std_lists_group_by(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.جمع_حسب", args, nargs, 2, line, col);
    NvList *items = j14r_need_list("قوائم.جمع_حسب", args[0], line, col);
    NvValue func = args[1];
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0;
    for (i = 0; i < items->len; i++) {
        NvValue key_v = j3_call_single(func, items->items[i], line, col);
        char *key = nv_fmt_value(key_v);
        int found = -1;
        size_t g = 0;
        for (g = 0; g < out.u.list->len; g++) {
            NvValue pair = out.u.list->items[g];
            if (pair.tag == NV_LIST && pair.u.list->len == 2 &&
                pair.u.list->items[0].tag == NV_STR &&
                strcmp(pair.u.list->items[0].u.str->data, key) == 0) {
                found = (int)g;
                break;
            }
        }
        if (found < 0) {
            size_t pidx = nv_root_push_idx();
            NvValue pair = nv_list_new();
            *nv_root_slot(pidx) = pair;
            nv_list_push_raw(pair.u.list, nv_str_new(key, strlen(key)));
            NvValue grp = nv_list_new();
            nv_list_push_raw(grp.u.list, nv_clone_value(items->items[i]));
            nv_list_push_raw(pair.u.list, grp);
            nv_list_push_raw(out.u.list, pair);
            nv_root_pop();
        } else {
            NvValue pair = out.u.list->items[found];
            nv_list_push_raw(pair.u.list->items[1].u.list, nv_clone_value(items->items[i]));
        }
        free(key);
    }
    nv_root_pop();
    return out;
}

// ---------- 58: البادئات واللاحقات والتراكمي ----------
static NvValue nv_std_lists_prefixes(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.بادئات", args, nargs, 1, line, col);
    NvList *items = j14r_need_list("قوائم.بادئات", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0, j = 0;
    for (i = 1; i <= items->len; i++) {
        size_t pidx = nv_root_push_idx();
        NvValue pre = nv_list_new();
        *nv_root_slot(pidx) = pre;
        for (j = 0; j < i; j++) {
            nv_list_push_raw(pre.u.list, nv_clone_value(items->items[j]));
        }
        nv_list_push_raw(out.u.list, pre);
        nv_root_pop();
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_lists_suffixes(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.لاحقات", args, nargs, 1, line, col);
    NvList *items = j14r_need_list("قوائم.لاحقات", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t i = 0, j = 0;
    for (i = 0; i < items->len; i++) {
        size_t pidx = nv_root_push_idx();
        NvValue suf = nv_list_new();
        *nv_root_slot(pidx) = suf;
        for (j = i; j < items->len; j++) {
            nv_list_push_raw(suf.u.list, nv_clone_value(items->items[j]));
        }
        nv_list_push_raw(out.u.list, suf);
        nv_root_pop();
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_lists_cumulative(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.مجاميع_تراكمية", args, nargs, 1, line, col);
    NvList *items = j14r_need_list("قوائم.مجاميع_تراكمية", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    double acc = 0.0;
    size_t i = 0;
    for (i = 0; i < items->len; i++) {
        acc += nv_std_need_number("قوائم.مجاميع_تراكمية", items->items[i], line, col);
        nv_list_push_raw(out.u.list, nv_num(acc));
    }
    nv_root_pop();
    return out;
}

// ---------- 59: النوافذ ----------
static long long j3_window_size(const char *display, NvValue *args, int nargs, NvList **items_out, int line, int col) {
    nv_std_need_args(display, args, nargs, 2, line, col);
    NvList *items = j14r_need_list(display, args[0], line, col);
    long long size = nv_std_need_integer(display, args[1], line, col);
    if (size < 1) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حجم النافذة في %s يجب أن يكون عددا صحيحا ≥ 1", display);
    }
    if ((size_t)size > items->len) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "حجم النافذة أكبر من القائمة في %s", display);
    }
    *items_out = items;
    return size;
}

static NvValue nv_std_lists_windows(NvValue *args, int nargs, int line, int col) {
    NvList *items = NULL;
    long long size = j3_window_size("قوائم.نوافذ", args, nargs, &items, line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t start = 0, j = 0;
    for (start = 0; start + (size_t)size <= items->len; start++) {
        size_t widx = nv_root_push_idx();
        NvValue win = nv_list_new();
        *nv_root_slot(widx) = win;
        for (j = 0; j < (size_t)size; j++) {
            nv_list_push_raw(win.u.list, nv_clone_value(items->items[start + j]));
        }
        nv_list_push_raw(out.u.list, win);
        nv_root_pop();
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_lists_windows_outside(NvValue *args, int nargs, int line, int col) {
    NvList *items = NULL;
    long long size = j3_window_size("قوائم.خارج_النوافذ", args, nargs, &items, line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    size_t start = 0, j = 0;
    for (start = 0; start + (size_t)size <= items->len; start++) {
        size_t widx = nv_root_push_idx();
        NvValue comp = nv_list_new();
        *nv_root_slot(widx) = comp;
        for (j = 0; j < start; j++) {
            nv_list_push_raw(comp.u.list, nv_clone_value(items->items[j]));
        }
        for (j = start + (size_t)size; j < items->len; j++) {
            nv_list_push_raw(comp.u.list, nv_clone_value(items->items[j]));
        }
        nv_list_push_raw(out.u.list, comp);
        nv_root_pop();
    }
    nv_root_pop();
    return out;
}

// ---------- 60: الترتيب تصاعديا وتنازليا ----------
static int j3_cmp_double_desc(const void *a, const void *b) {
    double x = *(const double *)a;
    double y = *(const double *)b;
    if (x < y) return 1;
    if (x > y) return -1;
    return 0;
}

static int j3_cmp_str(const void *a, const void *b) {
    const char *x = *(const char *const *)a;
    const char *y = *(const char *const *)b;
    return strcmp(x, y);
}

static int j3_cmp_str_desc(const void *a, const void *b) {
    return -j3_cmp_str(a, b);
}

static NvValue j3_sort_impl(NvValue *args, int nargs, const char *display, int desc, int line, int col) {
    nv_std_need_args(display, args, nargs, 1, line, col);
    NvList *items = j14r_need_list(display, args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    if (items->len == 0) {
        nv_root_pop();
        return out;
    }
    if (items->items[0].tag == NV_NUM) {
        double *xs = (double *)malloc(sizeof(double) * items->len);
        size_t i = 0;
        for (i = 0; i < items->len; i++) {
            double n = nv_std_need_number(display, items->items[i], line, col);
            if (isnan(n) || isinf(n)) {
                free(xs);
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "الترتيب في %s يرفض الأعداد غير المنتهية", display);
            }
            xs[i] = n;
        }
        if (desc) {
            qsort(xs, items->len, sizeof(double), j3_cmp_double_desc);
        } else {
            qsort(xs, items->len, sizeof(double), nv_std_stats_cmp_double);
        }
        for (i = 0; i < items->len; i++) {
            nv_list_push_raw(out.u.list, nv_num(xs[i]));
        }
        free(xs);
    } else if (items->items[0].tag == NV_STR) {
        const char **xs = (const char **)malloc(sizeof(char *) * items->len);
        size_t i = 0;
        for (i = 0; i < items->len; i++) {
            xs[i] = nv_std_need_string(display, items->items[i], line, col);
        }
        if (desc) {
            qsort(xs, items->len, sizeof(char *), j3_cmp_str_desc);
        } else {
            qsort(xs, items->len, sizeof(char *), j3_cmp_str);
        }
        for (i = 0; i < items->len; i++) {
            nv_list_push_raw(out.u.list, nv_str_new(xs[i], strlen(xs[i])));
        }
        free(xs);
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة %s تتوقع قائمة أعداد أو قائمة نصوص لكن أعطيت قائمة تبدأ بـ %s", display, nv_type_name(items->items[0]));
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_lists_sort(NvValue *args, int nargs, int line, int col) {
    return j3_sort_impl(args, nargs, "قوائم.رتب", 0, line, col);
}

static NvValue nv_std_lists_sort_desc(NvValue *args, int nargs, int line, int col) {
    return j3_sort_impl(args, nargs, "قوائم.رتب_تنازليا", 1, line, col);
}

// ---------- 73: القطع ----------
static NvValue nv_std_lists_chunk(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.قطع", args, nargs, 2, line, col);
    long long size = nv_std_need_integer("قوائم.قطع", args[1], line, col);
    if (size < 1) {
        nv_error(NV_ERR_RUNTIME, "حجم الجزء في قوائم.قطع يجب أن يكون عددا صحيحا ≥ 1", line, col);
    }
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    if (args[0].tag == NV_LIST) {
        NvList *items = args[0].u.list;
        size_t start = 0;
        for (start = 0; start < items->len; start += (size_t)size) {
            size_t widx = nv_root_push_idx();
            NvValue chunk = nv_list_new();
            *nv_root_slot(widx) = chunk;
            size_t end = start + (size_t)size;
            if (end > items->len) end = items->len;
            size_t j = 0;
            for (j = start; j < end; j++) {
                nv_list_push_raw(chunk.u.list, nv_clone_value(items->items[j]));
            }
            nv_list_push_raw(out.u.list, chunk);
            nv_root_pop();
        }
    } else if (args[0].tag == NV_STR) {
        const char *text = args[0].u.str->data;
        size_t tlen = strlen(text);
        size_t nchars = nv_utf8_chars(text, tlen);
        size_t start = 0;
        for (start = 0; start < nchars; start += (size_t)size) {
            size_t end = start + (size_t)size;
            if (end > nchars) end = nchars;
            size_t clen0 = 0, clen1 = 0;
            const char *s0 = nv_utf8_char_at(text, tlen, start, &clen0);
            const char *s1 = (end < nchars) ? nv_utf8_char_at(text, tlen, end, &clen1) : text + tlen;
            nv_list_push_raw(out.u.list, nv_str_new(s0, (size_t)(s1 - s0)));
        }
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة قوائم.قطع تتوقع قائمة أو نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    nv_root_pop();
    return out;
}

static NvValue nv_std_lists_split_sep(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("قوائم.قطع_بفاصل", args, nargs, 2, line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    if (args[0].tag == NV_LIST) {
        NvList *items = args[0].u.list;
        size_t widx = nv_root_push_idx();
        NvValue cur = nv_list_new();
        *nv_root_slot(widx) = cur;
        size_t i = 0;
        for (i = 0; i < items->len; i++) {
            if (nv_eq(items->items[i], args[1])) {
                nv_list_push_raw(out.u.list, cur);
                cur = nv_list_new();
                *nv_root_slot(widx) = cur;
            } else {
                nv_list_push_raw(cur.u.list, nv_clone_value(items->items[i]));
            }
        }
        nv_list_push_raw(out.u.list, cur);
        nv_root_pop();
    } else if (args[0].tag == NV_STR) {
        if (args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة قوائم.قطع_بفاصل تتوقع فاصلا نصيا للنص لكن أعطيت %s", nv_type_name(args[1]));
        }
        const char *text = args[0].u.str->data;
        const char *sep = args[1].u.str->data;
        if (sep[0] == '\0') {
            nv_error(NV_ERR_RUNTIME, "الفاصل الفارغ مرفوض في قوائم.قطع_بفاصل", line, col);
        }
        size_t seplen = strlen(sep);
        const char *p = text;
        const char *hit = NULL;
        while ((hit = strstr(p, sep)) != NULL) {
            nv_list_push_raw(out.u.list, nv_str_new(p, (size_t)(hit - p)));
            p = hit + seplen;
        }
        nv_list_push_raw(out.u.list, nv_str_new(p, strlen(p)));
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة قوائم.قطع_بفاصل تتوقع قائمة أو نصا لكن أعطيت %s", nv_type_name(args[0]));
    }
    nv_root_pop();
    return out;
}

// ---------- 74: تقسيم الكلمات (إكمال وحدة نص) ----------
// كـ قسم_كلمات لكن الفاصل أي مسافة بيضاء ASCII.
static int j3_is_space_byte(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\x0b' || c == '\x0c';
}

static NvValue nv_std_text_split_words_all(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("نص.قسم_إلى_كلمات", args, nargs, 1, line, col);
    const char *text = nv_std_need_string("نص.قسم_إلى_كلمات", args[0], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    const char *p = text;
    while (*p) {
        while (*p && j3_is_space_byte(*p)) {
            p++;
        }
        if (!*p) {
            break;
        }
        const char *start = p;
        while (*p && !j3_is_space_byte(*p)) {
            p++;
        }
        nv_list_push_raw(out.u.list, nv_str_new(start, (size_t)(p - start)));
    }
    nv_root_pop();
    return out;
}

// ---------- وحدة دوال جديدة (J3: 61+65 و63 و64 و75 و77 و78 و80 و74-آلة) ----------

// ---------- 61 + 65: دالة تركيب واحدة ----------
static NvValue nv_std_funcs_compose(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.ركب", args, nargs, 3, line, col);
    NvValue mid = j3_call_single(args[1], args[2], line, col);
    return j3_call_single(args[0], mid, line, col);
}

// ---------- 63: اختيار الدالة ----------
static NvValue nv_std_funcs_choose(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.اختر_دالة", args, nargs, 2, line, col);
    NvList *funcs = j14r_need_list("دوال.اختر_دالة", args[0], line, col);
    long long idx = nv_std_need_integer("دوال.اختر_دالة", args[1], line, col);
    if (idx < 0 || (size_t)idx >= funcs->len) {
        nv_error(NV_ERR_RUNTIME, "فهرس الدالة خارج حدود القائمة في دوال.اختر_دالة", line, col);
    }
    NvValue f = funcs->items[idx];
    j3_need_func("دوال.اختر_دالة", f, line, col);
    return f;
}

static NvValue nv_std_funcs_choose_if(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.اختر_دالة_بشرط", args, nargs, 3, line, col);
    NvList *funcs = j14r_need_list("دوال.اختر_دالة_بشرط", args[0], line, col);
    NvList *conds = j14r_need_list("دوال.اختر_دالة_بشرط", args[1], line, col);
    if (funcs->len != conds->len) {
        nv_error(NV_ERR_RUNTIME, "قائمتا الدوال والشروط في دوال.اختر_دالة_بشرط يجب أن تتساويا طولا", line, col);
    }
    size_t i = 0;
    for (i = 0; i < funcs->len; i++) {
        j3_need_func("دوال.اختر_دالة_بشرط", funcs->items[i], line, col);
        if (j3_call_bool(conds->items[i], args[2], "دوال.اختر_دالة_بشرط", line, col)) {
            return funcs->items[i];
        }
    }
    nv_error(NV_ERR_RUNTIME, "لا دالة يتحقق شرطها في دوال.اختر_دالة_بشرط", line, col);
    return nv_num(0);
}

// ---------- 64: التطبيق الجزئي ----------
static NvValue nv_std_funcs_partial(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.ثبت", args, nargs, 3, line, col);
    return j3_call_pair(args[0], args[1], args[2], line, col);
}

// ---------- 75: تبديل المعاملات ----------
static NvValue nv_std_funcs_swap(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.بدل_المعاملين", args, nargs, 3, line, col);
    return j3_call_pair(args[0], args[2], args[1], line, col);
}

// ---------- 77: التخزين المؤقت ----------
// ذاكرة قاموسية ساكنة مجذرة (جذر عام) بمفتاح
// اسم الدالة + الصيغة المطبوعة للمعامل — مرآة
// الذاكرة المحلية للخيط في stdlib.rs، بالقيدين
// نفسيهما (مسماة فقط؛ ولا مجهولة).
static NvValue nv_memo_cache;
static int nv_memo_cache_ready = 0;

static const char *nv_func_memo_name(NvValue v) {
    if (v.tag == NV_FUNC) return v.u.func->name;
    if (v.tag == NV_NATIVE) return v.u.native->name;
    if (v.tag == NV_BOUND) return nv_func_memo_name(v.u.bound->func);
    return NULL;
}

static NvValue nv_std_funcs_memoize(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.احفظ_النتائج", args, nargs, 2, line, col);
    const char *name = nv_func_memo_name(args[0]);
    if (!name || strcmp(name, "مجهولة") == 0) {
        nv_error(NV_ERR_RUNTIME, "احفظ_النتائج يستقبل دالة مسماة فقط — الدالة المجهولة بلا اسم لا تُحفظ نتائجها", line, col);
    }
    char *arg_s = nv_fmt_value(args[1]);
    size_t klen = strlen(name) + strlen(arg_s) + 2;
    char *key = (char *)malloc(klen);
    snprintf(key, klen, "%s|%s", name, arg_s);
    free(arg_s);
    if (!nv_memo_cache_ready) {
        nv_memo_cache = nv_dict_new();
        nv_add_global_root(nv_memo_cache);
        nv_memo_cache_ready = 1;
    }
    NvDict *d = nv_memo_cache.u.dict;
    int idx = nv_dict_find(d, key);
    if (idx >= 0) {
        NvValue hit = d->entries[idx].val;
        free(key);
        return hit;
    }
    NvValue res = j3_call_single(args[0], args[1], line, col);
    nv_dict_set_raw(d, key, res);
    free(key);
    return res;
}

// ---------- 78: التكرار حتى / التكرار ن مرات ----------
static NvValue nv_std_funcs_repeat_until(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.كرر_حتى", args, nargs, 3, line, col);
    NvValue cur = args[1];
    size_t ridx = nv_root_push_idx();
    *nv_root_slot(ridx) = cur;
    int i = 0;
    for (i = 0; i < 10000; i++) {
        if (j3_call_bool(args[2], cur, "دوال.كرر_حتى", line, col)) {
            nv_root_pop();
            return cur;
        }
        cur = j3_call_single(args[0], cur, line, col);
        *nv_root_slot(ridx) = cur;
    }
    nv_error(NV_ERR_RUNTIME, "تجاوز كرر_حتى حد التكرار (10000) — الشرط لم يتحقق", line, col);
    return nv_num(0);
}

static NvValue nv_std_funcs_repeat_n(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.كرر_ن_مرة", args, nargs, 3, line, col);
    long long n = nv_std_need_integer("دوال.كرر_ن_مرة", args[2], line, col);
    if (n < 0) {
        nv_error(NV_ERR_RUNTIME, "عدد التكرار في دوال.كرر_ن_مرة يجب أن يكون ≥ 0", line, col);
    }
    NvValue cur = args[1];
    size_t ridx = nv_root_push_idx();
    *nv_root_slot(ridx) = cur;
    long long i = 0;
    for (i = 0; i < n; i++) {
        cur = j3_call_single(args[0], cur, line, col);
        *nv_root_slot(ridx) = cur;
    }
    nv_root_pop();
    return cur;
}

// ---------- 80: عكس الدالة — مجموعة محدودة ----------
static NvValue nv_std_funcs_inverse(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.اعكس_الدالة", args, nargs, 3, line, col);
    const char *op = nv_std_need_string("دوال.اعكس_الدالة", args[0], line, col);
    double k = nv_std_need_number("دوال.اعكس_الدالة", args[1], line, col);
    double y = nv_std_need_number("دوال.اعكس_الدالة", args[2], line, col);
    double r = 0.0;
    if (strcmp(op, "أضف") == 0) {
        r = y - k;
    } else if (strcmp(op, "اطرح") == 0) {
        r = y + k;
    } else if (strcmp(op, "اضرب") == 0) {
        if (k == 0.0) {
            nv_error(NV_ERR_RUNTIME, "لا عكس للضرب في صفر في دوال.اعكس_الدالة", line, col);
        }
        r = y / k;
    } else if (strcmp(op, "اقسم") == 0) {
        if (k == 0.0) {
            nv_error(NV_ERR_RUNTIME, "لا عكس للقسمة على صفر في دوال.اعكس_الدالة", line, col);
        }
        r = y * k;
    } else {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "عملية بلا عكس معروف: %s — المتاح: أضف، اطرح، اضرب، اقسم", op);
    }
    return nv_num(r);
}

// ---------- 74: آلة حالات ----------
static NvValue nv_std_funcs_state_machine(NvValue *args, int nargs, int line, int col) {
    nv_std_need_args("دوال.آلة_حالات", args, nargs, 3, line, col);
    if (args[0].tag != NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "الدالة دوال.آلة_حالات تتوقع قاموسا لكن أعطيت %s", nv_type_name(args[0]));
    }
    NvDict *trans = args[0].u.dict;
    const char *start = nv_std_need_string("دوال.آلة_حالات", args[1], line, col);
    NvList *inputs = j14r_need_list("دوال.آلة_حالات", args[2], line, col);
    size_t ridx = nv_root_push_idx();
    NvValue out = nv_list_new();
    *nv_root_slot(ridx) = out;
    char *state = strdup(start);
    nv_list_push_raw(out.u.list, nv_str_new(state, strlen(state)));
    size_t i = 0;
    for (i = 0; i < inputs->len; i++) {
        const char *inp = nv_std_need_string("دوال.آلة_حالات", inputs->items[i], line, col);
        int ridx2 = nv_dict_find(trans, state);
        if (ridx2 < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "لا انتقالات من الحالة %s في دوال.آلة_حالات", state);
        }
        NvValue row = trans->entries[ridx2].val;
        if (row.tag != NV_DICT) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "صف الانتقالات للحالة %s ليس قاموسا في دوال.آلة_حالات", state);
        }
        int cidx = nv_dict_find(row.u.dict, inp);
        if (cidx < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "لا انتقال من الحالة %s بالمدخل %s في دوال.آلة_حالات", state, inp);
        }
        NvValue next = row.u.dict->entries[cidx].val;
        if (next.tag != NV_STR) {
            free(state);
            nv_error(NV_ERR_RUNTIME, "الحالة التالية ليست نصا في دوال.آلة_حالات", line, col);
        }
        free(state);
        state = strdup(next.u.str->data);
        nv_list_push_raw(out.u.list, nv_str_new(state, strlen(state)));
    }
    free(state);
    nv_root_pop();
    return out;
}

void nv_std_funcs_fill(NvDict *d) {
    nv_std_add_fn(d, "ركب", nv_std_funcs_compose);
    nv_std_add_fn(d, "اختر_دالة", nv_std_funcs_choose);
    nv_std_add_fn(d, "اختر_دالة_بشرط", nv_std_funcs_choose_if);
    nv_std_add_fn(d, "ثبت", nv_std_funcs_partial);
    nv_std_add_fn(d, "بدل_المعاملين", nv_std_funcs_swap);
    nv_std_add_fn(d, "احفظ_النتائج", nv_std_funcs_memoize);
    nv_std_add_fn(d, "كرر_حتى", nv_std_funcs_repeat_until);
    nv_std_add_fn(d, "كرر_ن_مرة", nv_std_funcs_repeat_n);
    nv_std_add_fn(d, "اعكس_الدالة", nv_std_funcs_inverse);
    nv_std_add_fn(d, "آلة_حالات", nv_std_funcs_state_machine);
}

// ---------- 69: تنفيذ نص — الثنائي الأصلي ----------
// لا مفسر مضمّن في الثنائي الأصلي — فالوحدة مسجلة
// باسمها ونداؤها رفض صريح بخطأ عربي (لا وحدة غائبة)؛
// والتنفيذ الحقيقي في المفسر وآلة البايت كود فقط،
// وحالة التكافؤ تُتخطَّى صراحة في c_parity (C_SKIP).
static NvValue nv_std_exec_text(NvValue *args, int nargs, int line, int col) {
    (void)args;
    (void)nargs;
    nv_error(NV_ERR_RUNTIME, "تنفيذ النص غير مدعوم في الثنائي الأصلي — نفّذ النص في المفسر أو آلة البايت كود", line, col);
    return nv_num(0);
}

void nv_std_exec_fill(NvDict *d) {
    nv_std_add_fn(d, "نفذ_نصا", nv_std_exec_text);
}
