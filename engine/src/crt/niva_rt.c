//! وقت تشغيل خلفية C — الجزء 1: الأخطاء، جامع القمامة
//! التتبعي (v1.3 §6)، النطاقات، بناء القيم، التنسيق.
//! كل الرسائل عربية مطابقة للمحرك (interpreter.rs / vm.rs).

#include "niva_rt.h"
#include <stdarg.h>
// الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
// `ioctl(FICLONE)` للنسخ الخفيف على لينكس،
// و`_utime` لتحديث الوقت على ويندوز.
#ifndef _WIN32
#include <sys/ioctl.h>
#ifdef __linux__
#ifndef FICLONE
#define FICLONE 0x40049409
#endif
#endif
#endif
#ifdef _WIN32
#include <sys/utime.h>
#endif
#ifdef __APPLE__
#include <mach-o/dyld.h> // _NSGetExecutablePath — بلا هذا
// التضمين تبقى الدالة غير معلنة في clang الحديث
// (خطأ خامس مخفي على ماك خلف أول 30 سطرا من سجل cc).
#endif
#ifdef _WIN32
// على ويندوز لا توجد getline وrealpath وlstat وmemmem وsymlink،
// فحزمة التوافقيات الصغيرة أدناه تغطيها دون تغيير المنطق.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h> // CreateSymbolicLinkA وGetFinalPathNameByHandleA
#include <io.h> // _setmode — الوضع الثنائي للمخرجات (اجتماع 36 §51)
#endif

// ---------- الحالة العامة ----------
NvFlow nv_flow = NV_FLOW_NORMAL;
NvValue nv_retval;
NvHandler *nv_handlers = NULL;

char *nv_program_dir = NULL;
char *nv_home_dir = NULL;
char *nv_cwd = NULL;
int nv_trusted = 0;
// اجتماع 56 §71 — الدفعة 2 (التنقل): مكدس المجلدات
// (الأول = الأحدث المدفوع)، وقائمة بحث CDPATH، وراية
// الوضع الفيزيائي العام، ونطاق الاستدعاء الحالي
// (لقراءة قيمة متغير في `انتقل` — الميزة 16).
// يُضبط في nv_call_ident_at قبل كل مدمجة ويُستعاد
// بعدها؛ وبعد قفزة خطأ يبقى قديما حتى أول استدعاء
// تال يضبطه من جديد — ولا مدمجة تُستدعى إلا عبر
// nv_call_ident_at، فلا قراءة من نطاق قديم أبدا.
static char **nv_dirstack = NULL;
static size_t nv_dirstack_len = 0, nv_dirstack_cap = 0;
static char **nv_cdpath = NULL;
static size_t nv_cdpath_len = 0, nv_cdpath_cap = 0;
static int nv_physical_mode = 0;
static NvScope *nv_current_call_scope = NULL;
// الدفعة 4 — المتغيرات (اجتماع 58 §73): أسماء
// المتغيرات المجمدة نهائيا (بلا فك)، وجدول
// المراجع الاسمية (اسم ← هدف)، وبدء العملية
// بالثواني (لـ`بيئة("ثواني")`).
static char **nv_frozen = NULL;
static size_t nv_frozen_len = 0, nv_frozen_cap = 0;
static char **nv_nameref_from = NULL;
static char **nv_nameref_to = NULL;
static size_t nv_nameref_len = 0, nv_nameref_cap = 0;
static unsigned long long nv_start_secs = 0;
int nv_network_allowed = 0;
int nv_server_allowed = 0;
char **nv_local_hosts = NULL;
int nv_n_local_hosts = 0;
int nv_call_depth = 0;
unsigned long long nv_alloc_bytes = 0;
// الدفعة 1 — رموز الخروج (اجتماع 54 §69): رمز آخر
// أمر مكتمل (0 أولا؛ و1 بعد خطأ ممسوك بـ`ماعدا`)،
// وطلب خروج معلق برمزه، واسم البرنامج كما استُدعي.
int nv_last_exit_code = 0;
int nv_exit_requested = 0;
int nv_exit_code = 0;
char *nv_program_name = NULL;

// حد عمق الاستدعاء (interpreter.rs: 500 أصليا).
#define NV_MAX_CALL_DEPTH 500
// حد تكرار الحلقة (interpreter.rs: 1٬000٬000).
#define NV_MAX_LOOP_ITERATIONS 1000000
// عتبات جامع القمامة: جمع صغير كل 256 كيلوبايت من
// التخصيصات الصغيرة؛ وجمع كبير كل 8 ميغابايت إجمالا.
#define NV_MINOR_THRESHOLD (256 * 1024)
#define NV_MAJOR_THRESHOLD (8 * 1024 * 1024)

// ---------- أسماء أنواع الأخطاء ----------
const char *nv_err_kind_ar(NvErrKind k) {
    switch (k) {
        case NV_ERR_LEX: return "لغوي";
        case NV_ERR_PARSE: return "نحوي";
        case NV_ERR_TYPE: return "نوع";
        case NV_ERR_RUNTIME: return "وقت التشغيل";
        case NV_ERR_IO: return "إدخال/إخراج";
        case NV_ERR_SEMANTIC: return "دلالي";
    }
    return "وقت التشغيل";
}

// ---------- جامع القمامة ----------
// قائمة كل الكائنات + المجموعة المتذكرة + مكدس الجذور.
static NvObj *nv_all_objects = NULL;
static NvObj **nv_remembered = NULL;
static size_t nv_remembered_len = 0, nv_remembered_cap = 0;
static unsigned long long nv_since_minor = 0;
static unsigned long long nv_since_major = 0;

// مكدس جذور مؤقتة (قيم NvValue كاملة).
static NvValue *nv_root_stack = NULL;
static size_t nv_root_len = 0, nv_root_cap = 0;

// مكدس جذور النطاقات: النطاق ليس قيمة فلا يدخل
// مكدس القيم؛ وهذا المكدس يمسك نطاقا أثناء بنائه
// (نسخ نطاق) حتى لا تكنسه الجامعة منتصف البناء.
static NvScope **nv_scope_roots = NULL;
static size_t nv_scope_roots_len = 0, nv_scope_roots_cap = 0;

static size_t nv_scope_root_push(NvScope *s) {
    if (nv_scope_roots_len == nv_scope_roots_cap) {
        nv_scope_roots_cap = nv_scope_roots_cap ? nv_scope_roots_cap * 2 : 16;
        nv_scope_roots = (NvScope **)realloc(nv_scope_roots, nv_scope_roots_cap * sizeof(NvScope *));
    }
    size_t idx = nv_scope_roots_len++;
    nv_scope_roots[idx] = s;
    return idx;
}

static void nv_scope_root_pop_n(int n) {
    if (n <= 0) return;
    if ((size_t)n > nv_scope_roots_len) nv_scope_roots_len = 0;
    else nv_scope_roots_len -= (size_t)n;
}

// خطاف تنظيف المقابض (بلا خطاف افتراضيا).
void (*nv_handle_finalize)(int kind, void *ptr) = NULL;
void nv_handle_set_finalizer(void (*fn)(int kind, void *ptr)) {
    nv_handle_finalize = fn;
}

// جذور عامة دائمة (وحدات أصلية، ...).
static NvValue *nv_global_roots = NULL;
static size_t nv_global_roots_len = 0, nv_global_roots_cap = 0;
// جذور بالمؤشر (قيم ساكنة).
static NvValue **nv_global_root_ptrs = NULL;
static size_t nv_global_root_ptrs_len = 0, nv_global_root_ptrs_cap = 0;

void nv_add_global_root(NvValue v) {
    if (nv_global_roots_len == nv_global_roots_cap) {
        nv_global_roots_cap = nv_global_roots_cap ? nv_global_roots_cap * 2 : 16;
        nv_global_roots = (NvValue *)realloc(nv_global_roots, nv_global_roots_cap * sizeof(NvValue));
    }
    nv_global_roots[nv_global_roots_len++] = v;
}

void nv_add_global_root_ptr(NvValue *slot) {
    for (size_t i = 0; i < nv_global_root_ptrs_len; i++) {
        if (nv_global_root_ptrs[i] == slot) return;
    }
    if (nv_global_root_ptrs_len == nv_global_root_ptrs_cap) {
        nv_global_root_ptrs_cap = nv_global_root_ptrs_cap ? nv_global_root_ptrs_cap * 2 : 16;
        nv_global_root_ptrs = (NvValue **)realloc(nv_global_root_ptrs, nv_global_root_ptrs_cap * sizeof(NvValue *));
    }
    nv_global_root_ptrs[nv_global_root_ptrs_len++] = slot;
}
typedef struct NvFrameLink {
    NvScope *scope;
    struct NvFrameLink *prev;
} NvFrameLink;
static NvFrameLink *nv_frames = NULL;

void nv_frames_push(NvScope *s) {
    NvFrameLink *l = (NvFrameLink *)malloc(sizeof(NvFrameLink));
    l->scope = s;
    l->prev = nv_frames;
    nv_frames = l;
}

void nv_frames_pop(void) {
    NvFrameLink *l = nv_frames;
    if (l) {
        nv_frames = l->prev;
        free(l);
    }
}

// عدد الإطارات الحية (لنقاط التفتيش).
static int nv_frames_depth(void) {
    int n = 0;
    for (NvFrameLink *f = nv_frames; f; f = f->prev) n++;
    return n;
}

NvCheckpoint nv_checkpoint(void) {
    NvCheckpoint cp;
    cp.roots = nv_root_len;
    cp.frames = nv_frames_depth();
    cp.depth = nv_call_depth;
    return cp;
}

void nv_restore_checkpoint(NvCheckpoint cp) {
    nv_root_len = cp.roots;
    while (nv_frames_depth() > cp.frames) nv_frames_pop();
    nv_call_depth = cp.depth;
}

static NvObj *nv_obj_of(NvValue v) {
    switch (v.tag) {
        case NV_STR: return (NvObj *)v.u.str;
        case NV_LIST: return (NvObj *)v.u.list;
        case NV_DICT: return (NvObj *)v.u.dict;
        case NV_TUPLE: return (NvObj *)v.u.tuple;
        case NV_SET: return (NvObj *)v.u.set;
        case NV_FUNC: return (NvObj *)v.u.func;
        case NV_NATIVE: return (NvObj *)v.u.native;
        case NV_MODULE: return (NvObj *)v.u.module;
        case NV_CLASS: return (NvObj *)v.u.klass;
        case NV_IFACE: return (NvObj *)v.u.iface;
        case NV_INST: return (NvObj *)v.u.inst;
        case NV_BOUND: return (NvObj *)v.u.bound;
        case NV_HANDLE: return (NvObj *)v.u.handle;
        case NV_BYTES: return (NvObj *)v.u.bytes;
        default: return NULL;
    }
}

// وسم كائن حيا (mark) — يتبع مراجعه.
static void nv_mark_obj(NvObj *o);
// البنود 145–147: تحرير مصفوفة نصوص (تعريفها لاحقا).
static void nv_free_str_array(char **arr, int n);
static void nv_mark_value(NvValue v) {
    if (v.tag == NV_NUM || v.tag == NV_BOOL || v.tag == NV_NIL || v.tag == NV_COMPLEX) return;
    NvObj *o = NULL;
    switch (v.tag) {
        case NV_STR: o = (NvObj *)v.u.str; break;
        case NV_LIST: o = (NvObj *)v.u.list; break;
        case NV_DICT: o = (NvObj *)v.u.dict; break;
        case NV_TUPLE: o = (NvObj *)v.u.tuple; break;
        case NV_SET: o = (NvObj *)v.u.set; break;
        case NV_FUNC: o = (NvObj *)v.u.func; break;
        case NV_NATIVE: o = (NvObj *)v.u.native; break;
        case NV_MODULE: o = (NvObj *)v.u.module; break;
        case NV_CLASS: o = (NvObj *)v.u.klass; break;
        case NV_IFACE: o = (NvObj *)v.u.iface; break;
        case NV_INST: o = (NvObj *)v.u.inst; break;
        case NV_BOUND: o = (NvObj *)v.u.bound; break;
        case NV_HANDLE: o = (NvObj *)v.u.handle; break;
        case NV_BYTES: o = (NvObj *)v.u.bytes; break;
        // ج6: الكبير والكسر والبت والمعلب كائنات جامع.
        case NV_BIGINT: o = (NvObj *)v.u.bigint; break;
        case NV_RATIONAL: o = (NvObj *)v.u.rational; break;
        case NV_BITS: o = (NvObj *)v.u.bits; break;
        case NV_BOXED: o = (NvObj *)v.u.boxed; break;
        default: return;
    }
    nv_mark_obj(o);
}

static void nv_mark_scope(NvScope *s) {
    if (!s) return;
    nv_mark_obj((NvObj *)s);
}

static void nv_mark_obj(NvObj *o) {
    if (!o || o->marked) return;
    o->marked = 1;
    switch (o->kind) {
        case NV_OBJ_STR:
        case NV_OBJ_BYTES:
        case NV_OBJ_BIGINT:
        case NV_OBJ_BITS:
            break;
        case NV_OBJ_RATIONAL: {
            NvRational *r = (NvRational *)o;
            nv_mark_obj((NvObj *)r->num);
            nv_mark_obj((NvObj *)r->den);
            break;
        }
        case NV_OBJ_BOXED: {
            NvBoxed *b = (NvBoxed *)o;
            nv_mark_value(b->inner);
            break;
        }
        case NV_OBJ_LIST: {
            NvList *l = (NvList *)o;
            for (size_t i = 0; i < l->len; i++) nv_mark_value(l->items[i]);
            break;
        }
        case NV_OBJ_DICT: {
            NvDict *d = (NvDict *)o;
            for (size_t i = 0; i < d->len; i++) nv_mark_value(d->entries[i].val);
            break;
        }
        case NV_OBJ_FUNC: {
            NvFunc *f = (NvFunc *)o;
            nv_mark_scope(f->captured);
            nv_mark_scope(f->module_scope);
            // البنود 109–112: القيم الافتراضية قيم
            // حية يملكها التعريف — تُعلَّم كغيرها.
            if (f->defaults) {
                for (int i = 0; i < f->nparams; i++) {
                    nv_mark_value(f->defaults[i]);
                }
            }
            break;
        }
        case NV_OBJ_MODULE: {
            NvModule *m = (NvModule *)o;
            if (m->exports) nv_mark_obj((NvObj *)m->exports);
            nv_mark_scope(m->scope);
            break;
        }
        case NV_OBJ_CLASS: {
            NvClass *c = (NvClass *)o;
            if (c->methods) nv_mark_obj((NvObj *)c->methods);
            if (c->base) nv_mark_obj((NvObj *)c->base);
            break;
        }
        case NV_OBJ_IFACE:
            break;
        case NV_OBJ_INST: {
            NvInst *in = (NvInst *)o;
            if (in->klass) nv_mark_obj((NvObj *)in->klass);
            if (in->fields) nv_mark_obj((NvObj *)in->fields);
            break;
        }
        case NV_OBJ_BOUND: {
            NvBound *b = (NvBound *)o;
            nv_mark_value(b->func);
            if (b->inst) nv_mark_obj((NvObj *)b->inst);
            break;
        }
        case NV_OBJ_SCOPE: {
            NvScope *s = (NvScope *)o;
            nv_mark_scope(s->parent);
            for (NvBinding *bd = s->bindings; bd; bd = bd->next) {
                if (bd->cell) nv_mark_obj((NvObj *)bd->cell);
            }
            break;
        }
        case NV_OBJ_CELL: {
            NvCell *c = (NvCell *)o;
            nv_mark_value(c->val);
            break;
        }
        case NV_OBJ_NATIVE:
        case NV_OBJ_HANDLE:
            break;
        default:
            break;
    }
}

// وسم كل الجذور: مكدس الجذور + الإطارات الحية +
// المجموعة المتذكرة (للجمع الصغير فقط).
static void nv_mark_roots(int minor_only) {
    for (size_t i = 0; i < nv_root_len; i++) nv_mark_value(nv_root_stack[i]);
    for (size_t i = 0; i < nv_global_roots_len; i++) nv_mark_value(nv_global_roots[i]);
    for (size_t i = 0; i < nv_global_root_ptrs_len; i++) nv_mark_value(*nv_global_root_ptrs[i]);
    for (NvFrameLink *f = nv_frames; f; f = f->prev) nv_mark_scope(f->scope);
    for (size_t i = 0; i < nv_scope_roots_len; i++) nv_mark_scope(nv_scope_roots[i]);
    if (minor_only) {
        for (size_t i = 0; i < nv_remembered_len; i++) nv_mark_obj(nv_remembered[i]);
    }
}

// كنس: تحرير غير الموسوم (صغير فقط أو الكل).
static void nv_sweep(int minor_only) {
    NvObj **link = &nv_all_objects;
    while (*link) {
        NvObj *o = *link;
        int is_young = (o->gen == 0);
        if (!o->marked && (!minor_only || is_young)) {
            *link = o->next;
            // تحرير الموارد الجانبية قبل تحرير الكائن.
            if (o->kind == NV_OBJ_DICT) {
                NvDict *d = (NvDict *)o;
                for (size_t i = 0; i < d->len; i++) free(d->entries[i].key);
                free(d->entries);
            } else if (o->kind == NV_OBJ_LIST) {
                NvList *l = (NvList *)o;
                free(l->items);
            } else if (o->kind == NV_OBJ_SCOPE) {
                NvScope *s = (NvScope *)o;
                NvBinding *b = s->bindings;
                while (b) {
                    NvBinding *nx = b->next;
                    free(b->name);
                    free(b);
                    b = nx;
                }
            } else if (o->kind == NV_OBJ_FUNC) {
                NvFunc *f = (NvFunc *)o;
                free(f->name);
                free(f->module);
                if (f->param_types) {
                    for (int i = 0; i < f->nparams; i++) free(f->param_types[i]);
                    free(f->param_types);
                }
                free(f->return_type);
                // البنود 145–147: جداول معاملات النوع والربط.
                nv_free_str_array(f->tp_names, f->ntype_params);
                nv_free_str_array(f->tp_bounds, f->ntype_params);
                nv_free_str_array(f->tp_defaults, f->ntype_params);
                nv_free_str_array(f->type_binding, f->ntype_params);
            } else if (o->kind == NV_OBJ_MODULE) {
                free(((NvModule *)o)->name);
            } else if (o->kind == NV_OBJ_CLASS) {
                NvClass *c = (NvClass *)o;
                free(c->name);
                for (int i = 0; i < c->ninterfaces; i++) free(c->interfaces[i]);
                free(c->interfaces);
                // البنود 145–147: جداول معاملات النوع والربط.
                nv_free_str_array(c->tp_names, c->ntype_params);
                nv_free_str_array(c->tp_bounds, c->ntype_params);
                nv_free_str_array(c->tp_defaults, c->ntype_params);
                nv_free_str_array(c->type_binding, c->ntype_params);
            } else if (o->kind == NV_OBJ_INST) {
                // البنود 145–147: ربط النوع على الكائن.
                NvInst *in = (NvInst *)o;
                if (in->klass) nv_free_str_array(in->type_binding, in->klass->ntype_params);
            } else if (o->kind == NV_OBJ_IFACE) {
                NvIface *in = (NvIface *)o;
                free(in->name);
                for (int i = 0; i < in->nmethods; i++) free(in->methods[i]);
                free(in->methods);
                free(in->arity);
            } else if (o->kind == NV_OBJ_NATIVE) {
                free(((NvNative *)o)->name);
            } else if (o->kind == NV_OBJ_HANDLE) {
                NvHandle *h = (NvHandle *)o;
                if (nv_handle_finalize) nv_handle_finalize(h->kind, h->ptr);
                free(h->ptr);
            }
            free(o);
        } else {
            // الباقي: أزل العلامة؛ والصغير الناجي يترقى
            // إلى الكبير بعد جمع كبير.
            o->marked = 0;
            if (!minor_only) o->gen = 1;
            link = &o->next;
        }
    }
    if (!minor_only) {
        // الجمع الكبير يمسح المجموعة المتذكرة.
        nv_remembered_len = 0;
    }
}

void nv_collect_minor(void) {
    nv_mark_roots(1);
    nv_sweep(1);
    nv_since_minor = 0;
}

void nv_collect_major(void) {
    nv_mark_roots(0);
    nv_sweep(0);
    nv_since_major = 0;
    nv_since_minor = 0;
}

// نقطة أمان (v1.3 §6.3): تُستدعى عند التخصيص وفي
// العمليات الأصلية الطويلة؛ والجمع الفعلي يجري عند
// التخصيص، فهنا فحص العتبة فقط.
void nv_safepoint(void) {
    if (nv_since_minor >= NV_MINOR_THRESHOLD) nv_collect_minor();
    else if (nv_since_major >= NV_MAJOR_THRESHOLD) nv_collect_major();
}

void *nv_alloc(size_t size, uint8_t kind) {
    nv_safepoint();
    NvObj *o = (NvObj *)calloc(1, size);
    if (!o) {
        fprintf(stderr, "خطأ وقت التشغيل: نفدت الذاكرة (السطر ٠، العمود ٠)\n");
        exit(1);
    }
    o->marked = 0;
    o->gen = 0;
    o->kind = kind;
    o->next = nv_all_objects;
    nv_all_objects = o;
    nv_alloc_bytes += size;
    nv_since_minor += size;
    nv_since_major += size;
    return o;
}

void nv_write_barrier(NvObj *old_obj, NvObj *young_obj) {
    if (!old_obj || !young_obj) return;
    if (old_obj->gen == 1 && young_obj->gen == 0) {
        // كبير يشير إلى صغير: سجّله في المتذكرة.
        for (size_t i = 0; i < nv_remembered_len; i++) {
            if (nv_remembered[i] == old_obj) return;
        }
        if (nv_remembered_len == nv_remembered_cap) {
            nv_remembered_cap = nv_remembered_cap ? nv_remembered_cap * 2 : 64;
            nv_remembered = (NvObj **)realloc(nv_remembered, nv_remembered_cap * sizeof(NvObj *));
        }
        nv_remembered[nv_remembered_len++] = old_obj;
    }
}

NvValue *nv_root_push(void) {
    if (nv_root_len == nv_root_cap) {
        nv_root_cap = nv_root_cap ? nv_root_cap * 2 : 256;
        nv_root_stack = (NvValue *)realloc(nv_root_stack, nv_root_cap * sizeof(NvValue));
    }
    NvValue *slot = &nv_root_stack[nv_root_len++];
    slot->tag = NV_NIL;
    return slot;
}

void nv_root_pop(void) {
    if (nv_root_len > 0) nv_root_len--;
}

void nv_root_pop_n(int n) {
    if (n <= 0) return;
    if ((size_t)n > nv_root_len) nv_root_len = 0;
    else nv_root_len -= (size_t)n;
}

// جذور بالفهرس (آمنة عبر realloc للمكدس).
size_t nv_root_push_idx(void) {
    if (nv_root_len == nv_root_cap) {
        nv_root_cap = nv_root_cap ? nv_root_cap * 2 : 256;
        nv_root_stack = (NvValue *)realloc(nv_root_stack, nv_root_cap * sizeof(NvValue));
    }
    size_t idx = nv_root_len++;
    nv_root_stack[idx].tag = NV_NIL;
    return idx;
}

NvValue *nv_root_slot(size_t idx) {
    if (idx >= nv_root_len) return NULL;
    return &nv_root_stack[idx];
}

// ---------- الأخطاء ----------
static NvError nv_last_error;
static int nv_has_error = 0;

// ملف التنفيذ الجاري (فهرس في جدول nv_source_files
// الذي يولّده cgen — [0] الملف الرئيسي). يضبطه الكود
// المولّد عند دخول كل دالة وعند كل جملة؛ ويلتقطه
// nv_error في الخطأ، فيسمّى الملف كما في المفسر
// (اجتماع 33: الحكم الخامس يمتد إلى الأصلي حرفيا).
int nv_cur_file_idx = 0;

// اطبع خطأ نيفا بالصيغة نفسها حرفيا كما في المفسر:
// «خطأ {نوع}: {رسالة} (الملف {ملف}، السطر {س}، العمود {ع})»
// حين يكون الملف معلوما، وإلا الصيغة القديمة بلا ملف.
static void nv_print_error(NvErrKind kind, const char *message, int line, int col, int file_idx) {
    if (file_idx >= 0 && file_idx < nv_n_source_files && nv_source_files[file_idx]) {
        fprintf(stderr, "خطأ %s: %s (الملف %s، السطر %d، العمود %d)\n",
                nv_err_kind_ar(kind), message, nv_source_files[file_idx], line, col);
    } else {
        fprintf(stderr, "خطأ %s: %s (السطر %d، العمود %d)\n",
                nv_err_kind_ar(kind), message, line, col);
    }
}

NvError *nv_current_error(void) {
    return nv_has_error ? &nv_last_error : NULL;
}

void nv_clear_error(void) {
    nv_has_error = 0;
    free(nv_last_error.message);
    nv_last_error.message = NULL;
}

static char *nv_strdup(const char *s) {
    if (!s) return NULL;
    size_t n = strlen(s) + 1;
    char *d = (char *)malloc(n);
    memcpy(d, s, n);
    return d;
}

void nv_error(NvErrKind kind, const char *message, int line, int col) {
    nv_clear_error();
    nv_last_error.kind = kind;
    nv_last_error.message = nv_strdup(message);
    nv_last_error.line = line;
    nv_last_error.col = col;
    nv_last_error.file_idx = nv_cur_file_idx;
    nv_has_error = 1;
    if (nv_handlers) {
        NvHandler *h = nv_handlers;
        nv_handlers = h->prev;
        longjmp(h->jb, 1);
    }
    // بلا معالج: اطبع واخرج برمز 1 كما يفعل CLI.
    nv_print_error(kind, message, line, col, nv_last_error.file_idx);
    exit(1);
}

// أعد رفع الخطأ الحالي (بعد أخيرا) إلى المعالج الأعمق.
void nv_reraise_current(void) {
    // الدفعة 1 — رموز الخروج (اجتماع 54 §69): طلب
    // خروج معلق يصعد كما هو — أخيرا الجارية انتهت،
    // والخروج يواصل إلى المعالج الأعمق (لا ماعدا
    // في طريقه أبدا).
    if (nv_exit_requested) {
        if (nv_handlers) {
            NvHandler *h = nv_handlers;
            nv_handlers = h->prev;
            longjmp(h->jb, 1);
        }
        exit(nv_exit_code);
    }
    if (!nv_has_error) {
        // بلا خطأ معلق: لا شيء يُرفع — عودة صامتة.
        // (تُستدعى فقط بعد أخيرا بلا معالج أعمق.)
        return;
    }
    NvError e = nv_last_error;
    if (nv_handlers) {
        NvHandler *h = nv_handlers;
        nv_handlers = h->prev;
        longjmp(h->jb, 1);
    }
    nv_print_error(e.kind, e.message, e.line, e.col, e.file_idx);
    exit(1);
}

// الدفعة 1 — رموز الخروج (اجتماع 54 §69): اقفز
// بطلب الخروج إلى أعمق معالج، بلا ضبط خطأ —
// فمسار الخطأ في حاول يميّز الخروج فيشغّل أخيرا
// وحدها (بلا ماعدا) ثم يعيد رفع الطلب.
void nv_exit_jump(void) {
    if (nv_handlers) {
        NvHandler *h = nv_handlers;
        nv_handlers = h->prev;
        longjmp(h->jb, 1);
    }
    // بلا معالج (لا يحدث داخل الجسم — المعالج
    // العلوي حاضر دائما): اخرج بالرمز مباشرة.
    exit(nv_exit_code);
}

// أضف بادئة إلى رسالة الخطأ الحالي دون تغيير ملفه
// أو سطره أو عموده — «في وحدة X: » عند عبور حدود
// وحدة أثناء تحميلها، كما في المفسر (تُستدعى من
// غلاف استيراد مولّد قبل إعادة الرفع).
void nv_prefix_error(const char *prefix) {
    if (!nv_has_error || !nv_last_error.message || !prefix) return;
    size_t pl = strlen(prefix);
    size_t ml = strlen(nv_last_error.message);
    char *nm = (char *)malloc(pl + ml + 1);
    if (!nm) return;
    memcpy(nm, prefix, pl);
    memcpy(nm + pl, nv_last_error.message, ml + 1);
    free(nv_last_error.message);
    nv_last_error.message = nm;
}

void nv_error_fmt(NvErrKind kind, int line, int col, const char *fmt, ...) {
    char buf[2048];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    nv_error(kind, buf, line, col);
}

// ---------- بناء القيم ----------
NvValue nv_num(double n) {
    NvValue v;
    v.tag = NV_NUM;
    v.u.num = n;
    return v;
}

NvValue nv_bool(int b) {
    NvValue v;
    v.tag = NV_BOOL;
    v.u.boolean = b ? 1 : 0;
    return v;
}

NvValue nv_nil(void) {
    NvValue v;
    v.tag = NV_NIL;
    v.u.num = 0;
    return v;
}

static size_t nv_count_utf8(const char *s, size_t len) {
    size_t n = 0;
    for (size_t i = 0; i < len;) {
        unsigned char c = (unsigned char)s[i];
        size_t adv = 1;
        if ((c & 0x80) == 0) adv = 1;
        else if ((c & 0xE0) == 0xC0) adv = 2;
        else if ((c & 0xF0) == 0xE0) adv = 3;
        else if ((c & 0xF8) == 0xF0) adv = 4;
        if (i + adv > len) adv = 1;
        i += adv;
        n++;
    }
    return n;
}

NvValue nv_str_new(const char *s, size_t len) {
    NvStr *st = (NvStr *)nv_alloc(sizeof(NvStr) + len + 1, NV_OBJ_STR);
    st->len = len;
    st->chars = nv_count_utf8(s, len);
    memcpy(st->data, s, len);
    st->data[len] = '\0';
    NvValue v;
    v.tag = NV_STR;
    v.u.str = st;
    return v;
}

NvValue nv_str_take(char *s, size_t len) {
    NvValue v = nv_str_new(s, len);
    free(s);
    return v;
}

NvValue nv_str_cstr(const char *s) {
    return nv_str_new(s, strlen(s));
}

NvValue nv_list_new(void) {
    NvList *l = (NvList *)nv_alloc(sizeof(NvList), NV_OBJ_LIST);
    l->len = 0;
    l->cap = 0;
    l->items = NULL;
    NvValue v;
    v.tag = NV_LIST;
    v.u.list = l;
    return v;
}

NvValue nv_dict_new(void) {
    NvDict *d = (NvDict *)nv_alloc(sizeof(NvDict), NV_OBJ_DICT);
    d->len = 0;
    d->cap = 0;
    d->entries = NULL;
    NvValue v;
    v.tag = NV_DICT;
    v.u.dict = d;
    return v;
}

// البندان 138 و139 (الدفعة 3 من الـ579): مترابطة
// وتشكيلة — التخزين NvList نفسه (نوع الكائن
// NV_OBJ_LIST، فيتتبعه الجامع كالقائمة تماما).
NvValue nv_tuple_new(void) {
    NvList *l = (NvList *)nv_alloc(sizeof(NvList), NV_OBJ_LIST);
    l->len = 0;
    l->cap = 0;
    l->items = NULL;
    NvValue v;
    v.tag = NV_TUPLE;
    v.u.tuple = l;
    return v;
}

NvValue nv_set_new(void) {
    NvList *l = (NvList *)nv_alloc(sizeof(NvList), NV_OBJ_LIST);
    l->len = 0;
    l->cap = 0;
    l->items = NULL;
    NvValue v;
    v.tag = NV_SET;
    v.u.set = l;
    return v;
}

// إلحاق بتشكيلة بلا تكرار (أول ظهور يفوز) —
// مرآة Value::make_set في Rust.
static void nv_set_push_dedup(NvList *set, NvValue v) {
    for (size_t i = 0; i < set->len; i++) {
        if (nv_eq(v, set->items[i])) return;
    }
    nv_list_push_raw(set, v);
}

// ============ ج6: الأعداد الجديدة ============
// حساب عشري دقيق على سلاسل موحدة (مرآة value.rs في
// Rust): السلسلة `[+-]?أرقام` بلا أصفار بادئة، والصفر
// "0" بلا إشارة. الدوال التي تعيد char* تعيد سلسلة
// جديدة تُحرَّر بـ free، أو NULL عند إدخال غير صالح.
static void nv_fmt_f64_plain(double n, char *out, size_t outsz);

// ----- الكبير: سلاسل عشرية موحدة -----
static int nv_bigint_digits_ok(const char *s, int *neg, const char **dig, size_t *len) {
    if (!s || !*s) return 0;
    const char *p = s;
    int ng = 0;
    if (*p == '-' || *p == '+') { ng = (*p == '-'); p++; }
    if (!*p) return 0;
    size_t n = 0;
    for (const char *q = p; *q; q++) {
        if (*q < '0' || *q > '9') return 0;
        n++;
    }
    *neg = ng; *dig = p; *len = n;
    return 1;
}

char *nv_bigint_norm(const char *s) {
    int neg; const char *dig; size_t len;
    if (!nv_bigint_digits_ok(s, &neg, &dig, &len)) return NULL;
    while (len > 0 && *dig == '0') { dig++; len--; }
    if (len == 0) return nv_strdup("0");
    char *out = (char *)malloc(len + (neg ? 2 : 1));
    size_t i = 0;
    if (neg) out[i++] = '-';
    memcpy(out + i, dig, len);
    out[i + len] = '\0';
    return out;
}

int nv_bigint_is_zero_str(const char *a) {
    if (!a) return 1;
    if (*a == '-' || *a == '+') a++;
    while (*a) { if (*a != '0') return 0; a++; }
    return 1;
}

static int nv_bigint_mag_cmp(const char *a, const char *b) {
    // مقداران موحدان بلا إشارة ولا أصفار بادئة.
    size_t la = strlen(a), lb = strlen(b);
    if (la != lb) return la < lb ? -1 : 1;
    int c = strcmp(a, b);
    return c < 0 ? -1 : (c > 0 ? 1 : 0);
}

int nv_bigint_cmp_str(const char *a, const char *b) {
    int an = 0, bn = 0;
    const char *ad = a, *bd = b;
    if (*ad == '-') { an = 1; ad++; } else if (*ad == '+') ad++;
    if (*bd == '-') { bn = 1; bd++; } else if (*bd == '+') bd++;
    while (*ad == '0') ad++;
    while (*bd == '0') bd++;
    if (!*ad && !*bd) return 0;
    if (an != bn) return an ? -1 : 1;
    int m = nv_bigint_mag_cmp(*ad ? ad : "0", *bd ? bd : "0");
    return an ? -m : m;
}

char *nv_bigint_neg_str(const char *a) {
    char *n = nv_bigint_norm(a);
    if (!n) return NULL;
    if (strcmp(n, "0") == 0) return n;
    if (n[0] == '-') {
        char *m = nv_strdup(n + 1);
        free(n);
        return m;
    }
    size_t len = strlen(n);
    char *m = (char *)malloc(len + 2);
    m[0] = '-';
    memcpy(m + 1, n, len + 1);
    free(n);
    return m;
}

static char *nv_bigint_add_mag(const char *a, const char *b) {
    size_t la = strlen(a), lb = strlen(b);
    size_t n = la > lb ? la : lb;
    char *out = (char *)malloc(n + 2);
    int carry = 0;
    size_t w = 0;
    for (size_t i = 0; i < n; i++) {
        int da = i < la ? a[la - 1 - i] - '0' : 0;
        int db = i < lb ? b[lb - 1 - i] - '0' : 0;
        int s = da + db + carry;
        out[w++] = (char)('0' + (s % 10));
        carry = s / 10;
    }
    if (carry) out[w++] = (char)('0' + carry);
    out[w] = '\0';
    // عكس.
    for (size_t i = 0, j = w - 1; i < j; i++, j--) {
        char t = out[i]; out[i] = out[j]; out[j] = t;
    }
    return out;
}

static char *nv_bigint_sub_mag(const char *a, const char *b) {
    // a >= b مقداران موحدان.
    size_t la = strlen(a), lb = strlen(b);
    char *out = (char *)malloc(la + 1);
    int borrow = 0;
    size_t w = 0;
    for (size_t i = 0; i < la; i++) {
        int da = a[la - 1 - i] - '0' - borrow;
        int db = i < lb ? b[lb - 1 - i] - '0' : 0;
        if (da < db) { da += 10; borrow = 1; } else borrow = 0;
        out[w++] = (char)('0' + (da - db));
    }
    while (w > 1 && out[w - 1] == '0') w--;
    out[w] = '\0';
    for (size_t i = 0, j = w - 1; i < j; i++, j--) {
        char t = out[i]; out[i] = out[j]; out[j] = t;
    }
    return out;
}

char *nv_bigint_add_str(const char *a, const char *b) {
    char *na = nv_bigint_norm(a), *nb = nv_bigint_norm(b);
    if (!na || !nb) { free(na); free(nb); return NULL; }
    int an = na[0] == '-', bn = nb[0] == '-';
    const char *ad = an ? na + 1 : na, *bd = bn ? nb + 1 : nb;
    char *mag;
    int neg = 0;
    if (an == bn) {
        mag = nv_bigint_add_mag(ad, bd);
        neg = an;
    } else {
        int c = nv_bigint_mag_cmp(ad, bd);
        if (c == 0) { free(na); free(nb); return nv_strdup("0"); }
        if (c > 0) { mag = nv_bigint_sub_mag(ad, bd); neg = an; }
        else { mag = nv_bigint_sub_mag(bd, ad); neg = bn; }
    }
    free(na); free(nb);
    if (strcmp(mag, "0") == 0) return mag;
    if (!neg) return mag;
    size_t len = strlen(mag);
    char *out = (char *)malloc(len + 2);
    out[0] = '-';
    memcpy(out + 1, mag, len + 1);
    free(mag);
    return out;
}

char *nv_bigint_sub_str(const char *a, const char *b) {
    char *nb = nv_bigint_neg_str(b);
    if (!nb) return NULL;
    char *r = nv_bigint_add_str(a, nb);
    free(nb);
    return r;
}

char *nv_bigint_mul_str(const char *a, const char *b) {
    char *na = nv_bigint_norm(a), *nb = nv_bigint_norm(b);
    if (!na || !nb) { free(na); free(nb); return NULL; }
    int neg = (na[0] == '-') != (nb[0] == '-');
    const char *ad = na[0] == '-' ? na + 1 : na;
    const char *bd = nb[0] == '-' ? nb + 1 : nb;
    if (strcmp(ad, "0") == 0 || strcmp(bd, "0") == 0) {
        free(na); free(nb);
        return nv_strdup("0");
    }
    size_t la = strlen(ad), lb = strlen(bd);
    int *acc = (int *)calloc(la + lb, sizeof(int));
    for (size_t i = 0; i < la; i++) {
        int da = ad[la - 1 - i] - '0';
        for (size_t j = 0; j < lb; j++) {
            acc[i + j] += da * (bd[lb - 1 - j] - '0');
        }
    }
    size_t n = la + lb;
    char *out = (char *)malloc(n + 2);
    size_t w = 0;
    int carry = 0;
    for (size_t i = 0; i < n; i++) {
        int s = acc[i] + carry;
        out[w++] = (char)('0' + (s % 10));
        carry = s / 10;
    }
    free(acc);
    while (w > 1 && out[w - 1] == '0') w--;
    out[w] = '\0';
    for (size_t i = 0, j = w - 1; i < j; i++, j--) {
        char t = out[i]; out[i] = out[j]; out[j] = t;
    }
    free(na); free(nb);
    if (!neg) return out;
    size_t len = strlen(out);
    char *r = (char *)malloc(len + 2);
    r[0] = '-';
    memcpy(r + 1, out, len + 1);
    free(out);
    return r;
}

int nv_bigint_divmod_str(const char *a, const char *b, char **q, char **r) {
    *q = NULL; *r = NULL;
    char *na = nv_bigint_norm(a), *nb = nv_bigint_norm(b);
    if (!na || !nb) { free(na); free(nb); return 0; }
    if (nv_bigint_is_zero_str(nb)) { free(na); free(nb); return 0; }
    int an = na[0] == '-', bn = nb[0] == '-';
    const char *ad = an ? na + 1 : na, *bd = bn ? nb + 1 : nb;
    if (nv_bigint_mag_cmp(ad, bd) < 0) {
        *q = nv_strdup("0");
        *r = nv_strdup(na);
        free(na); free(nb);
        return 1;
    }
    // قسمة مطولة على المقادير.
    size_t la = strlen(ad);
    char *cur = nv_strdup("");
    char *quot = (char *)malloc(la + 1);
    size_t qw = 0;
    for (size_t i = 0; i < la; i++) {
        size_t cl = strlen(cur);
        char *nc = (char *)malloc(cl + 2);
        memcpy(nc, cur, cl);
        nc[cl] = ad[i];
        nc[cl + 1] = '\0';
        free(cur);
        cur = nc;
        while (strlen(cur) > 1 && cur[0] == '0') {
            memmove(cur, cur + 1, strlen(cur));
        }
        int d = 0;
        while (nv_bigint_mag_cmp(cur, bd) >= 0) {
            char *ns = nv_bigint_sub_mag(cur, bd);
            free(cur);
            cur = ns;
            d++;
        }
        quot[qw++] = (char)('0' + d);
    }
    quot[qw] = '\0';
    while (qw > 1 && quot[0] == '0') { memmove(quot, quot + 1, qw); qw--; }
    int qneg = an != bn;
    if (qneg && strcmp(quot, "0") != 0) {
        size_t len = strlen(quot);
        char *nq = (char *)malloc(len + 2);
        nq[0] = '-';
        memcpy(nq + 1, quot, len + 1);
        free(quot);
        quot = nq;
    }
    char *rem = nv_strdup(cur);
    if (an && strcmp(rem, "0") != 0) {
        size_t len = strlen(rem);
        char *nr = (char *)malloc(len + 2);
        nr[0] = '-';
        memcpy(nr + 1, rem, len + 1);
        free(rem);
        rem = nr;
    }
    free(cur);
    free(na); free(nb);
    *q = quot;
    *r = rem;
    return 1;
}

char *nv_bigint_gcd_str(const char *a, const char *b) {
    char *x = nv_bigint_norm(a), *y = nv_bigint_norm(b);
    if (!x || !y) { free(x); free(y); return NULL; }
    if (x[0] == '-') { char *t = nv_strdup(x + 1); free(x); x = t; }
    if (y[0] == '-') { char *t = nv_strdup(y + 1); free(y); y = t; }
    while (!nv_bigint_is_zero_str(y)) {
        char *q = NULL, *r = NULL;
        nv_bigint_divmod_str(x, y, &q, &r);
        free(q);
        free(x);
        x = y;
        y = r;
    }
    free(y);
    return x;
}

static char *nv_bigint_pow10_str(size_t e) {
    char *out = (char *)malloc(e + 2);
    out[0] = '1';
    for (size_t i = 0; i < e; i++) out[1 + i] = '0';
    out[e + 1] = '\0';
    return out;
}

double nv_bigint_to_double(NvValue v) {
    if (v.tag != NV_BIGINT) return 0.0;
    char *s = nv_bigint_norm(v.u.bigint->digits);
    double d = strtod(s, NULL);
    free(s);
    return d;
}

// ----- الكسور: أزواج موحدة (بسط/مقام، المقام موجب) -----
int nv_rat_norm_str(const char *num, const char *den, char **n, char **d) {
    *n = NULL; *d = NULL;
    char *nn = nv_bigint_norm(num), *dd = nv_bigint_norm(den);
    if (!nn || !dd) { free(nn); free(dd); return 0; }
    if (nv_bigint_is_zero_str(dd)) { free(nn); free(dd); return 0; }
    if (dd[0] == '-') {
        char *tn = nv_bigint_neg_str(nn), *td = nv_bigint_neg_str(dd);
        free(nn); free(dd);
        nn = tn; dd = td;
    }
    if (nv_bigint_is_zero_str(nn)) {
        free(nn); free(dd);
        *n = nv_strdup("0");
        *d = nv_strdup("1");
        return 1;
    }
    char *g = nv_bigint_gcd_str(nn, dd);
    char *q1 = NULL, *r1 = NULL, *q2 = NULL, *r2 = NULL;
    nv_bigint_divmod_str(nn, g, &q1, &r1);
    nv_bigint_divmod_str(dd, g, &q2, &r2);
    free(r1); free(r2); free(g); free(nn); free(dd);
    *n = q1;
    *d = q2;
    return 1;
}

void nv_rat_add_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d) {
    char *t1 = nv_bigint_mul_str(a1, b2);
    char *t2 = nv_bigint_mul_str(b1, a2);
    char *num = nv_bigint_add_str(t1, t2);
    char *den = nv_bigint_mul_str(a2, b2);
    free(t1); free(t2);
    nv_rat_norm_str(num, den, n, d);
    free(num); free(den);
}

void nv_rat_sub_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d) {
    char *t1 = nv_bigint_mul_str(a1, b2);
    char *t2 = nv_bigint_mul_str(b1, a2);
    char *num = nv_bigint_sub_str(t1, t2);
    char *den = nv_bigint_mul_str(a2, b2);
    free(t1); free(t2);
    nv_rat_norm_str(num, den, n, d);
    free(num); free(den);
}

void nv_rat_mul_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d) {
    char *num = nv_bigint_mul_str(a1, b1);
    char *den = nv_bigint_mul_str(a2, b2);
    nv_rat_norm_str(num, den, n, d);
    free(num); free(den);
}

int nv_rat_div_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d) {
    if (nv_bigint_is_zero_str(b1)) return 0;
    char *num = nv_bigint_mul_str(a1, b2);
    char *den = nv_bigint_mul_str(a2, b1);
    int ok = nv_rat_norm_str(num, den, n, d);
    free(num); free(den);
    return ok;
}

int nv_rat_rem_str(const char *a1, const char *a2, const char *b1, const char *b2, char **n, char **d) {
    if (nv_bigint_is_zero_str(b1)) return 0;
    // q = قطع((a1/a2)/(b1/b2)) = قطع(a1*b2 / a2*b1).
    char *num = nv_bigint_mul_str(a1, b2);
    char *den = nv_bigint_mul_str(a2, b1);
    char *q = NULL, *r = NULL;
    nv_bigint_divmod_str(num, den, &q, &r);
    free(r);
    // rem = a - q*b = (a1*b2 - q*b1*a2) / (a2*b2).
    char *t1 = nv_bigint_mul_str(a1, b2);
    char *t2 = nv_bigint_mul_str(q, b1);
    char *t3 = nv_bigint_mul_str(t2, a2);
    char *rn = nv_bigint_sub_str(t1, t3);
    char *rd = nv_bigint_mul_str(a2, b2);
    free(num); free(den); free(q); free(t1); free(t2); free(t3);
    int ok = nv_rat_norm_str(rn, rd, n, d);
    free(rn); free(rd);
    return ok;
}

int nv_rat_cmp_str(const char *a1, const char *a2, const char *b1, const char *b2) {
    char *l = nv_bigint_mul_str(a1, b2);
    char *r = nv_bigint_mul_str(b1, a2);
    int c = nv_bigint_cmp_str(l, r);
    free(l); free(r);
    return c;
}

double nv_rat_to_double_str(const char *num, const char *den) {
    return strtod(num, NULL) / strtod(den, NULL);
}

double nv_rational_to_double(NvValue v) {
    if (v.tag != NV_RATIONAL) return 0.0;
    return nv_rat_to_double_str(v.u.rational->num->digits, v.u.rational->den->digits);
}

double nv_bits_to_double(NvValue v) {
    if (v.tag != NV_BITS) return 0.0;
    double n = 0.0;
    for (size_t i = 0; i < v.u.bits->nbits; i++) {
        size_t byte = i / 8;
        int bit = (v.u.bits->bytes[byte] >> (7 - (i % 8))) & 1;
        n = n * 2.0 + (double)bit;
    }
    return n;
}

// ----- منشئات القيم الجديدة -----
static NvBigInt *nv_bigint_alloc_normed(char *normed) {
    NvBigInt *b = (NvBigInt *)nv_alloc(sizeof(NvBigInt), NV_OBJ_BIGINT);
    int neg = normed[0] == '-';
    b->neg = neg;
    b->digits = nv_strdup(neg ? normed + 1 : normed);
    return b;
}

static char *nv_bigint_signed_str(NvBigInt *b) {
    if (!b->neg || strcmp(b->digits, "0") == 0) return nv_strdup(b->digits);
    size_t len = strlen(b->digits);
    char *out = (char *)malloc(len + 2);
    out[0] = '-';
    memcpy(out + 1, b->digits, len + 1);
    return out;
}

NvValue nv_bigint_cstr(const char *s, int line, int col) {
    char *n = nv_bigint_norm(s);
    if (!n) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "عدد كبير غير صالح: %s", s ? s : "");
    }
    NvBigInt *b = nv_bigint_alloc_normed(n);
    free(n);
    NvValue v;
    v.tag = NV_BIGINT;
    v.u.bigint = b;
    return v;
}

NvValue nv_bigint_from_double(double n, int line, int col) {
    if (!isfinite(n) || n != trunc(n)) {
        nv_error(NV_ERR_TYPE, "عدد_كبير يتطلب عددا صحيحا، وليس كسرا", line, col);
    }
    char buf[512];
    nv_fmt_f64_plain(n, buf, sizeof(buf));
    return nv_bigint_cstr(buf, line, col);
}

static NvBigInt *nv_bigint_from_value(NvValue v, int line, int col) {
    // عدد/كبير إلى كبير (العدد يجب أن يكون صحيحا).
    if (v.tag == NV_BIGINT) return v.u.bigint;
    if (v.tag == NV_NUM) {
        NvValue b = nv_bigint_from_double(v.u.num, line, col);
        return b.u.bigint;
    }
    return NULL;
}

NvValue nv_rational_new(NvValue num, NvValue den, int line, int col) {
    NvBigInt *n = nv_bigint_from_value(num, line, col);
    NvBigInt *d = nv_bigint_from_value(den, line, col);
    if (!n || !d) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "كسر يتطلب بسطا ومقاما صحيحين");
    }
    size_t rn = nv_root_push_idx();
    *nv_root_slot(rn) = num;
    size_t rd = nv_root_push_idx();
    *nv_root_slot(rd) = den;
    char *nn = NULL, *dd = NULL;
    int ok = nv_rat_norm_str(n->digits, d->digits, &nn, &dd);
    if (!ok) {
        nv_root_pop_n(2);
        nv_error(NV_ERR_TYPE, "مقام الكسر لا يمكن أن يكون صفرا", line, col);
    }
    NvRational *r = (NvRational *)nv_alloc(sizeof(NvRational), NV_OBJ_RATIONAL);
    size_t rr = nv_root_push_idx();
    NvValue rv;
    rv.tag = NV_RATIONAL;
    rv.u.rational = r;
    *nv_root_slot(rr) = rv;
    r->num = nv_bigint_alloc_normed(nn);
    r->den = nv_bigint_alloc_normed(dd);
    free(nn); free(dd);
    nv_root_pop_n(3);
    NvValue v;
    v.tag = NV_RATIONAL;
    v.u.rational = r;
    return v;
}

NvValue nv_complex_new(double re, double im) {
    NvValue v;
    v.tag = NV_COMPLEX;
    v.u.complex.re = re;
    v.u.complex.im = im;
    return v;
}

NvValue nv_bits_new(const unsigned char *bytes, size_t nbits) {
    size_t nbytes = (nbits + 7) / 8;
    NvBits *b = (NvBits *)nv_alloc(sizeof(NvBits), NV_OBJ_BITS);
    b->nbits = nbits;
    b->bytes = (unsigned char *)malloc(nbytes ? nbytes : 1);
    if (nbytes) memcpy(b->bytes, bytes, nbytes);
    if (nbits % 8) {
        b->bytes[nbytes - 1] &= (unsigned char)(0xFF << (8 - (nbits % 8)));
    }
    NvValue v;
    v.tag = NV_BITS;
    v.u.bits = b;
    return v;
}

NvValue nv_bits_from_u64(unsigned long long n, size_t len) {
    size_t need = 1;
    unsigned long long t = n;
    while (t >= 2) { need++; t >>= 1; }
    if (len == 0) len = need;
    size_t nbytes = (len + 7) / 8;
    unsigned char *bytes = (unsigned char *)calloc(nbytes ? nbytes : 1, 1);
    for (size_t i = 0; i < len; i++) {
        size_t bitpos = len - 1 - i;
        int bit = (int)((n >> bitpos) & 1ULL);
        if (bit) bytes[i / 8] |= (unsigned char)(1 << (7 - (i % 8)));
    }
    NvValue v = nv_bits_new(bytes, len);
    free(bytes);
    return v;
}

NvValue nv_boxed_new(NvValue inner) {
    size_t ri = nv_root_push_idx();
    *nv_root_slot(ri) = inner;
    NvBoxed *b = (NvBoxed *)nv_alloc(sizeof(NvBoxed), NV_OBJ_BOXED);
    b->inner = inner;
    nv_root_pop_n(1);
    NvValue v;
    v.tag = NV_BOXED;
    v.u.boxed = b;
    return v;
}

// ----- تحويلات القيم العددية (مرآة value.rs) -----
int nv_value_to_rational(NvValue v, char **num, char **den) {
    // عدد/كبير/كسر إلى كسره الدقيق.
    *num = NULL; *den = NULL;
    if (v.tag == NV_BIGINT) {
        *num = nv_strdup(v.u.bigint->digits);
        if (v.u.bigint->neg) {
            char *t = (char *)malloc(strlen(*num) + 2);
            t[0] = '-'; strcpy(t + 1, *num);
            free(*num); *num = t;
        }
        *den = nv_strdup("1");
        return 1;
    }
    if (v.tag == NV_RATIONAL) {
        *num = nv_strdup(v.u.rational->num->digits);
        if (v.u.rational->num->neg) {
            char *t = (char *)malloc(strlen(*num) + 2);
            t[0] = '-'; strcpy(t + 1, *num);
            free(*num); *num = t;
        }
        *den = nv_strdup(v.u.rational->den->digits);
        return 1;
    }
    if (v.tag == NV_NUM) {
        double n = v.u.num;
        if (!isfinite(n)) return 0;
        char plain[512];
        nv_fmt_f64_plain(n, plain, sizeof(plain));
        // عشري عادي إلى كسر: أرقام/10^أس.
        int neg = plain[0] == '-';
        const char *body = neg ? plain + 1 : plain;
        const char *dot = strchr(body, '.');
        char digits[512];
        size_t nd = 0, frac = 0;
        for (const char *p = body; *p; p++) {
            if (*p == '.') continue;
            digits[nd++] = *p;
        }
        digits[nd] = '\0';
        if (dot) frac = strlen(dot + 1);
        char *scale = nv_bigint_pow10_str(frac);
        char *nn = NULL, *dd = NULL;
        char *dig = nv_bigint_norm(digits);
        if (neg && strcmp(dig, "0") != 0) {
            size_t len = strlen(dig);
            char *t = (char *)malloc(len + 2);
            t[0] = '-'; memcpy(t + 1, dig, len + 1);
            free(dig); dig = t;
        }
        int ok = nv_rat_norm_str(dig, scale, &nn, &dd);
        free(dig); free(scale);
        if (!ok) return 0;
        *num = nn; *den = dd;
        return 1;
    }
    return 0;
}

int nv_value_to_complex(NvValue v, double *re, double *im) {
    if (v.tag == NV_NUM) { *re = v.u.num; *im = 0.0; return 1; }
    if (v.tag == NV_BIGINT) { *re = nv_bigint_to_double(v); *im = 0.0; return 1; }
    if (v.tag == NV_RATIONAL) { *re = nv_rational_to_double(v); *im = 0.0; return 1; }
    if (v.tag == NV_COMPLEX) { *re = v.u.complex.re; *im = v.u.complex.im; return 1; }
    return 0;
}

static int nv_is_j6_numeric(NvValue v) {
    return v.tag == NV_NUM || v.tag == NV_BIGINT || v.tag == NV_RATIONAL || v.tag == NV_COMPLEX;
}

static int nv_numeric_eq(NvValue l, NvValue r) {
    // مساواة دقيقة عبر الأنواع (مرآة numeric_eq في Rust).
    double lr, li, rr, ri;
    int lc = nv_value_to_complex(l, &lr, &li);
    int rc = nv_value_to_complex(r, &rr, &ri);
    if (!lc || !rc) return 0;
    if (li != 0.0 || ri != 0.0) return lr == rr && li == ri;
    if (l.tag == NV_NUM && r.tag == NV_NUM) return l.u.num == r.u.num;
    char *ln = NULL, *ld = NULL, *rn = NULL, *rd = NULL;
    if (!nv_value_to_rational(l, &ln, &ld) || !nv_value_to_rational(r, &rn, &rd)) {
        free(ln); free(ld); free(rn); free(rd);
        return lr == rr;
    }
    int c = nv_rat_cmp_str(ln, ld, rn, rd);
    free(ln); free(ld); free(rn); free(rd);
    return c == 0;
}

static int nv_numeric_cmp(NvValue l, NvValue r, int *out, int line, int col) {
    // 1 = يقارَن (out معبأة)، 0 = بلا ترتيب (مركب —
    // أي طرف مركب، حتى بصفر تخيلي، كما في Rust).
    // NaN تخطئ مباشرة «مقارنة غير صالحة».
    if (l.tag == NV_COMPLEX || r.tag == NV_COMPLEX) return 0;
    double lr, li, rr, ri;
    if (!nv_value_to_complex(l, &lr, &li) || !nv_value_to_complex(r, &rr, &ri)) return 0;
    if ((l.tag == NV_NUM && isnan(l.u.num)) || (r.tag == NV_NUM && isnan(r.u.num))) {
        nv_error(NV_ERR_RUNTIME, "مقارنة غير صالحة", line, col);
    }
    if (l.tag == NV_NUM && r.tag == NV_NUM) {
        *out = l.u.num < r.u.num ? -1 : (l.u.num > r.u.num ? 1 : 0);
        return 1;
    }
    char *ln = NULL, *ld = NULL, *rn = NULL, *rd = NULL;
    if (!nv_value_to_rational(l, &ln, &ld) || !nv_value_to_rational(r, &rn, &rd)) {
        free(ln); free(ld); free(rn); free(rd);
        *out = lr < rr ? -1 : (lr > rr ? 1 : 0);
        return 1;
    }
    *out = nv_rat_cmp_str(ln, ld, rn, rd);
    free(ln); free(ld); free(rn); free(rd);
    return 1;
}

static NvValue nv_exact_arith(NvValue l, NvValue r, char op, int line, int col) {
    // الحساب الدقيق (مرآة add_values/arith_values في
    // Rust): المركب أولا (بـf64)، ثم كبير/كبير خالصا،
    // ثم المختلط إلى كسر دقيق (دائما كسر، بلا طيّ إلى
    // كبير)، ثم عدد/عدد كما كان حرفيا. رسالة الخطأ
    // النوعي لـ`+` هي «لا يمكن جمع»، ولغيرها
    // «العملية الحسابية تتطلب عددين».
    if (!nv_is_j6_numeric(l) || !nv_is_j6_numeric(r)) {
        if (op == '+') {
            nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن جمع %s و %s",
                         nv_type_name(l), nv_type_name(r));
        }
        nv_error_fmt(NV_ERR_TYPE, line, col, "العملية الحسابية تتطلب عددين، وليس %s و %s",
                     nv_type_name(l), nv_type_name(r));
    }
    // المركب أولا — أي طرف مركب، حتى بصفر تخيلي،
    // والناتج مركب دائما (بـf64).
    if (l.tag == NV_COMPLEX || r.tag == NV_COMPLEX) {
        double lr, li, rr, ri;
        nv_value_to_complex(l, &lr, &li);
        nv_value_to_complex(r, &rr, &ri);
        if (op == '%') {
            nv_error_fmt(NV_ERR_TYPE, line, col, "العملية الحسابية تتطلب عددين، وليس %s و %s",
                         nv_type_name(l), nv_type_name(r));
        }
        double re = 0.0, im = 0.0;
        switch (op) {
            case '+': re = lr + rr; im = li + ri; break;
            case '-': re = lr - rr; im = li - ri; break;
            case '*': re = lr * rr - li * ri; im = lr * ri + li * rr; break;
            case '/': {
                double den = rr * rr + ri * ri;
                if (den == 0.0) nv_error(NV_ERR_RUNTIME, "قسمة على صفر", line, col);
                re = (lr * rr + li * ri) / den;
                im = (li * rr - lr * ri) / den;
                break;
            }
        }
        return nv_complex_new(re, im);
    }
    // كبير/كبير خالصا (دقيق، بلا كسر وسيط).
    if (l.tag == NV_BIGINT && r.tag == NV_BIGINT) {
        char *a = nv_bigint_signed_str(l.u.bigint);
        char *b = nv_bigint_signed_str(r.u.bigint);
        NvValue out;
        switch (op) {
            case '+': {
                char *t = nv_bigint_add_str(a, b);
                out = nv_bigint_cstr(t, line, col);
                free(t);
                break;
            }
            case '-': {
                char *t = nv_bigint_sub_str(a, b);
                out = nv_bigint_cstr(t, line, col);
                free(t);
                break;
            }
            case '*': {
                char *t = nv_bigint_mul_str(a, b);
                out = nv_bigint_cstr(t, line, col);
                free(t);
                break;
            }
            case '/': {
                char *q = NULL, *rem = NULL;
                if (!nv_bigint_divmod_str(a, b, &q, &rem)) {
                    free(a); free(b);
                    nv_error(NV_ERR_RUNTIME, "قسمة على صفر", line, col);
                }
                if (nv_bigint_is_zero_str(rem)) {
                    out = nv_bigint_cstr(q, line, col);
                    free(q); free(rem);
                } else {
                    free(q); free(rem);
                    char *n = NULL, *d = NULL;
                    nv_rat_norm_str(a, b, &n, &d);
                    NvRational *rat = (NvRational *)nv_alloc(sizeof(NvRational), NV_OBJ_RATIONAL);
                    size_t ri = nv_root_push_idx();
                    NvValue rv;
                    rv.tag = NV_RATIONAL;
                    rv.u.rational = rat;
                    *nv_root_slot(ri) = rv;
                    rat->num = nv_bigint_alloc_normed(n);
                    rat->den = nv_bigint_alloc_normed(d);
                    free(n); free(d);
                    out = rv;
                    nv_root_pop_n(1);
                }
                break;
            }
            case '%': {
                char *q = NULL, *rem = NULL;
                if (!nv_bigint_divmod_str(a, b, &q, &rem)) {
                    free(a); free(b);
                    nv_error(NV_ERR_RUNTIME, "باقي القسمة على صفر", line, col);
                }
                free(q);
                out = nv_bigint_cstr(rem, line, col);
                free(rem);
                break;
            }
            default: out = nv_nil(); break;
        }
        free(a); free(b);
        return out;
    }
    // مختلط (كبير/كسر/عدد) — يترقى إلى كسر دقيق،
    // والناتج كسر دائما (ولو كان مقامه 1).
    if (l.tag == NV_BIGINT || l.tag == NV_RATIONAL || r.tag == NV_BIGINT || r.tag == NV_RATIONAL) {
        char *ln = NULL, *ld = NULL, *rn = NULL, *rd = NULL;
        nv_value_to_rational(l, &ln, &ld);
        nv_value_to_rational(r, &rn, &rd);
        if (op == '/' || op == '%') {
            if (nv_bigint_is_zero_str(rn)) {
                free(ln); free(ld); free(rn); free(rd);
                nv_error(NV_ERR_RUNTIME, op == '/' ? "قسمة على صفر" : "باقي القسمة على صفر", line, col);
            }
        }
        char *n = NULL, *d = NULL;
        switch (op) {
            case '+': nv_rat_add_str(ln, ld, rn, rd, &n, &d); break;
            case '-': nv_rat_sub_str(ln, ld, rn, rd, &n, &d); break;
            case '*': nv_rat_mul_str(ln, ld, rn, rd, &n, &d); break;
            case '/': nv_rat_div_str(ln, ld, rn, rd, &n, &d); break;
            case '%': nv_rat_rem_str(ln, ld, rn, rd, &n, &d); break;
        }
        free(ln); free(ld); free(rn); free(rd);
        NvRational *rat = (NvRational *)nv_alloc(sizeof(NvRational), NV_OBJ_RATIONAL);
        size_t ri = nv_root_push_idx();
        NvValue rv;
        rv.tag = NV_RATIONAL;
        rv.u.rational = rat;
        *nv_root_slot(ri) = rv;
        rat->num = nv_bigint_alloc_normed(n);
        rat->den = nv_bigint_alloc_normed(d);
        free(n); free(d);
        NvValue out = rv;
        nv_root_pop_n(1);
        return out;
    }
    // عدد/عدد كما كان حرفيا.
    double a = l.u.num, b = r.u.num;
    switch (op) {
        case '+': return nv_num(a + b);
        case '-': return nv_num(a - b);
        case '*': return nv_num(a * b);
        case '/':
            if (b == 0.0) nv_error(NV_ERR_RUNTIME, "قسمة على صفر", line, col);
            return nv_num(a / b);
        case '%':
            if (b == 0.0) nv_error(NV_ERR_RUNTIME, "باقي القسمة على صفر", line, col);
            return nv_num(fmod(a, b));
    }
    return nv_nil();
}

// ---------- البنود 145–147: العموميات ----------
// ترميز جدول معاملات النوع: مدخلات مفصولة بـ `؛`
// (U+061B) — كل مدخل `اسم[:حد][=افتراضي]`.
static void nv_free_str_array(char **arr, int n) {
    if (!arr) return;
    for (int i = 0; i < n; i++) free(arr[i]);
    free(arr);
}

static char **nv_dup_str_array(char **arr, int n) {
    if (!arr || n <= 0) return NULL;
    char **out = (char **)malloc((size_t)n * sizeof(char *));
    for (int i = 0; i < n; i++) out[i] = arr[i] ? nv_strdup(arr[i]) : NULL;
    return out;
}

// حلّل جدول معاملات نوع مرمّز إلى ثلاث مصفوفات
// متوازية (أسماء وحدود وافتراضيات)؛ وNULL/فارغ
// يعني بلا معاملات نوع.
static void nv_parse_type_params(const char *encoded, char ***names, char ***bounds,
                                 char ***defaults, int *n_out) {
    *names = NULL;
    *bounds = NULL;
    *defaults = NULL;
    *n_out = 0;
    if (!encoded || !*encoded) return;
    // عدّ المدخلات (فاصل `؛` = بايتا 0xD8 0x9B).
    int n = 1;
    for (const char *p = encoded; *p; p++) {
        if ((unsigned char)p[0] == 0xD8 && (unsigned char)p[1] == 0x9B) n++;
    }
    char **nm = (char **)malloc((size_t)n * sizeof(char *));
    char **bd = (char **)malloc((size_t)n * sizeof(char *));
    char **df = (char **)malloc((size_t)n * sizeof(char *));
    int idx = 0;
    const char *start = encoded;
    for (;;) {
        const char *end = start;
        while (*end && !((unsigned char)end[0] == 0xD8 && (unsigned char)end[1] == 0x9B)) end++;
        size_t len = (size_t)(end - start);
        char *entry = (char *)malloc(len + 1);
        memcpy(entry, start, len);
        entry[len] = '\0';
        // مدخل `اسم[:حد][=افتراضي]`: الافتراضي بعد
        // أول `=`؛ والحد بعد أول `:` قبله.
        char *eq = strchr(entry, '=');
        char *head_end = eq ? eq : entry + len;
        char *colon = NULL;
        for (char *p = entry; p < head_end; p++) {
            if (*p == ':') { colon = p; break; }
        }
        size_t nlen = (size_t)((colon ? colon : head_end) - entry);
        nm[idx] = (char *)malloc(nlen + 1);
        memcpy(nm[idx], entry, nlen);
        nm[idx][nlen] = '\0';
        if (colon) {
            size_t blen = (size_t)(head_end - (colon + 1));
            bd[idx] = (char *)malloc(blen + 1);
            memcpy(bd[idx], colon + 1, blen);
            bd[idx][blen] = '\0';
        } else {
            bd[idx] = NULL;
        }
        df[idx] = eq ? nv_strdup(eq + 1) : NULL;
        free(entry);
        idx++;
        if (!*end) break;
        start = end + 2;
    }
    *names = nm;
    *bounds = bd;
    *defaults = df;
    *n_out = idx;
}

// الأساس النصي لنوع (ما قبل أول `[`) — للفحص الاسمي.
static void nv_type_base(const char *t, char *buf, size_t cap) {
    size_t i = 0;
    while (t[i] && t[i] != '[' && i + 1 < cap) {
        buf[i] = t[i];
        i++;
    }
    buf[i] = '\0';
}

// هل النوع الفعلي يحقق الحد؟ مساواة حرفية، أو
// الأساس نفسه لنوع عام، أو الحد `أي` — كما في Rust.
static int nv_type_satisfies_bound(const char *arg, const char *bound) {
    if (!bound || strcmp(bound, "أي") == 0) return 1;
    if (strcmp(arg, bound) == 0) return 1;
    char base[256];
    nv_type_base(arg, base, sizeof(base));
    return strcmp(base, bound) == 0;
}

// هل الحرف عند الموضع فاصل نوع (نهاية رمز)؟ الفواصل:
// بداية/نهاية النص، `[` `]` `,` ASCII، مسافة/تاب،
// والفاصلة العربية `،` (بايتا 0xD8 0x8C).
static int nv_type_is_delim(const char *s, size_t i) {
    if (i == 0) return 1;
    unsigned char c = (unsigned char)s[i - 1];
    if (c == '[' || c == ']' || c == ',' || c == ' ' || c == '\t') return 1;
    if (i >= 2 && (unsigned char)s[i - 2] == 0xD8 && c == 0x8C) return 1;
    return 0;
}

static int nv_type_delim_at(const char *s, size_t i) {
    unsigned char c = (unsigned char)s[i];
    if (c == '\0' || c == '[' || c == ']' || c == ',' || c == ' ' || c == '\t') return 1;
    if (c == 0xD8 && (unsigned char)s[i + 1] == 0x8C) return 1;
    return 0;
}

// استبدال أسماء معاملات النوع في نوع معلن بقيم
// الربط — على مستوى الرموز (`قائمة[ت]` تصير
// `قائمة[عدد]`)؛ وتعيد NULL إن لم يتغير شيء
// (فيستخدم المتصل النوع الأصلي).
static char *nv_substitute_type(const char *declared, char **names, char **values, int n) {
    if (!declared || n <= 0 || !names || !values) return NULL;
    size_t cap = strlen(declared) + 1;
    for (int i = 0; i < n; i++) {
        if (values[i]) cap += strlen(values[i]);
    }
    char *out = (char *)malloc(cap + 64);
    size_t o = 0;
    int changed = 0;
    size_t len = strlen(declared);
    size_t i = 0;
    while (i < len) {
        int matched = -1;
        size_t mlen = 0;
        for (int k = 0; k < n; k++) {
            if (!names[k] || !values[k]) continue;
            size_t nl = strlen(names[k]);
            if (nl > 0 && i + nl <= len && memcmp(declared + i, names[k], nl) == 0 &&
                nv_type_is_delim(declared, i) && nv_type_delim_at(declared, i + nl)) {
                matched = k;
                mlen = nl;
                break;
            }
        }
        if (matched >= 0) {
            size_t vl = strlen(values[matched]);
            memcpy(out + o, values[matched], vl);
            o += vl;
            i += mlen;
            changed = 1;
        } else {
            out[o++] = declared[i++];
        }
    }
    out[o] = '\0';
    if (!changed) {
        free(out);
        return NULL;
    }
    return out;
}

// حلّ ربط النوع: معاملات مفصولة بفاصلة ASCII؛
// والرسائل حرفيا كما في المفسر (نوع).
static char **nv_resolve_type_binding(char **tp_names, char **tp_bounds, char **tp_defaults,
                                      int ntp, const char *type_args, const char *kind,
                                      const char *owner, int line, int col) {
    // فصّ المعاملات (فاصلة ASCII فقط؛ فالفاصلة
    // العربية `،` داخل نوع عام جزء منه).
    char *args_copy = type_args ? nv_strdup(type_args) : nv_strdup("");
    // عدّها.
    int nargs = 0;
    if (*args_copy) {
        nargs = 1;
        for (char *p = args_copy; *p; p++) {
            if (*p == ',') nargs++;
        }
    }
    if (nargs > ntp) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "معاملات نوع زائدة: %s %s تتوقع %d لكن أعطيت %d",
                     kind, owner, ntp, nargs);
    }
    char **binding = NULL;
    if (ntp > 0) {
        binding = (char **)calloc((size_t)ntp, sizeof(char *));
    }
    // قسّم إلى مقاطع مقصوصة المسافات.
    char **given = NULL;
    if (nargs > 0) {
        given = (char **)malloc((size_t)nargs * sizeof(char *));
        int gi = 0;
        char *save = NULL;
        // strtok_r غير مضمونة التوفر هنا — قسّم يدويا.
        (void)save;
        char *cur = args_copy;
        for (;;) {
            char *comma = strchr(cur, ',');
            if (comma) *comma = '\0';
            while (*cur == ' ' || *cur == '\t') cur++;
            size_t l = strlen(cur);
            while (l > 0 && (cur[l - 1] == ' ' || cur[l - 1] == '\t')) cur[--l] = '\0';
            given[gi++] = nv_strdup(cur);
            if (!comma) break;
            cur = comma + 1;
        }
    }
    for (int i = 0; i < ntp; i++) {
        const char *value = NULL;
        if (i < nargs) {
            value = given[i];
        } else if (tp_defaults && tp_defaults[i]) {
            value = tp_defaults[i];
        } else {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "معامل النوع %s بلا قيمة ولا افتراضي في %s %s",
                         tp_names[i], kind, owner);
        }
        if (tp_bounds && tp_bounds[i] && !nv_type_satisfies_bound(value, tp_bounds[i])) {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "النوع %s لا يحقق حد معامل النوع %s: %s",
                         value, tp_names[i], tp_bounds[i]);
        }
        binding[i] = nv_strdup(value);
    }
    if (given) {
        for (int i = 0; i < nargs; i++) free(given[i]);
        free(given);
    }
    free(args_copy);
    return binding;
}

// ربط افتراضيات معاملات النوع (بلا معاملات
// معطاة): قيم مستعارة من الافتراضيات المعلنة —
// عناصرها لا تُحرَّر، والمصفوفة نفسها مملوكة
// للمتصل (يحررها هو). بلا معاملات نوع يعيد NULL.
static char **nv_default_type_binding(char **tp_names, char **tp_bounds, char **tp_defaults,
                                      int ntp, const char *kind, const char *owner,
                                      int line, int col) {
    if (ntp <= 0) return NULL;
    char **binding = (char **)calloc((size_t)ntp, sizeof(char *));
    for (int i = 0; i < ntp; i++) {
        if (!tp_defaults || !tp_defaults[i]) {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "معامل النوع %s بلا قيمة ولا افتراضي في %s %s",
                         tp_names[i], kind, owner);
        }
        if (tp_bounds && tp_bounds[i] && !nv_type_satisfies_bound(tp_defaults[i], tp_bounds[i])) {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "النوع %s لا يحقق حد معامل النوع %s: %s",
                         tp_defaults[i], tp_names[i], tp_bounds[i]);
        }
        binding[i] = tp_defaults[i];
    }
    return binding;
}

NvValue nv_func_new(const char *name, NvCFunc cfunc, NvScope *captured,
                    const char *module, NvScope *module_scope,
                    char **param_names, char **param_types,
                    const char *return_type, int nparams,
                    NvValue *defaults, int *has_default,
                    const char *vararg, const char *kwarg,
                    const char *type_params) {
    NvFunc *f = (NvFunc *)nv_alloc(sizeof(NvFunc), NV_OBJ_FUNC);
    f->name = nv_strdup(name);
    f->cfunc = cfunc;
    f->captured = captured;
    f->module = module ? nv_strdup(module) : NULL;
    f->module_scope = module_scope;
    // نسخ جداول المعاملات إلى الكومة: المولد يمرر حرفيات
    // مركبة تموت مع إطار الدالة المعرِّفة.
    if (nparams > 0) {
        f->param_names = (char **)malloc((size_t)nparams * sizeof(char *));
        f->param_types = (char **)malloc((size_t)nparams * sizeof(char *));
        for (int i = 0; i < nparams; i++) {
            f->param_names[i] = param_names && param_names[i] ? nv_strdup(param_names[i]) : NULL;
            f->param_types[i] = param_types && param_types[i] ? nv_strdup(param_types[i]) : NULL;
        }
    } else {
        f->param_names = NULL;
        f->param_types = NULL;
    }
    f->return_type = return_type ? nv_strdup(return_type) : NULL;
    f->nparams = nparams;
    // البنود 109–112: نسخ جدول الافتراضيات (قيم
    // مقيَّمة) وأعلامه؛ والجامعان نصان منسوخان.
    if (nparams > 0) {
        f->defaults = (NvValue *)malloc((size_t)nparams * sizeof(NvValue));
        f->has_default = (int *)malloc((size_t)nparams * sizeof(int));
        for (int i = 0; i < nparams; i++) {
            f->defaults[i] = defaults ? defaults[i] : nv_nil();
            f->has_default[i] = has_default ? has_default[i] : 0;
        }
    } else {
        f->defaults = NULL;
        f->has_default = NULL;
    }
    f->vararg = vararg ? nv_strdup(vararg) : NULL;
    f->kwarg = kwarg ? nv_strdup(kwarg) : NULL;
    // البنود 145–147: معاملات النوع المعلنة؛
    // والربط يبقى NULL حتى التخصيص.
    nv_parse_type_params(type_params, &f->tp_names, &f->tp_bounds, &f->tp_defaults,
                         &f->ntype_params);
    f->type_binding = NULL;
    NvValue v;
    v.tag = NV_FUNC;
    v.u.func = f;
    return v;
}

NvValue nv_native_new(const char *name, NvNativeFn fn) {
    NvNative *n = (NvNative *)nv_alloc(sizeof(NvNative), NV_OBJ_NATIVE);
    n->name = nv_strdup(name);
    n->fn = fn;
    NvValue v;
    v.tag = NV_NATIVE;
    v.u.native = n;
    return v;
}

NvValue nv_module_new(const char *name, NvScope *scope) {
    NvModule *m = (NvModule *)nv_alloc(sizeof(NvModule), NV_OBJ_MODULE);
    m->name = nv_strdup(name);
    m->exports = NULL;
    m->scope = scope;
    NvValue v;
    v.tag = NV_MODULE;
    v.u.module = m;
    return v;
}

NvValue nv_class_new(const char *name, NvClass *base, const char *type_params) {
    NvClass *c = (NvClass *)nv_alloc(sizeof(NvClass), NV_OBJ_CLASS);
    c->name = nv_strdup(name);
    // البنود 145–147: معاملات النوع المعلنة؛
    // والربط يبقى NULL حتى التخصيص.
    nv_parse_type_params(type_params, &c->tp_names, &c->tp_bounds, &c->tp_defaults,
                         &c->ntype_params);
    c->type_binding = NULL;
    NvValue v;
    v.tag = NV_CLASS;
    v.u.klass = c;
    // جذّر الوليد قبل أي تخصيص آخر: nv_dict_new قد
    // يشغّل جامع القمامة، والفئة بلا جذر حتى تُعاد.
    size_t ri = nv_root_push_idx();
    *nv_root_slot(ri) = v;
    // قاموس الطرائق يُخصص هنا دائما (فارغا)، وطرق الأب
    // تُنسخ مسطحة — فلا يبقى NULL أبدا فيضرب add_method.
    NvValue methods = nv_dict_new();
    c->methods = methods.u.dict;
    if (base && base->methods) {
        for (size_t i = 0; i < base->methods->len; i++) {
            nv_dict_set_raw(c->methods, base->methods->entries[i].key,
                            base->methods->entries[i].val);
        }
    }
    c->base = base;
    c->interfaces = NULL;
    c->ninterfaces = 0;
    nv_root_pop();
    return v;
}

NvValue nv_iface_new(const char *name) {
    NvIface *in = (NvIface *)nv_alloc(sizeof(NvIface), NV_OBJ_IFACE);
    in->name = nv_strdup(name);
    in->methods = NULL;
    in->arity = NULL;
    in->nmethods = 0;
    NvValue v;
    v.tag = NV_IFACE;
    v.u.iface = in;
    return v;
}

NvValue nv_inst_new(NvClass *klass) {
    NvInst *in = (NvInst *)nv_alloc(sizeof(NvInst), NV_OBJ_INST);
    in->klass = klass;
    NvValue v;
    v.tag = NV_INST;
    v.u.inst = in;
    // جذّر الوليد قبل تخصيص قاموس الحقول (نفس سبب
    // nv_class_new: التخصيص التالي قد يجمع القمامة).
    size_t ri = nv_root_push_idx();
    *nv_root_slot(ri) = v;
    NvValue f = nv_dict_new();
    in->fields = f.u.dict;
    nv_root_pop();
    return v;
}

NvValue nv_bound_new(NvValue func, NvInst *inst) {
    NvBound *b = (NvBound *)nv_alloc(sizeof(NvBound), NV_OBJ_BOUND);
    b->func = func;
    b->inst = inst;
    NvValue v;
    v.tag = NV_BOUND;
    v.u.bound = b;
    return v;
}

NvValue nv_handle_new(int kind, int id, void *ptr) {
    NvHandle *h = (NvHandle *)nv_alloc(sizeof(NvHandle), NV_OBJ_HANDLE);
    h->kind = kind;
    h->id = id;
    h->ptr = ptr;
    NvValue v;
    v.tag = NV_HANDLE;
    v.u.handle = h;
    return v;
}

NvValue nv_bytes_new(const unsigned char *data, size_t len) {
    NvBytes *b = (NvBytes *)nv_alloc(sizeof(NvBytes) + len, NV_OBJ_BYTES);
    b->len = len;
    if (len) memcpy(b->data, data, len);
    NvValue v;
    v.tag = NV_BYTES;
    v.u.bytes = b;
    return v;
}

// نسخ عميق: القائمة والقاموس تُنسخ عناصرهما؛ والباقي
// يُشارك (دوال، وحدات، فئات، كائنات — كائنات بلا نسخ).
NvValue nv_clone_value(NvValue v) {
    // جذّر المصدر والوجهة قبل أي تخصيص: النسخة قد
    // تطلق الجامعة أثناء النسخ العودي، والمصدر قد
    // لا يكون مجذرا عند المتصل (نسخة حديثة في
    // متغير C محلي فقط)، والوجهة نصف المبنية لا
    // يمسكها إلا متغير C محلي أيضا.
    size_t _rs = nv_root_push_idx();
    *nv_root_slot(_rs) = v;
    switch (v.tag) {
        case NV_LIST: {
            NvValue out = nv_list_new();
            size_t _rd = nv_root_push_idx();
            *nv_root_slot(_rd) = out;
            NvList *src = v.u.list, *dst = out.u.list;
            for (size_t i = 0; i < src->len; i++) {
                NvValue e = nv_clone_value(src->items[i]);
                if (dst->len == dst->cap) {
                    dst->cap = dst->cap ? dst->cap * 2 : 8;
                    dst->items = (NvValue *)realloc(dst->items, dst->cap * sizeof(NvValue));
                }
                dst->items[dst->len++] = e;
            }
            nv_root_pop_n(2);
            return out;
        }
        case NV_DICT: {
            NvValue out = nv_dict_new();
            size_t _rd = nv_root_push_idx();
            *nv_root_slot(_rd) = out;
            NvDict *src = v.u.dict, *dst = out.u.dict;
            for (size_t i = 0; i < src->len; i++) {
                if (dst->len == dst->cap) {
                    dst->cap = dst->cap ? dst->cap * 2 : 8;
                    dst->entries = (NvDictEntry *)realloc(dst->entries, dst->cap * sizeof(NvDictEntry));
                }
                dst->entries[dst->len].key = nv_strdup(src->entries[i].key);
                dst->entries[dst->len].val = nv_clone_value(src->entries[i].val);
                dst->len++;
            }
            nv_root_pop_n(2);
            return out;
        }
        // البندان 138 و139: نسخ عميق يحفظ الوسم —
        // فقراءة المتغير في الأصلي تنسخ المترابطة
        // والتشكيلة كما تنسخ القائمة والقاموس.
        case NV_TUPLE: {
            NvValue out = nv_tuple_new();
            size_t _rd = nv_root_push_idx();
            *nv_root_slot(_rd) = out;
            NvList *src = v.u.tuple, *dst = out.u.tuple;
            for (size_t i = 0; i < src->len; i++) {
                NvValue e = nv_clone_value(src->items[i]);
                if (dst->len == dst->cap) {
                    dst->cap = dst->cap ? dst->cap * 2 : 8;
                    dst->items = (NvValue *)realloc(dst->items, dst->cap * sizeof(NvValue));
                }
                dst->items[dst->len++] = e;
            }
            nv_root_pop_n(2);
            return out;
        }
        case NV_SET: {
            NvValue out = nv_set_new();
            size_t _rd = nv_root_push_idx();
            *nv_root_slot(_rd) = out;
            NvList *src = v.u.set, *dst = out.u.set;
            for (size_t i = 0; i < src->len; i++) {
                NvValue e = nv_clone_value(src->items[i]);
                if (dst->len == dst->cap) {
                    dst->cap = dst->cap ? dst->cap * 2 : 8;
                    dst->items = (NvValue *)realloc(dst->items, dst->cap * sizeof(NvValue));
                }
                dst->items[dst->len++] = e;
            }
            nv_root_pop_n(2);
            return out;
        }
        // ج6: نسخ عميق يحفظ الوسم — الكبير والكسر
        // والبت بنسخ سلاسلهم/بايتاتهم، والمركب مضمن
        // فينسخ بالقيمة، والمعلب بنسخ داخله عوديا.
        case NV_BIGINT: {
            char *sg = nv_bigint_signed_str(v.u.bigint);
            NvValue out = nv_bigint_cstr(sg, 0, 0);
            free(sg);
            nv_root_pop_n(1);
            return out;
        }
        case NV_RATIONAL: {
            char *sn = nv_bigint_signed_str(v.u.rational->num);
            char *sd = nv_bigint_signed_str(v.u.rational->den);
            NvValue out = nv_rational_new(
                nv_bigint_cstr(sn, 0, 0),
                nv_bigint_cstr(sd, 0, 0), 0, 0);
            free(sn); free(sd);
            nv_root_pop_n(1);
            return out;
        }
        case NV_COMPLEX:
            nv_root_pop_n(1);
            return v;
        case NV_BITS: {
            NvValue out = nv_bits_new(v.u.bits->bytes, v.u.bits->nbits);
            nv_root_pop_n(1);
            return out;
        }
        case NV_BOXED: {
            NvValue inner = nv_clone_value(v.u.boxed->inner);
            NvValue out = nv_boxed_new(inner);
            nv_root_pop_n(1);
            return out;
        }
        default:
            nv_root_pop_n(1);
            return v;
    }
}

NvValue nv_share_value(NvValue v) {
    return v;
}

// ---------- أسماء الأنواع ----------
const char *nv_type_name(NvValue v) {
    switch (v.tag) {
        case NV_NUM: return "عدد";
        case NV_STR: return "نص";
        case NV_BOOL: return "منطقي";
        case NV_LIST: return "قائمة";
        case NV_DICT: return "قاموس";
        case NV_TUPLE: return "مترابطة";
        case NV_SET: return "تشكيلة";
        case NV_NIL: return "لاشيء";
        case NV_FUNC: return "دالة";
        case NV_NATIVE: return "دالة";
        case NV_MODULE: return "وحدة";
        case NV_CLASS: return "فئة";
        case NV_IFACE: return "واجهة";
        case NV_INST: return "كائن";
        case NV_BOUND: return "دالة";
        case NV_HANDLE: return "عدد";
        case NV_BYTES: return "قائمة";
        // ج6.
        case NV_BIGINT: return "كبير";
        case NV_RATIONAL: return "كسر";
        case NV_COMPLEX: return "مركب";
        case NV_BITS: return "بت";
        case NV_BOXED: return "معلب";
    }
    return "لاشيء";
}

// ---------- UTF-8 ----------
size_t nv_utf8_chars(const char *s, size_t len) {
    return nv_count_utf8(s, len);
}

const char *nv_utf8_char_at(const char *s, size_t len, size_t idx, size_t *clen) {
    size_t i = 0, n = 0;
    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        size_t adv = 1;
        if ((c & 0x80) == 0) adv = 1;
        else if ((c & 0xE0) == 0xC0) adv = 2;
        else if ((c & 0xF0) == 0xE0) adv = 3;
        else if ((c & 0xF8) == 0xF0) adv = 4;
        if (i + adv > len) adv = 1;
        if (n == idx) {
            if (clen) *clen = adv;
            return s + i;
        }
        i += adv;
        n++;
    }
    return NULL;
}

int nv_str_eq(const char *a, size_t alen, const char *b, size_t blen) {
    return alen == blen && (alen == 0 || memcmp(a, b, alen) == 0);
}

// ---------- تنسيق الأعداد (صيغة عرض Rust) ----------
// أقصر تمثيل عشري يعيد التحويل نفسه، بلا أسّ (Display
// لا يستخدم e أبدا)؛ ثم الأرقام العربية والنقطة ٫.
static void nv_fmt_f64_plain(double n, char *out, size_t outsz) {
    if (isnan(n)) {
        snprintf(out, outsz, "NaN");
        return;
    }
    if (isinf(n)) {
        snprintf(out, outsz, n < 0 ? "-inf" : "inf");
        return;
    }
    if (n == 0.0) {
        snprintf(out, outsz, "0");
        return;
    }
    // عدد صحيح ضمن 9e15: صيغة صحيحة (كما في fmt_number).
    if (n == trunc(n) && fabs(n) < 9.0e15) {
        snprintf(out, outsz, "%lld", (long long)n);
        return;
    }
    // أقصر علمي يعيد التحويل: جرّب الدقة 1..17.
    char sci[64];
    double back = 0;
    int prec = 17;
    for (int p = 1; p <= 17; p++) {
        snprintf(sci, sizeof(sci), "%.*e", p - 1, n);
        back = strtod(sci, NULL);
        if (back == n) {
            prec = p;
            break;
        }
    }
    snprintf(sci, sizeof(sci), "%.*e", prec - 1, n);
    // sci = [-]d.dddde±XX — حوّله إلى عشري عادي.
    int neg = (sci[0] == '-');
    const char *p = sci + (neg ? 1 : 0);
    // الأرقام بعد الفاصلة.
    char digits[32];
    int ndig = 0;
    digits[ndig++] = p[0];
    const char *dot = strchr(p, '.');
    if (dot) {
        const char *e = strchr(dot, 'e');
        for (const char *q = dot + 1; q < e; q++) digits[ndig++] = *q;
    }
    // أزل الأصفار الزائدة من الأرقام (لكن أبق رقما واحدا).
    while (ndig > 1 && digits[ndig - 1] == '0') ndig--;
    const char *ep = strchr(sci, 'e');
    int exp10 = atoi(ep + 1);
    // الموضع العشري بعد الرقم الأول: القيمة = 0.digits × 10^(exp10+1)
    // عدد الأرقام قبل الفاصلة = exp10 + 1.
    char *w = out;
    size_t left = outsz;
    if (neg && left > 1) {
        *w++ = '-';
        left--;
    }
    int int_digits = exp10 + 1;
    if (int_digits <= 0) {
        // 0.000ddd
        if (left > 2) {
            *w++ = '0';
            *w++ = '.';
            left -= 2;
        }
        for (int i = 0; i < -int_digits && left > 1; i++) {
            *w++ = '0';
            left--;
        }
        for (int i = 0; i < ndig && left > 1; i++) {
            *w++ = digits[i];
            left--;
        }
    } else if (int_digits >= ndig) {
        for (int i = 0; i < ndig && left > 1; i++) {
            *w++ = digits[i];
            left--;
        }
        for (int i = 0; i < int_digits - ndig && left > 1; i++) {
            *w++ = '0';
            left--;
        }
    } else {
        for (int i = 0; i < int_digits && left > 1; i++) {
            *w++ = digits[i];
            left--;
        }
        if (left > 1) {
            *w++ = '.';
            left--;
        }
        for (int i = int_digits; i < ndig && left > 1; i++) {
            *w++ = digits[i];
            left--;
        }
    }
    *w = '\0';
}

// أرقام غربية إلى مشرقية، والنقطة إلى ٫ — تعيد
// سلسلة جديدة مخصصة (العربية أطول بايتيا من الغربية،
// فالكتابة في موضعها تفيض بمخزن المتصل).
char *nv_arabic_digits_dup(const char *s) {
    static const char *DIG[10] = {"٠", "١", "٢", "٣", "٤", "٥", "٦", "٧", "٨", "٩"};
    size_t n = strlen(s);
    char *tmp = (char *)malloc(n * 3 + 1);
    size_t w = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c >= '0' && c <= '9') {
            const char *d = DIG[c - '0'];
            size_t dl = strlen(d);
            memcpy(tmp + w, d, dl);
            w += dl;
        } else if (c == '.') {
            const char *d = "٫";
            size_t dl = strlen(d);
            memcpy(tmp + w, d, dl);
            w += dl;
        } else {
            tmp[w++] = (char)c;
        }
    }
    tmp[w] = '\0';
    return tmp;
}

char *nv_fmt_number(double n) {
    // أقصى عشري عادي لـ double: 309 أرقام صحيحة أو
    // 323 صفرا بعد الفاصلة + 17 رقما معنويا + إشارة.
    char plain[512];
    nv_fmt_f64_plain(n, plain, sizeof(plain));
    // inf و NaN يبقيان لاتينيين (لا أرقام فيهما).
    if (strcmp(plain, "inf") != 0 && strcmp(plain, "-inf") != 0 && strcmp(plain, "NaN") != 0) {
        return nv_arabic_digits_dup(plain);
    }
    return nv_strdup(plain);
}

// تحويل نص إلى عدد (لـ عدد()): يقبل الأرقام العربية
// والغربية والنقطة العشرية العربية والغربية.
int nv_parse_number(const char *s, double *out) {
    // طبّع إلى ASCII مؤقتا.
    size_t n = strlen(s);
    char *tmp = (char *)malloc(n + 1);
    size_t w = 0;
    for (size_t i = 0; i < n;) {
        unsigned char c = (unsigned char)s[i];
        if (c < 0x80) {
            tmp[w++] = (char)c;
            i++;
        } else if (c == 0xD9 && i + 1 < n) {
            // أرقام عربية مشرقية U+0660..U+0669
            unsigned char c2 = (unsigned char)s[i + 1];
            if (c2 >= 0xA0 && c2 <= 0xA9) {
                tmp[w++] = (char)('0' + (c2 - 0xA0));
                i += 2;
            } else if (c2 == 0xAB) {
                // ٫ (U+066B) فاصلة عشرية عربية
                tmp[w++] = '.';
                i += 2;
            } else {
                tmp[w++] = (char)c;
                i++;
            }
        } else if (c == 0xDB && i + 1 < n) {
            // أرقام عربية ممتدة U+06F0..U+06F9
            unsigned char c2 = (unsigned char)s[i + 1];
            if (c2 >= 0xB0 && c2 <= 0xB9) {
                tmp[w++] = (char)('0' + (c2 - 0xB0));
                i += 2;
            } else {
                tmp[w++] = (char)c;
                i++;
            }
        } else {
            tmp[w++] = (char)c;
            i++;
        }
    }
    tmp[w] = '\0';
    char *end = NULL;
    double v = strtod(tmp, &end);
    int ok = (end != tmp && *end == '\0');
    if (ok) *out = v;
    free(tmp);
    return ok;
}

// ---------- عرض القيم ----------
static void nv_escape_into(const char *s, size_t len, char **buf, size_t *blen, size_t *bcap) {
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        const char *esc = NULL;
        char one[2] = {(char)c, '\0'};
        switch (c) {
            case '"': esc = "\\\""; break;
            case '\\': esc = "\\\\"; break;
            case '\n': esc = "\\n"; break;
            case '\t': esc = "\\t"; break;
            default: esc = one; break;
        }
        size_t el = strlen(esc);
        if (*blen + el + 1 > *bcap) {
            *bcap = (*bcap + el + 16) * 2;
            *buf = (char *)realloc(*buf, *bcap);
        }
        memcpy(*buf + *blen, esc, el);
        *blen += el;
    }
}

static void nv_fmt_into(NvValue v, int in_collection, char **buf, size_t *blen, size_t *bcap);

static void nv_buf_append(char **buf, size_t *blen, size_t *bcap, const char *s, size_t n) {
    if (*blen + n + 1 > *bcap) {
        while (*blen + n + 1 > *bcap) *bcap = (*bcap ? *bcap * 2 : 64);
        *buf = (char *)realloc(*buf, *bcap);
    }
    memcpy(*buf + *blen, s, n);
    *blen += n;
}

static void nv_fmt_into(NvValue v, int in_collection, char **buf, size_t *blen, size_t *bcap) {
    char tmp[128];
    switch (v.tag) {
        case NV_NUM: {
            char *s = nv_fmt_number(v.u.num);
            nv_buf_append(buf, blen, bcap, s, strlen(s));
            free(s);
            break;
        }
        case NV_STR: {
            if (in_collection) {
                nv_buf_append(buf, blen, bcap, "\"", 1);
                nv_escape_into(v.u.str->data, v.u.str->len, buf, blen, bcap);
                nv_buf_append(buf, blen, bcap, "\"", 1);
            } else {
                nv_buf_append(buf, blen, bcap, v.u.str->data, v.u.str->len);
            }
            break;
        }
        case NV_BOOL:
            nv_buf_append(buf, blen, bcap, v.u.boolean ? "صحيح" : "خطأ",
                          v.u.boolean ? strlen("صحيح") : strlen("خطأ"));
            break;
        case NV_LIST: {
            nv_buf_append(buf, blen, bcap, "[", 1);
            for (size_t i = 0; i < v.u.list->len; i++) {
                if (i) nv_buf_append(buf, blen, bcap, ", ", 2);
                nv_fmt_into(v.u.list->items[i], 1, buf, blen, bcap);
            }
            nv_buf_append(buf, blen, bcap, "]", 1);
            break;
        }
        case NV_DICT: {
            nv_buf_append(buf, blen, bcap, "{", 1);
            for (size_t i = 0; i < v.u.dict->len; i++) {
                if (i) nv_buf_append(buf, blen, bcap, ", ", 2);
                nv_buf_append(buf, blen, bcap, "\"", 1);
                nv_escape_into(v.u.dict->entries[i].key, strlen(v.u.dict->entries[i].key), buf, blen, bcap);
                nv_buf_append(buf, blen, bcap, "\": ", 3);
                nv_fmt_into(v.u.dict->entries[i].val, 1, buf, blen, bcap);
            }
            nv_buf_append(buf, blen, bcap, "}", 1);
            break;
        }
        // البندان 138 و139 — مرآة fmt_value في Rust:
        // المترابطة `(أ، ب)` و`(أ،)` للوحيدة و`()`
        // للفارغة؛ والتشكيلة `{أ، ب}` و`{}` للفارغة.
        case NV_TUPLE: {
            nv_buf_append(buf, blen, bcap, "(", 1);
            for (size_t i = 0; i < v.u.tuple->len; i++) {
                if (i) nv_buf_append(buf, blen, bcap, ", ", 2);
                nv_fmt_into(v.u.tuple->items[i], 1, buf, blen, bcap);
            }
            if (v.u.tuple->len == 1) nv_buf_append(buf, blen, bcap, ",", 1);
            nv_buf_append(buf, blen, bcap, ")", 1);
            break;
        }
        case NV_SET: {
            nv_buf_append(buf, blen, bcap, "{", 1);
            for (size_t i = 0; i < v.u.set->len; i++) {
                if (i) nv_buf_append(buf, blen, bcap, ", ", 2);
                nv_fmt_into(v.u.set->items[i], 1, buf, blen, bcap);
            }
            nv_buf_append(buf, blen, bcap, "}", 1);
            break;
        }
        case NV_NIL:
            nv_buf_append(buf, blen, bcap, "لاشيء", strlen("لاشيء"));
            break;
        case NV_FUNC:
            snprintf(tmp, sizeof(tmp), "<دالة %s>", v.u.func->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_NATIVE:
            snprintf(tmp, sizeof(tmp), "<دالة %s>", v.u.native->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_MODULE:
            snprintf(tmp, sizeof(tmp), "<وحدة %s>", v.u.module->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_CLASS:
            snprintf(tmp, sizeof(tmp), "<فئة %s>", v.u.klass->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_IFACE:
            snprintf(tmp, sizeof(tmp), "<واجهة %s>", v.u.iface->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_INST:
            snprintf(tmp, sizeof(tmp), "<كائن %s>", v.u.inst->klass->name);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_BOUND: {
            NvValue f = v.u.bound->func;
            if (f.tag == NV_FUNC) {
                snprintf(tmp, sizeof(tmp), "<دالة %s>", f.u.func->name);
            } else {
                snprintf(tmp, sizeof(tmp), "<دالة>");
            }
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        }
        case NV_HANDLE:
            snprintf(tmp, sizeof(tmp), "%d", v.u.handle->id);
            nv_buf_append(buf, blen, bcap, tmp, strlen(tmp));
            break;
        case NV_BYTES: {
            // بايتات تُعرض كقائمة أعداد (دلالة المحرك).
            nv_buf_append(buf, blen, bcap, "[", 1);
            for (size_t i = 0; i < v.u.bytes->len; i++) {
                if (i) nv_buf_append(buf, blen, bcap, ", ", 2);
                char *s = nv_fmt_number((double)v.u.bytes->data[i]);
                nv_buf_append(buf, blen, bcap, s, strlen(s));
                free(s);
            }
            nv_buf_append(buf, blen, bcap, "]", 1);
            break;
        }
        // ج6: عرض الأنواع الجديدة — مرآة fmt_value في Rust.
        case NV_BIGINT: {
            char *plain = nv_bigint_norm(v.u.bigint->digits);
            if (v.u.bigint->neg && strcmp(plain, "0") != 0) {
                size_t len = strlen(plain);
                char *t = (char *)malloc(len + 2);
                t[0] = '-';
                memcpy(t + 1, plain, len + 1);
                free(plain);
                plain = t;
            }
            char *s = nv_arabic_digits_dup(plain);
            nv_buf_append(buf, blen, bcap, s, strlen(s));
            free(plain);
            free(s);
            break;
        }
        case NV_RATIONAL: {
            if (strcmp(v.u.rational->den->digits, "1") == 0) {
                NvValue n;
                n.tag = NV_BIGINT;
                n.u.bigint = v.u.rational->num;
                nv_fmt_into(n, in_collection, buf, blen, bcap);
            } else {
                NvValue n, d;
                n.tag = NV_BIGINT;
                n.u.bigint = v.u.rational->num;
                d.tag = NV_BIGINT;
                d.u.bigint = v.u.rational->den;
                nv_fmt_into(n, in_collection, buf, blen, bcap);
                nv_buf_append(buf, blen, bcap, "/", 1);
                nv_fmt_into(d, in_collection, buf, blen, bcap);
            }
            break;
        }
        case NV_COMPLEX: {
            double re = v.u.complex.re, im = v.u.complex.im;
            if (im == 0.0) {
                char *s = nv_fmt_number(re);
                nv_buf_append(buf, blen, bcap, s, strlen(s));
                free(s);
            } else if (re == 0.0) {
                char *s = nv_fmt_number(im);
                nv_buf_append(buf, blen, bcap, s, strlen(s));
                free(s);
                nv_buf_append(buf, blen, bcap, "ت", strlen("ت"));
            } else {
                char *s = nv_fmt_number(re);
                nv_buf_append(buf, blen, bcap, s, strlen(s));
                free(s);
                nv_buf_append(buf, blen, bcap, im > 0.0 ? "+" : "-", 1);
                char *t = nv_fmt_number(fabs(im));
                nv_buf_append(buf, blen, bcap, t, strlen(t));
                free(t);
                nv_buf_append(buf, blen, bcap, "ت", strlen("ت"));
            }
            break;
        }
        case NV_BITS: {
            nv_buf_append(buf, blen, bcap, "بت(", strlen("بت("));
            char *plain = (char *)malloc(v.u.bits->nbits + 1);
            for (size_t i = 0; i < v.u.bits->nbits; i++) {
                int bit = (v.u.bits->bytes[i / 8] >> (7 - (i % 8))) & 1;
                plain[i] = (char)('0' + bit);
            }
            plain[v.u.bits->nbits] = '\0';
            char *s = nv_arabic_digits_dup(plain);
            nv_buf_append(buf, blen, bcap, s, strlen(s));
            free(plain);
            free(s);
            nv_buf_append(buf, blen, bcap, ")", 1);
            break;
        }
        case NV_BOXED: {
            nv_buf_append(buf, blen, bcap, "معلب(", strlen("معلب("));
            nv_fmt_into(v.u.boxed->inner, in_collection, buf, blen, bcap);
            nv_buf_append(buf, blen, bcap, ")", 1);
            break;
        }
    }
}

char *nv_fmt_value(NvValue v) {
    char *buf = NULL;
    size_t blen = 0, bcap = 0;
    nv_fmt_into(v, 0, &buf, &blen, &bcap);
    if (!buf) return nv_strdup("");
    if (blen + 1 > bcap) buf = (char *)realloc(buf, blen + 1);
    buf[blen] = '\0';
    return buf;
}

// ---------- الدفعة 4 — المتغيرات (اجتماع 58 §73) ----------

// حلّ المرجع الاسمي إلى هدفه النهائي (بحد 32 —
// الدورة مستحيلة أصلا لأن `مرجع()` يرفضها).
// يعيد مؤشرا إلى سلسلة يملكها الجدول أو إلى
// الاسم نفسه — بلا تخصيص.
static const char *nv_resolve_name(const char *name) {
    const char *current = name;
    for (int i = 0; i < 32; i++) {
        const char *next = NULL;
        for (size_t j = 0; j < nv_nameref_len; j++) {
            if (strcmp(nv_nameref_from[j], current) == 0) { next = nv_nameref_to[j]; break; }
        }
        if (!next) break;
        current = next;
    }
    return current;
}

static int nv_frozen_contains(const char *name) {
    for (size_t i = 0; i < nv_frozen_len; i++) {
        if (strcmp(nv_frozen[i], name) == 0) return 1;
    }
    return 0;
}

void nv_check_not_frozen(const char *name, int line, int col) {
    if (nv_frozen_contains(nv_resolve_name(name))) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير مجمد: %s", name);
    }
}

// إزالة ربط اسم من سلسلة النطاقات (أقرب ربط) —
// لـ`احذف_متغير`.
static void nv_scope_unbind(NvScope *scope, const char *name) {
    for (NvScope *s = scope; s; s = s->parent) {
        NvBinding **link = &s->bindings;
        while (*link) {
            if (strcmp((*link)->name, name) == 0) {
                NvBinding *dead = *link;
                *link = dead->next;
                free(dead->name);
                return;
            }
            link = &(*link)->next;
        }
    }
}

// اسم المضيف من `/proc/sys/kernel/hostname`
// (لينكس/أندرويد) — سابقة `نظام.اسم_النظام`.
static int nv_read_hostname(char *out, size_t n) {
#if defined(__linux__) || defined(__ANDROID__)
    FILE *f = fopen("/proc/sys/kernel/hostname", "r");
    if (!f) return 0;
    if (!fgets(out, (int)n, f)) { fclose(f); return 0; }
    fclose(f);
    size_t len = strlen(out);
    while (len && (out[len - 1] == '\n' || out[len - 1] == '\r')) out[--len] = '\0';
    return len > 0;
#else
    (void)out; (void)n;
    return 0;
#endif
}

// اسم المستخدم: رقمه من `/proc/self/status` ثم
// اسمه من `/etc/passwd` (لينكس/أندرويد).
static int nv_read_username(char *out, size_t n) {
#if defined(__linux__) || defined(__ANDROID__)
    FILE *f = fopen("/proc/self/status", "r");
    if (!f) return 0;
    char line[256];
    char uid[32] = "";
    while (fgets(line, sizeof(line), f)) {
        if (strncmp(line, "Uid:", 4) == 0) {
            sscanf(line + 4, "%31s", uid);
            break;
        }
    }
    fclose(f);
    if (!uid[0]) return 0;
    f = fopen("/etc/passwd", "r");
    if (!f) return 0;
    while (fgets(line, sizeof(line), f)) {
        char name[128], pass[128], id[32];
        if (sscanf(line, "%127[^:]:%127[^:]:%31[^:]", name, pass, id) == 3
            && strcmp(id, uid) == 0 && name[0]) {
            snprintf(out, n, "%s", name);
            fclose(f);
            return 1;
        }
    }
    fclose(f);
    return 0;
#else
    (void)out; (void)n;
    return 0;
#endif
}

// ---------- النطاقات ----------
NvScope *nv_scope_new(NvScope *parent) {
    NvScope *s = (NvScope *)nv_alloc(sizeof(NvScope), NV_OBJ_SCOPE);
    s->parent = parent;
    s->bindings = NULL;
    return s;
}

NvCell *nv_scope_lookup_cell(NvScope *scope, const char *name) {
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): حلّ
    // المرجع الاسمي أولا — فكل قراءة (معرّف،
    // واستدعاء، وفتحة فهرس) تصل الهدف.
    name = nv_resolve_name(name);
    for (NvScope *s = scope; s; s = s->parent) {
        for (NvBinding *b = s->bindings; b; b = b->next) {
            if (strcmp(b->name, name) == 0) return b->cell;
        }
    }
    return NULL;
}

// مؤشر فتحة متغير (لتعيين الفهرس): يخطئ «متغير غير
// معرف» بموضع الهدف إن غاب الاسم.
NvValue *nv_scope_slot_ptr(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return &cell->val;
}

// الدفعة الأخيرة DEF (البنود 77 و78): فتحة موجَّهة
// بالإعلان — لتعيين الفهرس على اسم معلن عاما/محيطا.
NvValue *nv_scope_slot_ptr_global(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell_global(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return &cell->val;
}

NvValue *nv_scope_slot_ptr_nonlocal(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell_nonlocal(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return &cell->val;
}

NvCell *nv_scope_define(NvScope *scope, const char *name, NvValue v) {
    for (NvBinding *b = scope->bindings; b; b = b->next) {
        if (strcmp(b->name, name) == 0) {
            b->cell->val = nv_clone_value(v);
            nv_write_barrier((NvObj *)b->cell, nv_obj_of(v));
            return b->cell;
        }
    }
    NvCell *cell = (NvCell *)nv_alloc(sizeof(NvCell), NV_OBJ_CELL);
    cell->val = nv_clone_value(v);
    NvBinding *b = (NvBinding *)malloc(sizeof(NvBinding));
    b->name = nv_strdup(name);
    b->cell = cell;
    b->next = scope->bindings;
    scope->bindings = b;
    return cell;
}

void nv_scope_assign(NvScope *scope, const char *name, NvValue v, int line, int col) {
    // الدفعة 4 — المتغيرات (اجتماع 58 §73):
    // الكتابة عبر مرجع تصل هدفه، والكتابة في
    // مجمد مرفوضة قبل أي تغيير.
    name = nv_resolve_name(name);
    nv_check_not_frozen(name, line, col);
    for (NvScope *s = scope; s; s = s->parent) {
        for (NvBinding *b = s->bindings; b; b = b->next) {
            if (strcmp(b->name, name) == 0) {
                b->cell->val = nv_clone_value(v);
                nv_write_barrier((NvObj *)b->cell, nv_obj_of(v));
                return;
            }
        }
    }
    nv_scope_define(scope, name, v);
}

// الدفعة الأخيرة DEF (البنود 77 و78 — الاجتماع 83
// §98): مساعد الإسناد العام الواحد — يمشي إلى جذر
// السلسلة (نطاق البرنامج الجاري) ويكتب هناك،
// ويعرّف الاسم فيه إن غاب. كـ nv_scope_assign في
// فحص التجميد وحلّ المرجع.
void nv_scope_assign_global(NvScope *scope, const char *name, NvValue v, int line, int col) {
    name = nv_resolve_name(name);
    nv_check_not_frozen(name, line, col);
    NvScope *root = scope;
    while (root && root->parent) root = root->parent;
    if (!root) root = scope;
    for (NvBinding *b = root->bindings; b; b = b->next) {
        if (strcmp(b->name, name) == 0) {
            b->cell->val = nv_clone_value(v);
            nv_write_barrier((NvObj *)b->cell, nv_obj_of(v));
            return;
        }
    }
    nv_scope_define(root, name, v);
}

// الدفعة الأخيرة DEF (البند 78): مساعد الإسناد
// المحيط الواحد — يمشي من أب الإطار متخطيا الجذر
// (لا المحلي ولا العام)؛ وغياب الربط خطأ عربي صريح.
void nv_scope_assign_nonlocal(NvScope *scope, const char *name, NvValue v, int line, int col) {
    name = nv_resolve_name(name);
    nv_check_not_frozen(name, line, col);
    for (NvScope *s = scope ? scope->parent : NULL; s && s->parent; s = s->parent) {
        for (NvBinding *b = s->bindings; b; b = b->next) {
            if (strcmp(b->name, name) == 0) {
                b->cell->val = nv_clone_value(v);
                nv_write_barrier((NvObj *)b->cell, nv_obj_of(v));
                return;
            }
        }
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "لا يوجد ربط محيط للمتغير: %s", name);
}

// الدفعة الأخيرة DEF (البنود 77 و78): بحث موجَّه
// بالإعلان — عام من الجذر وحده، ونطاق من المحيط
// وحده (أب الإطار حتى ما قبل الجذر).
NvCell *nv_scope_lookup_cell_global(NvScope *scope, const char *name) {
    name = nv_resolve_name(name);
    NvScope *root = scope;
    while (root && root->parent) root = root->parent;
    if (!root) return NULL;
    for (NvBinding *b = root->bindings; b; b = b->next) {
        if (strcmp(b->name, name) == 0) return b->cell;
    }
    return NULL;
}

NvCell *nv_scope_lookup_cell_nonlocal(NvScope *scope, const char *name) {
    name = nv_resolve_name(name);
    for (NvScope *s = scope ? scope->parent : NULL; s && s->parent; s = s->parent) {
        for (NvBinding *b = s->bindings; b; b = b->next) {
            if (strcmp(b->name, name) == 0) return b->cell;
        }
    }
    return NULL;
}

NvScope *nv_scope_clone(NvScope *scope) {
    if (!scope) return NULL;
    // جذّر الأب المستنسخ قبل بناء النطاق الجديد،
    // والنطاق الجديد أثناء نسخ الروابط: التخصيص
    // قد يطلق الجامعة، وكلاهما لا يمسكه إلا متغير
    // C محلي حتى يُربط.
    NvScope *parent = nv_scope_clone(scope->parent);
    if (parent) nv_scope_root_push(parent);
    NvScope *out = nv_scope_new(parent);
    nv_scope_root_push(out);
    // انسخ الروابط بترتيبها (عكس الإدخال مرتين يحفظ الترتيب).
    // الترتيب لا يؤثر في الدلالة؛ انسخ مباشرة.
    for (NvBinding *b = scope->bindings; b; b = b->next) {
        NvCell *cell = (NvCell *)nv_alloc(sizeof(NvCell), NV_OBJ_CELL);
        cell->val = nv_clone_value(b->cell->val);
        NvBinding *nb = (NvBinding *)malloc(sizeof(NvBinding));
        nb->name = nv_strdup(b->name);
        nb->cell = cell;
        nb->next = out->bindings;
        out->bindings = nb;
    }
    nv_scope_root_pop_n(parent ? 2 : 1);
    return out;
}

NvScope *nv_module_call_scope(NvFunc *f) {
    return nv_scope_clone(f->module_scope);
}

// ---------- تهيئة ----------
static void nv_mkdir_best_effort(const char *path) {
#ifdef _WIN32
    mkdir(path);
#else
    mkdir(path, 0777);
#endif
}

// الاجتماع 53 §67 (الميزة 210): إنشاء متداخل
// بسلوك `mkdir -p` — كل مكون مفقود يُنشأ،
// والموجود كمجلد يُتخطى بصمت؛ والموجود كملف
// خطأ. المسح بايت بايت آمن على UTF-8 لأن
// `/` (و`\` على ويندوز) لا يظهر داخل تتابع
// متعدد البايتات.
static int nv_mkdir_p(const char *path) {
    size_t len = strlen(path);
    char *buf = (char *)malloc(len + 1);
    if (!buf) {
        errno = ENOMEM;
        return -1;
    }
    memcpy(buf, path, len + 1);
    for (size_t i = 1; i <= len; i++) {
#ifdef _WIN32
        int sep = (buf[i] == '/' || buf[i] == '\\' || buf[i] == '\0');
#else
        int sep = (buf[i] == '/' || buf[i] == '\0');
#endif
        if (sep) {
            char save = buf[i];
            buf[i] = '\0';
#ifdef _WIN32
            // جذر القرص (C:) وجذر UNC (\\خادم و\\خادم\مشاركة)
            // موجودان دائما — ومحاولة إنشائهما تفشل
            // ENOENT فتفسد mkdir -p كله على ويندوز
            // (تكافؤ C على ويندوز، التشغيل 42706100129:
            // كل `أنشئ_مجلد` أصلي فشل بـ«No such file»
            // لأن أول مكون مُنشأ كان "C:"). يُتخطى
            // إنشاؤهما ويُنشأ ما بعدهما فقط.
            int is_root = 0;
            if (i == 2 && buf[1] == ':' && isalpha((unsigned char)buf[0])) {
                is_root = 1;
            } else if (len >= 2 && buf[0] == '\\' && buf[1] == '\\') {
                size_t seps = 0;
                for (size_t j = 2; j < i; j++) {
                    if (buf[j] == '\\' || buf[j] == '/') seps++;
                }
                if (seps <= 1) is_root = 1;
            }
#else
            const int is_root = 0;
#endif
            if (buf[0] != '\0' && !is_root) {
                int rc;
#ifdef _WIN32
                rc = mkdir(buf);
#else
                rc = mkdir(buf, 0777);
#endif
                if (rc != 0 && errno != EEXIST) {
                    int e = errno;
                    free(buf);
                    errno = e;
                    return -1;
                }
                if (rc != 0) {
                    struct stat st;
                    if (stat(buf, &st) != 0 || !S_ISDIR(st.st_mode)) {
                        int e = errno;
                        free(buf);
                        errno = e;
                        return -1;
                    }
                }
            }
            buf[i] = save;
            if (save == '\0') {
                break;
            }
        }
    }
    free(buf);
    return 0;
}

// وحّد فواصل مسار إلى `/` في موضعه (ويندوز فقط):
// getcwd يرجع `\` بينما المسارات المقنّنة ترجع `/`
// (nv_win32_realpath)، فيفشل طيّ العرض بالمقارنة
// النصية؛ والتوحيد عند البدء يجعل كل المسارات
// المخزنة بصيغة واحدة مطابقة للمفسر.
#ifdef _WIN32
static void nv_normalize_seps(char *s) {
    for (size_t i = 0; s[i]; i++) if (s[i] == '\\') s[i] = '/';
}
#endif

void nv_init(const char *program_dir) {
    nv_program_dir = nv_strdup(program_dir);
#ifdef _WIN32
    nv_normalize_seps(nv_program_dir);
#endif
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): بدء
    // العملية بالثواني (لـ`بيئة("ثواني")`).
    nv_start_secs = (unsigned long long)time(NULL);
    // اجتماع 45 §59: البيت هو `program_dir/home`؛
    // يُنشأ بأفضل جهد (بلا رسالة إن نجح، وبلا خطأ
    // إن تعذّر — البرنامج المستقل لا يملك جلسة
    // تفاعلية تشرح له).
    size_t n = strlen(nv_program_dir) + 6;
    nv_home_dir = (char *)malloc(n);
    snprintf(nv_home_dir, n, "%s/home", nv_program_dir);
    nv_mkdir_best_effort(nv_home_dir);
    size_t tn = strlen(nv_program_dir) + 5;
    char *tmpdir = (char *)malloc(tn);
    snprintf(tmpdir, tn, "%s/tmp", nv_program_dir);
    nv_mkdir_best_effort(tmpdir);
    free(tmpdir);
    char *cwd = getcwd(NULL, 0);
    nv_cwd = cwd ? cwd : nv_strdup(".");
#ifdef _WIN32
    nv_normalize_seps(nv_cwd);
#endif
}

void nv_shutdown(void) {
    // الجمع الكبير الأخير يحرر كل شيء (لا حاجة فعلية
    // عند خروج العملية، لكنه يبقي المخصصات نظيفة).
    nv_collect_major();
}

// ================= الجزء 2: العمليات والاستدعاء =================

// ---------- مساواة عميقة ----------
int nv_dict_find(NvDict *d, const char *key) {
    for (size_t i = 0; i < d->len; i++) {
        if (strcmp(d->entries[i].key, key) == 0) return (int)i;
    }
    return -1;
}

void nv_dict_set_raw(NvDict *d, const char *key, NvValue val) {
    int idx = nv_dict_find(d, key);
    if (idx >= 0) {
        d->entries[idx].val = nv_clone_value(val);
        nv_write_barrier((NvObj *)d, nv_obj_of(val));
        return;
    }
    if (d->len == d->cap) {
        d->cap = d->cap ? d->cap * 2 : 8;
        d->entries = (NvDictEntry *)realloc(d->entries, d->cap * sizeof(NvDictEntry));
    }
    d->entries[d->len].key = nv_strdup(key);
    d->entries[d->len].val = nv_clone_value(val);
    nv_write_barrier((NvObj *)d, nv_obj_of(val));
    d->len++;
}

void nv_list_push_raw(NvList *l, NvValue val) {
    if (l->len == l->cap) {
        l->cap = l->cap ? l->cap * 2 : 8;
        l->items = (NvValue *)realloc(l->items, l->cap * sizeof(NvValue));
    }
    l->items[l->len++] = nv_clone_value(val);
    nv_write_barrier((NvObj *)l, nv_obj_of(val));
}

// قيمة عددية لقيمة: العدد نفسه، والمقبض معرفه.
// (في Rust لا نوع مقبض أصلا — المقابض أعداد؛ فهذا
// الإكراه يطابق دلالة المفسر في العمليات العامة.)
static int nv_is_num_like(NvValue v) {
    return v.tag == NV_NUM || v.tag == NV_HANDLE;
}

static double nv_num_of(NvValue v) {
    if (v.tag == NV_NUM) return v.u.num;
    return (double)v.u.handle->id;
}

int nv_eq(NvValue l, NvValue r) {
    // ج6: المعلبة تساوي داخلها (فك طبقة واحدة هنا،
    // والعودية تفك الطبقات المتداخلة).
    while (l.tag == NV_BOXED) l = l.u.boxed->inner;
    while (r.tag == NV_BOXED) r = r.u.boxed->inner;
    if (nv_is_num_like(l) && nv_is_num_like(r)) {
        return nv_num_of(l) == nv_num_of(r);
    }
    // ج6: مساواة دقيقة عبر عدد/كبير/كسر/مركب.
    if (nv_is_j6_numeric(l) && nv_is_j6_numeric(r)) {
        return nv_numeric_eq(l, r);
    }
    if (l.tag == NV_BITS && r.tag == NV_BITS) {
        if (l.u.bits->nbits != r.u.bits->nbits) return 0;
        size_t nbytes = (l.u.bits->nbits + 7) / 8;
        return nbytes == 0 || memcmp(l.u.bits->bytes, r.u.bits->bytes, nbytes) == 0;
    }
    if (l.tag != r.tag) {
        // عدد/منطقي/نص... الأنواع المختلفة لا تتساوى أبدا.
        return 0;
    }
    switch (l.tag) {
        case NV_NUM: return l.u.num == r.u.num;
        case NV_BOOL: return l.u.boolean == r.u.boolean;
        case NV_NIL: return 1;
        case NV_STR:
            return nv_str_eq(l.u.str->data, l.u.str->len, r.u.str->data, r.u.str->len);
        case NV_LIST: {
            if (l.u.list->len != r.u.list->len) return 0;
            for (size_t i = 0; i < l.u.list->len; i++) {
                if (!nv_eq(l.u.list->items[i], r.u.list->items[i])) return 0;
            }
            return 1;
        }
        case NV_DICT: {
            if (l.u.dict->len != r.u.dict->len) return 0;
            for (size_t i = 0; i < l.u.dict->len; i++) {
                int j = nv_dict_find(r.u.dict, l.u.dict->entries[i].key);
                if (j < 0) return 0;
                if (!nv_eq(l.u.dict->entries[i].val, r.u.dict->entries[j].val)) return 0;
            }
            return 1;
        }
        // البند 138: المترابطة مرتبة كالقائمة.
        case NV_TUPLE: {
            if (l.u.tuple->len != r.u.tuple->len) return 0;
            for (size_t i = 0; i < l.u.tuple->len; i++) {
                if (!nv_eq(l.u.tuple->items[i], r.u.tuple->items[i])) return 0;
            }
            return 1;
        }
        // البند 139: التشكيلة بلا ترتيب — الطول نفسه
        // وكل عنصر له مساوٍ في الأخرى.
        case NV_SET: {
            if (l.u.set->len != r.u.set->len) return 0;
            for (size_t i = 0; i < l.u.set->len; i++) {
                int found = 0;
                for (size_t j = 0; j < r.u.set->len; j++) {
                    if (nv_eq(l.u.set->items[i], r.u.set->items[j])) { found = 1; break; }
                }
                if (!found) return 0;
            }
            return 1;
        }
        case NV_FUNC: return l.u.func == r.u.func;
        case NV_NATIVE: return l.u.native == r.u.native;
        case NV_MODULE: return l.u.module == r.u.module;
        case NV_CLASS: return l.u.klass == r.u.klass;
        case NV_IFACE: return l.u.iface == r.u.iface;
        case NV_INST: return l.u.inst == r.u.inst;
        case NV_BOUND:
            return l.u.bound->inst == r.u.bound->inst && nv_eq(l.u.bound->func, r.u.bound->func);
        case NV_HANDLE: return l.u.handle->id == r.u.handle->id;
        case NV_BYTES: {
            if (l.u.bytes->len != r.u.bytes->len) return 0;
            return memcmp(l.u.bytes->data, r.u.bytes->data, l.u.bytes->len) == 0;
        }
    }
    return 0;
}

// ---------- العمليات الثنائية ----------
NvValue nv_add(NvValue l, NvValue r, int line, int col) {
    // ج6: أي طرف كبير/كسر/مركب يأخذ المسار الدقيق.
    if (nv_is_j6_numeric(l) || nv_is_j6_numeric(r)) return nv_exact_arith(l, r, '+', line, col);
    if (nv_is_num_like(l) && nv_is_num_like(r)) return nv_num(nv_num_of(l) + nv_num_of(r));
    if (l.tag == NV_STR && r.tag == NV_STR) {
        size_t n = l.u.str->len + r.u.str->len;
        char *buf = (char *)malloc(n + 1);
        memcpy(buf, l.u.str->data, l.u.str->len);
        memcpy(buf + l.u.str->len, r.u.str->data, r.u.str->len);
        buf[n] = '\0';
        return nv_str_take(buf, n);
    }
    if (l.tag == NV_LIST && r.tag == NV_LIST) {
        NvValue out = nv_list_new();
        for (size_t i = 0; i < l.u.list->len; i++) nv_list_push_raw(out.u.list, l.u.list->items[i]);
        for (size_t i = 0; i < r.u.list->len; i++) nv_list_push_raw(out.u.list, r.u.list->items[i]);
        return out;
    }
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): دمج
    // قاموسين — اليمين يفوز عند تكرار المفتاح.
    if (l.tag == NV_DICT && r.tag == NV_DICT) {
        NvValue out = nv_dict_new();
        for (size_t i = 0; i < l.u.dict->len; i++)
            nv_dict_set_raw(out.u.dict, l.u.dict->entries[i].key, l.u.dict->entries[i].val);
        for (size_t i = 0; i < r.u.dict->len; i++)
            nv_dict_set_raw(out.u.dict, r.u.dict->entries[i].key, r.u.dict->entries[i].val);
        return out;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن جمع %s و %s", nv_type_name(l), nv_type_name(r));
}

static NvValue nv_arith(NvValue l, NvValue r, char op, int line, int col) {
    // ج6: أي طرف كبير/كسر/مركب يأخذ المسار الدقيق.
    if (nv_is_j6_numeric(l) || nv_is_j6_numeric(r)) return nv_exact_arith(l, r, op, line, col);
    if (!nv_is_num_like(l) || !nv_is_num_like(r)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "العملية الحسابية تتطلب عددين، وليس %s و %s",
                     nv_type_name(l), nv_type_name(r));
    }
    double a = nv_num_of(l), b = nv_num_of(r);
    switch (op) {
        case '-': return nv_num(a - b);
        case '*': return nv_num(a * b);
        case '/':
            if (b == 0.0) nv_error(NV_ERR_RUNTIME, "قسمة على صفر", line, col);
            return nv_num(a / b);
        case '%':
            if (b == 0.0) nv_error(NV_ERR_RUNTIME, "باقي القسمة على صفر", line, col);
            return nv_num(fmod(a, b));
    }
    return nv_nil();
}

NvValue nv_sub(NvValue l, NvValue r, int line, int col) { return nv_arith(l, r, '-', line, col); }
NvValue nv_mul(NvValue l, NvValue r, int line, int col) { return nv_arith(l, r, '*', line, col); }
NvValue nv_div(NvValue l, NvValue r, int line, int col) { return nv_arith(l, r, '/', line, col); }
NvValue nv_mod(NvValue l, NvValue r, int line, int col) { return nv_arith(l, r, '%', line, col); }

static int nv_compare(NvValue l, NvValue r, int line, int col) {
    // ج6: أي طرف كبير/كسر/مركب — الترتيب الدقيق،
    // والمركب بلا ترتيب فيسقط إلى خطأ المقارنة.
    if (l.tag == NV_BIGINT || l.tag == NV_RATIONAL || l.tag == NV_COMPLEX ||
        r.tag == NV_BIGINT || r.tag == NV_RATIONAL || r.tag == NV_COMPLEX) {
        int out = 0;
        if (nv_numeric_cmp(l, r, &out, line, col)) return out;
    }
    if (nv_is_num_like(l) && nv_is_num_like(r)) {
        double a = nv_num_of(l), b = nv_num_of(r);
        if (isnan(a) || isnan(b)) nv_error(NV_ERR_RUNTIME, "مقارنة غير صالحة", line, col);
        if (a < b) return -1;
        if (a > b) return 1;
        return 0;
    }
    if (l.tag == NV_NUM && r.tag == NV_NUM) {
        if (isnan(l.u.num) || isnan(r.u.num)) nv_error(NV_ERR_RUNTIME, "مقارنة غير صالحة", line, col);
        if (l.u.num < r.u.num) return -1;
        if (l.u.num > r.u.num) return 1;
        return 0;
    }
    if (l.tag == NV_STR && r.tag == NV_STR) {
        size_t n = l.u.str->len < r.u.str->len ? l.u.str->len : r.u.str->len;
        int c = memcmp(l.u.str->data, r.u.str->data, n);
        if (c) return c < 0 ? -1 : 1;
        if (l.u.str->len < r.u.str->len) return -1;
        if (l.u.str->len > r.u.str->len) return 1;
        return 0;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "المقارنة تتطلب عددين أو نصين، وليس %s و %s",
                 nv_type_name(l), nv_type_name(r));
}

int nv_lt(NvValue l, NvValue r, int line, int col) { return nv_compare(l, r, line, col) < 0; }
int nv_gt(NvValue l, NvValue r, int line, int col) { return nv_compare(l, r, line, col) > 0; }
int nv_le(NvValue l, NvValue r, int line, int col) { return nv_compare(l, r, line, col) <= 0; }
int nv_ge(NvValue l, NvValue r, int line, int col) { return nv_compare(l, r, line, col) >= 0; }

int nv_is_in(NvValue item, NvValue coll, int line, int col) {
    if (coll.tag == NV_LIST) {
        for (size_t i = 0; i < coll.u.list->len; i++) {
            if (nv_eq(item, coll.u.list->items[i])) return 1;
        }
        return 0;
    }
    // البندان 138 و139: العضوية في المترابطة
    // والتشكيلة كالقائمة.
    if (coll.tag == NV_TUPLE) {
        for (size_t i = 0; i < coll.u.tuple->len; i++) {
            if (nv_eq(item, coll.u.tuple->items[i])) return 1;
        }
        return 0;
    }
    if (coll.tag == NV_SET) {
        for (size_t i = 0; i < coll.u.set->len; i++) {
            if (nv_eq(item, coll.u.set->items[i])) return 1;
        }
        return 0;
    }
    if (coll.tag == NV_STR && item.tag == NV_STR) {
        if (item.u.str->len == 0) return 1;
        return strstr(coll.u.str->data, item.u.str->data) != NULL;
    }
    if (coll.tag == NV_DICT && item.tag == NV_STR) {
        return nv_dict_find(coll.u.dict, item.u.str->data) >= 0;
    }
    // ج6: العضوية في البت — 0 أو 1 فقط.
    if (coll.tag == NV_BITS) {
        if (item.tag != NV_NUM || (item.u.num != 0.0 && item.u.num != 1.0)) return 0;
        int want = item.u.num == 1.0;
        for (size_t i = 0; i < coll.u.bits->nbits; i++) {
            int bit = (coll.u.bits->bytes[i / 8] >> (7 - (i % 8))) & 1;
            if (bit == want) return 1;
        }
        return 0;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "في تتطلب قائمة أو نصا أو قاموسا على اليمين، وليس %s",
                 nv_type_name(coll));
}

NvValue nv_not(NvValue v, int line, int col) {
    if (v.tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليس تتطلب منطقيا، وليس %s", nv_type_name(v));
    }
    return nv_bool(!v.u.boolean);
}

NvValue nv_neg(NvValue v, int line, int col) {
    // ج6: السالب الدقيق للكبير والكسر والمركب.
    if (v.tag == NV_BIGINT) {
        char *n = nv_bigint_neg_str(v.u.bigint->digits);
        NvValue out = nv_bigint_cstr(n, line, col);
        free(n);
        return out;
    }
    if (v.tag == NV_RATIONAL) {
        char *n = nv_bigint_neg_str(v.u.rational->num->digits);
        NvValue out = nv_rational_new(nv_bigint_cstr(n, line, col),
                                      nv_bigint_cstr(v.u.rational->den->digits, line, col),
                                      line, col);
        free(n);
        return out;
    }
    if (v.tag == NV_COMPLEX) {
        return nv_complex_new(-v.u.complex.re, -v.u.complex.im);
    }
    if (v.tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "السالب يتطلب عددا، وليس %s", nv_type_name(v));
    }
    return nv_num(-v.u.num);
}

int nv_check_bool(NvValue v, const char *ctx, int line, int col) {
    if (v.tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "%s يجب أن يكون منطقيا، وليس %s", ctx, nv_type_name(v));
    }
    return v.u.boolean;
}

// فحص منطقي لمعاملات و/أو — صيغتها «تتطلب» لا «يجب أن يكون».
int nv_check_bool_op(NvValue v, const char *word, int line, int col) {
    if (v.tag != NV_BOOL) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "%s تتطلب منطقيا، وليس %s", word, nv_type_name(v));
    }
    return v.u.boolean;
}

// تحويل عدد إلى فهرس بلا سلوك غير معرّف: تحويل double
// خارج [0، 2^64) إلى size_t غير معرّف في C — وعلى
// x86-64 يلتف إلى 0 فيلتقط الفهرس الضخم العنصر الأول
// بصمت بدلا من خطأ خارج النطاق. التشبع عند SIZE_MAX
// يجعل الفحص التالي يرفضه، كما يفعل تحويل Rust
// المشبع (`as usize`) — فتتطابق رسالة الخطأ حرفيا.
static size_t nv_num_to_index(double x) {
    if (!(x < 18446744073709551616.0)) return SIZE_MAX;
    return (size_t)x;
}

// تحويل عدد إلى long long مشبعا كما يفعل `as i64` في
// Rust: خارج [−2^63، 2^63) يلتف بلا حارس إلى
// INT64_MIN فيفسد حدود مدى (نهاية تصير سالبة ضخمة
// فتفرغ النطاق، وخطوة تصير سالبة فتقلبه).
static long long nv_num_to_i64(double x) {
    if (x >= 9223372036854775808.0) return LLONG_MAX;
    if (x < -9223372036854775808.0) return LLONG_MIN;
    return (long long)x;
}

// ---------- الفهرسة والنقطة ----------
NvValue nv_index(NvValue target, NvValue index, int line, int col, int idx_line, int idx_col) {
    if (target.tag == NV_LIST && index.tag == NV_NUM) {
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error(NV_ERR_TYPE, "فهرس القائمة يجب أن يكون عددا صحيحا", idx_line, idx_col);
        }
        size_t idx = nv_num_to_index(x);
        if (idx >= target.u.list->len) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "فهرس خارج النطاق: %zu والقائمة طولها %zu",
                         idx, target.u.list->len);
        }
        return target.u.list->items[idx];
    }
    // البند 138: فهرسة المترابطة كالقائمة برسائلها
    // الخاصة (المترابطة بدلا من القائمة).
    if (target.tag == NV_TUPLE && index.tag == NV_NUM) {
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error(NV_ERR_TYPE, "فهرس المترابطة يجب أن يكون عددا صحيحا", idx_line, idx_col);
        }
        size_t idx = nv_num_to_index(x);
        if (idx >= target.u.tuple->len) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "فهرس خارج النطاق: %zu والمترابطة طولها %zu",
                         idx, target.u.tuple->len);
        }
        return target.u.tuple->items[idx];
    }
    // ج6: فهرسة البت — عدد صحيح، والناتج 0 أو 1.
    if (target.tag == NV_BITS && index.tag == NV_NUM) {
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error(NV_ERR_TYPE, "فهرس البت يجب أن يكون عددا صحيحا", idx_line, idx_col);
        }
        size_t idx = nv_num_to_index(x);
        if (idx >= target.u.bits->nbits) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "فهرس خارج النطاق: %zu والبت طوله %zu",
                         idx, target.u.bits->nbits);
        }
        int bit = (target.u.bits->bytes[idx / 8] >> (7 - (idx % 8))) & 1;
        return nv_num((double)bit);
    }
    if (target.tag == NV_DICT && index.tag == NV_STR) {
        int i = nv_dict_find(target.u.dict, index.u.str->data);
        if (i < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "مفتاح غير موجود: %s", index.u.str->data);
        }
        return target.u.dict->entries[i].val;
    }
    if (target.tag == NV_STR && index.tag == NV_NUM) {
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error(NV_ERR_TYPE, "فهرس النص يجب أن يكون عددا صحيحا", idx_line, idx_col);
        }
        size_t clen = 0;
        const char *p = nv_utf8_char_at(target.u.str->data, target.u.str->len, nv_num_to_index(x), &clen);
        if (!p) nv_error(NV_ERR_RUNTIME, "فهرس خارج النطاق في النص", idx_line, idx_col);
        return nv_str_new(p, clen);
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن الفهرسة على نوع %s بمفتاح من نوع %s",
                 nv_type_name(target), nv_type_name(index));
}

void nv_index_assign(NvValue *slot, NvValue index, NvValue val,
                     int idx_line, int idx_col, int tgt_line, int tgt_col) {
    NvValue target = *slot;
    if (target.tag == NV_LIST) {
        if (index.tag != NV_NUM) {
            nv_error_fmt(NV_ERR_TYPE, idx_line, idx_col, "فهرس القائمة يجب أن يكون عددا صحيحا، وليس %s",
                         nv_type_name(index));
        }
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error_fmt(NV_ERR_TYPE, idx_line, idx_col, "فهرس القائمة يجب أن يكون عددا صحيحا، وليس %s",
                         nv_type_name(index));
        }
        size_t idx = nv_num_to_index(x);
        if (idx >= target.u.list->len) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "فهرس خارج النطاق: %zu والقائمة طولها %zu",
                         idx, target.u.list->len);
        }
        target.u.list->items[idx] = nv_clone_value(val);
        nv_write_barrier((NvObj *)target.u.list, nv_obj_of(val));
        return;
    }
    if (target.tag == NV_DICT && index.tag == NV_STR) {
        nv_dict_set_raw(target.u.dict, index.u.str->data, val);
        return;
    }
    if (target.tag == NV_DICT) {
        nv_error_fmt(NV_ERR_TYPE, idx_line, idx_col, "مفتاح القاموس يجب أن يكون نصا، وليس %s",
                     nv_type_name(index));
    }
    nv_error_fmt(NV_ERR_TYPE, tgt_line, tgt_col, "لا يمكن تعيين عنصر في نوع %s", nv_type_name(target));
}

NvValue nv_dot(NvValue target, const char *name, int line, int col) {
    if (target.tag == NV_MODULE) {
        NvDict *ex = target.u.module->exports;
        int i = ex ? nv_dict_find(ex, name) : -1;
        if (i < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col,
                         "الاسم %s غير مصدر من وحدة %s. أضف صدّر %s في وحدة %s",
                         name, target.u.module->name, name, target.u.module->name);
        }
        return ex->entries[i].val;
    }
    if (target.tag == NV_INST) {
        NvInst *in = target.u.inst;
        int i = nv_dict_find(in->fields, name);
        if (i >= 0) return in->fields->entries[i].val;
        // طريقة مرتبطة (بحث في السلسلة المسطحة).
        NvDict *methods = in->klass->methods;
        int m = methods ? nv_dict_find(methods, name) : -1;
        if (m >= 0) return nv_bound_new(methods->entries[m].val, in);
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الكائن من فئة %s ليس له حقل أو طريقة باسم %s",
                     in->klass->name, name);
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع %s",
                 nv_type_name(target));
}

void nv_dot_assign(NvValue target, const char *name, NvValue val, int line, int col) {
    if (target.tag == NV_INST) {
        nv_dict_set_raw(target.u.inst->fields, name, val);
        return;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن تعيين حقل في نوع %s", nv_type_name(target));
}

// ---------- فحص الأنواع المعلنة ----------
// البند 8 — مطابقة نوع معلن بنيويا: اسم مسمى، أو عام
// `اسم[نوع، نوع]` (مثل `قائمة[عدد]` و`قاموس[نص، عدد]`).
// العام يطابقه أساسه زائد فحص العناصر: كل عنصر في
// القائمة يطابق معامل النوع الواحد، وكل قيمة في
// القاموس تطابق معامل النوع الثاني (ومفتاحه نص
// دائما). الفاصلة عربية `،` (بايتا UTF-8) أو لاتينية،
// والتداخل مسموح (`قائمة[قائمة[عدد]]`).
static int nv_match_type_at(const char **pp, NvValue v);

static void nv_skip_type_ws(const char **pp) {
    while (**pp == ' ' || **pp == '\t') (*pp)++;
}

// هل الحرف الحالي فاصلة نوع (عربية U+060C أو لاتينية)؟
static int nv_is_type_comma(const char *p) {
    if (p[0] == ',') return 1;
    // الفاصلة العربية `،` بايتا UTF-8: D8 8C.
    return (unsigned char)p[0] == 0xD8 && (unsigned char)p[1] == 0x8C;
}

// انسخ معامل نوع واحد (حتى فاصلة أو `]` على هذا المستوى)
// إلى مخزن مؤقت، مع احترام تداخل الأقواس.
static int nv_copy_type_arg(const char **pp, char *buf, size_t cap) {
    size_t n = 0;
    int depth = 0;
    while (**pp) {
        if (**pp == '[') depth++;
        if (**pp == ']') {
            if (depth == 0) break;
            depth--;
        }
        if (depth == 0 && nv_is_type_comma(*pp)) break;
        if (n + 1 < cap) buf[n++] = **pp;
        (*pp)++;
    }
    buf[n] = '\0';
    return n > 0;
}

static int nv_value_matches_decl(const char *decl, NvValue v) {
    if (!decl || strcmp(decl, "أي") == 0) return 1;
    const char *p = decl;
    return nv_match_type_at(&p, v);
}

static int nv_match_type_at(const char **pp, NvValue v) {
    nv_skip_type_ws(pp);
    char name[128];
    size_t n = 0;
    while (**pp && **pp != '[' && **pp != ']' && **pp != ',' && **pp != ' '
           && **pp != '\t' && !nv_is_type_comma(*pp)) {
        if (n + 1 < sizeof(name)) name[n++] = **pp;
        (*pp)++;
    }
    name[n] = '\0';
    if (n == 0) return 0;
    if (strcmp(nv_type_name(v), name) != 0) return 0;
    nv_skip_type_ws(pp);
    if (**pp != '[') return 1;
    (*pp)++; // [
    // معاملات النوع كنصوص (بحد أقصى 8).
    char args[8][256];
    int nargs = 0;
    for (;;) {
        nv_skip_type_ws(pp);
        if (nargs < 8 && nv_copy_type_arg(pp, args[nargs], sizeof(args[0]))) {
            nargs++;
        } else if (nargs >= 8) {
            return 0;
        } else {
            return 0;
        }
        nv_skip_type_ws(pp);
        if (nv_is_type_comma(*pp)) {
            (*pp) += (**pp == ',') ? 1 : 2;
            continue;
        }
        break;
    }
    if (**pp != ']') return 0;
    (*pp)++; // ]
    if (strcmp(name, "قائمة") == 0 && nargs == 1 && v.tag == NV_LIST) {
        NvList *l = v.u.list;
        for (size_t i = 0; i < l->len; i++) {
            if (!nv_value_matches_decl(args[0], l->items[i])) return 0;
        }
        return 1;
    }
    if (strcmp(name, "قاموس") == 0 && nargs == 2 && v.tag == NV_DICT) {
        // المفتاح نص دائما: معامل المفتاح نص أو أي فقط.
        if (strcmp(args[0], "نص") != 0 && strcmp(args[0], "أي") != 0) return 0;
        NvDict *d = v.u.dict;
        for (size_t i = 0; i < d->len; i++) {
            if (!nv_value_matches_decl(args[1], d->entries[i].val)) return 0;
        }
        return 1;
    }
    // الدفعة 3 من الـ579 — البندان 138 و139: فحص
    // عناصر المترابطة والتشكيلة كالقائمة (مرآة
    // matches_type_expr في Rust).
    if (strcmp(name, "مترابطة") == 0 && nargs == 1 && v.tag == NV_TUPLE) {
        NvList *l = v.u.tuple;
        for (size_t i = 0; i < l->len; i++) {
            if (!nv_value_matches_decl(args[0], l->items[i])) return 0;
        }
        return 1;
    }
    if (strcmp(name, "تشكيلة") == 0 && nargs == 1 && v.tag == NV_SET) {
        NvList *l = v.u.set;
        for (size_t i = 0; i < l->len; i++) {
            if (!nv_value_matches_decl(args[0], l->items[i])) return 0;
        }
        return 1;
    }
    // عام على غير حاوية: الأساس وحده يكفي.
    return 1;
}

void nv_check_type(NvValue v, const char *type_name, const char *what,
                   const char *func_name, int line, int col) {
    if (!type_name || strcmp(type_name, "أي") == 0) return;
    if (!nv_value_matches_decl(type_name, v)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "نوع غير متطابق: %s معلن %s، لكن القيمة %s",
                     what, type_name, nv_type_name(v));
    }
}

// فحص نوع معامل دالة (رسالة المعامل).
static void nv_check_param_type(NvValue v, const char *param_name, const char *pt,
                                const char *func_name, int is_method, int line, int col) {
    if (!pt || strcmp(pt, "أي") == 0) return;
    if (!nv_value_matches_decl(pt, v)) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "نوع غير متطابق: معامل %s في %s %s معلن %s، لكن القيمة %s",
                     param_name, is_method ? "الطريقة" : "الدالة", func_name, pt, nv_type_name(v));
    }
}

// ---------- الاستدعاء ----------
// البنود 109–112 (الاجتماع 83 §98): مطابقة
// الاستدعاء المشتركة — نفس الترتيب والرسائل في
// المفسر والآلة: (1) فيض موضعي بلا vararg، (2)
// مفتاحي مكرر، (3) ازدواج موضعي/مفتاحي، (4)
// مفتاحي مجهول، (5) مطلوب مفقود — ثم فحص الأنواع
// على القيم النهائية (بما فيها الافتراضيات،
// وتُنسَخ الافتراضية عميقا كل استدعاء كما في
// المفسر). تملأ `bound` (معاملات بالترتيب ثم
// جامع موضعي ثم جامع مفتاحي) و`bound_names`،
// وتعيد عددها. الحالة العادية (بلا افتراضيات/
// vararg/kwarg/مفتاحيات) تعطي رسالة العدد القديمة حرفيا.
static int nv_resolve_call_args(NvFunc *f, NvValue *args, int nargs,
                                const char **kw_names, NvValue *kw_vals, int nkwargs,
                                int is_method, int line, int col,
                                NvValue *bound, const char **bound_names,
                                const char **param_types) {
    // البنود 145–147: أنواع معاملات فعالة بعد
    // استبدال الربط (أو NULL فيعني المعلنة نفسها).
    if (!param_types) param_types = (const char **)f->param_types;
    const char *kind = is_method ? "الطريقة" : "الدالة";
    int nparams = f->nparams;
    int has_any_default = 0;
    for (int i = 0; i < nparams; i++) {
        if (f->has_default && f->has_default[i]) { has_any_default = 1; break; }
    }
    int plain = !has_any_default && !f->vararg && !f->kwarg && nkwargs == 0;
    if (plain) {
        if (nargs != nparams) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col,
                         "عدد المعاملات غير صحيح: %s %s تتوقع %d لكن أعطيت %d",
                         kind, f->name, nparams, nargs);
        }
        for (int i = 0; i < nparams; i++) {
            bound[i] = args[i];
            bound_names[i] = f->param_names[i];
        }
        for (int i = 0; i < nparams; i++) {
            if (param_types && param_types[i]) {
                nv_check_param_type(bound[i], bound_names[i], param_types[i], f->name, is_method, line, col);
            }
        }
        return nparams;
    }
    // (1) فيض موضعي بلا جامع.
    if (!f->vararg && nargs > nparams) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "عدد المعاملات غير صحيح: %s %s تتوقع %d لكن أعطيت %d",
                     kind, f->name, nparams, nargs + nkwargs);
    }
    // (2) مفتاحي مكرر.
    for (int i = 0; i < nkwargs; i++) {
        for (int j = 0; j < i; j++) {
            if (strcmp(kw_names[i], kw_names[j]) == 0) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي مكرر: %s", kw_names[i]);
            }
        }
    }
    NvValue *filled = (NvValue *)malloc(sizeof(NvValue) * (size_t)(nparams > 0 ? nparams : 1));
    char *is_filled = (char *)calloc((size_t)(nparams > 0 ? nparams : 1), 1);
    for (int i = 0; i < nargs && i < nparams; i++) {
        filled[i] = args[i];
        is_filled[i] = 1;
    }
    const char **extra_kw_names = (const char **)malloc(sizeof(char *) * (size_t)(nkwargs > 0 ? nkwargs : 1));
    NvValue *extra_kw_vals = (NvValue *)malloc(sizeof(NvValue) * (size_t)(nkwargs > 0 ? nkwargs : 1));
    int n_extra_kw = 0;
    for (int k = 0; k < nkwargs; k++) {
        int idx = -1;
        for (int i = 0; i < nparams; i++) {
            if (f->param_names[i] && strcmp(kw_names[k], f->param_names[i]) == 0) { idx = i; break; }
        }
        if (idx >= 0) {
            // (3) ازدواج موضعي/مفتاحي.
            if (is_filled[idx]) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "المعامل %s أعطي موضعيا ومفتاحيا", kw_names[k]);
            }
            filled[idx] = kw_vals[k];
            is_filled[idx] = 1;
        } else {
            // (4) مفتاحي مجهول — واسم الجامع نفسه
            // ليس معاملا مسمى (كمثل بايثون).
            if ((f->vararg && strcmp(kw_names[k], f->vararg) == 0) ||
                (f->kwarg && strcmp(kw_names[k], f->kwarg) == 0)) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[k]);
            }
            if (f->kwarg) {
                extra_kw_names[n_extra_kw] = kw_names[k];
                extra_kw_vals[n_extra_kw] = kw_vals[k];
                n_extra_kw++;
            } else {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[k]);
            }
        }
    }
    // (5) مطلوب مفقود — ثم الافتراضيات.
    int nbound = 0;
    for (int i = 0; i < nparams; i++) {
        if (is_filled[i]) {
            bound[nbound] = filled[i];
        } else if (f->has_default && f->has_default[i]) {
            bound[nbound] = nv_clone_value(f->defaults[i]);
        } else {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "المعامل %s مطلوب في %s %s",
                         f->param_names[i], kind, f->name);
        }
        bound_names[nbound] = f->param_names[i];
        nbound++;
    }
    // فحص الأنواع على القيم النهائية.
    for (int i = 0; i < nparams; i++) {
        if (param_types && param_types[i]) {
            nv_check_param_type(bound[i], bound_names[i], param_types[i], f->name, is_method, line, col);
        }
    }
    if (f->vararg) {
        NvValue list = nv_list_new();
        for (int i = nparams; i < nargs; i++) {
            nv_list_push_raw(list.u.list, args[i]);
        }
        bound[nbound] = list;
        bound_names[nbound] = f->vararg;
        nbound++;
    }
    if (f->kwarg) {
        NvValue dict = nv_dict_new();
        for (int k = 0; k < n_extra_kw; k++) {
            nv_dict_set_raw(dict.u.dict, extra_kw_names[k], extra_kw_vals[k]);
        }
        bound[nbound] = dict;
        bound_names[nbound] = f->kwarg;
        nbound++;
    }
    free(filled);
    free(is_filled);
    free(extra_kw_names);
    free(extra_kw_vals);
    return nbound;
}

// استبدال مملوك دائما (نسخة جديدة حتى بلا تغيير).
static char *nv_substitute_type_owned(const char *declared, char **names, char **values, int n) {
    if (!declared) return NULL;
    char *s = nv_substitute_type(declared, names, values, n);
    return s ? s : nv_strdup(declared);
}

static NvValue nv_call_func_impl(NvFunc *f, NvValue self, int has_self,
                                 NvValue *args, int nargs,
                                 const char **kw_names, NvValue *kw_vals, int nkwargs,
                                 int line, int col,
                                 int is_method) {
    // البنود 145–147: الربط الفعال — المخصَّص أولا،
    // وإلا الافتراضيات المعلنة (كل استدعاء يحل ربطه
    // كما في المفسر)؛ وربط الكائن للطرائق.
    char **eff_binding = f->type_binding;
    char **eff_binding_owned = NULL;
    if (!eff_binding && f->ntype_params > 0) {
        eff_binding_owned = nv_default_type_binding(f->tp_names, f->tp_bounds, f->tp_defaults,
                                                    f->ntype_params,
                                                    is_method ? "الطريقة" : "الدالة", f->name,
                                                    line, col);
        eff_binding = eff_binding_owned;
    }
    // ربط الفئة (لطرائق الكائن) — يُستبدل بعد ربط
    // الدالة، فيغلب ظل الطريقة.
    char **cls_names = NULL;
    char **cls_binding = NULL;
    int cls_n = 0;
    if (has_self && self.tag == NV_INST && self.u.inst && self.u.inst->type_binding &&
        self.u.inst->klass) {
        cls_names = self.u.inst->klass->tp_names;
        cls_binding = self.u.inst->type_binding;
        cls_n = self.u.inst->klass->ntype_params;
    }
    // أنواع معاملات فعالة + نوع إرجاع فعال.
    char **eff_types = NULL;
    if (f->nparams > 0) {
        eff_types = (char **)malloc((size_t)f->nparams * sizeof(char *));
        for (int i = 0; i < f->nparams; i++) {
            const char *decl = (f->param_types && f->param_types[i]) ? f->param_types[i] : NULL;
            if (!decl) {
                eff_types[i] = NULL;
            } else {
                char *s1 = nv_substitute_type_owned(decl, f->tp_names, eff_binding, f->ntype_params);
                char *s2 = nv_substitute_type_owned(s1, cls_names, cls_binding, cls_n);
                free(s1);
                eff_types[i] = s2;
            }
        }
    }
    char *eff_ret = NULL;
    if (f->return_type) {
        char *r1 = nv_substitute_type_owned(f->return_type, f->tp_names, eff_binding, f->ntype_params);
        eff_ret = nv_substitute_type_owned(r1, cls_names, cls_binding, cls_n);
        free(r1);
    }
    // البنود 109–112: المطابقة المشتركة أولا.
    NvValue *bound = (NvValue *)malloc(sizeof(NvValue) * (size_t)(f->nparams + 2));
    const char **bound_names = (const char **)malloc(sizeof(char *) * (size_t)(f->nparams + 2));
    int nbound = nv_resolve_call_args(f, args, nargs, kw_names, kw_vals, nkwargs,
                                      is_method, line, col, bound, bound_names,
                                      (const char **)eff_types);
    // حد العمق قبل دفع النطاق.
    if (nv_call_depth >= NV_MAX_CALL_DEPTH) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "تجاوز حد عمق الاستدعاء (%d). أوقف الاستدعاء الذاتي أو قلل عمقه",
                     NV_MAX_CALL_DEPTH);
    }
    // نطاق الاستدعاء: دالة وحدة -> استنساخ جديد من نطاق
    // الوحدة؛ وإلا السلسلة الملتقَطة.
    NvScope *parent = NULL;
    if (f->module) {
        parent = nv_module_call_scope(f);
    } else {
        parent = f->captured;
    }
    NvScope *frame = nv_scope_new(parent);
    nv_frames_push(frame);
    // نفس (للميتدات) ثم المعاملات المحلولة.
    if (has_self) {
        nv_scope_define(frame, "نفس", self);
    }
    for (int i = 0; i < nbound; i++) {
        nv_scope_define(frame, bound_names[i], bound[i]);
    }
    free(bound);
    free(bound_names);
    nv_call_depth++;
    NvFlow saved_flow = nv_flow;
    nv_flow = NV_FLOW_NORMAL;
    // ملف المتصل يُستعاد بعد عودة الدالة: فحوص ما بعد
    // الاستدعاء (نوع الإرجاع) تُسنَد إلى ملف المتصل
    // كما في المفسر؛ والخطأ داخل الدالة يحتفظ بملفها
    // (القفز يتخطى الاستعادة عمدا).
    int saved_file = nv_cur_file_idx;
    NvValue result = f->cfunc(frame, args, nargs, line, col);
    nv_cur_file_idx = saved_file;
    NvFlow exit_flow = nv_flow;
    nv_flow = saved_flow;
    nv_call_depth--;
    nv_frames_pop();
    // ملاحظة: النطاق frame يبقى كائنا جامعيا (قد تلتقطه
    // إغلاقات أُنشئت داخله) — لا تحرير يدوي.
    if (exit_flow == NV_FLOW_RETURN) {
        NvValue v = result;
        if (eff_ret && strcmp(eff_ret, "أي") != 0 &&
            !nv_value_matches_decl(eff_ret, v)) {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "نوع غير متطابق: %s %s معلنة أن ترجع %s، لكن رجعت %s",
                         is_method ? "الطريقة" : "الدالة", f->name, eff_ret, nv_type_name(v));
        }
        // البنود 145–147: تحرير الأنواع الفعالة.
        if (eff_types) {
            for (int i = 0; i < f->nparams; i++) free(eff_types[i]);
            free(eff_types);
        }
        free(eff_ret);
        free(eff_binding_owned);
        return v;
    }
    // بلا إرجاع صريح: لاشيء — إلا إن كان نوع الإرجاع
    // المعلن غير لاشيء/أي.
    if (eff_ret && strcmp(eff_ret, "أي") != 0 &&
        strcmp(eff_ret, "لاشيء") != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "نوع غير متطابق: %s %s معلنة أن ترجع %s، لكن لم ترجع شيئا",
                     is_method ? "الطريقة" : "الدالة", f->name, eff_ret);
    }
    if (eff_types) {
        for (int i = 0; i < f->nparams; i++) free(eff_types[i]);
        free(eff_types);
    }
    free(eff_ret);
    free(eff_binding_owned);
    return nv_nil();
}

// البنود 145–147: خصّص دالة/فئة بمعاملات نوع —
// تنسخ القيمة مع ربط محلول مفحوص. المدمجة بلا
// معاملات نوع فأي تخصيص زائد؛ وغير الدالة/الفئة
// خطأ نوع برسالة المفسر.
NvValue nv_specialize(NvValue v, const char *type_args, int line, int col) {
    if (v.tag == NV_FUNC) {
        NvFunc *src = v.u.func;
        char **binding = nv_resolve_type_binding(src->tp_names, src->tp_bounds, src->tp_defaults,
                                                 src->ntype_params, type_args, "الدالة", src->name,
                                                 line, col);
        NvFunc *f = (NvFunc *)nv_alloc(sizeof(NvFunc), NV_OBJ_FUNC);
        NvObj saved_hdr = f->hdr;
        *f = *src;
        f->hdr = saved_hdr;
        f->name = nv_strdup(src->name);
        f->module = src->module ? nv_strdup(src->module) : NULL;
        f->return_type = src->return_type ? nv_strdup(src->return_type) : NULL;
        // نسخ عميق لما يحرره الجامع لكل نسخة على حدة؛
        // ومشاركة ما لا يحرره (الأسماء والافتراضيات
        // والجامعان والنطاق الملتقَط).
        f->param_types = nv_dup_str_array(src->param_types, src->nparams);
        f->tp_names = nv_dup_str_array(src->tp_names, src->ntype_params);
        f->tp_bounds = nv_dup_str_array(src->tp_bounds, src->ntype_params);
        f->tp_defaults = nv_dup_str_array(src->tp_defaults, src->ntype_params);
        f->type_binding = binding;
        NvValue out;
        out.tag = NV_FUNC;
        out.u.func = f;
        return out;
    }
    if (v.tag == NV_CLASS) {
        NvClass *src = v.u.klass;
        char **binding = nv_resolve_type_binding(src->tp_names, src->tp_bounds, src->tp_defaults,
                                                 src->ntype_params, type_args, "الفئة", src->name,
                                                 line, col);
        NvClass *c = (NvClass *)nv_alloc(sizeof(NvClass), NV_OBJ_CLASS);
        NvObj saved_hdr = c->hdr;
        *c = *src;
        c->hdr = saved_hdr;
        c->name = nv_strdup(src->name);
        c->interfaces = nv_dup_str_array(src->interfaces, src->ninterfaces);
        c->tp_names = nv_dup_str_array(src->tp_names, src->ntype_params);
        c->tp_bounds = nv_dup_str_array(src->tp_bounds, src->ntype_params);
        c->tp_defaults = nv_dup_str_array(src->tp_defaults, src->ntype_params);
        c->type_binding = binding;
        NvValue out;
        out.tag = NV_CLASS;
        out.u.klass = c;
        return out;
    }
    if (v.tag == NV_NATIVE) {
        // المدمجة بلا معاملات نوع — فأي تخصيص زائد.
        nv_resolve_type_binding(NULL, NULL, NULL, 0, type_args, "الدالة",
                                v.u.native->name, line, col);
        return v;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "القيمة %s ليست دالة", nv_type_name(v));
    return nv_nil();
}

NvValue nv_call_at_kw(NvValue callee, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                   int callee_line, int callee_col) {
    // دفاعي (الجولة 3، اجتماع 38 §53): لا يصل مؤشر
    // فارغ أبدا إلى دالة أصلية. المولّد يمرّر NULL
    // للاستدعاءات بلا وسائط (مثل شبكة.استقبل())، وهو
    // الفرق الهيكلي الوحيد بين هذا الاستدعاء المنهار
    // على ويندوز وبين كل الاستدعاءات الناجحة — وكل
    // الدوال الأصلية تفحص العدد قبل لمس الوسائط، فلا
    // سلوك يتغير على أي منصة؛ المصفوفة الثابتة الفارغة
    // تزيل الفارق وحده. السطر الدقيق للانهيار لم يُعزل
    // (انظر CATALOG-34)؛ وهذا تحصين لا ادعاء إصلاح.
    static NvValue nv_empty_args[1];
    if (!args) args = nv_empty_args;
    // البنود 145–147: جذّر المستدعى أيضا أثناء
    // الاستدعاء (قد تكون نسخة مخصَّصة حديثة بلا
    // جذر آخر)؛ ثم الوسائط (الإطار ينسخها بعدها).
    int base = (int)nv_root_len;
    size_t cbase = nv_root_push_idx();
    *nv_root_slot(cbase) = callee;
    for (int i = 0; i < nargs; i++) {
        size_t idx = nv_root_push_idx();
        *nv_root_slot(idx) = args[i];
    }
    for (int i = 0; i < nkwargs; i++) {
        size_t idx = nv_root_push_idx();
        *nv_root_slot(idx) = kw_vals[i];
    }
    NvValue result;
    switch (callee.tag) {
        case NV_FUNC:
            result = nv_call_func_impl(callee.u.func, nv_nil(), 0, args, nargs, kw_names, kw_vals, nkwargs, line, col, 0);
            break;
        case NV_NATIVE:
            // البنود 109–112: المدمجات ترفض المفتاحيات.
            if (nkwargs > 0) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[0]);
            }
            result = callee.u.native->fn(args, nargs, line, col);
            break;
        case NV_BOUND: {
            NvBound *b = callee.u.bound;
            NvValue self = nv_nil();
            self.tag = NV_INST;
            self.u.inst = b->inst;
            if (b->func.tag == NV_FUNC) {
                result = nv_call_func_impl(b->func.u.func, self, 1, args, nargs, kw_names, kw_vals, nkwargs, line, col, 1);
            } else if (b->func.tag == NV_NATIVE) {
                if (nkwargs > 0) {
                    nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[0]);
                }
                result = b->func.u.native->fn(args, nargs, line, col);
            } else {
                nv_error_fmt(NV_ERR_TYPE, callee_line, callee_col, "القيمة %s ليست دالة", nv_type_name(b->func));
            }
            break;
        }
        default:
            nv_error_fmt(NV_ERR_TYPE, callee_line, callee_col, "القيمة %s ليست دالة", nv_type_name(callee));
    }
    nv_root_pop_n((int)nv_root_len - base);
    return result;
}

NvValue nv_call_at(NvValue callee, NvValue *args, int nargs, int line, int col,
                   int callee_line, int callee_col) {
    return nv_call_at_kw(callee, args, nargs, NULL, NULL, 0, line, col, callee_line, callee_col);
}

NvValue nv_call_kw(NvValue callee, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    return nv_call_at_kw(callee, args, nargs, kw_names, kw_vals, nkwargs, line, col, line, col);
}

NvValue nv_call(NvValue callee, NvValue *args, int nargs, int line, int col) {
    return nv_call_at(callee, args, nargs, line, col, line, col);
}

// استدعاء باسم معرّف: النطاق أولا، ثم المدمجة.
// الدفعة الأخيرة DEF (البنود 77 و78): الاستدعاء
// بالاسم مع بحث موجَّه بالإعلان — الدالة نفسها
// تستقبل دالة البحث (الأقرب/الجذر/المحيط).
// البنود 109–112: النسخة المفتاحية تمرّر المفتاحيات.
static NvValue nv_call_ident_at_with_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col, NvCell *(*lookup_fn)(NvScope *, const char *)) {
    NvCell *cell = lookup_fn(scope, name);
    if (cell) {
        NvValue v = cell->val;
        if (v.tag == NV_CLASS) {
            nv_error_fmt(NV_ERR_TYPE, callee_line, callee_col,
                         "استخدم أنشئ لإنشاء كائن من فئة %s، مثل أنشئ %s()",
                         v.u.klass->name, v.u.klass->name);
        }
        return nv_call_at_kw(v, args, nargs, kw_names, kw_vals, nkwargs, line, col, callee_line, callee_col);
    }
    if (nv_is_builtin(name)) {
        // البنود 109–112: المدمجات ترفض المفتاحيات.
        if (nkwargs > 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[0]);
        }
        // اجتماع 56 §71 (الميزة 16): نطاق الاستدعاء
        // الحالي للمدمجة (لقراءة قيمة متغير في
        // `انتقل`)، ويُستعاد بعده.
        NvScope *prev_scope = nv_current_call_scope;
        nv_current_call_scope = scope;
        NvValue r = nv_call_builtin(name, args, nargs, line, col);
        nv_current_call_scope = prev_scope;
        return r;
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "دالة غير معرفة: %s", name);
}

static NvValue nv_call_ident_at_with(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col, NvCell *(*lookup_fn)(NvScope *, const char *)) {
    return nv_call_ident_at_with_kw(scope, name, args, nargs, NULL, NULL, 0, line, col, callee_line, callee_col, lookup_fn);
}

NvValue nv_call_ident_at(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with(scope, name, args, nargs, line, col, callee_line, callee_col, nv_scope_lookup_cell);
}

NvValue nv_call_ident_at_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with_kw(scope, name, args, nargs, kw_names, kw_vals, nkwargs, line, col, callee_line, callee_col, nv_scope_lookup_cell);
}

NvValue nv_call_ident_at_global(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with(scope, name, args, nargs, line, col, callee_line, callee_col, nv_scope_lookup_cell_global);
}

NvValue nv_call_ident_at_global_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with_kw(scope, name, args, nargs, kw_names, kw_vals, nkwargs, line, col, callee_line, callee_col, nv_scope_lookup_cell_global);
}

NvValue nv_call_ident_at_nonlocal(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with(scope, name, args, nargs, line, col, callee_line, callee_col, nv_scope_lookup_cell_nonlocal);
}

NvValue nv_call_ident_at_nonlocal_kw(NvScope *scope, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         int callee_line, int callee_col) {
    return nv_call_ident_at_with_kw(scope, name, args, nargs, kw_names, kw_vals, nkwargs, line, col, callee_line, callee_col, nv_scope_lookup_cell_nonlocal);
}

NvValue nv_call_ident(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col) {
    return nv_call_ident_at(scope, name, args, nargs, line, col, line, col);
}

// إنشاء كائن بالاسم (بحث النطاق أولا برسائل المفسر).
// الدفعة الأخيرة DEF (البنود 77 و78): إنشاء كائن
// بالاسم مع بحث موجَّه بالإعلان.
static NvValue nv_create_by_name_with_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                         NvCell *(*lookup_fn)(NvScope *, const char *)) {
    NvCell *cell = lookup_fn(scope, class_name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "فئة غير معرفة: %s", class_name);
    }
    NvValue class_val = cell->val;
    if (class_val.tag != NV_CLASS) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "%s ليست فئة، بل %s", class_name, nv_type_name(class_val));
    }
    return nv_create_kw(class_val, args, nargs, kw_names, kw_vals, nkwargs, line, col);
}

static NvValue nv_create_by_name_with(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col,
                         NvCell *(*lookup_fn)(NvScope *, const char *)) {
    return nv_create_by_name_with_kw(scope, class_name, args, nargs, NULL, NULL, 0, line, col, lookup_fn);
}

NvValue nv_create_by_name(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col) {
    return nv_create_by_name_with(scope, class_name, args, nargs, line, col, nv_scope_lookup_cell);
}

NvValue nv_create_by_name_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    return nv_create_by_name_with_kw(scope, class_name, args, nargs, kw_names, kw_vals, nkwargs, line, col, nv_scope_lookup_cell);
}

NvValue nv_create_by_name_global(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col) {
    return nv_create_by_name_with(scope, class_name, args, nargs, line, col, nv_scope_lookup_cell_global);
}

NvValue nv_create_by_name_global_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    return nv_create_by_name_with_kw(scope, class_name, args, nargs, kw_names, kw_vals, nkwargs, line, col, nv_scope_lookup_cell_global);
}

NvValue nv_create_by_name_nonlocal(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col) {
    return nv_create_by_name_with(scope, class_name, args, nargs, line, col, nv_scope_lookup_cell_nonlocal);
}

NvValue nv_create_by_name_nonlocal_kw(NvScope *scope, const char *class_name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    return nv_create_by_name_with_kw(scope, class_name, args, nargs, kw_names, kw_vals, nkwargs, line, col, nv_scope_lookup_cell_nonlocal);
}

// إنشاء كائن من قيمة فئة.
NvValue nv_create_kw(NvValue class_val, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    if (class_val.tag != NV_CLASS) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليست فئة، بل %s", nv_type_name(class_val));
    }
    NvClass *klass = class_val.u.klass;
    NvValue inst = nv_inst_new(klass);
    // البنود 145–147: ربط الفئة ينتقل إلى الكائن
    // قبل المنشئ — المخصَّص كما هو، وبلا تخصيص
    // تُحلّ الافتراضيات المعلنة (كما في المفسر).
    if (klass->type_binding) {
        inst.u.inst->type_binding = nv_dup_str_array(klass->type_binding, klass->ntype_params);
    } else if (klass->ntype_params > 0) {
        char **def = nv_default_type_binding(klass->tp_names, klass->tp_bounds, klass->tp_defaults,
                                             klass->ntype_params, "الفئة", klass->name, line, col);
        inst.u.inst->type_binding = nv_dup_str_array(def, klass->ntype_params);
        free(def);
    }
    // جذّر الفئة والكائن أثناء المنشئ (قد يجمع
    // القمامة؛ والفئة المخصَّصة نسخة جديدة).
    size_t cbase = nv_root_push_idx();
    *nv_root_slot(cbase) = class_val;
    size_t ibase = nv_root_push_idx();
    *nv_root_slot(ibase) = inst;
    NvDict *methods = klass->methods;
    int ctor = methods ? nv_dict_find(methods, "أنشئ") : -1;
    if (ctor >= 0) {
        NvValue ctor_fn = methods->entries[ctor].val;
        if (ctor_fn.tag == NV_FUNC) {
            nv_call_func_impl(ctor_fn.u.func, inst, 1, args, nargs, kw_names, kw_vals, nkwargs, line, col, 1);
        } else if (ctor_fn.tag == NV_NATIVE) {
            if (nkwargs > 0) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[0]);
            }
            ctor_fn.u.native->fn(args, nargs, line, col);
        }
    } else if (nargs + nkwargs > 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "الفئة %s ليس لها منشئ أنشئ، لكن أعطيت %d معاملات",
                     klass->name, nargs + nkwargs);
    }
    nv_root_pop_n((int)nv_root_len - (int)cbase);
    return inst;
}

NvValue nv_create(NvValue class_val, NvValue *args, int nargs, int line, int col) {
    return nv_create_kw(class_val, args, nargs, NULL, NULL, 0, line, col);
}

// ---------- الفئات والواجهات (مساعدات المولد) ----------
// أنشئ فئة (مع نسخ طرق الأب مسطحة).
NvValue nv_class_begin(const char *name, NvValue base_val, int line, int col, const char *type_params) {
    NvClass *base = NULL;
    if (base_val.tag == NV_NIL) {
        base = NULL;
    } else if (base_val.tag == NV_CLASS) {
        base = base_val.u.klass;
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليست فئة، بل %s", nv_type_name(base_val));
    }
    return nv_class_new(name, base, type_params);
}

// أضف طريقة (تغلب على الموروثة).
void nv_class_add_method(NvValue klass, const char *name, NvValue func) {
    nv_dict_set_raw(klass.u.klass->methods, name, func);
}

// تحقق من واجهة وأضفها (عند التعريف).
void nv_class_add_interface(NvValue klass, const char *iname, NvValue iface_val, int line, int col) {
    if (iface_val.tag != NV_IFACE) {
        if (iface_val.tag == NV_NIL) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "واجهة غير معرفة: %s", iname);
        }
        nv_error_fmt(NV_ERR_TYPE, line, col, "%s ليست واجهة، بل %s", iname, nv_type_name(iface_val));
    }
    NvIface *in = iface_val.u.iface;
    NvClass *c = klass.u.klass;
    for (int i = 0; i < in->nmethods; i++) {
        const char *mname = in->methods[i];
        int m = nv_dict_find(c->methods, mname);
        if (m < 0) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col,
                         "الفئة %s لا تنفذ طريقة %s المطلوبة من واجهة %s",
                         c->name, mname, in->name);
        }
        NvValue mv = c->methods->entries[m].val;
        int arity = -1;
        if (mv.tag == NV_FUNC) arity = mv.u.func->nparams;
        if (arity != in->arity[i]) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col,
                         "الفئة %s تنفذ طريقة %s من واجهة %s بعدد معاملات غير صحيح: تتوقع %d لكن وجد %d",
                         c->name, mname, in->name, in->arity[i], arity);
        }
    }
    c->interfaces = (char **)realloc(c->interfaces, (size_t)(c->ninterfaces + 1) * sizeof(char *));
    c->interfaces[c->ninterfaces++] = nv_strdup(iname);
}

// أنشئ واجهة واملأ طرقها.
NvValue nv_iface_begin(const char *name) {
    return nv_iface_new(name);
}

void nv_iface_add_method(NvValue iface, const char *name, int arity) {
    NvIface *in = iface.u.iface;
    in->methods = (char **)realloc(in->methods, (size_t)(in->nmethods + 1) * sizeof(char *));
    in->arity = (int *)realloc(in->arity, (size_t)(in->nmethods + 1) * sizeof(int));
    in->methods[in->nmethods] = nv_strdup(name);
    in->arity[in->nmethods] = arity;
    in->nmethods++;
}

// ---------- الطباعة والإدخال ----------
void nv_print_args(NvValue *args, int nargs) {
    for (int i = 0; i < nargs; i++) {
        if (i) fputc(' ', stdout);
        char *s = nv_fmt_value(args[i]);
        fputs(s, stdout);
        free(s);
    }
    fputc('\n', stdout);
    // اجتماع 33: البث الحي يمتد إلى الأصلي حرفيا —
    // فرّغ بعد كل طباعة، فيتداخل مخرج العمليات
    // المساعدة بالترتيب نفسه كما في المفسر، ويزول
    // الحد الصادق رقم 8 من تقرير المهمة 29.
    fflush(stdout);
}

#ifdef _WIN32
// قراءة سطر من stdin بديلا عن getline غير الموجودة في MinGW؛
// السلوك نفسه: يعيد عدد البايتات المقروءة أو -1 عند EOF/خطأ.
static ssize_t nv_getline(char **lineptr, size_t *n, FILE *stream) {
    size_t len = 0;
    size_t cap = 0;
    char *buf = NULL;
    int c;
    if (!lineptr || !n || !stream) return -1;
    if (*lineptr) { buf = *lineptr; cap = *n; }
    while ((c = fgetc(stream)) != EOF) {
        if (len + 2 > cap) {
            size_t ncap = cap ? cap * 2 : 128;
            char *nb = realloc(buf, ncap);
            if (!nb) { free(buf); return -1; }
            buf = nb; cap = ncap;
        }
        buf[len++] = (char)c;
        if (c == '\n') break;
    }
    if (len == 0 && c == EOF) { *lineptr = buf; *n = cap; return -1; }
    if (len + 1 > cap) {
        char *nb = realloc(buf, len + 1);
        if (!nb) { free(buf); return -1; }
        buf = nb; cap = len + 1;
    }
    buf[len] = '\0';
    *lineptr = buf; *n = cap;
    return (ssize_t)len;
}

// المسار المقنّن بديلا عن realpath غير الموجودة في MinGW؛
// تحل الروابط الرمزية عبر GetFinalPathNameByHandleA، ثم
// تُزال بادئة \\?\ وتُحوَّل \ إلى / ليبقى العرض بشرطات
// مائلة كما في المحرك — ففحص بادئة مجلد البرنامج سليم.
static char *nv_win32_realpath(const char *path) {
    HANDLE h = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, NULL);
    char *out = NULL;
    DWORD len;
    if (h == INVALID_HANDLE_VALUE) return NULL;
    len = GetFinalPathNameByHandleA(h, NULL, 0, FILE_NAME_NORMALIZED);
    if (len > 0) {
        char *buf = malloc(len + 1);
        if (buf && GetFinalPathNameByHandleA(h, buf, len + 1, FILE_NAME_NORMALIZED) > 0) {
            size_t i;
            if (strncmp(buf, "\\\\?\\UNC\\", 8) == 0) {
                memmove(buf + 2, buf + 8, strlen(buf + 8) + 1);
                buf[0] = '\\'; buf[1] = '\\';
            } else if (strncmp(buf, "\\\\?\\", 4) == 0) {
                memmove(buf, buf + 4, strlen(buf + 4) + 1);
            }
            for (i = 0; buf[i]; i++) if (buf[i] == '\\') buf[i] = '/';
            out = nv_strdup(buf);
        }
        free(buf);
    }
    CloseHandle(h);
    return out;
}

// نص خطأ ويندوز (Win32) بديلا عن strerror — رسائل Win32
// لا تصل إلى errno، فتُقرأ عبر GetLastError وFormatMessageA.
static char *nv_win32_error_str(void) {
    DWORD code = GetLastError();
    char buf[512];
    DWORD n = FormatMessageA(FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS, NULL, code, 0, buf, sizeof(buf), NULL);
    if (n == 0) return nv_strdup("خطأ ويندوز غير معروف");
    while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r' || buf[n - 1] == ' ')) buf[--n] = '\0';
    return nv_strdup(buf);
}

// البحث عن بايتات داخل بايتات بديلا عن memmem غير المعلنة في MinGW.
static const void *nv_memmem(const void *haystack, size_t haystack_len, const void *needle, size_t needle_len) {
    const unsigned char *h = haystack;
    const unsigned char *nd = needle;
    size_t i;
    if (needle_len == 0) return haystack;
    if (haystack_len < needle_len) return NULL;
    for (i = 0; i + needle_len <= haystack_len; i++) {
        if (h[i] == nd[0] && memcmp(h + i, nd, needle_len) == 0) return h + i;
    }
    return NULL;
}
#endif // _WIN32 — نهاية التوافقيات (lstat في niva_rt.h)

NvValue nv_input(NvValue *args, int nargs, int line, int col) {
    if (nargs > 1) {
        nv_error(NV_ERR_RUNTIME, "ادخل تتوقع 0 أو 1 معاملات", line, col);
    }
    if (nargs == 1) {
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "ادخل يتطلب نصا للسؤال، وليس %s",
                         nv_type_name(args[0]));
        }
        fputs(args[0].u.str->data, stdout);
        fflush(stdout);
    }
    char *line_buf = NULL;
    size_t cap = 0;
#ifdef _WIN32
    ssize_t n = nv_getline(&line_buf, &cap, stdin);
#else
    ssize_t n = getline(&line_buf, &cap, stdin);
#endif
    if (n < 0) {
        free(line_buf);
        return nv_str_cstr("");
    }
    // أزل نهاية السطر (\n و \r\n).
    while (n > 0 && (line_buf[n - 1] == '\n' || line_buf[n - 1] == '\r')) {
        line_buf[--n] = '\0';
    }
    NvValue v = nv_str_new(line_buf, (size_t)n);
    free(line_buf);
    return v;
}

// ================= الجزء 3: الدوال المدمجة وسياسة الملفات =================

// ---------- أخطاء نظام التشغيل بصيغة Rust ----------
// Rust يعرض io::Error كـ "رسالة (os error N)".
static void nv_os_error_str(int e, char *buf, size_t bufsz) {
    snprintf(buf, bufsz, "%s (os error %d)", strerror(e), e);
}

// ---------- سياسة الملفات ----------
// تطبيع معجمي: أزل . و.. (مثل FilePolicy::normalize).
static char *nv_normalize_lexical(const char *path) {
    // قسّم إلى مكونات وعالجها.
    int absolute = (path[0] == '/');
    char *copy = nv_strdup(path);
    char **parts = NULL;
    size_t nparts = 0, capp = 0;
    char *save = NULL;
    char *tok = strtok_r(copy, "/", &save);
    while (tok) {
        if (strcmp(tok, ".") == 0) {
            // تخط
        } else if (strcmp(tok, "..") == 0) {
            if (nparts > 0) nparts--;
        } else {
            if (nparts == capp) {
                capp = capp ? capp * 2 : 16;
                parts = (char **)realloc(parts, capp * sizeof(char *));
            }
            parts[nparts++] = tok;
        }
        tok = strtok_r(NULL, "/", &save);
    }
    size_t total = absolute ? 1 : 0;
    for (size_t i = 0; i < nparts; i++) total += strlen(parts[i]) + 1;
    if (total == 0) total = 1;
    char *out = (char *)malloc(total + 1);
    size_t w = 0;
    if (absolute) out[w++] = '/';
    for (size_t i = 0; i < nparts; i++) {
        size_t l = strlen(parts[i]);
        memcpy(out + w, parts[i], l);
        w += l;
        out[w++] = '/';
    }
    if (w > 1 && out[w - 1] == '/') w--;
    if (w == 0) out[w++] = '.';
    out[w] = '\0';
    free(parts);
    free(copy);
    return out;
}

// هل المسار (مطلق مطبّع) داخل القاعدة (مطلقة مطبعة)؟
static int nv_path_starts_with(const char *path, const char *base) {
    size_t bl = strlen(base);
    if (strncmp(path, base, bl) != 0) return 0;
    if (base[bl - 1] == '/') return 1;
    return path[bl] == '\0' || path[bl] == '/';
}

// canonicalize مع لاحقة (أطول بادئة موجودة + الباقي).
static char *nv_canonical_with_suffix(const char *path) {
    char *work = nv_strdup(path);
    char **suffix = NULL;
    size_t nsuf = 0, caps = 0;
    struct stat st;
    while (stat(work, &st) != 0) {
        char *slash = strrchr(work, '/');
        if (!slash || slash == work) break;
        if (nsuf == caps) {
            caps = caps ? caps * 2 : 8;
            suffix = (char **)realloc(suffix, caps * sizeof(char *));
        }
        suffix[nsuf++] = nv_strdup(slash + 1);
        *slash = '\0';
        if (work[0] == '\0') {
            free(work);
            work = nv_strdup("/");
            break;
        }
    }
#ifdef _WIN32
    char *canon = nv_win32_realpath(work);
#else
    char *canon = realpath(work, NULL);
#endif
    if (!canon) canon = nv_strdup(work);
    free(work);
    // أعد اللاحقة بترتيبها.
    size_t total = strlen(canon) + 1;
    for (size_t i = 0; i < nsuf; i++) total += strlen(suffix[i]) + 1;
    char *out = (char *)malloc(total + 1);
    strcpy(out, canon);
    size_t w = strlen(out);
    for (size_t i = nsuf; i-- > 0;) {
        if (w == 0 || out[w - 1] != '/') out[w++] = '/';
        size_t l = strlen(suffix[i]);
        memcpy(out + w, suffix[i], l);
        w += l;
        out[w] = '\0';
    }
    for (size_t i = 0; i < nsuf; i++) free(suffix[i]);
    free(suffix);
    free(canon);
    return out;
}

// توسيع ~ و$نيفا (اجتماع 45 §59، البندان 35 و38):
// ~ هو البيت (`program_dir/home`)، و~/س تحته؛
// $نيفا هو مجلد البرنامج، و$نيفا/س تحته.
static char *nv_expand_home(const char *path) {
    if (strcmp(path, "~") == 0) return nv_strdup(nv_home_dir);
    if (strncmp(path, "~/", 2) == 0) {
        size_t n = strlen(nv_home_dir) + strlen(path + 1) + 1;
        char *out = (char *)malloc(n);
        snprintf(out, n, "%s%s", nv_home_dir, path + 1);
        return out;
    }
    if (strcmp(path, "$نيفا") == 0) return nv_strdup(nv_program_dir);
    // "$نيفا/" عشرة بايتات UTF-8 ($ + 4 حروف × بايتين + /).
    if (strncmp(path, "$نيفا/", 10) == 0) {
        size_t n = strlen(nv_program_dir) + strlen(path + 9) + 1;
        char *out = (char *)malloc(n);
        snprintf(out, n, "%s%s", nv_program_dir, path + 9);
        return out;
    }
    return nv_strdup(path);
}

// هل المسار مطلق؟ يونكس: يبدأ بـ`/`. ويندوز: محرك
// (`C:\...` أو `C:/...`) أو UNC (`\\خادم\مشاركة`) —
// فحص `/` وحده يعتبر مسار `~` الموسّع على ويندوز
// (يبدأ بحرف المحرك) نسبيا فيُلحق به مجلد الجلسة
// فينتج مسارا مشوها لا يجده stat أبدا.
static int nv_is_absolute_path(const char *p) {
    if (!p || !p[0]) return 0;
    if (p[0] == '/') return 1;
#ifdef _WIN32
    if (((p[0] >= 'A' && p[0] <= 'Z') || (p[0] >= 'a' && p[0] <= 'z'))
        && p[1] == ':' && (p[2] == '/' || p[2] == '\\' || p[2] == '\0')) return 1;
    if (p[0] == '\\' && p[1] == '\\') return 1;
#endif
    return 0;
}

// هل base بادئة path على حدّ مكوّن (نهاية أو '/')؟
// على ويندوز المقارنة بلا حساسية لحالة الأحرف
// (مسارات ويندوز بلا حساسية)؛ وعلى يونكس بايتات
// حرفية. تستعمل في الطيّ القانوني للعرض.
static int nv_path_prefix_is(const char *path, const char *base) {
    size_t bl = strlen(base);
    if (bl == 0) return 0;
#ifdef _WIN32
    size_t i;
    for (i = 0; i < bl; i++) {
        unsigned char a = (unsigned char)path[i];
        unsigned char b = (unsigned char)base[i];
        if (a >= 'A' && a <= 'Z') a = (unsigned char)(a + ('a' - 'A'));
        if (b >= 'A' && b <= 'Z') b = (unsigned char)(b + ('a' - 'A'));
        if (a != b) return 0;
    }
#else
    if (strncmp(path, base, bl) != 0) return 0;
#endif
    if (base[bl - 1] == '/') return 1;
    return path[bl] == '\0' || path[bl] == '/';
}

char *nv_display_path(const char *path) {
    // أطول مطابقة أولا: تحت البيت يظهر ~/...
    // لا $نيفا/home/... أبدا.
    if (strcmp(path, nv_home_dir) == 0) return nv_strdup("~");
    size_t hl = strlen(nv_home_dir);
    if (strncmp(path, nv_home_dir, hl) == 0 && path[hl] == '/') {
        size_t n = strlen(path + hl) + 2;
        char *out = (char *)malloc(n);
        snprintf(out, n, "~%s", path + hl);
        return out;
    }
    if (strcmp(path, nv_program_dir) == 0) return nv_strdup("$نيفا");
    size_t bl = strlen(nv_program_dir);
    if (strncmp(path, nv_program_dir, bl) == 0 && path[bl] == '/') {
        size_t n = strlen(path + bl) + 10;
        char *out = (char *)malloc(n);
        snprintf(out, n, "$نيفا%s", path + bl);
        return out;
    }
    // مسار /data/... الخاص: الورقة فقط.
    if (path[0] == '/') {
        const char *p = path + 1;
        const char *slash = strchr(p, '/');
        if (slash && (size_t)(slash - p) == 4 && strncmp(p, "data", 4) == 0) {
            const char *leaf = strrchr(path, '/');
            return nv_strdup(leaf ? leaf + 1 : path);
        }
    }
    // طيّ قانوني للطرفين (مرآة المرحلة الثانية من
    // الطيّ في Rust — permissions.rs): المسار الفيزيائي
    // يأتي مقنّنا بينما المرساة خام، فتقنين الطرفين
    // يعيد الطيّ حين تختلف الصياغة (حالة أحرف أو اسم
    // قصير على ويندوز). يطبَّق فقط حين يفشل الطيّ
    // الخام والمسار مطلق — وإلا يبقى المسار كما هو.
    if (nv_is_absolute_path(path)) {
        char *canon = nv_canonical_with_suffix(path);
        char *canon_home = nv_canonical_with_suffix(nv_home_dir);
        char *canon_prog = nv_canonical_with_suffix(nv_program_dir);
        const char *anchor = NULL;
        const char *label = NULL;
        if (canon && canon_home && nv_path_prefix_is(canon, canon_home)) {
            anchor = canon_home;
            label = "~";
        } else if (canon && canon_prog && nv_path_prefix_is(canon, canon_prog)) {
            anchor = canon_prog;
            label = "$نيفا";
        }
        if (anchor) {
            size_t alen = strlen(anchor);
            char *out;
            if (canon[alen] == '\0') {
                out = nv_strdup(label);
            } else {
                size_t n = strlen(label) + strlen(canon + alen) + 1;
                out = (char *)malloc(n);
                snprintf(out, n, "%s%s", label, canon + alen);
            }
            free(canon);
            free(canon_home);
            free(canon_prog);
            return out;
        }
        free(canon);
        free(canon_home);
        free(canon_prog);
    }
    return nv_strdup(path);
}

char *nv_display_str(const char *s) {
    if (strcmp(s, "~") == 0 || strncmp(s, "~/", 2) == 0) return nv_strdup(s);
    if (strcmp(s, "$نيفا") == 0 || strncmp(s, "$نيفا/", 10) == 0) return nv_strdup(s);
    if (nv_is_absolute_path(s)) return nv_display_path(s);
    return nv_strdup(s);
}

// ---------- مساعدات التنقل (اجتماع 56 §71 — الدفعة 2) ----------

// فك نقطة شفرة UTF-8 واحدة: ترجع البايتات المستهلكة
// (1–4)، ونقطة الشفرة في *cp.
static size_t nv_utf8_decode(const unsigned char *s, unsigned int *cp) {
    unsigned char c = s[0];
    if (c < 0x80) {
        *cp = c;
        return 1;
    }
    if ((c & 0xE0) == 0xC0 && (s[1] & 0xC0) == 0x80) {
        *cp = ((c & 0x1F) << 6) | (s[1] & 0x3F);
        return 2;
    }
    if ((c & 0xF0) == 0xE0 && (s[1] & 0xC0) == 0x80 && (s[2] & 0xC0) == 0x80) {
        *cp = ((c & 0x0F) << 12) | (((unsigned char)s[1] & 0x3F) << 6) | ((unsigned char)s[2] & 0x3F);
        return 3;
    }
    if ((c & 0xF8) == 0xF0 && (s[1] & 0xC0) == 0x80 && (s[2] & 0xC0) == 0x80 && (s[3] & 0xC0) == 0x80) {
        *cp = ((c & 0x07) << 18) | (((unsigned char)s[1] & 0x3F) << 12) | (((unsigned char)s[2] & 0x3F) << 6) | ((unsigned char)s[3] & 0x3F);
        return 4;
    }
    *cp = c;
    return 1;
}

static int nv_is_arabic_digit(unsigned int cp, long *d) {
    if (cp >= '0' && cp <= '9') {
        *d = (long)(cp - '0');
        return 1;
    }
    if (cp >= 0x660 && cp <= 0x669) {
        *d = (long)(cp - 0x660);
        return 1;
    }
    if (cp >= 0x6F0 && cp <= 0x6F9) {
        *d = (long)(cp - 0x6F0);
        return 1;
    }
    return 0;
}

// عدد صحيح من نص بأرقام غربية أو عربية مشرقية، مع
// إشارة سالبة اختيارية. 1 = صالح، 0 = ليس عددا.
static int nv_parse_arabic_int(const char *s, long *out) {
    while (*s == ' ') s++;
    int neg = 0;
    if (*s == '-') {
        neg = 1;
        s++;
    }
    if (!*s) return 0;
    long n = 0;
    const unsigned char *p = (const unsigned char *)s;
    while (*p && *p != ' ') {
        unsigned int cp;
        size_t adv = nv_utf8_decode(p, &cp);
        long d;
        if (!nv_is_arabic_digit(cp, &d)) return 0;
        n = n * 10 + d;
        if (n > 1000000000L) return 0;
        p += adv;
    }
    while (*p == ' ') p++;
    if (*p) return 0;
    *out = neg ? -n : n;
    return 1;
}

// عدد طويل بأرقام عربية مشرقية (كـ Value::fmt_value
// في Rust) لرسائل الخطأ.
static void nv_fmt_arabic_long(long n, char *buf, size_t bufsz) {
    char tmp[32];
    snprintf(tmp, sizeof(tmp), "%ld", n);
    size_t w = 0;
    for (size_t i = 0; tmp[i] && w + 2 < bufsz; i++) {
        if (tmp[i] == '-') {
            buf[w++] = '-';
        } else if (tmp[i] >= '0' && tmp[i] <= '9') {
            buf[w++] = (char)0xD9;
            buf[w++] = (char)(0xA0 + (tmp[i] - '0'));
        }
    }
    buf[w] = '\0';
}

// تحليل بادئة `~+`: `~+` و`~+/...` (الحالي)،
// و`~+N` و`~+N/...` (عنصر المكدس N). 1 = الصيغة
// `~+`، و*tail_out يشير إلى الباقي بعد البادئة.
static int nv_parse_tilde_plus(const char *path, long *idx_out, const char **tail_out) {
    if (strncmp(path, "~+", 2) != 0) return 0;
    const char *rest = path + 2;
    if (rest[0] == '\0' || rest[0] == '/') {
        *idx_out = 0;
        *tail_out = rest;
        return 1;
    }
    const unsigned char *p = (const unsigned char *)rest;
    size_t end = 0;
    while (*p) {
        unsigned int cp;
        size_t adv = nv_utf8_decode(p, &cp);
        long d;
        if (!nv_is_arabic_digit(cp, &d)) break;
        end += adv;
        p += adv;
    }
    if (end == 0 || end >= 32) return 0;
    char numbuf[32];
    memcpy(numbuf, rest, end);
    numbuf[end] = '\0';
    long idx;
    if (!nv_parse_arabic_int(numbuf, &idx)) return 0;
    const char *tail = rest + end;
    if (tail[0] != '\0' && tail[0] != '/') return 0;
    *idx_out = idx;
    *tail_out = tail;
    return 1;
}

// توسعة `~+` و`~+N` إلى مسار مطلق للنظام، أو NULL
// إن لم تكن الصيغة `~+`. الفهرس الخارج خطأ عربي
// (بلا عودة). `~-` ليست هنا: محذوفة بأمر المالك.
static char *nv_expand_tilde_plus(const char *path, int line, int col) {
    long idx;
    const char *tail;
    if (!nv_parse_tilde_plus(path, &idx, &tail)) return NULL;
    const char *base;
    if (idx == 0) {
        base = nv_cwd;
    } else {
        if (idx < 1 || (size_t)(idx - 1) >= nv_dirstack_len) {
            char numbuf[64];
            nv_fmt_arabic_long(idx, numbuf, sizeof(numbuf));
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "عنصر المكدس غير موجود: %s", numbuf);
        }
        base = nv_dirstack[idx - 1];
    }
    size_t n = strlen(base) + strlen(tail) + 1;
    char *out = (char *)malloc(n);
    snprintf(out, n, "%s%s", base, tail);
    return out;
}

// حل مسار ضد مجلد الجلسة (resolve_in) + فحص السياسة.
// حل ناعم (اجتماع 56 §71): كـ nv_resolve_path لكن
// بلا خطأ عند رفض السياسة — يرجع NULL ويضبط
// *policy_err (0 نجاح، 1 خارج النطاق، 2 عبر رابط
// رمزي). لسلسلة محاولات `انتقل` التي تتخطى المرشح
// الفاشل وتجرّب التالي.
static char *nv_resolve_path_soft(const char *path, int line, int col, int *policy_err) {
    *policy_err = 0;
    char *plus = nv_expand_tilde_plus(path, line, col);
    char *given = plus ? plus : nv_expand_home(path);
    char *joined = NULL;
    if (nv_trusted) {
        if (nv_is_absolute_path(given)) {
            joined = given;
        } else {
            size_t n = strlen(nv_cwd) + 1 + strlen(given) + 1;
            joined = (char *)malloc(n);
            snprintf(joined, n, "%s/%s", nv_cwd, given);
            free(given);
        }
        char *norm = nv_normalize_lexical(joined);
        free(joined);
        return norm;
    }
    if (nv_is_absolute_path(given)) {
        joined = given;
    } else {
        size_t n = strlen(nv_cwd) + 1 + strlen(given) + 1;
        joined = (char *)malloc(n);
        snprintf(joined, n, "%s/%s", nv_cwd, given);
        free(given);
    }
    char *normalized = nv_normalize_lexical(joined);
    free(joined);
    char *base_norm = nv_normalize_lexical(nv_program_dir);
    if (!nv_path_starts_with(normalized, base_norm)) {
        free(normalized);
        free(base_norm);
        *policy_err = 1;
        return NULL;
    }
    char *canon_cand = nv_canonical_with_suffix(normalized);
    char *canon_base = nv_canonical_with_suffix(base_norm);
    if (!nv_path_starts_with(canon_cand, canon_base)) {
        free(normalized);
        free(canon_cand);
        free(canon_base);
        free(base_norm);
        *policy_err = 2;
        return NULL;
    }
    free(canon_cand);
    free(canon_base);
    free(base_norm);
    return normalized;
}

char *nv_resolve_path(const char *path, int line, int col) {
    int policy_err = 0;
    char *out = nv_resolve_path_soft(path, line, col, &policy_err);
    if (out) return out;
    char *disp = nv_display_str(path);
    if (policy_err == 2) {
        nv_error_fmt(NV_ERR_IO, line, col,
                     "الملف %s خارج مجلد البرنامج عبر رابط رمزي، والوصول مقيد بمجلد البرنامج افتراضيا",
                     disp);
    }
    nv_error_fmt(NV_ERR_IO, line, col,
                 "الملف %s خارج مجلد البرنامج، والوصول مقيد بمجلد البرنامج افتراضيا (استخدم --اسمح-بالكل للبرامج الموثوقة)",
                 disp);
    return NULL;
}

// (اجتماع 49 §63) حل مسار برنامج فرعي `.نيفا` نسبي:
// مجلد البرنامج أولا (إن وُجد الملف هناك) ثم cwd
// (البيت) — مرآة `شغّل` ومرآة `build_spawn_command`
// في Rust. المسار المطلق أو `~`/`$نيفا` كما هو؛
// والمرشح يمر بفحص السياسة نفسه عبر nv_resolve_path.
char *nv_resolve_subprogram(const char *program, int line, int col) {
    if (program[0] != '/' && program[0] != '~' && program[0] != '$') {
        size_t n = strlen(nv_program_dir) + 1 + strlen(program) + 1;
        char *cand = (char *)malloc(n);
        snprintf(cand, n, "%s/%s", nv_program_dir, program);
        struct stat st;
        if (nv_lstat(cand, &st) == 0 && S_ISREG(st.st_mode)) {
            char *resolved = nv_resolve_path(cand, line, col);
            free(cand);
            return resolved;
        }
        free(cand);
    }
    return nv_resolve_path(program, line, col);
}

int nv_path_inside_program(const char *abs_path) {
    char *norm = nv_normalize_lexical(abs_path);
    char *base_norm = nv_normalize_lexical(nv_program_dir);
    int ok = nv_path_starts_with(norm, base_norm);
    free(norm);
    free(base_norm);
    return ok;
}

// حجم مجلد (تكراري، ميزانية 10000 مدخل) — مثل dir_size.
static unsigned long long nv_dir_size_rec(const char *path, int *budget) {
    if (*budget <= 0) return 0;
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
            total += nv_dir_size_rec(child, budget);
            free(child);
            if (*budget <= 0) break;
        }
        closedir(d);
        return total;
    }
    return (unsigned long long)st.st_size;
}

void nv_check_quota(size_t add_bytes, int line, int col) {
    int budget = 10000;
    unsigned long long used = nv_dir_size_rec(nv_program_dir, &budget);
    if (used + add_bytes > 100ull * 1024 * 1024) {
        nv_error(NV_ERR_IO, "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة", line, col);
    }
}

size_t nv_quota_remaining(void) {
    int budget = 10000;
    unsigned long long used = nv_dir_size_rec(nv_program_dir, &budget);
    unsigned long long total = 100ull * 1024 * 1024;
    return (size_t)(used >= total ? 0 : total - used);
}

void nv_check_file_size(const char *abs_path, int line, int col) {
    struct stat st;
    if (stat(abs_path, &st) == 0 && (unsigned long long)st.st_size > 5ull * 1024 * 1024) {
        char *disp = nv_display_str(abs_path);
        nv_error_fmt(NV_ERR_IO, line, col, "الملف %s أكبر من الحد الأقصى (5 ميغابايت)", disp);
    }
}

// قراءة ملف محدودة (5 ميغابايت) كنص UTF-8.
static char *nv_read_file_capped(const char *abs_path, const char *display, int line, int col, size_t *out_len) {
    FILE *f = fopen(abs_path, "rb");
    if (!f) {
        int e = errno;
        char osb[256];
        nv_os_error_str(e, osb, sizeof(osb));
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر قراءة الملف %s: %s", display, osb);
    }
    size_t cap = 5 * 1024 * 1024 + 1;
    unsigned char *buf = (unsigned char *)malloc(cap + 1);
    size_t n = fread(buf, 1, cap, f);
    int read_err = ferror(f);
    fclose(f);
    if (read_err) {
        int e = errno;
        char osb[256];
        nv_os_error_str(e, osb, sizeof(osb));
        free(buf);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر قراءة الملف %s: %s", display, osb);
    }
    if (n > 5 * 1024 * 1024) {
        free(buf);
        nv_error_fmt(NV_ERR_IO, line, col, "الملف %s أكبر من الحد الأقصى (5 ميغابايت)", display);
    }
    // تحقق UTF-8 (صالح أم لا).
    size_t i = 0;
    while (i < n) {
        unsigned char c = buf[i];
        size_t adv = 1;
        if ((c & 0x80) == 0) adv = 1;
        else if ((c & 0xE0) == 0xC0) adv = 2;
        else if ((c & 0xF0) == 0xE0) adv = 3;
        else if ((c & 0xF8) == 0xF0) adv = 4;
        else {
            free(buf);
            nv_error_fmt(NV_ERR_IO, line, col, "الملف %s ليس نصا صالحا (UTF-8)", display);
        }
        if (i + adv > n) {
            free(buf);
            nv_error_fmt(NV_ERR_IO, line, col, "الملف %s ليس نصا صالحا (UTF-8)", display);
        }
        for (size_t k = 1; k < adv; k++) {
            if ((buf[i + k] & 0xC0) != 0x80) {
                free(buf);
                nv_error_fmt(NV_ERR_IO, line, col, "الملف %s ليس نصا صالحا (UTF-8)", display);
            }
        }
        i += adv;
    }
    buf[n] = '\0';
    if (out_len) *out_len = n;
    return (char *)buf;
}

unsigned char *nv_read_file_bytes(const char *abs_path, size_t *out_len) {
    FILE *f = fopen(abs_path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) sz = 0;
    unsigned char *buf = (unsigned char *)malloc((size_t)sz + 1);
    size_t n = fread(buf, 1, (size_t)sz, f);
    fclose(f);
    if (out_len) *out_len = n;
    return buf;
}

int nv_write_file_atomic(const char *abs_path, const unsigned char *data, size_t len) {
    char *tmp = nv_temp_sibling(abs_path);
    FILE *f = fopen(tmp, "wb");
    if (!f) {
        free(tmp);
        return -1;
    }
    if (len) fwrite(data, 1, len, f);
    fclose(f);
    int rc = rename(tmp, abs_path);
    free(tmp);
    return rc;
}

char *nv_temp_sibling(const char *abs_path) {
    static unsigned long counter = 0;
    size_t n = strlen(abs_path) + 64;
    char *out = (char *)malloc(n);
    snprintf(out, n, "%s.مؤقت-%d-%lu", abs_path, (int)getpid(), counter++);
    return out;
}

// ---------- تنسيق التاريخ UTC ----------
// "YYYY-MM-DD HH:MM:SS" بأرقام غربية (مثل format_unix_utc).
static void nv_format_unix_utc(unsigned long long secs, char *out, size_t outsz) {
    // civil_from_days ( Howard Hinnant ): days -> y/m/d.
    long long days = (long long)(secs / 86400);
    unsigned long long sod = secs % 86400;
    long long z = days + 719468;
    long long era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned long long doe = (unsigned long long)(z - era * 146097);
    unsigned long long yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long long y = (long long)yoe + era * 400;
    unsigned long long doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    unsigned long long mp = (5 * doy + 2) / 153;
    unsigned long long d = doy - (153 * mp + 2) / 5 + 1;
    unsigned long long m = mp < 10 ? mp + 3 : mp - 9;
    y += (m <= 2);
    snprintf(out, outsz, "%04lld-%02u-%02u %02u:%02u:%02u",
             y, (unsigned)m, (unsigned)d,
             (unsigned)(sod / 3600), (unsigned)((sod % 3600) / 60),
             (unsigned)(sod % 60));
}

// ---------- الدوال المدمجة التسع + المسطحة ----------
static void nv_arity(NvValue *args, int nargs, int expected, const char *name, int line, int col) {
    (void)args;
    if (nargs != expected) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع %d معاملات لكن أعطيت %d",
                     name, expected, nargs);
    }
}

// فحص بياض Unicode (لقص).
static int nv_is_space_cp(unsigned int cp) {
    if (cp == ' ' || cp == '\t' || cp == '\n' || cp == '\r' || cp == 0x0B || cp == 0x0C) return 1;
    if (cp == 0x00A0 || cp == 0x1680 || (cp >= 0x2000 && cp <= 0x200A) || cp == 0x2028 ||
        cp == 0x2029 || cp == 0x202F || cp == 0x205F || cp == 0x3000 || cp == 0xFEFF) return 1;
    return 0;
}

// فك محرف UTF-8 واحد (يعيد النقطة ويتقدم).
static unsigned int nv_decode_cp(const char **p, const char *end) {
    const unsigned char *s = (const unsigned char *)*p;
    if (s >= (const unsigned char *)end) return 0;
    unsigned char c = *s;
    unsigned int cp = c;
    size_t adv = 1;
    if ((c & 0x80) == 0) { cp = c; adv = 1; }
    else if ((c & 0xE0) == 0xC0 && s + 1 < (const unsigned char *)end) { cp = ((c & 0x1F) << 6) | (s[1] & 0x3F); adv = 2; }
    else if ((c & 0xF0) == 0xE0 && s + 2 < (const unsigned char *)end) { cp = ((c & 0x0F) << 12) | ((s[1] & 0x3F) << 6) | (s[2] & 0x3F); adv = 3; }
    else if ((c & 0xF8) == 0xF0 && s + 3 < (const unsigned char *)end) { cp = ((c & 0x07) << 18) | ((s[1] & 0x3F) << 12) | ((s[2] & 0x3F) << 6) | (s[3] & 0x3F); adv = 4; }
    *p += adv;
    return cp;
}

static void nv_encode_cp(char *out, size_t *w, unsigned int cp) {
    if (cp < 0x80) {
        out[(*w)++] = (char)cp;
    } else if (cp < 0x800) {
        out[(*w)++] = (char)(0xC0 | (cp >> 6));
        out[(*w)++] = (char)(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        out[(*w)++] = (char)(0xE0 | (cp >> 12));
        out[(*w)++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[(*w)++] = (char)(0x80 | (cp & 0x3F));
    } else {
        out[(*w)++] = (char)(0xF0 | (cp >> 18));
        out[(*w)++] = (char)(0x80 | ((cp >> 12) & 0x3F));
        out[(*w)++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[(*w)++] = (char)(0x80 | (cp & 0x3F));
    }
}

// تحويل حالة نص (كبير/صغير) عبر towupper/towlower.
static NvValue nv_str_case(NvValue str, int upper) {
    const char *s = str.u.str->data;
    const char *end = s + str.u.str->len;
    size_t cap = str.u.str->len * 2 + 8;
    char *out = (char *)malloc(cap);
    size_t w = 0;
    const char *p = s;
    while (p < end) {
        const char *save = p;
        unsigned int cp = nv_decode_cp(&p, end);
        if (p == save) break;
        wint_t mapped = upper ? towupper((wint_t)cp) : towlower((wint_t)cp);
        if (w + 4 >= cap) {
            cap *= 2;
            out = (char *)realloc(out, cap);
        }
        nv_encode_cp(out, &w, (unsigned int)mapped);
    }
    out[w] = '\0';
    return nv_str_take(out, w);
}

// قص البياض من الطرفين.
static NvValue nv_str_trim(NvValue str) {
    const char *s = str.u.str->data;
    const char *end = s + str.u.str->len;
    const char *a = s, *b = end;
    // البداية.
    while (a < end) {
        const char *save = a;
        unsigned int cp = nv_decode_cp(&a, end);
        if (!nv_is_space_cp(cp)) { a = save; break; }
    }
    // النهاية: امش من الخلف.
    while (b > a) {
        // ابحث عن بداية آخر محرف.
        const char *p = b - 1;
        while (p > a && ((unsigned char)*p & 0xC0) == 0x80) p--;
        const char *q = p;
        unsigned int cp = nv_decode_cp(&q, b);
        if (!nv_is_space_cp(cp)) break;
        b = p;
    }
    return nv_str_new(a, (size_t)(b - a));
}

// طرق النص والقائمة.
NvValue nv_call_str_method(NvValue str, const char *name, NvValue *args, int nargs, int line, int col) {
    const char *s = str.u.str->data;
    size_t len = str.u.str->len;
    if (strcmp(name, "طول") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_num((double)str.u.str->chars);
    }
    if (strcmp(name, "كبير") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_str_case(str, 1);
    }
    if (strcmp(name, "صغير") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_str_case(str, 0);
    }
    if (strcmp(name, "قص") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_str_trim(str);
    }
    if (strcmp(name, "استبدل") == 0) {
        if (nargs != 2) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 2 معاملات لكن أعطيت %d", name, nargs);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "طريقة استبدل تتوقع نصين: القديم والجديد", line, col);
        }
        const char *old = args[0].u.str->data, *nw = args[1].u.str->data;
        size_t olen = args[0].u.str->len, nlen = args[1].u.str->len;
        if (olen == 0) return nv_str_new(s, len);
        // عد الظهورات.
        size_t count = 0;
        const char *p = s;
        while ((p = strstr(p, old)) != NULL) { count++; p += olen; }
        size_t outlen = len + count * (nlen - olen);
        char *out = (char *)malloc(outlen + 1);
        char *w = out;
        p = s;
        const char *hit;
        while ((hit = strstr(p, old)) != NULL) {
            size_t seg = (size_t)(hit - p);
            memcpy(w, p, seg);
            w += seg;
            memcpy(w, nw, nlen);
            w += nlen;
            p = hit + olen;
        }
        size_t tail = (size_t)((s + len) - p);
        memcpy(w, p, tail);
        w += tail;
        *w = '\0';
        return nv_str_take(out, outlen);
    }
    if (strcmp(name, "استبدل_أول") == 0) {
        if (nargs != 2) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 2 معاملات لكن أعطيت %d", name, nargs);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "طريقة استبدل_أول تتوقع نصين: القديم والجديد", line, col);
        }
        const char *old = args[0].u.str->data, *nw = args[1].u.str->data;
        size_t olen = args[0].u.str->len, nlen = args[1].u.str->len;
        if (olen == 0) return nv_str_new(s, len);
        const char *hit = strstr(s, old);
        if (!hit) return nv_str_new(s, len);
        size_t outlen = len - olen + nlen;
        char *out = (char *)malloc(outlen + 1);
        size_t seg = (size_t)(hit - s);
        memcpy(out, s, seg);
        memcpy(out + seg, nw, nlen);
        size_t tail = (size_t)((s + len) - (hit + olen));
        memcpy(out + seg + nlen, hit + olen, tail);
        out[outlen] = '\0';
        return nv_str_take(out, outlen);
    }
    if (strcmp(name, "قسم") == 0) {
        if (nargs != 1) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
        if (args[0].tag != NV_STR) nv_error(NV_ERR_TYPE, "طريقة قسم تتوقع نصا كفاصل", line, col);
        const char *sep = args[0].u.str->data;
        size_t slen = args[0].u.str->len;
        NvValue out = nv_list_new();
        size_t _ro = nv_root_push_idx();
        *nv_root_slot(_ro) = out;
        if (slen == 0) {
            // Rust: split("") يعيد "" ثم كل محرف ثم "".
            nv_list_push_raw(out.u.list, nv_str_cstr(""));
            const char *p = s;
            const char *end = s + len;
            while (p < end) {
                size_t clen = 0;
                const char *ch = nv_utf8_char_at(p, (size_t)(end - p), 0, &clen);
                if (!ch || clen == 0) break;
                nv_list_push_raw(out.u.list, nv_str_new(ch, clen));
                p += clen;
            }
            nv_list_push_raw(out.u.list, nv_str_cstr(""));
            nv_root_pop_n(1);
            return out;
        }
        const char *p = s;
        const char *hit;
        while ((hit = strstr(p, sep)) != NULL) {
            nv_list_push_raw(out.u.list, nv_str_new(p, (size_t)(hit - p)));
            p = hit + slen;
        }
        nv_list_push_raw(out.u.list, nv_str_new(p, (size_t)((s + len) - p)));
        nv_root_pop_n(1);
        return out;
    }
    if (strcmp(name, "يحتوي") == 0) {
        if (nargs != 1) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
        if (args[0].tag != NV_STR) nv_error(NV_ERR_TYPE, "طريقة يحتوي على نص تتوقع نصا", line, col);
        if (args[0].u.str->len == 0) return nv_bool(1);
        return nv_bool(strstr(s, args[0].u.str->data) != NULL);
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "طريقة غير معروفة لنوع نص: %s", name);
}

// طرق القائمة (slot = خلية المتغير لإعادة الكتابة).
NvValue nv_call_list_method(NvValue *slot, const char *name, NvValue *args, int nargs, int line, int col) {
    NvList *l = slot->u.list;
    if (strcmp(name, "طول") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_num((double)l->len);
    }
    if (strcmp(name, "يحتوي") == 0) {
        if (nargs != 1) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
        for (size_t i = 0; i < l->len; i++) {
            if (nv_eq(l->items[i], args[0])) return nv_bool(1);
        }
        return nv_bool(0);
    }
    if (strcmp(name, "أضف") == 0) {
        if (nargs != 1) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
        nv_list_push_raw(l, args[0]);
        return nv_nil();
    }
    if (strcmp(name, "أزل") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة أزل تتوقع 0 أو 1 معاملات لكن أعطيت %d", nargs);
        }
        if (nargs == 0) {
            if (l->len == 0) nv_error(NV_ERR_RUNTIME, "لا يمكن الإزالة من قائمة فارغة", line, col);
            NvValue removed = l->items[--l->len];
            return removed;
        }
        if (args[0].tag != NV_NUM || args[0].u.num != trunc(args[0].u.num) || args[0].u.num < 0.0) {
            nv_error(NV_ERR_TYPE, "طريقة أزل تتوقع فهرسا عددا صحيحا", line, col);
        }
        size_t idx = nv_num_to_index(args[0].u.num);
        if (idx >= l->len) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "فهرس خارج النطاق: %zu والقائمة طولها %zu", idx, l->len);
        }
        NvValue removed = l->items[idx];
        memmove(&l->items[idx], &l->items[idx + 1], (l->len - idx - 1) * sizeof(NvValue));
        l->len--;
        return removed;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "طريقة غير معروفة لنوع قائمة: %s", name);
}

// طرق المترابطة والتشكيلة (بلا تعديل — `طول`
// و`يحتوي` فقط، مرآة call_builtin_method في Rust).
static NvValue nv_call_tuple_set_method(NvValue target, const char *name, NvValue *args, int nargs, int line, int col) {
    NvList *l = target.tag == NV_TUPLE ? target.u.tuple : target.u.set;
    const char *tname = target.tag == NV_TUPLE ? "مترابطة" : "تشكيلة";
    if (strcmp(name, "طول") == 0) {
        if (nargs != 0) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 0 معاملات لكن أعطيت %d", name, nargs);
        return nv_num((double)l->len);
    }
    if (strcmp(name, "يحتوي") == 0) {
        if (nargs != 1) nv_error_fmt(NV_ERR_RUNTIME, line, col, "الطريقة %s تتوقع 1 معاملات لكن أعطيت %d", name, nargs);
        for (size_t i = 0; i < l->len; i++) {
            if (nv_eq(l->items[i], args[0])) return nv_bool(1);
        }
        return nv_bool(0);
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "طريقة غير معروفة لنوع %s: %s", tname, name);
}

// ---------- استدعاء بنقطة (طريقة نص/قائمة أو عضو وحدة/كائن) ----------
// يوزع على نوع الهدف وقت التشغيل؛ والقائمة تُعدَّل في
// مكانها (كائن القائمة مشترك مع متغير المتصل)، فيكتب
// المولد القيمة عائدا إلى المتغير بعد الاستدعاء.
NvValue nv_call_dot_at(NvValue target, const char *name, NvValue *args, int nargs, int line, int col,
                     int callee_line, int callee_col) {
    return nv_call_dot_at_kw(target, name, args, nargs, NULL, NULL, 0, line, col, callee_line, callee_col);
}

NvValue nv_call_dot_at_kw(NvValue target, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col,
                     int callee_line, int callee_col) {
    // البنود 109–112: طرق النصوص والقوائم مدمجة
    // ترفض المفتاحيات.
    if (nkwargs > 0 && (target.tag == NV_STR || target.tag == NV_LIST || target.tag == NV_TUPLE || target.tag == NV_SET)) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "معامل مفتاحي غير معروف: %s", kw_names[0]);
    }
    if (target.tag == NV_STR) return nv_call_str_method(target, name, args, nargs, line, col);
    if (target.tag == NV_LIST) {
        NvValue slot = target;
        return nv_call_list_method(&slot, name, args, nargs, line, col);
    }
    if (target.tag == NV_TUPLE || target.tag == NV_SET) {
        return nv_call_tuple_set_method(target, name, args, nargs, line, col);
    }
    NvValue member = nv_dot(target, name, line, col);
    return nv_call_at_kw(member, args, nargs, kw_names, kw_vals, nkwargs, line, col, callee_line, callee_col);
}

NvValue nv_call_dot(NvValue target, const char *name, NvValue *args, int nargs, int line, int col) {
    return nv_call_dot_at(target, name, args, nargs, line, col, line, col);
}

NvValue nv_call_dot_kw(NvValue target, const char *name, NvValue *args, int nargs, const char **kw_names, NvValue *kw_vals, int nkwargs, int line, int col) {
    return nv_call_dot_at_kw(target, name, args, nargs, kw_names, kw_vals, nkwargs, line, col, line, col);
}

// ---------- عدّاد معاملات (للبداية/النهاية) ----------
static size_t nv_count_arg(NvValue v, int line, int col) {
    if (v.tag == NV_NUM && v.u.num >= 0.0 && v.u.num == trunc(v.u.num)) {
        return nv_num_to_index(v.u.num);
    }
    if (v.tag == NV_STR) {
        const char *s = v.u.str->data;
        // قص البياض ثم حوّل الأرقام العربية إلى غربية.
        char *tmp = nv_strdup(s);
        char *a = tmp;
        while (*a == ' ' || *a == '\t' || *a == '\n' || *a == '\r') a++;
        size_t tl = strlen(a);
        while (tl > 0 && (a[tl - 1] == ' ' || a[tl - 1] == '\t' || a[tl - 1] == '\n' || a[tl - 1] == '\r')) a[--tl] = '\0';
        // تحويل الأرقام العربية (مشرقية/ممتدة) إلى غربية.
        size_t n = strlen(a);
        char *norm = (char *)malloc(n + 1);
        size_t w = 0;
        for (size_t i = 0; i < n;) {
            unsigned char c = (unsigned char)a[i];
            if (c == 0xD9 && i + 1 < n && (unsigned char)a[i + 1] >= 0xA0 && (unsigned char)a[i + 1] <= 0xA9) {
                norm[w++] = (char)('0' + ((unsigned char)a[i + 1] - 0xA0));
                i += 2;
            } else if (c == 0xDB && i + 1 < n && (unsigned char)a[i + 1] >= 0xB0 && (unsigned char)a[i + 1] <= 0xB9) {
                norm[w++] = (char)('0' + ((unsigned char)a[i + 1] - 0xB0));
                i += 2;
            } else {
                norm[w++] = (char)c;
                i++;
            }
        }
        norm[w] = '\0';
        // حلل كعدد صحيح بلا إشارة (Rust parse::<usize>:
        // يقبل + بادئة، ويرفض الفاصلة والإشارة السالبة).
        const char *p = norm;
        if (*p == '+') p++;
        int ok = (*p != '\0');
        unsigned long long val = 0;
        for (; *p; p++) {
            if (*p < '0' || *p > '9') { ok = 0; break; }
            val = val * 10 + (unsigned long long)(*p - '0');
            if (val > 1000000000000ull) { ok = 0; break; }
        }
        free(norm);
        free(tmp);
        if (ok) return (size_t)val;
        nv_error_fmt(NV_ERR_TYPE, line, col, "العدد غير صالح: %s", s);
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "العدد يتطلب رقما، وليس %s", nv_type_name(v));
}

// ---------- بحث ملفات (ابحث_عن) ----------
static void nv_find_files(const char *dir, const char *base, const char *query, int depth, NvValue out_list) {
    if (out_list.u.list->len >= 100 || depth > 4) return;
    DIR *d = opendir(dir);
    if (!d) return;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (out_list.u.list->len >= 100) break;
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        if (strstr(e->d_name, query)) {
            size_t n = strlen(dir) + 1 + strlen(e->d_name) + 1;
            char *full = (char *)malloc(n);
            snprintf(full, n, "%s/%s", dir, e->d_name);
            const char *rel = full;
            size_t bl = strlen(base);
            if (strncmp(full, base, bl) == 0 && full[bl] == '/') rel = full + bl + 1;
            nv_list_push_raw(out_list.u.list, nv_str_cstr(rel));
            free(full);
        }
        // مجلد؟ (لا تتبع الروابط: lstat).
        size_t n = strlen(dir) + 1 + strlen(e->d_name) + 1;
        char *child = (char *)malloc(n);
        snprintf(child, n, "%s/%s", dir, e->d_name);
        struct stat st;
        if (nv_lstat(child, &st) == 0 && S_ISDIR(st.st_mode)) {
            nv_find_files(child, base, query, depth + 1, out_list);
        }
        free(child);
    }
    closedir(d);
}

// ---------- شجرة (اعرض_الشجرة) ----------
static int nv_cmp_str(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}

static void nv_tree_lines(const char *dir, const char *prefix, int depth, NvValue out_list) {
    if (out_list.u.list->len >= 200 || depth > 3) return;
    DIR *d = opendir(dir);
    if (!d) return;
    size_t room = 200 - out_list.u.list->len;
    char **names = NULL;
    size_t nnames = 0, capn = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL && nnames < room) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        if (nnames == capn) {
            capn = capn ? capn * 2 : 16;
            names = (char **)realloc(names, capn * sizeof(char *));
        }
        names[nnames++] = nv_strdup(e->d_name);
    }
    closedir(d);
    qsort(names, nnames, sizeof(char *), nv_cmp_str);
    for (size_t i = 0; i < nnames; i++) {
        if (out_list.u.list->len >= 200) break;
        size_t n = strlen(dir) + 1 + strlen(names[i]) + 1;
        char *child = (char *)malloc(n);
        snprintf(child, n, "%s/%s", dir, names[i]);
        struct stat st;
        int is_dir = (nv_lstat(child, &st) == 0 && S_ISDIR(st.st_mode));
        size_t ln = strlen(prefix) + strlen(names[i]) + 2;
        char *line = (char *)malloc(ln);
        if (is_dir) snprintf(line, ln, "%s%s/", prefix, names[i]);
        else snprintf(line, ln, "%s%s", prefix, names[i]);
        nv_list_push_raw(out_list.u.list, nv_str_take(line, strlen(line)));
        if (is_dir) {
            size_t pn = strlen(prefix) + 3;
            char *nprefix = (char *)malloc(pn);
            snprintf(nprefix, pn, "%s  ", prefix);
            nv_tree_lines(child, nprefix, depth + 1, out_list);
            free(nprefix);
        }
        free(child);
        free(names[i]);
    }
    free(names);
}

// ---------- تثبيت محلي ----------
// ثبّت أداة من مسار محلي إلى مجلد البرنامج (بلا بصمة).
static NvValue nv_install_local(const char *source, int line, int col) {
    // المسار عبر السياسة (مجلد الجلسة).
    char *resolved = nv_resolve_path(source, line, col);
    // ليس .نيفا؟
    size_t sl = strlen(source);
    if (sl < 6 || strcmp(source + sl - 5, ".نيفا") != 0) {
        char *disp = nv_display_str(source);
        nv_error_fmt(NV_ERR_IO, line, col, "التثبيت يتطلب ملف .نيفا: %s", disp);
    }
    struct stat st;
    if (stat(resolved, &st) != 0) {
        char *disp = nv_display_str(source);
        nv_error_fmt(NV_ERR_IO, line, col, "الملف غير موجود: %s", disp);
    }
    if (!S_ISREG(st.st_mode)) {
        char *disp = nv_display_str(source);
        nv_error_fmt(NV_ERR_IO, line, col, "ليس ملفا: %s", disp);
    }
    // الجذع = اسم الملف بلا .نيفا.
    const char *base = strrchr(resolved, '/');
    base = base ? base + 1 : resolved;
    char *stem = nv_strdup(base);
    size_t stemlen = strlen(stem);
    if (stemlen >= 5) stem[stemlen - 5] = '\0';
    // وحدة أصلية؟
    if (nv_is_native_module(stem)) {
        nv_error_fmt(NV_ERR_IO, line, col, "%s وحدة أصلية — لا يمكن تثبيت وحدة بهذا الاسم", stem);
    }
    size_t tn = strlen(nv_program_dir) + 1 + strlen(stem) + 6;
    char *target = (char *)malloc(tn);
    snprintf(target, tn, "%s/%s.نيفا", nv_program_dir, stem);
    if (access(target, F_OK) == 0) {
        nv_error_fmt(NV_ERR_IO, line, col, "الوحدة %s مثبّتة أصلا — احذف الملف أولا لإعادة تثبيتها", stem);
    }
    // اقرأ المحتوى واكتبه.
    size_t content_len = 0;
    unsigned char *content = nv_read_file_bytes(resolved, &content_len);
    if (!content) {
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر تثبيت الوحدة %s", stem);
    }
    nv_check_quota(content_len, line, col);
    FILE *f = fopen(target, "wb");
    if (!f) {
        free(content);
        nv_error_fmt(NV_ERR_IO, line, col, "تعذر تثبيت الوحدة %s", stem);
    }
    if (content_len) fwrite(content, 1, content_len, f);
    fclose(f);
    free(content);
    char *msg = (char *)malloc(strlen(stem) + 64);
    snprintf(msg, strlen(stem) + 64, "ثُبّتت الوحدة %s", stem);
    NvValue v = nv_str_take(msg, strlen(msg));
    free(resolved);
    free(stem);
    free(target);
    return v;
}

// ---------- مساعدات التنقل (اجتماع 56 §71 — الدفعة 2) ----------

// المسار الفيزيائي لمجلد (حل الروابط)، أو NULL.
static char *nv_canonical_dir(const char *path) {
#ifdef _WIN32
    char *canon = nv_win32_realpath(path);
#else
    char *canon = realpath(path, NULL);
#endif
    if (!canon) return NULL;
    struct stat st;
    if (stat(canon, &st) != 0 || !S_ISDIR(st.st_mode)) {
        free(canon);
        return NULL;
    }
    return canon;
}

static void nv_set_cwd_owned(char *owned) {
    free(nv_cwd);
    nv_cwd = owned;
}

static void nv_after_stack_move(void) {
    if (nv_physical_mode) {
        char *canon = nv_canonical_dir(nv_cwd);
        if (canon) {
            free(nv_cwd);
            nv_cwd = canon;
        }
    }
}

static char *nv_physical_cwd(void) {
    char *canon = nv_canonical_dir(nv_cwd);
    return canon ? canon : nv_strdup(nv_cwd);
}

// هل يفصل بين الاسمين خطأ داميراو واحد بالضبط
// (إدراج/حذف/استبدال/تبديل متجاورين)؟ مقارنة على
// نقاط الشفرة لا البايتات.
static int nv_edit_distance_one(const char *a, const char *b) {
    size_t na = strlen(a), nb = strlen(b);
    unsigned int *ca = (unsigned int *)malloc((na + 1) * sizeof(unsigned int));
    unsigned int *cb = (unsigned int *)malloc((nb + 1) * sizeof(unsigned int));
    size_t la = 0, lb = 0;
    const unsigned char *p = (const unsigned char *)a;
    while (*p) {
        unsigned int cp;
        p += nv_utf8_decode(p, &cp);
        ca[la++] = cp;
    }
    p = (const unsigned char *)b;
    while (*p) {
        unsigned int cp;
        p += nv_utf8_decode(p, &cp);
        cb[lb++] = cp;
    }
    int result = 0;
    if (la == lb) {
        size_t diffs[2];
        size_t nd = 0;
        for (size_t i = 0; i < la; i++) {
            if (ca[i] != cb[i]) {
                if (nd < 2) diffs[nd] = i;
                nd++;
            }
        }
        if (nd == 1) result = 1;
        else if (nd == 2) {
            size_t i = diffs[0], j = diffs[1];
            result = (j == i + 1 && ca[i] == cb[j] && ca[j] == cb[i]);
        }
    } else if (la > lb ? (la - lb == 1) : (lb - la == 1)) {
        const unsigned int *shorter = la < lb ? ca : cb;
        const unsigned int *longer = la < lb ? cb : ca;
        size_t ls = la < lb ? la : lb;
        size_t i = 0, j = 0;
        int skipped = 0;
        result = 1;
        while (i < ls) {
            if (shorter[i] == longer[j]) {
                i++;
                j++;
            } else if (!skipped) {
                skipped = 1;
                j++;
            } else {
                result = 0;
                break;
            }
        }
    }
    free(ca);
    free(cb);
    return result;
}

// صلاحية اسم متغير (مرآة env::valid_name في Rust):
// حروف عربية وأرقام وشرطة سفلية، ولا يبدأ برقم.
static int nv_valid_var_name(const char *s) {
    if (!s[0]) return 0;
    const unsigned char *p = (const unsigned char *)s;
    int first = 1;
    while (*p) {
        unsigned int cp;
        p += nv_utf8_decode(p, &cp);
        if (first) {
            long d;
            if (nv_is_arabic_digit(cp, &d)) return 0;
            first = 0;
        }
        long d;
        if (!(cp == '_' || nv_is_arabic_digit(cp, &d)
              || (cp >= 0x0600 && cp <= 0x06FF) || (cp >= 0x0750 && cp <= 0x077F)
              || (cp >= 0x08A0 && cp <= 0x08FF) || (cp >= 0xFB50 && cp <= 0xFDFF)
              || (cp >= 0xFE70 && cp <= 0xFEFF))) {
            return 0;
        }
    }
    return 1;
}

// حل فهرس مكدس (سالب من النهاية) إلى موضع. 1 = صالح.
static int nv_stack_index(long idx, size_t len, size_t *pos_out) {
    if (len == 0) return 0;
    long l = (long)len;
    long pos = idx >= 0 ? idx : l + idx;
    if (pos < 0 || pos >= l) return 0;
    *pos_out = (size_t)pos;
    return 1;
}

// فهرس مكدس من قيمة: عدد صحيح، أو نص رقمي (لعبارة
// REPL). 1 = فهرس، 0 = ليس فهرسا.
static int nv_stack_index_value(NvValue *v, long *out) {
    if (v->tag == NV_NUM) {
        double n = v->u.num;
        if (n != trunc(n) || n < -9007199254740992.0 || n > 9007199254740992.0) return 0;
        *out = (long)n;
        return 1;
    }
    if (v->tag == NV_STR) return nv_parse_arabic_int(v->u.str->data, out);
    return 0;
}

static void nv_dirstack_push_front(char *owned) {
    if (nv_dirstack_len == nv_dirstack_cap) {
        nv_dirstack_cap = nv_dirstack_cap ? nv_dirstack_cap * 2 : 8;
        nv_dirstack = (char **)realloc(nv_dirstack, nv_dirstack_cap * sizeof(char *));
    }
    memmove(&nv_dirstack[1], &nv_dirstack[0], nv_dirstack_len * sizeof(char *));
    nv_dirstack[0] = owned;
    nv_dirstack_len++;
}

static void nv_dirstack_remove(size_t pos) {
    free(nv_dirstack[pos]);
    memmove(&nv_dirstack[pos], &nv_dirstack[pos + 1], (nv_dirstack_len - pos - 1) * sizeof(char *));
    nv_dirstack_len--;
}

static void nv_dirstack_clear(void) {
    for (size_t i = 0; i < nv_dirstack_len; i++) free(nv_dirstack[i]);
    nv_dirstack_len = 0;
}

// تدوير إلى الموضع pos: العنصر يصير الحالي،
// والحالي القديم يدخل مكانه.
static void nv_dirstack_rotate(size_t pos) {
    char *target = nv_dirstack[pos];
    nv_dirstack[pos] = nv_strdup(nv_cwd);
    free(nv_cwd);
    nv_cwd = target;
}

// محاولة واحدة على مسار محلول: 0 نجاح (*out يملكه
// المتصل)، 1 غائب، 2 موجود ليس مجلدا.
static int nv_try_resolved(const char *resolved, int physical, char **out) {
    if (physical) {
        char *canon = nv_canonical_dir(resolved);
        if (canon) {
            *out = canon;
            return 0;
        }
        struct stat st;
        if (stat(resolved, &st) == 0) return 2;
        return 1;
    }
    struct stat st;
    if (stat(resolved, &st) != 0) return 1;
    if (!S_ISDIR(st.st_mode)) return 2;
    *out = nv_normalize_lexical(resolved);
    return 0;
}

// محاولة مرشح نصي واحد: 0 نجاح، 1 غائب، 2 ليس
// مجلدا، 3 رفض سياسة (يُتخطى في السلسلة).
// بوابة الوجود (إصلاح انهيار `..` المعجمي — مرآة
// Rust): يُرفض المرشح إن غاب مساره الموصول كما
// كُتب قبل أي تطبيع.
static int nv_try_candidate(const char *raw, int physical, int line, int col, char **out) {
    int policy_err = 0;
    char *resolved = nv_resolve_path_soft(raw, line, col, &policy_err);
    if (!resolved) return 3;
    char *plus = nv_expand_tilde_plus(raw, line, col);
    char *gate_given = plus ? plus : nv_expand_home(raw);
    char *gate_joined;
    if (nv_is_absolute_path(gate_given)) {
        gate_joined = gate_given;
    } else {
        size_t n = strlen(nv_cwd) + 1 + strlen(gate_given) + 1;
        gate_joined = (char *)malloc(n);
        snprintf(gate_joined, n, "%s/%s", nv_cwd, gate_given);
        free(gate_given);
    }
    struct stat gst;
    int gr = stat(gate_joined, &gst);
    free(gate_joined);
    if (gr != 0) {
        free(resolved);
        return 1;
    }
    if (!S_ISDIR(gst.st_mode)) {
        free(resolved);
        return 2;
    }
    int r = nv_try_resolved(resolved, physical, out);
    free(resolved);
    return r;
}

// المحاولة الحرفية (الميزة 13): النص كما كُتب بلا
// تطبيع معجمي، والنجاح من المسار الحقيقي؛ أو NULL.
static char *nv_resolve_literal_soft(const char *raw) {
    char *given = nv_expand_home(raw);
    char *joined;
    if (nv_is_absolute_path(given)) {
        joined = given;
    } else {
        size_t n = strlen(nv_cwd) + 1 + strlen(given) + 1;
        joined = (char *)malloc(n);
        snprintf(joined, n, "%s/%s", nv_cwd, given);
        free(given);
    }
    char *canon;
#ifdef _WIN32
    canon = nv_win32_realpath(joined);
#else
    canon = realpath(joined, NULL);
#endif
    free(joined);
    if (!canon) return NULL;
    if (nv_trusted) return canon;
    char *base_norm = nv_normalize_lexical(nv_program_dir);
    char *canon_base = nv_canonical_with_suffix(base_norm);
    int ok = nv_path_starts_with(canon, canon_base);
    free(base_norm);
    free(canon_base);
    if (!ok) {
        free(canon);
        return NULL;
    }
    return canon;
}

// تصحيح إملاء المكون الأخير (الميزة 17): 1 نجاح
// (*out)، 0 بلا تصحيح وحيد.
static int nv_spell_correct(const char *raw, int physical, int line, int col, char **out) {
    size_t rl = strlen(raw);
    if (rl == 0 || raw[rl - 1] == '/') return 0;
    const char *slash = strrchr(raw, '/');
    const char *last = slash ? slash + 1 : raw;
    if (last[0] == '\0' || strcmp(last, ".") == 0 || strcmp(last, "..") == 0) return 0;
    char *parent_path;
    if (!slash) {
        parent_path = nv_strdup(nv_cwd);
    } else {
        size_t plen = (size_t)(slash - raw);
        char *parent = (char *)malloc(plen + 1);
        memcpy(parent, raw, plen);
        parent[plen] = '\0';
        int pe = 0;
        parent_path = nv_resolve_path_soft(parent, line, col, &pe);
        free(parent);
        if (!parent_path) return 0;
    }
    DIR *d = opendir(parent_path);
    if (!d) {
        free(parent_path);
        return 0;
    }
    char *found = NULL;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        const char *name = e->d_name;
        if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) continue;
        size_t n = strlen(parent_path) + 1 + strlen(name) + 1;
        char *child = (char *)malloc(n);
        snprintf(child, n, "%s/%s", parent_path, name);
        struct stat st;
        int is_dir = (stat(child, &st) == 0 && S_ISDIR(st.st_mode));
        if (is_dir && nv_edit_distance_one(last, name)) {
            if (found) {
                free(child);
                free(found);
                closedir(d);
                free(parent_path);
                return 0;
            }
            found = child;
        } else {
            free(child);
        }
    }
    closedir(d);
    free(parent_path);
    if (!found) return 0;
    int r = nv_try_resolved(found, physical, out);
    free(found);
    return r == 0;
}

// سلسلة محاولات `انتقل` كاملة (مرآة change_dir في
// Rust): مطلق، ثم CDPATH، ثم الحالي، ثم الحرفي،
// ثم قيمة المتغير، ثم تصحيح الإملاء. النجاح يضبط
// nv_cwd؛ والفشل خطأ عربي (بلا عودة).
// flag: -1 بلا علم (الوضع العام)، 0 منطقي، 1 فيزيائي.
static void nv_change_dir(const char *raw, int flag, int line, int col) {
    int physical = (flag == 1) || (flag == -1 && nv_physical_mode);
    int is_abs = (raw[0] == '/') || strcmp(raw, "~") == 0 || strncmp(raw, "~/", 2) == 0
        || strcmp(raw, "$نيفا") == 0 || strncmp(raw, "$نيفا/", 10) == 0;
    int saw_not_dir = 0;
    int direct_policy = 0;
    char *out = NULL;
    if (is_abs) {
        int r = nv_try_candidate(raw, physical, line, col, &out);
        if (r == 0) {
            nv_set_cwd_owned(out);
            return;
        }
        if (r == 2) saw_not_dir = 1;
        if (r == 3) direct_policy = 1;
    } else {
        /* POSIX (مثبت بفحوص باش 5.2 الحية — مرآة
           Rust): لا `CDPATH` حين يكون المكون الأول
           `.` أو `..` (`.` و`..` و`./...` و`../...`). */
        int use_cdpath = !(strcmp(raw, ".") == 0 || strcmp(raw, "..") == 0
            || strncmp(raw, "./", 2) == 0 || strncmp(raw, "../", 3) == 0);
        if (use_cdpath) {
            for (size_t i = 0; i < nv_cdpath_len; i++) {
                char *cand;
                if (nv_cdpath[i][0] == '\0') {
                    cand = nv_strdup(raw);
                } else {
                    size_t n = strlen(nv_cdpath[i]) + 1 + strlen(raw) + 1;
                    cand = (char *)malloc(n);
                    snprintf(cand, n, "%s/%s", nv_cdpath[i], raw);
                }
                int r = nv_try_candidate(cand, physical, line, col, &out);
                free(cand);
                if (r == 0) {
                    nv_set_cwd_owned(out);
                    return;
                }
                if (r == 2) saw_not_dir = 1;
            }
        }
        int r = nv_try_candidate(raw, physical, line, col, &out);
        if (r == 0) {
            nv_set_cwd_owned(out);
            return;
        }
        if (r == 2) saw_not_dir = 1;
        if (r == 3) direct_policy = 1;
    }
    char *lit = nv_resolve_literal_soft(raw);
    if (lit) {
        struct stat st;
        if (stat(lit, &st) == 0 && S_ISDIR(st.st_mode)) {
            nv_set_cwd_owned(lit);
            return;
        }
        free(lit);
    }
    if (nv_valid_var_name(raw) && nv_current_call_scope) {
        NvCell *cell = nv_scope_lookup_cell(nv_current_call_scope, raw);
        if (cell && cell->val.tag == NV_STR) {
            const char *val = cell->val.u.str->data;
            int r = nv_try_candidate(val, physical, line, col, &out);
            if (r == 0) {
                nv_set_cwd_owned(out);
                return;
            }
            if (r == 2) saw_not_dir = 1;
        }
    }
    if (nv_spell_correct(raw, physical, line, col, &out)) {
        nv_set_cwd_owned(out);
        return;
    }
    if (direct_policy) {
        // أعد الحل الصلب ليُخرج رسالة السياسة نفسها.
        char *resolved = nv_resolve_path(raw, line, col);
        free(resolved);
    }
    char *disp = nv_display_str(raw);
    if (saw_not_dir) nv_error_fmt(NV_ERR_IO, line, col, "ليس مجلدا: %s", disp);
    nv_error_fmt(NV_ERR_IO, line, col, "المجلد غير موجود: %s", disp);
}

// ---------- الدفعة 4 — المتغيرات (اجتماع 58 §73) ----------

// مولد عشوائي صغير (xorshift64) لبـ`بيئة("عشوائي")`
// — بذرة من الوقت ورقم العملية، كمولد Rust.
static unsigned long long nv_rng_state = 0;
static unsigned long long nv_rng_next(void) {
    if (!nv_rng_state) {
        nv_rng_state = ((unsigned long long)time(NULL) << 1) ^ 0x2545F4914F6CDD1DULL;
#ifdef _WIN32
        nv_rng_state ^= (unsigned long long)GetCurrentProcessId() * 0x9E3779B97F4A7C15ULL;
#else
        nv_rng_state ^= (unsigned long long)getpid() * 0x9E3779B97F4A7C15ULL;
#endif
        if (!nv_rng_state) nv_rng_state = 0x2545F4914F6CDD1DULL;
    }
    unsigned long long x = nv_rng_state;
    x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
    nv_rng_state = x;
    return x * 0x2545F4914F6CDD1DULL;
}

// قراءة متغير بيئة واحد (الثابت + الديناميكي) —
// يشارك فيه ذراعا `بيئة` و`اعرض_البيئة`.
static NvValue nv_env_read(const char *n, int line, int col) {
    if (strcmp(n, "المجلد_الحالي") == 0) {
        char *disp;
        if (nv_physical_mode) {
            char *phys = nv_physical_cwd();
            disp = nv_display_path(phys);
            free(phys);
        } else {
            disp = nv_display_path(nv_cwd);
        }
        return nv_str_take(disp, strlen(disp));
    }
    if (strcmp(n, "المستخدم") == 0) {
        char buf[256];
#if defined(__linux__) || defined(__ANDROID__)
        if (!nv_read_username(buf, sizeof(buf))) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذر قراءة اسم المستخدم");
        }
#else
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "بيئة(\"المستخدم\") غير مدعومة على هذه المنصة");
#endif
        return nv_str_cstr(buf);
    }
    if (strcmp(n, "اسم_المضيف") == 0) {
        char buf[256];
#if defined(__linux__) || defined(__ANDROID__)
        if (!nv_read_hostname(buf, sizeof(buf))) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "تعذر قراءة اسم المضيف");
        }
#else
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "بيئة(\"اسم_المضيف\") غير مدعومة على هذه المنصة");
#endif
        return nv_str_cstr(buf);
    }
    if (strcmp(n, "عشوائي") == 0) {
        char *s = nv_fmt_number((double)(nv_rng_next() % 32768));
        NvValue v = nv_str_cstr(s);
        free(s);
        return v;
    }
    if (strcmp(n, "ثواني") == 0) {
        unsigned long long now = (unsigned long long)time(NULL);
        unsigned long long secs = now >= nv_start_secs ? now - nv_start_secs : 0;
        char *s = nv_fmt_number((double)secs);
        NvValue v = nv_str_cstr(s);
        free(s);
        return v;
    }
    if (strcmp(n, "رقم_السطر") == 0) {
        char *s = nv_fmt_number((double)line);
        NvValue v = nv_str_cstr(s);
        free(s);
        return v;
    }
    const char *v = NULL;
    if (strcmp(n, "البيت") == 0) v = "~";
    else if (strcmp(n, "مجلد_البرنامج") == 0) v = "$نيفا";
    else if (strcmp(n, "المسار") == 0) v = "$نيفا";
    else if (strcmp(n, "إصدار_نيفا") == 0) v = "١٫٠٫٠";
    else if (strcmp(n, "الطرفية") == 0) v = "نيفا";
    else if (strcmp(n, "اللغة") == 0) v = "ar_EG.UTF-8";
    else if (strcmp(n, "المجلد_المؤقت") == 0) v = "$نيفا/tmp";
    else if (strcmp(n, "الألوان") == 0) v = "truecolor";
    else {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير بيئة غير معروف: %s", n);
    }
    return nv_str_cstr(v);
}

// ---------- الدوال المدمجة: الجدول والتوزيع ----------
int nv_is_builtin(const char *name) {
    static const char *names[] = {
        "طول", "مدى", "مجموع", "أدنى", "أقصى", "مطلق", "عدد", "نص", "نوع",
        "اقرأ_ملف", "اكتب_ملف", "اعرض_الملفات", "المجلد_الحالي", "انتقل",
        "أنشئ_مجلد", "احذف", "احذف_مجلد", "احذف_ملف", "انسخ", "انقل",
        "أنشئ_ملف", "اعرض_البداية", "اعرض_النهاية", "عد_الأسطر", "عد_الكلمات",
        "ابحث_في_ملف", "ابحث_عن", "اعرض_الشجرة", "حجم_المسار", "أنشئ_رابط",
        "التاريخ_الآن", "النسخة", "موجود", "نوع_المسار", "تفاصيل_المسار",
        "تثبيت", "عد_الملفات", "ادخل", "بيئة",
        // الدفعة 1 — رموز الخروج (اجتماع 54 §69).
        "رمز_الخروج", "اسم_البرنامج", "خروج",
        // اجتماع 56 §71 — الدفعة 2 (التنقل): المكدس
        // وCDPATH والوضع الفيزيائي.
        "ادفع_مجلد", "أخرج_مجلد", "اعرض_المكدس", "امسح_المكدس",
        "مسار_بحث_المجلدات", "الوضع_الفيزيائي",
        // الدفعة 3 — التشغيل (اجتماع 57 §72): وصف
        // الأمر ومكان الأداة والاستبدال.
        "نوع_الامر", "مكان_الاداة", "استبدل_الجلسة",
        // الدفعة 4 — المتغيرات (اجتماع 58 §73).
        "جمد", "احذف_متغير", "افحص_متغير", "مرجع", "اعرض_البيئة", "مفاتيح",
        // الدفعة 5 — أدوات الملفات (اجتماع 59 §74).
        "انسخ_تكراري", "انسخ_محدث", "انسخ_أرشيف", "انسخ_خفيف", "احذف_تكراري",
        "أنشئ_رابط_صلب", "أنشئ_رابط_نسبي",
        // الدفعة 3 من الـ579 — الحاويات والفهم
        // (البنود 138 و139 و141–143).
        "مترابطة", "تشكيلة", "افهم_قائمة", "افهم_تشكيلة", "افهم_فهرس",
        // ج6 — مدمجات الصيغ الحرفية المفككة في المحلل.
        "عدد_كبير", "كسر", "مركب", "كسر_حرفي", "مركب_حرفي",
        // ب6 — الاستدعاءات الداخلية المفككة في المحلل
        // (`ولد`/`ولد من`/التعبير التوليدي): الثنائي
        // الأصلي يرفضها بالرسالة القانونية (انظر
        // nv_call_builtin) — بلا تغيير في cgen.rs.
        "__ولد", "__ولد_من", "__مولد_تعبير",
        // البند 88 — مدير السياق (الاجتماع 88 §103):
        // الاستدعاءات الداخلية المفككة في المحلل من `عند`.
        "__عند_افحص", "__عند_خطأ", "__عند_أعد_رفع", NULL
    };
    for (int i = 0; names[i]; i++) {
        if (strcmp(name, names[i]) == 0) return 1;
    }
    return 0;
}


// هل المساران ملف واحد؟ مرآة `paths_same_file` في
// files.rs: على يونكس جهاز+عقدة من stat؛ أما على
// ويندوز فـstat عبر MinGW كاذب — يعيد st_ino=0 لكل
// الملفات (مثبت بمسبار C تحت Wine: ملفان مختلفان
// يعيدان st_dev=25 وst_ino=0) فيرفض أي نسخ بين
// ملفين مختلفين — فالهوية الحقيقية من
// GetFileInformationByHandle (رقم المجلد + فهرس
// الملف). يرجع 0 عند أي فشل (لا تطابق مفترضا).
static int nv_same_file(const char *a, const char *b) {
#ifdef _WIN32
    HANDLE ha = CreateFileA(a, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, NULL);
    if (ha == INVALID_HANDLE_VALUE) return 0;
    HANDLE hb = CreateFileA(b, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, NULL);
    if (hb == INVALID_HANDLE_VALUE) {
        CloseHandle(ha);
        return 0;
    }
    BY_HANDLE_FILE_INFORMATION ia, ib;
    int same = 0;
    if (GetFileInformationByHandle(ha, &ia) && GetFileInformationByHandle(hb, &ib)) {
        same = (ia.dwVolumeSerialNumber == ib.dwVolumeSerialNumber
            && ia.nFileIndexHigh == ib.nFileIndexHigh
            && ia.nFileIndexLow == ib.nFileIndexLow);
    }
    CloseHandle(ha);
    CloseHandle(hb);
    return same;
#else
    struct stat sa, sb;
    if (stat(a, &sa) != 0 || stat(b, &sb) != 0) return 0;
    return sa.st_dev == sb.st_dev && sa.st_ino == sb.st_ino;
#endif
}

// نسخ ملف واحد (الميزة 204): يشارك فيه `انسخ`
// ومسار نسخ-ثم-حذف في `انقل`. يرجع 0 عند النجاح،
// و-1 عند الفشل مع بقاء errno.
static int nv_copy_file(const char *src, const char *dst) {
    FILE *in = fopen(src, "rb");
    FILE *out = in ? fopen(dst, "wb") : NULL;
    if (!in || !out) {
        int e = errno;
        if (in) fclose(in);
        if (out) fclose(out);
        errno = e;
        return -1;
    }
    unsigned char buf[65536];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), in)) > 0) {
        if (fwrite(buf, 1, n, out) != n) {
            int e = errno;
            fclose(in);
            fclose(out);
            errno = e ? e : EIO;
            return -1;
        }
    }
    if (ferror(in)) {
        int e = errno;
        fclose(in);
        fclose(out);
        errno = e ? e : EIO;
        return -1;
    }
    if (fclose(out) != 0) {
        int e = errno;
        fclose(in);
        errno = e;
        return -1;
    }
    fclose(in);
    return 0;
}
// نسخ شجرة تكراريا (الميزة 204): ملف بمحتواه،
// ومجلد بأبنائه، ورابط رمزي يعاد رابطا بلا اتباع
// هدفه. ومجلد هدف موجود فارغ يستخدم كما هو.
static int nv_copy_tree(const char *src, const char *dst) {
    struct stat st;
    if (nv_lstat(src, &st) != 0) return -1;
#ifndef _WIN32
    if (S_ISLNK(st.st_mode)) {
        char target[4096];
        ssize_t n = readlink(src, target, sizeof(target) - 1);
        if (n < 0) return -1;
        target[n] = '\0';
        if (symlink(target, dst) != 0) return -1;
        return 0;
    }
#endif
    if (S_ISDIR(st.st_mode)) {
        int rc;
#ifdef _WIN32
        rc = mkdir(dst);
#else
        rc = mkdir(dst, 0755);
#endif
        if (rc != 0) {
            if (errno != EEXIST) return -1;
            struct stat dst_st;
            if (stat(dst, &dst_st) != 0 || !S_ISDIR(dst_st.st_mode)) return -1;
            DIR *chk = opendir(dst);
            if (!chk) return -1;
            struct dirent *ce;
            int non_empty = 0;
            while ((ce = readdir(chk)) != NULL) {
                if (strcmp(ce->d_name, ".") != 0 && strcmp(ce->d_name, "..") != 0) {
                    non_empty = 1;
                    break;
                }
            }
            closedir(chk);
            if (non_empty) {
                errno = EEXIST;
                return -1;
            }
        }
        DIR *d = opendir(src);
        if (!d) return -1;
        struct dirent *e;
        int ret = 0;
        int saved = 0;
        while ((e = readdir(d)) != NULL) {
            if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
            size_t ns = strlen(src) + 1 + strlen(e->d_name) + 1;
            size_t nd = strlen(dst) + 1 + strlen(e->d_name) + 1;
            char *csrc = (char *)malloc(ns);
            char *cdst = (char *)malloc(nd);
            if (!csrc || !cdst) {
                free(csrc);
                free(cdst);
                saved = ENOMEM;
                ret = -1;
                break;
            }
            snprintf(csrc, ns, "%s/%s", src, e->d_name);
            snprintf(cdst, nd, "%s/%s", dst, e->d_name);
            if (nv_copy_tree(csrc, cdst) != 0) {
                saved = errno;
                ret = -1;
                free(csrc);
                free(cdst);
                break;
            }
            free(csrc);
            free(cdst);
        }
        closedir(d);
        if (ret != 0) errno = saved;
        return ret;
    }
    return nv_copy_file(src, dst);
}
// حذف شجرة (الميزة 204): ملف أو رابط يزال وحده،
// ومجلد يزال تكراريا.
static int nv_remove_tree(const char *path) {
    struct stat st;
    if (nv_lstat(path, &st) != 0) return -1;
    if (!S_ISDIR(st.st_mode)) return unlink(path);
    DIR *d = opendir(path);
    if (!d) return -1;
    struct dirent *e;
    int ret = 0;
    int saved = 0;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        size_t n = strlen(path) + 1 + strlen(e->d_name) + 1;
        char *child = (char *)malloc(n);
        if (!child) {
            saved = ENOMEM;
            ret = -1;
            break;
        }
        snprintf(child, n, "%s/%s", path, e->d_name);
        if (nv_remove_tree(child) != 0) {
            saved = errno;
            ret = -1;
            free(child);
            break;
        }
        free(child);
    }
    closedir(d);
    if (ret != 0) {
        errno = saved;
        return -1;
    }
    if (rmdir(path) != 0) return -1;
    return 0;
}

// ---------- الدفعة 5 — أدوات الملفات (اجتماع 59 §74) ----------
// مساعدات الميزات 196 و197 و199 و200 و201 و206 و208
// و211 و212 و213 و215 — مرآة files.rs في Rust.

// وقت التعديل بدقة النظام (للمقارنة في `انسخ_محدث`):
// نانو ثانية على يونكس، وثانية على ويندوز.
#ifdef __APPLE__
#define NV_MTIME_SEC(st) ((st).st_mtimespec.tv_sec)
#define NV_MTIME_NSEC(st) ((st).st_mtimespec.tv_nsec)
#elif defined(_WIN32)
#define NV_MTIME_SEC(st) ((st).st_mtime)
#define NV_MTIME_NSEC(st) (0)
#else
#define NV_MTIME_SEC(st) ((st).st_mtim.tv_sec)
#define NV_MTIME_NSEC(st) ((st).st_mtim.tv_nsec)
#endif

// رفض حذف الجذر نصا (الميزة 208): قاعدة صريحة قبل
// أي حل ساندبوكس. المكون الأخير `.` أو `..` مرفوض،
// والجذر `/` مرفوض؛ والمسار الفارغ ليس جذرا.
// يرجع رسالة الرفض أو NULL.
static const char *nv_root_refusal_text(const char *path) {
    if (!path || !path[0]) return NULL;
    size_t n = strlen(path);
    while (n > 0 && (path[n - 1] == '/' || path[n - 1] == '\\')) n--;
    if (n == 0) return "لا يمكن حذف الجذر: /";
    size_t start = n;
    while (start > 0 && path[start - 1] != '/' && path[start - 1] != '\\') start--;
    size_t last_len = n - start;
    if ((last_len == 1 && path[start] == '.')
        || (last_len == 2 && path[start] == '.' && path[start + 1] == '.'))
        return "لا يمكن حذف . أو ..";
    return NULL;
}

// هل هذا المسار المحلول هو جذر نظام الملفات `/`
// نفسه — بما فيه عبر رابط رمزي؟ المقارنة قانونية
// (تتبع الروابط) وجهاز+عقدة على يونكس، كما في
// `ROOT_DEV_INO_CHECK` في لينكس (الميزة 208).
static int nv_is_filesystem_root(const char *path) {
    char *canon = nv_canonical_with_suffix(path);
    int root = canon && strcmp(canon, "/") == 0;
    free(canon);
#ifndef _WIN32
    if (!root) {
        struct stat st, rst;
        if (stat(path, &st) == 0 && stat("/", &rst) == 0)
            root = (st.st_dev == rst.st_dev && st.st_ino == rst.st_ino);
    }
#endif
    return root;
}

// حلل وضع أذونات ثمانيا («755» أو «0755») —
// 3 أو 4 أرقام 0–7 فقط (الميزة 211). يرجع 0 عند
// النجاح والوضع في *out، و-1 عند الفشل.
// هل هدف الحذف هو الجذر؟ فحص قانوني قبل حل
// الساندبوكس (الميزة 208): يلحق المسار بالمجلد
// الحالي ثم يقنّن أطول بادئة موجودة — فيكشف
// الرابط الرمزي إلى الجذر قبل أن يرفضه
// الساندبوكس برسالة عامة.
static int nv_delete_target_is_root(const char *path) {
    char *joined;
    if (nv_is_absolute_path(path)) {
        joined = nv_strdup(path);
    } else {
        size_t n = strlen(nv_cwd) + 1 + strlen(path) + 1;
        joined = (char *)malloc(n);
        snprintf(joined, n, "%s/%s", nv_cwd, path);
    }
    char *canon = nv_canonical_with_suffix(joined);
    int root = nv_is_filesystem_root(canon);
    free(joined);
    free(canon);
    return root;
}

static int nv_parse_octal(const char *s, unsigned *out) {
    size_t n = strlen(s);
    if (n < 3 || n > 4) return -1;
    unsigned v = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] < '0' || s[i] > '7') return -1;
        v = v * 8 + (unsigned)(s[i] - '0');
    }
    *out = v;
    return 0;
}

// المسار النسبي من مجلد إلى هدف (الميزة 213 —
// `ln -r`): مكونات مشتركة ثم `..` لكل مكون زائد
// في مجلد المصدر. والطرفان مطلقان محلولان عند
// الاستدعاء؛ وبلا بادئة مشتركة (قرصان مختلفان
// على ويندوز) يبقى الهدف المطلق كما هو. يرجع
// سلسلة مملوكة للمتصل.
static char *nv_relative_path(const char *from_dir, const char *to) {
    // قسّم إلى مكونات على '/'.
    size_t fn = 0, tn = 0;
    char **fc = NULL, **tc = NULL;
    const char *p = from_dir;
    while (*p) {
        while (*p == '/') p++;
        if (!*p) break;
        const char *e = strchr(p, '/');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        fc = (char **)realloc(fc, (fn + 1) * sizeof(char *));
        fc[fn] = (char *)malloc(l + 1);
        memcpy(fc[fn], p, l);
        fc[fn][l] = '\0';
        fn++;
        p = e ? e : p + l;
    }
    p = to;
    while (*p) {
        while (*p == '/') p++;
        if (!*p) break;
        const char *e = strchr(p, '/');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        tc = (char **)realloc(tc, (tn + 1) * sizeof(char *));
        tc[tn] = (char *)malloc(l + 1);
        memcpy(tc[tn], p, l);
        tc[tn][l] = '\0';
        tn++;
        p = e ? e : p + l;
    }
    size_t i = 0;
    while (i < fn && i < tn && strcmp(fc[i], tc[i]) == 0) i++;
    if (i == 0) {
        for (size_t k = 0; k < fn; k++) free(fc[k]);
        for (size_t k = 0; k < tn; k++) free(tc[k]);
        free(fc);
        free(tc);
        return nv_strdup(to);
    }
    size_t total = 1;
    for (size_t k = i; k < fn; k++) total += 3;
    for (size_t k = i; k < tn; k++) total += strlen(tc[k]) + 1;
    char *out = (char *)malloc(total + 1);
    size_t w = 0;
    for (size_t k = i; k < fn; k++) {
        if (w > 0) out[w++] = '/';
        memcpy(out + w, "..", 2);
        w += 2;
    }
    for (size_t k = i; k < tn; k++) {
        if (w > 0) out[w++] = '/';
        size_t l = strlen(tc[k]);
        memcpy(out + w, tc[k], l);
        w += l;
    }
    if (w == 0) {
        out[w++] = '.';
    }
    out[w] = '\0';
    for (size_t k = 0; k < fn; k++) free(fc[k]);
    for (size_t k = 0; k < tn; k++) free(tc[k]);
    free(fc);
    free(tc);
    return out;
}

// المكونات الناقصة من مسار محلول (من أول سلف موجود
// إلى المسار نفسه) — لإعلان `mkdir -v` (الميزة
// 211): تُحسب قبل الإنشاء، فالقائمة الفارغة تعني
// «موجود أصلا». يرجع مصفوفة مملوكة للمتصل وعددها
// في *out_n (بالترتيب: الأعلى أولا).
static char **nv_missing_components(const char *path, size_t *out_n) {
    char **stack = NULL;
    size_t n = 0, cap = 0;
    char *cur = nv_strdup(path);
    while (1) {
        struct stat st;
        if (stat(cur, &st) == 0) break;
        if (n == cap) {
            cap = cap ? cap * 2 : 8;
            stack = (char **)realloc(stack, cap * sizeof(char *));
        }
        stack[n++] = nv_strdup(cur);
        char *slash = strrchr(cur, '/');
        if (!slash || slash == cur) break;
        *slash = '\0';
    }
    free(cur);
    // اقلب الترتيب (الأعلى أولا).
    for (size_t i = 0; i < n / 2; i++) {
        char *t = stack[i];
        stack[i] = stack[n - 1 - i];
        stack[n - 1 - i] = t;
    }
    *out_n = n;
    return stack;
}

// عرض مسار منشأ لإعلان `mkdir -v` (الميزة 211):
// نسبي إلى المجلد الحالي إن كان تحته، وإلا بصيغة
// العرض القائمة — مرآة `display_created` في Rust.
static char *nv_created_display(const char *path) {
    size_t cl = strlen(nv_cwd);
    if (strncmp(path, nv_cwd, cl) == 0 && path[cl] == '/') {
        return nv_strdup(path + cl + 1);
    }
    return nv_display_path(path);
}

// حدّث وقتي الوصول والتعديل إلى الآن بلا مساس
// بالمحتوى (الميزة 215 — `touch`).
static int nv_touch(const char *path) {
#ifdef _WIN32
    {
        struct _utimbuf tb;
        tb.actime = time(NULL);
        tb.modtime = tb.actime;
        return _utime(path, &tb);
    }
#else
    return utimensat(AT_FDCWD, path, NULL, 0);
#endif
}

// احفظ أذونات المصدر ووقتي الوصول والتعديل على
// نسخة (الميزة 200 — الأرشيف). الأذونات على
// يونكس وحده؛ والأوقات بـ`utimensat` خارجهما
// أفضل جهد (لا تفشل نسخا ناجحا).
static void nv_preserve_attrs(const struct stat *sst, const char *dst) {
#ifndef _WIN32
    chmod(dst, sst->st_mode & 07777);
    struct timespec ts[2];
#ifdef __APPLE__
    ts[0] = sst->st_atimespec;
    ts[1] = sst->st_mtimespec;
#else
    ts[0] = sst->st_atim;
    ts[1] = sst->st_mtim;
#endif
    utimensat(AT_FDCWD, dst, ts, 0);
#else
    (void)sst;
    (void)dst;
#endif
}

// جدول الروابط الصلبة في نسخ الأرشيف (الميزة 200):
// جهاز+عقدة → أول هدف منسوخ (يونكس وحده؛ وعلى
// ويندوز بلا تتبع فينسخ المكرر عاديا).
#ifndef _WIN32
typedef struct {
    dev_t dev;
    ino_t ino;
    char *dst;
} NvLinkMap;
#endif

// نسخ أرشيف (الميزة 200): تكراري مع حفظ الروابط
// الرمزية روابط والروابط الصلبة روابط صلبة
// والأذونات والأوقات. الملكية وسياق الأمان
// والسمات الموسعة لا تُحفظ (مسجل في §74).
#ifndef _WIN32
static int nv_copy_archive_inner(
    const char *src,
    const char *dst,
    NvLinkMap **map,
    size_t *map_len,
    size_t *map_cap
) {
    struct stat st;
    if (nv_lstat(src, &st) != 0) return -1;
    if (S_ISLNK(st.st_mode)) {
        char target[4096];
        ssize_t n = readlink(src, target, sizeof(target) - 1);
        if (n < 0) return -1;
        target[n] = '\0';
        if (symlink(target, dst) != 0) return -1;
        return 0;
    }
    if (S_ISDIR(st.st_mode)) {
        int rc = mkdir(dst, 0755);
        if (rc != 0) {
            if (errno != EEXIST) return -1;
            struct stat dst_st;
            if (stat(dst, &dst_st) != 0 || !S_ISDIR(dst_st.st_mode)) return -1;
            DIR *chk = opendir(dst);
            if (!chk) return -1;
            struct dirent *ce;
            int non_empty = 0;
            while ((ce = readdir(chk)) != NULL) {
                if (strcmp(ce->d_name, ".") != 0 && strcmp(ce->d_name, "..") != 0) {
                    non_empty = 1;
                    break;
                }
            }
            closedir(chk);
            if (non_empty) {
                errno = EEXIST;
                return -1;
            }
        }
        DIR *d = opendir(src);
        if (!d) return -1;
        struct dirent *e;
        int ret = 0;
        int saved = 0;
        while ((e = readdir(d)) != NULL) {
            if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
            size_t ns = strlen(src) + 1 + strlen(e->d_name) + 1;
            size_t nd = strlen(dst) + 1 + strlen(e->d_name) + 1;
            char *csrc = (char *)malloc(ns);
            char *cdst = (char *)malloc(nd);
            if (!csrc || !cdst) {
                free(csrc);
                free(cdst);
                saved = ENOMEM;
                ret = -1;
                break;
            }
            snprintf(csrc, ns, "%s/%s", src, e->d_name);
            snprintf(cdst, nd, "%s/%s", dst, e->d_name);
            if (nv_copy_archive_inner(csrc, cdst, map, map_len, map_cap) != 0) {
                saved = errno;
                ret = -1;
                free(csrc);
                free(cdst);
                break;
            }
            free(csrc);
            free(cdst);
        }
        closedir(d);
        if (ret != 0) {
            errno = saved;
            return ret;
        }
        nv_preserve_attrs(&st, dst);
        return 0;
    }
    if (st.st_nlink > 1) {
        for (size_t i = 0; i < *map_len; i++) {
            if ((*map)[i].dev == st.st_dev && (*map)[i].ino == st.st_ino) {
                if (link((*map)[i].dst, dst) != 0) return -1;
                return 0;
            }
        }
    }
    if (nv_copy_file(src, dst) != 0) return -1;
    if (st.st_nlink > 1) {
        if (*map_len == *map_cap) {
            *map_cap = *map_cap ? *map_cap * 2 : 16;
            *map = (NvLinkMap *)realloc(*map, *map_cap * sizeof(NvLinkMap));
        }
        (*map)[*map_len].dev = st.st_dev;
        (*map)[*map_len].ino = st.st_ino;
        (*map)[*map_len].dst = nv_strdup(dst);
        (*map_len)++;
    }
    nv_preserve_attrs(&st, dst);
    return 0;
}
#endif

static int nv_copy_archive(const char *src, const char *dst) {
#ifdef _WIN32
    // ويندوز: بلا `link` ولا `utimensat` — نسخ شجرة
    // عادي (المحتوى والروابط الرمزية كما في
    // `nv_copy_tree`)؛ والقيد مسجل.
    return nv_copy_tree(src, dst);
#else
    NvLinkMap *map = NULL;
    size_t map_len = 0, map_cap = 0;
    int rc = nv_copy_archive_inner(src, dst, &map, &map_len, &map_cap);
    for (size_t i = 0; i < map_len; i++) free(map[i].dst);
    free(map);
    return rc;
#endif
}

#ifdef __linux__
// انسخ ملفا متناثرا بـ`SEEK_DATA`/`SEEK_HOLE`
// (الميزة 201): الامتدادات الحقيقية فقط تنسخ،
// والثقوب تبقى ثقوبا. يرجع 0 إن وجد ثقبا واحدا
// على الأقل، و1 إن كان بلا ثقوب أصلا، و-2 إن كان
// نظام الملفات بلا دعم للمسح (فيتراجع المتصل
// إلى النسخ العادي)، و-1 عند خطأ حقيقي.
static int nv_copy_sparse(int sfd, const char *dst, off_t len) {
    int dfd = open(dst, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (dfd < 0) return -1;
    if (len == 0) {
        close(dfd);
        return 1;
    }
    unsigned char buf[65536];
    off_t pos = 0;
    int had_hole = 0;
    int ret = 0;
    while (1) {
        off_t data = lseek(sfd, pos, SEEK_DATA);
        if (data < 0) {
            if (errno == ENXIO) break;
            ret = -2;
            break;
        }
        if (data > pos) had_hole = 1;
        off_t hole = lseek(sfd, data, SEEK_HOLE);
        if (hole < 0) {
            ret = -2;
            break;
        }
        off_t off = data;
        while (off < hole) {
            size_t want = (size_t)((hole - off) < (off_t)sizeof(buf)
                ? (hole - off)
                : (off_t)sizeof(buf));
            ssize_t n = pread(sfd, buf, want, off);
            if (n < 0) {
                ret = -1;
                break;
            }
            if (n == 0) break;
            if (pwrite(dfd, buf, (size_t)n, off) != n) {
                ret = -1;
                break;
            }
            off += n;
        }
        if (ret != 0) break;
        pos = hole;
        if (pos >= len) break;
    }
    if (ret == 0) {
        if (ftruncate(dfd, len) != 0) ret = -1;
        else ret = (had_hole || pos < len) ? 0 : 1;
    }
    close(dfd);
    return ret;
}
#endif

// نسخ خفيف (الميزة 201): يرجع 0 مستنسخا، و1
// متناثرا، و2 عاديا، و-1 عند الفشل مع بقاء errno.
static int nv_copy_light(const char *src, const char *dst) {
#ifdef __linux__
    int sfd = open(src, O_RDONLY);
    if (sfd < 0) return -1;
    struct stat sst;
    if (fstat(sfd, &sst) != 0) {
        int e = errno;
        close(sfd);
        errno = e;
        return -1;
    }
    int dfd = open(dst, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (dfd < 0) {
        int e = errno;
        close(sfd);
        errno = e;
        return -1;
    }
    // الاستنساخ: `ioctl(FICLONE)` عملية O(1) بلا
    // نسخ بيانات حيث يدعمها نظام الملفات؛ والفشل
    // هنا تراجع لا خطأ.
    if (ioctl(dfd, FICLONE, sfd) == 0) {
        fchmod(dfd, sst.st_mode & 07777);
        close(sfd);
        close(dfd);
        return 0;
    }
    close(dfd);
    int rc = nv_copy_sparse(sfd, dst, (off_t)sst.st_size);
    close(sfd);
    if (rc == 0 || rc == 1) {
        chmod(dst, sst.st_mode & 07777);
        return rc == 0 ? 1 : 2;
    }
    if (rc == -2) {
        if (nv_copy_file(src, dst) != 0) return -1;
        return 2;
    }
    return -1;
#else
    if (nv_copy_file(src, dst) != 0) return -1;
    return 2;
#endif
}

// ---------- الدفعة 3 — التشغيل (اجتماع 57 §72) ----------
// وصف الأمر ومكان الأداة: طبقات البحث ومساعد البحث
// عن ملف أداة — مرآة command_names.rs وfiles.rs
// في Rust.

// ترميز نقطة شفرة إلى UTF-8 في out؛ يعيد عدد البايتات.
static int nv_utf8_encode(char *out, unsigned int cp) {
    if (cp < 0x80) { out[0] = (char)cp; return 1; }
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

// تطبيع كلمة كما يفعل normalize_cli_word في Rust:
// تجريد التشكيل (U+064B–U+0652 وU+0670) والتطويل
// (U+0640)، وتوحيد الهمزات والتاء المربوطة والألف
// المقصورة؛ وطي الفراغات (كلمة كلمة بفاصل واحد،
// بلا فراغ بادئ أو لاحق). يعيد سلسلة مخصصة
// يحررها المتصل.
static char *nv_normalize_word(const char *s) {
    size_t len = strlen(s);
    char *out = (char *)malloc(len + 1);
    size_t w = 0;
    int last_space = 1;
    const unsigned char *p = (const unsigned char *)s;
    while (*p) {
        if (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') {
            if (!last_space) {
                out[w++] = ' ';
                last_space = 1;
            }
            p++;
            continue;
        }
        unsigned int cp = 0;
        // فك UTF-8 الموجود (مساعدات التنقل، الدفعة 2).
        size_t adv = nv_utf8_decode(p, &cp);
        p += adv;
        if ((cp >= 0x064B && cp <= 0x0652) || cp == 0x0670 || cp == 0x0640) continue;
        if (cp == 0x0622 || cp == 0x0623 || cp == 0x0625) cp = 0x0627;
        else if (cp == 0x0624) cp = 0x0648;
        else if (cp == 0x0626) cp = 0x064A;
        else if (cp == 0x0629) cp = 0x0647;
        else if (cp == 0x0649) cp = 0x064A;
        char buf[4];
        int m = nv_utf8_encode(buf, cp);
        memcpy(out + w, buf, (size_t)m);
        w += (size_t)m;
        last_space = 0;
    }
    if (w > 0 && out[w - 1] == ' ') w--;
    out[w] = '\0';
    return out;
}

// أمر طرفي (بالصيغة المطبَّعة) — مرآة
// REPL_COMMANDS في command_names.rs.
static int nv_is_repl_command(const char *norm) {
    static const char *names[] = {
        "شغل", "استبدل", "تثبيت", "تن", "معلومات",
        "اعد-الاعدادات", "اعد-الضبط", "خروج", "عكس",
        ":مساعده", ":اعرض", ":امسح-المحفوظات", ":مسح",
        ":اسمح-بالشبكه", ":اسمح-بالخادم", ":تنزيلات", ":خروج",
        "ارجع للبيت", "كرر الاخير", "امسح الجلسه", NULL
    };
    for (int i = 0; names[i]; i++) {
        if (strcmp(norm, names[i]) == 0) return 1;
    }
    return 0;
}

// الدالة المدمجة التي تمرر إليها هذه العبارة
// الطرفية (بالصيغة المطبَّعة)، أو NULL.
static const char *nv_phrase_builtin(const char *norm) {
    static const char *pairs[][2] = {
        {"ادفع مجلد", "ادفع_مجلد"},
        {"اخرج مجلد", "أخرج_مجلد"},
        {"اعرض المكدس", "اعرض_المكدس"},
        {"امسح المكدس", "امسح_المكدس"},
        {"ابحث في ملف", "ابحث_في_ملف"},
        {"ابحث عن ملف", "ابحث_عن"},
        {"حول الي عدد", "عدد"},
        {"انتقل", "انتقل"},
        {"انسخ", "انسخ"},
        {"انقل", "انقل"},
        {"موجود", "موجود"},
        {NULL, NULL}
    };
    for (int i = 0; pairs[i][0]; i++) {
        if (strcmp(norm, pairs[i][0]) == 0) return pairs[i][1];
    }
    return NULL;
}

// اختصار طرفي معروف — مصدر الحقيقة الوحيد جدول
// shortcut_def في repl.rs (28 رمزا).
static int nv_is_shortcut_code(const char *norm) {
    static const char *codes[] = {
        "ات", "مح", "شم", "حم", "حف", "اف", "شف", "عب",
        "عن", "عس", "عك", "عش", "حج", "شر", "اك", "ام",
        "تم", "عم", "اس", "اص", "رع", "تا", "وق", "نو",
        "اج", "عع", "نب", "اب", NULL
    };
    for (int i = 0; codes[i]; i++) {
        if (strcmp(norm, codes[i]) == 0) return 1;
    }
    return 0;
}

// كلمة محجوزة (بالتهجئة المطبَّعة كما في
// keyword_spelling في lexer.rs).
static int nv_is_keyword_word(const char *norm) {
    static const char *words[] = {
        "اطبع", "اذا", "والا", "بينما", "لكل", "في",
        "داله", "داله_مجهوله", "ارجع", "و", "او", "ليس",
        "صحيح", "خطا", "لاشيء", "اكسر", "تابع", "مرر",
        "حاول", "ماعدا", "اخيرا", "فيه", "انشي", "ورث",
        "واجهه", "استورد", "صدر", "باسم", "من", "كرر",
        "مرات", "طالما", "اكبر", "اصغر", "يساوي", "لا", NULL
    };
    for (int i = 0; words[i]; i++) {
        if (strcmp(norm, words[i]) == 0) return 1;
    }
    return 0;
}

// اسم الملف الكامل لأداة (يلحق `.نيفا` عند غيابه)
// — سلسلة مخصصة يحررها المتصل.
static char *nv_tool_file_name(const char *name) {
    static const char *ext = ".نيفا";
    size_t nlen = strlen(name);
    size_t elen = strlen(ext);
    if (nlen >= elen && strcmp(name + nlen - elen, ext) == 0) {
        return nv_strdup(name);
    }
    char *out = (char *)malloc(nlen + elen + 1);
    memcpy(out, name, nlen);
    memcpy(out + nlen, ext, elen + 1);
    return out;
}

// ابحث عن ملف أداة في `$نيفا` ثم البيت — مرآة
// find_tool_file: المجلد وغير المقروء يُتخطيان
// (بت القراءة 0444). يعيد مسارا مخصصا يحرره
// المتصل، أو NULL؛ ويضبط *in_home (1 إن كان في
// البيت).
static char *nv_locate_tool(const char *file_name, int *in_home) {
    const char *bases[2];
    bases[0] = nv_program_dir;
    bases[1] = nv_home_dir;
    for (int i = 0; i < 2; i++) {
        if (!bases[i]) continue;
        size_t n = strlen(bases[i]) + 1 + strlen(file_name) + 1;
        char *cand = (char *)malloc(n);
        snprintf(cand, n, "%s/%s", bases[i], file_name);
        struct stat st;
        if (stat(cand, &st) == 0 && S_ISREG(st.st_mode)
            && (st.st_mode & 0444) != 0) {
            *in_home = (i == 1);
            return cand;
        }
        free(cand);
    }
    return NULL;
}

// طبقة الأداة في الوصف: مكانها أو «غير موجود».
// الاسم الحاوي مسارا لا يُبحث في القرص إطلاقا.
static const char *nv_describe_tool(const char *name) {
    if (strchr(name, '/') || strchr(name, '\\') || strstr(name, "..")) {
        return "غير موجود";
    }
    char *file_name = nv_tool_file_name(name);
    int in_home = 0;
    char *path = nv_locate_tool(file_name, &in_home);
    free(file_name);
    if (!path) return "غير موجود";
    free(path);
    return in_home ? "اداة في ~" : "اداة في $نيفا";
}

// صف أمرا باسمه بترتيب الطبقات — مرآة
// describe_command في Rust. يعيد سلسلة ثابتة.
static const char *nv_describe_command(const char *name) {
    char *norm = nv_normalize_word(name);
    NvCell *cell = NULL;
    if (nv_current_call_scope) {
        cell = nv_scope_lookup_cell(nv_current_call_scope, name);
    }
    const char *result;
    if (nv_is_repl_command(norm)) result = "امر طرفي";
    else if (!strchr(norm, ' ') && nv_is_shortcut_code(norm)) result = "اختصار";
    else if (!strchr(norm, ' ') && nv_is_keyword_word(norm)) result = "كلمة محجوزة";
    else if (cell && cell->val.tag == NV_FUNC) result = "دالة معرفة";
    else if (nv_is_builtin(name) || nv_phrase_builtin(norm) != NULL) result = "دالة مدمجة";
    else result = nv_describe_tool(name);
    free(norm);
    return result;
}

// البند 88 — مدير السياق: عضو بلا رفع — حقل أولا
// ثم طريقة مربوطة في الكائن، وتصدير في الوحدة؛
// و0 للغائب أو لغيرهما (مرآة `عند_عضو` في Rust).
static int nv_ctx_member(NvValue v, const char *name, NvValue *out) {
    if (v.tag == NV_INST) {
        NvInst *in = v.u.inst;
        int i = nv_dict_find(in->fields, name);
        if (i >= 0) {
            *out = in->fields->entries[i].val;
            return 1;
        }
        NvDict *methods = in->klass->methods;
        int m = methods ? nv_dict_find(methods, name) : -1;
        if (m >= 0) {
            *out = nv_bound_new(methods->entries[m].val, in);
            return 1;
        }
        return 0;
    }
    if (v.tag == NV_MODULE) {
        NvDict *ex = v.u.module->exports;
        int i = ex ? nv_dict_find(ex, name) : -1;
        if (i < 0) return 0;
        *out = ex->entries[i].val;
        return 1;
    }
    return 0;
}

// البند 88: هل القيمة قابلة للاستدعاء كدالة مدير —
// دالة أو مدمجة أو مربوطة؛ والفئة ليست قابلة.
static int nv_ctx_callable(NvValue v) {
    return v.tag == NV_FUNC || v.tag == NV_NATIVE || v.tag == NV_BOUND;
}

// البند 88: حقل نصي/عددي من قاموس الخطأ المحفوظ؛
// وNULL/الافتراضي عند الغياب أو النوع الخاطئ.
static const char *nv_ctx_dict_str(NvDict *d, const char *key) {
    int i = d ? nv_dict_find(d, key) : -1;
    if (i < 0) return NULL;
    NvValue v = d->entries[i].val;
    if (v.tag != NV_STR) return NULL;
    return v.u.str->data;
}

static double nv_ctx_dict_num(NvDict *d, const char *key, double def) {
    int i = d ? nv_dict_find(d, key) : -1;
    if (i < 0) return def;
    NvValue v = d->entries[i].val;
    if (v.tag != NV_NUM) return def;
    return v.u.num;
}

NvValue nv_call_builtin(const char *name, NvValue *args, int nargs, int line, int col) {
    // التسع النقية.
    if (strcmp(name, "طول") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue a = args[0];
        if (a.tag == NV_LIST) return nv_num((double)a.u.list->len);
        if (a.tag == NV_STR) return nv_num((double)a.u.str->chars);
        if (a.tag == NV_DICT) return nv_num((double)a.u.dict->len);
        if (a.tag == NV_TUPLE) return nv_num((double)a.u.tuple->len);
        if (a.tag == NV_SET) return nv_num((double)a.u.set->len);
        // ج6: طول البت عدد بتاته.
        if (a.tag == NV_BITS) return nv_num((double)a.u.bits->nbits);
        nv_error_fmt(NV_ERR_TYPE, line, col, "طول يتطلب قائمة أو نصا أو قاموسا، وليس %s", nv_type_name(a));
    }
    // الدفعة 3 من الـ579 — البند 138: `مترابطة(...)`.
    if (strcmp(name, "مترابطة") == 0) {
        NvValue out = nv_tuple_new();
        size_t _ro = nv_root_push_idx();
        *nv_root_slot(_ro) = out;
        for (int i = 0; i < nargs; i++) nv_list_push_raw(out.u.tuple, args[i]);
        nv_root_pop_n(1);
        return out;
    }
    // الدفعة 3 من الـ579 — البند 139: `تشكيلة(...)`
    // بلا تكرار (أول ظهور يفوز).
    if (strcmp(name, "تشكيلة") == 0) {
        NvValue out = nv_set_new();
        size_t _ro = nv_root_push_idx();
        *nv_root_slot(_ro) = out;
        for (int i = 0; i < nargs; i++) nv_set_push_dedup(out.u.set, args[i]);
        nv_root_pop_n(1);
        return out;
    }
    // ب6 — المولدات (الاجتماع 85 §100): الرفض الأصلي.
    // `ولد`/`ولد من`/التعبير التوليدي تُفكَّك في المحلل
    // إلى هذه الاستدعاءات الداخلية؛ والثنائي الأصلي
    // بلا آلة حالات ولا إطار معلق، فيرفضها بالرسالة
    // القانونية حرفيا (خروج 1 بلا معالج؛ و`ماعدا`
    // يلتقطها كأي خطأ وقت تشغيل، كما في باقي المدمجات).
    // لا تخفيض آلة-حالة هنا — عمل مستقبلي مسمى.
    if (strcmp(name, "__ولد") == 0 || strcmp(name, "__ولد_من") == 0
        || strcmp(name, "__مولد_تعبير") == 0) {
        nv_error(NV_ERR_RUNTIME, "المولدات غير مدعومة في الثنائي الأصلي — شغّل في المفسر أو آلة البايت كود", line, col);
    }
    // البند 88 — مدير السياق (الاجتماع 88 §103):
    // الاستدعاءات الداخلية المفككة في المحلل من `عند`
    // — مرآة call_builtin في Rust حرفيا.
    if (strcmp(name, "__عند_افحص") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue m;
        if (!nv_ctx_member(args[0], "دخول", &m) || !nv_ctx_callable(m)) {
            nv_error(NV_ERR_RUNTIME, "القيمة ليست مدير سياق — لا تحمل دالة دخول", line, col);
        }
        if (!nv_ctx_member(args[0], "خروج", &m) || !nv_ctx_callable(m)) {
            nv_error(NV_ERR_RUNTIME, "القيمة ليست مدير سياق — لا تحمل دالة خروج", line, col);
        }
        return nv_nil();
    }
    if (strcmp(name, "__عند_خطأ") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        NvError *e = nv_current_error();
        if (!e) return nv_nil();
        NvValue out = nv_dict_new();
        nv_dict_set_raw(out.u.dict, "نوع", nv_str_cstr(nv_err_kind_ar(e->kind)));
        nv_dict_set_raw(out.u.dict, "رسالة", nv_str_cstr(e->message ? e->message : ""));
        nv_dict_set_raw(out.u.dict, "سطر", nv_num((double)e->line));
        nv_dict_set_raw(out.u.dict, "عمود", nv_num((double)e->col));
        if (e->file_idx >= 0 && e->file_idx < nv_n_source_files && nv_source_files[e->file_idx]) {
            nv_dict_set_raw(out.u.dict, "ملف", nv_str_cstr(nv_source_files[e->file_idx]));
        } else {
            nv_dict_set_raw(out.u.dict, "ملف", nv_nil());
        }
        // الثنائي الأصلي بلا خطأ وحدة غائبة (لا 127).
        nv_dict_set_raw(out.u.dict, "وحدة_غائبة", nv_bool(0));
        return out;
    }
    if (strcmp(name, "__عند_أعد_رفع") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_DICT) return nv_nil();
        NvDict *d = args[0].u.dict;
        const char *kind_s = nv_ctx_dict_str(d, "نوع");
        const char *msg = nv_ctx_dict_str(d, "رسالة");
        double ln = nv_ctx_dict_num(d, "سطر", (double)line);
        double cl = nv_ctx_dict_num(d, "عمود", (double)col);
        const char *file = nv_ctx_dict_str(d, "ملف");
        NvErrKind kind = NV_ERR_RUNTIME;
        if (kind_s) {
            if (strcmp(kind_s, "لغوي") == 0) kind = NV_ERR_LEX;
            else if (strcmp(kind_s, "نحوي") == 0) kind = NV_ERR_PARSE;
            else if (strcmp(kind_s, "نوع") == 0) kind = NV_ERR_TYPE;
            else if (strcmp(kind_s, "إدخال/إخراج") == 0) kind = NV_ERR_IO;
            else if (strcmp(kind_s, "دلالي") == 0) kind = NV_ERR_SEMANTIC;
        }
        // استعد الخطأ نفسه (لا خطأ جديد بموضع الرفع):
        // النوع والرسالة والسطر والعمود والملف كما حُفظت.
        nv_clear_error();
        nv_last_error.kind = kind;
        nv_last_error.message = nv_strdup(msg ? msg : "");
        nv_last_error.line = (int)ln;
        nv_last_error.col = (int)cl;
        nv_last_error.file_idx = -1;
        if (file) {
            for (int i = 0; i < nv_n_source_files; i++) {
                if (nv_source_files[i] && strcmp(nv_source_files[i], file) == 0) {
                    nv_last_error.file_idx = i;
                    break;
                }
            }
        }
        nv_has_error = 1;
        nv_reraise_current();
        return nv_nil();
    }
    // ج6 — مدمجات الصيغ الحرفية المفككة في المحلل
    // (مرآة call_builtin في Rust حرفيا).
    if (strcmp(name, "عدد_كبير") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue a = args[0];
        if (a.tag == NV_STR) return nv_bigint_cstr(a.u.str->data, line, col);
        if (a.tag == NV_NUM) return nv_bigint_from_double(a.u.num, line, col);
        if (a.tag == NV_BIGINT) return a;
        nv_error_fmt(NV_ERR_TYPE, line, col, "عدد_كبير يتطلب نصا أو عددا، وليس %s", nv_type_name(a));
    }
    if (strcmp(name, "كسر") == 0 || strcmp(name, "كسر_حرفي") == 0) {
        if (nargs < 1 || nargs > 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 1 أو 2 معاملات لكن أعطيت %d",
                         name, nargs);
        }
        NvValue parts[2];
        for (int i = 0; i < nargs; i++) {
            NvValue v = args[i];
            if (v.tag == NV_NUM) {
                if (!isfinite(v.u.num) || v.u.num != trunc(v.u.num)) {
                    nv_error(NV_ERR_TYPE, "كسر يتطلب بسطا ومقاما صحيحين", line, col);
                }
                parts[i] = nv_bigint_from_double(v.u.num, line, col);
            } else if (v.tag == NV_BIGINT) {
                parts[i] = v;
            } else if (v.tag == NV_STR) {
                parts[i] = nv_bigint_cstr(v.u.str->data, line, col);
            } else {
                nv_error_fmt(NV_ERR_TYPE, line, col, "كسر يتطلب أعدادا صحيحة، وليس %s", nv_type_name(v));
            }
        }
        if (nargs == 1) parts[1] = nv_bigint_cstr("1", line, col);
        return nv_rational_new(parts[0], parts[1], line, col);
    }
    if (strcmp(name, "مركب") == 0 || strcmp(name, "مركب_حرفي") == 0) {
        if (nargs < 1 || nargs > 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 1 أو 2 معاملات لكن أعطيت %d",
                         name, nargs);
        }
        double vals[2];
        for (int i = 0; i < nargs; i++) {
            double re = 0.0, im = 0.0;
            if (!nv_value_to_complex(args[i], &re, &im)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "مركب يتطلب أعدادا، وليس %s", nv_type_name(args[i]));
            }
            if (im != 0.0) {
                nv_error(NV_ERR_TYPE, "مركب يتطلب جزأين حقيقيين", line, col);
            }
            vals[i] = re;
        }
        return nv_complex_new(vals[0], nargs == 2 ? vals[1] : 0.0);
    }
    // الدفعة 3 من الـ579 — البنود 141–143: الفهم
    // الضمني `افهم_قائمة/افهم_تشكيلة/افهم_فهرس
    // (دالة التعبير، دالة الشرط أو لاشيء، القابل)` —
    // مرآة call_builtin في Rust: لقطة القابل عبر
    // nv_for_snapshot، ثم استدعاء الدالتين بـnv_call
    // كما في `قوائم.اطو_ب`، والشرط غير المنطقي
    // والزوج غير الثنائي والمفتاح غير النصي أخطاء
    // عربية بالرسائل نفسها.
    if (strcmp(name, "افهم_قائمة") == 0 || strcmp(name, "افهم_تشكيلة") == 0
        || strcmp(name, "افهم_فهرس") == 0) {
        nv_arity(args, nargs, 3, name, line, col);
        int is_dict = strcmp(name, "افهم_فهرس") == 0;
        int is_set = strcmp(name, "افهم_تشكيلة") == 0;
        NvValue snap = nv_for_snapshot(args[2], line, col);
        size_t _rs = nv_root_push_idx();
        *nv_root_slot(_rs) = snap;
        NvValue out = is_dict ? nv_dict_new() : is_set ? nv_set_new() : nv_list_new();
        size_t _ro = nv_root_push_idx();
        *nv_root_slot(_ro) = out;
        NvList *items = snap.u.list;
        for (size_t i = 0; i < items->len; i++) {
            NvValue item = items->items[i];
            if (args[1].tag != NV_NIL) {
                NvValue argv[1];
                argv[0] = item;
                NvValue c = nv_call(args[1], argv, 1, line, col);
                if (c.tag != NV_BOOL) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "شرط الفهم يجب أن يكون منطقيا، وليس %s", nv_type_name(c));
                }
                if (!c.u.boolean) continue;
            }
            NvValue argv[1];
            argv[0] = item;
            NvValue v = nv_call(args[0], argv, 1, line, col);
            size_t _rv = nv_root_push_idx();
            *nv_root_slot(_rv) = v;
            if (is_dict) {
                if (v.tag != NV_LIST || v.u.list->len != 2) {
                    nv_error(NV_ERR_RUNTIME, "دالة الزوج في فهم الفهرس يجب أن ترجع قائمة من عنصرين", line, col);
                }
                NvValue k = v.u.list->items[0];
                if (k.tag != NV_STR) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "مفتاح القاموس يجب أن يكون نصا، وليس %s", nv_type_name(k));
                }
                nv_dict_set_raw(out.u.dict, k.u.str->data, v.u.list->items[1]);
            } else if (is_set) {
                nv_set_push_dedup(out.u.set, v);
            } else {
                nv_list_push_raw(out.u.list, v);
            }
            nv_root_pop_n(1);
            nv_safepoint();
        }
        nv_root_pop_n(2);
        return out;
    }
    if (strcmp(name, "مدى") == 0) {
        if (nargs < 1 || nargs > 3) {
            nv_error(NV_ERR_RUNTIME, "مدى تتوقع 1 إلى 3 معاملات", line, col);
        }
        long long ints[3];
        for (int i = 0; i < nargs; i++) {
            if (args[i].tag != NV_NUM || args[i].u.num != trunc(args[i].u.num)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "مدى يتطلب أعدادا صحيحة، وليس %s", nv_type_name(args[i]));
            }
            ints[i] = nv_num_to_i64(args[i].u.num);
        }
        long long start = 0, end = ints[0], step = 1;
        if (nargs == 2) { start = ints[0]; end = ints[1]; }
        if (nargs == 3) { start = ints[0]; end = ints[1]; step = ints[2]; }
        if (step == 0) nv_error(NV_ERR_RUNTIME, "خطوة مدى لا يمكن أن تكون صفرا", line, col);
        NvValue out = nv_list_new();
        size_t _ro = nv_root_push_idx();
        *nv_root_slot(_ro) = out;
        long long cur = start;
        while ((step > 0 && cur < end) || (step < 0 && cur > end)) {
            nv_list_push_raw(out.u.list, nv_num((double)cur));
            cur += step;
            if (out.u.list->len > NV_MAX_LOOP_ITERATIONS) {
                nv_error(NV_ERR_RUNTIME, "نطاق مدى كبير جدا", line, col);
            }
            nv_safepoint();
        }
        nv_root_pop_n(1);
        return out;
    }
    if (strcmp(name, "مجموع") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_LIST) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مجموع يتطلب قائمة، وليس %s", nv_type_name(args[0]));
        }
        double sum = 0;
        for (size_t i = 0; i < args[0].u.list->len; i++) {
            NvValue e = args[0].u.list->items[i];
            if (!nv_is_num_like(e)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "مجموع يتطلب أعدادا، وليس %s", nv_type_name(e));
            }
            sum += nv_num_of(e);
        }
        return nv_num(sum);
    }
    if (strcmp(name, "أدنى") == 0 || strcmp(name, "أقصى") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_LIST) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "%s يتطلب قائمة، وليس %s", name, nv_type_name(args[0]));
        }
        NvList *l = args[0].u.list;
        if (l->len == 0) nv_error(NV_ERR_RUNTIME, "القائمة فارغة", line, col);
        double best = 0;
        int first = 1;
        for (size_t i = 0; i < l->len; i++) {
            NvValue e = l->items[i];
            if (!nv_is_num_like(e)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "%s يتطلب أعدادا، وليس %s", name, nv_type_name(e));
            }
            if (first) { best = nv_num_of(e); first = 0; }
            else if (strcmp(name, "أدنى") == 0) { if (nv_num_of(e) < best) best = nv_num_of(e); }
            else { if (nv_num_of(e) > best) best = nv_num_of(e); }
        }
        return nv_num(best);
    }
    if (strcmp(name, "مطلق") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        // ج6: مطلق الكبير/الكسر دقيق، والمركب مقداره.
        if (args[0].tag == NV_BIGINT) {
            char *sg = nv_bigint_signed_str(args[0].u.bigint);
            char *mag = sg[0] == '-' ? nv_strdup(sg + 1) : nv_strdup(sg);
            free(sg);
            NvValue out = nv_bigint_cstr(mag, line, col);
            free(mag);
            return out;
        }
        if (args[0].tag == NV_RATIONAL) {
            char *sn = nv_bigint_signed_str(args[0].u.rational->num);
            char *mag = sn[0] == '-' ? nv_strdup(sn + 1) : nv_strdup(sn);
            free(sn);
            NvValue out = nv_rational_new(nv_bigint_cstr(mag, line, col),
                                          nv_bigint_cstr(args[0].u.rational->den->digits, line, col),
                                          line, col);
            free(mag);
            return out;
        }
        if (args[0].tag == NV_COMPLEX) {
            double re = args[0].u.complex.re, im = args[0].u.complex.im;
            return nv_num(sqrt(re * re + im * im));
        }
        if (!nv_is_num_like(args[0])) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مطلق يتطلب عددا، وليس %s", nv_type_name(args[0]));
        }
        return nv_num(fabs(nv_num_of(args[0])));
    }
    if (strcmp(name, "عدد") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue a = args[0];
        if (a.tag == NV_NUM) return a;
        // ج6: الكبير/الكسر إلى f64، والمركب الحقيقي
        // الخالص إلى جزئه الحقيقي، والبت إلى قيمته.
        if (a.tag == NV_BIGINT) return nv_num(nv_bigint_to_double(a));
        if (a.tag == NV_RATIONAL) return nv_num(nv_rational_to_double(a));
        if (a.tag == NV_COMPLEX) {
            if (a.u.complex.im == 0.0) return nv_num(a.u.complex.re);
            nv_error(NV_ERR_TYPE, "لا يمكن تحويل عدد مركب بتخيلي غير صفري إلى عدد", line, col);
        }
        if (a.tag == NV_BITS) return nv_num(nv_bits_to_double(a));
        if (a.tag == NV_HANDLE) return nv_num((double)a.u.handle->id);
        if (a.tag == NV_STR) {
            double d = 0;
            if (nv_parse_number(a.u.str->data, &d)) return nv_num(d);
            nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن تحويل النص إلى عدد: %s", a.u.str->data);
        }
        if (a.tag == NV_BOOL) return nv_num(a.u.boolean ? 1.0 : 0.0);
        nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن تحويل نوع %s إلى عدد", nv_type_name(a));
    }
    if (strcmp(name, "نص") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        char *s = nv_fmt_value(args[0]);
        NvValue v = nv_str_take(s, strlen(s));
        return v;
    }
    if (strcmp(name, "نوع") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        return nv_str_cstr(nv_type_name(args[0]));
    }
    if (strcmp(name, "ادخل") == 0) {
        return nv_input(args, nargs, line, col);
    }
    // الملفات المسطحة.
    if (strcmp(name, "اقرأ_ملف") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "اقرأ_ملف يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        char *disp = nv_display_str(args[0].u.str->data);
        size_t n = 0;
        char *content = nv_read_file_capped(resolved, disp, line, col, &n);
        NvValue v = nv_str_take(content, n);
        free(resolved);
        free(disp);
        return v;
    }
    if (strcmp(name, "اكتب_ملف") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "اكتب_ملف يتطلب مسارا ونصا", line, col);
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        unsigned long long existing = 0;
        if (stat(resolved, &st) == 0) existing = (unsigned long long)st.st_size;
        size_t clen = args[1].u.str->len;
        unsigned long long extra = clen > existing ? (unsigned long long)(clen - existing) : 0;
        nv_check_quota((size_t)extra, line, col);
        FILE *f = fopen(resolved, "wb");
        if (!f) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر كتابة الملف %s: %s", disp, osb);
        }
        if (clen) fwrite(args[1].u.str->data, 1, clen, f);
        fclose(f);
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "اعرض_الملفات") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        DIR *d = opendir(nv_cwd);
        if (!d) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر عرض المجلد: %s", osb);
        }
        char **names = NULL;
        size_t nnames = 0, capn = 0, total = 0;
        struct dirent *e;
        while ((e = readdir(d)) != NULL) {
            if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
            total++;
            if (nnames < 1000) {
                if (nnames == capn) {
                    capn = capn ? capn * 2 : 16;
                    names = (char **)realloc(names, capn * sizeof(char *));
                }
                // مجلد؟ (lstat بلا تتبع).
                size_t n = strlen(nv_cwd) + 1 + strlen(e->d_name) + 1;
                char *child = (char *)malloc(n);
                snprintf(child, n, "%s/%s", nv_cwd, e->d_name);
                struct stat st;
                int is_dir = (nv_lstat(child, &st) == 0 && S_ISDIR(st.st_mode));
                free(child);
                size_t ln = strlen(e->d_name) + 2;
                char *entry = (char *)malloc(ln);
                if (is_dir) snprintf(entry, ln, "%s/", e->d_name);
                else snprintf(entry, ln, "%s", e->d_name);
                names[nnames++] = entry;
            }
        }
        closedir(d);
        qsort(names, nnames, sizeof(char *), nv_cmp_str);
        NvValue out = nv_list_new();
        for (size_t i = 0; i < nnames; i++) {
            nv_list_push_raw(out.u.list, nv_str_take(names[i], strlen(names[i])));
        }
        free(names);
        if (total > 1000) {
            char *more = nv_fmt_number((double)(total - 1000));
            size_t mn = strlen("…و ") + strlen(more) + strlen(" أخرى") + 1;
            char *marker = (char *)malloc(mn);
            snprintf(marker, mn, "…و %s أخرى", more);
            nv_list_push_raw(out.u.list, nv_str_take(marker, strlen(marker)));
            free(more);
        }
        return out;
    }
    if (strcmp(name, "المجلد_الحالي") == 0) {
        // اجتماع 56 §71 (الميزتان 9 و12): 0 أو 1
        // معاملات — بلا معامل حسب الوضع العام للجلسة،
        // و«منطقي»/«فيزيائي» صريحا.
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        int physical = nv_physical_mode;
        if (nargs == 1) {
            if (args[0].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "المجلد_الحالي يتطلب علما نصيا، وليس %s", nv_type_name(args[0]));
            }
            const char *f = args[0].u.str->data;
            if (strcmp(f, "منطقي") == 0) physical = 0;
            else if (strcmp(f, "فيزيائي") == 0) physical = 1;
            else nv_error_fmt(NV_ERR_RUNTIME, line, col, "علم المجلد_الحالي غير معروف: %s — العلم الصالح: منطقي أو فيزيائي", f);
        }
        char *disp;
        if (physical) {
            char *phys = nv_physical_cwd();
            disp = nv_display_path(phys);
            free(phys);
        } else {
            disp = nv_display_path(nv_cwd);
        }
        NvValue v = nv_str_take(disp, strlen(disp));
        return v;
    }
    if (strcmp(name, "بيئة") == 0) {
        // اجتماع 45 §59 (39–41): قراءة فقط، الثمانية
        // فقط — مرآة ذراع المحرك نفسه.
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "بيئة تتوقع نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *n = args[0].u.str->data;
        // التحقق على مستوى البايت: الحرف العربي بايتان
        // أو أكثر (≥ 0x80) فيمر؛ والحرف اللاتيني أو
        // المسافة أو الترقيم ASCII مرفوض؛ والرقم الأول
        // مرفوض — غربي (0-9) أو عربي مشرقي (٠-٩) أو
        // ممتد (۰-۹). يطابق قاعدة Rust للمدخلات المعتادة.
        // أول نقطة شفرة: فك بايتي UTF-8 الأولين.
        unsigned int first_cp = (unsigned char)n[0];
        if ((first_cp & 0xE0) == 0xC0 && n[1]) {
            first_cp = ((first_cp & 0x1F) << 6) | ((unsigned char)n[1] & 0x3F);
        } else if ((first_cp & 0xF0) == 0xE0 && n[1] && n[2]) {
            first_cp = ((first_cp & 0x0F) << 12) | (((unsigned char)n[1] & 0x3F) << 6) | ((unsigned char)n[2] & 0x3F);
        }
        if (n[0] == '\0' || (first_cp >= '0' && first_cp <= '9')
            || (first_cp >= 0x660 && first_cp <= 0x669)
            || (first_cp >= 0x6F0 && first_cp <= 0x6F9)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "اسم متغير البيئة غير صالح: %s", n);
        }
        for (const unsigned char *p = (const unsigned char *)n; *p; p++) {
            unsigned char c = *p;
            if (c < 0x80 && !(c == '_' || (c >= '0' && c <= '9'))) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "اسم متغير البيئة غير صالح: %s", n);
            }
        }
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // الثابت والديناميكي بالمساعد المشترك.
        return nv_env_read(n, line, col);
    }
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): التجميد
    // والحذف والفحص والمرجع وعرض البيئة ومفاتيح
    // القاموس — مرآة أذرع المفسر نفسها.
    if (strcmp(name, "جمد") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "جمد يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *nm = args[0].u.str->data;
        const char *resolved = nv_resolve_name(nm);
        if (!nv_current_call_scope || !nv_scope_lookup_cell(nv_current_call_scope, resolved)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", nm);
        }
        if (!nv_frozen_contains(resolved)) {
            if (nv_frozen_len == nv_frozen_cap) {
                nv_frozen_cap = nv_frozen_cap ? nv_frozen_cap * 2 : 8;
                nv_frozen = (char **)realloc(nv_frozen, nv_frozen_cap * sizeof(char *));
            }
            nv_frozen[nv_frozen_len++] = nv_strdup(resolved);
        }
        return nv_nil();
    }
    if (strcmp(name, "احذف_متغير") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف_متغير يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *nm = args[0].u.str->data;
        for (size_t i = 0; i < nv_nameref_len; i++) {
            if (strcmp(nv_nameref_from[i], nm) == 0) {
                free(nv_nameref_from[i]);
                free(nv_nameref_to[i]);
                for (size_t j = i + 1; j < nv_nameref_len; j++) {
                    nv_nameref_from[j - 1] = nv_nameref_from[j];
                    nv_nameref_to[j - 1] = nv_nameref_to[j];
                }
                nv_nameref_len--;
                return nv_nil();
            }
        }
        const char *resolved = nv_resolve_name(nm);
        if (nv_frozen_contains(resolved)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير مجمد: %s", nm);
        }
        if (nv_current_call_scope) nv_scope_unbind(nv_current_call_scope, resolved);
        return nv_nil();
    }
    if (strcmp(name, "افحص_متغير") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "افحص_متغير يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *nm = args[0].u.str->data;
        for (size_t i = 0; i < nv_nameref_len; i++) {
            if (strcmp(nv_nameref_from[i], nm) == 0) {
                size_t n = strlen(nm) + strlen(nv_nameref_to[i]) + 32;
                char *buf = (char *)malloc(n);
                snprintf(buf, n, "متغير %s: مرجع إلى %s", nm, nv_nameref_to[i]);
                return nv_str_take(buf, strlen(buf));
            }
        }
        const char *resolved = nv_resolve_name(nm);
        NvCell *cell = nv_current_call_scope
            ? nv_scope_lookup_cell(nv_current_call_scope, resolved)
            : NULL;
        if (!cell) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", nm);
        }
        char *val = nv_fmt_value(cell->val);
        const char *tn = nv_type_name(cell->val);
        char *shown;
        if (cell->val.tag == NV_STR) {
            size_t n = strlen(val) + 3;
            shown = (char *)malloc(n);
            snprintf(shown, n, "\"%s\"", val);
            free(val);
        } else {
            shown = val;
        }
        int frozen = nv_frozen_contains(resolved);
        size_t n = strlen(nm) + strlen(tn) + strlen(shown) + 32;
        char *buf = (char *)malloc(n);
        snprintf(buf, n, "متغير %s: %s = %s%s", nm, tn, shown, frozen ? " مجمد" : "");
        free(shown);
        return nv_str_take(buf, strlen(buf));
    }
    if (strcmp(name, "مرجع") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مرجع يتطلب نصين");
        }
        const char *nw = args[0].u.str->data;
        const char *tg = args[1].u.str->data;
        if (!nv_valid_var_name(nw) || !nv_valid_var_name(tg)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "اسم مرجع غير صالح: %s أو %s", nw, tg);
        }
        if (!nv_current_call_scope) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", tg);
        }
        if (nv_scope_lookup_cell(nv_current_call_scope, nw)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الاسم مستخدم: %s", nw);
        }
        for (size_t i = 0; i < nv_nameref_len; i++) {
            if (strcmp(nv_nameref_from[i], nw) == 0) {
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "الاسم مستخدم: %s", nw);
            }
        }
        if (!nv_scope_lookup_cell(nv_current_call_scope, tg)) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", tg);
        }
        const char *cur = tg;
        for (int i = 0; i < 32; i++) {
            if (strcmp(cur, nw) == 0) {
                nv_error(NV_ERR_RUNTIME, "مرجع دائري", line, col);
            }
            const char *next = NULL;
            for (size_t j = 0; j < nv_nameref_len; j++) {
                if (strcmp(nv_nameref_from[j], cur) == 0) { next = nv_nameref_to[j]; break; }
            }
            if (!next) break;
            cur = next;
        }
        if (nv_nameref_len == nv_nameref_cap) {
            nv_nameref_cap = nv_nameref_cap ? nv_nameref_cap * 2 : 8;
            nv_nameref_from = (char **)realloc(nv_nameref_from, nv_nameref_cap * sizeof(char *));
            nv_nameref_to = (char **)realloc(nv_nameref_to, nv_nameref_cap * sizeof(char *));
        }
        nv_nameref_from[nv_nameref_len] = nv_strdup(nw);
        nv_nameref_to[nv_nameref_len] = nv_strdup(tg);
        nv_nameref_len++;
        return nv_nil();
    }
    if (strcmp(name, "اعرض_البيئة") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        static const char *names[] = {
            "البيت", "مجلد_البرنامج", "المسار", "إصدار_نيفا",
            "الطرفية", "اللغة", "المجلد_المؤقت", "الألوان",
            "المجلد_الحالي", "المستخدم", "اسم_المضيف",
            "عشوائي", "ثواني", "رقم_السطر",
        };
        NvValue list = nv_list_new();
        for (int i = 0; i < 14; i++) {
            const char *nm = names[i];
            NvValue v;
#if !defined(__linux__) && !defined(__ANDROID__)
            if (strcmp(nm, "المستخدم") == 0 || strcmp(nm, "اسم_المضيف") == 0) {
                v = nv_str_cstr("غير مدعوم");
            } else {
                v = nv_env_read(nm, line, col);
            }
#else
            v = nv_env_read(nm, line, col);
#endif
            char *vs = nv_fmt_value(v);
            size_t n = strlen(nm) + strlen(vs) + 2;
            char *entry = (char *)malloc(n);
            snprintf(entry, n, "%s=%s", nm, vs);
            free(vs);
            nv_list_push_raw(list.u.list, nv_str_take(entry, strlen(entry)));
        }
        return list;
    }
    if (strcmp(name, "مفاتيح") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_DICT) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مفاتيح يتطلب قاموسا، وليس %s", nv_type_name(args[0]));
        }
        NvValue list = nv_list_new();
        for (size_t i = 0; i < args[0].u.dict->len; i++) {
            nv_list_push_raw(list.u.list, nv_str_cstr(args[0].u.dict->entries[i].key));
        }
        return list;
    }
    // الدفعة 1 — رموز الخروج (اجتماع 54 §69)، مرآة
    // أذرع المفسر والآلة الافتراضية نفسها.
    if (strcmp(name, "رمز_الخروج") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        return nv_num((double)nv_last_exit_code);
    }
    if (strcmp(name, "اسم_البرنامج") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        const char *n = nv_program_name ? nv_program_name : "نيفا";
        return nv_str_cstr(n);
    }
    if (strcmp(name, "خروج") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        int code;
        if (nargs == 0) {
            // بلا معامل: رمز آخر أمر، كـ exit في باش.
            code = nv_last_exit_code;
        } else {
            // عدد صحيح 0–255، وإلا خطأ وقت تشغيل —
            // بلا التفاف كـباش.
            if (args[0].tag != NV_NUM || args[0].u.num != trunc(args[0].u.num)
                || args[0].u.num < 0.0 || args[0].u.num > 255.0) {
                nv_error(NV_ERR_RUNTIME, "رمز الخروج يجب أن يكون عددا صحيحا بين ٠ و٢٥٥", line, col);
            }
            code = (int)args[0].u.num;
        }
        nv_exit_code = code;
        nv_exit_requested = 1;
        // القفز يغادر — لا عودة هنا أبدا.
        nv_exit_jump();
        return nv_nil();
    }
    // الدفعة 3 — التشغيل (اجتماع 57 §72): وصف
    // الأمر ومكان الأداة والاستبدال — مرآة أذرع
    // المفسر والآلة الافتراضية نفسها.
    if (strcmp(name, "نوع_الامر") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "نوع_الامر يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        return nv_str_cstr(nv_describe_command(args[0].u.str->data));
    }
    if (strcmp(name, "مكان_الاداة") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مكان_الاداة يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *tool = args[0].u.str->data;
        if (strchr(tool, '/') || strchr(tool, '\\') || strstr(tool, "..")) {
            nv_error(NV_ERR_RUNTIME, "مكان_الاداة يقبل اسم ملف بلا مسار", line, col);
        }
        char *file_name = nv_tool_file_name(tool);
        int in_home = 0;
        char *path = nv_locate_tool(file_name, &in_home);
        free(file_name);
        if (!path) return nv_nil();
        NvValue r = nv_str_cstr(path);
        free(path);
        return r;
    }
    if (strcmp(name, "استبدل_الجلسة") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "استبدل_الجلسة يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        const char *tool = args[0].u.str->data;
        if (strchr(tool, '/') || strchr(tool, '\\') || strstr(tool, "..")) {
            nv_error(NV_ERR_RUNTIME, "استبدل_الجلسة يقبل اسم ملف بلا مسار", line, col);
        }
        // لا مفسر داخل الثنائي الأصلي يشغّل ملف
        // `.نيفا` آخر — خطأ صريح لا ادعاء نجاح.
        nv_error(NV_ERR_RUNTIME, "استبدل_الجلسة غير مدعوم في البرنامج الأصلي", line, col);
        return nv_nil();
    }
    if (strcmp(name, "انتقل") == 0) {
        /* اجتماع 56 §71 — الدفعة 2 (التنقل): 0 أو 2
           معاملات — بلا معاملات → الأب `..` (أمر
           المالك المباشر 2026-11-09)؛ و`-` اسم عادي
           غير موجود؛ والمعامل الثاني علم نصي اختياري
           («منطقي»/«فيزيائي»/«منطقي_متحقق»/
           «فيزيائي_متحقق»)؛ والصامت في كل الصور. */
        if (nargs > 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 2 معاملات لكن أعطيت %d", name, nargs);
        }
        const char *dir = (nargs == 0) ? ".." : NULL;
        if (nargs >= 1) {
            if (args[0].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "انتقل يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
            }
            dir = args[0].u.str->data;
        }
        int flag = -1;
        if (nargs == 2) {
            if (args[1].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "علم انتقل يتطلب نصا، وليس %s", nv_type_name(args[1]));
            }
            const char *f = args[1].u.str->data;
            if (strcmp(f, "منطقي") == 0 || strcmp(f, "منطقي_متحقق") == 0) flag = 0;
            else if (strcmp(f, "فيزيائي") == 0 || strcmp(f, "فيزيائي_متحقق") == 0) flag = 1;
            else nv_error_fmt(NV_ERR_RUNTIME, line, col, "علم انتقل غير معروف: %s — العلم الصالح: منطقي أو فيزيائي أو منطقي_متحقق أو فيزيائي_متحقق", f);
        }
        nv_change_dir(dir, flag, line, col);
        return nv_nil();
    }
    /* اجتماع 56 §71 — الدفعة 2 (التنقل): مكدس
       المجلدات (الميزات 19–22 و26) — تمر عبر
       `انتقل` نفسها (nv_change_dir) فيرث قواعدها. */
    if (strcmp(name, "ادفع_مجلد") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        if (nargs == 0) {
            if (nv_dirstack_len == 0) nv_error(NV_ERR_RUNTIME, "المكدس فارغ", line, col);
            char *top = nv_dirstack[0];
            nv_dirstack[0] = nv_strdup(nv_cwd);
            free(nv_cwd);
            nv_cwd = top;
            nv_after_stack_move();
            return nv_nil();
        }
        if (args[0].tag == NV_STR) {
            const char *s = args[0].u.str->data;
            long idx;
            size_t pos;
            if (nv_parse_arabic_int(s, &idx) && nv_stack_index(idx, nv_dirstack_len, &pos)) {
                nv_dirstack_rotate(pos);
                nv_after_stack_move();
                return nv_nil();
            }
            char *old = nv_strdup(nv_cwd);
            nv_change_dir(s, -1, line, col);
            nv_dirstack_push_front(old);
            return nv_nil();
        }
        if (args[0].tag == NV_NUM) {
            long idx;
            if (!nv_stack_index_value(&args[0], &idx)) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "ادفع_مجلد يتطلب مسارا نصيا أو رقما، وليس %s", nv_type_name(args[0]));
            }
            size_t pos;
            if (!nv_stack_index(idx, nv_dirstack_len, &pos)) {
                char numbuf[64];
                nv_fmt_arabic_long(idx, numbuf, sizeof(numbuf));
                nv_error_fmt(NV_ERR_RUNTIME, line, col, "عنصر المكدس غير موجود: %s", numbuf);
            }
            nv_dirstack_rotate(pos);
            nv_after_stack_move();
            return nv_nil();
        }
        nv_error_fmt(NV_ERR_TYPE, line, col, "ادفع_مجلد يتطلب مسارا نصيا أو رقما، وليس %s", nv_type_name(args[0]));
    }
    if (strcmp(name, "أخرج_مجلد") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        if (nargs == 0) {
            if (nv_dirstack_len == 0) nv_error(NV_ERR_RUNTIME, "المكدس فارغ", line, col);
            const char *target = nv_dirstack[0];
            struct stat st;
            if (stat(target, &st) != 0) {
                char *disp = nv_display_path(target);
                nv_error_fmt(NV_ERR_IO, line, col, "المجلد غير موجود: %s", disp);
            }
            if (!S_ISDIR(st.st_mode)) {
                char *disp = nv_display_path(target);
                nv_error_fmt(NV_ERR_IO, line, col, "ليس مجلدا: %s", disp);
            }
            char *norm = nv_normalize_lexical(target);
            nv_dirstack_remove(0);
            free(nv_cwd);
            nv_cwd = norm;
            nv_after_stack_move();
            return nv_nil();
        }
        long idx;
        if (!nv_stack_index_value(&args[0], &idx)) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "أخرج_مجلد يتطلب رقما، وليس %s", nv_type_name(args[0]));
        }
        size_t pos;
        if (!nv_stack_index(idx, nv_dirstack_len, &pos)) {
            char numbuf[64];
            nv_fmt_arabic_long(idx, numbuf, sizeof(numbuf));
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "عنصر المكدس غير موجود: %s", numbuf);
        }
        nv_dirstack_remove(pos);
        return nv_nil();
    }
    if (strcmp(name, "اعرض_المكدس") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        NvValue out = nv_list_new();
        char *disp = nv_display_path(nv_cwd);
        nv_list_push_raw(out.u.list, nv_str_take(disp, strlen(disp)));
        for (size_t i = 0; i < nv_dirstack_len; i++) {
            char *d = nv_display_path(nv_dirstack[i]);
            nv_list_push_raw(out.u.list, nv_str_take(d, strlen(d)));
        }
        return out;
    }
    if (strcmp(name, "امسح_المكدس") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        nv_dirstack_clear();
        return nv_nil();
    }
    if (strcmp(name, "مسار_بحث_المجلدات") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        if (nargs == 1) {
            if (args[0].tag != NV_LIST) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "مسار_بحث_المجلدات يتطلب قائمة نصوص، وليس %s", nv_type_name(args[0]));
            }
            NvList *l = args[0].u.list;
            for (size_t i = 0; i < l->len; i++) {
                if (l->items[i].tag != NV_STR) {
                    nv_error_fmt(NV_ERR_TYPE, line, col, "مسار_بحث_المجلدات يتطلب قائمة نصوص، وليس عنصرا %s", nv_type_name(l->items[i]));
                }
            }
            for (size_t i = 0; i < nv_cdpath_len; i++) free(nv_cdpath[i]);
            nv_cdpath_len = 0;
            for (size_t i = 0; i < l->len; i++) {
                if (nv_cdpath_len == nv_cdpath_cap) {
                    nv_cdpath_cap = nv_cdpath_cap ? nv_cdpath_cap * 2 : 8;
                    nv_cdpath = (char **)realloc(nv_cdpath, nv_cdpath_cap * sizeof(char *));
                }
                nv_cdpath[nv_cdpath_len++] = nv_strdup(l->items[i].u.str->data);
            }
        }
        NvValue out = nv_list_new();
        for (size_t i = 0; i < nv_cdpath_len; i++) {
            nv_list_push_raw(out.u.list, nv_str_cstr(nv_cdpath[i]));
        }
        return out;
    }
    if (strcmp(name, "الوضع_الفيزيائي") == 0) {
        if (nargs > 1) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "الدالة المدمجة %s تتوقع 0 أو 1 معاملات لكن أعطيت %d", name, nargs);
        }
        if (nargs == 1) {
            if (args[0].tag != NV_BOOL) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "الوضع_الفيزيائي يتطلب قيمة منطقية، وليس %s", nv_type_name(args[0]));
            }
            nv_physical_mode = args[0].u.boolean ? 1 : 0;
        }
        return nv_bool(nv_physical_mode);
    }
    if (strcmp(name, "أنشئ_مجلد") == 0) {
        if (nargs < 1 || nargs > 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "أنشئ_مجلد تتوقع 1 أو 2 معاملات لكن أعطيت %d", nargs);
        }
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "أنشئ_مجلد يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        // الاجتماع 53 §67 (الميزة 210): سلوك `mkdir -p`
        // دائما — ينشئ كل الآباء الناقصين، وينجح صامتا
        // على الموجود.
        if (args[0].u.str->data[0] == '\0') {
            nv_error_fmt(NV_ERR_IO, line, col, "أنشئ_مجلد يتطلب مسارا غير فارغ");
        }
        // الاجتماع 59 §74 (الميزة 211): معامل وضع
        // اختياري + إرجاع قائمة ما أُنشئ فعلا.
        unsigned mode = 0;
        int has_mode = 0;
        if (nargs == 2) {
            if (args[1].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "أنشئ_مجلد يتطلب وضعا نصيا، وليس %s", nv_type_name(args[1]));
            }
            if (nv_parse_octal(args[1].u.str->data, &mode) != 0) {
                nv_error_fmt(NV_ERR_IO, line, col, "الوضع يجب أن يكون ثمانيا مثل 755");
            }
            has_mode = 1;
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        size_t nmiss = 0;
        char **miss = nv_missing_components(resolved, &nmiss);
        if (nv_mkdir_p(resolved) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء المجلد %s: %s", disp, osb);
        }
        // الوضع على الأخير وحده، وإن كان منشأ حديثا
        // فقط (الموجود لا يمس — كما في `mkdir -m -p`).
        if (has_mode && nmiss > 0 && strcmp(miss[nmiss - 1], resolved) == 0) {
            if (chmod(resolved, (int)mode) != 0) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر ضبط وضع المجلد %s: %s", disp, osb);
            }
        }
        NvValue out = nv_list_new();
        for (size_t i = 0; i < nmiss; i++) {
            char *d = nv_created_display(miss[i]);
            nv_list_push_raw(out.u.list, nv_str_take(d, strlen(d)));
            free(miss[i]);
        }
        free(miss);
        free(resolved);
        return out;
    }
    if (strcmp(name, "احذف") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        // الاجتماع 59 §74 (الميزة 208): حارس الجذر
        // قبل أي حل ساندبوكس.
        const char *root_msg = nv_root_refusal_text(args[0].u.str->data);
        if (root_msg) {
            nv_error_fmt(NV_ERR_IO, line, col, "%s", root_msg);
        }
        // فحص قانوني قبل الحل: الرابط الرمزي إلى
        // الجذر يرفضه الساندبوكس برسالة عامة،
        // والقرار يطلب رسالة الجذر.
        if (nv_delete_target_is_root(args[0].u.str->data)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        if (nv_is_filesystem_root(resolved)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        struct stat st;
        if (stat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المسار غير موجود: %s", disp);
        }
        int is_dir = S_ISDIR(st.st_mode);
        int rc = is_dir ? rmdir(resolved) : unlink(resolved);
        if (rc != 0) {
            int e = errno;
            char *disp = nv_display_str(args[0].u.str->data);
            if (is_dir && e == ENOTEMPTY) {
                nv_error_fmt(NV_ERR_IO, line, col, "المجلد غير فارغ: %s — استخدم احذف_تكراري", disp);
            }
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر حذف %s: %s", disp, osb);
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "احذف_مجلد") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف_مجلد يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        const char *root_msg = nv_root_refusal_text(args[0].u.str->data);
        if (root_msg) {
            nv_error_fmt(NV_ERR_IO, line, col, "%s", root_msg);
        }
        // فحص قانوني قبل الحل: الرابط الرمزي إلى
        // الجذر يرفضه الساندبوكس برسالة عامة،
        // والقرار يطلب رسالة الجذر.
        if (nv_delete_target_is_root(args[0].u.str->data)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        if (nv_is_filesystem_root(resolved)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        struct stat st;
        if (stat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المسار غير موجود: %s", disp);
        }
        if (!S_ISDIR(st.st_mode)) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "ليس مجلدا: %s", disp);
        }
        if (rmdir(resolved) != 0) {
            int e = errno;
            char *disp = nv_display_str(args[0].u.str->data);
            if (e == ENOTEMPTY) {
                nv_error_fmt(NV_ERR_IO, line, col, "المجلد غير فارغ: %s — استخدم احذف_تكراري", disp);
            }
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر حذف المجلد %s: %s", disp, osb);
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "احذف_ملف") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف_ملف يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        const char *root_msg = nv_root_refusal_text(args[0].u.str->data);
        if (root_msg) {
            nv_error_fmt(NV_ERR_IO, line, col, "%s", root_msg);
        }
        // فحص قانوني قبل الحل: الرابط الرمزي إلى
        // الجذر يرفضه الساندبوكس برسالة عامة،
        // والقرار يطلب رسالة الجذر.
        if (nv_delete_target_is_root(args[0].u.str->data)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        if (nv_is_filesystem_root(resolved)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        struct stat st;
        if (stat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المسار غير موجود: %s", disp);
        }
        if (S_ISDIR(st.st_mode)) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "ليس ملفا: %s", disp);
        }
        if (unlink(resolved) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر حذف الملف %s: %s", disp, osb);
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "احذف_تكراري") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف_تكراري يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        const char *root_msg = nv_root_refusal_text(args[0].u.str->data);
        if (root_msg) {
            nv_error_fmt(NV_ERR_IO, line, col, "%s", root_msg);
        }
        // فحص قانوني قبل الحل: الرابط الرمزي إلى
        // الجذر يرفضه الساندبوكس برسالة عامة،
        // والقرار يطلب رسالة الجذر.
        if (nv_delete_target_is_root(args[0].u.str->data)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        if (nv_is_filesystem_root(resolved)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن حذف الجذر: /");
        }
        struct stat st;
        if (nv_lstat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المسار غير موجود: %s", disp);
        }
        if (nv_remove_tree(resolved) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر حذف %s: %s", disp, osb);
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "انسخ") == 0 || strcmp(name, "انقل") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "%s يتطلب مسارين نصيين", name);
        }
        int is_copy = (strcmp(name, "انسخ") == 0);
        char *src_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *dst_path = nv_resolve_path(args[1].u.str->data, line, col);
        struct stat dst_st;
        if (stat(dst_path, &dst_st) == 0 && S_ISDIR(dst_st.st_mode)) {
            const char *leaf = strrchr(src_path, '/');
            leaf = leaf ? leaf + 1 : src_path;
            size_t n = strlen(dst_path) + 1 + strlen(leaf) + 1;
            char *joined = (char *)malloc(n);
            snprintf(joined, n, "%s/%s", dst_path, leaf);
            free(dst_path);
            dst_path = joined;
        }
        if (is_copy) {
            struct stat sst, dst_e;
            unsigned long long src_len = 0, dst_existing = 0;
            int src_ok = (stat(src_path, &sst) == 0);
            int dst_ok = (stat(dst_path, &dst_e) == 0);
            if (src_ok) src_len = (unsigned long long)sst.st_size;
            if (dst_ok) dst_existing = (unsigned long long)dst_e.st_size;
            // الاجتماع 59 §74 (الميزة 196): نسخ مجلد
            // بلا خيار تكراري مرفوض برسالة صريحة تسمي
            // البديل — لا خطأ إدخال/إخراج مبهم.
            if (src_ok && S_ISDIR(sst.st_mode)) {
                nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري");
            }
            // الميزة 202: نسخ الملف على نفسه يفرّغه
            // (فتح الهدف "wb" يقطعه قبل القراءة) —
            // ارفض قبل أي كتابة (هوية ملف حقيقية عبر
            // nv_same_file — لا st_ino مباشرة).
            if (src_ok && dst_ok && nv_same_file(src_path, dst_path)) {
                nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ الملف على نفسه");
            }
            unsigned long long extra = src_len > dst_existing ? src_len - dst_existing : 0;
            nv_check_quota((size_t)extra, line, col);
            if (nv_copy_file(src_path, dst_path) != 0) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر نسخ %s: %s", disp, osb);
            }
        } else {
            // الميزة 204: نقل مجلد داخل نفسه أو داخل
            // فرع منه مرفوض قبل أي كتابة (كما في `mv`)؛
            // والملف على نفسه يبقى بلا أثر (إعادة
            // تسمية على النفس).
            struct stat sst2;
            if (stat(src_path, &sst2) == 0 && S_ISDIR(sst2.st_mode)) {
                char *src_c = nv_canonical_with_suffix(src_path);
                char *dst_c = nv_canonical_with_suffix(dst_path);
                if (nv_path_starts_with(dst_c, src_c)) {
                    free(src_c);
                    free(dst_c);
                    nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نقل المجلد داخل نفسه");
                }
                free(src_c);
                free(dst_c);
            }
            if (rename(src_path, dst_path) != 0) {
                int e = errno;
                if (e != EXDEV) {
                    char osb[256];
                    nv_os_error_str(e, osb, sizeof(osb));
                    char *disp = nv_display_str(args[0].u.str->data);
                    nv_error_fmt(NV_ERR_IO, line, col, "تعذر نقل %s: %s", disp, osb);
                }
                // الميزة 204: عبر الأنظمة — نسخ ثم حذف
                // المصدر بعد نجاح النسخ كاملا فقط.
                struct stat sst3, dst_e;
                int is_dir = (stat(src_path, &sst3) == 0 && S_ISDIR(sst3.st_mode));
                int dst_ok = (stat(dst_path, &dst_e) == 0);
                unsigned long long dst_existing = dst_ok ? (unsigned long long)dst_e.st_size : 0;
                size_t extra = 0;
                if (is_dir) {
                    int budget = 10000;
                    extra = (size_t)nv_dir_size_rec(src_path, &budget);
                } else {
                    unsigned long long src_len = (stat(src_path, &sst3) == 0) ? (unsigned long long)sst3.st_size : 0;
                    extra = (size_t)(src_len > dst_existing ? src_len - dst_existing : 0);
                }
                // الحصة على نمو مجلد البرنامج وحده.
                if (nv_path_starts_with(dst_path, nv_program_dir)) {
                    nv_check_quota(extra, line, col);
                }
                struct stat dst_l;
                int dst_existed = (nv_lstat(dst_path, &dst_l) == 0);
                int copy_rc = is_dir ? nv_copy_tree(src_path, dst_path) : nv_copy_file(src_path, dst_path);
                if (copy_rc != 0) {
                    int ce = errno;
                    if (!dst_existed) nv_remove_tree(dst_path);
                    char osb[256];
                    nv_os_error_str(ce, osb, sizeof(osb));
                    char *disp = nv_display_str(args[0].u.str->data);
                    nv_error_fmt(NV_ERR_IO, line, col, "تعذر نقل %s: %s", disp, osb);
                }
                if (nv_remove_tree(src_path) != 0) {
                    int re = errno;
                    char osb[256];
                    nv_os_error_str(re, osb, sizeof(osb));
                    char *dsp = nv_display_str(args[0].u.str->data);
                    char *ddp = nv_display_str(args[1].u.str->data);
                    nv_error_fmt(NV_ERR_IO, line, col, "نُسخ %s إلى %s لكن تعذر حذف المصدر: %s", dsp, ddp, osb);
                }
            }
        }
        free(src_path);
        free(dst_path);
        return nv_nil();
    }
    if (strcmp(name, "انسخ_تكراري") == 0 || strcmp(name, "انسخ_أرشيف") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "%s يتطلب مسارين نصيين", name);
        }
        int is_archive = (strcmp(name, "انسخ_أرشيف") == 0);
        char *src_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *dst_path = nv_resolve_path(args[1].u.str->data, line, col);
        struct stat dst_st;
        if (stat(dst_path, &dst_st) == 0 && S_ISDIR(dst_st.st_mode)) {
            const char *leaf = strrchr(src_path, '/');
            leaf = leaf ? leaf + 1 : src_path;
            size_t n = strlen(dst_path) + 1 + strlen(leaf) + 1;
            char *joined = (char *)malloc(n);
            snprintf(joined, n, "%s/%s", dst_path, leaf);
            free(dst_path);
            dst_path = joined;
        }
        struct stat sst, dst_e;
        int src_ok = (stat(src_path, &sst) == 0);
        int dst_ok = (stat(dst_path, &dst_e) == 0);
        int src_is_dir = src_ok && S_ISDIR(sst.st_mode);
        if (src_is_dir) {
            // الهدف داخل المصدر مرفوض قبل أي كتابة
            // (كما في `mv` — الميزة 204).
            char *src_c = nv_canonical_with_suffix(src_path);
            char *dst_c = nv_canonical_with_suffix(dst_path);
            if (nv_path_starts_with(dst_c, src_c)) {
                free(src_c);
                free(dst_c);
                nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ المجلد داخل نفسه");
            }
            free(src_c);
            free(dst_c);
        } else if (src_ok && dst_ok && nv_same_file(src_path, dst_path)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ الملف على نفسه");
        }
        unsigned long long extra = 0;
        if (src_is_dir) {
            int budget = 10000;
            extra = nv_dir_size_rec(src_path, &budget);
        } else if (src_ok) {
            unsigned long long dst_existing = dst_ok ? (unsigned long long)dst_e.st_size : 0;
            extra = (unsigned long long)sst.st_size > dst_existing
                ? (unsigned long long)sst.st_size - dst_existing
                : 0;
        }
        nv_check_quota((size_t)extra, line, col);
        int rc = is_archive ? nv_copy_archive(src_path, dst_path)
            : (src_is_dir ? nv_copy_tree(src_path, dst_path)
                : nv_copy_file(src_path, dst_path));
        if (rc != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر نسخ %s: %s", disp, osb);
        }
        free(src_path);
        free(dst_path);
        return nv_nil();
    }
    if (strcmp(name, "انسخ_محدث") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "انسخ_محدث يتطلب مسارين نصيين");
        }
        char *src_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *dst_path = nv_resolve_path(args[1].u.str->data, line, col);
        struct stat dst_st;
        if (stat(dst_path, &dst_st) == 0 && S_ISDIR(dst_st.st_mode)) {
            const char *leaf = strrchr(src_path, '/');
            leaf = leaf ? leaf + 1 : src_path;
            size_t n = strlen(dst_path) + 1 + strlen(leaf) + 1;
            char *joined = (char *)malloc(n);
            snprintf(joined, n, "%s/%s", dst_path, leaf);
            free(dst_path);
            dst_path = joined;
        }
        struct stat sst, dst_e;
        int src_ok = (stat(src_path, &sst) == 0);
        int dst_ok = (stat(dst_path, &dst_e) == 0);
        // الاجتماع 59 §74 (الميزة 199): المجلد مصدرا
        // مرفوض برسالة 196 نفسها.
        if (src_ok && S_ISDIR(sst.st_mode)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري");
        }
        if (src_ok && dst_ok && nv_same_file(src_path, dst_path)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ الملف على نفسه");
        }
        // ينسخ فقط حين يكون المصدر أحدث من الهدف
        // (المساوي يتخطى كما في `cp -u`)، وينسخ إن
        // غاب الهدف؛ ويرجع منطقيا (نُسخ؟).
        if (src_ok && dst_ok) {
            long s_sec = (long)NV_MTIME_SEC(sst);
            long d_sec = (long)NV_MTIME_SEC(dst_e);
            long s_nsec = (long)NV_MTIME_NSEC(sst);
            long d_nsec = (long)NV_MTIME_NSEC(dst_e);
            if (s_sec < d_sec || (s_sec == d_sec && s_nsec <= d_nsec)) {
                free(src_path);
                free(dst_path);
                return nv_bool(0);
            }
        }
        unsigned long long src_len = src_ok ? (unsigned long long)sst.st_size : 0;
        unsigned long long dst_existing = dst_ok ? (unsigned long long)dst_e.st_size : 0;
        unsigned long long extra = src_len > dst_existing ? src_len - dst_existing : 0;
        nv_check_quota((size_t)extra, line, col);
        if (nv_copy_file(src_path, dst_path) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر نسخ %s: %s", disp, osb);
        }
        free(src_path);
        free(dst_path);
        return nv_bool(1);
    }
    if (strcmp(name, "انسخ_خفيف") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "انسخ_خفيف يتطلب مسارين نصيين");
        }
        char *src_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *dst_path = nv_resolve_path(args[1].u.str->data, line, col);
        struct stat dst_st;
        if (stat(dst_path, &dst_st) == 0 && S_ISDIR(dst_st.st_mode)) {
            const char *leaf = strrchr(src_path, '/');
            leaf = leaf ? leaf + 1 : src_path;
            size_t n = strlen(dst_path) + 1 + strlen(leaf) + 1;
            char *joined = (char *)malloc(n);
            snprintf(joined, n, "%s/%s", dst_path, leaf);
            free(dst_path);
            dst_path = joined;
        }
        struct stat sst, dst_e;
        int src_ok = (stat(src_path, &sst) == 0);
        int dst_ok = (stat(dst_path, &dst_e) == 0);
        if (src_ok && S_ISDIR(sst.st_mode)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري");
        }
        if (src_ok && dst_ok && nv_same_file(src_path, dst_path)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن نسخ الملف على نفسه");
        }
        unsigned long long src_len = src_ok ? (unsigned long long)sst.st_size : 0;
        unsigned long long dst_existing = dst_ok ? (unsigned long long)dst_e.st_size : 0;
        unsigned long long extra = src_len > dst_existing ? src_len - dst_existing : 0;
        nv_check_quota((size_t)extra, line, col);
        int method = nv_copy_light(src_path, dst_path);
        if (method < 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر نسخ %s: %s", disp, osb);
        }
        free(src_path);
        free(dst_path);
        return nv_str_cstr(method == 0 ? "مستنسخ" : method == 1 ? "متناثر" : "عادي");
    }
    if (strcmp(name, "أنشئ_ملف") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "أنشئ_ملف يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        if (access(resolved, F_OK) != 0) {
            FILE *f = fopen(resolved, "wb");
            if (!f) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الملف %s: %s", disp, osb);
            }
            fclose(f);
        } else {
            // الاجتماع 59 §74 (الميزة 215): الموجود
            // يحدَّث وقته بلا مساس بالمحتوى (`touch`).
            if (nv_touch(resolved) != 0) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر تحديث وقت %s: %s", disp, osb);
            }
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "اعرض_البداية") == 0 || strcmp(name, "اعرض_النهاية") == 0) {
        if (nargs < 1 || nargs > 2) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "%s تتوقع 1 أو 2 معاملات", name);
        }
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "%s يتطلب مسارا نصيا، وليس %s", name, nv_type_name(args[0]));
        }
        size_t count = (nargs == 2) ? nv_count_arg(args[1], line, col) : 5;
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        char *disp = nv_display_str(args[0].u.str->data);
        size_t n = 0;
        char *content = nv_read_file_capped(resolved, disp, line, col, &n);
        // قسّم إلى أسطر (Rust lines(): بلا \n، وبلا سطر أخير فارغ).
        char **lines = NULL;
        size_t nlines = 0, capl = 0;
        char *p = content;
        char *end = content + n;
        while (p < end) {
            char *nl = memchr(p, '\n', (size_t)(end - p));
            size_t llen = nl ? (size_t)(nl - p) : (size_t)(end - p);
            if (llen > 0 && p[llen - 1] == '\r') llen--;
            if (nlines == capl) {
                capl = capl ? capl * 2 : 16;
                lines = (char **)realloc(lines, capl * sizeof(char *));
            }
            lines[nlines++] = nv_str_new(p, llen).u.str->data;
            p = nl ? nl + 1 : end;
        }
        // اجمع المطلوب.
        size_t total_len = 0;
        size_t from = 0, to = nlines;
        if (strcmp(name, "اعرض_البداية") == 0) {
            to = count < nlines ? count : nlines;
        } else {
            from = nlines > count ? nlines - count : 0;
        }
        for (size_t i = from; i < to; i++) total_len += strlen(lines[i]) + 1;
        char *out = (char *)malloc(total_len + 1);
        size_t w = 0;
        for (size_t i = from; i < to; i++) {
            if (i > from) out[w++] = '\n';
            size_t l = strlen(lines[i]);
            memcpy(out + w, lines[i], l);
            w += l;
        }
        out[w] = '\0';
        NvValue v = nv_str_take(out, w);
        free(resolved);
        free(disp);
        free(content);
        free(lines);
        return v;
    }
    if (strcmp(name, "عد_الأسطر") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "عد_الأسطر يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        char *disp = nv_display_str(args[0].u.str->data);
        size_t n = 0;
        char *content = nv_read_file_capped(resolved, disp, line, col, &n);
        size_t count = 0;
        char *p = content, *end = content + n;
        while (p < end) {
            char *nl = memchr(p, '\n', (size_t)(end - p));
            count++;
            p = nl ? nl + 1 : end;
        }
        free(resolved);
        free(disp);
        free(content);
        return nv_num((double)count);
    }
    if (strcmp(name, "عد_الكلمات") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "عد_الكلمات يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        char *disp = nv_display_str(args[0].u.str->data);
        size_t n = 0;
        char *content = nv_read_file_capped(resolved, disp, line, col, &n);
        // split_whitespace: كلمات مفصولة ببياض Unicode.
        size_t count = 0;
        const char *p = content, *end = content + n;
        int in_word = 0;
        while (p < end) {
            const char *save = p;
            unsigned int cp = nv_decode_cp(&p, end);
            if (p == save) break;
            if (nv_is_space_cp(cp)) in_word = 0;
            else if (!in_word) { in_word = 1; count++; }
        }
        free(resolved);
        free(disp);
        free(content);
        return nv_num((double)count);
    }
    if (strcmp(name, "ابحث_في_ملف") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "ابحث_في_ملف يتطلب نصا ومسارا", line, col);
        }
        char *resolved = nv_resolve_path(args[1].u.str->data, line, col);
        char *disp = nv_display_str(args[1].u.str->data);
        size_t n = 0;
        char *content = nv_read_file_capped(resolved, disp, line, col, &n);
        NvValue out = nv_list_new();
        const char *pat = args[0].u.str->data;
        char *p = content, *end = content + n;
        while (p < end && out.u.list->len < 100) {
            char *nl = memchr(p, '\n', (size_t)(end - p));
            size_t llen = nl ? (size_t)(nl - p) : (size_t)(end - p);
            size_t ll = llen;
            if (ll > 0 && p[ll - 1] == '\r') ll--;
            // يحتوي النمط؟ (بحث بايتات؛ والنمط نص UTF-8).
#ifdef _WIN32
            if (args[0].u.str->len == 0 || nv_memmem(p, ll, pat, args[0].u.str->len)) {
#else
            if (args[0].u.str->len == 0 || memmem(p, ll, pat, args[0].u.str->len)) {
#endif
                nv_list_push_raw(out.u.list, nv_str_new(p, ll));
            }
            p = nl ? nl + 1 : end;
        }
        free(resolved);
        free(disp);
        free(content);
        return out;
    }
    if (strcmp(name, "ابحث_عن") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "ابحث_عن يتطلب نصا، وليس %s", nv_type_name(args[0]));
        }
        NvValue out = nv_list_new();
        nv_find_files(nv_cwd, nv_cwd, args[0].u.str->data, 0, out);
        // رتب النتائج (المحرك يرتب بعد الجمع).
        NvList *l = out.u.list;
        for (size_t i = 0; i < l->len; i++) {
            for (size_t j = i + 1; j < l->len; j++) {
                if (strcmp(l->items[j].u.str->data, l->items[i].u.str->data) < 0) {
                    NvValue t = l->items[i];
                    l->items[i] = l->items[j];
                    l->items[j] = t;
                }
            }
        }
        return out;
    }
    if (strcmp(name, "اعرض_الشجرة") == 0) {
        if (nargs > 1) {
            nv_error(NV_ERR_RUNTIME, "اعرض_الشجرة تتوقع 0 أو 1 معاملات", line, col);
        }
        char *dir = NULL;
        if (nargs == 1) {
            if (args[0].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "اعرض_الشجرة يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
            }
            dir = nv_resolve_path(args[0].u.str->data, line, col);
        } else {
            dir = nv_strdup(nv_cwd);
        }
        NvValue out = nv_list_new();
        nv_tree_lines(dir, "", 0, out);
        free(dir);
        return out;
    }
    if (strcmp(name, "حجم_المسار") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "حجم_المسار يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        int budget = 10000;
        unsigned long long sz = nv_dir_size_rec(resolved, &budget);
        free(resolved);
        return nv_num((double)sz);
    }
    if (strcmp(name, "أنشئ_رابط") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "أنشئ_رابط يتطلب مسارين نصيين", line, col);
        }
        char *target_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *link_path = nv_resolve_path(args[1].u.str->data, line, col);
#ifdef _WIN32
        // الرابط الرمزي مدعوم على ويندوز عبر CreateSymbolicLinkA
        // (المحرك يدعمه أيضا)؛ وعلم الدليل من فحص الهدف، وعلم
        // ALLOW_UNPRIVILEGED_CREATE يسمح به بلا صلاحيات إدارية.
        {
            struct stat stt;
            DWORD flags = SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE;
            if (stat(target_path, &stt) == 0 && S_ISDIR(stt.st_mode)) flags |= SYMBOLIC_LINK_FLAG_DIRECTORY;
            if (!CreateSymbolicLinkA(link_path, target_path, flags)) {
                char *werr = nv_win32_error_str();
                char *disp = nv_display_str(args[1].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط %s: %s", disp, werr);
            }
        }
#else
        if (symlink(target_path, link_path) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[1].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط %s: %s", disp, osb);
        }
#endif
        free(target_path);
        free(link_path);
        return nv_nil();
    }
    if (strcmp(name, "أنشئ_رابط_صلب") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "أنشئ_رابط_صلب يتطلب مسارين نصيين", line, col);
        }
        char *target_path = nv_resolve_path(args[0].u.str->data, line, col);
        char *link_path = nv_resolve_path(args[1].u.str->data, line, col);
        // الاجتماع 59 §74 (الميزة 212): الصلب يشترط
        // هدفا موجودا، ويرفض المجلد صراحة.
        struct stat stt;
        if (stat(target_path, &stt) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "الهدف غير موجود: %s", disp);
        }
        if (S_ISDIR(stt.st_mode)) {
            nv_error_fmt(NV_ERR_IO, line, col, "لا يمكن إنشاء رابط صلب لمجلد");
        }
#ifdef _WIN32
        // ويندوز: بلا `link` في MinGW — يُبنى عبر
        // `CreateHardLinkA` (القيد مسجل: الفشل هنا
        // خطأ إدخال/إخراج عربي كغيره).
        if (!CreateHardLinkA(link_path, target_path, NULL)) {
            char *werr = nv_win32_error_str();
            char *disp = nv_display_str(args[1].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط الصلب %s: %s", disp, werr);
        }
#else
        if (link(target_path, link_path) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[1].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط الصلب %s: %s", disp, osb);
        }
#endif
        free(target_path);
        free(link_path);
        return nv_nil();
    }
    if (strcmp(name, "أنشئ_رابط_نسبي") == 0) {
        nv_arity(args, nargs, 2, name, line, col);
        if (args[0].tag != NV_STR || args[1].tag != NV_STR) {
            nv_error(NV_ERR_TYPE, "أنشئ_رابط_نسبي يتطلب مسارين نصيين", line, col);
        }
        // الاجتماع 59 §74 (الميزة 213): النسبي يحسب
        // من موقع الرابط لا من الجلسة — يحل الطرفين
        // إلى مطلقين ثم يحسب النسبي بينهما، ويخزن
        // النص النسبي حرفيا.
        char *target_abs = nv_resolve_path(args[0].u.str->data, line, col);
        char *link_path = nv_resolve_path(args[1].u.str->data, line, col);
        char *link_dir = nv_strdup(link_path);
        char *slash = strrchr(link_dir, '/');
        if (slash) *slash = '\0';
        if (link_dir[0] == '\0') {
            free(link_dir);
            link_dir = nv_strdup(nv_cwd);
        }
        char *rel = nv_relative_path(link_dir, target_abs);
#ifdef _WIN32
        {
            struct stat stt;
            DWORD flags = SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE;
            if (stat(target_abs, &stt) == 0 && S_ISDIR(stt.st_mode)) flags |= SYMBOLIC_LINK_FLAG_DIRECTORY;
            if (!CreateSymbolicLinkA(link_path, rel, flags)) {
                char *werr = nv_win32_error_str();
                char *disp = nv_display_str(args[1].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط %s: %s", disp, werr);
            }
        }
#else
        if (symlink(rel, link_path) != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[1].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء الرابط %s: %s", disp, osb);
        }
#endif
        free(target_abs);
        free(link_path);
        free(link_dir);
        free(rel);
        return nv_nil();
    }
    if (strcmp(name, "التاريخ_الآن") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        unsigned long long secs = (unsigned long long)time(NULL);
        char buf[40];
        nv_format_unix_utc(secs, buf, sizeof(buf));
        return nv_str_cstr(buf);
    }
    if (strcmp(name, "النسخة") == 0) {
        nv_arity(args, nargs, 0, name, line, col);
        return nv_str_cstr("محرك نيفا ١٫٠٫٠");
    }
    if (strcmp(name, "موجود") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "موجود يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        int ok = (stat(resolved, &st) == 0);
        free(resolved);
        return nv_bool(ok);
    }
    if (strcmp(name, "نوع_المسار") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "نوع_المسار يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        const char *kind = "غير موجود";
        if (stat(resolved, &st) == 0) kind = S_ISDIR(st.st_mode) ? "مجلد" : "ملف";
        free(resolved);
        return nv_str_cstr(kind);
    }
    if (strcmp(name, "تفاصيل_المسار") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "تفاصيل_المسار يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        const char *kind = "غير موجود";
        if (stat(resolved, &st) == 0) kind = S_ISDIR(st.st_mode) ? "مجلد" : "ملف";
        int budget = 10000;
        unsigned long long sz = nv_dir_size_rec(resolved, &budget);
        char modified[40];
        if (stat(resolved, &st) == 0) {
            nv_format_unix_utc((unsigned long long)st.st_mtime, modified, sizeof(modified));
        } else {
            snprintf(modified, sizeof(modified), "—");
        }
        NvValue out = nv_dict_new();
        nv_dict_set_raw(out.u.dict, "نوع", nv_str_cstr(kind));
        nv_dict_set_raw(out.u.dict, "حجم", nv_num((double)sz));
        nv_dict_set_raw(out.u.dict, "معدل", nv_str_cstr(modified));
        free(resolved);
        return out;
    }
    if (strcmp(name, "تثبيت") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "تثبيت يتطلب مصدرا نصيا، وليس %s", nv_type_name(args[0]));
        }
        const char *src = args[0].u.str->data;
        // رابط (http/https أو github) يحتاج الشبكة — والخلفية
        // C تدعم HTTP العادي فقط؛ وHTTPS مرفوض برسالة عربية.
        if (strncmp(src, "http://", 7) == 0 || strncmp(src, "https://", 8) == 0 ||
            strncmp(src, "github.com/", 11) == 0 || strncmp(src, "www.github.com/", 15) == 0) {
            nv_error(NV_ERR_RUNTIME, "تثبيت من رابط غير مدعوم في الخلفية الأصلية — استخدم مسارا محليا", line, col);
        }
        return nv_install_local(src, line, col);
    }
    if (strcmp(name, "عد_الملفات") == 0) {
        if (nargs > 1) {
            nv_error(NV_ERR_RUNTIME, "عد_الملفات تتوقع 0 أو 1 معاملات", line, col);
        }
        char *dir = NULL;
        if (nargs == 1) {
            if (args[0].tag != NV_STR) {
                nv_error_fmt(NV_ERR_TYPE, line, col, "عد_الملفات يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
            }
            dir = nv_resolve_path(args[0].u.str->data, line, col);
        } else {
            dir = nv_strdup(nv_cwd);
        }
        DIR *d = opendir(dir);
        if (!d) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر عرض المجلد: %s", osb);
        }
        size_t count = 0;
        struct dirent *e;
        while ((e = readdir(d)) != NULL) {
            if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
            count++;
        }
        closedir(d);
        free(dir);
        return nv_num((double)count);
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "دالة غير معرفة: %s", name);
}

// ---------- مساعدات المولد ----------
void nv_check_assign_type(NvValue v, const char *type_name, const char *var_name, int line, int col) {
    if (!type_name || strcmp(type_name, "أي") == 0) return;
    if (!nv_value_matches_decl(type_name, v)) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "نوع غير متطابق: المتغير %s معلن %s، لكن القيمة %s",
                     var_name, type_name, nv_type_name(v));
    }
}

NvValue nv_lookup_or_nil(NvScope *scope, const char *name) {
    NvCell *cell = nv_scope_lookup_cell(scope, name);
    if (!cell) return nv_nil();
    return cell->val;
}

NvValue nv_module_export(NvValue module, const char *name, int line, int col) {
    if (module.tag != NV_MODULE) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليست وحدة، بل %s", nv_type_name(module));
    }
    NvDict *ex = module.u.module->exports;
    int i = ex ? nv_dict_find(ex, name) : -1;
    if (i < 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "الاسم %s غير مصدر من وحدة %s. أضف صدّر %s في وحدة %s",
                     name, module.u.module->name, name, module.u.module->name);
    }
    return ex->entries[i].val;
}

NvValue nv_class_begin_named(NvScope *scope, const char *name, const char *base_name, int line, int col, const char *type_params) {
    NvValue base_val = nv_nil();
    if (base_name) {
        NvCell *cell = nv_scope_lookup_cell(scope, base_name);
        if (!cell) {
            nv_error_fmt(NV_ERR_RUNTIME, line, col, "فئة أب غير معرفة: %s", base_name);
        }
        base_val = cell->val;
        if (base_val.tag != NV_CLASS) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "%s ليست فئة، بل %s", base_name, nv_type_name(base_val));
        }
    }
    return nv_class_begin(name, base_val, line, col, type_params);
}

// ================= الجزء 4: نقطة دخول الثنائي المولد =================
// أعلام التشغيل كما في standalone_usage (main.rs): المساعدة
// والنسخة والصلاحيات الأربع؛ وأي علم آخر خطأ عربي.

static const char *nv_standalone_usage =
    "استخدام: <البرنامج> [--اسمح-بالكل] [--اسمح-بالشبكة] [--مضيف-محلي مضيف]\n"
    "برنامج نيفا مستقل — يعمل وحده بلا تثبيت نيفا.\n"
    "--مساعدة         عرض هذه المساعدة\n"
    "--نسخة           عرض إصدار المحرك\n"
    "--اسمح-بالكل   السماح بملفات خارج مجلد البرنامج (للموثوقة فقط)\n"
    "--اسمح-بالشبكة السماح بالشبكة — مغلقة افتراضيا\n"
    "--اسمح-بالخادم السماح بالخادم — مغلق افتراضيا، وعلم الشبكة وحده لا يكفي\n"
    "--مضيف-محلي مضيف السماح بمضيف محلي صراحة (مثل 127.0.0.1) — وحده لا يفتح الشبكة\n";

// مجلد الملف التنفيذي الحالي (مجلد البرنامج للثنائي الأصلي).
static char *nv_exe_dir(void) {
#if defined(__linux__) || defined(__APPLE__)
    char buf[4096];
#endif
#if defined(__linux__)
    ssize_t n = readlink("/proc/self/exe", buf, sizeof(buf) - 1);
    if (n > 0) {
        buf[n] = '\0';
        char *slash = strrchr(buf, '/');
        if (slash) {
            *slash = '\0';
            return nv_strdup(buf[0] ? buf : "/");
        }
    }
#elif defined(__APPLE__)
    uint32_t size = (uint32_t)sizeof(buf);
    if (_NSGetExecutablePath(buf, &size) == 0) {
        char *real = realpath(buf, NULL);
        if (real) {
            char *slash = strrchr(real, '/');
            if (slash) *slash = '\0';
            char *out = nv_strdup(real);
            free(real);
            return out;
        }
    }
#endif
    // احتياطي: argv[0] كما كُتب.
    return NULL;
}

int nv_main(int argc, char **argv, void (*body)(NvScope *top)) {
#ifdef _WIN32
    // الوضع الثنائي للمخرجات على ويندوز حتى تطابق
    // بايتات الأصلي بايتات المفسر (اجتماع 36 §51):
    // فالوضع النصي يترجم \n إلى \r\n عند الطباعة،
    // فيختلف المخرج عن المفسر بايتًا بايتًا؛ وهذا
    // أول سطر ينفَّذ في البرنامج الأصلي كله، فيغطي
    // حتى --مساعدة و--نسخة.
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
    setlocale(LC_ALL, "");
    // الدفعة 1: اسم البرنامج كما استُدعي (argv[0]) —
    // يقرأه `اسم_البرنامج()`؛ وفي التفاعلي «نيفا».
    if (argc > 0 && argv[0]) {
        nv_program_name = argv[0];
    }
    // حلل الأعلام (المطابقة على الصيغة المطبّعة: ة -> ه).
    // (الأعلام هنا بلا تشكيل أصلا في الاستخدام.)
    for (int i = 1; i < argc; i++) {
        const char *a = argv[i];
        if (strcmp(a, "--مساعدة") == 0 || strcmp(a, "--مساعده") == 0) {
            fputs(nv_standalone_usage, stdout);
            return 0;
        }
        if (strcmp(a, "--نسخة") == 0 || strcmp(a, "--نسخه") == 0) {
            printf("محرك نيفا ١٫٠٫٠\n");
            return 0;
        }
        if (strcmp(a, "--اسمح-بالكل") == 0) {
            nv_trusted = 1;
        } else if (strcmp(a, "--اسمح-بالشبكة") == 0 || strcmp(a, "--اسمح-بالشبكه") == 0) {
            nv_network_allowed = 1;
        } else if (strcmp(a, "--اسمح-بالخادم") == 0) {
            nv_server_allowed = 1;
        } else if (strcmp(a, "--مضيف-محلي") == 0) {
            if (i + 1 >= argc) {
                fprintf(stderr, "خيار --مضيف-محلي يحتاج مضيفا بعده\n");
                fputs(nv_standalone_usage, stderr);
                // الدفعة 1: خطأ استخدام → 2.
                return 2;
            }
            nv_local_hosts = (char **)realloc(nv_local_hosts, (size_t)(nv_n_local_hosts + 1) * sizeof(char *));
            nv_local_hosts[nv_n_local_hosts++] = argv[++i];
        } else if (a[0] == '-') {
            fprintf(stderr, "خيار غير معروف: %s\n", a);
            fputs(nv_standalone_usage, stderr);
            // الدفعة 1: علم غير معروف → 2.
            return 2;
        } else {
            fprintf(stderr, "خطأ: البرنامج المستقل لا يأخذ ملف مصدر — شغّله مباشرة\n");
            fputs(nv_standalone_usage, stderr);
            // الدفعة 1: وسائط خاطئة → 2.
            return 2;
        }
    }
    char *dir = nv_exe_dir();
    if (!dir) {
        // احتياطي: مجلد التشغيل الحالي.
        char *cwd = getcwd(NULL, 0);
        dir = cwd ? cwd : nv_strdup(".");
    }
    nv_init(dir);
    free(nv_cwd);
    // اجتماع 45 §59: الجلسة تبدأ في البيت.
    nv_cwd = nv_strdup(nv_home_dir);
    free(dir);
    // النطاق العلوي + إطار الجذر.
    NvScope *top = nv_scope_new(NULL);
    nv_frames_push(top);
    volatile NvCheckpoint cp = nv_checkpoint();
    NvHandler top_h;
    top_h.prev = NULL;
    top_h.has_except = 0;
    nv_handlers = &top_h;
    int code = 0;
    if (setjmp(top_h.jb) == 0) {
        body(top);
    } else {
        nv_restore_checkpoint(cp);
        // الدفعة 1: خروج معلق → الرمز كما طُلب بلا
        // طباعة خطأ؛ وإلا خطأ وقت تشغيل غير ممسوك → 1.
        if (nv_exit_requested) {
            code = nv_exit_code;
        } else {
            NvError *e = nv_current_error();
            if (e) {
                nv_print_error(e->kind, e->message, e->line, e->col, e->file_idx);
            }
            code = 1;
        }
    }
    nv_handlers = NULL;
    nv_frames_pop();
    fflush(stdout);
    return code;
}

// ---------- لقطة التكرار وعدّ التكرار وبحث يخطئ ----------
// لقطة لكل: قائمة -> نسخة سطحية من عناصرها؛ نص -> قائمة
// حروف؛ وإلا «لا يمكن التكرار على نوع X».
NvValue nv_for_snapshot(NvValue v, int line, int col) {
    if (v.tag == NV_LIST) {
        NvValue snap = nv_list_new();
        NvList *src = v.u.list;
        NvList *dst = snap.u.list;
        for (size_t i = 0; i < src->len; i++) {
            nv_list_push_raw(dst, src->items[i]);
        }
        return snap;
    }
    // البندان 138 و139: لقطة المترابطة والتشكيلة
    // كالقائمة (عناصرها بترتيب أول ظهور).
    if (v.tag == NV_TUPLE) {
        NvValue snap = nv_list_new();
        NvList *src = v.u.tuple;
        NvList *dst = snap.u.list;
        for (size_t i = 0; i < src->len; i++) {
            nv_list_push_raw(dst, src->items[i]);
        }
        return snap;
    }
    if (v.tag == NV_SET) {
        NvValue snap = nv_list_new();
        NvList *src = v.u.set;
        NvList *dst = snap.u.list;
        for (size_t i = 0; i < src->len; i++) {
            nv_list_push_raw(dst, src->items[i]);
        }
        return snap;
    }
    if (v.tag == NV_STR) {
        NvValue snap = nv_list_new();
        const char *s = v.u.str->data;
        size_t slen = v.u.str->len;
        size_t i = 0;
        while (i < slen) {
            size_t n = 0;
            (void)nv_utf8_char_at(s + i, slen - i, 0, &n);
            char *one = (char *)malloc(n + 1);
            memcpy(one, s + i, n);
            one[n] = '\0';
            nv_list_push_raw(snap.u.list, nv_str_take(one, n));
            i += n;
        }
        return snap;
    }
    // ج6: البت — لقطة أعداد 0/1 (الأهم أولا، كما في المفسر).
    if (v.tag == NV_BITS) {
        NvValue snap = nv_list_new();
        NvBits *b = v.u.bits;
        for (size_t i = 0; i < b->nbits; i++) {
            int bit = (b->bytes[i / 8] >> (7 - (i % 8))) & 1;
            nv_list_push_raw(snap.u.list, nv_num((double)bit));
        }
        return snap;
    }
    nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن التكرار على نوع %s", nv_type_name(v));
}

// عدّ كرر: عدد صحيح غير سالب، وأقل من حد التكرار.
long nv_repeat_count(NvValue v, int line, int col) {
    if (v.tag != NV_NUM) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "عدد مرات التكرار يجب أن يكون عددا، وليس %s", nv_type_name(v));
    }
    if (v.u.num != trunc(v.u.num) || v.u.num < 0.0) {
        nv_error(NV_ERR_TYPE, "عدد مرات التكرار يجب أن يكون عددا صحيحا غير سالب", line, col);
    }
    if (v.u.num > 1000000.0) {
        nv_error(NV_ERR_RUNTIME, "عدد مرات التكرار كبير جدا", line, col);
    }
    return (long)v.u.num;
}

// بحث متغير يخطئ «متغير غير معرف: X» (لقراءة معرّف
// وتعيين مركّب — كما في المفسر).
NvValue nv_lookup_or_error(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return nv_clone_value(cell->val);
}

// الدفعة الأخيرة DEF (البنود 77 و78): قراءة موجَّهة
// بالإعلان — كـ nv_lookup_or_error لكن من الجذر
// وحده (عام) أو من المحيط وحده (نطاق).
NvValue nv_lookup_or_error_global(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell_global(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return nv_clone_value(cell->val);
}

NvValue nv_lookup_or_error_nonlocal(NvScope *scope, const char *name, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell_nonlocal(scope, name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "متغير غير معرف: %s", name);
    }
    return nv_clone_value(cell->val);
}
