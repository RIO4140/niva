package com.rio4140.niva;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.provider.Settings;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Rect;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.os.Bundle;
import android.os.PowerManager;
import android.text.Editable;
import android.text.InputFilter;
import android.text.InputType;
import android.text.Spanned;
import android.text.style.ForegroundColorSpan;
import android.view.ActionMode;
import android.view.Gravity;
import android.view.HapticFeedbackConstants;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewTreeObserver;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.PopupMenu;
import android.widget.TextView;
import android.widget.Toast;

import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * نيفا — ترمنال أصلي.
 *
 * ترمنال حقيقي لا نموذج شات: حقل إدخال واحد يمتد بعرض
 * الترمنال كله، والموجّه (المجلد الحالي + $) والنص الذي
 * يكتبه المستخدم على السطر نفسه، والمؤشر داخل الترمنال،
 * والمخرجات تتدفق فوق سطر الكتابة. لا صندوق إدخال منفصل
 * أسفل الشاشة.
 *
 * الواجهة RTL بالكامل (أمر المالك 2026-11-05 ~22:13):
 * اتجاه التخطيط من اليمين لليسار، واتجاه النص RTL، والموجّه
 * على اليمين، والكتابة تتدفق من اليمين لليسار، والسجل RTL.
 * هذا عرض فقط — ترتيب الكود نفسه لم يتغير.
 *
 * الموجّه (أمرا المالك 2026-11-05 ~22:58 و~23:03): المجلد
 * الحالي بأسلوب الترمنال العادي — `~ $ ` في البيت،
 * `~/اسم المجلد $ ` في المجلدات الفرعية — وموجّه
 * الاستكمال `> `. لا يظهر `نيفا: ` كموجّه أبدا.
 *
 * كل جلسة عملية محرك نيفا مستقلة (stdin/stdout عبر أنابيب)
 * مع سجلها ومحفوظاتها وقرار الشبكة الخاص بها.
 */
public class MainActivity extends Activity {

    private static final String CONT_PROMPT_TEXT = "> ";
    private static final String ENDED_MESSAGE = "انتهت الجلسة.";

    /** سقف السجل (تدقيق م-1): آخر 2000 سطر / 256KB. */
    private static final int MAX_LINES = 2000;
    private static final int MAX_CHARS = 256 * 1024;
    /** سقف سطر الإدخال (تدقيق م-10). */
    private static final int MAX_LINE_CHARS = 64 * 1024;
    /** سقف الجلسات (تدقيق م-8). */
    private static final int MAX_SESSIONS = 8;

    /** أمر التحكم في الشبكة داخل المحرك (تدقيق م-2): يقلب
     * علم الشبكة في العملية نفسها — بلا إعادة تشغيل وبلا
     * فقدان حالة الجلسة. */
    private static final String ALLOW_NETWORK_COMMAND = ":اسمح-بالشبكة";

    /** بادئة إشارات التحكم على stderr (تدقيق م-6): سطر
     * يبدأ بهذا الحرف لا يظهر أبدا للمستخدم، ولا يمكن
     * لبرنامج نيفا طباعته. */
    private static final String CONTROL_PREFIX = "\u0001نيفا-تحكم:";

    /** إعدادات محفوظة (SharedPreferences) + ملف
     * niva.properties في مجلد المحرك (قابل للتحرير من
     * الترمنال نفسه؛ الملف يغلب عند التعارض). */
    private static final String PREFS_NAME = "niva";
    private static final String PREF_FONT_SIZE = "font_size_sp";
    private static final String PREF_KEEP_SCREEN = "keep_screen_on";
    private static final String PREF_ACTIVE_SESSION = "active_session";
    private static final String NIVA_PROPS_FILE = "niva.properties";
    private static final float DEFAULT_FONT_SP = 16f;
    private static final float MIN_FONT_SP = 10f;
    private static final float MAX_FONT_SP = 32f;

    /** سعة طابور الإدخال المحدود إلى المحرك (بند 15):
     * 64 سطرا؛ الزائد يُرفض برسالة ولا يُفقَد بصمت. */
    private static final int IN_QUEUE_CAP = 64;
    /** سقف طابور اللصق/الكتابة (بند 2 + بند 10): 200 سطر. */
    private static final int PASTE_QUEUE_CAP = 200;

    /** سطر الصدق عند استعادة جلسة قديمة (بند 13): السجل
     * يعود للقراءة فقط، والمتغيرات لا تعود أبدا. */
    private static final String RESTORE_NOTE =
            "سجل قديم للقراءة فقط — المتغيرات لا تعود بعد إعادة التشغيل.";
    /** رسالة انقطاع الشبكة (بند 12). */
    private static final String OFFLINE_MESSAGE = "لا يوجد اتصال بالإنترنت";

    /** التحديث الذاتي من جيت هاب (v1.1.4، أمر المالك
     * 2026-11-06 ~00:05): عند الطلب فقط — كتابة العبارة
     * «تحديث نيفا» أو الاختصار «تن» هي الموافقة على
     * فحص واحد؛ بلا فحص تلقائي عند البدء وبلا استطلاع
     * في الخلفية. الفحص HTTPS من التطبيق نفسه (لا
     * المحرك — اجلب HTTP عاديا فقط ولا يصل جيت هاب).
     * الوسم المتوقع v<النسخة> (مثلا v1.1.4)؛ وملف
     * التثبيت أول أصل .apk في الإصدار. */
    private static final String UPDATE_PHRASE = "تحديث نيفا";
    private static final String UPDATE_CODE = "تن";
    private static final String RELEASES_URL =
            "https://api.github.com/repos/RIO4140/niva/releases/latest";
    private static final String UPDATE_APK_NAME = "niva-update.apk";
    /** سقف حجم التحديث: رفض ما زاد عنه (APK نحو 1.2MB). */
    private static final long MAX_UPDATE_BYTES = 50L * 1024 * 1024;
    /** علامات خطأ جلب من المحرك (بند 12، النصف الثاني):
     * إن ظهرت أثناء جلب وكان الجهاز دون اتصال، تُعرض
     * رسالة الانقطاع بدلا من انتظار مهلة DNS. */
    private static final String[] FETCH_ERROR_MARKERS = {
            "تعذر حل اسم المضيف", "تعذر الاتصال بالمضيف",
            "تعذر إرسال الطلب", "تعذر قراءة الرد",
            "تعذر الوصول إلى الرابط"
    };
    /** روابط http(s) — أُعيد تنفيذها من الصفر (بند 7)؛
     * علامات الترقيم اللاحقة تُقصّ عند الفتح. */
    private static final Pattern URL_PATTERN =
            Pattern.compile("https?://[^\\s\"'<>\\[\\]{}|\\\\^`]+");
    private static final String URL_TRAILING = ".,;:!?)]}»”’";

    /** ألوان المظهر الداكن (الافتراضي) والفاتح (بند 9). */
    private static final int DARK_BG = 0xFF0B0B0B;
    private static final int DARK_TEXT = 0xFFE8E8E8;
    private static final int DARK_PANEL = 0xFF1A1A1A;
    private static final int DARK_BTN = 0xFF2A2A2A;
    private static final int DARK_ACTIVE = 0xFF2A5A3A;
    private static final int DARK_PROMPT = 0xFF00E676;
    private static final int LIGHT_BG = 0xFFF5F5F0;
    private static final int LIGHT_TEXT = 0xFF1A1A1A;
    private static final int LIGHT_PANEL = 0xFFE8E8E8;
    private static final int LIGHT_BTN = 0xFFD8D8D8;
    private static final int LIGHT_ACTIVE = 0xFFA8D8B8;
    private static final int LIGHT_PROMPT = 0xFF007A33;
    private static final int ERROR_RED = 0xFFFF5252;
    private static final int SUBTITLE_GRAY_DARK = 0xFF9A9A9A;
    private static final int SUBTITLE_GRAY_LIGHT = 0xFF5A5A5A;

    /** أكواد الاختصارات التي يعالجها التطبيق نفسه (لا المحرك):
     * جلسات / جلسة جديدة / مسح الشاشة / حالة الشبكة /
     * السماح بالشبكة / مدة الجلسة / إعادة تشغيل الجلسة /
     * تحديث نيفا (v1.1.4). */
    private static final String[] APP_CODES = {
            "جل", "جد", "مش", "شب", "سش", "مد", "اع", "تن"
    };

    /** صف المفاتيح (أمر المالك 2026-11-05 ~23:02): الأكواد
     * بالترتيب — 43 للمحرك + 8 للتطبيق. «تن» (تحديث نيفا،
     * v1.1.4) كود تطبيق إضافي خارج الخمسين المشهورة؛
     * ضغطة زر تُدرج الكود متبوعا بمسافة، ولا تنفذ تلقائيا. */
    private static final String[] KEY_CODES = {
            "ات", "مح", "نم", "شم", "حم", "حف", "سم", "قم", "اف", "شف",
            "عب", "عن", "عس", "عك", "بم", "بع", "عش", "حج", "شر", "اك",
            "وج", "ام", "عم", "اس", "عر", "مس", "خج", "اص", "طن", "مع",
            "اد", "اق", "طع", "حع", "مى", "رع", "جذ", "قو", "تا", "وق",
            "نو", "اج", "اط",
            "جل", "جد", "مش", "شب", "سش", "مد", "اع", "تن"
    };

    /** رموز صف المفاتيح الصعبة على لوحة مفاتيح الهاتف. */
    private static final String[] KEY_SYMBOLS = {
            "/", "-", "_", ".", ":", "\"", "'", "(", ")", "[", "]", "|", "$", "~"
    };

    static class Session {
        StringBuilder text = new StringBuilder();
        int inputStart = -1;
        String lastPrompt = "~ $ ";
        Process process;
        OutputStream stdin;
        Thread reader;
        Thread errReader;
        List<String> history = new ArrayList<>();
        int histIdx = -1;
        String draft = "";
        boolean networkAllowed = false;
        boolean networkAsked = false;
        boolean ended = false;
        long startTime = System.currentTimeMillis();
        String pendingLine = null;
        /** السطر معلّق لإعادة إرساله بعد إعادة التشغيل. */
        boolean awaitingRestartResend = false;
        /** إعادة تشغيل جارية (فلا تُطلب ثانية). */
        boolean restarting = false;
        /** حوار شبكة مؤجل حتى تصبح الجلسة نشطة (م-7). */
        boolean networkPending = false;
        /** السطر الجزئي الحالي (بعد آخر \n) — لاكتشاف
         * الموجّه بلا مسح للنص كله (م-1). */
        String tail = "";
        int lineCount = 0;
        /** اسم مخصص من المستخدم (بند 6)؛ null = الاسم
         * الافتراضي «جلسة N». */
        String customName = null;
        /** المجلد الحالي كما يراه الموجّه (بند 6). */
        String currentDir = "~";
        /** انتهت العملية، وبخطأ (خروج غير صفري) أم لا
         * (بند 6 + بند 15). */
        boolean endedWithError = false;
        /** أُبلغ عن نهاية العملية مرة واحدة فقط (بند 15:
         * القارئ + خيط الانتظار يبلغان معا). */
        boolean endNotified = false;
        /** طابور الإدخال المحدود + خيط الكاتب (بند 15). */
        ArrayBlockingQueue<String> inQueue =
                new ArrayBlockingQueue<>(IN_QUEUE_CAP);
        Thread writer;
        /** خيط انتظار موت العملية (بند 15). */
        Thread waiter;
        /** دمج مخرجات القارئ قبل خيط الواجهة (بند 15):
         * قطعة واحدة معلّقة لكل جلسة كحد أقصى. */
        final StringBuilder outBuf = new StringBuilder();
        boolean outPending = false;
        final Object outLock = new Object();
        /** طابور اللصق/الكتابة: سطر لكل موجّه (بند 2 + 10). */
        final ArrayDeque<String> pasteQueue = new ArrayDeque<>();
        /** جلب شبكة جارٍ في هذه الجلسة (بند 11 + 12). */
        boolean fetchWakeHeld = false;
        /** خطأ جلب ظهر في المخرجات منذ آخر إرسال (بند 12). */
        boolean lastFetchError = false;
        /** جيل العملية (بند 15): مخرجات عملية ميتة لا
         * تُلحق بجلسة أعيد تشغيلها. */
        int procGen = 0;
    }

    /** حقل الترمنال: السجل كله + سطر الإدخال الحالي في نص
     * واحد. الكتابة مسموحة فقط بعد inputStart (بعد الموجّه)؛
     * كل ما قبله محمي من التعديل والمسح. */
    static class TerminalEditText extends EditText {
        int inputStart = -1;
        boolean programmatic = false;
        /** وضع التحديد (بند 1): المؤشر حر في السجل،
         * ولا يُقيَّد بسطر الإدخال. */
        boolean selecting = false;
        /** يُستدعى عند ضغطة إنتر (من أي مصدر). */
        Runnable onEnterSubmit;

        public TerminalEditText(Context ctx) {
            super(ctx);
            setFilters(new InputFilter[]{new InputFilter() {
                @Override
                public CharSequence filter(CharSequence source, int start, int end,
                                           Spanned dest, int dstart, int dend) {
                    if (programmatic) return null;
                    if (inputStart < 0) return "";
                    if (dstart < inputStart) return "";
                    // سقف سطر الإدخال (م-10): يُفحص عند اللصق
                    // فقط (إدخال كبير دفعة واحدة) لا عند كل حرف.
                    if (end - start > 1024) {
                        int lineLen = dest.length() - inputStart
                                + (end - start) - (dend - dstart);
                        if (lineLen > MAX_LINE_CHARS) return "";
                    }
                    return null;
                }
            }});
        }

        @Override
        public void setSelection(int start, int end) {
            // إصلاح عطل مسح v1.1.2 (بند 0): لوحة المفاتيح
            // قد تطلب تحديدا بموضع قديم بعد تغيّر النص
            // (مثلا بعد `مش`) — التحديد غير المقيّد يرمي
            // IndexOutOfBoundsException فيقتل التطبيق كله.
            // القصّ هنا يحمي كل مسارات التحديد، ومنها
            // مسارات لوحة المفاتيح نفسها.
            Editable text = getText();
            int len = text == null ? 0 : text.length();
            int s = Math.max(0, Math.min(start, len));
            int e = Math.max(0, Math.min(end, len));
            super.setSelection(s, e);
        }

        @Override
        protected void onSelectionChanged(int selStart, int selEnd) {
            super.onSelectionChanged(selStart, selEnd);
            if (programmatic) return;
            // في وضع التحديد المؤشر حر في السجل كله.
            if (selecting) return;
            int len = getText() == null ? 0 : getText().length();
            int min = inputStart >= 0 ? Math.min(inputStart, len) : len;
            if (selStart < min || selEnd < min) {
                setSelection(min, min);
            }
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            InputConnection ic = super.onCreateInputConnection(outAttrs);
            if (ic == null) return null;
            // إصلاح إنتر (شكوى المالك 2026-11-05 ~22:56): مع
            // حقل متعدد الأسطر تُدخل لوحات مفاتيح الهواتف
            // سطرا جديدا عبر commitText بدلا من زر الإرسال،
            // فيتجاوز OnKeyListener وOnEditorActionListener
            // معا. الاعتراض هنا — على مستوى الاتصال نفسه —
            // يلتقط إنتر من كل لوحة مفاتيح، فلا يُدرج سطر
            // جديد أبدا، بل يُرسَل السطر الحالي دائما.
            return new InputConnectionWrapper(ic, true) {
                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    if (text != null) {
                        String t = text.toString();
                        int nl = t.indexOf('\n');
                        if (nl >= 0) {
                            String before = t.substring(0, nl);
                            if (!before.isEmpty()) {
                                super.commitText(before, newCursorPosition);
                            }
                            if (onEnterSubmit != null) onEnterSubmit.run();
                            return true;
                        }
                    }
                    return super.commitText(text, newCursorPosition);
                }

                @Override
                public boolean setComposingText(CharSequence text, int newCursorPosition) {
                    if (text != null && text.toString().contains("\n")) {
                        String clean = text.toString().replace("\n", "");
                        return super.setComposingText(clean, newCursorPosition);
                    }
                    return super.setComposingText(text, newCursorPosition);
                }

                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    if (event.getAction() == KeyEvent.ACTION_DOWN
                            && event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                        if (onEnterSubmit != null) onEnterSubmit.run();
                        return true;
                    }
                    return super.sendKeyEvent(event);
                }
            };
        }
    }

    private TerminalEditText terminalView;
    private TextView netStateView;
    /** درج الجلسات: لوحة جانبية بسيطة (لا DrawerLayout —
     * لا اعتماديات خارجية)، تظهر وتختفي فقط. */
    private View drawerPanel;
    private View scrim;
    private LinearLayout sessionList;
    private final List<Session> sessions = new ArrayList<>();
    private int activeIdx = 0;

    /** صف المفاتيح فوق لوحة المفاتيح (أمر المالك
     * 2026-11-05 ~23:02): يظهر مع الكيبورد ويختفي معه. */
    private HorizontalScrollView keyRow;
    /** زر «بديل» كمعدِّل للسهم التالي (ALT + سهم). */
    private Button altButton;
    private boolean altNext = false;
    /** موجّه الجلسة النشطة الحالي (يُعاد رسمه عند تغير
     * موضعه فقط — بلا مسح كل الـspans مع كل قطعة، م-1). */
    private ForegroundColorSpan currentPromptSpan = null;
    private int currentPromptStart = -1;
    /** لون الموجّه الحالي (يتغير مع المظهر — بند 9). */
    private int promptColor = DARK_PROMPT;

    /** الإعدادات (بند 3 + 4 + 9). */
    private SharedPreferences prefs;
    private float fontSizeSp = DEFAULT_FONT_SP;
    private boolean keepScreenOn = false;
    private boolean lightTheme = false;
    private boolean showKeyRowPref = true;
    /** ألوان المظهر الحالي (تُضبط قبل بناء الواجهة). */
    private int colorBg = DARK_BG;
    private int colorText = DARK_TEXT;
    private int colorPanel = DARK_PANEL;
    private int colorBtn = DARK_BTN;
    private int colorActive = DARK_ACTIVE;
    private int colorSubtitle = SUBTITLE_GRAY_DARK;
    private LinearLayout rootLayout;
    private TextView titleView;

    /** التمرير التلقائي (بند 8): true ما دام المستخدم
     * في أسفل السجل؛ قراءة التاريخ توقفه. */
    private boolean autoScroll = true;
    private Button scrollButton;

    /** كشف القرص لتغيير حجم الخط (بند 3). */
    private ScaleGestureDetector scaleDetector;

    /** قفل الاستيقاظ أثناء الجلب (بند 11). */
    private PowerManager.WakeLock fetchWakeLock;
    private WifiManager.WifiLock fetchWifiLock;
    private ConnectivityManager connectivityManager;

    /** استقبال أمر الخروج من إشعار الخدمة (بند 13). */
    private BroadcastReceiver exitReceiver;

    /** لوحة كتابة البرنامج (بند 10). */
    private View composePanel;
    private EditText composeEdit;

    private void openDrawer() {
        // الدرج مقفل أثناء التحديد (بند 1، كما في Termux:
        // لا يفتح الدرج ومقابض التحديد ظاهرة).
        if (terminalView != null && terminalView.selecting) return;
        scrim.setVisibility(View.VISIBLE);
        drawerPanel.setVisibility(View.VISIBLE);
    }

    private void closeDrawer() {
        scrim.setVisibility(View.GONE);
        drawerPanel.setVisibility(View.GONE);
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE);
        connectivityManager =
                (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);
        loadSettings();

        LinearLayout root = new LinearLayout(this);
        rootLayout = root;
        root.setOrientation(LinearLayout.VERTICAL);
        root.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        root.setBackgroundColor(colorBg);

        // الشريط العلوي البسيط: الجلسات + الاسم + حالة الشبكة.
        LinearLayout topBar = new LinearLayout(this);
        topBar.setOrientation(LinearLayout.HORIZONTAL);
        topBar.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        topBar.setPadding(16, 16, 16, 16);
        Button sessionsBtn = new Button(this);
        sessionsBtn.setText("الجلسات");
        sessionsBtn.setTextColor(colorText);
        sessionsBtn.setBackgroundColor(colorBtn);
        sessionsBtn.setOnClickListener(v -> openDrawer());
        topBar.addView(sessionsBtn);
        TextView title = new TextView(this);
        titleView = title;
        title.setText("نيفا — وحدة التحكم");
        title.setTextColor(colorText);
        title.setTextSize(18);
        title.setPadding(24, 0, 24, 0);
        // الضغطة الطويلة على العنوان تفتح قائمة الإعدادات
        // (بند 4 + 5 + 7 + 9 + 10): إبقاء الشاشة، المظهر،
        // صف المفاتيح، حجم الخط، مشاركة السجل، الروابط،
        // كتابة برنامج — بلا شاشة إعدادات كبيرة.
        title.setOnLongClickListener(v -> {
            showTopMenu();
            return true;
        });
        LinearLayout.LayoutParams titleParams = new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        topBar.addView(title, titleParams);
        netStateView = new TextView(this);
        netStateView.setText("الشبكة: مغلقة");
        netStateView.setTextColor(colorText);
        topBar.addView(netStateView);
        root.addView(topBar, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // الترمنال نفسه: حقل واحد يملأ الشاشة.
        terminalView = new TerminalEditText(this);
        terminalView.setBackgroundColor(colorBg);
        terminalView.setTextColor(colorText);
        terminalView.setTextSize(fontSizeSp);
        terminalView.setTextDirection(View.TEXT_DIRECTION_RTL);
        terminalView.setGravity(Gravity.TOP | Gravity.START);
        // MULTI_LINE هو إصلاح v1.1.1 لخطأ السطر الواحد
        // (رأى المالك الأوامر متراكمة على سطر واحد): بدونه
        // يطوي EditText كل \n على سطر بصري واحد. معه يبدأ
        // كل سطر جديد سطرا خاصا به، مثل Termux. وإنتر نفسه
        // يُعترض في onCreateInputConnection (أعلاه) فلا
        // يُدرج سطرا جديدا أبدا.
        terminalView.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        terminalView.setImeOptions(EditorInfo.IME_ACTION_SEND);
        terminalView.setPadding(16, 8, 16, 16);
        terminalView.setHorizontallyScrolling(false);
        terminalView.onEnterSubmit = this::submitLine;
        // النقر في أي مكان يضع المؤشر في نهاية سطر الكتابة
        // ويُظهر لوحة المفاتيح (أمر المالك 2026-11-05
        // ~23:02: اللمس في منتصف الشاشة أيضا). النقرة على
        // رابط في السجل تفتحه في المتصفح (بند 7) بدلا من
        // تحريك المؤشر؛ والقرص يغيّر حجم الخط (بند 3).
        // يُتخذ القرار عند رفع الإصبع (نقرة لا سحب)، فلا
        // يتعارض مع التحديد بالضغطة الطويلة.
        final float[] downXY = new float[2];
        final long[] downTime = new long[1];
        terminalView.setOnTouchListener((v, event) -> {
            if (scaleDetector != null) {
                scaleDetector.onTouchEvent(event);
                if (scaleDetector.isInProgress()) return true;
            }
            int action = event.getAction();
            if (action == MotionEvent.ACTION_DOWN) {
                downXY[0] = event.getX();
                downXY[1] = event.getY();
                downTime[0] = event.getEventTime();
                terminalView.requestFocus();
            } else if (action == MotionEvent.ACTION_UP) {
                float dx = event.getX() - downXY[0];
                float dy = event.getY() - downXY[1];
                long dt = event.getEventTime() - downTime[0];
                boolean tap = dx * dx + dy * dy < 144 && dt < 500;
                if (tap) {
                    if (openUrlAtTap(event.getX(), event.getY())) {
                        return true;
                    }
                    int end = terminalView.getText().length();
                    terminalView.setSelection(end, end);
                    showKeyboard();
                }
            }
            return false;
        });
        // شريط إجراءات التحديد العربي (بند 1): الضغطة
        // الطويلة تدخل وضع التحديد بمقابض أندرويد الأصلية؛
        // القائمة عربية كاملة (نسخ، لصق، مشاركة، تحديد
        // الكل)، والتحديد يعبر كل أسطر السجل ويحفظ فواصل
        // الأسطر عند النسخ. نقرة واحدة أو زر الرجوع يخرجان
        // من الوضع ويعيدان المؤشر إلى سطر الإدخال.
        terminalView.setCustomSelectionActionModeCallback(
                new ActionMode.Callback() {
                    @Override
                    public boolean onCreateActionMode(ActionMode mode, Menu menu) {
                        terminalView.selecting = true;
                        terminalView.performHapticFeedback(
                                HapticFeedbackConstants.LONG_PRESS);
                        closeDrawer();
                        menu.clear();
                        menu.add(Menu.NONE, 1, Menu.NONE, "نسخ");
                        ClipboardManager cm = (ClipboardManager)
                                getSystemService(Context.CLIPBOARD_SERVICE);
                        if (cm != null && cm.hasPrimaryClip()) {
                            menu.add(Menu.NONE, 2, Menu.NONE, "لصق");
                        }
                        menu.add(Menu.NONE, 3, Menu.NONE, "مشاركة");
                        menu.add(Menu.NONE, 4, Menu.NONE, "تحديد الكل");
                        return true;
                    }

                    @Override
                    public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
                        return false;
                    }

                    @Override
                    public boolean onActionItemClicked(ActionMode mode, MenuItem item) {
                        int id = item.getItemId();
                        if (id == 1) {
                            copySelection();
                            mode.finish();
                            return true;
                        }
                        if (id == 2) {
                            mode.finish();
                            pasteFromClipboard();
                            return true;
                        }
                        if (id == 3) {
                            shareSelection();
                            mode.finish();
                            return true;
                        }
                        if (id == 4) {
                            terminalView.selectAll();
                            return true;
                        }
                        return false;
                    }

                    @Override
                    public void onDestroyActionMode(ActionMode mode) {
                        terminalView.selecting = false;
                        moveCursorToEnd();
                    }
                });
        // كشف القرص (بند 3) + إيقاف التمرير التلقائي عند
        // قراءة التاريخ (بند 8).
        scaleDetector = new ScaleGestureDetector(this,
                new ScaleGestureDetector.SimpleOnScaleGestureListener() {
                    @Override
                    public boolean onScale(ScaleGestureDetector detector) {
                        setFontSize(fontSizeSp * detector.getScaleFactor());
                        return true;
                    }
                });
        terminalView.setOnScrollChangeListener((v, sx, sy, ox, oy) -> {
            if (terminalView.getLayout() == null) return;
            int visible = terminalView.getHeight()
                    - terminalView.getPaddingTop()
                    - terminalView.getPaddingBottom();
            int maxScroll = Math.max(0,
                    terminalView.getLayout().getHeight() - visible);
            boolean atBottom = sy >= maxScroll - 4;
            if (atBottom != autoScroll) {
                autoScroll = atBottom;
                updateScrollHighlight();
                if (atBottom) moveCursorToEnd();
            }
        });
        terminalView.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEND) {
                submitLine();
                return true;
            }
            return false;
        });
        terminalView.setOnKeyListener((v, keyCode, event) -> {
            if (event.getAction() != KeyEvent.ACTION_DOWN) return false;
            // لوحة مفاتيح مادية: تحكم+بديل+ (+/−) يغيّر حجم
            // الخط (بند 3).
            if (event.isCtrlPressed() && event.isAltPressed()) {
                if (keyCode == KeyEvent.KEYCODE_EQUALS
                        || keyCode == KeyEvent.KEYCODE_NUMPAD_ADD) {
                    setFontSize(fontSizeSp + 2);
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_MINUS
                        || keyCode == KeyEvent.KEYCODE_NUMPAD_SUBTRACT) {
                    setFontSize(fontSizeSp - 2);
                    return true;
                }
            }
            if (keyCode == KeyEvent.KEYCODE_ENTER) {
                submitLine();
                return true;
            }
            if (keyCode == KeyEvent.KEYCODE_DPAD_UP) {
                arrowUp();
                return true;
            }
            if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN) {
                arrowDown();
                return true;
            }
            if (keyCode == KeyEvent.KEYCODE_DPAD_LEFT) {
                arrowLeft();
                return true;
            }
            if (keyCode == KeyEvent.KEYCODE_DPAD_RIGHT) {
                arrowRight();
                return true;
            }
            return false;
        });
        root.addView(terminalView, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        // صف المفاتيح (أمر المالك 2026-11-05 ~23:02): صف
        // واحد قابل للتمرير أفقيا، فوق لوحة المفاتيح، يظهر
        // معها ويختفي معها. لا شاشات إضافية.
        keyRow = buildKeyRow();
        keyRow.setVisibility(View.GONE);
        root.addView(keyRow, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // درج الجلسات (من جهة البداية: اليمين في RTL):
        // لوحة فوق المحتوى + ستارة للنقر خارجها.
        sessionList = new LinearLayout(this);
        sessionList.setOrientation(LinearLayout.VERTICAL);
        sessionList.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        sessionList.setBackgroundColor(colorPanel);
        sessionList.setPadding(16, 16, 16, 16);
        TextView drawerTitle = new TextView(this);
        drawerTitle.setText("الجلسات");
        drawerTitle.setTextColor(colorText);
        drawerTitle.setTextSize(18);
        sessionList.addView(drawerTitle);
        LinearLayout drawerButtons = new LinearLayout(this);
        drawerButtons.setOrientation(LinearLayout.HORIZONTAL);
        drawerButtons.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        Button addBtn = new Button(this);
        addBtn.setText("+");
        addBtn.setTextColor(colorText);
        addBtn.setBackgroundColor(colorBtn);
        addBtn.setOnClickListener(v -> {
            if (createSession()) {
                closeDrawer();
            } else {
                Toast.makeText(this, "وصلت إلى الحد الأقصى (٨ جلسات)", Toast.LENGTH_SHORT).show();
            }
        });
        drawerButtons.addView(addBtn);
        Button kbdBtn = new Button(this);
        kbdBtn.setText("⌨");
        kbdBtn.setTextColor(colorText);
        kbdBtn.setBackgroundColor(colorBtn);
        kbdBtn.setOnClickListener(v -> toggleKeyboard());
        drawerButtons.addView(kbdBtn);
        sessionList.addView(drawerButtons);

        FrameLayout frame = new FrameLayout(this);
        frame.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        frame.addView(root, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        scrim = new View(this);
        scrim.setBackgroundColor(Color.parseColor("#80000000"));
        scrim.setVisibility(View.GONE);
        scrim.setOnClickListener(v -> closeDrawer());
        frame.addView(scrim, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        drawerPanel = sessionList;
        FrameLayout.LayoutParams drawerParams = new FrameLayout.LayoutParams(
                (int) (getResources().getDisplayMetrics().widthPixels * 0.7),
                ViewGroup.LayoutParams.MATCH_PARENT);
        drawerParams.gravity = Gravity.START;
        drawerPanel.setVisibility(View.GONE);
        frame.addView(drawerPanel, drawerParams);
        setContentView(frame);

        // صف المفاتيح يتبع لوحة المفاتيح: يظهر معها
        // ويختفي معها — إلا إن أخفاه المستخدم من الإعدادات
        // (بند 9).
        frame.getViewTreeObserver().addOnGlobalLayoutListener(
                new ViewTreeObserver.OnGlobalLayoutListener() {
                    @Override
                    public void onGlobalLayout() {
                        int want = (showKeyRowPref && isKeyboardVisible())
                                ? View.VISIBLE : View.GONE;
                        if (keyRow.getVisibility() != want) {
                            keyRow.setVisibility(want);
                        }
                    }
                });

        // اللمس في أي موضع من منطقة الترمنال (حتى الفراغ
        // تحت السجل) يركّز الإدخال ويُظهر الكيبورد.
        frame.setOnTouchListener((v, event) -> {
            if (event.getAction() == MotionEvent.ACTION_DOWN
                    && drawerPanel.getVisibility() != View.VISIBLE) {
                terminalView.requestFocus();
                showKeyboard();
            }
            return false;
        });

        // لوحة كتابة البرنامج (بند 10): صفحة ثانية لكتابة
        // برنامج نيفا طويل ثم إرساله دفعة واحدة (عبر طابور
        // اللصق نفسه). مخفية حتى تُطلب من قائمة العنوان.
        buildComposePanel(frame);

        if (!restoreSessions()) {
            createSession();
        }
        terminalView.requestFocus();
        showKeyboard();

        // الخدمة الأمامية (بند 13): إشعار عربي بعدد
        // الجلسات، يبقى ما دام التطبيق حيا.
        startService(new Intent(this, NivaService.class));
        refreshServiceNotification();
        exitReceiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                if (NivaService.ACTION_EXIT.equals(intent.getAction())) {
                    finish();
                }
            }
        };
        // أندرويد 14 (targetSdk 34) يلزم تحديد عدم تصدير
        // المستقبِل الديناميكي؛ قبله التسجيل العادي.
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(exitReceiver,
                    new IntentFilter(NivaService.ACTION_EXIT),
                    Context.RECEIVER_NOT_EXPORTED);
        } else {
            registerReceiver(exitReceiver,
                    new IntentFilter(NivaService.ACTION_EXIT));
        }
    }

    @Override
    protected void onPause() {
        // القفل مقصور على الجلب نفسه (بند 11): يُحرَّر
        // فقط إن لم يعد أي جلب جاريا — جلب بدأ قبل
        // المغادرة يكمل بقفله حتى وصول الموجّه.
        releaseFetchWakeLockIfIdle();
        saveAllSessions();
        super.onPause();
    }

    /** زر الرجوع (شكوى المالك 2026-11-05 ~22:56): ضغطة
     * واحدة لا تُنهي النشاط أبدا ولا تقتل الجلسات. الدرج
     * المفتوح يُغلق أولا، ثم الكيبورد تُخفى، ثم يُنقَل
     * التطبيق إلى الخلفية وتبقى الجلسات حية. وضع التحديد
     * يخرج أولا (بند 1) قبل كل ذلك. */
    @Override
    public void onBackPressed() {
        if (terminalView != null && terminalView.selecting) {
            int end = terminalView.getText().length();
            terminalView.setSelection(end, end);
            return;
        }
        if (drawerPanel.getVisibility() == View.VISIBLE) {
            closeDrawer();
            return;
        }
        if (isKeyboardVisible()) {
            hideKeyboard();
            return;
        }
        moveTaskToBack(true);
    }

    private boolean isKeyboardVisible() {
        Rect r = new Rect();
        terminalView.getWindowVisibleDisplayFrame(r);
        int screenHeight = terminalView.getRootView().getHeight();
        if (screenHeight <= 0) return false;
        return screenHeight - r.bottom > screenHeight * 0.15;
    }

    private Session active() {
        return sessions.get(activeIdx);
    }

    private void showKeyboard() {
        terminalView.requestFocus();
        InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm == null) return;
        // طلب صريح بعد التركيز (لا ضمني): الطلب الضمني
        // تتجاهله لوحة المفاتيح بعد إخفاء صريح سابق — وهذا
        // ما رآه المالك حين لمس منتصف الشاشة. والبديل
        // القسري احتياط إن رفض الطلب الصريح.
        terminalView.post(() -> {
            if (!imm.showSoftInput(terminalView, 0)) {
                imm.toggleSoftInput(InputMethodManager.SHOW_FORCED, 0);
            }
        });
    }

    private void hideKeyboard() {
        InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm != null) {
            imm.hideSoftInputFromWindow(terminalView.getWindowToken(), 0);
        }
    }

    private void toggleKeyboard() {
        InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm == null) return;
        if (isKeyboardVisible()) {
            hideKeyboard();
        } else {
            showKeyboard();
        }
    }

    // ---------- صف المفاتيح ----------

    private HorizontalScrollView buildKeyRow() {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        row.setBackgroundColor(colorPanel);
        row.setPadding(4, 4, 4, 4);

        // مفاتيح التحكم والأسهم (من صورة Termux، معرّبة —
        // لا كلمات لاتينية في الواجهة).
        row.addView(keyButton("إلغاء", v -> onEsc()));
        row.addView(keyButton("إزاحة", v -> insertAtCursor("    ")));
        row.addView(keyButton("تحكم", v -> onCtrl()));
        row.addView(keyButton("لصق", v -> pasteFromClipboard()));
        altButton = keyButton("بديل", v -> toggleAlt());
        row.addView(altButton);
        row.addView(keyButton("↑", v -> arrowUp()));
        row.addView(keyButton("↓", v -> arrowDown()));
        row.addView(keyButton("←", v -> arrowLeft()));
        row.addView(keyButton("→", v -> arrowRight()));
        row.addView(keyButton("بداية", v -> moveCursorToLineStart()));
        row.addView(keyButton("نهاية", v -> moveCursorToLineEnd()));
        row.addView(keyButton("صفحة↑", v -> pageScroll(-1)));
        row.addView(keyButton("صفحة↓", v -> pageScroll(1)));
        // التمرير التلقائي (بند 8): مضاء وهو يعمل، مطفأ
        // وهو متوقف عند قراءة التاريخ.
        scrollButton = keyButton("تمرير", v -> toggleAutoScroll());
        row.addView(scrollButton);
        updateScrollHighlight();

        // الرموز الصعبة على لوحة مفاتيح الهاتف.
        for (String sym : KEY_SYMBOLS) {
            row.addView(keyButton(sym, v -> insertAtCursor(sym)));
        }

        // الاختصارات العربية: تُدرج الكود متبوعا
        // بمسافة، ليكتب المستخدم المعاملات ثم إنتر — ولا
        // تنفذ تلقائيا.
        for (String code : KEY_CODES) {
            row.addView(keyButton(code, v -> insertAtCursor(code + " ")));
        }

        HorizontalScrollView scroll = new HorizontalScrollView(this);
        scroll.setHorizontalScrollBarEnabled(false);
        scroll.addView(row, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return scroll;
    }

    private Button keyButton(String label, View.OnClickListener action) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextColor(colorText);
        b.setBackgroundColor(colorBtn);
        b.setPadding(20, 8, 20, 8);
        // الزر لا يسرق التركيز من الترمنال — الكيبورد تبقى.
        b.setFocusable(false);
        b.setFocusableInTouchMode(false);
        b.setOnClickListener(v -> {
            terminalView.requestFocus();
            action.onClick(v);
        });
        return b;
    }

    private void insertAtCursor(String text) {
        if (terminalView.inputStart < 0) return;
        int pos = terminalView.getSelectionStart();
        int len = terminalView.getText().length();
        if (pos < terminalView.inputStart || pos > len) pos = len;
        terminalView.programmatic = true;
        terminalView.getText().insert(pos, text);
        terminalView.programmatic = false;
        terminalView.setSelection(Math.min(len + text.length(), terminalView.getText().length()));
    }

    private void moveCursor(int delta) {
        if (terminalView.inputStart < 0) return;
        int pos = terminalView.getSelectionStart();
        int len = terminalView.getText().length();
        if (pos < terminalView.inputStart || pos > len) pos = len;
        int next = Math.max(terminalView.inputStart, Math.min(len, pos + delta));
        terminalView.setSelection(next, next);
    }

    private void moveCursorToLineStart() {
        if (terminalView.inputStart < 0) return;
        terminalView.setSelection(terminalView.inputStart, terminalView.inputStart);
    }

    private void moveCursorToLineEnd() {
        int end = terminalView.getText().length();
        terminalView.setSelection(end, end);
    }

    private void pageScroll(int direction) {
        terminalView.scrollBy(0, direction * terminalView.getHeight() * 4 / 5);
    }

    private void toggleAlt() {
        altNext = !altNext;
        updateAltHighlight();
    }

    private void updateAltHighlight() {
        if (altButton == null) return;
        altButton.setBackgroundColor(altNext ? colorActive : colorBtn);
    }

    private void clearAlt() {
        if (altNext) {
            altNext = false;
            updateAltHighlight();
        }
    }

    /** إلغاء: في كتلة معلّقة تُلغى الكتلة في المحرك بسطر
     * فارغ؛ وإلا يُمسح سطر الإدخال الحالي فقط. */
    private void onEsc() {
        Session s = active();
        if (CONT_PROMPT_TEXT.equals(s.lastPrompt) && s.process != null && !s.ended) {
            if (terminalView.inputStart >= 0) {
                terminalView.inputStart = -1;
                s.inputStart = -1;
                appendSessionText(s, "\n");
                appendToTerminal("\n");
            }
            sendToEngine(s, "");
        } else {
            setCurrentLine("");
        }
        clearAlt();
    }

    /** تحكم: أثناء تشغيل برنامج يقاطعه (يقتل المحرك ويعيد
     * تشغيله — والجلسة تبدأ من جديد باللافتة)؛ وإلا يمسح
     * سطر الإدخال الحالي. */
    private void onCtrl() {
        Session s = active();
        boolean busy = terminalView.inputStart < 0
                && s.process != null && !s.ended && !s.restarting;
        if (busy) {
            s.pendingLine = null;
            s.awaitingRestartResend = false;
            restartProcess(s);
        } else {
            setCurrentLine("");
        }
        clearAlt();
    }

    private void arrowUp() {
        if (altNext) {
            clearAlt();
            historyJump(true);
            return;
        }
        historyNav(-1);
    }

    private void arrowDown() {
        if (altNext) {
            clearAlt();
            historyJump(false);
            return;
        }
        historyNav(1);
    }

    private void arrowLeft() {
        if (altNext) {
            clearAlt();
            moveCursorToLineStart();
            return;
        }
        moveCursor(-1);
    }

    private void arrowRight() {
        if (altNext) {
            clearAlt();
            moveCursorToLineEnd();
            return;
        }
        moveCursor(1);
    }

    /** قفز المحفوظات: الأقدم (ALT+↑) أو المسودة (ALT+↓). */
    private void historyJump(boolean oldest) {
        Session s = active();
        if (s.history.isEmpty() || terminalView.inputStart < 0) return;
        if (oldest) {
            if (s.histIdx == -1) s.draft = currentLine();
            s.histIdx = 0;
            setCurrentLine(s.history.get(0));
        } else {
            s.histIdx = -1;
            setCurrentLine(s.draft);
        }
    }

    // ---------- الجلسات ----------

    private boolean createSession() {
        if (sessions.size() >= MAX_SESSIONS) {
            return false;
        }
        Session s = new Session();
        sessions.add(s);
        startProcess(s);
        rebuildSessionList();
        switchTo(sessions.size() - 1);
        refreshServiceNotification();
        return true;
    }

    private void switchTo(int idx) {
        saveTerminalToSession();
        activeIdx = idx;
        syncTerminalFromSession();
        updateNetState();
        rebuildSessionList();
        Session s = active();
        if (s.networkPending) {
            s.networkPending = false;
            showNetworkDialog(s);
        }
        terminalView.requestFocus();
    }

    private void saveTerminalToSession() {
        if (sessions.isEmpty()) return;
        Session s = active();
        s.text = new StringBuilder(terminalView.getText().toString());
        s.inputStart = terminalView.inputStart;
        recountSession(s);
    }

    private void syncTerminalFromSession() {
        Session s = active();
        terminalView.programmatic = true;
        terminalView.setText(s.text.toString());
        terminalView.inputStart = s.inputStart;
        terminalView.programmatic = false;
        currentPromptSpan = null;
        currentPromptStart = -1;
        paintPrompt();
        int end = terminalView.getText().length();
        terminalView.setSelection(end, end);
    }

    private void rebuildSessionList() {
        // أبقِ العنوان وصف الأزرار، وأعد بناء صفوف الجلسات.
        while (sessionList.getChildCount() > 2) {
            sessionList.removeViewAt(2);
        }
        for (int i = 0; i < sessions.size(); i++) {
            final int idx = i;
            final Session s = sessions.get(i);
            LinearLayout row = new LinearLayout(this);
            row.setOrientation(LinearLayout.VERTICAL);
            row.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
            row.setBackgroundColor(i == activeIdx ? colorActive : colorPanel);
            row.setPadding(24, 12, 24, 12);
            TextView name = new TextView(this);
            String title = s.customName != null ? s.customName
                    : "جلسة " + arabicDigits(i + 1);
            name.setText(title);
            name.setTextColor(s.endedWithError ? ERROR_RED : colorText);
            name.setTextSize(16);
            if (s.ended) {
                name.setPaintFlags(name.getPaintFlags()
                        | Paint.STRIKE_THRU_TEXT_FLAG);
            }
            row.addView(name);
            TextView sub = new TextView(this);
            sub.setText(sessionSubtitle(s));
            sub.setTextColor(colorSubtitle);
            sub.setTextSize(13);
            row.addView(sub);
            row.setOnClickListener(v -> {
                switchTo(idx);
                closeDrawer();
            });
            // الضغطة الطويلة على الصف: تسمية أو إنهاء
            // (بند 6).
            row.setOnLongClickListener(v -> {
                showSessionRowMenu(idx);
                return true;
            });
            sessionList.addView(row);
        }
    }

    /** سطر العنوان تحت اسم الجلسة (بند 6): المجلد الحالي،
     * وآخر أمر بعده إن وُجد؛ والمنتهية تُعلن انتهاءها. */
    private String sessionSubtitle(Session s) {
        if (s.ended) return "انتهت الجلسة.";
        String dir = s.currentDir;
        if (s.history.isEmpty()) return dir;
        String last = s.history.get(s.history.size() - 1);
        String line = dir + " • " + last;
        return line.length() > 40 ? line.substring(0, 40) + "…" : line;
    }

    /** قائمة صف الجلسة (بند 6): تسمية / إنهاء الجلسة. */
    private void showSessionRowMenu(final int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        new AlertDialog.Builder(this)
                .setItems(new CharSequence[]{"تسمية", "إنهاء الجلسة"},
                        (d, which) -> {
                            if (which == 0) showRenameDialog(idx);
                            else showCloseSessionDialog(idx);
                        })
                .show();
    }

    private void showRenameDialog(final int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        final Session s = sessions.get(idx);
        final EditText input = new EditText(this);
        input.setText(s.customName != null ? s.customName
                : "جلسة " + arabicDigits(idx + 1));
        input.setTextDirection(View.TEXT_DIRECTION_RTL);
        input.selectAll();
        new AlertDialog.Builder(this)
                .setTitle("تسمية الجلسة")
                .setView(input)
                .setPositiveButton("حفظ", (d, w) -> {
                    String name = input.getText().toString().trim();
                    s.customName = name.isEmpty() ? null : name;
                    rebuildSessionList();
                })
                .setNegativeButton("إلغاء", null)
                .show();
    }

    private void showCloseSessionDialog(final int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        new AlertDialog.Builder(this)
                .setMessage("أنهي الجلسة؟")
                .setPositiveButton("إنهاء", (d, w) -> closeSession(idx))
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** إنهاء جلسة (بند 6): تُقتل عمليتها وتُزال؛ إن كانت
     * الأخيرة أُنشئت جلسة جديدة بدلا منها، فلا يبقى
     * التطبيق بلا جلسة أبدا. */
    private void closeSession(int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        Session s = sessions.get(idx);
        if (s.process != null) {
            try {
                s.process.destroy();
            } catch (Exception ignored) {
            }
        }
        if (s.writer != null) s.writer.interrupt();
        if (s.waiter != null) s.waiter.interrupt();
        s.fetchWakeHeld = false;
        releaseFetchWakeLockIfIdle();
        boolean wasActive = idx == activeIdx;
        sessions.remove(idx);
        if (sessions.isEmpty()) {
            activeIdx = 0;
            createSession();
        } else {
            if (wasActive) {
                activeIdx = Math.max(0, Math.min(idx, sessions.size() - 1));
                syncTerminalFromSession();
                updateNetState();
            } else if (idx < activeIdx) {
                activeIdx--;
            }
            rebuildSessionList();
        }
        saveAllSessions();
        refreshServiceNotification();
    }

    private static String arabicDigits(int n) {
        StringBuilder sb = new StringBuilder();
        for (char c : String.valueOf(n).toCharArray()) {
            sb.append((char) ('٠' + (c - '0')));
        }
        return sb.toString();
    }

    // ---------- استمرارية الجلسات (م-9) ----------

    /** مجلد حفظ الجلسات: خارج مجلد ملفات المحرك، فلا
     * يظهر أبدا في `ات`. يُحفظ السجل والمحفوظات لكل جلسة
     * (لا حالة المحرك نفسه: المتغيرات تُفقد عند إعادة
     * التشغيل، والسجل يعود باللافتة الجديدة بعده). */
    private File sessionsDir() {
        return getDir("sessions", MODE_PRIVATE);
    }

    private void saveAllSessions() {
        if (sessions.isEmpty()) return;
        saveTerminalToSession();
        for (int i = 0; i < sessions.size(); i++) {
            saveSessionToDisk(sessions.get(i), i);
        }
        try {
            writeFile(new File(sessionsDir(), "active"),
                    String.valueOf(activeIdx));
        } catch (IOException ignored) {
        }
        // مرآة رقم الجلسة النشطة في الإعدادات (بند 14):
        // تُستعاد عند العودة إن كانت ما زالت حية.
        if (prefs != null) {
            prefs.edit().putInt(PREF_ACTIVE_SESSION, activeIdx).apply();
        }
    }

    private void saveSessionToDisk(Session s, int idx) {
        try {
            File dir = sessionsDir();
            // السجل: يُسقط السطر الجزئي الأخير (الموجّه +
            // أي نص نصف مكتوب — لا يُستعاد).
            String text = s.text.toString();
            int lastNl = text.lastIndexOf('\n');
            String saved = lastNl >= 0 ? text.substring(0, lastNl + 1) : "";
            writeFile(new File(dir, "session-" + idx + ".txt"), saved);
            StringBuilder meta = new StringBuilder();
            meta.append(s.networkAllowed ? "1" : "0").append('\n');
            meta.append(s.startTime).append('\n');
            for (String h : s.history) {
                meta.append(h.replace("\n", " ")).append('\u0000');
            }
            writeFile(new File(dir, "session-" + idx + ".meta"), meta.toString());
        } catch (IOException ignored) {
        }
    }

    private boolean restoreSessions() {
        File dir = sessionsDir();
        if (!dir.isDirectory()) return false;
        File[] files = dir.listFiles();
        if (files == null) return false;
        List<Integer> idxs = new ArrayList<>();
        for (File f : files) {
            String name = f.getName();
            if (name.startsWith("session-") && name.endsWith(".txt")) {
                try {
                    idxs.add(Integer.parseInt(
                            name.substring(8, name.length() - 4)));
                } catch (NumberFormatException ignored) {
                }
            }
        }
        if (idxs.isEmpty()) return false;
        idxs.sort(Integer::compareTo);
        for (int idx : idxs) {
            if (sessions.size() >= MAX_SESSIONS) break;
            Session s = new Session();
            try {
                s.text = new StringBuilder(readFile(new File(dir, "session-" + idx + ".txt")));
            } catch (IOException ignored) {
                continue;
            }
            recountSession(s);
            s.inputStart = -1;
            // سطر الصدق (بند 13): السجل المستعاد للقراءة
            // فقط، والمتغيرات لا تعود — يُضاف مرة واحدة
            // (الحارس يمنع التكرار عند كل حفظ واستعادة).
            if (!s.text.toString().endsWith(RESTORE_NOTE + "\n")) {
                s.text.append(RESTORE_NOTE).append("\n");
                recountSession(s);
            }
            try {
                String meta = readFile(new File(dir, "session-" + idx + ".meta"));
                String[] lines = meta.split("\n", 3);
                if (lines.length >= 2) {
                    s.networkAllowed = "1".equals(lines[0]);
                    s.networkAsked = s.networkAllowed;
                    try {
                        s.startTime = Long.parseLong(lines[1].trim());
                    } catch (NumberFormatException ignored) {
                    }
                    if (lines.length == 3) {
                        for (String h : lines[2].split("\u0000")) {
                            if (!h.isEmpty()) s.history.add(h);
                        }
                    }
                }
            } catch (IOException ignored) {
            }
            sessions.add(s);
            startProcess(s);
        }
        if (sessions.isEmpty()) return false;
        int want = 0;
        boolean haveWant = false;
        try {
            want = Integer.parseInt(readFile(new File(dir, "active")).trim());
            haveWant = true;
        } catch (Exception ignored) {
        }
        if (!haveWant && prefs != null) {
            // بديل: رقم الجلسة من الإعدادات (بند 14).
            want = prefs.getInt(PREF_ACTIVE_SESSION, 0);
        }
        activeIdx = Math.max(0, Math.min(want, sessions.size() - 1));
        syncTerminalFromSession();
        updateNetState();
        rebuildSessionList();
        return true;
    }

    private static void writeFile(File f, String content) throws IOException {
        try (FileOutputStream fos = new FileOutputStream(f)) {
            fos.write(content.getBytes(StandardCharsets.UTF_8));
        }
    }

    private static String readFile(File f) throws IOException {
        try (FileInputStream fis = new FileInputStream(f)) {
            byte[] bytes = new byte[(int) f.length()];
            int read = 0;
            while (read < bytes.length) {
                int n = fis.read(bytes, read, bytes.length - read);
                if (n < 0) break;
                read += n;
            }
            return new String(bytes, 0, read, StandardCharsets.UTF_8);
        }
    }

    // ---------- عملية المحرك ----------

    private String binaryPath() {
        return new File(getApplicationInfo().nativeLibraryDir, "libniva.so").getAbsolutePath();
    }

    private void startProcess(Session s) {
        try {
            List<String> cmd = new ArrayList<>();
            cmd.add(binaryPath());
            cmd.add("--تفاعلي");
            if (s.networkAllowed) cmd.add("--اسمح-بالشبكة");
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.directory(getFilesDir());
            pb.redirectErrorStream(false);
            // قناة التحكم (م-6): المحرك يرسل إشاراته
            // المنظمة على stderr فقط حين تُطلب.
            pb.environment().put("NIVA_CONTROL_CHANNEL", "stderr");
            s.process = pb.start();
            s.stdin = s.process.getOutputStream();
            s.ended = false;
            s.endedWithError = false;
            s.endNotified = false;
            s.restarting = false;
            s.procGen++;
            synchronized (s.outLock) {
                s.outBuf.setLength(0);
                s.outPending = false;
            }
            s.inQueue = new ArrayBlockingQueue<>(IN_QUEUE_CAP);
            final Process proc = s.process;
            final InputStream in = s.process.getInputStream();
            final InputStream errIn = s.process.getErrorStream();
            // كاتب الإدخال (بند 15): خيط واحد يسحب من
            // الطابور المحدود ويكتب في stdin بالترتيب؛
            // لا كتابة مباشرة من خيط الواجهة أبدا.
            s.writer = new Thread(() -> {
                try {
                    while (true) {
                        String line = s.inQueue.take();
                        OutputStream out = s.stdin;
                        if (out == null) break;
                        out.write((line + "\n").getBytes(StandardCharsets.UTF_8));
                        out.flush();
                    }
                } catch (InterruptedException | IOException ignored) {
                }
            });
            s.writer.start();
            // خيط الانتظار (بند 15): يكتشف موت العملية
            // بنفسه عبر waitFor، بدلا من الاعتماد على EOF
            // وحده — وينقل رمز الخروج للعرض.
            s.waiter = new Thread(() -> {
                try {
                    int code = proc.waitFor();
                    runOnUiThread(() -> onProcessEnded(s, proc, code));
                } catch (InterruptedException ignored) {
                }
            });
            s.waiter.start();
            // قارئ stdout: يدمج القطع الواصلة بسرعة (م-1)
            // ويحلّ UTF-8 عبر InputStreamReader فلا ينكسر
            // حرف عربي على حدود القطع.
            s.reader = new Thread(() -> {
                try {
                    InputStreamReader reader =
                            new InputStreamReader(in, StandardCharsets.UTF_8);
                    char[] buf = new char[4096];
                    StringBuilder pend = new StringBuilder();
                    int n;
                    while ((n = reader.read(buf)) != -1) {
                        pend.append(buf, 0, n);
                        boolean shortRead = n < buf.length;
                        boolean promptTail = isPromptTail(pend);
                        if (shortRead || promptTail || pend.length() >= 8192) {
                            final String out = pend.toString();
                            pend.setLength(0);
                            postOutput(s, out);
                        }
                    }
                    if (pend.length() > 0) {
                        postOutput(s, pend.toString());
                    }
                } catch (IOException ignored) {
                }
                runOnUiThread(() -> onProcessEnded(s, proc, -1));
            });
            s.reader.start();
            // قارئ stderr: إشارات تحكم منظمة (م-6) + نص
            // الانهيار إن وجد.
            s.errReader = new Thread(() -> {
                try {
                    BufferedReader br = new BufferedReader(
                            new InputStreamReader(errIn, StandardCharsets.UTF_8));
                    String line;
                    while ((line = br.readLine()) != null) {
                        final String l = line;
                        if (l.startsWith(CONTROL_PREFIX)) {
                            final String signal = l.substring(CONTROL_PREFIX.length());
                            runOnUiThread(() -> handleControl(s, signal));
                        } else {
                            postOutput(s, l + "\n");
                        }
                    }
                } catch (IOException ignored) {
                }
            });
            s.errReader.start();
        } catch (IOException e) {
            // فشل التشغيل (م-5): خطأ داخل الترمنال نفسه مع
            // موجّه يعمل، فيكتب المستخدم `اع` لإعادة المحاولة.
            s.ended = true;
            s.endedWithError = true;
            s.restarting = false;
            String prompt = CONT_PROMPT_TEXT.equals(s.lastPrompt) ? "~ $ " : s.lastPrompt;
            s.lastPrompt = prompt;
            String msg = "تعذر تشغيل المحرك — اكتب اع لإعادة المحاولة.\n" + prompt;
            appendSessionText(s, msg);
            if (s == active()) {
                appendToTerminal(msg);
                s.inputStart = s.text.length();
                terminalView.inputStart = s.inputStart;
                paintPrompt();
                scrollToBottomIfAuto();
            } else {
                s.inputStart = s.text.length();
            }
        }
    }

    /** إلحاق مخرجات إلى خيط الواجهة عبر مخزن مدموج
     * (بند 15): قطعة معلّقة واحدة لكل جلسة كحد أقصى —
     * القطع المتتالية تندمج في قطعة واحدة بدلا من إغراق
     * طابور الواجهة بلا ضغط عكسي. */
    private void postOutput(Session s, String out) {
        final int gen;
        synchronized (s.outLock) {
            gen = s.procGen;
            s.outBuf.append(out);
            if (s.outPending) return;
            s.outPending = true;
        }
        runOnUiThread(() -> {
            String drained;
            synchronized (s.outLock) {
                if (gen != s.procGen) {
                    s.outPending = false;
                    return;
                }
                drained = s.outBuf.toString();
                s.outBuf.setLength(0);
                s.outPending = false;
            }
            if (!drained.isEmpty()) appendOutput(s, drained);
        });
    }

    /** إشارة تحكم منظمة من المحرك (م-6): لا مطابقة نصية
     * على مخرجات المستخدم أبدا. */
    private void handleControl(Session s, String signal) {
        if ("شبكة".equals(signal)) {
            if (!s.networkAsked && !s.networkAllowed) {
                showNetworkDialog(s);
            }
        } else if ("مسح".equals(signal)) {
            s.history.clear();
            s.histIdx = -1;
            s.draft = "";
        }
    }

    /** هل ينتهي النص المتراكم بموجّه؟ (السطر الجزئي
     * الأخير هو `~ $ ` أو `~/... $ ` أو `> `.) */
    private static boolean isPromptTail(StringBuilder pend) {
        int lastNl = pend.lastIndexOf("\n");
        String tail = lastNl >= 0 ? pend.substring(lastNl + 1) : pend.toString();
        return isPrompt(tail);
    }

    /** هل هذا السطر الجزئي موجّه؟ */
    private static boolean isPrompt(String tail) {
        if (CONT_PROMPT_TEXT.equals(tail)) return true;
        if ("~ $ ".equals(tail)) return true;
        if (tail.startsWith("~/") && tail.endsWith(" $ ")) {
            String mid = tail.substring(2, tail.length() - 3);
            return !mid.isEmpty() && !mid.contains(" ") && !mid.contains("\n");
        }
        return false;
    }

    private void restartProcess(Session s) {
        if (s.restarting) return;
        // إغلاق stdin القديم وانتظار القارئين على خيط
        // عامل (م-4) — لا يُجمَّد خيط الواجهة أبدا — ثم
        // بدء العملية الجديدة على خيط الواجهة.
        final Process oldProc = s.process;
        final Thread oldReader = s.reader;
        final Thread oldErrReader = s.errReader;
        final Thread oldWriter = s.writer;
        final Thread oldWaiter = s.waiter;
        final OutputStream oldIn = s.stdin;
        s.process = null;
        s.stdin = null;
        s.reader = null;
        s.errReader = null;
        s.writer = null;
        s.waiter = null;
        s.ended = true;
        s.restarting = true;
        // إعادة التشغيل تقتل أي جلب جارٍ (بند 11).
        s.fetchWakeHeld = false;
        releaseFetchWakeLockIfIdle();
        new Thread(() -> {
            if (oldWriter != null) oldWriter.interrupt();
            if (oldWaiter != null) oldWaiter.interrupt();
            if (oldIn != null) {
                try {
                    oldIn.close();
                } catch (IOException ignored) {
                }
            }
            if (oldProc != null) {
                try {
                    oldProc.destroy();
                } catch (Exception ignored) {
                }
            }
            try {
                if (oldReader != null) oldReader.join(1000);
            } catch (InterruptedException ignored) {
            }
            try {
                if (oldErrReader != null) oldErrReader.join(1000);
            } catch (InterruptedException ignored) {
            }
            try {
                if (oldWriter != null) oldWriter.join(1000);
            } catch (InterruptedException ignored) {
            }
            try {
                if (oldWaiter != null) oldWaiter.join(1000);
            } catch (InterruptedException ignored) {
            }
            if ((oldReader != null && oldReader.isAlive())
                    || (oldErrReader != null && oldErrReader.isAlive())
                    || (oldWriter != null && oldWriter.isAlive())
                    || (oldWaiter != null && oldWaiter.isAlive())) {
                if (oldProc != null) {
                    try {
                        oldProc.destroyForcibly();
                    } catch (Exception ignored) {
                    }
                }
            }
            runOnUiThread(() -> {
                s.restarting = false;
                startProcess(s);
            });
        }).start();
    }

    private void onProcessEnded(Session s, Process proc, int exitCode) {
        // تجاهل قارئ قديم بعد إعادة التشغيل.
        if (s.process != proc) return;
        // القارئ وخيط الانتظار يبلغان معا — مرة واحدة فقط.
        if (s.endNotified) return;
        s.endNotified = true;
        s.ended = true;
        // رمز الخروج: -1 تعني «من مسار EOF، غير معروف» —
        // يُحلّ عبر exitValue إن أمكن.
        int code = exitCode;
        if (code < 0) {
            try {
                code = proc.exitValue();
            } catch (Exception ignored) {
                code = 0;
            }
        }
        s.endedWithError = code != 0;
        s.fetchWakeHeld = false;
        releaseFetchWakeLockIfIdle();
        // موجّه الاستكمال لا يصلح لمحرك ميت (الكتلة ذهبت
        // معه) — الموجّه الوهمي موجّه عادي.
        String prompt = CONT_PROMPT_TEXT.equals(s.lastPrompt) ? "~ $ " : s.lastPrompt;
        s.lastPrompt = prompt;
        String msg = "\n" + ENDED_MESSAGE + "\n" + prompt;
        appendSessionText(s, msg);
        if (s == active()) {
            appendToTerminal(msg);
            s.inputStart = s.text.length();
            terminalView.inputStart = s.inputStart;
            paintPrompt();
            scrollToBottomIfAuto();
        } else {
            s.inputStart = s.text.length();
        }
        rebuildSessionList();
        refreshServiceNotification();
    }

    // ---------- الإخراج ----------

    /** إلحاق واحد موحّد بنص الجلسة: يحدّث السطر الجزئي
     * وعدّ الأسطر ويقصّ الرأس عند السقف (م-1). */
    private void appendSessionText(Session s, String text) {
        s.text.append(text);
        for (int i = 0; i < text.length(); i++) {
            if (text.charAt(i) == '\n') s.lineCount++;
        }
        String combined = s.tail + text;
        int lastNl = combined.lastIndexOf('\n');
        s.tail = lastNl >= 0 ? combined.substring(lastNl + 1) : combined;
        if (s.tail.length() > 128) s.tail = s.tail.substring(s.tail.length() - 128);
        trimTranscript(s);
    }

    private void recountSession(Session s) {
        s.lineCount = 0;
        for (int i = 0; i < s.text.length(); i++) {
            if (s.text.charAt(i) == '\n') s.lineCount++;
        }
        int lastNl = s.text.lastIndexOf("\n");
        String tail = lastNl >= 0 ? s.text.substring(lastNl + 1) : s.text.toString();
        s.tail = tail.length() > 128 ? tail.substring(tail.length() - 128) : tail;
    }

    /** قصّ رأس السجل عند السقف (م-1): فقط حين لا موجّه
     * معروضا (فلا يُفقد نص مكتوب)، ومن الرأس إلى حدود سطر. */
    private void trimTranscript(Session s) {
        if (s.inputStart >= 0) return;
        while ((s.lineCount > MAX_LINES || s.text.length() > MAX_CHARS) && s.lineCount > 100) {
            int idx = s.text.indexOf("\n");
            if (idx < 0) break;
            int removed = idx + 1;
            s.text.delete(0, removed);
            s.lineCount--;
            if (s == active()) {
                // الـspan يبقى في النص (الموجّه القديم يحافظ
                // على لونه الأخضر)؛ يُصفَّر التتبع فقط، فيرسم
                // الموجّه الجديد span جديدا في موضعه.
                currentPromptSpan = null;
                currentPromptStart = -1;
                terminalView.programmatic = true;
                // إصلاح عطل مسح v1.1.2 (بند 0): الحذف من
                // العرض يُقصّ بطول العرض نفسه — حذف أكثر
                // من طوله يرمي StringIndexOutOfBoundsException
                // فيقتل التطبيق (يحدث إن اختلف العرض عن
                // السجل، مثلا بعد استعادة أو تبديل جلسة).
                int viewLen = terminalView.getText().length();
                int del = Math.min(removed, viewLen);
                if (del > 0) terminalView.getText().delete(0, del);
                terminalView.programmatic = false;
            }
        }
    }

    private void appendOutput(Session s, String chunk) {
        appendSessionText(s, chunk);
        boolean promptArrived = isPrompt(s.tail);
        if (promptArrived) {
            s.inputStart = s.text.length();
            s.lastPrompt = s.tail;
            String dir = dirFromPrompt(s.tail);
            if (dir != null) s.currentDir = dir;
        }
        if (s == active()) {
            appendToTerminal(chunk);
            terminalView.inputStart = s.inputStart;
            paintPrompt();
            scrollToBottomIfAuto();
        }
        // خطأ جلب في هذه القطعة (بند 12): يُحفَظ حتى وصول
        // الموجّه، فقد تصل القطعة والموجّه منفصلين.
        if (s.fetchWakeHeld && isFetchError(chunk)) {
            s.lastFetchError = true;
        }
        // نهاية جلب (بند 11 + 12): وصول الموجّه يعني أن
        // الجلب انتهى أو فشل أو بلغ مهلته — يُحرَّر القفل
        // قبل إرسال أي سطر جديد.
        if (promptArrived && s.fetchWakeHeld) {
            boolean fetchErr = s.lastFetchError;
            s.lastFetchError = false;
            s.fetchWakeHeld = false;
            releaseFetchWakeLockIfIdle();
            // خطأ جلب والجهاز دون اتصال: رسالة الانقطاع
            // الواضحة بعد خطأ المحرك، ثم موجّه جديد.
            if (fetchErr && !isOnline()) {
                appLine(s, OFFLINE_MESSAGE);
            }
        }
        // سطر معلّق من إعادة تشغيل: يُرسَل حين يصل الموجّه
        // الجديد (فالمحرك جاهز حينها) — ولو كانت الجلسة في
        // الخلفية، فالسطر عُلّق فيها أصلا.
        if (promptArrived && s.awaitingRestartResend && s.pendingLine != null
                && s.process != null && !s.ended) {
            String pl = s.pendingLine;
            s.pendingLine = null;
            s.awaitingRestartResend = false;
            sendLineToEngine(s, pl);
            return;
        }
        // طابور اللصق/الكتابة (بند 2 + 10): سطر واحد لكل
        // موجّه يصل، فيبقى التتابع سليما مع كتل المحرك.
        if (promptArrived && !s.pasteQueue.isEmpty()
                && s.process != null && !s.ended) {
            drainPasteQueue(s);
        }
    }

    /** المجلد من نص الموجّه («~ $ » أو «~/x $ »)؛ null
     * لموجّه الاستكمال «> ». */
    private static String dirFromPrompt(String tail) {
        if ("~ $ ".equals(tail)) return "~";
        if (tail.startsWith("~/") && tail.endsWith(" $ ")) {
            String mid = tail.substring(2, tail.length() - 3);
            if (!mid.isEmpty() && !mid.contains(" ") && !mid.contains("\n")) {
                return "~/" + mid;
            }
        }
        return null;
    }

    /** هل القطعة خطأ جلب من المحرك؟ (بند 12). */
    private static boolean isFetchError(String chunk) {
        for (String marker : FETCH_ERROR_MARKERS) {
            if (chunk.contains(marker)) return true;
        }
        return false;
    }

    private void appendToTerminal(String chunk) {
        terminalView.programmatic = true;
        terminalView.getText().append(chunk);
        terminalView.programmatic = false;
    }

    /** لون الموجّه الحالي بالأخضر (داخل الترمنال فقط).
     * يُعاد الرسم فقط عند تغيّر موضع الموجّه (م-1): لا
     * مسح لكل الـspans مع كل قطعة، والموجّهات القديمة
     * تبقى خضراء. */
    private void paintPrompt() {
        Session s = active();
        if (s.inputStart < 0) return;
        int start = s.inputStart - s.lastPrompt.length();
        if (start < 0) return;
        if (currentPromptSpan != null && currentPromptStart == start) return;
        Editable text = terminalView.getText();
        // إصلاح عطل مسح v1.1.2 (بند 0): موجّه أبعد من
        // طول النص يرمي IndexOutOfBoundsException في
        // setSpan فيقتل التطبيق.
        if (s.inputStart > text.length()) return;
        ForegroundColorSpan span = new ForegroundColorSpan(promptColor);
        text.setSpan(span, start, s.inputStart,
                Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
        currentPromptSpan = span;
        currentPromptStart = start;
    }

    private void moveCursorToEnd() {
        int end = terminalView.getText().length();
        terminalView.setSelection(end, end);
    }

    /** النزول إلى أسفل السجل فقط حين التمرير التلقائي
     * يعمل (بند 8)؛ وإلا يبقى المستخدم حيث يقرأ. */
    private void scrollToBottomIfAuto() {
        if (autoScroll) moveCursorToEnd();
    }

    /** تبديل التمرير التلقائي من زر «تمرير» (بند 8). */
    private void toggleAutoScroll() {
        autoScroll = !autoScroll;
        updateScrollHighlight();
        if (autoScroll) moveCursorToEnd();
    }

    private void updateScrollHighlight() {
        if (scrollButton == null) return;
        scrollButton.setBackgroundColor(autoScroll ? colorActive : colorBtn);
    }

    // ---------- الإدخال ----------

    private String currentLine() {
        int start = terminalView.inputStart;
        if (start < 0) return "";
        // إصلاح عطل مسح v1.1.2 (بند 0): بداية أبعد من
        // طول النص تُقصّ بدلا من رمي استثناء.
        int len = terminalView.getText().length();
        if (start > len) return "";
        return terminalView.getText().toString().substring(start);
    }

    private void setCurrentLine(String line) {
        int start = terminalView.inputStart;
        if (start < 0) return;
        terminalView.programmatic = true;
        Editable text = terminalView.getText();
        int safe = Math.min(start, text.length());
        text.replace(safe, text.length(), line);
        terminalView.programmatic = false;
        moveCursorToEnd();
    }

    private void submitLine() {
        Session s = active();
        if (terminalView.inputStart < 0) return;
        String line = currentLine();
        // أزل سطرا فارغا ألصقه المستخدم (لا يؤثر في الأمر).
        if (line.endsWith("\n")) line = line.substring(0, line.length() - 1);
        // سقف سطر الإدخال (م-10).
        if (line.length() > MAX_LINE_CHARS) {
            appLine(s, "السطر أطول من الحد الأقصى (64 كيلوبايت)");
            return;
        }
        dispatchLine(s, line, true);
    }

    /** إرسال سطر واحد: المحفوظات + العرض + التوجيه
     * (اختصار تطبيق / إعادة تشغيل / المحرك). تُستخدم من
     * الكتابة واللصق وطابور الكتابة على السواء (بند 2 +
     * 10)؛ lineAlreadyInView تعني أن السطر مكتوب في
     * العرض أصلا (من لوحة المفاتيح) فيُلحق سطر جديد فقط. */
    private void dispatchLine(Session s, String line, boolean lineAlreadyInView) {
        if (!line.trim().isEmpty()) {
            s.history.add(line);
            s.histIdx = -1;
            s.draft = "";
        }
        // أغلق سطر الإدخال في السجل والترمنال، ثم انتظر المحرك.
        terminalView.inputStart = -1;
        s.inputStart = -1;
        appendSessionText(s, line + "\n");
        appendToTerminal(lineAlreadyInView ? "\n" : line + "\n");
        if (line.trim().isEmpty()) {
            // سطر فارغ: لا شيء يرسل؛ أعد الموجّه.
            String prompt = s.lastPrompt;
            appendSessionText(s, prompt);
            appendToTerminal(prompt);
            s.inputStart = s.text.length();
            terminalView.inputStart = s.inputStart;
            paintPrompt();
            scrollToBottomIfAuto();
            return;
        }
        // اختصارات التطبيق أولا: لا تصل إلى المحرك أبدا.
        String trimmed = line.trim();
        String first = trimmed.split("\\s+")[0];
        if (isAppCode(first)) {
            handleAppShortcut(s, first, trimmed);
            return;
        }
        // عبارة التحديث الذاتي (v1.1.4): لا تصل إلى
        // المحرك أبدا — فحص إصدارات جيت هاب من التطبيق.
        if (UPDATE_PHRASE.equals(trimmed)) {
            runSelfUpdate(s);
            return;
        }
        // المحرك متوقف: علّق السطر وأعد التشغيل، ويُرسَل
        // السطر حين يصل الموجّه الجديد (م-5).
        if (s.ended || s.process == null) {
            s.pendingLine = line;
            s.awaitingRestartResend = true;
            if (!s.restarting) restartProcess(s);
            return;
        }
        // سجل السطر المعلق: إن طلب المحرك إذن الشبكة، يعاد
        // إرسال هذا السطر نفسه بعد السماح.
        s.pendingLine = line;
        sendLineToEngine(s, line);
    }

    /** إرسال سطر إلى المحرك عبر الطابور المحدود (بند 15)،
     * مع فحص الشبكة وقفل الاستيقاظ للجلب (بند 11 + 12). */
    private void sendLineToEngine(Session s, String line) {
        if (isFetchCommand(line.trim())) {
            if (!isOnline()) {
                // دون اتصال: لا إرسال ولا انتظار مهلة DNS —
                // رسالة واضحة وموجّه جديد فورا.
                String out = OFFLINE_MESSAGE + "\n" + s.lastPrompt;
                appendSessionText(s, out);
                appendToTerminal(out);
                s.inputStart = s.text.length();
                terminalView.inputStart = s.inputStart;
                paintPrompt();
                scrollToBottomIfAuto();
                return;
            }
            acquireFetchWakeLock(s);
        }
        sendToEngine(s, line);
    }

    /** هل هذا السطر أمر جلب؟ اختصار `اج` أو استدعاء
     * `شبكة.اجلب(` صريح — تقدير متحفظ وموثّق: جلب داخل
     * كتلة متعددة الأسطر لا يُكتشف من سطرها الأول. */
    private static boolean isFetchCommand(String trimmed) {
        if (trimmed.isEmpty()) return false;
        String first = trimmed.split("\\s+")[0];
        return "اج".equals(first) || trimmed.contains("شبكة.اجلب(");
    }

    private void sendToEngine(Session s, String line) {
        if (s.stdin == null) return;
        // الطابور المحدود (بند 15): عرض غير حاجب؛ الامتلاء
        // (64 سطرا معلقا) يُبلَّغ ولا يُفقَد بصمت.
        boolean ok = s.inQueue.offer(line);
        if (!ok) {
            Toast.makeText(this, "تعذر إرسال السطر", Toast.LENGTH_SHORT).show();
        }
    }

    /** سحب سطر واحد من طابور اللصق/الكتابة وإرساله
     * (بند 2 + 10): الأسطر الفارغة تُتخطى حتى لا يتوقف
     * الطابور بانتظار موجّه لن يأتي. */
    private void drainPasteQueue(Session s) {
        while (!s.pasteQueue.isEmpty()) {
            String line = s.pasteQueue.poll();
            if (line == null || line.trim().isEmpty()) continue;
            dispatchLine(s, line, false);
            return;
        }
    }

    private void historyNav(int delta) {
        Session s = active();
        if (s.history.isEmpty() || terminalView.inputStart < 0) return;
        if (s.histIdx == -1 && delta < 0) {
            s.draft = currentLine();
            s.histIdx = s.history.size() - 1;
        } else if (delta < 0 && s.histIdx > 0) {
            s.histIdx--;
        } else if (delta > 0 && s.histIdx >= 0 && s.histIdx < s.history.size() - 1) {
            s.histIdx++;
        } else if (delta > 0 && s.histIdx == s.history.size() - 1) {
            s.histIdx = -1;
        } else {
            return;
        }
        setCurrentLine(s.histIdx == -1 ? s.draft : s.history.get(s.histIdx));
    }

    // ---------- اختصارات التطبيق (7 من الـ50) ----------

    private static boolean isAppCode(String word) {
        for (String code : APP_CODES) {
            if (code.equals(word)) return true;
        }
        return false;
    }

    /** سطر معلومات من التطبيق نفسه، يعقبه الموجّه الحالي. */
    private void appLine(Session s, String info) {
        String out = info.isEmpty() ? s.lastPrompt : info + "\n" + s.lastPrompt;
        appendSessionText(s, out);
        appendToTerminal(out);
        s.inputStart = s.text.length();
        terminalView.inputStart = s.inputStart;
        paintPrompt();
        scrollToBottomIfAuto();
    }

    private void handleAppShortcut(Session s, String code, String trimmed) {
        if (!trimmed.equals(code)) {
            appLine(s, "خطأ: " + code + " بدون معاملات");
            return;
        }
        switch (code) {
            case "جل":
                openDrawer();
                // لا مخرجات: الدرج نفسه هو الرد. أعد الموجّه.
                appLine(s, "");
                break;
            case "جد":
                if (!createSession()) {
                    appLine(s, "وصلت إلى الحد الأقصى (٨ جلسات)");
                }
                break;
            case "مش":
                // مسح الشاشة فقط (مثل clear في أي ترمنال):
                // السجل يختفي من العرض، لكن محفوظات المحرك
                // (:اعرض) والمتغيرات تبقى كما هي — فـ`مش`
                // ليست `مس`. موثّق في SHORTCUTS-50.md (م-11).
                // إصلاح عطل مسح v1.1.2 (بند 0): لا setText
                // هنا أبدا — استبدال النص كله تحت لوحة
                // مفاتيح نشطة يتركها تطلب تحديدا بموضع قديم
                // فيقتل التطبيق؛ clear() على النص نفسه مع
                // مؤشر مصفَّر صراحة هو الآمن.
                s.text.setLength(0);
                s.lineCount = 0;
                s.tail = "";
                terminalView.programmatic = true;
                terminalView.getText().clear();
                terminalView.setSelection(0, 0);
                terminalView.programmatic = false;
                currentPromptSpan = null;
                currentPromptStart = -1;
                appendSessionText(s, s.lastPrompt);
                appendToTerminal(s.lastPrompt);
                s.inputStart = s.text.length();
                terminalView.inputStart = s.inputStart;
                paintPrompt();
                scrollToBottomIfAuto();
                break;
            case "شب":
                appLine(s, "الشبكة: " + (s.networkAllowed ? "مفتوحة" : "مغلقة"));
                break;
            case "سش":
                // السماح بالشبكة بلا إعادة تشغيل (م-2): أمر
                // تحكم داخل العملية نفسها، فتبقى كل متغيرات
                // الجلسة. الموجّه التالي من المحرك هو الرد.
                if (s.networkAllowed) {
                    appLine(s, "الشبكة: مفتوحة بالفعل");
                } else {
                    s.networkAllowed = true;
                    s.networkAsked = true;
                    updateNetState();
                    sendToEngine(s, ALLOW_NETWORK_COMMAND);
                }
                break;
            case "مد": {
                long seconds = (System.currentTimeMillis() - s.startTime) / 1000;
                String when = seconds < 60
                        ? seconds + " ثانية"
                        : (seconds / 60) + " دقيقة " + (seconds % 60) + " ثانية";
                appLine(s, "مدة الجلسة: " + when);
                break;
            }
            case "اع":
                restartProcess(s);
                break;
            case "تن":
                // التحديث الذاتي (v1.1.4): فحص إصدارات
                // جيت هاب عند الطلب — الكتابة نفسها هي
                // الموافقة على الفحص.
                runSelfUpdate(s);
                break;
            default:
                break;
        }
    }

    // ---------- الشبكة ----------

    private void updateNetState() {
        Session s = active();
        netStateView.setText("الشبكة: " + (s.networkAllowed ? "مفتوحة" : "مغلقة"));
    }

    private void showNetworkDialog(Session s) {
        // حراسة الحوار (م-7): لا يعرض بعد تدمير النشاط،
        // ويذكر رقم الجلسة، ولا يعرض تلقائيا إلا للجلسة
        // النشطة — طلب الجلسات الخلفية يُؤجَّل.
        if (isFinishing() || isDestroyed()) return;
        if (s != active()) {
            s.networkPending = true;
            return;
        }
        s.networkAsked = true;
        int num = sessions.indexOf(s) + 1;
        new AlertDialog.Builder(this)
                .setMessage("الجلسة " + arabicDigits(num)
                        + ": هذا البرنامج يريد الوصول إلى الشبكة — هل تسمح؟")
                .setPositiveButton("سماح", (d, w) -> {
                    // السماح بلا إعادة تشغيل (م-2): أمر تحكم
                    // داخل العملية، ثم إعادة إرسال السطر المعلق.
                    s.networkAllowed = true;
                    updateNetState();
                    sendToEngine(s, ALLOW_NETWORK_COMMAND);
                    if (s.pendingLine != null) {
                        String pl = s.pendingLine;
                        s.pendingLine = null;
                        // عبر فحص الجلب (بند 11 + 12): دون
                        // اتصال يُعرض «لا يوجد اتصال» بدلا من
                        // انتظار مهلة DNS.
                        sendLineToEngine(s, pl);
                    }
                })
                .setNegativeButton("رفض", (d, w) -> {
                    // لا يسأل مرة أخرى في هذه الجلسة.
                    s.pendingLine = null;
                })
                .setCancelable(false)
                .show();
    }

    // ---------- التحديث الذاتي من جيت هاب (v1.1.4) ----------

    /** بدء فحص التحديث: دون اتصال رسالة فورية؛ وإلا
     * سطر تقدّم ثم الفحص على خيط عامل (لا شبكة على
     * خيط الواجهة أبدا). الكتابة نفسها هي الموافقة
     * على فحص واحد — بلا فحص تلقائي عند البدء. */
    private void runSelfUpdate(Session s) {
        if (!isOnline()) {
            appLine(s, OFFLINE_MESSAGE);
            return;
        }
        updateLine(s, "جارٍ التحقق من التحديث…");
        new Thread(() -> checkRelease(s)).start();
    }

    /** سطر تقدّم بلا موجّه (يُستدعى من خيط الواجهة
     * فقط): يُلحق بسجل الجلسة، وبالعرض إن كانت ما
     * زالت نشطة — ولا يمكّن الإدخال؛ الموجّه يعود
     * مع سطر النتيجة الأخير. */
    private void updateLine(Session s, String info) {
        String out = info + "\n";
        appendSessionText(s, out);
        if (s == active()) {
            appendToTerminal(out);
            scrollToBottomIfAuto();
        }
    }

    /** سطر تقدّم من خيط عامل (يُرسَل إلى الواجهة). */
    private void postUpdateLine(Session s, String info) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) return;
            updateLine(s, info);
        });
    }

    /** سطر النتيجة الأخير + موجّه جديد (من خيط
     * الواجهة): كـappLine لكنه آمن إن بدّل المستخدم
     * الجلسة أثناء الفحص — يُلحق بالمخزن دائما،
     * وبالعرض فقط إن كانت الجلسة ما زالت نشطة. */
    private void updateResultLine(Session s, String msg) {
        String out = msg.isEmpty() ? s.lastPrompt : msg + "\n" + s.lastPrompt;
        appendSessionText(s, out);
        s.inputStart = s.text.length();
        if (s == active()) {
            appendToTerminal(out);
            terminalView.inputStart = s.inputStart;
            paintPrompt();
            scrollToBottomIfAuto();
        }
    }

    /** سطر النتيجة الأخير (من خيط عامل). */
    private void finishUpdate(Session s, String msg) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) return;
            updateResultLine(s, msg);
        });
    }

    /** فحص إصدارات جيت هاب (خيط عامل): أحدث إصدار،
     * ووسمه، وأول أصل .apk فيه. 404 = لا إصدار منشور
     * بعد — ولا إصدار للتطبيق حتى الآن، فالنشر معلّق
     * حتى اعتماد المالك الكامل. */
    private void checkRelease(Session s) {
        HttpURLConnection conn = null;
        try {
            conn = (HttpURLConnection) new URL(RELEASES_URL).openConnection();
            conn.setRequestMethod("GET");
            conn.setRequestProperty("Accept", "application/vnd.github+json");
            conn.setRequestProperty("User-Agent", "NivaTerminal");
            conn.setConnectTimeout(15000);
            conn.setReadTimeout(30000);
            int code = conn.getResponseCode();
            if (code == 404) {
                finishUpdate(s, "لا يوجد إصدار منشور بعد");
                return;
            }
            if (code != 200) {
                finishUpdate(s, "تعذّر التحقق من التحديث (خطأ " + code + ")");
                return;
            }
            String body = readAllCapped(conn.getInputStream(), 1024 * 1024);
            String tag = jsonString(body, "tag_name");
            int[] remote = tag == null ? null : parseVersion(tag);
            if (remote == null) {
                finishUpdate(s, "تعذّر قراءة إصدار التحديث");
                return;
            }
            int[] local = parseVersion(localVersionName());
            if (local == null) {
                finishUpdate(s, "تعذّر قراءة إصدار التحديث");
                return;
            }
            if (compareVersion(remote, local) <= 0) {
                finishUpdate(s, "لا يوجد تحديث جديد");
                return;
            }
            String assetUrl = findApkAsset(body);
            if (assetUrl == null) {
                finishUpdate(s, "لا يوجد ملف تثبيت في الإصدار");
                return;
            }
            // الإذن قبل التنزيل: بلا سماح لا تنزيل ولا
            // تثبيت — رسالة واضحة + فتح شاشة الإعدادات.
            if (!canInstallPackages()) {
                runOnUiThread(() -> {
                    if (isFinishing() || isDestroyed()) return;
                    updateLine(s, "التحديث " + tag + " متاح — لكن التثبيت غير مسموح.");
                    updateLine(s, "اسمح بتثبيت تطبيقات غير معروفة لتطبيق نيفا من الإعدادات، ثم اكتب تحديث نيفا من جديد.");
                    openUnknownSourcesSettings();
                    updateResultLine(s, "");
                });
                return;
            }
            downloadUpdate(s, assetUrl, tag);
        } catch (Exception e) {
            finishUpdate(s, "تعذّر التحقق من التحديث");
        } finally {
            if (conn != null) conn.disconnect();
        }
    }

    /** تنزيل ملف التثبيت إلى الذاكرة المؤقتة الخاصة
     * (خيط عامل): تقدّم كل 25٪، ثم فتح المثبّت.
     * التثبيت نفسه يؤكده المستخدم على شاشة النظام —
     * لا تثبيت صامت أبدا. */
    private void downloadUpdate(Session s, String url, String tag) {
        postUpdateLine(s, "جارٍ تنزيل التحديث " + tag + "…");
        HttpURLConnection conn = null;
        File out = new File(getCacheDir(), UPDATE_APK_NAME);
        try {
            conn = (HttpURLConnection) new URL(url).openConnection();
            conn.setRequestProperty("User-Agent", "NivaTerminal");
            conn.setRequestProperty("Accept", "application/octet-stream");
            conn.setConnectTimeout(15000);
            conn.setReadTimeout(60000);
            int code = conn.getResponseCode();
            if (code != 200) {
                finishUpdate(s, "تعذّر تنزيل التحديث (خطأ " + code + ")");
                return;
            }
            long total = conn.getContentLengthLong();
            if (total > MAX_UPDATE_BYTES) {
                finishUpdate(s, "حجم التحديث أكبر من المتوقع");
                return;
            }
            int nextMilestone = 25;
            try (InputStream in = new BufferedInputStream(conn.getInputStream());
                 OutputStream os = new FileOutputStream(out)) {
                byte[] buf = new byte[8192];
                long done = 0;
                int n;
                while ((n = in.read(buf)) >= 0) {
                    os.write(buf, 0, n);
                    done += n;
                    if (done > MAX_UPDATE_BYTES) {
                        throw new IOException("حجم زائد");
                    }
                    if (total > 0) {
                        int pct = (int) (done * 100 / total);
                        if (pct >= nextMilestone) {
                            int shown = (pct / 25) * 25;
                            postUpdateLine(s, "التنزيل: " + shown + "٪");
                            nextMilestone = shown + 25;
                        }
                    }
                }
            }
            if (out.length() == 0 || (total > 0 && out.length() != total)) {
                out.delete();
                finishUpdate(s, "تعذّر تنزيل التحديث");
                return;
            }
            launchInstaller(s, out);
        } catch (Exception e) {
            out.delete();
            finishUpdate(s, "تعذّر تنزيل التحديث");
        } finally {
            if (conn != null) conn.disconnect();
        }
    }

    /** فتح مثبّت النظام على الملف المنزَّل (خيط
     * الواجهة): التأكيد على شاشة النظام، والتطبيق
     * نفسه لا يثبّت شيئا بصمت أبدا. */
    private void launchInstaller(Session s, File apk) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) return;
            try {
                Uri uri = Uri.parse("content://" + ApkProvider.AUTHORITY
                        + "/" + UPDATE_APK_NAME);
                Intent intent = new Intent(Intent.ACTION_VIEW);
                intent.setDataAndType(uri,
                        "application/vnd.android.package-archive");
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK
                        | Intent.FLAG_GRANT_READ_URI_PERMISSION);
                startActivity(intent);
                updateResultLine(s, "جارٍ فتح المثبّت — أكّد التثبيت على شاشة النظام");
            } catch (Exception e) {
                apk.delete();
                updateResultLine(s, "تعذّر فتح المثبّت");
            }
        });
    }

    /** هل سُمح للتطبيق بتثبيت تطبيقات غير معروفة؟ */
    private boolean canInstallPackages() {
        try {
            return getPackageManager().canRequestPackageInstalls();
        } catch (Exception e) {
            return false;
        }
    }

    /** فتح شاشة «تثبيت تطبيقات غير معروفة» الخاصة
     * بنيفا في إعدادات النظام. */
    private void openUnknownSourcesSettings() {
        try {
            Intent intent = new Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:" + getPackageName()));
            startActivity(intent);
        } catch (Exception ignored) {
        }
    }

    /** اسم نسخة التطبيق المثبتة (مثلا 1.1.4). */
    private String localVersionName() {
        try {
            PackageInfo info;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                info = getPackageManager().getPackageInfo(getPackageName(),
                        PackageManager.PackageInfoFlags.of(0));
            } else {
                info = getPackageManager().getPackageInfo(getPackageName(), 0);
            }
            return info.versionName;
        } catch (Exception e) {
            return null;
        }
    }

    /** قراءة دفق كاملا بسقف (حتى لا يبتلع ردا ضخما). */
    private static String readAllCapped(InputStream in, int cap) throws IOException {
        BufferedReader reader = new BufferedReader(
                new InputStreamReader(in, StandardCharsets.UTF_8));
        StringBuilder out = new StringBuilder();
        char[] buf = new char[4096];
        int n;
        while ((n = reader.read(buf)) > 0) {
            out.append(buf, 0, n);
            if (out.length() > cap) throw new IOException("رد زائد");
        }
        return out.toString();
    }

    /** قيمة نصية لحقل JSON بسيط ("key": "value") —
     * بلا مكتبة JSON خارجية. */
    private static String jsonString(String json, String key) {
        Matcher m = Pattern.compile("\"" + Pattern.quote(key)
                + "\"\\s*:\\s*\"([^\"]*)\"").matcher(json);
        return m.find() ? m.group(1) : null;
    }

    /** رابط تنزيل أول أصل .apk في الإصدار (اسم الأصل
     * يسبق رابط تنزيله في الكائن نفسه). */
    private static String findApkAsset(String json) {
        Matcher name = Pattern.compile("\"name\"\\s*:\\s*\"([^\"]*\\.[Aa][Pp][Kk])\"")
                .matcher(json);
        while (name.find()) {
            Matcher url = Pattern.compile("\"browser_download_url\"\\s*:\\s*\"([^\"]*)\"")
                    .matcher(json);
            if (url.find(name.end())) return url.group(1);
        }
        return null;
    }

    /** ثلاثية نسخة من وسم (v1.1.4 أو 1.1.4)؛ null
     * إن لم يُقرأ. */
    private static int[] parseVersion(String tag) {
        if (tag == null) return null;
        Matcher m = Pattern.compile("^v?(\\d+)\\.(\\d+)\\.(\\d+)")
                .matcher(tag.trim());
        if (!m.find()) return null;
        try {
            return new int[]{Integer.parseInt(m.group(1)),
                    Integer.parseInt(m.group(2)), Integer.parseInt(m.group(3))};
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /** مقارنة ثلاثيات: موجب = الأول أحدث. */
    private static int compareVersion(int[] a, int[] b) {
        for (int i = 0; i < 3; i++) {
            if (a[i] != b[i]) return a[i] - b[i];
        }
        return 0;
    }

    @Override
    protected void onDestroy() {
        releaseFetchWakeLockIfIdle();
        if (exitReceiver != null) {
            try {
                unregisterReceiver(exitReceiver);
            } catch (Exception ignored) {
            }
            exitReceiver = null;
        }
        // إيقاف الخدمة الأمامية مع التطبيق (بند 13).
        try {
            stopService(new Intent(this, NivaService.class));
        } catch (Exception ignored) {
        }
        saveAllSessions();
        for (Session s : sessions) {
            if (s.process != null) {
                try {
                    s.process.destroy();
                } catch (Exception ignored) {
                }
            }
        }
        super.onDestroy();
    }

    // ---------- الإعدادات: niva.properties + الإعدادات المحفوظة (بند 9) ----------

    /** تحميل الإعدادات قبل بناء الواجهة: الإعدادات
     * المحفوظة أولا، ثم ملف niva.properties (يغلب عند
     * التعارض — فهو الملف الذي يحرره المستخدم). */
    private void loadSettings() {
        if (prefs != null) {
            fontSizeSp = prefs.getFloat(PREF_FONT_SIZE, DEFAULT_FONT_SP);
            keepScreenOn = prefs.getBoolean(PREF_KEEP_SCREEN, false);
        }
        loadNivaProperties();
        applyThemeFields();
        applyKeepScreenFlag();
    }

    /** ألوان المظهر من lightTheme (تُضبط قبل بناء الواجهة،
     * وتُعاد عند تبديل المظهر أثناء التشغيل). */
    private void applyThemeFields() {
        if (lightTheme) {
            colorBg = LIGHT_BG;
            colorText = LIGHT_TEXT;
            colorPanel = LIGHT_PANEL;
            colorBtn = LIGHT_BTN;
            colorActive = LIGHT_ACTIVE;
            promptColor = LIGHT_PROMPT;
            colorSubtitle = SUBTITLE_GRAY_LIGHT;
        } else {
            colorBg = DARK_BG;
            colorText = DARK_TEXT;
            colorPanel = DARK_PANEL;
            colorBtn = DARK_BTN;
            colorActive = DARK_ACTIVE;
            promptColor = DARK_PROMPT;
            colorSubtitle = SUBTITLE_GRAY_DARK;
        }
    }

    /** إعادة تلوين الواجهة المبنية عند تبديل المظهر. */
    private void applyThemeColors() {
        applyThemeFields();
        if (rootLayout != null) rootLayout.setBackgroundColor(colorBg);
        if (terminalView != null) {
            terminalView.setBackgroundColor(colorBg);
            terminalView.setTextColor(colorText);
        }
        if (titleView != null) titleView.setTextColor(colorText);
        if (netStateView != null) netStateView.setTextColor(colorText);
        if (sessionList != null) {
            sessionList.setBackgroundColor(colorPanel);
            rebuildSessionList();
        }
        if (keyRow != null && keyRow.getChildCount() > 0) {
            View child = keyRow.getChildAt(0);
            if (child instanceof LinearLayout) {
                LinearLayout row = (LinearLayout) child;
                row.setBackgroundColor(colorPanel);
                for (int i = 0; i < row.getChildCount(); i++) {
                    View b = row.getChildAt(i);
                    if (b instanceof Button) {
                        b.setBackgroundColor(colorBtn);
                        ((Button) b).setTextColor(colorText);
                    }
                }
            }
        }
        updateAltHighlight();
        updateScrollHighlight();
        currentPromptSpan = null;
        currentPromptStart = -1;
        paintPrompt();
    }

    /** قراءة niva.properties (مفتاح=قيمة، UTF-8، مفاتيح
     * عربية): حجم_الخط، ابقاء_الشاشة، اظهار_صف_المفاتيح،
     * المظهر. أسطر # تعليقات؛ المفاتيح المجهولة تُحفظ
     * عند الكتابة ولا تُفسَّر. */
    private void loadNivaProperties() {
        File f = new File(getFilesDir(), NIVA_PROPS_FILE);
        if (!f.isFile()) return;
        try (BufferedReader br = new BufferedReader(
                new InputStreamReader(new FileInputStream(f), StandardCharsets.UTF_8))) {
            String line;
            while ((line = br.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty() || line.startsWith("#")) continue;
                int eq = line.indexOf('=');
                if (eq < 0) continue;
                String key = line.substring(0, eq).trim();
                String value = line.substring(eq + 1).trim();
                if ("حجم_الخط".equals(key)) {
                    int size = arabicToInt(value);
                    if (size >= (int) MIN_FONT_SP && size <= (int) MAX_FONT_SP) {
                        fontSizeSp = size;
                    }
                } else if ("ابقاء_الشاشة".equals(key)) {
                    keepScreenOn = "نعم".equals(value);
                } else if ("اظهار_صف_المفاتيح".equals(key)) {
                    showKeyRowPref = !"لا".equals(value);
                } else if ("المظهر".equals(key)) {
                    lightTheme = "فاتح".equals(value);
                }
            }
        } catch (IOException ignored) {
        }
    }

    /** كتابة niva.properties: تُحفَظ الأسطر المجهولة
     * والتعليقات كما هي، وتُحدَّث المفاتيح الأربعة. */
    private void saveNivaProperties() {
        File f = new File(getFilesDir(), NIVA_PROPS_FILE);
        List<String> lines = new ArrayList<>();
        boolean hasSize = false, hasKeep = false, hasRow = false, hasTheme = false;
        if (f.isFile()) {
            try (BufferedReader br = new BufferedReader(
                    new InputStreamReader(new FileInputStream(f), StandardCharsets.UTF_8))) {
                String line;
                while ((line = br.readLine()) != null) {
                    String t = line.trim();
                    int eq = t.indexOf('=');
                    String key = eq >= 0 ? t.substring(0, eq).trim() : "";
                    if ("حجم_الخط".equals(key)) {
                        lines.add("حجم_الخط=" + arabicDigits((int) fontSizeSp));
                        hasSize = true;
                    } else if ("ابقاء_الشاشة".equals(key)) {
                        lines.add("ابقاء_الشاشة=" + (keepScreenOn ? "نعم" : "لا"));
                        hasKeep = true;
                    } else if ("اظهار_صف_المفاتيح".equals(key)) {
                        lines.add("اظهار_صف_المفاتيح=" + (showKeyRowPref ? "نعم" : "لا"));
                        hasRow = true;
                    } else if ("المظهر".equals(key)) {
                        lines.add("المظهر=" + (lightTheme ? "فاتح" : "داكن"));
                        hasTheme = true;
                    } else {
                        lines.add(line);
                    }
                }
            } catch (IOException ignored) {
            }
        }
        if (!hasSize) lines.add("حجم_الخط=" + arabicDigits((int) fontSizeSp));
        if (!hasKeep) lines.add("ابقاء_الشاشة=" + (keepScreenOn ? "نعم" : "لا"));
        if (!hasRow) lines.add("اظهار_صف_المفاتيح=" + (showKeyRowPref ? "نعم" : "لا"));
        if (!hasTheme) lines.add("المظهر=" + (lightTheme ? "فاتح" : "داكن"));
        StringBuilder sb = new StringBuilder();
        for (String l : lines) sb.append(l).append('\n');
        try {
            writeFile(f, sb.toString());
        } catch (IOException ignored) {
        }
    }

    /** عدد من نص عربي أو غربي («١٦» أو «16»). */
    private static int arabicToInt(String s) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c >= '٠' && c <= '٩') sb.append((char) ('0' + (c - '٠')));
            else if (c >= '0' && c <= '9') sb.append(c);
        }
        if (sb.length() == 0) return -1;
        try {
            return Integer.parseInt(sb.toString());
        } catch (NumberFormatException e) {
            return -1;
        }
    }

    private void saveSettings() {
        if (prefs != null) {
            prefs.edit()
                    .putFloat(PREF_FONT_SIZE, fontSizeSp)
                    .putBoolean(PREF_KEEP_SCREEN, keepScreenOn)
                    .apply();
        }
        saveNivaProperties();
    }

    /** حجم الخط (بند 3): قصّ [10، 32]، تطبيق فوري،
     * وحفظ في الإعدادات والملف معا. */
    private void setFontSize(float size) {
        fontSizeSp = Math.max(MIN_FONT_SP, Math.min(MAX_FONT_SP, size));
        if (terminalView != null) terminalView.setTextSize(fontSizeSp);
        saveSettings();
    }

    /** إبقاء الشاشة مضاءة (بند 4). */
    private void setKeepScreenOn(boolean on) {
        keepScreenOn = on;
        applyKeepScreenFlag();
        saveSettings();
    }

    private void applyKeepScreenFlag() {
        if (keepScreenOn) {
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        } else {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        }
    }

    /** إظهار صف المفاتيح (بند 9). */
    private void setShowKeyRow(boolean show) {
        showKeyRowPref = show;
        if (keyRow != null) {
            keyRow.setVisibility((show && isKeyboardVisible())
                    ? View.VISIBLE : View.GONE);
        }
        saveSettings();
    }

    /** تبديل المظهر الفاتح/الداكن (بند 9). */
    private void setLightTheme(boolean light) {
        lightTheme = light;
        applyThemeColors();
        saveSettings();
    }

    // ---------- قائمة العنوان (الضغطة الطويلة) ----------

    /** قائمة الإعدادات والإجراءات من الضغطة الطويلة على
     * عنوان الشريط العلوي (بند 4 + 5 + 7 + 9 + 10). */
    private void showTopMenu() {
        if (titleView == null) return;
        PopupMenu popup = new PopupMenu(this, titleView);
        Menu menu = popup.getMenu();
        MenuItem keep = menu.add(Menu.NONE, 1, Menu.NONE, "إبقاء الشاشة مضاءة");
        keep.setCheckable(true);
        keep.setChecked(keepScreenOn);
        MenuItem theme = menu.add(Menu.NONE, 2, Menu.NONE, "المظهر الفاتح");
        theme.setCheckable(true);
        theme.setChecked(lightTheme);
        MenuItem row = menu.add(Menu.NONE, 3, Menu.NONE, "إظهار صف المفاتيح");
        row.setCheckable(true);
        row.setChecked(showKeyRowPref);
        menu.add(Menu.NONE, 4, Menu.NONE, "تكبير الخط");
        menu.add(Menu.NONE, 5, Menu.NONE, "تصغير الخط");
        menu.add(Menu.NONE, 6, Menu.NONE, "مشاركة نص الجلسة");
        menu.add(Menu.NONE, 7, Menu.NONE, "اختر رابطا");
        menu.add(Menu.NONE, 8, Menu.NONE, "كتابة برنامج");
        popup.setOnMenuItemClickListener(item -> {
            int id = item.getItemId();
            if (id == 1) {
                setKeepScreenOn(!keepScreenOn);
                return true;
            }
            if (id == 2) {
                setLightTheme(!lightTheme);
                return true;
            }
            if (id == 3) {
                setShowKeyRow(!showKeyRowPref);
                return true;
            }
            if (id == 4) {
                setFontSize(fontSizeSp + 2);
                return true;
            }
            if (id == 5) {
                setFontSize(fontSizeSp - 2);
                return true;
            }
            if (id == 6) {
                shareTranscript();
                return true;
            }
            if (id == 7) {
                showUrlDialog();
                return true;
            }
            if (id == 8) {
                showCompose();
                return true;
            }
            return false;
        });
        popup.show();
    }

    // ---------- التحديد والنسخ والمشاركة (بند 1) ----------

    /** النص المحدد حاليا (بفواصل الأسطر كما هي). */
    private String selectedText() {
        Editable text = terminalView.getText();
        if (text == null) return "";
        int start = terminalView.getSelectionStart();
        int end = terminalView.getSelectionEnd();
        if (start < 0 || end < 0 || start == end) return "";
        int lo = Math.max(0, Math.min(start, end));
        int hi = Math.min(text.length(), Math.max(start, end));
        if (lo >= hi) return "";
        return text.subSequence(lo, hi).toString();
    }

    /** نسخ النص المحدد إلى الحافظة (بند 1). */
    private void copySelection() {
        String sel = selectedText();
        if (sel.isEmpty()) return;
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null) return;
        cm.setPrimaryClip(ClipData.newPlainText("نيفا", sel));
        Toast.makeText(this, "تم النسخ", Toast.LENGTH_SHORT).show();
    }

    /** مشاركة النص المحدد فقط (بند 1 — تأتي مع النسخ). */
    private void shareSelection() {
        String sel = selectedText();
        if (sel.isEmpty()) return;
        shareText(sel);
    }

    /** مشاركة نص الجلسة كله (بند 5). */
    private void shareTranscript() {
        Session s = active();
        shareText(s.text.toString());
    }

    private void shareText(String text) {
        if (text == null || text.isEmpty()) return;
        Intent send = new Intent(Intent.ACTION_SEND);
        send.setType("text/plain");
        send.putExtra(Intent.EXTRA_TEXT, text);
        try {
            startActivity(Intent.createChooser(send, "مشاركة نص الجلسة"));
        } catch (Exception ignored) {
        }
    }

    // ---------- اللصق (بند 2) ----------

    /** لصق من الحافظة: السطر الأول في سطر الإدخال الحالي،
     * والباقي في الطابور (سطر لكل موجّه). */
    private void pasteFromClipboard() {
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null || !cm.hasPrimaryClip()) return;
        ClipData clip = cm.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) return;
        CharSequence t = clip.getItemAt(0).coerceToText(this);
        if (t == null) return;
        pasteText(t.toString());
    }

    /** لصق نص (من الحافظة أو من لوحة الكتابة): السطر
     * الأول يُدرج عند المؤشر ويُرسَل فورا، والباقي
     * يُصفّ في الطابور. */
    private void pasteText(String text) {
        if (text == null || text.isEmpty()) return;
        Session s = active();
        String[] lines = text.split("\n", -1);
        if (terminalView.inputStart >= 0) {
            // السطر الأول عند المؤشر (يحترم inputStart
            // عبر insertAtCursor)، ثم يُرسَل كسطر مكتوب.
            insertAtCursor(lines[0]);
            submitLine();
            for (int i = 1; i < lines.length; i++) {
                if (s.pasteQueue.size() >= PASTE_QUEUE_CAP) break;
                s.pasteQueue.add(lines[i]);
            }
            if (lines.length - 1 > PASTE_QUEUE_CAP) {
                Toast.makeText(this, "النص الملصوق طويل جدا",
                        Toast.LENGTH_SHORT).show();
            }
        } else {
            // المحرك مشغول: كل الأسطر إلى الطابور، وتُرسَل
            // مع وصول الموجّه.
            for (String line : lines) {
                if (s.pasteQueue.size() >= PASTE_QUEUE_CAP) break;
                s.pasteQueue.add(line);
            }
        }
    }

    // ---------- الروابط (بند 7) ----------

    /** كل الروابط في نص ما، بلا تكرار، مع قصّ علامات
     * الترقيم اللاحقة (من تنفيذنا، لا نسخ من Termux). */
    private static List<String> extractUrls(String text) {
        List<String> out = new ArrayList<>();
        if (text == null) return out;
        Matcher m = URL_PATTERN.matcher(text);
        while (m.find()) {
            String url = cleanUrl(m.group());
            if (!url.isEmpty() && !out.contains(url)) out.add(url);
        }
        return out;
    }

    /** قصّ علامات الترقيم والأقواس من نهاية الرابط. */
    private static String cleanUrl(String url) {
        int end = url.length();
        while (end > 0 && URL_TRAILING.indexOf(url.charAt(end - 1)) >= 0) {
            end--;
        }
        return url.substring(0, end);
    }

    /** الرابط عند موضع لمس (x, y) في الترمنال، أو null. */
    private String findUrlAtTap(float x, float y) {
        Editable text = terminalView.getText();
        if (text == null) return null;
        int offset = terminalView.getOffsetForPosition(x, y);
        if (offset < 0) return null;
        Matcher m = URL_PATTERN.matcher(text.toString());
        while (m.find()) {
            int start = m.start();
            int end = m.start() + cleanUrl(m.group()).length();
            if (offset >= start && offset < end) {
                return cleanUrl(m.group());
            }
        }
        return null;
    }

    /** نقرة على رابط في السجل تفتحه (بند 7)؛ نقرة في
     * سطر الإدخال ليست رابطا (تعيد false). */
    private boolean openUrlAtTap(float x, float y) {
        if (terminalView.inputStart < 0) return false;
        String url = findUrlAtTap(x, y);
        if (url == null) return false;
        int offset = terminalView.getOffsetForPosition(x, y);
        if (offset < 0 || offset >= terminalView.inputStart) return false;
        openUrl(url);
        return true;
    }

    private void openUrl(String url) {
        try {
            Intent view = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
            startActivity(view);
        } catch (Exception e) {
            Toast.makeText(this, "تعذر فتح الرابط", Toast.LENGTH_SHORT).show();
        }
    }

    /** حوار «اختر رابطا» (بند 7): كل روابط الجلسة. */
    private void showUrlDialog() {
        Session s = active();
        final List<String> urls = extractUrls(s.text.toString());
        if (urls.isEmpty()) {
            Toast.makeText(this, "لا توجد روابط في هذه الجلسة",
                    Toast.LENGTH_SHORT).show();
            return;
        }
        new AlertDialog.Builder(this)
                .setTitle("اختر رابطا")
                .setItems(urls.toArray(new CharSequence[0]),
                        (d, which) -> openUrl(urls.get(which)))
                .setNegativeButton("إلغاء", null)
                .show();
    }

    // ---------- لوحة كتابة البرنامج (بند 10) ----------

    /** لوحة الكتابة: حقل تحرير حر (أسطر متعددة، مؤشر حر)
     * فوق الترمنال، مع زرّي إرسال وإغلاق. النص يبقى عند
     * الإغلاق، ويُرسَل عبر طابور اللصق نفسه. */
    private void buildComposePanel(FrameLayout frame) {
        LinearLayout panel = new LinearLayout(this);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        panel.setBackgroundColor(colorPanel);
        panel.setPadding(16, 16, 16, 16);
        EditText edit = new EditText(this);
        composeEdit = edit;
        edit.setBackgroundColor(colorBg);
        edit.setTextColor(colorText);
        edit.setTextSize(fontSizeSp);
        edit.setTextDirection(View.TEXT_DIRECTION_RTL);
        edit.setGravity(Gravity.TOP | Gravity.START);
        edit.setInputType(InputType.TYPE_CLASS_TEXT
                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        edit.setHint("برنامج نيفا");
        LinearLayout.LayoutParams editParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f);
        panel.addView(edit, editParams);
        LinearLayout buttons = new LinearLayout(this);
        buttons.setOrientation(LinearLayout.HORIZONTAL);
        buttons.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        Button sendBtn = new Button(this);
        sendBtn.setText("إرسال");
        sendBtn.setTextColor(colorText);
        sendBtn.setBackgroundColor(colorBtn);
        sendBtn.setOnClickListener(v -> sendCompose());
        buttons.addView(sendBtn);
        Button closeBtn = new Button(this);
        closeBtn.setText("إغلاق");
        closeBtn.setTextColor(colorText);
        closeBtn.setBackgroundColor(colorBtn);
        closeBtn.setOnClickListener(v -> hideCompose());
        buttons.addView(closeBtn);
        panel.addView(buttons, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));
        panel.setVisibility(View.GONE);
        composePanel = panel;
        frame.addView(panel, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
    }

    private void showCompose() {
        if (composePanel == null) return;
        composePanel.setVisibility(View.VISIBLE);
        composeEdit.requestFocus();
        showKeyboard();
    }

    private void hideCompose() {
        if (composePanel == null) return;
        composePanel.setVisibility(View.GONE);
        terminalView.requestFocus();
        showKeyboard();
    }

    private void sendCompose() {
        if (composeEdit == null) return;
        String text = composeEdit.getText().toString();
        hideCompose();
        pasteText(text);
    }

    // ---------- الشبكة: قفل الاستيقاظ والانقطاع (بند 11 + 12) ----------

    /** هل الجهاز متصل بشبكة (فحص في التطبيق، لا المحرك)؟ */
    private boolean isOnline() {
        if (connectivityManager == null) return false;
        try {
            android.net.Network net = connectivityManager.getActiveNetwork();
            if (net == null) return false;
            NetworkCapabilities caps =
                    connectivityManager.getNetworkCapabilities(net);
            return caps != null && caps.hasCapability(
                    NetworkCapabilities.NET_CAPABILITY_INTERNET);
        } catch (Exception e) {
            return false;
        }
    }

    /** هل الاتصال الحالي واي-فاي؟ (لقفل الواي-فاي مع الجلب). */
    private boolean isWifi() {
        if (connectivityManager == null) return false;
        try {
            android.net.Network net = connectivityManager.getActiveNetwork();
            if (net == null) return false;
            NetworkCapabilities caps =
                    connectivityManager.getNetworkCapabilities(net);
            return caps != null && caps.hasTransport(
                    NetworkCapabilities.TRANSPORT_WIFI);
        } catch (Exception e) {
            return false;
        }
    }

    /** أخذ قفل الاستيقاظ عند بدء جلب (بند 11): جزئي +
     * واي-فاي إن كان الاتصال واي-فاي؛ تلقائي ومقصور على
     * الجلب — بلا زر يدوي وبلا قفل دائم. مهلة 60 ثانية
     * احتياطا (مهلة المحرك نفسه 30 ثانية). */
    private void acquireFetchWakeLock(Session s) {
        s.fetchWakeHeld = true;
        s.lastFetchError = false;
        try {
            PowerManager pm = (PowerManager)
                    getSystemService(Context.POWER_SERVICE);
            if (pm != null) {
                if (fetchWakeLock == null) {
                    fetchWakeLock = pm.newWakeLock(
                            PowerManager.PARTIAL_WAKE_LOCK, "niva:fetch");
                }
                if (!fetchWakeLock.isHeld()) {
                    fetchWakeLock.acquire(60_000);
                }
            }
            if (isWifi()) {
                WifiManager wm = (WifiManager) getApplicationContext()
                        .getSystemService(Context.WIFI_SERVICE);
                if (wm != null) {
                    if (fetchWifiLock == null) {
                        // FULL_LOW_LATENCY من API 29؛ قبله
                        // الوضع الكامل العادي.
                        int mode = Build.VERSION.SDK_INT
                                >= Build.VERSION_CODES.Q
                                ? WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                                : WifiManager.WIFI_MODE_FULL;
                        fetchWifiLock = wm.createWifiLock(mode, "niva:fetch");
                    }
                    if (!fetchWifiLock.isHeld()) fetchWifiLock.acquire();
                }
            }
        } catch (Exception ignored) {
        }
        refreshServiceNotification();
    }

    /** تحرير قفل الجلب حين لا جلب جارٍ في أي جلسة. */
    private void releaseFetchWakeLockIfIdle() {
        for (Session s : sessions) {
            if (s.fetchWakeHeld) return;
        }
        try {
            if (fetchWakeLock != null && fetchWakeLock.isHeld()) {
                fetchWakeLock.release();
            }
            if (fetchWifiLock != null && fetchWifiLock.isHeld()) {
                fetchWifiLock.release();
            }
        } catch (Exception ignored) {
        }
        refreshServiceNotification();
    }

    /** هل أي قفل استيقاظ ممسوك (جلب أو يدوي)؟ للإشعار. */
    private boolean isAnyWakeHeld() {
        for (Session s : sessions) {
            if (s.fetchWakeHeld) return true;
        }
        if (fetchWakeLock != null && fetchWakeLock.isHeld()) return true;
        return NivaService.isManualWakeHeld();
    }

    // ---------- الخدمة الأمامية (بند 13) ----------

    /** تحديث إشعار الخدمة بعدد الجلسات وحالة القفل. */
    private void refreshServiceNotification() {
        try {
            Intent intent = new Intent(this, NivaService.class);
            intent.setAction(NivaService.ACTION_REFRESH);
            intent.putExtra("sessions", sessions.size());
            intent.putExtra("wake_held", isAnyWakeHeld());
            startService(intent);
        } catch (Exception ignored) {
        }
    }
}
