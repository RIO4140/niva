//! وقت تشغيل خلفية C — الجزء 1: الأخطاء، جامع القمامة
//! التتبعي (v1.3 §6)، النطاقات، بناء القيم، التنسيق.
//! كل الرسائل عربية مطابقة للمحرك (interpreter.rs / vm.rs).

#include "niva_rt.h"
#include <stdarg.h>
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
char *nv_cwd = NULL;
int nv_trusted = 0;
int nv_network_allowed = 0;
int nv_server_allowed = 0;
char **nv_local_hosts = NULL;
int nv_n_local_hosts = 0;
int nv_call_depth = 0;
unsigned long long nv_alloc_bytes = 0;

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
static void nv_mark_value(NvValue v) {
    if (v.tag == NV_NUM || v.tag == NV_BOOL || v.tag == NV_NIL) return;
    NvObj *o = NULL;
    switch (v.tag) {
        case NV_STR: o = (NvObj *)v.u.str; break;
        case NV_LIST: o = (NvObj *)v.u.list; break;
        case NV_DICT: o = (NvObj *)v.u.dict; break;
        case NV_FUNC: o = (NvObj *)v.u.func; break;
        case NV_NATIVE: o = (NvObj *)v.u.native; break;
        case NV_MODULE: o = (NvObj *)v.u.module; break;
        case NV_CLASS: o = (NvObj *)v.u.klass; break;
        case NV_IFACE: o = (NvObj *)v.u.iface; break;
        case NV_INST: o = (NvObj *)v.u.inst; break;
        case NV_BOUND: o = (NvObj *)v.u.bound; break;
        case NV_HANDLE: o = (NvObj *)v.u.handle; break;
        case NV_BYTES: o = (NvObj *)v.u.bytes; break;
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
            break;
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
            } else if (o->kind == NV_OBJ_MODULE) {
                free(((NvModule *)o)->name);
            } else if (o->kind == NV_OBJ_CLASS) {
                NvClass *c = (NvClass *)o;
                free(c->name);
                for (int i = 0; i < c->ninterfaces; i++) free(c->interfaces[i]);
                free(c->interfaces);
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

NvValue nv_func_new(const char *name, NvCFunc cfunc, NvScope *captured,
                    const char *module, NvScope *module_scope,
                    char **param_names, char **param_types,
                    const char *return_type, int nparams) {
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

NvValue nv_class_new(const char *name, NvClass *base) {
    NvClass *c = (NvClass *)nv_alloc(sizeof(NvClass), NV_OBJ_CLASS);
    c->name = nv_strdup(name);
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

// ---------- النطاقات ----------
NvScope *nv_scope_new(NvScope *parent) {
    NvScope *s = (NvScope *)nv_alloc(sizeof(NvScope), NV_OBJ_SCOPE);
    s->parent = parent;
    s->bindings = NULL;
    return s;
}

NvCell *nv_scope_lookup_cell(NvScope *scope, const char *name) {
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

void nv_scope_assign(NvScope *scope, const char *name, NvValue v) {
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
void nv_init(const char *program_dir) {
    nv_program_dir = nv_strdup(program_dir);
    char *cwd = getcwd(NULL, 0);
    nv_cwd = cwd ? cwd : nv_strdup(".");
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
    if (nv_is_num_like(l) && nv_is_num_like(r)) {
        return nv_num_of(l) == nv_num_of(r);
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
    nv_error_fmt(NV_ERR_TYPE, line, col, "لا يمكن جمع %s و %s", nv_type_name(l), nv_type_name(r));
}

static NvValue nv_arith(NvValue l, NvValue r, char op, int line, int col) {
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
    if (coll.tag == NV_STR && item.tag == NV_STR) {
        if (item.u.str->len == 0) return 1;
        return strstr(coll.u.str->data, item.u.str->data) != NULL;
    }
    if (coll.tag == NV_DICT && item.tag == NV_STR) {
        return nv_dict_find(coll.u.dict, item.u.str->data) >= 0;
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

// ---------- الفهرسة والنقطة ----------
NvValue nv_index(NvValue target, NvValue index, int line, int col, int idx_line, int idx_col) {
    if (target.tag == NV_LIST && index.tag == NV_NUM) {
        double x = index.u.num;
        if (x != trunc(x) || x < 0.0) {
            nv_error(NV_ERR_TYPE, "فهرس القائمة يجب أن يكون عددا صحيحا", idx_line, idx_col);
        }
        size_t idx = (size_t)x;
        if (idx >= target.u.list->len) {
            nv_error_fmt(NV_ERR_RUNTIME, idx_line, idx_col, "فهرس خارج النطاق: %zu والقائمة طولها %zu",
                         idx, target.u.list->len);
        }
        return target.u.list->items[idx];
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
        const char *p = nv_utf8_char_at(target.u.str->data, target.u.str->len, (size_t)x, &clen);
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
        size_t idx = (size_t)x;
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
void nv_check_type(NvValue v, const char *type_name, const char *what,
                   const char *func_name, int line, int col) {
    if (!type_name || strcmp(type_name, "أي") == 0) return;
    if (strcmp(nv_type_name(v), type_name) != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "نوع غير متطابق: %s معلن %s، لكن القيمة %s",
                     what, type_name, nv_type_name(v));
    }
}

// فحص نوع معامل دالة (رسالة المعامل).
static void nv_check_param_type(NvValue v, const char *param_name, const char *pt,
                                const char *func_name, int is_method, int line, int col) {
    if (!pt || strcmp(pt, "أي") == 0) return;
    if (strcmp(nv_type_name(v), pt) != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "نوع غير متطابق: معامل %s في %s %s معلن %s، لكن القيمة %s",
                     param_name, is_method ? "الطريقة" : "الدالة", func_name, pt, nv_type_name(v));
    }
}

// ---------- الاستدعاء ----------
static NvValue nv_call_func_impl(NvFunc *f, NvValue self, int has_self,
                                 NvValue *args, int nargs, int line, int col,
                                 int is_method) {
    // عدد المعاملات أولا (رسالة الدالة أو الطريقة).
    if (nargs != f->nparams) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "عدد المعاملات غير صحيح: %s %s تتوقع %d لكن أعطيت %d",
                     is_method ? "الطريقة" : "الدالة", f->name, f->nparams, nargs);
    }
    // أنواع المعاملات (بأسمائها).
    for (int i = 0; i < nargs; i++) {
        if (f->param_types && f->param_types[i]) {
            const char *pname = (f->param_names && f->param_names[i]) ? f->param_names[i] : "?";
            nv_check_param_type(args[i], pname, f->param_types[i], f->name, is_method, line, col);
        }
    }
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
    // نفس (للميتدات) ثم المعاملات.
    if (has_self) {
        nv_scope_define(frame, "نفس", self);
    }
    for (int i = 0; i < nargs; i++) {
        const char *pname = (f->param_names && f->param_names[i]) ? f->param_names[i] : "?";
        nv_scope_define(frame, pname, args[i]);
    }
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
        if (f->return_type && strcmp(f->return_type, "أي") != 0 &&
            strcmp(nv_type_name(v), f->return_type) != 0) {
            nv_error_fmt(NV_ERR_TYPE, line, col,
                         "نوع غير متطابق: %s %s معلنة أن ترجع %s، لكن رجعت %s",
                         is_method ? "الطريقة" : "الدالة", f->name, f->return_type, nv_type_name(v));
        }
        return v;
    }
    // بلا إرجاع صريح: لاشيء — إلا إن كان نوع الإرجاع
    // المعلن غير لاشيء/أي.
    if (f->return_type && strcmp(f->return_type, "أي") != 0 &&
        strcmp(f->return_type, "لاشيء") != 0) {
        nv_error_fmt(NV_ERR_TYPE, line, col,
                     "نوع غير متطابق: %s %s معلنة أن ترجع %s، لكن لم ترجع شيئا",
                     is_method ? "الطريقة" : "الدالة", f->name, f->return_type);
    }
    return nv_nil();
}

NvValue nv_call_at(NvValue callee, NvValue *args, int nargs, int line, int col,
                   int callee_line, int callee_col) {
    // جذّر الوسائط أثناء الاستدعاء (الإطار ينسخها بعدها).
    int base = (int)nv_root_len;
    for (int i = 0; i < nargs; i++) {
        size_t idx = nv_root_push_idx();
        *nv_root_slot(idx) = args[i];
    }
    NvValue result;
    switch (callee.tag) {
        case NV_FUNC:
            result = nv_call_func_impl(callee.u.func, nv_nil(), 0, args, nargs, line, col, 0);
            break;
        case NV_NATIVE:
            result = callee.u.native->fn(args, nargs, line, col);
            break;
        case NV_BOUND: {
            NvBound *b = callee.u.bound;
            NvValue self = nv_nil();
            self.tag = NV_INST;
            self.u.inst = b->inst;
            if (b->func.tag == NV_FUNC) {
                result = nv_call_func_impl(b->func.u.func, self, 1, args, nargs, line, col, 1);
            } else if (b->func.tag == NV_NATIVE) {
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

NvValue nv_call(NvValue callee, NvValue *args, int nargs, int line, int col) {
    return nv_call_at(callee, args, nargs, line, col, line, col);
}

// استدعاء باسم معرّف: النطاق أولا، ثم المدمجة.
NvValue nv_call_ident_at(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col,
                         int callee_line, int callee_col) {
    NvCell *cell = nv_scope_lookup_cell(scope, name);
    if (cell) {
        NvValue v = cell->val;
        if (v.tag == NV_CLASS) {
            nv_error_fmt(NV_ERR_TYPE, callee_line, callee_col,
                         "استخدم أنشئ لإنشاء كائن من فئة %s، مثل أنشئ %s()",
                         v.u.klass->name, v.u.klass->name);
        }
        return nv_call_at(v, args, nargs, line, col, callee_line, callee_col);
    }
    if (nv_is_builtin(name)) {
        return nv_call_builtin(name, args, nargs, line, col);
    }
    nv_error_fmt(NV_ERR_RUNTIME, line, col, "دالة غير معرفة: %s", name);
}

NvValue nv_call_ident(NvScope *scope, const char *name, NvValue *args, int nargs, int line, int col) {
    return nv_call_ident_at(scope, name, args, nargs, line, col, line, col);
}

// إنشاء كائن بالاسم (بحث النطاق أولا برسائل المفسر).
NvValue nv_create_by_name(NvScope *scope, const char *class_name, NvValue *args, int nargs, int line, int col) {
    NvCell *cell = nv_scope_lookup_cell(scope, class_name);
    if (!cell) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col, "فئة غير معرفة: %s", class_name);
    }
    NvValue class_val = cell->val;
    if (class_val.tag != NV_CLASS) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "%s ليست فئة، بل %s", class_name, nv_type_name(class_val));
    }
    return nv_create(class_val, args, nargs, line, col);
}

// إنشاء كائن من قيمة فئة.
NvValue nv_create(NvValue class_val, NvValue *args, int nargs, int line, int col) {
    if (class_val.tag != NV_CLASS) {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليست فئة، بل %s", nv_type_name(class_val));
    }
    NvClass *klass = class_val.u.klass;
    NvValue inst = nv_inst_new(klass);
    NvDict *methods = klass->methods;
    int ctor = methods ? nv_dict_find(methods, "أنشئ") : -1;
    if (ctor >= 0) {
        NvValue ctor_fn = methods->entries[ctor].val;
        if (ctor_fn.tag == NV_FUNC) {
            nv_call_func_impl(ctor_fn.u.func, inst, 1, args, nargs, line, col, 1);
        } else if (ctor_fn.tag == NV_NATIVE) {
            ctor_fn.u.native->fn(args, nargs, line, col);
        }
    } else if (nargs > 0) {
        nv_error_fmt(NV_ERR_RUNTIME, line, col,
                     "الفئة %s ليس لها منشئ أنشئ، لكن أعطيت %d معاملات",
                     klass->name, nargs);
    }
    return inst;
}

// ---------- الفئات والواجهات (مساعدات المولد) ----------
// أنشئ فئة (مع نسخ طرق الأب مسطحة).
NvValue nv_class_begin(const char *name, NvValue base_val, int line, int col) {
    NvClass *base = NULL;
    if (base_val.tag == NV_NIL) {
        base = NULL;
    } else if (base_val.tag == NV_CLASS) {
        base = base_val.u.klass;
    } else {
        nv_error_fmt(NV_ERR_TYPE, line, col, "ليست فئة، بل %s", nv_type_name(base_val));
    }
    return nv_class_new(name, base);
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

// توسيع ~ : ~ هو مجلد البرنامج؛ و~/س تحته.
static char *nv_expand_home(const char *path) {
    if (strcmp(path, "~") == 0) return nv_strdup(nv_program_dir);
    if (strncmp(path, "~/", 2) == 0) {
        size_t n = strlen(nv_program_dir) + strlen(path + 1) + 1;
        char *out = (char *)malloc(n);
        snprintf(out, n, "%s%s", nv_program_dir, path + 1);
        return out;
    }
    return nv_strdup(path);
}

char *nv_display_path(const char *path) {
    if (strcmp(path, nv_program_dir) == 0) return nv_strdup("~");
    size_t bl = strlen(nv_program_dir);
    if (strncmp(path, nv_program_dir, bl) == 0 && path[bl] == '/') {
        size_t n = strlen(path + bl) + 2;
        char *out = (char *)malloc(n);
        snprintf(out, n, "~%s", path + bl);
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
    return nv_strdup(path);
}

char *nv_display_str(const char *s) {
    if (strcmp(s, "~") == 0 || strncmp(s, "~/", 2) == 0) return nv_strdup(s);
    if (s[0] == '/') return nv_display_path(s);
    return nv_strdup(s);
}

// حل مسار ضد مجلد الجلسة (resolve_in) + فحص السياسة.
char *nv_resolve_path(const char *path, int line, int col) {
    char *given = nv_expand_home(path);
    char *joined = NULL;
    if (nv_trusted) {
        if (given[0] == '/') {
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
    if (given[0] == '/') {
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
        char *disp = nv_display_str(path);
        nv_error_fmt(NV_ERR_IO, line, col,
                     "الملف %s خارج مجلد البرنامج، والوصول مقيد بمجلد البرنامج افتراضيا (استخدم --اسمح-بالكل للبرامج الموثوقة)",
                     disp);
    }
    char *canon_cand = nv_canonical_with_suffix(normalized);
    char *canon_base = nv_canonical_with_suffix(base_norm);
    if (!nv_path_starts_with(canon_cand, canon_base)) {
        char *disp = nv_display_str(path);
        nv_error_fmt(NV_ERR_IO, line, col,
                     "الملف %s خارج مجلد البرنامج عبر رابط رمزي، والوصول مقيد بمجلد البرنامج افتراضيا",
                     disp);
    }
    free(canon_cand);
    free(canon_base);
    free(base_norm);
    return normalized;
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
        size_t idx = (size_t)args[0].u.num;
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

// ---------- استدعاء بنقطة (طريقة نص/قائمة أو عضو وحدة/كائن) ----------
// يوزع على نوع الهدف وقت التشغيل؛ والقائمة تُعدَّل في
// مكانها (كائن القائمة مشترك مع متغير المتصل)، فيكتب
// المولد القيمة عائدا إلى المتغير بعد الاستدعاء.
NvValue nv_call_dot_at(NvValue target, const char *name, NvValue *args, int nargs, int line, int col,
                     int callee_line, int callee_col) {
    if (target.tag == NV_STR) return nv_call_str_method(target, name, args, nargs, line, col);
    if (target.tag == NV_LIST) {
        NvValue slot = target;
        return nv_call_list_method(&slot, name, args, nargs, line, col);
    }
    NvValue member = nv_dot(target, name, line, col);
    return nv_call_at(member, args, nargs, line, col, callee_line, callee_col);
}

NvValue nv_call_dot(NvValue target, const char *name, NvValue *args, int nargs, int line, int col) {
    return nv_call_dot_at(target, name, args, nargs, line, col, line, col);
}

// ---------- عدّاد معاملات (للبداية/النهاية) ----------
static size_t nv_count_arg(NvValue v, int line, int col) {
    if (v.tag == NV_NUM && v.u.num >= 0.0 && v.u.num == trunc(v.u.num)) {
        return (size_t)v.u.num;
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

// ---------- الدوال المدمجة: الجدول والتوزيع ----------
int nv_is_builtin(const char *name) {
    static const char *names[] = {
        "طول", "مدى", "مجموع", "أدنى", "أقصى", "مطلق", "عدد", "نص", "نوع",
        "اقرأ_ملف", "اكتب_ملف", "اعرض_الملفات", "المجلد_الحالي", "انتقل",
        "أنشئ_مجلد", "احذف", "احذف_مجلد", "احذف_ملف", "انسخ", "انقل",
        "أنشئ_ملف", "اعرض_البداية", "اعرض_النهاية", "عد_الأسطر", "عد_الكلمات",
        "ابحث_في_ملف", "ابحث_عن", "اعرض_الشجرة", "حجم_المسار", "أنشئ_رابط",
        "التاريخ_الآن", "النسخة", "موجود", "نوع_المسار", "تفاصيل_المسار",
        "تثبيت", "عد_الملفات", "ادخل", NULL
    };
    for (int i = 0; names[i]; i++) {
        if (strcmp(name, names[i]) == 0) return 1;
    }
    return 0;
}

NvValue nv_call_builtin(const char *name, NvValue *args, int nargs, int line, int col) {
    // التسع النقية.
    if (strcmp(name, "طول") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue a = args[0];
        if (a.tag == NV_LIST) return nv_num((double)a.u.list->len);
        if (a.tag == NV_STR) return nv_num((double)a.u.str->chars);
        if (a.tag == NV_DICT) return nv_num((double)a.u.dict->len);
        nv_error_fmt(NV_ERR_TYPE, line, col, "طول يتطلب قائمة أو نصا أو قاموسا، وليس %s", nv_type_name(a));
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
            ints[i] = (long long)args[i].u.num;
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
        if (!nv_is_num_like(args[0])) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "مطلق يتطلب عددا، وليس %s", nv_type_name(args[0]));
        }
        return nv_num(fabs(nv_num_of(args[0])));
    }
    if (strcmp(name, "عدد") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        NvValue a = args[0];
        if (a.tag == NV_NUM) return a;
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
        nv_arity(args, nargs, 0, name, line, col);
        char *disp = nv_display_path(nv_cwd);
        NvValue v = nv_str_take(disp, strlen(disp));
        return v;
    }
    if (strcmp(name, "انتقل") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "انتقل يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        if (stat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المجلد غير موجود: %s", disp);
        }
        if (!S_ISDIR(st.st_mode)) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "ليس مجلدا: %s", disp);
        }
        char *norm = nv_normalize_lexical(resolved);
        free(nv_cwd);
        nv_cwd = norm;
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "أنشئ_مجلد") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "أنشئ_مجلد يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
#ifdef _WIN32
        if (mkdir(resolved) != 0) {
#else
        if (mkdir(resolved, 0777) != 0) {
#endif
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "تعذر إنشاء المجلد %s: %s", disp, osb);
        }
        free(resolved);
        return nv_nil();
    }
    if (strcmp(name, "احذف") == 0) {
        nv_arity(args, nargs, 1, name, line, col);
        if (args[0].tag != NV_STR) {
            nv_error_fmt(NV_ERR_TYPE, line, col, "احذف يتطلب مسارا نصيا، وليس %s", nv_type_name(args[0]));
        }
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
        struct stat st;
        if (stat(resolved, &st) != 0) {
            char *disp = nv_display_str(args[0].u.str->data);
            nv_error_fmt(NV_ERR_IO, line, col, "المسار غير موجود: %s", disp);
        }
        int rc = S_ISDIR(st.st_mode) ? rmdir(resolved) : unlink(resolved);
        if (rc != 0) {
            int e = errno;
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
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
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
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
            char osb[256];
            nv_os_error_str(e, osb, sizeof(osb));
            char *disp = nv_display_str(args[0].u.str->data);
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
        char *resolved = nv_resolve_path(args[0].u.str->data, line, col);
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
            if (stat(src_path, &sst) == 0) src_len = (unsigned long long)sst.st_size;
            if (stat(dst_path, &dst_e) == 0) dst_existing = (unsigned long long)dst_e.st_size;
            unsigned long long extra = src_len > dst_existing ? src_len - dst_existing : 0;
            nv_check_quota((size_t)extra, line, col);
            FILE *in = fopen(src_path, "rb");
            FILE *out = in ? fopen(dst_path, "wb") : NULL;
            if (!in || !out) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                if (in) fclose(in);
                if (out) fclose(out);
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر نسخ %s: %s", disp, osb);
            }
            unsigned char buf[65536];
            size_t n;
            while ((n = fread(buf, 1, sizeof(buf), in)) > 0) fwrite(buf, 1, n, out);
            fclose(in);
            fclose(out);
        } else {
            if (rename(src_path, dst_path) != 0) {
                int e = errno;
                char osb[256];
                nv_os_error_str(e, osb, sizeof(osb));
                char *disp = nv_display_str(args[0].u.str->data);
                nv_error_fmt(NV_ERR_IO, line, col, "تعذر نقل %s: %s", disp, osb);
            }
        }
        free(src_path);
        free(dst_path);
        return nv_nil();
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
    if (strcmp(nv_type_name(v), type_name) != 0) {
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

NvValue nv_class_begin_named(NvScope *scope, const char *name, const char *base_name, int line, int col) {
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
    return nv_class_begin(name, base_val, line, col);
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
                return 1;
            }
            nv_local_hosts = (char **)realloc(nv_local_hosts, (size_t)(nv_n_local_hosts + 1) * sizeof(char *));
            nv_local_hosts[nv_n_local_hosts++] = argv[++i];
        } else if (a[0] == '-') {
            fprintf(stderr, "خيار غير معروف: %s\n", a);
            fputs(nv_standalone_usage, stderr);
            return 1;
        } else {
            fprintf(stderr, "خطأ: البرنامج المستقل لا يأخذ ملف مصدر — شغّله مباشرة\n");
            fputs(nv_standalone_usage, stderr);
            return 1;
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
    nv_cwd = nv_strdup(dir);
    free(dir);
    // النطاق العلوي + إطار الجذر.
    NvScope *top = nv_scope_new(NULL);
    nv_frames_push(top);
    NvCheckpoint cp = nv_checkpoint();
    NvHandler top_h;
    top_h.prev = NULL;
    top_h.has_except = 0;
    nv_handlers = &top_h;
    int code = 0;
    if (setjmp(top_h.jb) == 0) {
        body(top);
    } else {
        nv_restore_checkpoint(cp);
        NvError *e = nv_current_error();
        if (e) {
            nv_print_error(e->kind, e->message, e->line, e->col, e->file_idx);
        }
        code = 1;
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
