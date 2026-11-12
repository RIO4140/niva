//! مكتبة وقت التشغيل لخلفية C — محرك نيفا الأصلي (مهمة 29).
//! كل القيم والرسائل عربية؛ والمعرّفات الداخلية لاتينية فقط.
//! النموذج: سلسلة نطاقات ديناميكية مثل الآلة الافتراضية
//! (أقرب نطاق يربط الاسم، وإلا الأعمق)؛ والقوائم والقواميس
//! قيم تُنسخ عند التعيين؛ والاستدعاء يحسب عمقه بحد ٥٠٠.

#ifdef __APPLE__
// على Darwin يعريف _POSIX_C_SOURCE بلا _DARWIN_C_SOURCE
// يقيّد الترويسات بـ POSIX الصارم ويخفي امتدادات BSD
// (memmem وstrcasestr) وINADDR_LOOPBACK — اجتماع 36 §51؛
// فعلى Apple يُعرَّف _DARWIN_C_SOURCE وحده (وهو يعيد
// واجهة Darwin الكاملة)، ولا يُعرَّف _POSIX_C_SOURCE
// ولا _GNU_SOURCE هناك.
#define _DARWIN_C_SOURCE 1
#else
#define _GNU_SOURCE 1
#define _POSIX_C_SOURCE 200809L
#endif

#ifndef NIVA_RT_H
#define NIVA_RT_H

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <setjmp.h>
#include <math.h>
#include <ctype.h>
#include <wchar.h>
#include <wctype.h>
#include <locale.h>
#include <errno.h>
#include <limits.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <sys/stat.h>
#include <sys/types.h>
// lstat غير موجودة في MinGW؛ والبديل stat يتبع الروابط
// الرمزية، وهو سلوك المحرك نفسه على ويندوز — والعمق
// محدود في كل مواضع الاستخدام (بحث ≤ 4، شجرة ≤ 3،
// حجم بميزانية مدخلات).
#ifdef _WIN32
#define nv_lstat(path, st) stat((path), (st))
#else
#define nv_lstat(path, st) lstat((path), (st))
#endif
#ifndef _WIN32
// sys/wait.h غير موجود في MinGW (ويندوز): دوال
// العمليات التي تحتاجه معطلة هناك برفض عربي
// (اجتماع 35 §50)؛ فلا يُضمَّن أصلا على ويندوز.
#include <sys/wait.h>
#endif
#include <signal.h>
#include <pthread.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---------- أنواع الأخطاء ----------
// (نفس أسماء الأنواع العربية في error.rs: لغوي، نحوي، نوع،
// وقت التشغيل، إدخال/إخراج، دلالي — تُطبع كما هي.)
typedef enum {
    NV_ERR_LEX = 0,
    NV_ERR_PARSE,
    NV_ERR_TYPE,
    NV_ERR_RUNTIME,
    NV_ERR_IO,
    NV_ERR_SEMANTIC
} NvErrKind;

const char *nv_err_kind_ar(NvErrKind k);

// خطأ نيفا حي: الرسالة + السطر + العمود + ملف المصدر
// (اجتماع 33: كل خطأ يسمّي الملف كما في المفسر —
// الفهرس في جدول nv_source_files الذي يولّده cgen،
// و-1 حين لا ملف معلوم).
typedef struct {
    NvErrKind kind;
    char *message; // عربي، مملوك لهيكل الخطأ
    int line;
    int col;
    int file_idx;
} NvError;

// ---------- جامع القمامة التتبعي (v1.3 §6) ----------
// جيلان: صغير (يُجمع كثيرا) وكبير (يُجمع نادرا)؛ بلا نقل
// (عناوين ثابتة)؛ مجموعة متذكَّرة للكتابة من كبير إلى صغير؛
// ونقاط أمان عند التخصيص وفي العمليات الأصلية الطويلة.
typedef struct NvObj {
    uint8_t marked;   // بت العلامة في الجمع الجاري
    uint8_t gen;      // 0 = صغير، 1 = كبير
    uint8_t kind;     // نوع الكائن (انظر NvObjKind)
    struct NvObj *next; // قائمة كل الكائنات الحية
} NvObj;

typedef enum {
    NV_OBJ_STR = 1,
    NV_OBJ_LIST,
    NV_OBJ_DICT,
    NV_OBJ_FUNC,
    NV_OBJ_MODULE,
    NV_OBJ_CLASS,
    NV_OBJ_IFACE,
    NV_OBJ_INST,
    NV_OBJ_BOUND,
    NV_OBJ_SCOPE,
    NV_OBJ_CELL,
    NV_OBJ_NATIVE,
    NV_OBJ_HANDLE, // مقابض: ملف، عملية، قناة، حلقة، كتلة ذاكرة
    NV_OBJ_BYTES,   // بايتات خام (تشفير.عشوائي_آمن)
    // ج6: أنواع الأعداد الجديدة — في آخر التعداد عمدا.
    NV_OBJ_BIGINT,  // عدد صحيح ممتد (سلسلة عشرية موحدة)
    NV_OBJ_RATIONAL, // كسر دقيق (بسط/مقام من كبيرين)
    NV_OBJ_BITS,    // بتات 0/1 (بايتات معبأة، الأهم أولا)
    NV_OBJ_BOXED    // معلب (قيمة واحدة داخله)
} NvObjKind;

// ---------- القيم ----------
// القيمة 24 بايتا تُمرَّر بالقيمة؛ والكائنات الثقيلة خلف
// مؤشرات تُدار بجامع القمامة. النص والقائمة والقاموس
// قيم دلاليا (تُنسخ عند التعيين) لكن النسخ يشارك الكائن
// حتى أول تعديل؟ لا — النسخ عميق كما في المحرك: ب = قائمة
// تنسخ العناصر، فتعديل ب لا يمس قائمة.
typedef struct NvStr NvStr;
typedef struct NvList NvList;
typedef struct NvDict NvDict;
typedef struct NvFunc NvFunc;
typedef struct NvModule NvModule;
typedef struct NvClass NvClass;
typedef struct NvIface NvIface;
typedef struct NvInst NvInst;
typedef struct NvBound NvBound;
typedef struct NvScope NvScope;
typedef struct NvCell NvCell;
typedef struct NvNative NvNative;
typedef struct NvHandle NvHandle;
typedef struct NvBytes NvBytes;
// ج6: الأنواع الجديدة.
typedef struct NvBigInt NvBigInt;
typedef struct NvRational NvRational;
typedef struct NvBits NvBits;
typedef struct NvBoxed NvBoxed;

typedef enum {
    NV_NUM = 1,
    NV_STR,
    NV_BOOL,
    NV_LIST,
    NV_DICT,
    NV_NIL,
    NV_FUNC,
    NV_NATIVE,
    NV_MODULE,
    NV_CLASS,
    NV_IFACE,
    NV_INST,
    NV_BOUND,
    NV_HANDLE,
    NV_BYTES,
    // البندان 138 و139 (الدفعة 3 من الـ579):
    // مترابطة وتشكيلة — في آخر التعداد عمدا، فلا
    // تتغير أرقام الوسوم القائمة. التخزين NvList
    // نفسه (طول/سعة/عناصر)، والوسم وحده يميز
    // الدلالة (ترتيب وثبات، أو فرادة بلا ترتيب).
    NV_TUPLE,
    NV_SET,
    // ج6: الأنواع الجديدة — في آخر التعداد عمدا، فلا
    // تتغير أرقام الوسوم القائمة أبدا.
    NV_BIGINT,
    NV_RATIONAL,
    NV_COMPLEX,
    NV_BITS,
    NV_BOXED
} NvTag;

typedef struct {
    NvTag tag;
    union {
        double num;
        NvStr *str;
        int boolean;
        NvList *list;
        NvDict *dict;
        // البندان 138 و139: المترابطة والتشكيلة
        // تخزينهما NvList (الأعضاء أسماء مستعارة
        // للوضوح في مواضع المترابطة/التشكيلة).
        NvList *tuple;
        NvList *set;
        // ج6: الأنواع الجديدة — المركب مضمن (حقيقي،
        // تخيلي) بلا كومة؛ والباقي كائنات جامع قمامة.
        NvBigInt *bigint;
        NvRational *rational;
        struct { double re, im; } complex;
        NvBits *bits;
        NvBoxed *boxed;
        NvFunc *func;
        NvNative *native;
        NvModule *module;
        NvClass *klass;
        NvIface *iface;
        NvInst *inst;
        NvBound *bound;
        NvHandle *handle;
        NvBytes *bytes;
    } u;
} NvValue;

// دالة نيفا مولدة: تستلم إطار الاستدعاء (النطاق الحي،
// أبوه السلسلة الملتقَطة أو نطاق الوحدة) والوسائط.
typedef NvValue (*NvCFunc)(NvScope *frame, NvValue *args, int nargs,
                            int line, int col);
// دالة أصلية: تستلم الوسائط والموضع، وتعيد قيمة.
typedef NvValue (*NvNativeFn)(NvValue *args, int nargs, int line, int col);

// ---------- الكائنات ----------
struct NvStr {
    NvObj hdr;
    size_t len;   // بايتات UTF-8
    size_t chars; // عدد المحارف (لطول والفهرسة)
    char data[];  // منتهية بصفر
};

struct NvList {
    NvObj hdr;
    size_t len;
    size_t cap;
    NvValue *items; // مصفوفة قيم (تُمسح من الجامع)
};

typedef struct {
    char *key; // مملوك للقاموس
    NvValue val;
} NvDictEntry;

struct NvDict {
    NvObj hdr;
    size_t len;
    size_t cap;
    NvDictEntry *entries; // بترتيب الإدخال
};

struct NvFunc {
    NvObj hdr;
    char *name;          // اسم الدالة (لرسائل الأخطاء)
    NvCFunc cfunc;       // جسم الدالة المولد
    NvScope *captured;   // السلسلة الملتقَطة عند التعريف
    char *module;        // اسم الوحدة أو NULL (دالة وحدة)
    NvScope *module_scope; // نطاق الوحدة (يُستنسخ جديدا كل استدعاء)
    char **param_names;  // أسماء المعاملات (لرسائل الأنواع)
    char **param_types;  // أنواع المعاملات المعلنة أو NULL
    char *return_type;   // نوع الإرجاع المعلن أو NULL
    int nparams;
    // البنود 109–112 (الاجتماع 83 §98): قيم
    // افتراضية مقيَّمة وقت التعريف (طولها nparams،
    // ويُعتدّ بها حيث has_default)، وجامعا الزائد.
    NvValue *defaults;
    int *has_default;
    char *vararg;        // اسم جامع الموضعي أو NULL
    char *kwarg;         // اسم جامع المفتاحي أو NULL
    // البنود 145–147 (الاجتماع 83 §98): معاملات النوع
    // المعلنة (أسماء وحدود وافتراضيات، منسوخة)، والربط
    // المحلول عند التخصيص (قيم موازية للأسماء أو NULL).
    char **tp_names;
    char **tp_bounds;
    char **tp_defaults;
    int ntype_params;
    char **type_binding; // NULL إن لم تُخصَّص بعد
};

struct NvNative {
    NvObj hdr;
    char *name;
    NvNativeFn fn;
};

struct NvModule {
    NvObj hdr;
    char *name;
    NvDict *exports; // الاسم -> القيمة (المصدرة فقط)
    NvScope *scope;  // النطاق الكامل بعد تنفيذ الوحدة
};

struct NvClass {
    NvObj hdr;
    char *name;
    NvDict *methods; // الاسم -> دالة (مسطحة: الأب ثم الابن يغلب)
    NvClass *base;
    char **interfaces;
    int ninterfaces;
    // البنود 145–147: معاملات النوع المعلنة والربط
    // المحلول عند التخصيص (كما في NvFunc).
    char **tp_names;
    char **tp_bounds;
    char **tp_defaults;
    int ntype_params;
    char **type_binding; // NULL إن لم تُخصَّص بعد
};

struct NvIface {
    NvObj hdr;
    char *name;
    char **methods;
    int *arity;
    int nmethods;
};

struct NvInst {
    NvObj hdr;
    NvClass *klass;
    NvDict *fields; // الحقول (قاموس)
    // البنود 145–147: ربط النوع المحلول وقت الإنشاء
    // (قيم موازية لـ klass->tp_names أو NULL).
    char **type_binding;
};

struct NvBound {
    NvObj hdr;
    NvValue func; // دالة نيفا
    NvInst *inst;
};

// خلية قيمة في نطاق: التعديل عبر سلسلة مشتركة يصل
// كل من يشارك النطاق (الإغلاقات).
struct NvCell {
    NvObj hdr;
    NvValue val;
};

typedef struct NvBinding {
    char *name;
    NvCell *cell;
    struct NvBinding *next;
} NvBinding;

struct NvScope {
    NvObj hdr;
    NvScope *parent;
    NvBinding *bindings;
};

// مقابض الأنواع الأصلية (ملف، عملية، قناة، حلقة، كتلة).
struct NvHandle {
    NvObj hdr;
    int kind; // 1=ملف 2=عملية 3=قناة 4=حلقة 5=كتلة-ذاكرة 6=مقبس
    int id;
    void *ptr; // بيانات خاصة بالنوع (تُحرَّر عند الجمع)
};

struct NvBytes {
    NvObj hdr;
    size_t len;
    unsigned char data[];
};

// ج6: بنى الأنواع الجديدة.
// كبير: أرقام عشرية موحدة بلا إشارة (الإشارة في neg)،
// بلا أصفار بادئة، و"0" للصفر.
// كسر: بسط/مقام موحدان (المقام موجب والقاسم المشترك 1).
// بت: بايتات معبأة (البت الأهم أولا في كل بايت، ثم
// البتات التالية)؛ والبتات الزائدة في البايت الأخير صفر.
// معلب: قيمة واحدة (تُمسح من الجامع كابن).
struct NvBigInt {
    NvObj hdr;
    char *digits; // مملوكة للكائن
    int neg;      // 0 أو 1 (والصفر بلا إشارة أبدا)
};

struct NvRational {
    NvObj hdr;
    NvBigInt *num;
    NvBigInt *den;
};

struct NvBits {
    NvObj hdr;
    size_t nbits;
    unsigned char *bytes; // مملوكة للكائن
};

struct NvBoxed {
    NvObj hdr;
    NvValue inner;
};

// ---------- تدفق التنفيذ ----------
// حالات كتلة: عادي، إرجاع (مع قيمة)، كسر، تابع.
typedef enum {
    NV_FLOW_NORMAL = 0,
    NV_FLOW_RETURN,
    NV_FLOW_BREAK,
    NV_FLOW_CONTINUE
} NvFlow;

extern NvFlow nv_flow;
extern NvValue nv_retval;

// ---------- معالجات الأخطاء (حاول/ماعدا) ----------
// مكدس معالجات: كل حاول يدفع معالجا؛ والخطأ يقفز
// (longjmp) إلى أعمق معالج. الخطأ نفسه في متغير عام
// (nv_last_error) لا في المعالج — فقراءته بعد longjmp
// آمنة (المتغيرات المحلية المعدلة بين setjmp وlongjmp
// غير مضمونة في C).
typedef struct NvHandler {
    jmp_buf jb;
    struct NvHandler *prev;
    int has_except;
} NvHandler;

extern NvHandler *nv_handlers;

// ---------- سياسة الملفات والشبكة (وقت التشغيل) ----------
// تُضبط من main المولد حسب أعلام التشغيل.
extern char *nv_program_dir; // مجلد البرنامج (مطلق)
extern char *nv_cwd;         // المجلد الحالي للجلسة
extern int nv_trusted;       // --اسمح-بالكل
extern int nv_network_allowed; // --اسمح-بالشبكة
extern int nv_server_allowed;  // --اسمح-بالخادم
extern char **nv_local_hosts;  // مضيفات محلية مصرّح بها
extern int nv_n_local_hosts;
extern int nv_call_depth;    // عمق الاستدعاء الحالي
// الدفعة 1 — رموز الخروج (اجتماع 54 §69).
extern int nv_last_exit_code; // رمز آخر أمر مكتمل (0 أولا)
extern int nv_exit_requested; // طلب خروج معلق (0/1)
extern int nv_exit_code;      // رمز الخروج المطلوب
extern char *nv_program_name; // اسم البرنامج كما استُدعي
extern unsigned long long nv_alloc_bytes; // بايتات مخصصة منذ آخر جمع

// خطاف تنظيف المقابض: تستدعيه الجامعة قبل تحرير ptr
// (لإغلاق ملف/مقبس/عملية)؛ تسجله مكتبة الوحدات.
extern void (*nv_handle_finalize)(int kind, void *ptr);
void nv_handle_set_finalizer(void (*fn)(int kind, void *ptr));

// ---------- تهيئة وإنهاء ----------
void nv_init(const char *program_dir);
void nv_shutdown(void);
// نقطة أمان: يتحقق من حاجة الجمع قبل عمل طويل.
void nv_safepoint(void);

// ---------- التخصيص والجامع ----------
// تخصيص كائن يُدار بالجامع (يُطلق جمعا صغيرا عند الحاجة).
void *nv_alloc(size_t size, uint8_t kind);
void nv_collect_minor(void);
void nv_collect_major(void);
// حاجز كتابة: كائن كبير يشير إلى صغير.
void nv_write_barrier(NvObj *old_obj, NvObj *young_obj);
// ---------- نقاط تفتيش الحالة (للمعاجلات) ----------
// longjmp يتخطى تحرير الجذور والإطارات وعدّاد العمق،
// فالمعالج يستعيدها من نقطة التفتيش قبل setjmp.
typedef struct {
    size_t roots;
    int frames;
    int depth;
} NvCheckpoint;

NvCheckpoint nv_checkpoint(void);
void nv_restore_checkpoint(NvCheckpoint cp);

// جذور مؤقتة: مكدس قيم يحمي الوسائط أثناء التخصيص.
NvValue *nv_root_push(void);
void nv_root_pop(void);
void nv_root_pop_n(int n);
// جذور بالفهرس (آمنة عبر realloc للمكدس).
size_t nv_root_push_idx(void);
NvValue *nv_root_slot(size_t idx);
// جذور عامة دائمة (وحدات أصلية، ...).
void nv_add_global_root(NvValue v);
// جذر عام بالمؤشر (قيمة ساكنة تُعاد كتابتها —
// الجامع يقرأ *slot في كل جمع).
void nv_add_global_root_ptr(NvValue *slot);

// ---------- بناء القيم ----------
NvValue nv_num(double n);
NvValue nv_bool(int b);
NvValue nv_nil(void);
NvValue nv_str_new(const char *s, size_t len); // نسخة
NvValue nv_str_take(char *s, size_t len);       // يملك البايتات
NvValue nv_str_cstr(const char *s);
NvValue nv_list_new(void);
NvValue nv_dict_new(void);
// البندان 138 و139 (الدفعة 3 من الـ579): مترابطة
// فارغة، وتشكيلة فارغة (التخزين NvList).
NvValue nv_tuple_new(void);
NvValue nv_set_new(void);
NvValue nv_func_new(const char *name, NvCFunc cfunc, NvScope *captured,
                    const char *module, NvScope *module_scope,
                    char **param_names, char **param_types,
                    const char *return_type, int nparams,
                    NvValue *defaults, int *has_default,
                    const char *vararg, const char *kwarg,
                    const char *type_params);
NvValue nv_native_new(const char *name, NvNativeFn fn);
NvValue nv_module_new(const char *name, NvScope *scope);
NvValue nv_class_new(const char *name, NvClass *base, const char *type_params);
// البنود 145–147: خصّص دالة/فئة بمعاملات نوع
// (نص مفصول بفاصلة ASCII) — تنسخ القيمة مع ربط
// محلول مفحوص (أو خطأ نوع عربي عند المخالفة).
NvValue nv_specialize(NvValue v, const char *type_args, int line, int col);
NvValue nv_iface_new(const char *name);
NvValue nv_inst_new(NvClass *klass);
NvValue nv_bound_new(NvValue func, NvInst *inst);
NvValue nv_handle_new(int kind, int id, void *ptr);
NvValue nv_bytes_new(const unsigned char *data, size_t len);

// نسخ عميق للقيم (قائمة/قاموس) — دلالة التعيين في المحرك.
NvValue nv_clone_value(NvValue v);
// نسخ سطحي لمشاركة الكائن (للاستخدام الداخلي فقط).
NvValue nv_share_value(NvValue v);

// ---------- أسماء الأنواع والعرض ----------
// اسم النوع العربي لقيمة (عدد، نص، ...).
const char *nv_type_name(NvValue v);
// تنسيق قيمة كما يطبعها اطبع (نص خام، وإلا صيغة بايثون).
// تعيد سلسلة مملوكة للمتصل (تُحرَّر بـ free) أو مؤقتة؟
char *nv_fmt_value(NvValue v); // تُحرَّر بـ free
// تنسيق عدد بصيغة عرض Rust (أرقام عربية، ٫ عشرية).
char *nv_fmt_number(double n); // تُحرَّر بـ free
// تحويل نص إلى عدد (لـ عدد()) — 1 على النجاح.
// ---------- ج6: الأنواع والأعداد والدقة ----------
// منشئات القيم الجديدة (تُخصَّص عبر جامع القمامة).
// nv_bigint_cstr: يوحّد النص (`[+-]?أرقام`) أو يخطئ
// «عدد كبير غير صالح: ...»؛ وnv_rational_new يوحّد
// البسط/المقام أو يخطئ «مقام الكسر لا يمكن أن يكون صفرا».
NvValue nv_bigint_cstr(const char *s, int line, int col);
NvValue nv_bigint_from_double(double n, int line, int col);
NvValue nv_rational_new(NvValue num, NvValue den, int line, int col);
NvValue nv_complex_new(double re, double im);
NvValue nv_bits_new(const unsigned char *bytes, size_t nbits);
NvValue nv_bits_from_u64(unsigned long long n, size_t len);
NvValue nv_boxed_new(NvValue inner);
// تحويلات إلى f64 (للرياضيات والتحويل الصريح).
double nv_bigint_to_double(NvValue v);
// تحويلات القيم العددية (تُستخدم أيضا في niva_std.c):
// nv_value_to_rational تعيد 1 مع بسط/مقام موحدين
// جديدين (يُحرَّران بـ free) أو 0؛ وnv_value_to_complex
// تعيد 1 مع الجزأين أو 0.
int nv_value_to_rational(NvValue v, char **num, char **den);
int nv_value_to_complex(NvValue v, double *re, double *im);
double nv_rational_to_double(NvValue v);
double nv_bits_to_double(NvValue v);
// حساب الكبير الدقيق (سلاسل عشرية موحدة). nv_bigint_divmod
// تعيد 0 عند القسمة على صفر.
char *nv_bigint_norm(const char *s); // تُحرَّر بـ free، أو NULL
int nv_bigint_cmp_str(const char *a, const char *b);
int nv_bigint_is_zero_str(const char *a);
char *nv_bigint_add_str(const char *a, const char *b); // تُحرَّر بـ free
char *nv_bigint_sub_str(const char *a, const char *b); // تُحرَّر بـ free
char *nv_bigint_mul_str(const char *a, const char *b); // تُحرَّر بـ free
char *nv_bigint_neg_str(const char *a); // تُحرَّر بـ free
int nv_bigint_divmod_str(const char *a, const char *b, char **q, char **r);
char *nv_bigint_gcd_str(const char *a, const char *b); // تُحرَّر بـ free
// حساب الكسر الدقيق (أزواج موحدة؛ والمقام موجب دائما).
int nv_rat_norm_str(const char *num, const char *den, char **n, char **d);
void nv_rat_add_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d);
void nv_rat_sub_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d);
void nv_rat_mul_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d);
int nv_rat_div_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d);
int nv_rat_rem_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d);
int nv_rat_cmp_str(const char *a1, const char *a2, const char *b1, const char *b2);
double nv_rat_to_double_str(const char *num, const char *den);
int nv_parse_number(const char *s, double *out);
// عدد محارف UTF-8 في نص بايتات.
size_t nv_utf8_chars(const char *s, size_t len);
// المحرف رقم idx (بايتات البداية والطول) أو NULL.
const char *nv_utf8_char_at(const char *s, size_t len, size_t idx, size_t *clen);
// مقارنة نصين بايتيا.
int nv_str_eq(const char *a, size_t alen, const char *b, size_t blen);

// ---------- الأخطاء ----------
// أطلق خطأ نيفا (لا يعود: يقفز إلى المعالج أو ينهي).
void nv_error(NvErrKind kind, const char *message, int line, int col) __attribute__((noreturn));
void nv_error_fmt(NvErrKind kind, int line, int col, const char *fmt, ...) __attribute__((noreturn));
// الخطأ الحالي بعد التقاطه من معالج.
NvError *nv_current_error(void);
void nv_clear_error(void);
// أعد رفع الخطأ الحالي إلى المعالج الأعمق (أو اخرج).
// بلا خطأ معلق تعود صامتة.
void nv_reraise_current(void);
// الدفعة 1 — رموز الخروج (اجتماع 54 §69): اقفز
// بطلب خروج معلق إلى أعمق معالج، بلا ضبط خطأ —
// فمسار الخطأ في حاول يميّز الخروج فيشغّل أخيرا
// وحدها (بلا ماعدا) ثم يعيد رفع الطلب. لا تعود.
void nv_exit_jump(void) __attribute__((noreturn));
// أضف بادئة إلى رسالة الخطأ الحالي (دون تغيير ملفه
// أو سطره أو عموده) — «في وحدة X: » عند عبور حدود
// وحدة أثناء تحميلها، كما في المفسر.
void nv_prefix_error(const char *prefix);
// جدول ملفات المصدر (يولّده cgen: [0] الملف الرئيسي
// ثم كل وحدة مستوردة) — وملف التنفيذ الجاري (فهرسه).
// الكود المولّد يضبط nv_cur_file_idx عند دخول كل
// دالة وعند كل جملة؛ وnv_error يلتقطه في الخطأ.
extern const char *nv_source_files[];
extern const int nv_n_source_files;
extern int nv_cur_file_idx;
// فحص نوع تعيين معلن (رسالة المتغير).
void nv_check_assign_type(NvValue v, const char *type_name, const char *var_name, int line, int col);
// بحث اسم في نطاق أو لاشيء (للبدائل التي لها رسائلها).
NvValue nv_lookup_or_nil(NvScope *scope, const char *name);
NvValue nv_lookup_or_error(NvScope *scope, const char *name, int line, int col);
// الدفعة الأخيرة DEF (البنود 77 و78): قراءة موجَّهة
// بالإعلان — عام من الجذر وحده، ونطاق من المحيط وحده.
NvValue nv_lookup_or_error_global(NvScope *scope, const char *name, int line, int col);
NvValue nv_lookup_or_error_nonlocal(NvScope *scope, const char *name, int line, int col);
NvValue nv_for_snapshot(NvValue v, int line, int col);
long nv_repeat_count(NvValue v, int line, int col);
// فهرس مفتاح في قاموس أو -1.
int nv_dict_find(NvDict *d, const char *key);
// إضافة/استبدال خام في قاموس/قائمة (بلا فحص — للمولد).
void nv_dict_set_raw(NvDict *d, const char *key, NvValue val);
void nv_list_push_raw(NvList *l, NvValue val);
// تصدير وحدة (رسالة «غير مصدر» عند الغياب).
NvValue nv_module_export(NvValue module, const char *name, int line, int col);
// أنشئ فئة بالاسم (رسالتا «فئة أب غير معرفة» و«ليست فئة»).
NvValue nv_class_begin_named(NvScope *scope, const char *name, const char *base_name, int line, int col, const char *type_params);
// أنشئ فئة من قيمة أب (مساعد داخلي).
NvValue nv_class_begin(const char *name, NvValue base_val, int line, int col, const char *type_params);
void nv_class_add_method(NvValue klass, const char *name, NvValue func);
void nv_class_add_interface(NvValue klass, const char *iname, NvValue iface_val, int line, int col);
NvValue nv_iface_begin(const char *name);
void nv_iface_add_method(NvValue iface, const char *name, int arity);
NvValue nv_call_ident(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col);
NvValue nv_call_ident_at(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col);
// الدفعة الأخيرة DEF (البنود 77 و78): استدعاء بالاسم
// مع بحث موجَّه بالإعلان.
NvValue nv_call_ident_at_global(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col);
NvValue nv_call_ident_at_nonlocal(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col);
// البنود 109–112 (الاجتماع 83 §98): مسارات
// الاستدعاء المفتاحية — نفس التوقيعات مع أسماء
// المفتاحيات وقيمها وعددها.
NvValue nv_call_ident_at_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col);
NvValue nv_call_ident_at_global_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col);
NvValue nv_call_ident_at_nonlocal_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col);

// ---------- النطاقات ----------
// نطاق جديد (أب = parent أو NULL).
NvScope *nv_scope_new(NvScope *parent);
// بحث: أقرب نطاق يربط الاسم (أو NULL).
NvCell *nv_scope_lookup_cell(NvScope *scope, const char *name);
NvValue *nv_scope_slot_ptr(NvScope *scope, const char *name, int line, int col);
// تعريف في أعمق نطاق (يعيد الخلية).
NvCell *nv_scope_define(NvScope *scope, const char *name, NvValue v);
// تعيين: أقرب نطاق يربط الاسم، وإلا تعريف في الأعمق.
// الدفعة 4 — المتغيرات (اجتماع 58 §73): يحلّ
// المرجع الاسمي ويرفض الكتابة في مجمد.
void nv_scope_assign(NvScope *scope, const char *name, NvValue v, int line, int col);
// الدفعة الأخيرة DEF (البنود 77 و78): مساعدا
// الإسناد الموجَّه بالإعلان — عام إلى الجذر،
// ونطاق إلى أقرب ربط محيط (وغيابه خطأ).
void nv_scope_assign_global(NvScope *scope, const char *name, NvValue v, int line, int col);
void nv_scope_assign_nonlocal(NvScope *scope, const char *name, NvValue v, int line, int col);
// بحث موجَّه بالإعلان — عام من الجذر وحده،
// ونطاق من المحيط وحده.
NvCell *nv_scope_lookup_cell_global(NvScope *scope, const char *name);
NvCell *nv_scope_lookup_cell_nonlocal(NvScope *scope, const char *name);
NvValue *nv_scope_slot_ptr_global(NvScope *scope, const char *name, int line, int col);
NvValue *nv_scope_slot_ptr_nonlocal(NvScope *scope, const char *name, int line, int col);
// فحص التجميد لتعريف دالة — إعادة تعريف اسم مجمد
// مرفوضة كإعادة الإسناد.
void nv_check_not_frozen(const char *name, int line, int col);
// نسخ نطاق (قيم منسوخة عميقا) — لاستدعاء دالة وحدة.
NvScope *nv_scope_clone(NvScope *scope);
// نطاق الوحدة: استنساخ جديد كل استدعاء.
NvScope *nv_module_call_scope(NvFunc *f);

// ---------- العمليات ----------
// عمليات ثنائية وأحادية برسائل المحرك نفسها.
NvValue nv_add(NvValue l, NvValue r, int line, int col);
NvValue nv_sub(NvValue l, NvValue r, int line, int col);
NvValue nv_mul(NvValue l, NvValue r, int line, int col);
NvValue nv_div(NvValue l, NvValue r, int line, int col);
NvValue nv_mod(NvValue l, NvValue r, int line, int col);
int nv_eq(NvValue l, NvValue r);
int nv_lt(NvValue l, NvValue r, int line, int col);
int nv_gt(NvValue l, NvValue r, int line, int col);
int nv_le(NvValue l, NvValue r, int line, int col);
int nv_ge(NvValue l, NvValue r, int line, int col);
int nv_is_in(NvValue item, NvValue coll, int line, int col);
NvValue nv_not(NvValue v, int line, int col);
NvValue nv_neg(NvValue v, int line, int col);
// شرط منطقي (إذا/بينما/و/أو) برسالة المحرك.
int nv_check_bool(NvValue v, const char *ctx, int line, int col);
int nv_check_bool_op(NvValue v, const char *word, int line, int col);

// ---------- الفهرسة والنقطة ----------
// قراءة فهرس (قائمة/قاموس/نص).
NvValue nv_index(NvValue target, NvValue index, int line, int col, int idx_line, int idx_col);
// تعيين فهرس (على متغير مباشرة — لا تعيين معقد).
void nv_index_assign(NvValue *slot, NvValue index, NvValue val,
                     int idx_line, int idx_col, int tgt_line, int tgt_col);
// وصول بالنقطة (وحدة/كائن/طرق نص وقائمة).
NvValue nv_dot(NvValue target, const char *name, int line, int col);
// تعيين حقل (كائن فقط).
void nv_dot_assign(NvValue target, const char *name, NvValue val, int line, int col);

// ---------- الاستدعاء ----------
// استدعاء أي قيمة قابلة للاستدعاء (دالة/أصلية/مرتبطة/فئة؟
// الفئة ليست قابلة للاستدعاء المباشر — خطأ).
NvValue nv_call(NvValue callee, NvValue *args, int nargs, int line, int col);
// الاستدعاء بموضعين: (line,col) لأخطاء الاستدعاء
// (عدد/نوع)، و(callee_line,callee_col) لخطأ «ليست
// دالة»/«استخدم أنشئ» — كما في المفسر والآلة.
NvValue nv_call_at(NvValue callee, NvValue *args, int nargs, int line, int col,
                   int callee_line, int callee_col);
// البنود 109–112: استدعاء عام مفتاحي.
NvValue nv_call_at_kw(NvValue callee, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                   int callee_line, int callee_col);
NvValue nv_call_kw(NvValue callee, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);
// إنشاء كائن بالاسم: أنشئ فئة(وسائط) — البحث بالاسم
// أولا برسائل المفسر نفسها.
NvValue nv_create_by_name(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col);
NvValue nv_create_by_name_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);
// الدفعة الأخيرة DEF (البنود 77 و78): إنشاء بالاسم
// مع بحث موجَّه بالإعلان.
NvValue nv_create_by_name_global(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col);
NvValue nv_create_by_name_nonlocal(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col);
NvValue nv_create_by_name_global_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);
NvValue nv_create_by_name_nonlocal_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);
// إنشاء كائن من قيمة فئة (للمسارات الداخلية).
NvValue nv_create(NvValue class_val, NvValue *args, int nargs, int line, int col);
// البنود 109–112: إنشاء مفتاحي.
NvValue nv_create_kw(NvValue class_val, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);
// فحص نوع معلن (تعيين/معامل/إرجاع) برسائل المحرك.
void nv_check_type(NvValue v, const char *type_name, const char *what,
                   const char *func_name, int line, int col);

// ---------- الطباعة والإدخال ----------
// اطبع: وسائط مفصولة بمسافة + سطر جديد (إلى stdout).
void nv_print_args(NvValue *args, int nargs);
// ادخل: سؤال اختياري + سطر من stdin (EOF = نص فارغ).
NvValue nv_input(NvValue *args, int nargs, int line, int col);

// ---------- الدوال المدمجة المسطحة ----------
// طول، مدى، مجموع، أدنى، أقصى، مطلق، عدد، نص، نوع،
// + ملفات مسطحة + ادخل + تثبيت. تعيد 1 إن عالجت الاسم.
int nv_is_builtin(const char *name);
NvValue nv_call_builtin(const char *name, NvValue *args, int nargs, int line, int col);
// طرق النص والقائمة (طول، كبير، ...، أضف، أزل).
NvValue nv_call_str_method(NvValue str, const char *name, NvValue *args, int nargs, int line, int col);
NvValue nv_call_list_method(NvValue *slot, const char *name, NvValue *args, int nargs, int line, int col);
NvValue nv_call_dot(NvValue target, const char *name, NvValue *args, int nargs, int line, int col);
NvValue nv_call_dot_at(NvValue target, const char *name, NvValue *args, int nargs, int line, int col,
                       int callee_line, int callee_col);
// البنود 109–112: استدعاء بالنقطة مفتاحي.
NvValue nv_call_dot_at_kw(NvValue target, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                       int callee_line, int callee_col);
NvValue nv_call_dot_kw(NvValue target, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col);

// ---------- الوحدات الأصلية ----------
// وحدة أصلية بالاسم أو NULL.
NvValue *nv_native_module(const char *name);
// استدعاء دالة وحدة أصلية بالاسم الكامل (وحدة.دالة).
NvValue nv_call_native(const char *module, const char *name, NvValue *args, int nargs, int line, int col);
// هل الاسم وحدة أصلية؟
int nv_is_native_module(const char *name);

// ---------- سياسة الملفات ----------
// حل مسار نسبي ضد مجلد البرنامج/الجلسة + فحص السياسة.
// تعيد مسارا مطلقا مملوكا (free) أو NULL مع خطأ مطلق.
char *nv_resolve_path(const char *path, int line, int col);
// حل مسار برنامج فرعي `.نيفا` نسبي: مجلد البرنامج
// أولا ثم cwd — مرآة `شغّل` (اجتماع 49 §63).
char *nv_resolve_subprogram(const char *program, int line, int col);
// عرض مسار كما يعرضه المحرك (‎~ للبيت، شرطات مائلة).
char *nv_display_path(const char *path); // free
// هل المسار داخل مجلد البرنامج؟
int nv_path_inside_program(const char *abs_path);
// فحص حجم الملف (5 ميغابايت) والحصة (100 ميغابايت).
void nv_check_file_size(const char *abs_path, int line, int col);
void nv_check_quota(size_t add_bytes, int line, int col);
// بايتات الحصة المتبقية (100MB ناقص مستخدم مجلد البرنامج).
size_t nv_quota_remaining(void);

// ---------- أدوات ----------
// تحويل أرقام غربية في نص إلى عربية مشرقية (للعرض فقط).
char *nv_arabic_digits_dup(const char *s);
// قراءة ملف كاملا إلى بايتات (أو NULL + errno).
unsigned char *nv_read_file_bytes(const char *abs_path, size_t *out_len);
// كتابة ملف كاملا (ذريا: مؤقت ثم إعادة تسمية).
int nv_write_file_atomic(const char *abs_path, const unsigned char *data, size_t len);
// اسم مؤقت فريد بجانب مسار.
char *nv_temp_sibling(const char *abs_path);

// ---------- الخادم والشبكة (خلفية C: HTTP عادي فقط) ----------
// اجلب/أرسل/نزّل/اخدم عبر HTTP/1.1 بمقابس POSIX؛
// HTTPS يُرفض برسالة عربية (يحتاج قرار TLS منفصلا).
NvValue nv_net_fetch(NvValue *args, int nargs, int line, int col);
NvValue nv_net_post(NvValue *args, int nargs, int line, int col);
NvValue nv_net_download(NvValue *args, int nargs, int line, int col);
NvValue nv_net_serve(NvValue handler, NvValue *args, int nargs, int line, int col);

// ---------- نقطة دخول البرنامج المولد ----------
// main المولد يستدعيها: تحليل أعلام التشغيل ثم تشغيل
// جسم البرنامج. تعيد رمز الخروج.
int nv_main(int argc, char **argv, void (*body)(NvScope *top));

#ifdef __cplusplus
}
#endif

#endif // NIVA_RT_H
