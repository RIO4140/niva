package com.rio4140.niva;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.DownloadManager;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ActivityNotFoundException;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.UriPermission;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.database.Cursor;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.Settings;
import android.speech.RecognitionListener;
import android.speech.RecognizerIntent;
import android.speech.SpeechRecognizer;
import android.speech.tts.TextToSpeech;
import android.speech.tts.UtteranceProgressListener;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraManager;
import android.location.Location;
import android.location.LocationListener;
import android.location.LocationManager;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.media.MediaMetadataRetriever;
import android.media.MediaPlayer;
import android.media.MediaRecorder;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.net.wifi.ScanResult;
import android.net.wifi.WifiInfo;
import android.net.wifi.WifiManager;
import android.os.BatteryManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.os.PersistableBundle;
import android.os.PowerManager;
import android.os.SystemClock;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.text.Editable;
import android.text.InputFilter;
import android.text.InputType;
import android.text.Spanned;
import android.text.TextWatcher;
import android.text.style.ForegroundColorSpan;
import android.view.ActionMode;
import android.view.Gravity;
import android.view.HapticFeedbackConstants;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewTreeObserver;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.webkit.MimeTypeMap;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.DatePicker;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ListView;
import android.widget.NumberPicker;
import android.widget.PopupMenu;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.TimePicker;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.ByteBuffer;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Calendar;
import java.util.Collections;
import java.util.Date;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.TimeZone;
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
 * مع سجلها ومحفوظاتها. والشبكة تلقائية في كل الجلسات
 * (أمر المالك 2026-11-06) — كما في Termux، بلا سؤال
 * إذن وبلا تفعيل يدوي لكل جلسة.
 */
public class MainActivity extends Activity {

    private static final String CONT_PROMPT_TEXT = "> ";
    private static final String ENDED_MESSAGE = "انتهت الجلسة.";

    /** سقف السجل (تدقيق م-1): آخر 2000 سطر / 256KB —
     * والسقف السطري قابل للضبط من niva.properties
     * (v1.1.6 بند 12) بحد أدنى 100 وأقصى 50000. */
    private static final int MAX_LINES = 2000;
    private static final int MIN_TRANSCRIPT_LINES = 100;
    private static final int MAX_TRANSCRIPT_LINES = 50000;
    private static final int MAX_CHARS = 256 * 1024;
    /** عمر ملف السجل الذي يُحذف تلقائيا عند البدء
     * (v1.1.6 بند 11): 14 يوما بلا جلسة حية. */
    private static final long OLD_LOG_MILLIS = 14L * 24 * 60 * 60 * 1000;
    /** سقف سطر الإدخال (تدقيق م-10). */
    private static final int MAX_LINE_CHARS = 64 * 1024;
    /** سقف الجلسات (تدقيق م-8). */
    private static final int MAX_SESSIONS = 8;

    /** أمر التحكم في الشبكة داخل المحرك (تدقيق م-2): يقلب
     * علم الشبكة في العملية نفسها — بلا إعادة تشغيل وبلا
     * فقدان حالة الجلسة. */
    private static final String ALLOW_NETWORK_COMMAND = ":اسمح-بالشبكة";
    /** بادئة أمر كشف التنزيلات داخل المحرك (الدفعة 5 —
     * البندان 94 و96): تضبط جذر التنزيلات للقراءة فقط
     * في العملية نفسها — بلا إعادة تشغيل وبلا فقدان
     * حالة الجلسة. */
    private static final String SET_DOWNLOADS_PREFIX = ":تنزيلات ";
    /** قناة تقارير الأعطال الصامتة (الدفعة 5 — البند
     * 107): أهمية منخفضة — فلا صوت ولا اهتزاز ولا
     * ظهور فوق التطبيق؛ ومعرّف نظام ثابت واحد
     * (فالتقرير الأحدث يحل محل السابق). */
    private static final String CRASH_CHANNEL_ID = "niva_crash";
    private static final int CRASH_NOTIFICATION_ID = 9101;

    /** بادئة إشارات التحكم على stderr (تدقيق م-6): سطر
     * يبدأ بهذا الحرف لا يظهر أبدا للمستخدم، ولا يمكن
     * لبرنامج نيفا طباعته. */
    private static final String CONTROL_PREFIX = "\u0001نيفا-تحكم:";

    /** بادئة نتائج الحوار على stdin (المهمة 20): سطر
     * يبدأ بها يقرؤه المحرك نتيجة حوار — قيمة أو إلغاء
     * أو خطأ — فلا تُكتب أبدا في الترمنال. (وتُعاد
     * لنتيجة التنبيه في المهمة 21 — البادئة نفسها.) */
    private static final String DIALOG_RESULT_PREFIX = "\u0001نيفا-نتيجة:";

    /** قناة تنبيهات برامج نيفا (المهمة 21) — تُنشأ
     * تلقائيا عند أول نبّه؛ معرّف مختلف عن قناة
     * الخدمة الأمامية فلا تتداخل الأهميتان. */
    private static final String NOTIFY_CHANNEL_ID = "niva_notify";
    /** رمز طلب إذن الإشعارات (أندرويد 13+). */
    private static final int REQ_NOTIFY_PERMISSION = 2101;
    /** رموز طلبات أذونات الدفعة 4أ: الموقع (آخر
     * ومباشر)، الكاميرا (التقاط)، ومسح واي فاي. */
    private static final int REQ_LOCATION_PERMISSION = 2103;
    private static final int REQ_CAPTURE_PERMISSION = 2104;
    private static final int REQ_WIFI_SCAN_PERMISSION = 2106;
    /** رموز طلبات أذونات الدفعة 4ب: الكاميرا
     * (المصباح)، والميكروفون (التسجيل). */
    private static final int REQ_TORCH_PERMISSION = 2105;
    private static final int REQ_RECORD_PERMISSION = 2107;
    /** رموز نتائج نوايا الدفعة 4أ: منتقي ملف عام،
     * والتقاط صورة. */
    private static final int REQ_PICK_FILE = 3103;
    private static final int REQ_CAPTURE_PHOTO = 3104;
    /** رموز نتائج نوايا الدفعة 5 (البنود 94 و96 و98
     * و99 خطوة 1): قراءة مستند، وعرض مجلد، وشاشة
     * إعدادات إذن التخزين. */
    private static final int REQ_READ_DOCUMENT = 3105;
    private static final int REQ_LIST_FOLDER = 3106;
    private static final int REQ_STORAGE_SETTINGS = 3107;
    /** سقف قراءة المستند (البند 99 خطوة 1) — كما
     * سقف منتقي الملفات العام: 5 ميغابايت. */
    private static final int READ_DOCUMENT_CAP = 5 * 1024 * 1024;
    /** مهلة قراءة الموقع المباشرة ومسح واي فاي (10
     * ثوان — كما مهلة الحساس 5 ثوان: كل طلب يجيب
     * بنتيجة واحدة بالضبط، فلا يعلق المحرك أبدا). */
    private static final int DEVICE_LOCATION_TIMEOUT_MS = 10_000;
    private static final int DEVICE_WIFI_SCAN_TIMEOUT_MS = 10_000;
    /** سقف نسخ منتقي الملفات العام (البند 28) — كما
     * سقف SAF: 5 ميغابايت. */
    private static final int PICK_FILE_CAP = 5 * 1024 * 1024;
    /** سقف نص الجلسة المشارك (البند 62 — الدفعة 4ب):
     * آخر 100 ألف حرف فقط، بلا تجاوز حد المعاملة. */
    private static final int SESSION_SHARE_CAP = 100_000;
    /** سقف نص الحافظة المكتوب من برنامج (H5): 100
     * ألف حرف — بلا تجاوز حد معاملة بايندر. */
    private static final int CLIPBOARD_CAP = 100_000;

    // ===== خطوة ملفات .نيفا (المهمة 28، CATALOG-19) =====
    /** رمزا منتقيي SAF: المنح والتشغيل. */
    private static final int REQ_SAF_GRANT = 3101;
    private static final int REQ_SAF_RUN = 3102;
    /** سقف الشجرات الممنوحة (CATALOG-19 §3). */
    private static final int MAX_GRANTS = 64;
    /** سقف قراءة SAF (نص UTF-8 — CATALOG-19 §2). */
    private static final int SAF_READ_CAP = 5 * 1024 * 1024;
    /** مجلد الملفات المؤقتة داخل مجلد التطبيق. */
    private static final String TEMP_DIR_NAME = "tmp";

    /** تنبيه معلّق بانتظار إذن الإشعارات (المهمة 21):
     * الجلسة + معرّف المحرك + العنوان + النص. */
    private static class PendingNotify {
        final Session session;
        final long id;
        final String title;
        final String text;

        PendingNotify(Session s, long i, String t, String x) {
            session = s;
            id = i;
            title = t;
            text = x;
        }
    }

    /** شجرة ممنوحة (المهمة 28 — CATALOG-19 §3):
     * معرّف رتيب من 1 + الاسم + عنوان الشجرة. */
    private static class Grant {
        final long id;
        final String name;
        final Uri uri;

        Grant(long i, String n, Uri u) {
            id = i;
            name = n;
            uri = u;
        }
    }

    /** سجل الشجرات الممنوحة (ملك التطبيق — يبقى عبر
     * الجلسات؛ والإبطال من إعدادات النظام فقط). */
    private final ArrayList<Grant> grants = new ArrayList<>();
    /** معرّف الشجرة التالي (رتيب من 1 — لا يُعاد
     * استخدامه أبدا في عمر التطبيق). */
    private long nextGrantId = 1;
    /** الجلسة المنتظرة لنتيجة منتقي المنح. */
    private Session safGrantSession = null;

    /** طلب قراءة حساس معلّق (المهمة 25): الجلسة +
     * المستمع + المهلة + علامة الإجابة (مرة واحدة
     * بالضبط — حدث أو مهلة، أيهما أولا). */
    private static class PendingSensor {
        final Session session;
        SensorEventListener listener;
        Runnable timeout;
        boolean answered;

        PendingSensor(Session s) {
            session = s;
            answered = false;
        }
    }

    /** طابور التنبيهات المعلّقة بإذن (قد يضم جلسات
     * عدة — الإذن على مستوى النشاط). */
    private final ArrayList<PendingNotify> pendingNotifies = new ArrayList<>();
    /** طلب إذن إشعارات جارٍ (فلا حوار ثان فوق الأول). */
    private boolean notifyPermissionRequestInFlight = false;
    /** عدّاد معرّفات نظام التنبيهات — فريد عبر كل
     * الجلسات (معرّف المحرك لكل عملية على حدة فقد
     * يتكرر بين جلستين؛ ومعرّف النظام لا يتكرر أبدا). */
    private int nextSystemNotifyId = 1;

    /** نطق جهاز.انطق + محركات_النطق (المهمة 23):
     * كائن TextToSpeech واحد على مستوى النشاط
     * (تهيئة كسولة عند أول طلب) — لا واحد لكل جلسة. */
    private TextToSpeech tts;
    /** بدأت تهيئة TTS (فلا إنشاء ثان). */
    private boolean ttsInitStarted = false;
    /** اكتملت تهيئة TTS بنجاح. */
    private boolean ttsReady = false;
    /** فشلت تهيئة TTS (فلا إعادة محاولة صامتة). */
    private boolean ttsInitFailed = false;
    /** الصوت العربي متاح في المحرك الحالي. */
    private boolean ttsArabicOk = false;
    /** طلب نطق وصل قبل اكتمال التهيئة (المهمة 23):
     * الجلسة + النص. */
    private static class PendingSpeak {
        final Session session;
        final String text;

        PendingSpeak(Session s, String t) {
            session = s;
            text = t;
        }
    }

    /** طابور طلبات النطق المعلّقة بالتهيئة. */
    private final ArrayList<PendingSpeak> pendingSpeaks = new ArrayList<>();
    /** النطق الجاري الواحد في كل التطبيق (لا نطق
     * ثانٍ متزامن — انظر CATALOG-15). */
    private PendingSpeak speakingNow = null;
    /** معرّف الملفوظ الجاري (لتمييز الردود القديمة). */
    private String speakingUtteranceId = null;
    /** عدّاد معرّفات الملفوظات — رتيب. */
    private int nextSpeakUtterance = 1;

    /** تشغيل صوت جهاز.شغّل_صوت + أوقف_صوت (المهمة 24):
     * مشغّل MediaPlayer واحد على مستوى النشاط (تهيئة
     * كسولة عند أول تشغيل) — لا واحد لكل جلسة؛
     * والجلسة صاحبة الصوت الجاري (واحدة على الأكثر)
     * — وتشغيل جديد يوقف الجاري ويستبدله. */
    private MediaPlayer audioPlayer;
    /** الجلسة صاحبة الصوت الجاري (null إن لم يكن
     * شيء يشتغل). */
    private Session audioPlayingSession = null;

    /** قراءة حساس جهاز.حساس (المهمة 25):
     * مدير حساسات كسول + قائمة طلبات القراءة
     * المعلقة (كل طلب بمستمعه ومهلته وعلامة
     * إجابته وجلسته — وعادة عنصر واحد؛ وطلب ثانٍ
     * مستمع مستقل لا يرفض). */
    private SensorManager sensorManager;
    private final ArrayList<PendingSensor> pendingSensors = new ArrayList<>();

    /** استماع جهاز.اسمع (المهمة 26):
     * رمز طلب إذن الميكروفون. */
    private static final int REQ_LISTEN_PERMISSION = 2102;

    /** طلب استماع معلّق (المهمة 26): الجلسة فقط —
     * الخانة الواحدة للحالات الثلاث (إفصاح معروض /
     * إذن مطلوب / استماع جارٍ). */
    private static class PendingListen {
        final Session session;

        PendingListen(Session s) {
            session = s;
        }
    }

    /** كائن التعرف الصوتي (كسول — يُنشأ عند أول
     * استماع)؛ والاستماع الجاري الواحد في كل
     * التطبيق (لا استماع ثانٍ متزامن — انظر
     * CATALOG-18)؛ وطلب إذن ميكروفون جارٍ (فلا
     * حوار ثان فوق الأول)؛ وقبول الإفصاح مرة في
     * كل تشغيل؛ ومرجع حوار الإفصاح المعروض. */
    private SpeechRecognizer speechRecognizer;
    private PendingListen listeningNow = null;
    private boolean listenPermissionRequestInFlight = false;
    private boolean speechDisclosureAccepted = false;
    private AlertDialog listenDisclosureDialog = null;

    /** واجهة الجهاز من البرنامج (الدفعة 4أ — البنود
     * 22–34): طلب موقع معلّق واحد (آخر أو مباشر)
     * بمستمعه ومهلته وعلامة إجابته وجلسته؛ وطلب
     * ثانٍ يُجاب بخطأ «طلب آخر جارٍ» بدلا من طابور
     * — فكل طلب يجيب بنتيجة واحدة بالضبط، فلا يعلق
     * المحرك أبدا. وطلب إذن موقع جارٍ (فلا حوار ثان
     * فوق الأول). */
    private static class PendingLocation {
        final Session session;
        final boolean once;
        LocationListener listener;
        Handler handler;
        Runnable timeout;
        boolean answered;

        PendingLocation(Session s, boolean o) {
            session = s;
            once = o;
            answered = false;
        }
    }

    private PendingLocation pendingLocation = null;
    private boolean locationPermissionRequestInFlight = false;

    /** مسح واي فاي (البند 34): طلب واحد بمستقبله
     * ومهلته وعلامة إجابته وجلسته؛ وطلب إذن مسح
     * جارٍ (فلا حوار ثان فوق الأول). */
    private static class PendingWifiScan {
        final Session session;
        BroadcastReceiver receiver;
        Handler handler;
        Runnable timeout;
        boolean answered;

        PendingWifiScan(Session s) {
            session = s;
            answered = false;
        }
    }

    private PendingWifiScan pendingWifiScan = null;
    private boolean wifiScanPermissionRequestInFlight = false;

    /** منتقي ملف عام (البند 28): الجلسة الطالبة
     * فقط — طلب واحد في كل التطبيق. */
    private Session pickFileSession = null;

    /** الدفعة 5 (البنود 94 و96 و98 و99 خطوة 1):
     * قراءة مستند وعرض مجلد لمرة واحدة — الجلسة
     * الطالبة فقط، طلب واحد من كل نوع في كل التطبيق؛
     * وجلسة «اكشف التخزين» المعلقة بانتظار جواب
     * شاشة إعدادات الإذن. */
    private Session readDocumentSession = null;
    private Session listFolderSession = null;
    private Session pendingStorageSession = null;

    /** هل إذن إدارة كل الملفات ممنوح (أندرويد 11+
     * فقط — دونه لا وجود للإذن أصلا)؟ */
    private static boolean storageManagerGranted() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return false;
        return Environment.isExternalStorageManager();
    }

    /** مسار مجلد التنزيلات المشترك — يُمرَّر للمحرك
     * براية --تنزيلات أو بأمر :تنزيلات. */
    private static String downloadsPath() {
        return Environment.getExternalStoragePublicDirectory(
                Environment.DIRECTORY_DOWNLOADS).getAbsolutePath();
    }

    /** التقاط صورة (البند 32): الجلسة الطالبة +
     * ملف الصورة الهدف؛ وطلب إذن كاميرا جارٍ (فلا
     * حوار ثان فوق الأول). */
    private Session pendingCaptureSession = null;
    private File pendingCaptureFile = null;
    private boolean capturePermissionRequestInFlight = false;

    /** واجهة الجهاز من البرنامج (الدفعة 4ب — البنود
     * 61 و62 و66–72 و79 و105 و106 و119):
     * - إيقاف مؤقت للصوت (البند 66): علم واحد — فلا
     * خانة معلّقة؛ والتشغيل الجديد والإيقاف يصفّرانه
     * عبر releaseAudio.
     * - توست واحد محتفظ به (البند 106 — قاعدة
     * التوست 196-1: إلغاء السابق قبل عرض الجديد).
     * - مصباح (البند 72): الجلسة المشغّلة + الحالة
     * + معرّف الكاميرا؛ وطلب إذن كاميرا جارٍ (فلا
     * حوار ثان فوق الأول)؛ وموت الجلسة المشغّلة
     * يطفئ المصباح.
     * - تسجيل صوت (البند 79): مسجّل واحد + الجلسة
     * + الملف + المدة + مهلة إيقاف تلقائي؛ وطلب
     * إذن ميكروفون جارٍ (فلا حوار ثان فوق الأول)؛
     * وحوار تنبيه «يُحفظ محليا فقط» قبل أول طلب
     * إذن؛ وموت الجلسة يوقف التسجيل صامتا. */
    private boolean audioPaused = false;
    private Toast currentToast = null;
    private Session torchSession = null;
    private boolean torchOn = false;
    private String torchCameraId = null;
    private boolean torchPermissionRequestInFlight = false;

    private static class PendingTorch {
        final Session session;
        final boolean on;

        PendingTorch(Session s, boolean o) {
            session = s;
            on = o;
        }
    }

    private PendingTorch pendingTorch = null;

    private static class PendingRecord {
        final Session session;
        final File file;
        final int seconds;

        PendingRecord(Session s, File f, int sec) {
            session = s;
            file = f;
            seconds = sec;
        }
    }

    private MediaRecorder audioRecorder = null;
    private Session recordingSession = null;
    private File recordingFile = null;
    private Handler recordingTimeoutHandler = null;
    private Runnable recordingTimeout = null;
    private boolean recordPermissionRequestInFlight = false;
    private PendingRecord pendingRecord = null;
    private AlertDialog recordDisclosureDialog = null;

    /** إعدادات محفوظة (SharedPreferences) + ملف
     * niva.properties في مجلد المحرك (قابل للتحرير من
     * الترمنال نفسه؛ الملف يغلب عند التعارض). */
    private static final String PREFS_NAME = "niva";
    private static final String PREF_FONT_SIZE = "font_size_sp";
    private static final String PREF_KEEP_SCREEN = "keep_screen_on";
    private static final String PREF_ACTIVE_SESSION = "active_session";
    private static final String NIVA_PROPS_FILE = "niva.properties";
    /** مجلد البيت تحت مجلد البرنامج (اجتماع 45 §59). */
    private static final String HOME_DIR_NAME = "home";
    /** مجلد الإعدادات الظاهر تحت البيت (اجتماع 45 §59). */
    private static final String SETTINGS_DIR_NAME = "إعدادات";
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
    /** لاحقة ملف التجهيز لتنزيل التحديث (53ب): يُنزَّل
     * الجديد إلى ملف تجهيز، ويُتحقق منه، ثم يُبدَّل مع
     * النهائي — فتنزيل فاشل لا يمس ملفا صالحا أبدا. */
    private static final String UPDATE_STAGING_SUFFIX = ".جديد";
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
    /** ألوان الأسود الخالص (v1.1.7 بند 45): أسود نقي
     * لشاشات AMOLED — الخلفية واللوحة #000000. */
    private static final int BLACK_BG = 0xFF000000;
    private static final int BLACK_TEXT = 0xFFE8E8E8;
    private static final int BLACK_PANEL = 0xFF000000;
    private static final int BLACK_BTN = 0xFF111111;
    private static final int BLACK_ACTIVE = 0xFF1A1A1A;
    private static final int BLACK_PROMPT = 0xFF00E676;
    private static final int ERROR_RED = 0xFFFF5252;
    /** ألوان سطور عرض الملفات (`ات`): المجلد أزرق والرابط
     * الفيروزي سماوي، والتنفيذي أخضر بلون نجاح التثبيت
     * نفسه (promptColor) — مثل ألوان `ls` الافتراضية.
     * الأزرق والسماوي مقروءان على الخلفيتين الداكنة
     * والفاتحة معا. */
    private static final int LS_DIR_BLUE = 0xFF1E88E5;
    private static final int LS_LINK_CYAN = 0xFF00ACC1;
    /** بادئات سطور المخرجات الملوّنة (أمر المالك 2026-11-08):
     * سطر يبدأ بـ«خطأ» أو فيه «تعذّر» فهو خطأ (أحمر)،
     * وسطر يبدأ بـ«ثُبّت» فهو نجاح تثبيت (أخضر)؛ فتُقرأ
     * النتيجة من أول نظرة على خلفية سوداء. الصيغتان
     * بالشدة وبلا شدة معا: المحرك يكتبها أحيانا بلا شدة
     * («تعذر تشغيل المحرك») وأحيانا بها. */
    private static final String ERROR_PREFIX = "خطأ";
    private static final String CANNOT_WITH_SHADDA = "تعذّر";
    private static final String CANNOT_PLAIN = "تعذر";
    private static final String INSTALL_PREFIX = "ثُبّت";
    private static final int SUBTITLE_GRAY_DARK = 0xFF9A9A9A;
    private static final int SUBTITLE_GRAY_LIGHT = 0xFF5A5A5A;

    /** أكواد الاختصارات التي يعالجها التطبيق نفسه (لا المحرك):
     * جلسات / تنظيف الشاشة / مدة الجلسة /
     * إعادة تشغيل الجلسة / تحديث نيفا (v1.1.4).
     * (جد خرج 2026-11-06 بقاعدة بلا-ألف — إنشاء الجلسة
     * من الدرج والقائمة يبقى؛ ومش/شب/سش خرجت 2026-11-08
     * بأمر المالك «اكتب دول كاملين» إلى عباراتها الكاملة:
     * «امسح الشاشة» و«حالة الشبكة» و«اسمح بالشبكة» —
     * انظر appPhraseFor — فبقي من الجدول 3 أكواد تطبيق.) */
    private static final String[] APP_CODES = {
            "جل", "تنظيف", "مد", "اع", "تن"
    };

    /** صف المفاتيح (أمر المالك 2026-11-05 ~23:02): الأكواد
     * بالترتيب — 34 للمحرك + 5 للتطبيق (2026-11-08، أمر
     * المالك «اكتب دول كاملين»: خرجت 13 كودا إلى صيغها
     * الكاملة — بم/بع/مس/مع/اد/اق/حع/مى/ال/كا من المحرك
     * ومش/شب/سش من التطبيق — بلا زر جديد للعبارات
     * الكاملة: الصف للأكواد القصيرة والأسماء الكاملة
     * الموجودة تنظيف/تن فقط). «تن» (تحديث نيفا،
     * v1.1.4) و«تنظيف» (مسح الشاشة بالاسم الكامل، مهمة 12)
     * كودا تطبيق إضافيان خارج الجدول السبعة والثلاثين؛
     * ضغطة زر تُدرج الكود متبوعا بمسافة، ولا تنفذ تلقائيا. */
    private static final String[] KEY_CODES = {
            "ات", "مح", "شم", "حم", "حف", "اف", "شف",
            "عب", "عن", "عس", "عك", "عش", "حج", "شر", "اك",
            "ام", "تم", "عم", "اس", "اص", "رع", "تا", "وق", "نو", "اج", "عع",
            "نب", "اب",
            "جل", "تنظيف", "مد", "اع", "تن"
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
        /** آخر نص محذوف بأزرار الحذف (مفاتيح التحكم
         * المعربة v1.2.1): يستعيده زر «استعد». */
        String killedText = "";
        // الشبكة تلقائية (أمر المالك 2026-11-06 «المفروض
        // الشبكه تكون شغاله بشكل تلقائي زي ترمكس»): كل
        // جلسة جديدة تبدأ مفتوحة، بلا سؤال إذن أول مرة
        // وبلا تفعيل يدوي — الشبكة قدرة جهاز وليست سرا
        // لكل جلسة، كما في Termux. حمايات المحرك
        // (فحص IP والحدود) تبقى كما هي — الذي رُفع
        // بوابة الموافقة في التطبيق فقط.
        boolean networkAllowed = true;
        boolean ended = false;
        /** قُبل إفصاح قراءة الحافظة في هذه الجلسة (H4):
         * أول قراءة من برنامج تطلب موافقة المستخدم،
         * وبعدها تقرأ بقية طلبات الجلسة بلا حوار. */
        boolean clipboardReadDisclosed = false;
        long startTime = System.currentTimeMillis();
        String pendingLine = null;
        /** حوار جهاز.حوار المفتوح حاليا لهذه الجلسة
         * (المهمة 20)؛ null = لا حوار مفتوح. */
        AlertDialog dialog;
        /** طلب حوار معلّق لجلسة خلفية — يُعرض حين
         * تصير الجلسة نشطة (المهمة 20). */
        String pendingDialog;
        /** ملف برنامج .نيفا الذي تشغّله هذه الجلسة
         * (المهمة 28): null = جلسة REPL عادية؛ وإلا
         * جلسة برنامج — ملفها نسخة في مجلد المؤقت،
         * ولا تُعاد عند الاستعادة (الملف ذهب مع
         * الخروج). */
        File programFile;
        /** اسم البرنامج للعرض (ملف .نيفا المختار). */
        String programName;
        /** طلب SAF معلّق لجلسة خلفية (المهمة 28) —
         * يُنفَّذ حين تصير الجلسة نشطة (نمط الحوار). */
        String pendingSaf;
        /** انتظار SAF جارٍ لهذه الجلسة (طلب أُرسل
         * والنتيجة لم تُرسَل بعد) — لالتقاط ضغطة
         * «ألغِ الأمر» في أثنائه. */
        boolean safWaitPending = false;
        /** ضغطة إلغاء وصلت أثناء انتظار SAF (أو قبله
         * بقليل) — نتيجته التالية شاردة فتُسقَط. */
        boolean safCancelArmed = false;
        /** خريطة معرّفات تنبيهات الجلسة (المهمة 21):
         * معرّف المحرك ← معرّف نظام أندرويد الفريد.
         * تُمسح عند موت العملية/إعادة التشغيل (محرك
         * جديد = عدّاد جديد) — والمنشور يبقى في
         * الشريط حتى يمسحه المستخدم. */
        HashMap<Long, Integer> notifySystemIds = new HashMap<>();
        /** السطر معلّق لإعادة إرساله بعد إعادة التشغيل. */
        boolean awaitingRestartResend = false;
        /** إعادة تشغيل جارية (فلا تُطلب ثانية). */
        boolean restarting = false;
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
        /** مسودة لوحة كتابة البرنامج لهذه الجلسة
         * (v1.1.6 بند 13): تُحفظ في ملف الجلسة عند
         * onPause وتُستعاد إن ماتت العملية. */
        String composeDraft = "";
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
        /** يُستدعى عند الخروج البرمجي من وضع التحديد
         * (لإنهاء شريط الإجراءات من النشاط). */
        Runnable onExitSelection;

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

        /** الخروج من وضع التحديد إلى سطر الإدخال (حارس
         * السجل v1.1.5): المؤشر إلى نهاية النص، بلا حذف
         * وبلا إدراج. يُستدعى من مسارات لوحة المفاتيح
         * قبل أي كتابة أو مسح، فينهي شريط الإجراءات عبر
         * onExitSelection أولا. */
        void exitSelectionToInput() {
            selecting = false;
            if (onExitSelection != null) onExitSelection.run();
            int len = getText() == null ? 0 : getText().length();
            setSelection(len, len);
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
                // إصلاح نسخ الأوامر القديمة (مهمة 12): تحديد
                // غير فارغ في السجل (ضغطة طويلة على أمر
                // قديم) يُترك كما هو — شريط التحديد يملكه،
                // وينهار وحده إن لم يكن تحديدا حقيقيا.
                // أما المؤشر المنهار وحده (بلا تحديد) فيُعاد
                // إلى سطر الإدخال، كما كان.
                if (selStart != selEnd) return;
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
                    // الكتابة في وضع التحديد تخرج منه إلى
                    // سطر الإدخال (حارس السجل v1.1.5) — لا
                    // تُدرج أبدا في السجل.
                    if (selecting) exitSelectionToInput();
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
                    if (selecting) exitSelectionToInput();
                    if (text != null && text.toString().contains("\n")) {
                        String clean = text.toString().replace("\n", "");
                        return super.setComposingText(clean, newCursorPosition);
                    }
                    return super.setComposingText(text, newCursorPosition);
                }

                @Override
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                    // حارس السجل (v1.1.5): مسح لوحة المفاتيح
                    // البرمجية يصل هنا مباشرة ويستدعي
                    // Editable.delete — الذي لا يمر على فلتر
                    // الإدخال إطلاقا. فبدون هذا الحارس يأكل
                    // المسحُ والمؤشر عند بداية سطر الإدخال
                    // أحرفَ الموجّه ثم سطر الرد الأخير حرفا
                    // حرفا — وهذا عطل الهاتف المُبلَّغ
                    // (السطر الأخير يُحذف رغم أنه رد).
                    if (selecting) {
                        exitSelectionToInput();
                        return true;
                    }
                    if (inputStart < 0) return true;
                    int selStart = getSelectionStart();
                    int selEnd = getSelectionEnd();
                    if (selStart < 0 || selEnd < 0) return true;
                    if (selStart < inputStart) return true;
                    int allowedBefore = selStart - inputStart;
                    int b = Math.min(beforeLength, allowedBefore);
                    if (b <= 0 && afterLength <= 0) return true;
                    return super.deleteSurroundingText(b, afterLength);
                }

                @Override
                public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
                    // النسخة بنقاط الشفرة من الحارس نفسه —
                    // بعض اللوحات تستدعيها بدلا من نسخة
                    // الأحرف (كالرموز التعبيرية).
                    if (selecting) {
                        exitSelectionToInput();
                        return true;
                    }
                    if (inputStart < 0) return true;
                    int selStart = getSelectionStart();
                    int selEnd = getSelectionEnd();
                    if (selStart < 0 || selEnd < 0) return true;
                    if (selStart < inputStart) return true;
                    CharSequence text = getText();
                    int allowedBefore = text == null ? 0
                            : Character.codePointCount(text, inputStart, selStart);
                    int b = Math.min(beforeLength, allowedBefore);
                    if (b <= 0 && afterLength <= 0) return true;
                    return super.deleteSurroundingTextInCodePoints(b, afterLength);
                }

                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    if (event.getAction() == KeyEvent.ACTION_DOWN
                            && event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                        if (onEnterSubmit != null) onEnterSubmit.run();
                        return true;
                    }
                    if (event.getAction() == KeyEvent.ACTION_DOWN
                            && (event.getKeyCode() == KeyEvent.KEYCODE_DEL
                            || event.getKeyCode() == KeyEvent.KEYCODE_FORWARD_DEL)) {
                        // حارس السجل (v1.1.5): مسح لوحة مفاتيح
                        // مادية يصل هنا فيستدعي
                        // deleteSurroundingText على الاتصال
                        // الأصلي مباشرة متجاوزا الحارس أعلاه —
                        // فيُفحص هنا قبل التمرير.
                        if (selecting) {
                            exitSelectionToInput();
                            return true;
                        }
                        if (inputStart < 0) return true;
                        int selStart = getSelectionStart();
                        int selEnd = getSelectionEnd();
                        if (selStart < 0 || selEnd < 0) return true;
                        if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                            if (selStart <= inputStart) return true;
                        } else if (selStart < inputStart) {
                            return true;
                        }
                    }
                    return super.sendKeyEvent(event);
                }
            };
        }
    }

    private TerminalEditText terminalView;
    /** شريط إجراءات التحديد الحالي (يُنهى عند الخروج
     * البرمجي من وضع التحديد — حارس السجل v1.1.5). */
    private ActionMode selectionMode;
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
    /** صف المفاتيح الثاني (مفاتيح التحكم المعربة
     * v1.2.1): أزرار لمس واحدة لكل وظيفة — خروج،
     * حذف، بحث، السابق/التالي، بدّل، استعد — يظهر
     * مع صف المفاتيح الأول ويختفي معه. */
    private HorizontalScrollView keyRow2;
    /** صف البحث في المحفوظات (v1.2.1): حقل بحث
     * صغير + نتائج؛ يظهر بزر «بحث» فقط. */
    private LinearLayout searchRow;
    private EditText searchField;
    private ListView searchList;
    private ArrayAdapter<String> searchAdapter;
    private final ArrayList<String> searchItems = new ArrayList<>();
    private boolean searchOpen = false;
    /** صف البحث في المخرجات (الدفعة 3، البند 60): حقل
     * كلمة + زر «التالي»؛ يظهر من القائمة فقط. */
    private LinearLayout outputSearchRow;
    private EditText outputSearchField;
    private boolean outputSearchOpen = false;
    private int outputSearchFrom = 0;
    /** زر «بديل» كمعدِّل للسهم التالي (ALT + سهم). */
    private Button altButton;
    private boolean altNext = false;
    /** موجّه الجلسة النشطة الحالي (يُعاد رسمه عند تغير
     * موضعه فقط — بلا مسح كل الـspans مع كل قطعة، م-1). */
    private ForegroundColorSpan currentPromptSpan = null;
    private int currentPromptStart = -1;
    /** لون الموجّه الحالي (يتغير مع المظهر — بند 9). */
    private int promptColor = DARK_PROMPT;
    /** الجرس (الدفعة 3، البند 57): آخر اهتزاز خطأ،
     * وهل التطبيق مرئي الآن. */
    private long lastBellTime = 0;
    private boolean appVisible = false;

    /** الإعدادات (بند 3 + 4 + 9). */
    private SharedPreferences prefs;
    private float fontSizeSp = DEFAULT_FONT_SP;
    private boolean keepScreenOn = false;
    /** تعطيل اختصارات لوحة المفاتيح المادية (الدفعة 3،
     * البند 74) — الافتراضي لا. */
    private boolean disableHardwareShortcuts = false;
    /** لون الخطأ ولون النجاح من niva.properties (الدفعة 3،
     * البند 63)؛ 0 للنجاح = لون الموجّه حسب المظهر. */
    private int errorColor = ERROR_RED;
    private int successColor = 0;
    /** لون المؤشر الصريح من niva.properties (الدفعة 3،
     * البند 64)؛ 0 = تلقائي من سطوع الخلفية. */
    private int cursorColor = 0;
    private boolean lightTheme = false;
    private boolean showKeyRowPref = true;
    /** المظهر (v1.1.6 بند 10): 0 داكن دائما، 1 فاتح
     * دائما، 2 يتبع وضع الجهاز؛ lightTheme هو القيمة
     * المحلولة بعد تطبيق الوضع. */
    private int themeMode = 0;
    /** إخفاء لوحة المفاتيح عند البدء (v1.1.6 بند 5) —
     * الافتراضي لا (يبقى السلوك الحالي). */
    private boolean hideKeyboardOnStartup = false;
    /** لوحة المفاتيح البرمجية مفعلة (v1.1.6 بند 6) —
     * الافتراضي نعم. */
    private boolean softKeyboardEnabled = true;
    /** هوامش الترمنال بالبكسل (v1.1.6 بند 11). */
    private int marginH = 16;
    private int marginV = 8;
    /** سقف أسطر السجل القابل للضبط (v1.1.6 بند 12). */
    private int maxTranscriptLines = MAX_LINES;
    /** ملء الشاشة (v1.1.7 بند 30) — الافتراضي معطّل. */
    private boolean fullscreen = false;
    /** بديل ملء الشاشة (v1.1.7 بند 51) — الافتراضي
     * الحل الأساسي (تغيير ارتفاع الجذر). */
    private boolean fullscreenWorkaroundAlt = false;
    /** مستمع إعادة قياس الجذر في ملء الشاشة (بند 30)
     * وآخر ارتفاع مرئي رُئي (لمنع إعادة تخطيط بلا داعٍ). */
    private ViewTreeObserver.OnGlobalLayoutListener fullscreenLayoutListener = null;
    private int lastVisibleHeight = -1;
    /** الخط المخصص (v1.1.7 بند 32): المسار كما كتبه
     * المستخدم (داخل ~ أو مطلق)؛ فارغ = خط النظام. */
    private String customFontPath = "";
    /** الخط المحمّل فعلا؛ null = خط النظام الافتراضي. */
    private Typeface customTypeface = null;
    /** تنبيه تبديل الجلسة (v1.1.7 بنود 9 + 50) —
     * الافتراضي مفعّل. */
    private boolean sessionChangeToast = true;
    /** مفاتيح الصوت كتحكم (مهمة 12 — كما في Termux:
     * خفض الصوت = تحكم): الافتراضي معطّل، لأن ضغطة
     * صوت عارضة أثناء تشغيل برنامج تقاطعه (تعيد تشغيل
     * المحرك وتفقد متغيراته)؛ يُفعَّل من niva.properties
     * بالمفتاح «مفاتيح_الصوت=تحكم». */
    private boolean volumeKeysCtrl = false;
    /** استقبال الملفات (المهمة 28 — CATALOG-19 §13):
     * تعطيل استقبال المشاركة / العرض من
     * niva.properties (نعم = معطّل)؛ الافتراضي مفعّل. */
    private boolean disableReceiveShare = false;
    private boolean disableReceiveView = false;
    /** عمر المؤقت عند الخروج (المهمة 28 — CATALOG-19
     * §12): سالب = لا حذف أبدا، 0 = الكل (الافتراضي)،
     * موجب N = الأقدم من N أيام. */
    private int tempAgeDays = 0;
    /** اكتمل إنشاء النشاط واستعادة الجلسات (بند 9):
     * تنبيه التبديل لا يظهر أثناء الاستعادة. */
    private boolean startupDone = false;
    /** تكرار الأزرار بالضغط المطوّل (v1.1.6 بند 1):
     * أول تكرار بعد 400ms ثم كل 80ms حتى رفع الإصبع. */
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private Runnable repeatAction = null;
    private final Runnable repeatRunnable = new Runnable() {
        @Override
        public void run() {
            if (repeatAction != null) {
                repeatAction.run();
                mainHandler.postDelayed(this, 80);
            }
        }
    };
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

    // خروج إشعار الخدمة ألغى كل الجلسات (أمر المالك
    // 2026-11-09) — فيُمنع أي حفظ بعدها يعيد بناءها.
    private boolean exitFromNotification = false;

    /** لوحة كتابة البرنامج (بند 10). */
    private View composePanel;
    private EditText composeEdit;

    /** تنبيه عابر قصير (v1.1.6 بند 25): تأكيدات خفيفة
     * بلا حوار («تم النسخ»، «تم الحفظ»…). */
    private void toast(String msg) {
        Toast.makeText(this, msg, Toast.LENGTH_SHORT).show();
    }

    /** اهتزاز تأكيد (v1.1.6 بند 26): VIBRATE إذن عادي
     * بلا حوار؛ 1000ms هي المدة الافتراضية. */
    private void vibrate(long ms) {
        try {
            Vibrator vib = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
            if (vib == null || !vib.hasVibrator()) return;
            if (Build.VERSION.SDK_INT >= 26) {
                vib.vibrate(VibrationEffect.createOneShot(
                        ms, VibrationEffect.DEFAULT_AMPLITUDE));
            } else {
                vib.vibrate(ms);
            }
        } catch (Exception ignored) {
        }
    }

    private void vibrateDefault() {
        vibrate(1000);
    }

    /** الجرس (الدفعة 3، البند 57): اهتزاز 50ms واحد عند
     * سطر خطأ أحمر جديد، مخنوق بفاصل أدنى 150ms، ولا
     * يعمل إطلاقا إن كان التطبيق غير مرئي. */
    private void bellOnErrorLine() {
        if (!appVisible) return;
        long now = SystemClock.uptimeMillis();
        if (now - lastBellTime < 150) return;
        lastBellTime = now;
        vibrate(50);
    }

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
        ensureDefaultProperties();
        loadSettings();
        // سجل المنح (المهمة 28): يُحمَّل بعد الإعدادات —
        // والميتة تُقلم بصمت عند البدء.
        loadGrants();

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
        netStateView.setText("الشبكة: مفتوحة");
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
        // الهوامش من niva.properties (v1.1.6 بند 11).
        terminalView.setShowSoftInputOnFocus(softKeyboardEnabled);
        applyMargins();
        terminalView.setHorizontallyScrolling(false);
        terminalView.onEnterSubmit = this::submitLine;
        terminalView.onExitSelection = () -> {
            if (selectionMode != null) selectionMode.finish();
        };
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
        // سلوكيات الفأرة (v1.1.6 بند 14 — أولوية منخفضة،
        // أجهزة لوحية وDeX وChromebook): يمين يفتح قائمة
        // العنوان، عجلة تمرّر 3 أسطر لكل نقرة، وسط يلصق.
        // هذا سلوك نظام تشغيل للفأرة، لا بروتوكول فأرة VT
        // (ما زال مرفوضا).
        terminalView.setOnGenericMotionListener((v, event) -> {
            if (event.getAction() == MotionEvent.ACTION_BUTTON_PRESS) {
                int btn = event.getActionButton();
                if (btn == MotionEvent.BUTTON_SECONDARY) {
                    showTopMenu();
                    return true;
                }
                if (btn == MotionEvent.BUTTON_TERTIARY) {
                    pasteFromClipboard();
                    return true;
                }
            }
            if (event.getAction() == MotionEvent.ACTION_SCROLL) {
                float vscroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL);
                if (vscroll != 0) {
                    int lineH = Math.max(1, terminalView.getLineHeight());
                    terminalView.scrollBy(0, (int) (-vscroll * lineH * 3));
                    return true;
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
                        selectionMode = mode;
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
                        // مهمة 12 — نسخ الأمر القديم كاملا:
                        // الضغطة الطويلة تختار كلمة واحدة
                        // فقط، وهذا ينسخ السطر كله (بلا
                        // الموجّه) فيعيد لصقه أمرا جاهزا.
                        menu.add(Menu.NONE, 6, Menu.NONE, "نسخ السطر");
                        // v1.1.6 بند 12: حذف المحدد من العرض
                        // ومن السجل المحفوظ.
                        menu.add(Menu.NONE, 5, Menu.NONE, "احذف المحدد");
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
                        if (id == 5) {
                            deleteSelectedText();
                            mode.finish();
                            return true;
                        }
                        if (id == 6) {
                            copyLineAtSelection();
                            mode.finish();
                            return true;
                        }
                        return false;
                    }

                    @Override
                    public void onDestroyActionMode(ActionMode mode) {
                        selectionMode = null;
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
                        // الدفعة 3، البند 20: القرص يبدّل
                        // الخطوة نقطتين — التقريب لأقرب
                        // عدد زوجي يجعل القرص البطيء بلا
                        // أثر حتى يتجاوز 10% تقريبا.
                        float stepped = Math.round(fontSizeSp
                                * detector.getScaleFactor() / 2f) * 2f;
                        setFontSize(stepped);
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
            // الخط (بند 3)؛ ومجموعة Termux للتحكم بالجلسات
            // (مهمة 12): التالية/السابقة، جديدة، لصق،
            // كيبورد، قائمة، تسمية، روابط، 1–9 للتبديل.
            if (!disableHardwareShortcuts
                    && event.isCtrlPressed() && event.isAltPressed()) {
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
                if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN
                        || keyCode == KeyEvent.KEYCODE_N) {
                    switchToAdjacentSession(true);
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_DPAD_UP
                        || keyCode == KeyEvent.KEYCODE_P) {
                    switchToAdjacentSession(false);
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_C) {
                    if (!createSession()) {
                        showSessionCapDialog();
                    }
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_V) {
                    pasteFromClipboard();
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_K) {
                    toggleKeyboard();
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_M) {
                    showTopMenu();
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_R) {
                    showRenameDialog(activeIdx);
                    return true;
                }
                if (keyCode == KeyEvent.KEYCODE_U) {
                    showUrlDialog();
                    return true;
                }
                if (keyCode >= KeyEvent.KEYCODE_1
                        && keyCode <= KeyEvent.KEYCODE_9) {
                    int idx = keyCode - KeyEvent.KEYCODE_1;
                    if (idx < sessions.size()) switchTo(idx);
                    return true;
                }
            }
            // لوحة مفاتيح مادية: تحكم + حرف عربي (مفاتيح
            // التحكم المعربة v1.2.1) — مفتاح CTRL الحقيقي
            // هو «تحكم»؛ والحرف هو الحرف العربي الذي
            // ينتجه المفتاح على التخطيط العربي. لا يمس
            // اختصارات Ctrl+Alt أعلاه ولا سلوك أندرويد
            // الافتراضي خارج القائمة. «ع» (تعليق) مستبعد
            // عمدا: يحتاج سطح محرك جديد وقرارا منفصلا.
            if (!disableHardwareShortcuts
                    && event.isCtrlPressed() && !event.isAltPressed()) {
                char ch = (char) event.getUnicodeChar(0);
                switch (ch) {
                    case 'خ': doExit(); return true;
                    case 'م':
                        if (terminalView.inputStart >= 0) clearDisplayOnly(active());
                        return true;
                    case 'ب': moveCursorToLineStart(); return true;
                    case 'ن': moveCursorToLineEnd(); return true;
                    case 'ح': deleteToLineEnd(); return true;
                    case 'ل': deleteToLineStart(); return true;
                    case 'ك': deletePreviousWord(); return true;
                    case 'س': toggleHistorySearch(); return true;
                    case 'ق': historyNav(-1); return true;
                    case 'ت': historyNav(+1); return true;
                    case 'ا': swapLastTwoChars(); return true;
                    case 'ر': restoreKilledText(); return true;
                    default: break;
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

        // صف البحث في المحفوظات (v1.2.1): مخفي حتى
        // يطلبه زر «بحث»؛ وفوق صف المفاتيح.
        searchRow = buildHistorySearchRow();
        searchRow.setVisibility(View.GONE);
        root.addView(searchRow, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // صف البحث في المخرجات (الدفعة 3، البند 60):
        // مخفي حتى يطلبه عنصر القائمة؛ وفوق صف المفاتيح.
        outputSearchRow = buildOutputSearchRow();
        outputSearchRow.setVisibility(View.GONE);
        root.addView(outputSearchRow, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // صف المفاتيح الثاني (v1.2.1): أزرار التحكم
        // المعربة؛ يتبع صف المفاتيح الأول ظهورا واختفاء.
        keyRow2 = buildKeyRow2();
        keyRow2.setVisibility(View.GONE);
        root.addView(keyRow2, new LinearLayout.LayoutParams(
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
                showSessionCapDialog();
            }
        });
        // الضغطة الطويلة على «+» تفتح حوار إنشاء جلسة
        // مسمّاة (v1.1.6 بند 3): اكتب الاسم ثم «إنشاء»
        // فتولد الجلسة بالاسم.
        addBtn.setOnLongClickListener(v -> {
            showCreateNamedSessionDialog();
            return true;
        });
        drawerButtons.addView(addBtn);
        Button kbdBtn = new Button(this);
        kbdBtn.setText("⌨");
        kbdBtn.setTextColor(colorText);
        kbdBtn.setBackgroundColor(colorBtn);
        kbdBtn.setOnClickListener(v -> toggleKeyboard());
        // الضغطة الطويلة على زر الكيبورد تبدّل شريط
        // الأدوات (v1.1.6 بند 4): إظهار/إخفاء صف
        // المفاتيح (ولوحة الكتابة إن كانت ظاهرة)، مع
        // تنبيه قصير؛ والحالة محفوظة.
        kbdBtn.setOnLongClickListener(v -> {
            boolean show = !showKeyRowPref;
            setShowKeyRow(show);
            if (!show) hideCompose();
            toast(show ? "تم إظهار شريط الأدوات" : "تم إخفاء شريط الأدوات");
            return true;
        });
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
                        if (keyRow2 != null && keyRow2.getVisibility() != want) {
                            keyRow2.setVisibility(want);
                        }
                        updateSearchRowVisibility();
                        updateOutputSearchRowVisibility();
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

        // مسح ملفات السجلات القديمة بلا جلسة حية (v1.1.6
        // بند 11) قبل الاستعادة، ثم استعادة الجلسات
        // وضغط ملفاتها (v1.1.6 بند 2: ملفات يتيمة تُحذف).
        sweepOldSessionFiles();
        if (!restoreSessions()) {
            createSession();
        }
        saveAllSessions();
        terminalView.requestFocus();
        // إخفاء لوحة المفاتيح عند البدء (v1.1.6 بند 5):
        // الافتراضي يبقى كما هو (تظهر).
        if (!hideKeyboardOnStartup) showKeyboard();
        // (v1.1.7): ملء الشاشة (بند 30) والخط المخصص
        // (بند 32) بعد بناء الواجهة. خط تالف يُرجع
        // للافتراضي مع تنبيه واحد، ولا يُحفظ مساره.
        applyFullscreen();
        if (customFontPath != null && !customFontPath.trim().isEmpty()
                && !applyCustomFont()) {
            toast("تعذّر تحميل الخط المخصص — عاد الخط الافتراضي");
            customFontPath = "";
            saveSettings();
        }
        // (v1.1.7 بند 9): من هنا يسمح تنبيه التبديل —
        // فلا ينبّه أثناء استعادة الجلسات.
        startupDone = true;
        // اختصارات المشغّل (v1.1.6 بند 2): قصد أتى مع
        // تشغيل التطبيق من الأيقونة.
        handleShortcutIntent(getIntent());

        // الخدمة الأمامية (بند 13): إشعار عربي بعدد
        // الجلسات، يبقى ما دام التطبيق حيا.
        startService(new Intent(this, NivaService.class));
        refreshServiceNotification();
        exitReceiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                if (NivaService.ACTION_EXIT.equals(intent.getAction())) {
                    exitFromNotification();
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

    /** اختصارات المشغّل مع وضع singleTask (v1.1.6
     * بند 2 + 9): قصد جديد يصل إلى النسخة الحية نفسها
     * بدلا من بناء نسخة ثانية فوقها. */
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleShortcutIntent(intent);
    }

    private void handleShortcutIntent(Intent intent) {
        if (intent == null || intent.getAction() == null) return;
        String action = intent.getAction();
        if (Intent.ACTION_SEND.equals(action) || Intent.ACTION_VIEW.equals(action)) {
            // استقبال ملف .نيفا (المهمة 28 — CATALOG-19 §13).
            handleReceivedFile(intent);
            return;
        }
        if ("com.rio4140.niva.NEW_SESSION".equals(action)) {
            if (!createSession()) {
                showSessionCapDialog();
            }
        } else if ("com.rio4140.niva.OPEN_SETTINGS".equals(action)) {
            showTopMenu();
        } else if ("com.rio4140.niva.NEXT_SESSION".equals(action)) {
            // v1.1.7 بند 27: الجلسة التالية (تدوير).
            if (!sessions.isEmpty()) {
                switchTo((activeIdx + 1) % sessions.size());
            }
        } else if ("com.rio4140.niva.PREVIOUS_SESSION".equals(action)) {
            // v1.1.7 بند 28: الجلسة السابقة (تدوير).
            if (!sessions.isEmpty()) {
                switchTo((activeIdx - 1 + sessions.size()) % sessions.size());
            }
        } else if ("com.rio4140.niva.RENAME_SESSION".equals(action)) {
            // v1.1.7 بند 29: تسمية الجلسة الحالية.
            if (!sessions.isEmpty()) showRenameDialog(activeIdx);
        }
    }

    /** تغيير وضع النظام الليلي (v1.1.6 بند 10): مع
     * uiMode في configChanges لا يُعاد إنشاء النشاط؛
     * نعيد حلّ المظهر إن كان «النظام». */
    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        if (themeMode == 2) {
            lightTheme = !isSystemNight();
            applyThemeColors();
        }
    }

    @Override
    protected void onPause() {
        appVisible = false;
        // إيقاف تكرار الأزرار (v1.1.6 بند 1) قبل كل شيء.
        stopRepeat();
        // القفل مقصور على الجلب نفسه (بند 11): يُحرَّر
        // فقط إن لم يعد أي جلب جاريا — جلب بدأ قبل
        // المغادرة يكمل بقفله حتى وصول الموجّه.
        releaseFetchWakeLockIfIdle();
        saveAllSessions();
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        appVisible = true;
        // أشرطة النظام قد تعود بعد مغادرة التطبيق؛ أعد
        // إخفاءها إن كان ملء الشاشة مفعّلا (v1.1.7 بند 30).
        applyFullscreen();
        // إعادة مزامنة ألوان المظهر عند كل عودة للواجهة
        // (شكوى المالك 2026-11-08: كتابة سوداء فجأة):
        // إن انحرفت ألوان العرض عن حقول المظهر لأي سبب
        // (تحديث، تبديل ليلي للنظام أثناء الخلفية)،
        // تُعاد هنا قبل أن يرى المستخدم أي سطر.
        applyThemeColors();
    }

    /** زر الرجوع (شكوى المالك 2026-11-05 ~22:56): ضغطة
     * واحدة لا تُنهي النشاط أبدا ولا تقتل الجلسات. الدرج
     * المفتوح يُغلق أولا، ثم الكيبورد تُخفى، ثم يُنقَل
     * التطبيق إلى الخلفية وتبقى الجلسات حية. وضع التحديد
     * يخرج أولا (بند 1) قبل كل ذلك. */
    @Override
    public void onBackPressed() {
        if (terminalView != null && terminalView.selecting) {
            // إصلاح مهمة 12: الخروج من وضع التحديد كاملا —
            // setSelection وحده كان يترك `selecting` صحيحا
            // وشريط الإجراءات حيا والدرج مقفلا.
            terminalView.exitSelectionToInput();
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

    /** مفاتيح الصوت كتحكم (مهمة 12 — كما في Termux:
     * خفض الصوت = تحكم): مفعّلة فقط بالمفتاح
     * «مفاتيح_الصوت=تحكم» في niva.properties؛
     * والافتراضي أنها مفاتيح صوت عادية. لا تُسرق من
     * لوحة مفاتيح مادية كاملة. رفع الصوت يبقى صوت
     * النظام — لا مكافئ لـFn عندنا يحتاجه. */
    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (volumeKeysCtrl && keyCode == KeyEvent.KEYCODE_VOLUME_DOWN
                && event.getRepeatCount() == 0) {
            InputDevice dev = event.getDevice();
            if (dev == null
                    || dev.getKeyboardType() != InputDevice.KEYBOARD_TYPE_ALPHABETIC) {
                onCtrl();
                return true;
            }
        }
        return super.onKeyDown(keyCode, event);
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
        // تعطيل لوحة المفاتيح البرمجية كليا (v1.1.6 بند 6):
        // لا تُستدعى أبدا حين يكون المفتاح «لا».
        if (!softKeyboardEnabled) return;
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
        // الأسهم بأزرار متكررة (v1.1.6 بند 1): إمساك السهم
        // يكرر فعله كل 80ms حتى رفع الإصبع.
        row.addView(repeatButton("↑", this::arrowUp));
        row.addView(repeatButton("↓", this::arrowDown));
        row.addView(repeatButton("←", this::arrowLeft));
        row.addView(repeatButton("→", this::arrowRight));
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
            // نبضة لمسية قصيرة لكل زر (v1.1.6 بند 7) —
            // تحترم حالة الصامت الكلي للجهاز.
            v.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
            terminalView.requestFocus();
            action.onClick(v);
        });
        return b;
    }

    /** زر متكرر بالضغط المطوّل (v1.1.6 بند 1): نقرة
     * واحدة تنفذ الفعل مرة، والإمساك يكرره كل 80ms بعد
     * تأخير أولي 400ms؛ والتكرار يتوقف فور رفع الإصبع
     * أو إلغاء اللمس — فلا يأكل نصا. */
    private Button repeatButton(String label, Runnable action) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextColor(colorText);
        b.setBackgroundColor(colorBtn);
        b.setPadding(20, 8, 20, 8);
        b.setFocusable(false);
        b.setFocusableInTouchMode(false);
        b.setOnTouchListener((v, event) -> {
            int act = event.getAction();
            if (act == MotionEvent.ACTION_DOWN) {
                v.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
                terminalView.requestFocus();
                repeatAction = action;
                action.run();
                mainHandler.postDelayed(repeatRunnable, 400);
                return true;
            }
            if (act == MotionEvent.ACTION_UP
                    || act == MotionEvent.ACTION_CANCEL
                    || act == MotionEvent.ACTION_OUTSIDE) {
                repeatAction = null;
                mainHandler.removeCallbacks(repeatRunnable);
                return true;
            }
            return true;
        });
        return b;
    }

    /** إيقاف التكرار احتياطا عند مغادرة النشاط. */
    private void stopRepeat() {
        repeatAction = null;
        mainHandler.removeCallbacks(repeatRunnable);
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

    // ---------- صف المفاتيح الثاني: مفاتيح التحكم المعربة (v1.2.1) ----------

    /** صف المفاتيح الثاني: زر لمس واحد لكل وظيفة
     * بمسماها العربي — ضغطة واحدة = الفعل، بلا حالة
     * معدِّل يتعلمها المستخدم. «بداية» و«نهاية»
     * و«تنظيف» و«تحكم» موجودة في الصف الأول فلا
     * تُكرَّر هنا. */
    private HorizontalScrollView buildKeyRow2() {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        row.setBackgroundColor(colorPanel);
        row.setPadding(4, 4, 4, 4);
        row.addView(keyButton("ألغِ الأمر", v -> onCancelCommand()));
        row.addView(keyButton("خروج", v -> doExit()));
        row.addView(keyButton("احذف للنهاية", v -> deleteToLineEnd()));
        row.addView(keyButton("احذف للبداية", v -> deleteToLineStart()));
        row.addView(keyButton("احذف الكلمة", v -> deletePreviousWord()));
        row.addView(keyButton("بحث", v -> toggleHistorySearch()));
        row.addView(keyButton("السابق", v -> historyNav(-1)));
        row.addView(keyButton("التالي", v -> historyNav(1)));
        row.addView(keyButton("بدّل", v -> swapLastTwoChars()));
        row.addView(keyButton("استعد", v -> restoreKilledText()));
        HorizontalScrollView scroll = new HorizontalScrollView(this);
        scroll.setHorizontalScrollBarEnabled(false);
        scroll.addView(row, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return scroll;
    }

    /** خروج: يرسل «:خروج» نفسه كما يكتبه المستخدم —
     * فيُسجَّل في المحفوظات ويخرج المحرك، بلا مسار
     * جديد. ولا شيء أثناء تشغيل برنامج (الموجّه
     * غائب) — قاطعه أولا بزر «تحكم». */
    private void doExit() {
        if (terminalView.inputStart < 0) return;
        setCurrentLine(":خروج");
        submitLine();
    }

    /** مؤشر الإدخال مقصورا على سطر الإدخال: المؤشر
     * قد يقف في السجل (وضع التحديد) فيُقصر على نهاية
     * السطر — كما تفعل insertAtCursor. */
    private int clampInputCursor() {
        int pos = terminalView.getSelectionStart();
        int len = terminalView.getText().length();
        if (pos < terminalView.inputStart || pos > len) pos = len;
        return pos;
    }

    /** حذف من المؤشر لنهاية السطر: يحفظ المحذوف
     * لزر «استعد». */
    private void deleteToLineEnd() {
        if (terminalView.inputStart < 0) return;
        Editable text = terminalView.getText();
        int len = text.length();
        int pos = clampInputCursor();
        if (pos >= len) return;
        active().killedText = text.toString().substring(pos, len);
        terminalView.programmatic = true;
        text.delete(pos, len);
        terminalView.programmatic = false;
        terminalView.setSelection(pos, pos);
    }

    /** حذف من المؤشر لبداية السطر: يحفظ المحذوف
     * لزر «استعد». */
    private void deleteToLineStart() {
        if (terminalView.inputStart < 0) return;
        Editable text = terminalView.getText();
        int start = terminalView.inputStart;
        int pos = clampInputCursor();
        if (pos <= start) return;
        active().killedText = text.toString().substring(start, pos);
        terminalView.programmatic = true;
        text.delete(start, pos);
        terminalView.programmatic = false;
        terminalView.setSelection(start, start);
    }

    /** حذف الكلمة السابقة للمؤشر (حد الكلمة = فراغ):
     * يحفظ المحذوف لزر «استعد». */
    private void deletePreviousWord() {
        if (terminalView.inputStart < 0) return;
        Editable text = terminalView.getText();
        int start = terminalView.inputStart;
        int pos = clampInputCursor();
        if (pos <= start) return;
        int i = pos;
        while (i > start && Character.isWhitespace(text.charAt(i - 1))) i--;
        while (i > start && !Character.isWhitespace(text.charAt(i - 1))) i--;
        if (i >= pos) return;
        active().killedText = text.toString().substring(i, pos);
        terminalView.programmatic = true;
        text.delete(i, pos);
        terminalView.programmatic = false;
        terminalView.setSelection(i, i);
    }

    /** تبديل آخر حرفين قبل المؤشر (داخل سطر الإدخال). */
    private void swapLastTwoChars() {
        if (terminalView.inputStart < 0) return;
        Editable text = terminalView.getText();
        int start = terminalView.inputStart;
        int pos = clampInputCursor();
        if (pos - start < 2) return;
        char a = text.charAt(pos - 2);
        char b = text.charAt(pos - 1);
        terminalView.programmatic = true;
        text.replace(pos - 2, pos, "" + b + a);
        terminalView.programmatic = false;
        terminalView.setSelection(pos, pos);
    }

    /** استعادة آخر نص محذوف بأزرار الحذف. */
    private void restoreKilledText() {
        if (terminalView.inputStart < 0) return;
        String killed = active().killedText;
        if (killed == null || killed.isEmpty()) return;
        insertAtCursor(killed);
    }

    // ---------- البحث في المحفوظات (v1.2.1) ----------

    /** صف البحث: حقل بحث صغير + قائمة نتائج (الأحدث
     * أولا)؛ الضغط على نتيجة يضعها في سطر الإدخال. */
    private LinearLayout buildHistorySearchRow() {
        LinearLayout col = new LinearLayout(this);
        col.setOrientation(LinearLayout.VERTICAL);
        col.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        col.setBackgroundColor(colorPanel);
        col.setPadding(8, 8, 8, 8);
        searchField = new EditText(this);
        searchField.setHint("ابحث في المحفوظات");
        searchField.setTextColor(colorText);
        searchField.setHintTextColor(colorSubtitle);
        searchField.setSingleLine(true);
        searchField.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        searchField.setBackgroundColor(colorBtn);
        searchField.setPadding(16, 12, 16, 12);
        searchField.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int st, int c, int a) {
            }

            @Override
            public void onTextChanged(CharSequence s, int st, int b, int c) {
            }

            @Override
            public void afterTextChanged(Editable s) {
                filterHistorySearch(s.toString());
            }
        });
        searchField.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                if (searchAdapter != null && searchAdapter.getCount() > 0) {
                    applySearchResult(searchAdapter.getItem(0));
                } else {
                    hideHistorySearch();
                }
                return true;
            }
            return false;
        });
        col.addView(searchField, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        searchList = new ListView(this);
        searchAdapter = new ArrayAdapter<String>(this,
                android.R.layout.simple_list_item_1, searchItems) {
            @Override
            public View getView(int position, View convertView, ViewGroup parent) {
                View v = super.getView(position, convertView, parent);
                TextView tv = (TextView) v.findViewById(android.R.id.text1);
                tv.setTextColor(colorText);
                return v;
            }
        };
        searchList.setAdapter(searchAdapter);
        searchList.setBackgroundColor(colorPanel);
        int listH = (int) (180 * getResources().getDisplayMetrics().density);
        col.addView(searchList, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, listH));
        searchList.setOnItemClickListener((parent, view, pos, id) -> {
            String item = searchAdapter.getItem(pos);
            if (item != null) applySearchResult(item);
        });
        return col;
    }

    /** إظهار/إخفاء البحث: لا يفتح أثناء تشغيل برنامج
     * (بلا سطر إدخال يتلقى النتيجة). */
    private void toggleHistorySearch() {
        if (searchOpen) {
            hideHistorySearch();
            return;
        }
        if (terminalView.inputStart < 0) return;
        searchOpen = true;
        updateSearchRowVisibility();
        filterHistorySearch(searchField.getText().toString());
        searchField.requestFocus();
        showKeyboard();
    }

    private void hideHistorySearch() {
        searchOpen = false;
        if (searchField != null) searchField.setText("");
        updateSearchRowVisibility();
        if (terminalView != null) terminalView.requestFocus();
    }

    private void updateSearchRowVisibility() {
        if (searchRow == null) return;
        searchRow.setVisibility((searchOpen && showKeyRowPref && isKeyboardVisible())
                ? View.VISIBLE : View.GONE);
    }

    /** تصفية المحفوظات أثناء الكتابة: مطابقة جزئية،
     * الأحدث أولا، بحد 50 نتيجة؛ وبحث فارغ يعرض
     * الأحدث كما هي. */
    private void filterHistorySearch(String query) {
        if (searchAdapter == null) return;
        searchItems.clear();
        Session s = active();
        String q = query == null ? "" : query.trim();
        for (int i = s.history.size() - 1; i >= 0 && searchItems.size() < 50; i--) {
            String h = s.history.get(i);
            if (q.isEmpty() || h.contains(q)) searchItems.add(h);
        }
        searchAdapter.notifyDataSetChanged();
    }

    private void applySearchResult(String item) {
        setCurrentLine(item);
        hideHistorySearch();
        terminalView.requestFocus();
    }

    // ---------- البحث في المخرجات (الدفعة 3، البند 60) ----------

    /** صف البحث في المخرجات: حقل كلمة + «التالي» +
     * «✕»؛ الكتابة تقفز لأول تطابق حيا، و«التالي»
     * للتالي مع التفاف للبداية. */
    private LinearLayout buildOutputSearchRow() {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setLayoutDirection(View.LAYOUT_DIRECTION_RTL);
        row.setBackgroundColor(colorPanel);
        row.setPadding(8, 8, 8, 8);
        outputSearchField = new EditText(this);
        outputSearchField.setHint("ابحث في المخرجات");
        outputSearchField.setTextColor(colorText);
        outputSearchField.setHintTextColor(colorSubtitle);
        outputSearchField.setSingleLine(true);
        outputSearchField.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        outputSearchField.setBackgroundColor(colorBtn);
        outputSearchField.setPadding(16, 12, 16, 12);
        outputSearchField.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int st, int c, int a) {
            }

            @Override
            public void onTextChanged(CharSequence s, int st, int b, int c) {
            }

            @Override
            public void afterTextChanged(Editable s) {
                outputSearchFrom = 0;
                findOutputNext(false);
            }
        });
        outputSearchField.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                findOutputNext(true);
                return true;
            }
            return false;
        });
        row.addView(outputSearchField, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));
        Button nextBtn = new Button(this);
        nextBtn.setText("التالي");
        nextBtn.setTextColor(colorText);
        nextBtn.setBackgroundColor(colorBtn);
        nextBtn.setOnClickListener(v -> findOutputNext(true));
        row.addView(nextBtn, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        Button closeBtn = new Button(this);
        closeBtn.setText("✕");
        closeBtn.setTextColor(colorText);
        closeBtn.setBackgroundColor(colorBtn);
        closeBtn.setOnClickListener(v -> hideOutputSearch());
        row.addView(closeBtn, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return row;
    }

    /** إظهار/إخفاء البحث في المخرجات. */
    private void toggleOutputSearch() {
        if (outputSearchOpen) {
            hideOutputSearch();
            return;
        }
        outputSearchOpen = true;
        updateOutputSearchRowVisibility();
        outputSearchFrom = 0;
        outputSearchField.requestFocus();
        showKeyboard();
    }

    private void hideOutputSearch() {
        outputSearchOpen = false;
        if (outputSearchField != null) outputSearchField.setText("");
        updateOutputSearchRowVisibility();
        if (terminalView != null) {
            moveCursorToEnd();
            terminalView.requestFocus();
        }
    }

    private void updateOutputSearchRowVisibility() {
        if (outputSearchRow == null) return;
        outputSearchRow.setVisibility((outputSearchOpen && showKeyRowPref && isKeyboardVisible())
                ? View.VISIBLE : View.GONE);
    }

    /** القفز للتطابق التالي لكلمة البحث في المخرجات:
     * من الموضع الحالي، مع التفاف للبداية. */
    private void findOutputNext(boolean fromButton) {
        if (outputSearchField == null || terminalView == null) return;
        String q = outputSearchField.getText().toString();
        if (q.isEmpty()) return;
        Editable text = terminalView.getText();
        if (text == null) return;
        String all = text.toString();
        int start = fromButton ? outputSearchFrom : 0;
        int found = all.indexOf(q, start);
        boolean wrapped = false;
        if (found < 0 && start > 0) {
            found = all.indexOf(q, 0);
            wrapped = found >= 0;
        }
        if (found < 0) {
            toast("لا توجد نتائج");
            return;
        }
        int end = found + q.length();
        outputSearchFrom = end;
        terminalView.programmatic = true;
        terminalView.setSelection(found, end);
        terminalView.programmatic = false;
        terminalView.bringPointIntoView(found);
        if (wrapped) toast("عاد إلى البداية");
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

    /** «ألغِ الأمر» (المهمة 22 — شبكة.نزّل): يرسل
     * سطر الإلغاء «\u0001نيفا-إلغاء» إلى محرك الجلسة
     * النشطة عبر stdin — فيلغي المحرك الجلب الجاري
     * بلطف (خطأ «أُلغي الأمر» يُمسك بـحاول/ماعدا)
     * ويعود الموجّه، بلا قتل ولا إعادة تشغيل (ذلك
     * زر «تحكم»). وبلا جلب جارٍ لا أثر له. ولا شيء
     * يُرسل حين لا عملية أو جلسة منتهية؛ والسطر
     * ليس أمر جلب (لا يطابق isFetchCommand) فلا
     * فحص اتصال ولا قفل استيقاظ. */
    private void onCancelCommand() {
        Session s = active();
        if (s == null || s.process == null || s.ended) return;
        if (s.safWaitPending) {
            // إلغاء أثناء انتظار SAF (المهمة 28): المحرك
            // يجهض انتظاره؛ ونتيجته التالية — إن وصلت —
            // شاردة فتُسقَط (علم safCancelArmed).
            s.safCancelArmed = true;
        }
        sendLineToEngine(s, "\u0001نيفا-إلغاء");
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
        return createSession(null);
    }

    /** إنشاء جلسة باسم اختياري (v1.1.6 بند 3: من حوار
     * الضغطة الطويلة على زر «جلسة جديدة»). */
    private boolean createSession(String name) {
        return createSession(name, null, null);
    }

    /** إنشاء جلسة برنامج (المهمة 28): تشغّل ملف
     * .نيفا من المؤقت في جلسة جديدة باسم الملف. */
    private boolean createSession(String name, File programFile, String programName) {
        if (sessions.size() >= MAX_SESSIONS) {
            return false;
        }
        Session s = new Session();
        if (name != null && !name.trim().isEmpty()) {
            s.customName = name.trim();
        }
        s.programFile = programFile;
        s.programName = programName;
        sessions.add(s);
        startProcess(s);
        rebuildSessionList();
        switchTo(sessions.size() - 1);
        refreshServiceNotification();
        return true;
    }

    /** حوار بلوغ سقف الجلسات 8 (الدفعة 3، البند 65):
     * رفض صريح بدلا من الصمت — المستخدم يعرف أن الزر
     * يعمل وأن السقف هو السبب. */
    private void showSessionCapDialog() {
        new AlertDialog.Builder(this)
                .setTitle("بلغت الحد الأقصى")
                .setMessage("ثماني جلسات كحد أقصى — احذف جلسة منتهية أو قديمة لتفتح جديدة.")
                .setPositiveButton("حسنا", null)
                .show();
    }

    private void switchTo(int idx) {
        // مسودة لوحة الكتابة تتبع جلستها (v1.1.6 بند 13):
        // تُحفظ للمغادِرة وتُحمَّل للداخلة.
        int prevIdx = activeIdx;
        if (!sessions.isEmpty() && composeEdit != null) {
            active().composeDraft = composeEdit.getText().toString();
        }
        saveTerminalToSession();
        // البحث في المحفوظات يخص الجلسة المغادَرة —
        // يُغلق عند التبديل حتى لا تعرض نتائج قديمة.
        hideHistorySearch();
        // والبحث في المخرجات كذلك (الدفعة 3، البند 60).
        hideOutputSearch();
        activeIdx = idx;
        syncTerminalFromSession();
        if (composeEdit != null) {
            composeEdit.setText(active().composeDraft);
        }
        updateNetState();
        rebuildSessionList();
        // تنبيه تبديل الجلسة (v1.1.7 بند 9): بعد اكتمال
        // البدء فقط، ولتبديل فعلي (لا لنفس الجلسة)؛
        // ويُعطَّل بالمفتاح (بند 50).
        if (startupDone && sessionChangeToast && prevIdx != idx
                && !sessions.isEmpty()) {
            toast("تم التبديل إلى " + sessionDisplayName(idx));
        }
        terminalView.requestFocus();
    }

    /** تبديل الجلسة التالية/السابقة مع الالتفاف من
     * الأخيرة إلى الأولى وبالعكس (مهمة 12: اختصارات
     * لوحة المفاتيح المادية Ctrl+Alt+N/P وCtrl+Alt+↓/↑
     * كما في Termux). */
    private void switchToAdjacentSession(boolean forward) {
        if (sessions.isEmpty()) return;
        int next = activeIdx + (forward ? 1 : -1);
        if (next >= sessions.size()) next = 0;
        if (next < 0) next = sessions.size() - 1;
        if (next != activeIdx) switchTo(next);
    }

    /** اسم الجلسة للعرض (مخصص أو «جلسة N») — يُستخدم
     * في تنبيه التبديل (v1.1.7 بند 9) كما في الدرج. */
    private String sessionDisplayName(int idx) {
        if (idx < 0 || idx >= sessions.size()) return "";
        Session s = sessions.get(idx);
        return s.customName != null ? s.customName
                : "جلسة " + arabicDigits(idx + 1);
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
        // أي وضع تحديد من الجلسة السابقة ينتهي هنا —
        // setText يمحو التحديد لكن شريط الإجراءات قد
        // يبقى حيا (مهمة 12).
        if (selectionMode != null) selectionMode.finish();
        terminalView.selecting = false;
        terminalView.programmatic = true;
        terminalView.setText(s.text.toString());
        terminalView.inputStart = s.inputStart;
        terminalView.programmatic = false;
        // إعادة تأكيد ألوان الترمنال عند استعادة الجلسة
        // (شكوى المالك 2026-11-08: كتابة سوداء فجأة):
        // الخلفية والنص من حقول المظهر نفسها، فلا يبقى
        // سطر بلون قديم إن انحرف العرض عنها.
        terminalView.setBackgroundColor(colorBg);
        terminalView.setTextColor(colorText);
        currentPromptSpan = null;
        currentPromptStart = -1;
        paintPrompt();
        colorAllOutputLines();
        int end = terminalView.getText().length();
        terminalView.setSelection(end, end);
        // إصلاح التبديل (مهمة 12): setText يستبدل النص
        // فيبقى اتصال لوحة المفاتيح مربوطا بالنص القديم
        // على بعض الأجهزة، فتظهر اللوحة ولا يصل حرف.
        // إعادة الربط هنا تجعل الكتابة تعمل فور التبديل.
        InputMethodManager imm = (InputMethodManager)
                getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm != null) imm.restartInput(terminalView);
        // حوار معلّق لهذه الجلسة (المهمة 20) يُعرض الآن.
        maybeShowPendingDialog(s);
        // وطلب SAF معلّق (المهمة 28) يُنفَّذ الآن.
        maybeShowPendingSaf(s);
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
            name.setTextColor(s.endedWithError ? errorColor : colorText);
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

    /** قائمة صف الجلسة (بند 6): تسمية / إنهاء الجلسة —
     * والمنتهية وحدها تحصل على «احذف الجلسة»
     * (v1.1.6 بند 1): الحية لا تُحذف من الدرج أبدا،
     * تُنهى أولا ثم تُحذف. */
    private void showSessionRowMenu(final int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        final Session s = sessions.get(idx);
        if (s.ended) {
            new AlertDialog.Builder(this)
                    .setItems(new CharSequence[]{"تسمية", "إنهاء الجلسة", "احذف الجلسة"},
                            (d, which) -> {
                                if (which == 0) showRenameDialog(idx);
                                else if (which == 1) showCloseSessionDialog(idx);
                                else showDeleteSessionDialog(idx);
                            })
                    .show();
        } else {
            new AlertDialog.Builder(this)
                    .setItems(new CharSequence[]{"تسمية", "إنهاء الجلسة"},
                            (d, which) -> {
                                if (which == 0) showRenameDialog(idx);
                                else showCloseSessionDialog(idx);
                            })
                    .show();
        }
    }

    /** حوار إنشاء جلسة مسمّاة (v1.1.6 بند 3). */
    private void showCreateNamedSessionDialog() {
        final EditText input = new EditText(this);
        input.setTextDirection(View.TEXT_DIRECTION_RTL);
        input.setHint("اسم الجلسة");
        new AlertDialog.Builder(this)
                .setTitle("جلسة جديدة")
                .setView(input)
                .setPositiveButton("إنشاء", (d, w) -> {
                    String name = input.getText().toString().trim();
                    if (createSession(name.isEmpty() ? null : name)) {
                        closeDrawer();
                    } else {
                        showSessionCapDialog();
                    }
                })
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** حوار تأكيد حذف جلسة منتهية (v1.1.6 بند 1):
     * يسمّي الجلسة، ويحذف سجلها المحفوظ معها. */
    private void showDeleteSessionDialog(final int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        Session s = sessions.get(idx);
        String name = s.customName != null ? s.customName
                : "جلسة " + arabicDigits(idx + 1);
        new AlertDialog.Builder(this)
                .setMessage("احذف الجلسة «" + name + "»؟\nسيُحذف سجلها المحفوظ أيضا.")
                .setPositiveButton("حذف", (d, w) -> deleteSession(idx, false))
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** حذف جلسة منتهية (v1.1.6 بند 1): تُزال من
     * القائمة وتُحذف ملفات سجلها؛ الحية لا تصل هنا
     * أبدا (القائمة لا تعرض الحذف إلا للمنتهية).
     * silent = بلا تنبيه (للحذف التلقائي بند 10). */
    private void deleteSession(int idx, boolean silent) {
        if (idx < 0 || idx >= sessions.size()) return;
        Session s = sessions.get(idx);
        if (!s.ended) {
            toast("الجلسة ما زالت تعمل — أنهِها أولا");
            return;
        }
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
        // إغلاق حوار الجلسة ومسح تنبيهاتها المعلقة
        // (إصلاح الجلسات): كإعادة التشغيل وموت العملية —
        // فلا يبقى حوار بلا محرك ولا تنبيه معلق لجلسة
        // لم تعد موجودة.
        dismissSessionDialog(s);
        dropPendingNotifies(s);
        // وإيقاف نطق/صوت/حساس/استماع/جهاز الجلسة (كإعادة
        // التشغيل وموت العملية) — فلا يبقى نطق أو
        // استماع أو قراءة معلقة لجلسة لم تعد موجودة.
        stopSpeakForSession(s);
        stopAudioForSession(s);
        stopSensorForSession(s);
        stopListenForSession(s);
        cancelDeviceRequestsForSession(s);
        boolean wasActive = idx == activeIdx;
        sessions.remove(idx);
        deleteSessionFiles(idx);
        if (sessions.isEmpty()) {
            activeIdx = 0;
            createSession();
        } else {
            if (wasActive) {
                activeIdx = Math.max(0, Math.min(idx, sessions.size() - 1));
                syncTerminalFromSession();
                if (composeEdit != null) {
                    composeEdit.setText(active().composeDraft);
                }
                updateNetState();
            } else if (idx < activeIdx) {
                activeIdx--;
            }
            rebuildSessionList();
        }
        saveAllSessions();
        refreshServiceNotification();
        if (!silent) toast("تم حذف الجلسة");
    }

    /** خروج إشعار الخدمة (أمر المالك 2026-11-09): إلغاء
     * كل الجلسات تماما — إيقاف العمليات وحذف كل ملفات
     * الجلسات (كل لاحقة session-N.* وملف active) ومسح
     * علامة النشطة — ثم إنهاء التطبيق، فيفتح بعدها بلا
     * أي جلسة. والخروج العادي لا يمر من هنا إطلاقا
     * فتبقى جلساته محفوظة وتُستعاد (الاجتماع 39). */
    /** خروج من إشعار الخدمة: يغلق كل الجلسات فقط
     * ويخرج من التطبيق — بلا حذف أي ملف إطلاقا
     * لحظة الخروج (لا ملفات تطبيق ولا أدوات ولا
     * جلسات). كل جلسة تُعلَّم منتهية بكتابة علامة
     * `.ended` الخاصة بها (آلية الإنهاء القائمة):
     * فلا تُستعاد حية عند الفتح التالي، وتُنظَّف
     * بمنطق المنتهية القائم. */
    private void exitFromNotification() {
        exitFromNotification = true;
        File dir = sessionsDir();
        for (int i = 0; i < sessions.size(); i++) {
            Session s = sessions.get(i);
            if (s.process != null) {
                try {
                    s.process.destroy();
                } catch (Exception ignored) {
                }
            }
            s.ended = true;
            try {
                writeFile(new File(dir, "session-" + i + ".ended"), "");
            } catch (IOException ignored) {
            }
        }
        if (prefs != null) {
            prefs.edit().remove(PREF_ACTIVE_SESSION).apply();
        }
        sessions.clear();
        activeIdx = 0;
        try {
            stopService(new Intent(this, NivaService.class));
        } catch (Exception ignored) {
        }
        finish();
    }

    /** حذف ملفات جلسة بعينها (سجل + محفوظات + مسودة
     * الكتابة) من مجلد الجلسات. */
    private void deleteSessionFiles(int idx) {
        File dir = sessionsDir();
        new File(dir, "session-" + idx + ".txt").delete();
        new File(dir, "session-" + idx + ".meta").delete();
        new File(dir, "session-" + idx + ".compose").delete();
        new File(dir, "session-" + idx + ".prog").delete();
        new File(dir, "session-" + idx + ".ended").delete();
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
        // مهمة 12 — كما في Termux (حوار القتل يسمّي
        // العملية): الحوار يسمّي الجلسة، ورقم العملية
        // حين يتوفر (API 26+)، فلا تُنهى جلسة بالخطأ.
        Session s = sessions.get(idx);
        String name = sessionDisplayName(idx);
        String msg = "أنهي «" + name + "»؟";
        if (s.process != null
                && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            try {
                msg += "\n(العملية " + s.process.pid() + ")";
            } catch (Exception ignored) {
            }
        }
        new AlertDialog.Builder(this)
                .setMessage(msg)
                .setPositiveButton("إنهاء", (d, w) -> closeSession(idx))
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** إنهاء جلسة (بند 6): تُقتل عمليتها وتُزال؛ إن كانت
     * الأخيرة أُنشئت جلسة جديدة بدلا منها، فلا يبقى
     * التطبيق بلا جلسة أبدا. القتل بـSIGKILL مباشرة
     * (مهمة 12 — كما في Termux finishIfRunning):
     * بلا مهلة إغلاق لطيف، فالمستخدم أكّد الحوار. */
    private void closeSession(int idx) {
        if (idx < 0 || idx >= sessions.size()) return;
        Session s = sessions.get(idx);
        if (s.process != null) {
            try {
                s.process.destroyForcibly();
            } catch (Exception ignored) {
            }
        }
        if (s.writer != null) s.writer.interrupt();
        if (s.waiter != null) s.waiter.interrupt();
        s.fetchWakeHeld = false;
        releaseFetchWakeLockIfIdle();
        // إغلاق حوار الجلسة ومسح تنبيهاتها المعلقة
        // (إصلاح الجلسات): كإعادة التشغيل وموت العملية —
        // فلا يبقى حوار بلا محرك ولا تنبيه معلق لجلسة
        // لم تعد موجودة.
        dismissSessionDialog(s);
        dropPendingNotifies(s);
        // وإيقاف نطق/صوت/حساس/استماع/جهاز الجلسة (كإعادة
        // التشغيل وموت العملية) — فلا يبقى نطق أو
        // استماع أو قراءة معلقة لجلسة لم تعد موجودة.
        stopSpeakForSession(s);
        stopAudioForSession(s);
        stopSensorForSession(s);
        stopListenForSession(s);
        cancelDeviceRequestsForSession(s);
        boolean wasActive = idx == activeIdx;
        sessions.remove(idx);
        if (sessions.isEmpty()) {
            activeIdx = 0;
            createSession();
        } else {
            if (wasActive) {
                activeIdx = Math.max(0, Math.min(idx, sessions.size() - 1));
                syncTerminalFromSession();
                if (composeEdit != null) {
                    composeEdit.setText(active().composeDraft);
                }
                updateNetState();
                terminalView.requestFocus();
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
        // خروج إشعار الخدمة أغلق كل الجلسات — فلا حفظ
        // بعدها يعيد إحياء ما أُغلق.
        if (exitFromNotification) return;
        if (sessions.isEmpty()) return;
        // مسودة لوحة الكتابة للجلسة النشطة (v1.1.6 بند 13).
        if (composeEdit != null) {
            active().composeDraft = composeEdit.getText().toString();
        }
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
        // ضغط ملفات الجلسات (v1.1.6 بند 2): أي ملف
        // يتيم بلا جلسة يُحذف عند كل حفظ.
        pruneSessionFiles();
    }

    /** حذف ملفات جلسات يتيمة: أي session-N بلا جلسة
     * حية بنفس الرقم (بعد حذف أو عطل) — يُحذف سجلها
     * ومحفوظاتها ومسودتها. */
    private void pruneSessionFiles() {
        File dir = sessionsDir();
        File[] files = dir.listFiles();
        if (files == null) return;
        for (File f : files) {
            Integer idx = sessionFileIndex(f.getName());
            if (idx == null) continue;
            if (idx >= sessions.size()) {
                f.delete();
            }
        }
    }

    /** رقم ملف جلسة من اسمه (session-3.txt → 3)، أو
     * null إن لم يكن ملف جلسة. */
    private static Integer sessionFileIndex(String name) {
        if (!name.startsWith("session-")) return null;
        String rest = name.substring(8);
        int dot = rest.indexOf('.');
        if (dot < 0) return null;
        try {
            return Integer.parseInt(rest.substring(0, dot));
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /** مسح ملفات سجلات قديمة عند البدء (v1.1.6 بند 11):
     * ملف سجل أقدم من 14 يوما بلا جلسة حية — أي لا
     * يُستعاد (خارج أول 8 ملفات صالحة) — يُحذف مع
     * ملفاته المرافقة. ملاحظة صادقة: في بنيتنا كل ملف
     * صالح يُستعاد جلسة حية عند البدء، فـ«بلا جلسة
     * حية» تعني عمليا الملفات اليتيمة؛ والضغط في
     * saveAllSessions يزيل أي يتيم متبقٍّ مهما كان
     * عمره (بند 2). */
    private void sweepOldSessionFiles() {
        File dir = sessionsDir();
        if (!dir.isDirectory()) return;
        File[] files = dir.listFiles();
        if (files == null) return;
        List<Integer> valid = new ArrayList<>();
        for (File f : files) {
            Integer idx = sessionFileIndex(f.getName());
            if (idx != null && f.getName().endsWith(".txt")) {
                valid.add(idx);
            }
        }
        Collections.sort(valid);
        Set<Integer> live = new HashSet<>();
        for (int i = 0; i < Math.min(MAX_SESSIONS, valid.size()); i++) {
            live.add(valid.get(i));
        }
        long cutoff = System.currentTimeMillis() - OLD_LOG_MILLIS;
        for (File f : files) {
            Integer idx = sessionFileIndex(f.getName());
            if (idx == null || live.contains(idx)) continue;
            if (f.lastModified() < cutoff) {
                f.delete();
            }
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
            // مسودة لوحة الكتابة (v1.1.6 بند 13): ملف
            // مستقل؛ الفارغة تحذف ملفها.
            File composeFile = new File(dir, "session-" + idx + ".compose");
            if (s.composeDraft != null && !s.composeDraft.isEmpty()) {
                writeFile(composeFile, s.composeDraft);
            } else {
                composeFile.delete();
            }
            // جلسة برنامج (المهمة 28): علامة مستقلة
            // باسم البرنامج — فلا تُعاد عند الاستعادة
            // (الملف المؤقت ذهب مع الخروج).
            File progFile = new File(dir, "session-" + idx + ".prog");
            if (s.programFile != null && s.programName != null) {
                writeFile(progFile, s.programName);
            } else {
                progFile.delete();
            }
            // جلسة منتهية (اجتماع 37): علامة مستقلة
            // فارغة — فتُستعاد منتهية للقراءة فقط ولا
            // تُبعث حية عند البدء التالي (كما في .prog).
            File endedFile = new File(dir, "session-" + idx + ".ended");
            if (s.ended) {
                writeFile(endedFile, "");
            } else {
                endedFile.delete();
            }
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
                    // الشبكة تلقائية (أمر المالك 2026-11-06):
                    // الجلسة المستعادة تُجبَر على مفتوحة —
                    // السطر المحفوظ (lines[0]) يُتجاهل عمدا.
                    s.networkAllowed = true;
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
            // مسودة لوحة الكتابة (v1.1.6 بند 13).
            try {
                s.composeDraft = readFile(new File(dir, "session-" + idx + ".compose"));
            } catch (IOException ignored) {
                s.composeDraft = "";
            }
            // جلسة برنامج (المهمة 28): تُستعاد منتهية
            // للقراءة فقط — الملف المؤقت ذهب مع الخروج
            // فلا تُعاد أبدا (والملف يُبنى كعلامة فقط).
            boolean isProgram = false;
            try {
                File progFile = new File(dir, "session-" + idx + ".prog");
                if (progFile.isFile()) {
                    s.programName = readFile(progFile);
                    s.programFile = new File(tempDir(), s.programName);
                    isProgram = true;
                }
            } catch (IOException ignored) {
            }
            // جلسة منتهية (اجتماع 39 — نموذج ترمكس):
            // علامة .ended تعني أنها أُغلقت نهائيا —
            // فتُحذف ملفاتها ولا تُستعاد أبدا؛ وجلسة
            // البرنامج مستثناة (تُستعاد منتهية للقراءة
            // كما صممت المهمة 28).
            boolean isEnded = new File(dir, "session-" + idx + ".ended").isFile();
            if (isEnded && !isProgram) {
                deleteSessionFiles(idx);
                continue;
            }
            sessions.add(s);
            if (isProgram || isEnded) {
                s.ended = true;
                s.endNotified = true;
            } else {
                startProcess(s);
            }
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
        if (composeEdit != null) {
            composeEdit.setText(active().composeDraft);
        }
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
            if (s.programFile != null) {
                // جلسة برنامج (المهمة 28): تشغيل الملف
                // لا REPL — ومجلد البرنامج هو المؤقت
                // (أب الملف) فالسياسة والمسارات كلها
                // فيه.
                if (s.networkAllowed) cmd.add("--اسمح-بالشبكة");
                cmd.add(s.programFile.getAbsolutePath());
            } else {
                cmd.add("--تفاعلي");
                if (s.networkAllowed) cmd.add("--اسمح-بالشبكة");
            }
            // الدفعة 5 (البندان 94 و96): كشف مجلد
            // التنزيلات للقراءة فقط باسم
            // تخزين/تنزيلات — عند المنح فقط، ومن
            // المشغّل لا من ملف قابل للتحرير.
            if (storageManagerGranted()) {
                cmd.add("--تنزيلات");
                cmd.add(downloadsPath());
            }
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.directory(getFilesDir());
            pb.redirectErrorStream(false);
            // قناة التحكم (م-6): المحرك يرسل إشاراته
            // المنظمة على stderr فقط حين تُطلب.
            pb.environment().put("NIVA_CONTROL_CHANNEL", "stderr");
            // نسخة الترمنال للمحرك (بلاغ 2026-11-08): `اص`
            // يعرض نسخة الترمنال الحقيقية لا نسخة المحرك.
            String terminalVersion = localVersionName();
            if (terminalVersion != null) {
                pb.environment().put("NIVA_TERMINAL_VERSION", terminalVersion);
            }
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
            // الشبكة تلقائية (أمر المالك 2026-11-06): لا
            // سؤال إذن أبدا — كما في Termux. المحرك يبدأ
            // دائما بعلم الشبكة، فلا تصل هذه الإشارة في
            // العادة؛ وإن وصلت (محرك قديم أو حالة حدّية)
            // تُثبَّت الموافقة ويُعاد إرسال السطر المعلق
            // بلا أي حوار — كما كان يفعل زر «سماح».
            if (!s.networkAllowed) {
                s.networkAllowed = true;
                updateNetState();
                sendToEngine(s, ALLOW_NETWORK_COMMAND);
            }
            if (s.pendingLine != null) {
                String pl = s.pendingLine;
                s.pendingLine = null;
                // عبر فحص الجلب (بند 11 + 12): دون
                // اتصال يُعرض «لا يوجد اتصال» بدلا من
                // انتظار مهلة DNS.
                sendLineToEngine(s, pl);
            }
        } else if ("مسح".equals(signal)) {
            s.history.clear();
            s.histIdx = -1;
            s.draft = "";
        } else if ("إعدادات".equals(signal)) {
            // أعد-الإعدادات (اجتماع 45 §59): إعادة قراءة
            // الملف وتطبيق ما له مسار حي فورا؛ وما يحتاج
            // إعادة تشغيل (إخفاء لوحة المفاتيح عند البدء،
            // الخط المخصص، ملء الشاشة) يسمّيه المحرك في
            // سطر تأكيده ولا يُطبَّق هنا.
            loadSettings();
            applyThemeColors();
            applyMargins();
            applyKeepScreenFlag();
            if (terminalView != null) terminalView.setTextSize(fontSizeSp);
            if (keyRow != null) {
                keyRow.setVisibility((showKeyRowPref && isKeyboardVisible())
                        ? View.VISIBLE : View.GONE);
            }
        } else if (signal.startsWith("حوار:")) {
            // طلب حوار من المحرك (المهمة 20): الحمولة JSON.
            handleDialogRequest(s, signal.substring("حوار:".length()));
        } else if (signal.startsWith("نبّه:")) {
            // طلب تنبيه من المحرك (المهمة 21): الحمولة JSON.
            handleNotifyRequest(s, signal.substring("نبّه:".length()));
        } else if (signal.startsWith("أزل_تنبيه:")) {
            // طلب إزالة تنبيه من المحرك (المهمة 21): بلا انتظار.
            handleRemoveNotify(s, signal.substring("أزل_تنبيه:".length()));
        } else if (signal.startsWith("انطق:")) {
            // طلب نطق من المحرك (المهمة 23): الحمولة JSON.
            handleSpeakRequest(s, signal.substring("انطق:".length()));
        } else if (signal.startsWith("محركات_النطق:")) {
            // طلب سرد محركات النطق من المحرك (المهمة 23).
            handleListEngines(s, signal.substring("محركات_النطق:".length()));
        } else if (signal.startsWith("شغّل_صوت:")) {
            // طلب تشغيل صوت من المحرك (المهمة 24): الحمولة JSON.
            handlePlayAudioRequest(s, signal.substring("شغّل_صوت:".length()));
        } else if (signal.startsWith("أوقف_صوت:")) {
            // طلب إيقاف صوت من المحرك (المهمة 24): بلا حمولة تذكر.
            handleStopAudioRequest(s, signal.substring("أوقف_صوت:".length()));
        } else if (signal.startsWith("حساس:")) {
            // طلب قراءة حساس من المحرك (المهمة 25): الحمولة JSON.
            handleSensorRequest(s, signal.substring("حساس:".length()));
        } else if (signal.startsWith("اسمع:")) {
            // طلب استماع من المحرك (المهمة 26): الحمولة JSON.
            handleListenRequest(s, signal.substring("اسمع:".length()));
        } else if (signal.startsWith("بطارية:")) {
            // طلب قراءة بطارية من المحرك (المهمة 26): الحمولة JSON.
            handleBatteryRequest(s, signal.substring("بطارية:".length()));
        } else if (signal.startsWith("شارك_ملف:")) {
            // مشاركة ملف من البرنامج (الدفعة 4أ — البند 22).
            handleShareFileRequest(s, signal.substring("شارك_ملف:".length()));
        } else if (signal.startsWith("شارك_نص:")) {
            // مشاركة نص من البرنامج (الدفعة 4أ — البند 22).
            handleShareTextRequest(s, signal.substring("شارك_نص:".length()));
        } else if (signal.startsWith("افتح_ملف:")) {
            // فتح ملف بتطبيق خارجي (الدفعة 4أ — البند 23).
            handleOpenFileRequest(s, signal.substring("افتح_ملف:".length()));
        } else if (signal.startsWith("افتح_رابط:")) {
            // فتح رابط في المتصفح (الدفعة 4أ — البند 23).
            handleOpenUrlRequest(s, signal.substring("افتح_رابط:".length()));
        } else if (signal.startsWith("انسخ_للحافظة:")) {
            // كتابة نص في الحافظة (الدفعة 4أ — البند 24).
            handleClipboardSetRequest(s, signal.substring("انسخ_للحافظة:".length()));
        } else if (signal.startsWith("الحافظة:")) {
            // قراءة نص الحافظة (الدفعة 4أ — البند 25).
            handleClipboardGetRequest(s, signal.substring("الحافظة:".length()));
        } else if (signal.startsWith("اختر_ملف:")) {
            // منتقي ملف عام (الدفعة 4أ — البند 28).
            handlePickFileRequest(s, signal.substring("اختر_ملف:".length()));
        } else if (signal.startsWith("موقع:")) {
            // آخر موقع معروف (الدفعة 4أ — البند 29).
            handleLocationRequest(s, signal.substring("موقع:".length()), false);
        } else if (signal.startsWith("موقع_حي:")) {
            // قراءة موقع مباشرة بمهلة (الدفعة 4أ — البند 30).
            handleLocationRequest(s, signal.substring("موقع_حي:".length()), true);
        } else if (signal.startsWith("كاميرات:")) {
            // سرد الكاميرات (الدفعة 4أ — البند 31).
            handleCamerasRequest(s, signal.substring("كاميرات:".length()));
        } else if (signal.startsWith("التقط_صورة:")) {
            // التقاط صورة إلى ملف (الدفعة 4أ — البند 32).
            handleCapturePhotoRequest(s, signal.substring("التقط_صورة:".length()));
        } else if (signal.startsWith("واي_فاي:")) {
            // معلومات اتصال واي فاي الحالي (الدفعة 4أ — البند 33).
            handleWifiInfoRequest(s, signal.substring("واي_فاي:".length()));
        } else if (signal.startsWith("مسح_واي_فاي:")) {
            // مسح الشبكات القريبة (الدفعة 4أ — البند 34).
            handleWifiScanRequest(s, signal.substring("مسح_واي_فاي:".length()));
        } else if (signal.startsWith("انسخ_آخر_خطأ:")) {
            // نسخ آخر سطر خطأ (الدفعة 4ب — البند 61).
            handleCopyLastErrorRequest(s, signal.substring("انسخ_آخر_خطأ:".length()));
        } else if (signal.startsWith("شارك_الجلسة:")) {
            // مشاركة نص الجلسة الحالية (الدفعة 4ب — البند 62).
            handleShareSessionRequest(s, signal.substring("شارك_الجلسة:".length()));
        } else if (signal.startsWith("أوقف_صوت_مؤقتا:")) {
            // إيقاف الصوت مؤقتا (الدفعة 4ب — البند 66).
            handlePauseAudioRequest(s, signal.substring("أوقف_صوت_مؤقتا:".length()));
        } else if (signal.startsWith("استأنف_صوت:")) {
            // استئناف الصوت الموقوف مؤقتا (الدفعة 4ب — البند 66).
            handleResumeAudioRequest(s, signal.substring("استأنف_صوت:".length()));
        } else if (signal.startsWith("معلومات_صوت:")) {
            // معلومات ملف صوت قبل تشغيله (الدفعة 4ب — البند 67).
            handleAudioInfoRequest(s, signal.substring("معلومات_صوت:".length()));
        } else if (signal.startsWith("حساسات:")) {
            // سرد الحساسات الموجودة (الدفعة 4ب — البند 68).
            handleListSensorsRequest(s, signal.substring("حساسات:".length()));
        } else if (signal.startsWith("مستوى_الصوت:")) {
            // قراءة مستوى الصوت (الدفعة 4ب — البند 69).
            handleVolumeLevelRequest(s, signal.substring("مستوى_الصوت:".length()));
        } else if (signal.startsWith("اضبط_مستوى_الصوت:")) {
            // ضبط مستوى الصوت (الدفعة 4ب — البند 70).
            handleSetVolumeRequest(s, signal.substring("اضبط_مستوى_الصوت:".length()));
        } else if (signal.startsWith("اهتز:")) {
            // اهتزاز من البرنامج (الدفعة 4ب — البند 71).
            handleVibrateRequest(s, signal.substring("اهتز:".length()));
        } else if (signal.startsWith("مصباح:")) {
            // مصباح من البرنامج (الدفعة 4ب — البند 72).
            handleTorchRequest(s, signal.substring("مصباح:".length()));
        } else if (signal.startsWith("سجل_صوت:")) {
            // تسجيل صوت إلى ملف (الدفعة 4ب — البند 79).
            handleRecordAudioRequest(s, signal.substring("سجل_صوت:".length()));
        } else if (signal.startsWith("سطوع:")) {
            // سطوع من البرنامج (الدفعة 4ب — البند 105).
            handleBrightnessRequest(s, signal.substring("سطوع:".length()));
        } else if (signal.startsWith("توست:")) {
            // توست من البرنامج (الدفعة 4ب — البند 106).
            handleToastRequest(s, signal.substring("توست:".length()));
        } else if (signal.startsWith("معلومات_الصوت:")) {
            // معلومات الصوت (الدفعة 4ب — البند 119).
            handleAudioDeviceInfoRequest(s, signal.substring("معلومات_الصوت:".length()));
        } else if (signal.startsWith("نزل:")) {
            // تنزيل نظامي (الدفعة 5 — البند 98).
            handleDownloadRequest(s, signal.substring("نزل:".length()));
        } else if (signal.startsWith("اقرأ_مستند:")) {
            // قراءة مستند لمرة واحدة (الدفعة 5 — البند 99 خطوة 1).
            handleReadDocumentRequest(s, signal.substring("اقرأ_مستند:".length()));
        } else if (signal.startsWith("اعرض_مجلد:")) {
            // عرض مجلد لمرة واحدة (الدفعة 5 — البند 99 خطوة 1).
            handleListFolderRequest(s, signal.substring("اعرض_مجلد:".length()));
        } else if (signal.startsWith("ملف-ممنوح:")) {
            // طلب SAF من المحرك (المهمة 28): الحمولة JSON.
            handleSafControl(s, signal.substring("ملف-ممنوح:".length()));
        }
    }

    // ===== تنبيه جهاز.نبّه + أزل_تنبيه (المهمة 21) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": <معرف>} أو
    // {"خطأ": "..."} — كما الحوار تماما (البادئة
    // نفسها). والإزالة بلا انتظار: إلغاء النظام
    // متزامن ولا يفشل فشلا ذا معنى حين يكون المعرّف
    // حيّا. القناة تُنشأ تلقائيا عند أول نبّه (لا دالة
    // قناة للمستخدم). التنبيه في شريط النظام لا في
    // واجهة جلسة — فيعمل للنشطة والخلفية معا، ويعيش
    // بعد البرنامج حتى يُمسح أو يُزال بالمعرّف.
    // بوابة الإذن: POST_NOTIFICATIONS (أندرويد 13+)
    // يُطلب عند أول نبّه بلا إذن فقط — لا عند الفتح
    // ولا عند بدء جلسة.

    /** هل إذن الإشعارات ممنوح (دون 13: وقت التثبيت)؟ */
    private boolean notifyPermissionGranted() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return true;
        return checkSelfPermission("android.permission.POST_NOTIFICATIONS")
                == PackageManager.PERMISSION_GRANTED;
    }

    /** إنشاء قناة تنبيهات البرامج كسولا (أندرويد 8+). */
    private void ensureNotifyChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm == null) return;
        NotificationChannel channel = new NotificationChannel(
                NOTIFY_CHANNEL_ID, "تنبيهات نيفا",
                NotificationManager.IMPORTANCE_DEFAULT);
        channel.setDescription("تنبيهات برامج نيفا");
        nm.createNotificationChannel(channel);
    }

    /** استقبال طلب تنبيه من قناة التحكم. */
    private void handleNotifyRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendNotifyError(s, "طلب تنبيه غير صالح");
            return;
        }
        long id = req.optLong("معرف", -1);
        String title = req.optString("عنوان", "");
        String text = req.optString("نص", "");
        if (id < 1 || title.isEmpty() || text.isEmpty()) {
            sendNotifyError(s, "طلب تنبيه غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendNotifyError(s, "التطبيق غير متاح");
            return;
        }
        if (notifyPermissionGranted()) {
            postNotify(s, id, title, text);
            sendNotifyValue(s, id);
            return;
        }
        // بلا إذن: يُحفظ ويُطلب الإذن بحوار النظام؛
        // وإن كان طلب جاريا يُكتفى بالطابور (لا حوار
        // ثان فوق الأول).
        pendingNotifies.add(new PendingNotify(s, id, title, text));
        if (!notifyPermissionRequestInFlight) {
            notifyPermissionRequestInFlight = true;
            requestPermissions(
                    new String[]{"android.permission.POST_NOTIFICATIONS"},
                    REQ_NOTIFY_PERMISSION);
        }
    }

    /** جواب منتقيي SAF (المهمة 28): المنح (معرف جديد
     * أو إلغاء) والتشغيل (تحقق ثم نسخ ثم جلسة جديدة). */
    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQ_SAF_GRANT) {
            Session s = safGrantSession;
            safGrantSession = null;
            if (s == null) return;
            if (s.safCancelArmed) {
                // إلغاء مسلّح — المحرك أجهض الانتظار؛
                // فلا نتيجة تُرسَل (سطر شارد).
                s.safCancelArmed = false;
                s.safWaitPending = false;
                return;
            }
            if (resultCode != RESULT_OK || data == null || data.getData() == null) {
                sendSafError(s, "ألغى المستخدم اختيار المجلد");
                return;
            }
            Uri treeUri = data.getData();
            try {
                getContentResolver().takePersistableUriPermission(treeUri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION
                                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            } catch (Exception e) {
                sendSafError(s, "تعذّر حفظ المنحة");
                return;
            }
            long id = nextGrantId++;
            grants.add(new Grant(id, safTreeDisplayName(treeUri), treeUri));
            saveGrants();
            // الجلسة ماتت أثناء المنتقي؟ المنحة تُحفظ
            // (اختارها المستخدم) لكن بلا نتيجة تُرسَل.
            if (s.process == null || s.ended) return;
            sendSafValue(s, id);
            return;
        }
        if (requestCode == REQ_SAF_RUN) {
            if (resultCode != RESULT_OK || data == null || data.getData() == null) {
                // إلغاء المنتقي: لا شيء إطلاقا.
                return;
            }
            runChosenNivaFile(data.getData());
            return;
        }
        if (requestCode == REQ_PICK_FILE) {
            // جواب منتقي الملف العام (الدفعة 4أ — البند 28).
            onPickFileResult(resultCode, data);
            return;
        }
        if (requestCode == REQ_CAPTURE_PHOTO) {
            // جواب نية الكاميرا (الدفعة 4أ — البند 32).
            onCapturePhotoResult(resultCode);
            return;
        }
        if (requestCode == REQ_READ_DOCUMENT) {
            // جواب منتقي المستند (الدفعة 5 — البند 99 خطوة 1).
            onReadDocumentResult(resultCode, data);
            return;
        }
        if (requestCode == REQ_LIST_FOLDER) {
            // جواب منتقي المجلد (الدفعة 5 — البند 99 خطوة 1).
            onListFolderResult(resultCode, data);
            return;
        }
        if (requestCode == REQ_STORAGE_SETTINGS) {
            // جواب شاشة إعدادات إذن التخزين (الدفعة 5 —
            // البندان 94 و96): منح — يُكشف جذر
            // التنزيلات في الجلسة الجارية بلا إعادة
            // تشغيل؛ رفض — سطر عربي فقط، بلا إعادة
            // فتح الشاشة.
            Session s = pendingStorageSession;
            pendingStorageSession = null;
            if (s == null) return;
            if (s.process == null || s.ended) return;
            if (storageManagerGranted()) {
                sendToEngine(s, SET_DOWNLOADS_PREFIX + downloadsPath());
                appLine(s, "التخزين مكشوف: تخزين/تنزيلات");
            } else {
                appLine(s, "لم يُمنح إذن التخزين");
            }
            return;
        }
    }

    /** جواب المستخدم على حوار إذن الإشعارات: يُفرَّغ
     * الطابور — منح: نشر + نتائج قيم؛ رفض: نتائج
     * أخطاء (فيعلم البرنامج ويقرر بديله). */
    @Override
    public void onRequestPermissionsResult(int requestCode,
            String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQ_NOTIFY_PERMISSION) {
            notifyPermissionRequestInFlight = false;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            drainPendingNotifies(granted);
            return;
        }
        if (requestCode == REQ_LISTEN_PERMISSION) {
            listenPermissionRequestInFlight = false;
            if (listeningNow == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            if (granted) {
                startListening();
            } else {
                Session s = listeningNow.session;
                listeningNow = null;
                sendListenError(s, "رفض إذن الميكروفون — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
        if (requestCode == REQ_LOCATION_PERMISSION) {
            // جواب إذن الموقع (الدفعة 4أ — البندان 29 و30):
            // منح: نفّذ المعلّق؛ رفض: خطأ عربي واضح.
            locationPermissionRequestInFlight = false;
            if (pendingLocation == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            if (granted) {
                runLocationRequest(pendingLocation);
            } else {
                PendingLocation p = pendingLocation;
                pendingLocation = null;
                p.answered = true;
                sendDeviceError(p.session,
                        "رفض إذن الموقع — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
        if (requestCode == REQ_CAPTURE_PERMISSION) {
            // جواب إذن الكاميرا (الدفعة 4أ — البند 32).
            capturePermissionRequestInFlight = false;
            if (pendingCaptureSession == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            if (granted) {
                launchCapturePhoto();
            } else {
                Session s = pendingCaptureSession;
                pendingCaptureSession = null;
                pendingCaptureFile = null;
                sendDeviceError(s,
                        "رفض إذن الكاميرا — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
        if (requestCode == REQ_WIFI_SCAN_PERMISSION) {
            // جواب إذن مسح واي فاي (الدفعة 4أ — البند 34).
            wifiScanPermissionRequestInFlight = false;
            if (pendingWifiScan == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            if (granted) {
                runWifiScan(pendingWifiScan);
            } else {
                PendingWifiScan p = pendingWifiScan;
                pendingWifiScan = null;
                p.answered = true;
                sendDeviceError(p.session,
                        "رفض إذن الموقع — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
        if (requestCode == REQ_TORCH_PERMISSION) {
            // جواب إذن الكاميرا للمصباح (الدفعة 4ب — البند 72).
            torchPermissionRequestInFlight = false;
            if (pendingTorch == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            PendingTorch p = pendingTorch;
            pendingTorch = null;
            if (granted) {
                setTorchMode(p.session, p.on);
            } else {
                sendDeviceError(p.session,
                        "رفض إذن الكاميرا — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
        if (requestCode == REQ_RECORD_PERMISSION) {
            // جواب إذن الميكروفون للتسجيل (الدفعة 4ب — البند 79).
            recordPermissionRequestInFlight = false;
            if (pendingRecord == null) return;
            boolean granted = grantResults.length > 0
                    && grantResults[0] == PackageManager.PERMISSION_GRANTED;
            PendingRecord p = pendingRecord;
            pendingRecord = null;
            if (granted) {
                startRecording(p);
            } else {
                sendDeviceError(p.session,
                        "رفض إذن الميكروفون — فعّل الإذن من إعدادات التطبيق");
            }
            return;
        }
    }

    /** تفريغ طابور التنبيهات المعلّقة بإذن. */
    private void drainPendingNotifies(boolean granted) {
        ArrayList<PendingNotify> queued = new ArrayList<>(pendingNotifies);
        pendingNotifies.clear();
        for (PendingNotify p : queued) {
            if (granted) {
                postNotify(p.session, p.id, p.title, p.text);
                sendNotifyValue(p.session, p.id);
            } else {
                sendNotifyError(p.session,
                        "الإشعارات مرفوضة — فعّلها من إعدادات التطبيق");
            }
        }
    }

    /** نشر التنبيه في شريط النظام (معرّف نظام فريد
     * يُسجَّل في خريطة الجلسة للإزالة لاحقا). */
    private void postNotify(Session s, long id, String title, String text) {
        ensureNotifyChannel();
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm == null) return;
        int systemId = nextSystemNotifyId++;
        s.notifySystemIds.put(id, systemId);
        Intent open = new Intent(this, MainActivity.class);
        open.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP
                | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent openPi = PendingIntent.getActivity(this, systemId, open,
                PendingIntent.FLAG_UPDATE_CURRENT
                        | PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, NOTIFY_CHANNEL_ID);
        } else {
            builder = new Notification.Builder(this);
            builder.setPriority(Notification.PRIORITY_DEFAULT);
        }
        builder.setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(title)
                .setContentText(text)
                .setContentIntent(openPi)
                .setAutoCancel(true);
        nm.notify(systemId, builder.build());
    }

    /** إنشاء قناة تقارير الأعطال الصامتة كسولا
     * (الدفعة 5 — البند 107): أهمية منخفضة — فلا
     * صوت ولا اهتزاز ولا ظهور فوق التطبيق. */
    private void ensureCrashChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm == null) return;
        NotificationChannel channel = new NotificationChannel(
                CRASH_CHANNEL_ID, "تقارير الأعطال",
                NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("تقارير فشل تحديث نيفا");
        nm.createNotificationChannel(channel);
    }

    /** تقرير عطل صامت (الدفعة 5 — البند 107): إشعار
     * واحد بمعرّف ثابت (فالأحدث يحل محل السابق)،
     * والضغط عليه يفتح التطبيق، ويختفي وحده عند
     * الضغط. */
    private void postCrashNotification(String msg) {
        ensureCrashChannel();
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm == null) return;
        Intent open = new Intent(this, MainActivity.class);
        open.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP
                | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent openPi = PendingIntent.getActivity(this,
                CRASH_NOTIFICATION_ID, open,
                PendingIntent.FLAG_UPDATE_CURRENT
                        | PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, CRASH_CHANNEL_ID);
        } else {
            builder = new Notification.Builder(this);
            builder.setPriority(Notification.PRIORITY_LOW);
        }
        builder.setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle("تعذّر تحديث نيفا")
                .setContentText(msg)
                .setContentIntent(openPi)
                .setAutoCancel(true);
        nm.notify(CRASH_NOTIFICATION_ID, builder.build());
    }

    /** استقبال طلب إزالة تنبيه (بلا انتظار — بلا نتيجة). */
    private void handleRemoveNotify(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            return;
        }
        long id = req.optLong("معرف", -1);
        if (id < 1) return;
        // معلّق بإذن؟ يُحذف من الطابور فلا يظهر أبدا
        // (بدلا من نشره ثم إلغائه).
        for (int i = pendingNotifies.size() - 1; i >= 0; i--) {
            if (pendingNotifies.get(i).id == id
                    && pendingNotifies.get(i).session == s) {
                pendingNotifies.remove(i);
                return;
            }
        }
        Integer systemId = s.notifySystemIds.remove(id);
        if (systemId == null) return;
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm != null) nm.cancel(systemId);
    }

    /** إسقاط تنبيهات الجلسة المعلّقة بإذن — عند إعادة
     * التشغيل أو موت العملية (المحرك ميت فلا نتيجة
     * تُرسل)؛ ومسح خريطتها (محرك جديد = عدّاد جديد).
     * والمنشور يبقى عمدا — حتى يمسحه المستخدم. */
    private void dropPendingNotifies(Session s) {
        for (int i = pendingNotifies.size() - 1; i >= 0; i--) {
            if (pendingNotifies.get(i).session == s) pendingNotifies.remove(i);
        }
        s.notifySystemIds.clear();
    }

    /** إرسال نتيجة تنبيه واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار. */
    private void sendNotifyResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendNotifyValue(Session s, long id) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", id);
            sendNotifyResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendNotifyError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendNotifyResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ===== نطق جهاز.انطق + محركات_النطق (المهمة 23) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": true} عند انتهاء
    // النطق، أو {"قيمة": [...]} لقائمة المحركات، أو
    // {"خطأ": "..."} — كما الحوار والتنبيه تماما
    // (البادئة نفسها). كائن TextToSpeech واحد على
    // مستوى النشاط (تهيئة كسولة عند أول طلب)؛ واللغة
    // عربية دائما في v1؛ والنطق بلا إذن نظام (البوابة
    // توفر محرك نطق وصوت عربي)؛ ونطق واحد جارٍ في
    // كل التطبيق (طلب ثانٍ متزامن يرفض فورا)؛
    // والجلسة الخلفية تنطق فورا كالنشطة (النطق صوت
    // لا واجهة). إعادة تشغيل جلسة أو موتها أثناء
    // نطقها توقف الصوت وتمسح السجل (المحرك مات فلا
    // نتيجة تُرسَل إليه).

    /** تهيئة TTS كسولة (مرة واحدة): عند النجاح تُضبط
     * العربية ويُفرَّغ طابور المعلّق؛ وعند الفشل
     * يُفرَّغ بأخطاء. */
    private void ensureTts() {
        if (ttsInitStarted) return;
        ttsInitStarted = true;
        try {
            tts = new TextToSpeech(this, status -> {
                runOnUiThread(() -> {
                    if (status == TextToSpeech.SUCCESS) {
                        ttsReady = true;
                        try {
                            tts.setOnUtteranceProgressListener(
                                    new UtteranceProgressListener() {
                                @Override
                                public void onStart(String id) {
                                }

                                @Override
                                public void onDone(String id) {
                                    runOnUiThread(() -> onSpeakDone(id));
                                }

                                @Override
                                public void onError(String id) {
                                    runOnUiThread(() -> onSpeakError(id));
                                }

                                @Override
                                public void onError(String id, int code) {
                                    runOnUiThread(() -> onSpeakError(id));
                                }

                                @Override
                                public void onStop(String id, boolean interrupted) {
                                    runOnUiThread(() -> onSpeakStopped(id));
                                }
                            });
                            int lang = tts.setLanguage(new Locale("ar"));
                            ttsArabicOk = lang != TextToSpeech.LANG_MISSING_DATA
                                    && lang != TextToSpeech.LANG_NOT_SUPPORTED;
                        } catch (Exception ignored) {
                            ttsArabicOk = false;
                        }
                        drainPendingSpeaks();
                    } else {
                        ttsInitFailed = true;
                        drainPendingSpeaks();
                    }
                });
            });
        } catch (Exception ignored) {
            ttsInitFailed = true;
            drainPendingSpeaks();
        }
    }

    /** تفريغ طابور طلبات النطق المعلّقة بالتهيئة:
     * الأول ينطق، والباقي «نطق آخر جارٍ»؛ وعند فشل
     * التهيئة الكل «تعذّر تهيئة محرك النطق». */
    private void drainPendingSpeaks() {
        ArrayList<PendingSpeak> queued = new ArrayList<>(pendingSpeaks);
        pendingSpeaks.clear();
        for (PendingSpeak p : queued) {
            if (p.session.process == null) continue;
            if (ttsInitFailed) {
                sendSpeakError(p.session, "تعذّر تهيئة محرك النطق");
            } else if (!ttsArabicOk) {
                sendSpeakError(p.session,
                        "الصوت العربي غير متاح على هذا الجهاز — ثبّت بيانات الصوت العربية من إعدادات النطق");
            } else if (speakingNow != null) {
                sendSpeakError(p.session, "نطق آخر جارٍ — انتظر انتهاءه");
            } else {
                startSpeak(p.session, p.text);
            }
        }
    }

    /** استقبال طلب نطق من قناة التحكم. */
    private void handleSpeakRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendSpeakError(s, "طلب نطق غير صالح");
            return;
        }
        String text = req.optString("نص", "");
        if (text.isEmpty()) {
            sendSpeakError(s, "طلب نطق غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendSpeakError(s, "التطبيق غير متاح");
            return;
        }
        if (speakingNow != null) {
            sendSpeakError(s, "نطق آخر جارٍ — انتظر انتهاءه");
            return;
        }
        if (ttsInitFailed) {
            sendSpeakError(s, "تعذّر تهيئة محرك النطق");
            return;
        }
        if (!ttsReady) {
            pendingSpeaks.add(new PendingSpeak(s, text));
            ensureTts();
            return;
        }
        if (!ttsArabicOk) {
            sendSpeakError(s,
                    "الصوت العربي غير متاح على هذا الجهاز — ثبّت بيانات الصوت العربية من إعدادات النطق");
            return;
        }
        startSpeak(s, text);
    }

    /** بدء نطق نص لجلسة (بعد كل الفحوص): يسجّل
     * النطق الجاري ثم ينطق بـ QUEUE_FLUSH. */
    private void startSpeak(Session s, String text) {
        if (tts == null) {
            sendSpeakError(s, "تعذّر تهيئة محرك النطق");
            return;
        }
        String id = "niva-" + (nextSpeakUtterance++);
        speakingNow = new PendingSpeak(s, text);
        speakingUtteranceId = id;
        int rc;
        try {
            rc = tts.speak(text, TextToSpeech.QUEUE_FLUSH, null, id);
        } catch (Exception ignored) {
            rc = TextToSpeech.ERROR;
        }
        if (rc == TextToSpeech.ERROR) {
            speakingNow = null;
            speakingUtteranceId = null;
            sendSpeakError(s, "تعذّر النطق");
        }
    }

    /** انتهى النطق الجاري بنجاح: مسح السجل + نتيجة
     * صحيح للجلسة الناطقة. الردود القديمة تُتجاهل. */
    private void onSpeakDone(String id) {
        if (speakingNow == null || !id.equals(speakingUtteranceId)) return;
        Session s = speakingNow.session;
        speakingNow = null;
        speakingUtteranceId = null;
        sendSpeakValue(s);
    }

    /** خطأ نطق: مسح السجل + نتيجة خطأ للجلسة. */
    private void onSpeakError(String id) {
        if (speakingNow == null || !id.equals(speakingUtteranceId)) return;
        Session s = speakingNow.session;
        speakingNow = null;
        speakingUtteranceId = null;
        sendSpeakError(s, "تعذّر النطق");
    }

    /** أُوقف النطق (stop): مسح السجل + نتيجة خطأ
     * للجلسة — إلا إن كان السجل ممسوحا أصلا (إيقاف
     * من إعادة تشغيل الجلسة نفسها فلا مجيب). */
    private void onSpeakStopped(String id) {
        if (speakingNow == null || !id.equals(speakingUtteranceId)) return;
        Session s = speakingNow.session;
        speakingNow = null;
        speakingUtteranceId = null;
        sendSpeakError(s, "أُوقف النطق");
    }

    /** إيقاف نطق جلسة تموت ومسح معلّقاتها (المحرك
     * مات فلا نتيجة تُرسَل إليه) — يُستدعى من
     * restartProcess وonProcessEnded. */
    private void stopSpeakForSession(Session s) {
        if (speakingNow != null && speakingNow.session == s) {
            if (tts != null) {
                try {
                    tts.stop();
                } catch (Exception ignored) {
                }
            }
            speakingNow = null;
            speakingUtteranceId = null;
        }
        for (int i = pendingSpeaks.size() - 1; i >= 0; i--) {
            if (pendingSpeaks.get(i).session == s) pendingSpeaks.remove(i);
        }
    }

    /** استقبال طلب سرد محركات النطق من قناة التحكم:
     * بلا انتظار تهيئة (getEngines استعلام مدير حزم)
     * — ويرجع قائمة قواميس {اسم، وصف} (فارغة إن لم
     * يوجد محرك). */
    private void handleListEngines(Session s, String payload) {
        if (isFinishing() || isDestroyed()) {
            sendSpeakError(s, "التطبيق غير متاح");
            return;
        }
        ensureTts();
        if (tts == null) {
            sendSpeakError(s, "تعذّر تهيئة محرك النطق");
            return;
        }
        try {
            JSONArray arr = new JSONArray();
            java.util.List<TextToSpeech.EngineInfo> engines = tts.getEngines();
            if (engines != null) {
                for (TextToSpeech.EngineInfo e : engines) {
                    JSONObject o = new JSONObject();
                    o.put("اسم", e.name == null ? "" : e.name);
                    o.put("وصف", e.label == null ? "" : e.label);
                    arr.put(o);
                }
            }
            JSONObject r = new JSONObject();
            r.put("قيمة", arr);
            sendLineToEngine(s, DIALOG_RESULT_PREFIX + r.toString());
        } catch (Exception ignored) {
            sendSpeakError(s, "تعذّر تهيئة محرك النطق");
        }
    }

    /** إرسال نتيجة نطق واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار والتنبيه. */
    private void sendSpeakResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendSpeakValue(Session s) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", true);
            sendSpeakResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendSpeakError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendSpeakResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ===== تشغيل صوت جهاز.شغّل_صوت + أوقف_صوت (المهمة 24) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": true} عند بدء
    // التشغيل أو عند الإيقاف، أو {"خطأ": "..."} — كما
    // الحوار والتنبيه والنطق تماما (البادئة نفسها).
    // التشغيل غير حاجب: يرجع فورا ويكمل الصوت في
    // الخلفية (عكس النطق) — وإلا ما استطاع البرنامج
    // استدعاء أوقف_صوت أبدا. مشغّل واحد في كل
    // التطبيق؛ وتشغيل جديد يوقف الجاري ويستبدله
    // (آمن لأن التشغيل لا ينتظر أحد انتهاءه — على
    // عكس النطق الذي يرفض الثاني). والجلسة الخلفية
    // تشغّل فورا كالنشطة (الصوت لا واجهة). والمسار
    // مطلق محلول من المحرك عبر FilePolicy (التطبيق
    // لا يعرف مجلد البرنامج). بلا إذن نظام (ملف
    // محلي في مجلد البرنامج). والاكتمال والخطأ
    // أثناء التشغيل صمت (لا نتيجة معلّقة أصلا).

    /** إيقاف الصوت الجاري وتحرير المشغّل ومسح
     * السجل — بلا نتيجة (داخلي). */
    private void releaseAudio() {
        if (audioPlayer != null) {
            try {
                audioPlayer.stop();
            } catch (Exception ignored) {
            }
            try {
                audioPlayer.release();
            } catch (Exception ignored) {
            }
            audioPlayer = null;
        }
        audioPlayingSession = null;
        audioPaused = false;
    }

    /** استقبال طلب تشغيل صوت من قناة التحكم. */
    private void handlePlayAudioRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendAudioError(s, "طلب صوت غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        if (path.isEmpty()) {
            sendAudioError(s, "طلب صوت غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendAudioError(s, "التطبيق غير متاح");
            return;
        }
        // تشغيل جديد يستبدل الجاري (بلا رفض — انظر
        // CATALOG-16): إيقافه وتحريره أولا.
        releaseAudio();
        MediaPlayer player = new MediaPlayer();
        try {
            player.setDataSource(path);
        } catch (java.io.FileNotFoundException e) {
            try {
                player.release();
            } catch (Exception ignored) {
            }
            sendAudioError(s, "ملف الصوت غير موجود");
            return;
        } catch (Exception e) {
            try {
                player.release();
            } catch (Exception ignored) {
            }
            sendAudioError(s, "ملف الصوت غير مدعوم أو تالف");
            return;
        }
        player.setOnCompletionListener(mp -> {
            runOnUiThread(() -> {
                if (audioPlayer == mp) releaseAudio();
            });
        });
        player.setOnErrorListener((mp, what, extra) -> {
            runOnUiThread(() -> {
                if (audioPlayer == mp) releaseAudio();
            });
            return true;
        });
        try {
            // تجهيز متزامن على خيط الواجهة — ملف
            // محلي سريع عادة؛ والحد الصادق: ملف شاذ
            // قد يجمّد الواجهة لحظة (وprepareAsync
            // خطوة لاحقة).
            player.prepare();
            player.start();
        } catch (Exception e) {
            try {
                player.release();
            } catch (Exception ignored) {
            }
            sendAudioError(s, "ملف الصوت غير مدعوم أو تالف");
            return;
        }
        audioPlayer = player;
        audioPlayingSession = s;
        sendAudioValue(s);
    }

    /** استقبال طلب إيقاف صوت من قناة التحكم — بلا
     * حمولة تذكر: إيقاف الجاري (إن وُجد) ثم نتيجة
     * صحيح دائما (بلا صوت جارٍ نجاح صامت). */
    private void handleStopAudioRequest(Session s, String payload) {
        if (isFinishing() || isDestroyed()) {
            sendAudioError(s, "التطبيق غير متاح");
            return;
        }
        releaseAudio();
        sendAudioValue(s);
    }

    /** إيقاف صوت جلسة تموت ومسح سجلها (المحرك مات
     * فلا نتيجة تُرسَل إليه) — يُستدعى من
     * restartProcess وonProcessEnded بجانب
     * stopSpeakForSession. وموت جلسة أخرى لا يوقف
     * صوت الأولى (السجل يخص جلسة واحدة فقط). */
    private void stopAudioForSession(Session s) {
        if (audioPlayingSession == s) {
            releaseAudio();
        }
    }

    /** إرسال نتيجة صوت واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار والتنبيه والنطق. */
    private void sendAudioResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendAudioValue(Session s) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", true);
            sendAudioResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendAudioError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendAudioResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ===== قراءة حساس جهاز.حساس (المهمة 25) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": {"قيم": [...],
    // "دقة": n}} أو {"خطأ": "..."} — كما الحوار تماما
    // (البادئة نفسها). القراءة لمرة واحدة: مستمع
    // يسجَّل وينتظر أول حدث ثم يلغي تسجيله؛ ومهلة
    // 5 ثوان تجيب بخطأ إن لم يصل حدث — فكل طلب يجيب
    // بنتيجة واحدة بالضبط، فلا يعلق المحرك أبدا.
    // بلا إذن نظام (الحساسات الثمانية بلا إذن).

    /** نوع حساس أندرويد لاسم نيفا — الثمانية في v1
     * (القائمة نفسها في المحرك SENSOR_NAMES)؛ و-1
     * لاسم غير معروف (لا يصل عمليا — المحرك يفحص
     * الاسم أولا). */
    private static int sensorTypeForName(String name) {
        switch (name) {
            case "تسارع": return Sensor.TYPE_ACCELEROMETER;
            case "جيروسكوب": return Sensor.TYPE_GYROSCOPE;
            case "ضوء": return Sensor.TYPE_LIGHT;
            case "قرب": return Sensor.TYPE_PROXIMITY;
            case "مغناطيسي": return Sensor.TYPE_MAGNETIC_FIELD;
            case "ضغط": return Sensor.TYPE_PRESSURE;
            case "رطوبة": return Sensor.TYPE_RELATIVE_HUMIDITY;
            case "حرارة": return Sensor.TYPE_AMBIENT_TEMPERATURE;
            default: return -1;
        }
    }

    /** استقبال طلب قراءة حساس من قناة التحكم. */
    private void handleSensorRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendSensorError(s, "طلب حساس غير صالح");
            return;
        }
        String name = req.optString("اسم", "");
        if (name.isEmpty()) {
            sendSensorError(s, "طلب حساس غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendSensorError(s, "التطبيق غير متاح");
            return;
        }
        int type = sensorTypeForName(name);
        if (type < 0) {
            sendSensorError(s, "حساس غير معروف: " + name);
            return;
        }
        if (sensorManager == null) {
            sensorManager = (SensorManager) getSystemService(Context.SENSOR_SERVICE);
        }
        if (sensorManager == null) {
            sendSensorError(s, "الحساس غير متاح على هذا الجهاز");
            return;
        }
        Sensor sensor = sensorManager.getDefaultSensor(type);
        if (sensor == null) {
            sendSensorError(s, "الحساس غير متاح على هذا الجهاز");
            return;
        }
        // مستمع لمرة واحدة: أول حدث يجيب ويلغي
        // التسجيل؛ ومهلة 5 ثوان تجيب بخطأ إن لم
        // يصل حدث — وfinishSensor تحرس الإجابة
        // الواحدة (حدث ومهلة معا).
        final PendingSensor pending = new PendingSensor(s);
        SensorEventListener listener = new SensorEventListener() {
            @Override
            public void onSensorChanged(SensorEvent event) {
                float[] values = event.values.clone();
                int accuracy = event.accuracy;
                finishSensor(pending, values, accuracy);
            }

            @Override
            public void onAccuracyChanged(Sensor sensor, int accuracy) {
                // الدقة تُؤخذ من الحدث نفسه — لا تجيب.
            }
        };
        pending.listener = listener;
        pending.timeout = new Runnable() {
            @Override
            public void run() {
                finishSensor(pending, null, 0);
            }
        };
        pendingSensors.add(pending);
        sensorManager.registerListener(listener, sensor, SensorManager.SENSOR_DELAY_NORMAL);
        new Handler(Looper.getMainLooper()).postDelayed(pending.timeout, 5000);
    }

    /** إنهاء طلب قراءة معلّق بنتيجة واحدة بالضبط:
     * قيم غير null ← قيمة القراءة؛ وnull ← خطأ
     * المهلة. يلغي التسجيل والمهلة ويمسح السجل. */
    private void finishSensor(PendingSensor pending, float[] values, int accuracy) {
        if (pending.answered) return;
        pending.answered = true;
        pendingSensors.remove(pending);
        if (sensorManager != null && pending.listener != null) {
            try {
                sensorManager.unregisterListener(pending.listener);
            } catch (Exception ignored) {
            }
        }
        if (pending.timeout != null) {
            new Handler(Looper.getMainLooper()).removeCallbacks(pending.timeout);
        }
        if (values == null) {
            sendSensorError(pending.session, "تعذّر قراءة الحساس — لم تصل قراءة");
            return;
        }
        sendSensorValue(pending.session, values, accuracy);
    }

    /** إلغاء قراءات حساس معلقة لجلسة تموت (المحرك
     * مات فلا نتيجة تُرسَل إليها) — يُستدعى من
     * restartProcess وonProcessEnded بجانب
     * stopSpeakForSession وstopAudioForSession. */
    private void stopSensorForSession(Session s) {
        ArrayList<PendingSensor> copy = new ArrayList<>(pendingSensors);
        for (PendingSensor p : copy) {
            if (p.session == s) {
                p.answered = true;
                pendingSensors.remove(p);
                if (sensorManager != null && p.listener != null) {
                    try {
                        sensorManager.unregisterListener(p.listener);
                    } catch (Exception ignored) {
                    }
                }
                if (p.timeout != null) {
                    new Handler(Looper.getMainLooper()).removeCallbacks(p.timeout);
                }
            }
        }
    }

    /** إلغاء كل قراءات الحساس المعلقة (موت التطبيق). */
    private void stopAllSensors() {
        ArrayList<PendingSensor> copy = new ArrayList<>(pendingSensors);
        for (PendingSensor p : copy) {
            stopSensorForSession(p.session);
        }
    }

    /** إرسال نتيجة حساس واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار والتنبيه والنطق والصوت. */
    private void sendSensorResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendSensorValue(Session s, float[] values, int accuracy) {
        try {
            JSONObject r = new JSONObject();
            JSONObject reading = new JSONObject();
            JSONArray arr = new JSONArray();
            for (float v : values) arr.put((double) v);
            reading.put("قيم", arr);
            reading.put("دقة", accuracy);
            r.put("قيمة", reading);
            sendSensorResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendSensorError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendSensorResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ===== استماع جهاز.اسمع + بطارية جهاز.بطارية (المهمة 26) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": "..."} لنص الكلام،
    // أو {"قيمة": {...}} لقاموس البطارية، أو
    // {"خطأ": "..."} — كما الحوار والتنبيه والنطق
    // والصوت والحساس تماما (البادئة نفسها).
    // بوابة الإذن (مكتوبة أولا): حوار إفصاح — الصوت
    // يذهب لخدمة تعرف صوتي خارجية — مرة في كل تشغيل؛
    // ثم إذن الميكروفون RECORD_AUDIO (يُطلب عند أول
    // اسمع بلا إذن فقط). واستماع واحد جارٍ في كل
    // التطبيق (طلب ثانٍ متزامن يرفض فورا). والبطارية
    // بلا إذن نظام (قراءة بث لاصق). والبصمة مؤجلة
    // صراحة في v1 (لا قفل تطبيق ولا قفل جلسة بعد).

    /** هل إذن الميكروفون ممنوح؟ */
    private boolean recordAudioGranted() {
        return checkSelfPermission("android.permission.RECORD_AUDIO")
                == PackageManager.PERMISSION_GRANTED;
    }

    /** استقبال طلب استماع من قناة التحكم. */
    private void handleListenRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendListenError(s, "طلب استماع غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendListenError(s, "التطبيق غير متاح");
            return;
        }
        if (listeningNow != null) {
            sendListenError(s, "استماع آخر جارٍ — انتظر انتهاءه");
            return;
        }
        listeningNow = new PendingListen(s);
        if (!speechDisclosureAccepted) {
            showListenDisclosure();
            return;
        }
        proceedToListen();
    }

    /** حوار الإفصاح عن التعرف الصوتي (مرة في كل
     * تشغيل): الصوت يذهب لخدمة خارجية — «موافق»
     * يقبل ويتابع؛ و«إلغاء» يرفع خطأ ويفرغ الخانة. */
    private void showListenDisclosure() {
        if (listeningNow == null) return;
        listenDisclosureDialog = new AlertDialog.Builder(this)
                .setTitle("التعرف الصوتي")
                .setMessage("سيُرسل صوتك إلى خدمة التعرف الصوتي من النظام — وهي خدمة خارجية قد تعالج الصوت على خوادمها — وذلك فقط ليعيد البرنامج النص الذي قلته. هل توافق؟")
                .setCancelable(false)
                .setPositiveButton("موافق", (d, w) -> {
                    listenDisclosureDialog = null;
                    speechDisclosureAccepted = true;
                    proceedToListen();
                })
                .setNegativeButton("إلغاء", (d, w) -> {
                    listenDisclosureDialog = null;
                    PendingListen p = listeningNow;
                    listeningNow = null;
                    if (p != null) {
                        sendListenError(p.session,
                                "ألغى المستخدم التعرف الصوتي");
                    }
                })
                .show();
    }

    /** متابعة الاستماع بعد الإفصاح: إذن الميكروفون
     * أولا (طابور واحد بلا حوار ثان)، ثم بدء الاستماع. */
    private void proceedToListen() {
        if (listeningNow == null) return;
        if (!recordAudioGranted()) {
            if (!listenPermissionRequestInFlight) {
                listenPermissionRequestInFlight = true;
                requestPermissions(
                        new String[]{"android.permission.RECORD_AUDIO"},
                        REQ_LISTEN_PERMISSION);
            }
            return;
        }
        startListening();
    }

    /** بدء الاستماع بعد كل الفحوص: توفر الخدمة
     * أولا، ثم نيّة التعرف (نموذج حر، اللغة
     * عربية مصرية، نتيجة قصوى 1). */
    private void startListening() {
        if (listeningNow == null) return;
        Session s = listeningNow.session;
        if (!SpeechRecognizer.isRecognitionAvailable(this)) {
            listeningNow = null;
            sendListenError(s, "خدمة التعرف الصوتي غير متاحة على هذا الجهاز");
            return;
        }
        try {
            if (speechRecognizer == null) {
                speechRecognizer = SpeechRecognizer.createSpeechRecognizer(this);
                speechRecognizer.setRecognitionListener(new RecognitionListener() {
                    @Override
                    public void onReadyForSpeech(Bundle params) {
                    }

                    @Override
                    public void onBeginningOfSpeech() {
                    }

                    @Override
                    public void onRmsChanged(float rmsdB) {
                    }

                    @Override
                    public void onBufferReceived(byte[] buffer) {
                    }

                    @Override
                    public void onEndOfSpeech() {
                    }

                    @Override
                    public void onError(int error) {
                        runOnUiThread(() -> onListenError(error));
                    }

                    @Override
                    public void onResults(Bundle results) {
                        ArrayList<String> list = results.getStringArrayList(
                                SpeechRecognizer.RESULTS_RECOGNITION);
                        runOnUiThread(() -> onListenResults(list));
                    }

                    @Override
                    public void onPartialResults(Bundle partialResults) {
                        // النتائج الجزئية تُتجاهل في v1.
                    }

                    @Override
                    public void onEvent(int eventType, Bundle params) {
                    }
                });
            }
            Intent intent = new Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH);
            intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                    RecognizerIntent.LANGUAGE_MODEL_FREE_FORM);
            intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE, "ar-EG");
            intent.putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1);
            speechRecognizer.startListening(intent);
        } catch (Exception ignored) {
            listeningNow = null;
            sendListenError(s, "تعذّر بدء الاستماع");
        }
    }

    /** نتيجة التعرف: أول نص يرجع للجلسة المنتظرة؛
     * وقائمة فارغة خطأ «لم يُسمع كلام واضح». */
    private void onListenResults(ArrayList<String> list) {
        if (listeningNow == null) return;
        Session s = listeningNow.session;
        listeningNow = null;
        destroyRecognizer();
        if (list == null || list.isEmpty() || list.get(0) == null
                || list.get(0).isEmpty()) {
            sendListenError(s, "تعذّر التعرّف على الكلام — لم يُسمع كلام واضح");
            return;
        }
        sendListenValue(s, list.get(0));
    }

    /** خطأ التعرف: ترجمة الرمز إلى عربي + مسح
     * الخانة + نتيجة خطأ للجلسة. */
    private void onListenError(int error) {
        if (listeningNow == null) return;
        Session s = listeningNow.session;
        listeningNow = null;
        destroyRecognizer();
        String message;
        switch (error) {
            case SpeechRecognizer.ERROR_NO_MATCH:
                message = "تعذّر التعرّف على الكلام — لم يُسمع كلام واضح";
                break;
            case SpeechRecognizer.ERROR_SPEECH_TIMEOUT:
                message = "انتهت مهلة الاستماع — لم يُسمع كلام";
                break;
            case SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS:
                message = "رفض إذن الميكروفون — فعّل الإذن من إعدادات التطبيق";
                break;
            case SpeechRecognizer.ERROR_RECOGNIZER_BUSY:
                message = "خدمة التعرف مشغولة";
                break;
            case SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED:
            case SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE:
                message = "اللغة العربية غير مدعومة في خدمة التعرف على هذا الجهاز";
                break;
            default:
                message = "تعذّر التعرّف على الكلام";
                break;
        }
        sendListenError(s, message);
    }

    /** تدمير كائن التعرف (بعد نتيجة أو إيقاف). */
    private void destroyRecognizer() {
        if (speechRecognizer != null) {
            try {
                speechRecognizer.destroy();
            } catch (Exception ignored) {
            }
            speechRecognizer = null;
        }
    }

    /** إيقاف استماع جلسة تموت ومسح خانته (المحرك
     * مات فلا نتيجة تُرسَل إليه) — يُستدعى من
     * restartProcess وonProcessEnded. */
    private void stopListenForSession(Session s) {
        if (listeningNow != null && listeningNow.session == s) {
            listeningNow = null;
            if (speechRecognizer != null) {
                try {
                    speechRecognizer.cancel();
                } catch (Exception ignored) {
                }
            }
            destroyRecognizer();
            if (listenDisclosureDialog != null) {
                try {
                    listenDisclosureDialog.dismiss();
                } catch (Exception ignored) {
                }
                listenDisclosureDialog = null;
            }
        }
    }

    /** إيقاف أي استماع معلق (موت التطبيق). */
    private void stopAllListens() {
        if (listeningNow != null) {
            stopListenForSession(listeningNow.session);
        }
    }

    /** إرسال نتيجة استماع واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار والتنبيه والنطق والصوت والحساس. */
    private void sendListenResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendListenValue(Session s, String text) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", text);
            sendListenResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendListenError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendListenResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    /** استقبال طلب قراءة بطارية من قناة التحكم:
     * بث لاصق جاهز — بلا إذن وبلا انتظار. */
    private void handleBatteryRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendBatteryError(s, "طلب بطارية غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendBatteryError(s, "التطبيق غير متاح");
            return;
        }
        Intent battery = null;
        try {
            battery = registerReceiver(null,
                    new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
        } catch (Exception ignored) {
        }
        if (battery == null) {
            sendBatteryError(s, "تعذّر قراءة البطارية");
            return;
        }
        int level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1);
        int scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1);
        int percent = (level >= 0 && scale > 0) ? (level * 100 / scale) : 0;
        int status = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1);
        String state;
        switch (status) {
            case BatteryManager.BATTERY_STATUS_CHARGING:
                state = "يشحن";
                break;
            case BatteryManager.BATTERY_STATUS_DISCHARGING:
                state = "يفرغ";
                break;
            case BatteryManager.BATTERY_STATUS_FULL:
                state = "ممتلئ";
                break;
            case BatteryManager.BATTERY_STATUS_NOT_CHARGING:
                state = "لا يشحن";
                break;
            default:
                state = "غير معروف";
                break;
        }
        int health = battery.getIntExtra(BatteryManager.EXTRA_HEALTH, -1);
        String healthText;
        switch (health) {
            case BatteryManager.BATTERY_HEALTH_GOOD:
                healthText = "جيدة";
                break;
            case BatteryManager.BATTERY_HEALTH_OVERHEAT:
                healthText = "ساخنة";
                break;
            case BatteryManager.BATTERY_HEALTH_DEAD:
                healthText = "تالفة";
                break;
            case BatteryManager.BATTERY_HEALTH_OVER_VOLTAGE:
                healthText = "جهد زائد";
                break;
            case BatteryManager.BATTERY_HEALTH_UNSPECIFIED_FAILURE:
                healthText = "فشل غير محدد";
                break;
            case BatteryManager.BATTERY_HEALTH_COLD:
                healthText = "باردة";
                break;
            default:
                healthText = "غير معروفة";
                break;
        }
        int tempTenths = battery.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, -1);
        double temp = tempTenths >= 0 ? tempTenths / 10.0 : 0.0;
        int voltage = battery.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1);
        if (voltage < 0) voltage = 0;
        int plugged = battery.getIntExtra(BatteryManager.EXTRA_PLUGGED, -1);
        String source;
        switch (plugged) {
            case BatteryManager.BATTERY_PLUGGED_AC:
                source = "تيار متردد";
                break;
            case BatteryManager.BATTERY_PLUGGED_USB:
                source = "يواسبي";
                break;
            case BatteryManager.BATTERY_PLUGGED_WIRELESS:
                source = "لاسلكي";
                break;
            default:
                source = "لا شيء";
                break;
        }
        try {
            JSONObject r = new JSONObject();
            JSONObject dict = new JSONObject();
            dict.put("مستوى", percent);
            dict.put("حالة", state);
            dict.put("صحة", healthText);
            dict.put("حرارة", temp);
            dict.put("جهد", voltage);
            dict.put("مصدر", source);
            r.put("قيمة", dict);
            sendBatteryResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    /** إرسال نتيجة بطارية واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها. */
    private void sendBatteryResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendBatteryError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendBatteryResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ===== واجهة الجهاز من البرنامج (الدفعة 4أ —
    // البنود 22–34) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": ...} أو
    // {"خطأ": "..."} — كما الحوار تماما (البادئة
    // نفسها). وكل طلب يجيب بنتيجة واحدة بالضبط، فلا
    // يعلق المحرك أبدا — حتى طلبات الأذونات والنوايا
    // المعلّقة (موقع/مسح/التقاط/منتقي) لها مهلة أو
    // جواب إلغاء. والمشاركة والفتح والتوست بلا نتيجة
    // نظام (fire-and-forget): النتيجة تُرسَل فور
    // إطلاق النية — فالمشاركة لا ردّ لها أصلا.
    // والملفات لا تغادر مجلد البرنامج أبدا
    // (المحرك يحلّ المسار داخل سياسته، والتطبيق
    // يعيد الفحص، والمزوّد يرفض ما خرج عنه)؛
    // وniva.properties مرفوض في الطبقات الثلاث.

    /** إرسال نتيجة جهاز واحدة إلى المحرك (قيمة أو
     * خطأ) عبر الطابور المحدود — البادئة نفسها
     * كالحوار والتنبيه والصوت. */
    private void sendDeviceResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendDeviceValue(Session s, Object value) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", value);
            sendDeviceResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendDeviceError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendDeviceResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    /** نوع MIME لملف من امتداده؛ مجهول → نوع عام. */
    private String mimeTypeForFile(File f) {
        String name = f.getName();
        int dot = name.lastIndexOf('.');
        if (dot >= 0 && dot + 1 < name.length()) {
            String mime = MimeTypeMap.getSingleton()
                    .getMimeTypeFromExtension(
                            name.substring(dot + 1).toLowerCase());
            if (mime != null) return mime;
        }
        return "application/octet-stream";
    }

    /** ملف داخل مجلد البرنامج؟ (إعادة فحص — المحرك
     * حلّ المسار داخل سياسته أولا؛ والرابط قانونيا
     * داخل المجلد؛ وniva.properties مرفوض.) */
    private File deviceFileInsideProgramDir(String path) {
        File base = getFilesDir();
        File file = new File(path);
        try {
            File canonBase = base.getCanonicalFile();
            File canon = file.getCanonicalFile();
            String basePath = canonBase.getAbsolutePath() + File.separator;
            if (!canon.getAbsolutePath().startsWith(basePath)) return null;
            if (NivaFileProvider.PROPERTIES_NAME.equals(canon.getName())) return null;
            return canon;
        } catch (IOException e) {
            return null;
        }
    }

    /** استقبال طلب مشاركة ملف (البند 22): رابط
     * محتوى للقراءة فقط + نية مشاركة + منتقي
     * النظام؛ والنتيجة فور الإطلاق. */
    private void handleShareFileRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مشاركة غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        if (path.isEmpty()) {
            sendDeviceError(s, "طلب مشاركة غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        File file = deviceFileInsideProgramDir(path);
        if (file == null || !file.isFile()) {
            sendDeviceError(s, "الملف غير موجود أو خارج مجلد البرنامج");
            return;
        }
        Uri uri = NivaFileProvider.getUriForFile(this, file);
        Intent intent = new Intent(Intent.ACTION_SEND);
        intent.setType(mimeTypeForFile(file));
        intent.putExtra(Intent.EXTRA_STREAM, uri);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivity(Intent.createChooser(intent, "مشاركة ملف"));
        } catch (ActivityNotFoundException e) {
            sendDeviceError(s, "لا يوجد تطبيق للمشاركة");
            return;
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب مشاركة نص (البند 22): نية مشاركة
     * + منتقي النظام؛ والنتيجة فور الإطلاق. */
    private void handleShareTextRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مشاركة غير صالح");
            return;
        }
        String text = req.optString("نص", "");
        if (text.isEmpty()) {
            sendDeviceError(s, "طلب مشاركة غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        Intent intent = new Intent(Intent.ACTION_SEND);
        intent.setType("text/plain");
        intent.putExtra(Intent.EXTRA_TEXT, text);
        try {
            startActivity(Intent.createChooser(intent, "مشاركة نص"));
        } catch (ActivityNotFoundException e) {
            sendDeviceError(s, "لا يوجد تطبيق للمشاركة");
            return;
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب فتح ملف بتطبيق خارجي (البند 23):
     * رابط محتوى للقراءة فقط + نية عرض؛ والنتيجة
     * فور الإطلاق. */
    private void handleOpenFileRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب فتح غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        if (path.isEmpty()) {
            sendDeviceError(s, "طلب فتح غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        File file = deviceFileInsideProgramDir(path);
        if (file == null || !file.isFile()) {
            sendDeviceError(s, "الملف غير موجود أو خارج مجلد البرنامج");
            return;
        }
        Uri uri = NivaFileProvider.getUriForFile(this, file);
        Intent intent = new Intent(Intent.ACTION_VIEW);
        intent.setDataAndType(uri, mimeTypeForFile(file));
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivity(intent);
        } catch (ActivityNotFoundException e) {
            sendDeviceError(s, "لا يوجد تطبيق لفتح هذا الملف");
            return;
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب فتح رابط في المتصفح (البند 23):
     * نية عرض مباشرة بلا منتقي — المحرك فحص أنه
     * رابط http(s) أولا؛ والنتيجة فور الإطلاق. */
    private void handleOpenUrlRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب فتح غير صالح");
            return;
        }
        String url = req.optString("رابط", "");
        if (url.isEmpty()) {
            sendDeviceError(s, "طلب فتح غير صالح");
            return;
        }
        // قائمة سماح المخططات (H3): http وhttps فقط —
        // دفاع في طبقة التطبيق لا يعتمد على فحص المحرك
        // وحده؛ فـintent: وtel: وsmsto: ونحوها مرفوضة.
        String scheme = Uri.parse(url).getScheme();
        if (scheme == null
                || (!scheme.equalsIgnoreCase("http")
                        && !scheme.equalsIgnoreCase("https"))) {
            sendDeviceError(s, "المخطط غير مسموح — http وhttps فقط");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        try {
            startActivity(intent);
        } catch (ActivityNotFoundException e) {
            sendDeviceError(s, "لا يوجد متصفح لفتح الرابط");
            return;
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب كتابة نص في الحافظة (البند 24):
     * حوار تأكيد قبل كل كتابة (H5) — الكتابة تستبدل
     * ما نسخه المستخدم؛ وسقف طول؛ ووسم حساسية. */
    private void handleClipboardSetRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب حافظة غير صالح");
            return;
        }
        String text = req.optString("نص", "");
        if (text.isEmpty()) {
            sendDeviceError(s, "طلب حافظة غير صالح");
            return;
        }
        if (text.length() > CLIPBOARD_CAP) {
            sendDeviceError(s, "النص طويل جدا");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        final String clipText = text;
        String preview = clipText.length() > 100
                ? clipText.substring(0, 100) + "…"
                : clipText;
        new AlertDialog.Builder(this)
                .setTitle("نسخ إلى الحافظة؟")
                .setMessage("برنامج في هذه الجلسة يطلب نسخ نص إلى الحافظة — وسيستبدل ما نسخته حاليا.\n\n«" + preview + "»")
                .setCancelable(false)
                .setPositiveButton("موافق", (d, w) -> {
                    ClipboardManager cm = (ClipboardManager)
                            getSystemService(Context.CLIPBOARD_SERVICE);
                    if (cm == null) {
                        sendDeviceError(s, "الحافظة غير متاحة");
                        return;
                    }
                    ClipData clip = ClipData.newPlainText("نيفا", clipText);
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        PersistableBundle extras = new PersistableBundle();
                        extras.putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true);
                        clip.getDescription().setExtras(extras);
                    }
                    cm.setPrimaryClip(clip);
                    sendDeviceValue(s, true);
                })
                .setNegativeButton("إلغاء", (d, w) ->
                        sendDeviceError(s, "ألغى المستخدم النسخ إلى الحافظة"))
                .show();
    }

    /** استقبال طلب قراءة نص الحافظة (البند 25): حوار
     * إفصاح قبل أول قراءة في الجلسة (H4) — الحافظة
     * قد تحوي كلمة مرور أو رمزا؛ وفارغة → نص فارغ
     * صادق. ملاحظة: أندرويد 12+ يعرض إشعار نظام
     * عند قراءة الحافظة — سلوك النظام لا عيب في
     * نيفا. */
    private void handleClipboardGetRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب حافظة غير صالح");
            return;
        }
        if (!s.clipboardReadDisclosed) {
            if (isFinishing() || isDestroyed()) {
                sendDeviceError(s, "التطبيق غير متاح");
                return;
            }
            new AlertDialog.Builder(this)
                    .setTitle("قراءة الحافظة؟")
                    .setMessage("برنامج في هذه الجلسة يطلب قراءة الحافظة — وقد تحوي كلمة مرور أو رمزا نسخته من تطبيق آخر. السماح بالقراءة في هذه الجلسة؟")
                    .setCancelable(false)
                    .setPositiveButton("موافق", (d, w) -> {
                        s.clipboardReadDisclosed = true;
                        doClipboardGet(s);
                    })
                    .setNegativeButton("إلغاء", (d, w) ->
                            sendDeviceError(s, "ألغى المستخدم قراءة الحافظة"))
                    .show();
            return;
        }
        doClipboardGet(s);
    }

    /** قراءة الحافظة الفعلية بعد الإفصاح (H4). */
    private void doClipboardGet(Session s) {
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null || !cm.hasPrimaryClip()) {
            sendDeviceValue(s, "");
            return;
        }
        ClipData clip = cm.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            sendDeviceValue(s, "");
            return;
        }
        CharSequence text = clip.getItemAt(0).coerceToText(this);
        sendDeviceValue(s, text == null ? "" : text.toString());
    }

    /** استقبال طلب منتقي ملف عام (البند 28): نية
     * مستندات النظام؛ والنسخ إلى مجلد البرنامج
     * عند الجواب (سقف 5 ميغابايت كما SAF)، باسم
     * فريد بلا كتابة فوق ملف موجود. */
    private void handlePickFileRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب اختيار غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (pickFileSession != null) {
            sendDeviceError(s, "اختيار ملف آخر جارٍ");
            return;
        }
        pickFileSession = s;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        try {
            startActivityForResult(intent, REQ_PICK_FILE);
        } catch (ActivityNotFoundException e) {
            pickFileSession = null;
            sendDeviceError(s, "لا يوجد منتقي ملفات");
        }
    }

    /** اسم فريد داخل مجلد — بلا كتابة فوق ملف موجود
     * (يضيف « (2)» وهكذا قبل الامتداد). */
    private String uniqueNameInDir(File dir, String name) {
        name = name.replace('/', '_').replace('\\', '_');
        if (name.isEmpty()) name = "ملف";
        if (!new File(dir, name).exists()) return name;
        int dot = name.lastIndexOf('.');
        String base = dot > 0 ? name.substring(0, dot) : name;
        String ext = dot > 0 ? name.substring(dot) : "";
        int n = 2;
        while (true) {
            String cand = base + " (" + n + ")" + ext;
            if (!new File(dir, cand).exists()) return cand;
            n++;
        }
    }

    /** جواب منتقي الملف العام: نسخ إلى مجلد
     * البرنامج بسقف 5 ميغابايت، ثم الاسم النسبي. */
    private void onPickFileResult(int resultCode, Intent data) {
        Session s = pickFileSession;
        pickFileSession = null;
        if (s == null) return;
        if (s.process == null || s.ended) return;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            sendDeviceError(s, "ألغى المستخدم اختيار الملف");
            return;
        }
        Uri uri = data.getData();
        String name = safDocumentDisplayName(uri);
        if (name == null || name.isEmpty()) {
            String last = uri.getLastPathSegment();
            name = last != null ? last : "ملف";
        }
        name = uniqueNameInDir(getFilesDir(), name);
        File dest = new File(getFilesDir(), name);
        try (InputStream in = getContentResolver().openInputStream(uri);
             FileOutputStream out = new FileOutputStream(dest)) {
            if (in == null) {
                sendDeviceError(s, "تعذّر فتح الملف المختار");
                return;
            }
            byte[] buf = new byte[8192];
            int n;
            long total = 0;
            while ((n = in.read(buf)) != -1) {
                total += n;
                if (total > PICK_FILE_CAP) {
                    dest.delete();
                    sendDeviceError(s, "الملف أكبر من 5 ميغابايت");
                    return;
                }
                out.write(buf, 0, n);
            }
        } catch (IOException e) {
            dest.delete();
            sendDeviceError(s, "تعذّر نسخ الملف المختار");
            return;
        }
        sendDeviceValue(s, name);
    }

    // ===== الدفعة 5 (البنود 98 و99 خطوة 1): تنزيل
    // نظامي وقراءة مستند وعرض مجلد — منتقيات لمرة
    // واحدة بلا إذن دائم، وطلب واحد من كل نوع في كل
    // التطبيق (كالمنتقي العام — البند 28). =====

    /** استقبال طلب تنزيل نظامي (البند 98): الحمولة
     * JSON {رابط، عنوان، وصف، اسم_ملف}؛ والرابط
     * https فقط، واسم الملف بلا مسار (المحرك نظّفه
     * أصلا والتطبيق يعيد الفحص)؛ ومدير التنزيلات
     * يكتب بنفسه في التنزيلات العامة — بلا إذن
     * تخزين — ويرجع معرّف التنزيل عددا. */
    private void handleDownloadRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب تنزيل غير صالح");
            return;
        }
        String url = req.optString("رابط", "");
        String title = req.optString("عنوان", "");
        String desc = req.optString("وصف", "");
        String filename = req.optString("اسم_ملف", "");
        if (url.isEmpty() || title.isEmpty() || desc.isEmpty()
                || filename.isEmpty() || !url.startsWith("https://")
                || filename.contains("/") || filename.contains("\\")
                || filename.equals(".") || filename.equals("..")) {
            sendDeviceError(s, "طلب تنزيل غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        DownloadManager dm = (DownloadManager)
                getSystemService(Context.DOWNLOAD_SERVICE);
        if (dm == null) {
            sendDeviceError(s, "مدير التنزيلات غير متاح");
            return;
        }
        try {
            DownloadManager.Request request =
                    new DownloadManager.Request(Uri.parse(url));
            request.setTitle(title);
            request.setDescription(desc);
            request.setDestinationInExternalPublicDir(
                    Environment.DIRECTORY_DOWNLOADS, filename);
            request.setNotificationVisibility(
                    DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED);
            long id = dm.enqueue(request);
            sendDeviceValue(s, id);
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر بدء التنزيل");
        }
    }

    /** استقبال طلب قراءة مستند (البند 99 خطوة 1):
     * منتقي مستندات النظام لمرة واحدة — بلا نسخ إلى
     * مجلد البرنامج وبلا إذن دائم. */
    private void handleReadDocumentRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب قراءة غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (readDocumentSession != null) {
            sendDeviceError(s, "قراءة مستند أخرى جارية");
            return;
        }
        readDocumentSession = s;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        try {
            startActivityForResult(intent, REQ_READ_DOCUMENT);
        } catch (ActivityNotFoundException e) {
            readDocumentSession = null;
            sendDeviceError(s, "لا يوجد منتقي مستندات");
        }
    }

    /** جواب منتقي المستند: نصه UTF-8 بسقف 5
     * ميغابايت؛ والإلغاء «ألغى المستخدم الاختيار». */
    private void onReadDocumentResult(int resultCode, Intent data) {
        Session s = readDocumentSession;
        readDocumentSession = null;
        if (s == null) return;
        if (s.process == null || s.ended) return;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            sendDeviceError(s, "ألغى المستخدم الاختيار");
            return;
        }
        try (InputStream in = getContentResolver().openInputStream(data.getData());
             ByteArrayOutputStream buf = new ByteArrayOutputStream()) {
            if (in == null) {
                sendDeviceError(s, "تعذّر فتح المستند");
                return;
            }
            byte[] chunk = new byte[8192];
            int n;
            long total = 0;
            while ((n = in.read(chunk)) != -1) {
                total += n;
                if (total > READ_DOCUMENT_CAP) {
                    sendDeviceError(s, "المستند أكبر من 5 ميغابايت");
                    return;
                }
                buf.write(chunk, 0, n);
            }
            sendDeviceValue(s, buf.toString("UTF-8"));
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر قراءة المستند");
        }
    }

    /** استقبال طلب عرض مجلد (البند 99 خطوة 1):
     * منتقي مجلدات النظام لمرة واحدة — بلا إذن دائم
     * وبلا نسخ. */
    private void handleListFolderRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب عرض غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (listFolderSession != null) {
            sendDeviceError(s, "عرض مجلد آخر جارٍ");
            return;
        }
        listFolderSession = s;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        try {
            startActivityForResult(intent, REQ_LIST_FOLDER);
        } catch (ActivityNotFoundException e) {
            listFolderSession = null;
            sendDeviceError(s, "لا يوجد منتقي مجلدات");
        }
    }

    /** جواب منتقي المجلد: قائمة أسماء الأبناء (`/`
     * في نهاية المجلدات — كعلامات `ات`)؛ والإلغاء
     * «ألغى المستخدم الاختيار». */
    private void onListFolderResult(int resultCode, Intent data) {
        Session s = listFolderSession;
        listFolderSession = null;
        if (s == null) return;
        if (s.process == null || s.ended) return;
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            sendDeviceError(s, "ألغى المستخدم الاختيار");
            return;
        }
        Uri treeUri = data.getData();
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
                treeUri, DocumentsContract.getTreeDocumentId(treeUri));
        JSONArray names = new JSONArray();
        try (Cursor c = getContentResolver().query(childrenUri,
                new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                        DocumentsContract.Document.COLUMN_MIME_TYPE},
                null, null, null)) {
            if (c == null) {
                sendDeviceError(s, "تعذّر فتح المجلد");
                return;
            }
            while (c.moveToNext()) {
                String name = c.getString(0);
                if (name == null) continue;
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(1))) {
                    name = name + "/";
                }
                names.put(name);
            }
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر فتح المجلد");
            return;
        }
        sendDeviceValue(s, names);
    }

    /** إلغاء طلبات الجهاز المعلّقة لجلسة ماتت: المستمعون
     * والمستقبلون والمهلات تُلغى، والخانات تُمسح —
     * والمحرك ميت فلا نتيجة تُرسَل. */
    private void cancelDeviceRequestsForSession(Session s) {
        if (pendingLocation != null && pendingLocation.session == s) {
            PendingLocation p = pendingLocation;
            pendingLocation = null;
            p.answered = true;
            if (p.listener != null) {
                LocationManager lm = (LocationManager)
                        getSystemService(Context.LOCATION_SERVICE);
                if (lm != null) {
                    try {
                        lm.removeUpdates(p.listener);
                    } catch (Exception ignored) {
                    }
                }
            }
            if (p.timeout != null && p.handler != null) {
                p.handler.removeCallbacks(p.timeout);
            }
        }
        if (pendingWifiScan != null && pendingWifiScan.session == s) {
            PendingWifiScan p = pendingWifiScan;
            pendingWifiScan = null;
            p.answered = true;
            if (p.receiver != null) {
                try {
                    unregisterReceiver(p.receiver);
                } catch (Exception ignored) {
                }
            }
            if (p.timeout != null && p.handler != null) {
                p.handler.removeCallbacks(p.timeout);
            }
        }
        if (pickFileSession == s) pickFileSession = null;
        // (الدفعة 5): منتقي مستند أو مجلد أو شاشة
        // إعدادات التخزين من هذه الجلسة تُلغى خانته —
        // فلا جواب يصل إلى جلسة ميتة.
        if (readDocumentSession == s) readDocumentSession = null;
        if (listFolderSession == s) listFolderSession = null;
        if (pendingStorageSession == s) pendingStorageSession = null;
        if (pendingCaptureSession == s) {
            pendingCaptureSession = null;
            pendingCaptureFile = null;
        }
        // (الدفعة 4ب): مصباح مشتغل من هذه الجلسة
        // يُطفأ — فلا يبقى مصباح مضاء بلا برنامج؛
        // وتسجيل جارٍ يُوقف صامتا.
        if (torchOn && torchSession == s) {
            try {
                CameraManager cm = (CameraManager)
                        getSystemService(Context.CAMERA_SERVICE);
                if (cm != null && torchCameraId != null) {
                    cm.setTorchMode(torchCameraId, false);
                }
            } catch (Exception ignored) {
            }
            torchOn = false;
            torchSession = null;
        }
        if (pendingTorch != null && pendingTorch.session == s) {
            pendingTorch = null;
        }
        stopRecordingForSession(s);
    }

    /** هل إذن الموقع الدقيق ممنوح؟ */
    private boolean locationPermissionGranted() {
        return checkSelfPermission("android.permission.ACCESS_FINE_LOCATION")
                == PackageManager.PERMISSION_GRANTED;
    }

    /** استقبال طلب موقع (البندان 29 و30): آخر معروف
     * فوري، أو قراءة مباشرة أول إصلاح يفوز بمهلة
     * 10 ثوان. الإذن يُطلب عند أول استعمال فقط —
     * والرفض خطأ عربي واضح. */
    private void handleLocationRequest(Session s, String payload, boolean once) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب موقع غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (pendingLocation != null) {
            sendDeviceError(s, "طلب موقع آخر جارٍ");
            return;
        }
        pendingLocation = new PendingLocation(s, once);
        if (locationPermissionGranted()) {
            runLocationRequest(pendingLocation);
            return;
        }
        if (!locationPermissionRequestInFlight) {
            locationPermissionRequestInFlight = true;
            requestPermissions(
                    new String[]{"android.permission.ACCESS_FINE_LOCATION"},
                    REQ_LOCATION_PERMISSION);
        }
        // وإن كان طلب إذن جاريا، يبقى المعلّق حتى
        // جواب الإذن — فرع onRequestPermissionsResult
        // ينفّذه أو يخطئه.
    }

    /** تنفيذ طلب الموقع المعلّق بعد منح الإذن. */
    private void runLocationRequest(PendingLocation p) {
        LocationManager lm = (LocationManager)
                getSystemService(Context.LOCATION_SERVICE);
        if (lm == null) {
            answerLocationError(p, "خدمة الموقع غير متاحة");
            return;
        }
        if (!p.once) {
            // آخر معروف: GPS أولا ثم الشبكة — فوري،
            // بلا انتظار GPS وبلا بث.
            Location best = null;
            try {
                best = lm.getLastKnownLocation(LocationManager.GPS_PROVIDER);
            } catch (SecurityException ignored) {
            }
            if (best == null) {
                try {
                    best = lm.getLastKnownLocation(
                            LocationManager.NETWORK_PROVIDER);
                } catch (SecurityException ignored) {
                }
            }
            if (best == null) {
                answerLocationError(p,
                        "لا يوجد موقع محفوظ — فعّل الموقع ثم أعد المحاولة");
                return;
            }
            answerLocationValue(p, best);
            return;
        }
        // مباشر: أول إصلاح من أي مزوّد يفوز؛ ومهلة
        // 10 ثوان تجيب بخطأ إن لم يصل إصلاح.
        Handler handler = new Handler(Looper.getMainLooper());
        p.handler = handler;
        LocationListener listener = new LocationListener() {
            @Override
            public void onLocationChanged(Location location) {
                answerLocationValue(p, location);
            }

            @Override
            public void onStatusChanged(String provider, int status,
                                        Bundle extras) {
            }

            @Override
            public void onProviderEnabled(String provider) {
            }

            @Override
            public void onProviderDisabled(String provider) {
            }
        };
        p.listener = listener;
        p.timeout = () -> answerLocationError(p, "انتهت مهلة قراءة الموقع");
        handler.postDelayed(p.timeout, DEVICE_LOCATION_TIMEOUT_MS);
        try {
            lm.requestLocationUpdates(LocationManager.GPS_PROVIDER,
                    0, 0, listener, Looper.getMainLooper());
        } catch (Exception ignored) {
        }
        try {
            lm.requestLocationUpdates(LocationManager.NETWORK_PROVIDER,
                    0, 0, listener, Looper.getMainLooper());
        } catch (Exception ignored) {
        }
    }

    /** جواب موقع واحد — قيمة؛ ويلغي المستمع
     * والمهلة ويمسح الخانة. */
    private void answerLocationValue(PendingLocation p, Location loc) {
        if (p.answered) return;
        p.answered = true;
        if (p.listener != null) {
            LocationManager lm = (LocationManager)
                    getSystemService(Context.LOCATION_SERVICE);
            if (lm != null) {
                try {
                    lm.removeUpdates(p.listener);
                } catch (Exception ignored) {
                }
            }
        }
        if (p.timeout != null && p.handler != null) {
            p.handler.removeCallbacks(p.timeout);
        }
        if (pendingLocation == p) pendingLocation = null;
        try {
            JSONObject v = new JSONObject();
            v.put("خط_العرض", loc.getLatitude());
            v.put("خط_الطول", loc.getLongitude());
            v.put("دقة", (double) loc.getAccuracy());
            v.put("ارتفاع", loc.hasAltitude() ? loc.getAltitude() : 0.0);
            v.put("اتجاه", loc.hasBearing() ? (double) loc.getBearing() : 0.0);
            v.put("سرعة", loc.hasSpeed() ? (double) loc.getSpeed() : 0.0);
            v.put("وقت", loc.getTime());
            sendDeviceValue(p.session, v);
        } catch (JSONException ignored) {
        }
    }

    /** جواب موقع واحد — خطأ؛ ويلغي المستمع
     * والمهلة ويمسح الخانة. */
    private void answerLocationError(PendingLocation p, String message) {
        if (p.answered) return;
        p.answered = true;
        if (p.listener != null) {
            LocationManager lm = (LocationManager)
                    getSystemService(Context.LOCATION_SERVICE);
            if (lm != null) {
                try {
                    lm.removeUpdates(p.listener);
                } catch (Exception ignored) {
                }
            }
        }
        if (p.timeout != null && p.handler != null) {
            p.handler.removeCallbacks(p.timeout);
        }
        if (pendingLocation == p) pendingLocation = null;
        sendDeviceError(p.session, message);
    }

    /** استقبال طلب سرد الكاميرات (البند 31): معرّف
     * واتجاه لكل كاميرا — بلا إذن (سرد القدرات
     * فقط). */
    private void handleCamerasRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب كاميرات غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        CameraManager cm = (CameraManager)
                getSystemService(Context.CAMERA_SERVICE);
        if (cm == null) {
            sendDeviceError(s, "الكاميرا غير متاحة");
            return;
        }
        JSONArray list = new JSONArray();
        try {
            for (String id : cm.getCameraIdList()) {
                CameraCharacteristics c = cm.getCameraCharacteristics(id);
                Integer facing = c.get(CameraCharacteristics.LENS_FACING);
                String dir;
                if (facing != null
                        && facing == CameraCharacteristics.LENS_FACING_FRONT) {
                    dir = "أمامية";
                } else if (facing != null
                        && facing == CameraCharacteristics.LENS_FACING_BACK) {
                    dir = "خلفية";
                } else if (facing != null
                        && facing == CameraCharacteristics.LENS_FACING_EXTERNAL) {
                    dir = "خارجية";
                } else {
                    dir = "غير معروف";
                }
                JSONObject d = new JSONObject();
                d.put("معرف", id);
                d.put("اتجاه", dir);
                list.put(d);
            }
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر سرد الكاميرات");
            return;
        }
        sendDeviceValue(s, list);
    }

    /** هل إذن الكاميرا ممنوح؟ */
    private boolean cameraPermissionGranted() {
        return checkSelfPermission("android.permission.CAMERA")
                == PackageManager.PERMISSION_GRANTED;
    }

    /** استقبال طلب التقاط صورة (البند 32): ملف داخل
     * مجلد البرنامج عبر نية كاميرا النظام (أبسط
     * وأوثق على كل الأجهزة) — بلا معاينة حية داخل
     * الترمنال؛ والصورة ملف يعرضه المستخدم بتطبيق
     * خارجي (البند 23). الإذن يُطلب عند أول
     * استعمال فقط. */
    private void handleCapturePhotoRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب التقاط غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        if (path.isEmpty()) {
            sendDeviceError(s, "طلب التقاط غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (pendingCaptureSession != null) {
            sendDeviceError(s, "التقاط آخر جارٍ");
            return;
        }
        File file = deviceFileInsideProgramDir(path);
        if (file == null) {
            sendDeviceError(s, "مسار الصورة خارج مجلد البرنامج");
            return;
        }
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            sendDeviceError(s, "تعذّر إنشاء مجلد الصورة");
            return;
        }
        pendingCaptureSession = s;
        pendingCaptureFile = file;
        if (cameraPermissionGranted()) {
            launchCapturePhoto();
            return;
        }
        if (!capturePermissionRequestInFlight) {
            capturePermissionRequestInFlight = true;
            requestPermissions(
                    new String[]{"android.permission.CAMERA"},
                    REQ_CAPTURE_PERMISSION);
        }
    }

    /** إطلاق نية كاميرا النظام بالملف الهدف عبر
     * رابط المزوّد (كتابة لمرة واحدة). */
    private void launchCapturePhoto() {
        Session s = pendingCaptureSession;
        File file = pendingCaptureFile;
        if (s == null || file == null) return;
        Uri uri = NivaFileProvider.getUriForFile(this, file);
        Intent intent = new Intent(MediaStore.ACTION_IMAGE_CAPTURE);
        intent.putExtra(MediaStore.EXTRA_OUTPUT, uri);
        intent.addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivityForResult(intent, REQ_CAPTURE_PHOTO);
        } catch (ActivityNotFoundException e) {
            pendingCaptureSession = null;
            pendingCaptureFile = null;
            sendDeviceError(s, "لا يوجد تطبيق كاميرا");
        } catch (Exception e) {
            pendingCaptureSession = null;
            pendingCaptureFile = null;
            sendDeviceError(s, "تعذّر فتح الكاميرا");
        }
    }

    /** جواب نية الكاميرا: نجاح + ملف غير فارغ →
     * صحيح؛ وإلغاء أو ملف فارغ → خطأ عربي. */
    private void onCapturePhotoResult(int resultCode) {
        Session s = pendingCaptureSession;
        File f = pendingCaptureFile;
        pendingCaptureSession = null;
        pendingCaptureFile = null;
        if (s == null || f == null) return;
        if (s.process == null || s.ended) return;
        if (resultCode != RESULT_OK) {
            sendDeviceError(s, "ألغى المستخدم التقاط الصورة");
            return;
        }
        if (f.isFile() && f.length() > 0) {
            sendDeviceValue(s, true);
        } else {
            sendDeviceError(s, "تعذّر حفظ الصورة");
        }
    }

    /** استقبال طلب معلومات واي فاي الحالي (البند 33):
     * بلا إذن موقع — فالشبكة وعنوان الجهاز يظهران
     * «غير معروف» بلا إذن موقع ممنوح (سلوك النظام
     * منذ أندرويد 8: قراءة SSID تحتاج إذن موقع)؛
     * وباقي الحقول (تردد وقوة وسرعة وعنوان) بلا
     * إذن. وغير المتصل → خطأ عربي واضح. */
    private void handleWifiInfoRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب واي فاي غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        WifiManager wm = (WifiManager) getApplicationContext()
                .getSystemService(Context.WIFI_SERVICE);
        if (wm == null) {
            sendDeviceError(s, "واي فاي غير متاح");
            return;
        }
        WifiInfo info = wm.getConnectionInfo();
        if (info == null || info.getNetworkId() == -1) {
            sendDeviceError(s, "غير متصل بأي شبكة واي فاي");
            return;
        }
        String ssid = info.getSSID();
        if (ssid == null || ssid.equals("<unknown ssid>")) {
            ssid = "غير معروف";
        } else if (ssid.length() >= 2 && ssid.startsWith("\"")
                && ssid.endsWith("\"")) {
            ssid = ssid.substring(1, ssid.length() - 1);
        }
        String bssid = info.getBSSID();
        if (bssid == null) bssid = "غير معروف";
        int ip = info.getIpAddress();
        String ipStr = (ip & 0xff) + "." + ((ip >> 8) & 0xff) + "."
                + ((ip >> 16) & 0xff) + "." + ((ip >> 24) & 0xff);
        try {
            JSONObject v = new JSONObject();
            v.put("شبكة", ssid);
            v.put("عنوان_الجهاز", bssid);
            v.put("تردد", info.getFrequency());
            v.put("عنوان", ipStr);
            v.put("قوة_الإشارة", info.getRssi());
            v.put("سرعة", info.getLinkSpeed());
            v.put("متصل", true);
            sendDeviceValue(s, v);
        } catch (JSONException ignored) {
        }
    }

    /** استقبال طلب مسح واي فاي (البند 34): الإذن
     * هنا إذن مسح لا تتبع — يُطلب عند أول استعمال
     * فقط؛ والنتائج عبر مستقبل بث بمهلة 10 ثوان
     * (فشل بدء المسح لا يفشل فورا — المهلة تجيب
     * بالنتائج الموجودة، ولو فارغة). */
    private void handleWifiScanRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مسح غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (pendingWifiScan != null) {
            sendDeviceError(s, "مسح آخر جارٍ");
            return;
        }
        pendingWifiScan = new PendingWifiScan(s);
        if (locationPermissionGranted()) {
            runWifiScan(pendingWifiScan);
            return;
        }
        if (!wifiScanPermissionRequestInFlight) {
            wifiScanPermissionRequestInFlight = true;
            requestPermissions(
                    new String[]{"android.permission.ACCESS_FINE_LOCATION"},
                    REQ_WIFI_SCAN_PERMISSION);
        }
    }

    /** تنفيذ المسح المعلّق بعد منح الإذن. */
    private void runWifiScan(PendingWifiScan p) {
        WifiManager wm = (WifiManager) getApplicationContext()
                .getSystemService(Context.WIFI_SERVICE);
        if (wm == null) {
            answerWifiScanError(p, "واي فاي غير متاح");
            return;
        }
        Handler handler = new Handler(Looper.getMainLooper());
        p.handler = handler;
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                answerWifiScanResults(p, wm);
            }
        };
        p.receiver = receiver;
        registerReceiver(receiver,
                new IntentFilter(WifiManager.SCAN_RESULTS_AVAILABLE_ACTION));
        p.timeout = () -> answerWifiScanResults(p, wm);
        handler.postDelayed(p.timeout, DEVICE_WIFI_SCAN_TIMEOUT_MS);
        try {
            wm.startScan();
        } catch (Exception ignored) {
            // فشل البدء (خنق النظام مثلا): المهلة
            // تجيب بالنتائج الموجودة بدلا من الفشل
            // الفوري.
        }
    }

    /** جواب مسح واحد — النتائج؛ ويلغي المستقبل
     * والمهلة ويمسح الخانة. */
    private void answerWifiScanResults(PendingWifiScan p, WifiManager wm) {
        if (p.answered) return;
        p.answered = true;
        if (p.receiver != null) {
            try {
                unregisterReceiver(p.receiver);
            } catch (Exception ignored) {
            }
        }
        if (p.timeout != null && p.handler != null) {
            p.handler.removeCallbacks(p.timeout);
        }
        if (pendingWifiScan == p) pendingWifiScan = null;
        JSONArray list = new JSONArray();
        try {
            for (ScanResult r : wm.getScanResults()) {
                JSONObject d = new JSONObject();
                d.put("شبكة",
                        r.SSID == null || r.SSID.isEmpty() ? "مخفية" : r.SSID);
                d.put("عنوان_الجهاز",
                        r.BSSID == null ? "غير معروف" : r.BSSID);
                d.put("تردد", r.frequency);
                d.put("قوة_الإشارة", r.level);
                d.put("قدرات", r.capabilities == null ? "" : r.capabilities);
                list.put(d);
            }
        } catch (SecurityException e) {
            sendDeviceError(p.session,
                    "رفض إذن الموقع — فعّل الإذن من إعدادات التطبيق");
            return;
        } catch (JSONException ignored) {
            sendDeviceError(p.session, "تعذّر قراءة نتائج المسح");
            return;
        }
        sendDeviceValue(p.session, list);
    }

    /** جواب مسح واحد — خطأ؛ ويلغي المستقبل
     * والمهلة ويمسح الخانة. */
    private void answerWifiScanError(PendingWifiScan p, String message) {
        if (p.answered) return;
        p.answered = true;
        if (p.receiver != null) {
            try {
                unregisterReceiver(p.receiver);
            } catch (Exception ignored) {
            }
        }
        if (p.timeout != null && p.handler != null) {
            p.handler.removeCallbacks(p.timeout);
        }
        if (pendingWifiScan == p) pendingWifiScan = null;
        sendDeviceError(p.session, message);
    }

    // ===== واجهة الجهاز من البرنامج — الدفعة 4ب
    // (البنود 61 و62 و66–72 و79 و105 و106 و119) =====
    // النمط نفسه كالدفعة 4أ: طلب JSON عبر قناة
    // التحكم، ونتيجة واحدة بالضبط (قيمة أو خطأ)
    // على stdin — فلا يعلق المحرك أبدا. والتسجيل
    // والمصباح يُنظَّفان عند موت الجلسة (انظر
    // cancelDeviceRequestsForSession).

    /** استقبال طلب نسخ آخر خطأ (البند 61): آخر سطر
     * في نص الجلسة يحوي «خطأ» يُنسخ إلى الحافظة
     * وتُرجع قيمته نصا — وبلا سطر خطأ خطأ عربي
     * واضح. النسخ اليدوي بالتحديد موجود مجانا من
     * النظام؛ وهذا للحالة الشائعة: إرسال الخطأ
     * للدعم بلا تحديد يدوي. */
    private void handleCopyLastErrorRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب نسخ غير صالح");
            return;
        }
        String text = s.text.toString();
        String lastError = null;
        int start = 0;
        while (true) {
            int nl = text.indexOf('\n', start);
            String line = nl < 0 ? text.substring(start)
                    : text.substring(start, nl);
            if (line.contains("خطأ")) lastError = line;
            if (nl < 0) break;
            start = nl + 1;
        }
        if (lastError == null || lastError.isEmpty()) {
            sendDeviceError(s, "لا يوجد سطر خطأ في الجلسة");
            return;
        }
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null) {
            sendDeviceError(s, "الحافظة غير متاحة");
            return;
        }
        cm.setPrimaryClip(ClipData.newPlainText("نيفا", lastError));
        sendDeviceValue(s, lastError);
    }

    /** استقبال طلب مشاركة نص الجلسة (البند 62): آخر
     * 100 ألف حرف فقط (بلا تجاوز حد المعاملة) عبر
     * نافذة المشاركة؛ والنتيجة فور الإطلاق. وبلا
     * تقرير تشخيص تلقائي ولا معلومات جهاز مرفقة —
     * المستخدم يرسل سطر الخطأ الأحمر بنفسه (البند
     * 61 يغطي الحالة الشائعة). */
    private void handleShareSessionRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مشاركة غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        String text = s.text.toString();
        if (text.length() > SESSION_SHARE_CAP) {
            text = text.substring(text.length() - SESSION_SHARE_CAP);
        }
        if (text.isEmpty()) {
            sendDeviceError(s, "نص الجلسة فارغ");
            return;
        }
        final String shareText = text;
        confirmShareText(shareText, () -> {
            Intent intent = new Intent(Intent.ACTION_SEND);
            intent.setType("text/plain");
            intent.putExtra(Intent.EXTRA_TEXT, shareText);
            try {
                startActivity(Intent.createChooser(intent, "مشاركة الجلسة"));
            } catch (ActivityNotFoundException e) {
                sendDeviceError(s, "لا يوجد تطبيق للمشاركة");
                return;
            }
            sendDeviceValue(s, true);
        }, () -> sendDeviceError(s, "ألغى المستخدم مشاركة الجلسة"));
    }

    /** استقبال طلب إيقاف الصوت مؤقتا (البند 66):
     * الإيقاف يبقي المشغّل حيا لا يطلقه — فيستأنف
     * من موضعه؛ وبلا صوت جارٍ نجاح صامت
     * كـ`أوقف_صوت`. */
    private void handlePauseAudioRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب إيقاف غير صالح");
            return;
        }
        if (audioPlayer != null && audioPlayingSession == s && !audioPaused) {
            try {
                audioPlayer.pause();
                audioPaused = true;
            } catch (Exception ignored) {
            }
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب استئناف الصوت (البند 66): يعيد
     * تشغيل الموقوف مؤقتا من موضعه؛ وبلا صوت موقوف
     * نجاح صامت. */
    private void handleResumeAudioRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب استئناف غير صالح");
            return;
        }
        if (audioPlayer != null && audioPlayingSession == s && audioPaused) {
            try {
                audioPlayer.start();
                audioPaused = false;
            } catch (Exception ignored) {
            }
        }
        sendDeviceValue(s, true);
    }

    /** استقبال طلب معلومات ملف صوت (البند 67):
     * قاموس {مدة} بالثواني قبل التشغيل — بلا إذن. */
    private void handleAudioInfoRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب معلومات غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        if (path.isEmpty()) {
            sendDeviceError(s, "طلب معلومات غير صالح");
            return;
        }
        File file = new File(path);
        if (!file.isFile()) {
            sendDeviceError(s, "ملف الصوت غير موجود");
            return;
        }
        MediaMetadataRetriever retriever = new MediaMetadataRetriever();
        try {
            retriever.setDataSource(path);
            String ms = retriever.extractMetadata(
                    MediaMetadataRetriever.METADATA_KEY_DURATION);
            if (ms == null) {
                sendDeviceError(s, "ملف الصوت غير مدعوم أو تالف");
                return;
            }
            double seconds = Long.parseLong(ms) / 1000.0;
            JSONObject v = new JSONObject();
            v.put("مدة", seconds);
            sendDeviceValue(s, v);
        } catch (Exception e) {
            sendDeviceError(s, "ملف الصوت غير مدعوم أو تالف");
        } finally {
            try {
                retriever.release();
            } catch (Exception ignored) {
            }
        }
    }

    /** اسم نيفا لنوع حساس أندرويد — عكس
     * sensorTypeForName؛ وnull لما ليس من الثمانية. */
    private static String sensorNameForType(int type) {
        switch (type) {
            case Sensor.TYPE_ACCELEROMETER: return "تسارع";
            case Sensor.TYPE_GYROSCOPE: return "جيروسكوب";
            case Sensor.TYPE_LIGHT: return "ضوء";
            case Sensor.TYPE_PROXIMITY: return "قرب";
            case Sensor.TYPE_MAGNETIC_FIELD: return "مغناطيسي";
            case Sensor.TYPE_PRESSURE: return "ضغط";
            case Sensor.TYPE_RELATIVE_HUMIDITY: return "رطوبة";
            case Sensor.TYPE_AMBIENT_TEMPERATURE: return "حرارة";
            default: return null;
        }
    }

    /** استقبال طلب سرد الحساسات (البند 68): أسماء
     * الحساسات الثمانية الموجودة على الجهاز فقط —
     * بلا إذن؛ وتساعد البرنامج قبل السؤال عن حساس
     * غائب. والقراءة المستمرة مرفوضة (البند 143) —
     * من أراد تكرارا كرر الأمر بنفسه. */
    private void handleListSensorsRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب حساس غير صالح");
            return;
        }
        if (sensorManager == null) {
            sensorManager = (SensorManager)
                    getSystemService(Context.SENSOR_SERVICE);
        }
        if (sensorManager == null) {
            sendDeviceError(s, "الحساسات غير متاحة");
            return;
        }
        java.util.HashSet<String> present = new java.util.HashSet<>();
        try {
            for (Sensor sensor : sensorManager.getSensorList(Sensor.TYPE_ALL)) {
                String name = sensorNameForType(sensor.getType());
                if (name != null) present.add(name);
            }
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر سرد الحساسات");
            return;
        }
        JSONArray list = new JSONArray();
        String[] order = {"تسارع", "جيروسكوب", "ضوء", "قرب",
                "مغناطيسي", "ضغط", "رطوبة", "حرارة"};
        for (String name : order) {
            if (present.contains(name)) list.put(name);
        }
        sendDeviceValue(s, list);
    }

    /** مدير الصوت (كسول — بلا إذن للقراءة). */
    private AudioManager audioManager() {
        return (AudioManager) getSystemService(Context.AUDIO_SERVICE);
    }

    /** استقبال طلب قراءة مستوى الصوت (البند 69):
     * قاموس {مستوى، أقصى} لتيار الموسيقى — بلا إذن. */
    private void handleVolumeLevelRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مستوى غير صالح");
            return;
        }
        AudioManager am = audioManager();
        if (am == null) {
            sendDeviceError(s, "الصوت غير متاح");
            return;
        }
        try {
            JSONObject v = new JSONObject();
            v.put("مستوى", am.getStreamVolume(AudioManager.STREAM_MUSIC));
            v.put("أقصى", am.getStreamMaxVolume(AudioManager.STREAM_MUSIC));
            sendDeviceValue(s, v);
        } catch (JSONException ignored) {
        }
    }

    /** استقبال طلب ضبط مستوى الصوت (البند 70):
     * يُقصّ إلى [0، الأقصى] ويرجع المستوى الجديد.
     * الإذن MODIFY_AUDIO_SETTINGS إذن عادي يُمنح
     * عند التثبيت (في البيان) — تحقّق وقت البناء. */
    private void handleSetVolumeRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب ضبط غير صالح");
            return;
        }
        int level = req.optInt("مستوى", -1);
        if (level < 0) {
            sendDeviceError(s, "طلب ضبط غير صالح");
            return;
        }
        AudioManager am = audioManager();
        if (am == null) {
            sendDeviceError(s, "الصوت غير متاح");
            return;
        }
        int max = am.getStreamMaxVolume(AudioManager.STREAM_MUSIC);
        if (level > max) level = max;
        try {
            am.setStreamVolume(AudioManager.STREAM_MUSIC, level, 0);
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر ضبط مستوى الصوت");
            return;
        }
        sendDeviceValue(s, (double) am.getStreamVolume(AudioManager.STREAM_MUSIC));
    }

    /** استقبال طلب اهتزاز (البند 71): مدة بالمللي
     * ثانية (المحرك فحص 1–10000) — بلا إذن عمليا. */
    private void handleVibrateRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب اهتزاز غير صالح");
            return;
        }
        long ms = req.optLong("مدة", -1);
        if (ms < 1) {
            sendDeviceError(s, "طلب اهتزاز غير صالح");
            return;
        }
        vibrate(ms);
        sendDeviceValue(s, true);
    }

    /** هل إذن الكاميرا ممنوح؟ (للمصباح أيضا — مدير
     * الكاميرا هو من يشغّله.) */
    private boolean torchPermissionGranted() {
        return cameraPermissionGranted();
    }

    /** استقبال طلب مصباح (البند 72): تشغيل/إيقاف؛
     * ويرجع الحالة الجديدة. يحتاج إذن الكاميرا في
     * الحقيقة — يُطلب عند أول تشغيل فقط؛ والإيقاف
     * بلا إذن جديد (مصباح مطفأ أصلا إن لم يُمنح). */
    private void handleTorchRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب مصباح غير صالح");
            return;
        }
        boolean on = req.optBoolean("تشغيل", false);
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (!on) {
            // إيقاف: بلا إذن جديد — وإن لم يكن مشتغلا
            // أصلا نجاح صامت.
            setTorchMode(s, false);
            return;
        }
        if (pendingTorch != null) {
            sendDeviceError(s, "طلب مصباح آخر جارٍ");
            return;
        }
        pendingTorch = new PendingTorch(s, true);
        if (torchPermissionGranted()) {
            setTorchMode(s, true);
            pendingTorch = null;
            return;
        }
        if (!torchPermissionRequestInFlight) {
            torchPermissionRequestInFlight = true;
            requestPermissions(
                    new String[]{"android.permission.CAMERA"},
                    REQ_TORCH_PERMISSION);
        }
    }

    /** تنفيذ المصباح (تشغيل/إيقاف) وإرسال الحالة
     * الجديدة؛ وموت الجلسة المشغّلة يطفئه (انظر
     * cancelDeviceRequestsForSession). */
    private void setTorchMode(Session s, boolean on) {
        CameraManager cm = (CameraManager)
                getSystemService(Context.CAMERA_SERVICE);
        if (cm == null) {
            sendDeviceError(s, "الكاميرا غير متاحة");
            return;
        }
        try {
            if (on) {
                String id = torchCameraId;
                if (id == null) {
                    for (String cid : cm.getCameraIdList()) {
                        CameraCharacteristics c =
                                cm.getCameraCharacteristics(cid);
                        Boolean flash = c.get(
                                CameraCharacteristics.FLASH_INFO_AVAILABLE);
                        if (flash != null && flash) {
                            id = cid;
                            break;
                        }
                    }
                }
                if (id == null) {
                    sendDeviceError(s, "لا توجد كاميرا بمصباح");
                    return;
                }
                cm.setTorchMode(id, true);
                torchCameraId = id;
                torchOn = true;
                torchSession = s;
                sendDeviceValue(s, true);
            } else {
                if (torchOn && torchCameraId != null) {
                    try {
                        cm.setTorchMode(torchCameraId, false);
                    } catch (Exception ignored) {
                    }
                }
                torchOn = false;
                torchSession = null;
                sendDeviceValue(s, false);
            }
        } catch (Exception e) {
            sendDeviceError(s, on ? "تعذّر تشغيل المصباح"
                    : "تعذّر إيقاف المصباح");
        }
    }

    /** استقبال طلب تسجيل صوت (البند 79): ملف داخل
     * مجلد البرنامج بمدة قصوى (المحرك فحص 1–300،
     * الافتراضي 60)؛ والتسجيل الواحد في كل التطبيق
     * (تسجيل ثانٍ يُرفض بخطأ). الإذن يُطلب عند أول
     * استعمال بعد حوار تنبيه أن الملف يُحفظ محليا
     * فقط ولا يُرسل إلى أي مكان. */
    private void handleRecordAudioRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب تسجيل غير صالح");
            return;
        }
        String path = req.optString("مسار", "");
        int seconds = req.optInt("مدة", -1);
        if (path.isEmpty() || seconds < 1) {
            sendDeviceError(s, "طلب تسجيل غير صالح");
            return;
        }
        if (isFinishing() || isDestroyed()) {
            sendDeviceError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) return;
        if (recordingSession != null || pendingRecord != null) {
            sendDeviceError(s, "تسجيل آخر جارٍ");
            return;
        }
        File file = deviceFileInsideProgramDir(path);
        if (file == null) {
            sendDeviceError(s, "مسار التسجيل خارج مجلد البرنامج");
            return;
        }
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            sendDeviceError(s, "تعذّر إنشاء مجلد التسجيل");
            return;
        }
        pendingRecord = new PendingRecord(s, file, seconds);
        if (recordPermissionGranted()) {
            PendingRecord p = pendingRecord;
            pendingRecord = null;
            startRecording(p);
            return;
        }
        // بلا إذن: حوار تنبيه أولا (الملف محلي فقط) —
        // وطلب ثانٍ أثناء الحوار يرفضه فحص
        // pendingRecord في الأعلى بخطأ.
        recordDisclosureDialog = new AlertDialog.Builder(this)
                .setTitle("تسجيل صوت")
                .setMessage("سيُسجَّل الصوت من الميكروفون ويُحفظ "
                        + "ملفا في مجلد البرنامج فقط — ولا يُرسل "
                        + "إلى أي مكان.")
                .setPositiveButton("متابعة", (d, w) -> {
                    recordDisclosureDialog = null;
                    if (!recordPermissionRequestInFlight) {
                        recordPermissionRequestInFlight = true;
                        requestPermissions(
                                new String[]{"android.permission.RECORD_AUDIO"},
                                REQ_RECORD_PERMISSION);
                    }
                })
                .setNegativeButton("إلغاء", (d, w) -> {
                    recordDisclosureDialog = null;
                    PendingRecord p = pendingRecord;
                    pendingRecord = null;
                    if (p != null) {
                        sendDeviceError(p.session, "ألغى المستخدم التسجيل");
                    }
                })
                .setCancelable(false)
                .show();
    }

    /** هل إذن الميكروفون ممنوح؟ */
    private boolean recordPermissionGranted() {
        return checkSelfPermission("android.permission.RECORD_AUDIO")
                == PackageManager.PERMISSION_GRANTED;
    }

    /** بدء التسجيل الفعلي (بعد الإذن): MPEG_4/AAC،
     * وإيقاف تلقائي عند المدة القصوى عبر مستمع
     * المعلومات + مهلة احتياطية. */
    private void startRecording(PendingRecord p) {
        MediaRecorder recorder = new MediaRecorder();
        try {
            recorder.setAudioSource(MediaRecorder.AudioSource.MIC);
            recorder.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4);
            recorder.setAudioEncoder(MediaRecorder.AudioEncoder.AAC);
            recorder.setOutputFile(p.file.getAbsolutePath());
            recorder.setMaxDuration(p.seconds * 1000);
            recorder.setOnInfoListener((mr, what, extra) -> {
                if (what == MediaRecorder.MEDIA_RECORDER_INFO_MAX_DURATION_REACHED) {
                    runOnUiThread(() -> finishRecording(true, null));
                }
            });
            recorder.setOnErrorListener((mr, what, extra) -> {
                runOnUiThread(() -> finishRecording(false, "تعذّر التسجيل"));
            });
            recorder.prepare();
            recorder.start();
        } catch (Exception e) {
            try {
                recorder.release();
            } catch (Exception ignored) {
            }
            sendDeviceError(p.session, "تعذّر بدء التسجيل");
            return;
        }
        audioRecorder = recorder;
        recordingSession = p.session;
        recordingFile = p.file;
        Handler handler = new Handler(Looper.getMainLooper());
        recordingTimeoutHandler = handler;
        recordingTimeout = () -> finishRecording(true, null);
        handler.postDelayed(recordingTimeout, (long) p.seconds * 1000 + 1000);
    }

    /** إنهاء التسجيل (نجاح عند المدة أو خطأ) —
     * تحرير المسجّل ومسح الخانات وإرسال النتيجة
     * مرة واحدة. */
    private void finishRecording(boolean ok, String errorMessage) {
        Session s = recordingSession;
        File f = recordingFile;
        MediaRecorder recorder = audioRecorder;
        audioRecorder = null;
        recordingSession = null;
        recordingFile = null;
        if (recordingTimeout != null && recordingTimeoutHandler != null) {
            recordingTimeoutHandler.removeCallbacks(recordingTimeout);
        }
        recordingTimeout = null;
        recordingTimeoutHandler = null;
        if (recorder != null) {
            try {
                recorder.stop();
            } catch (Exception ignored) {
            }
            try {
                recorder.release();
            } catch (Exception ignored) {
            }
        }
        if (s == null) return;
        if (s.process == null || s.ended) return;
        if (!ok) {
            if (f != null) f.delete();
            sendDeviceError(s, errorMessage == null ? "تعذّر التسجيل" : errorMessage);
            return;
        }
        if (f == null || !f.isFile() || f.length() == 0) {
            if (f != null) f.delete();
            sendDeviceError(s, "تعذّر حفظ التسجيل");
            return;
        }
        sendDeviceValue(s, true);
    }

    /** إيقاف تسجيل جلسة تموت صامتا (المحرك مات فلا
     * نتيجة تُرسَل) — يُستدعى من
     * cancelDeviceRequestsForSession. */
    private void stopRecordingForSession(Session s) {
        if (recordingSession == s) {
            MediaRecorder recorder = audioRecorder;
            audioRecorder = null;
            recordingSession = null;
            recordingFile = null;
            if (recordingTimeout != null && recordingTimeoutHandler != null) {
                recordingTimeoutHandler.removeCallbacks(recordingTimeout);
            }
            recordingTimeout = null;
            recordingTimeoutHandler = null;
            if (recorder != null) {
                try {
                    recorder.stop();
                } catch (Exception ignored) {
                }
                try {
                    recorder.release();
                } catch (Exception ignored) {
                }
            }
        }
        if (pendingRecord != null && pendingRecord.session == s) {
            pendingRecord = null;
        }
        if (recordDisclosureDialog != null && recordingSession == null
                && pendingRecord == null) {
            try {
                recordDisclosureDialog.dismiss();
            } catch (Exception ignored) {
            }
            recordDisclosureDialog = null;
        }
    }

    /** استقبال طلب سطوع (البند 105): عدد 0–100 أو
     * «تلقائي»؛ ويرجع قاموس {مستوى، تلقائي}.
     * الكتابة في إعدادات النظام إذن خاص — بلا منح
     * يفتح التطبيق شاشة إعدادات النظام ويرجع خطأ
     * يعيد المحاولة بعد المنح (لا نتيجة معلّقة). */
    private void handleBrightnessRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب سطوع غير صالح");
            return;
        }
        boolean auto = req.optBoolean("تلقائي", false);
        int level = req.optInt("مستوى", -1);
        if (!auto && level < 0) {
            sendDeviceError(s, "طلب سطوع غير صالح");
            return;
        }
        if (!Settings.System.canWrite(this)) {
            Intent intent = new Intent(
                    Settings.ACTION_MANAGE_WRITE_SETTINGS,
                    Uri.parse("package:" + getPackageName()));
            try {
                startActivity(intent);
            } catch (ActivityNotFoundException ignored) {
            }
            sendDeviceError(s,
                    "افتح إعدادات النظام وامنح إذن الكتابة ثم أعد المحاولة");
            return;
        }
        try {
            if (auto) {
                Settings.System.putInt(getContentResolver(),
                        Settings.System.SCREEN_BRIGHTNESS_MODE,
                        Settings.System.SCREEN_BRIGHTNESS_MODE_AUTOMATIC);
                int current = Settings.System.getInt(getContentResolver(),
                        Settings.System.SCREEN_BRIGHTNESS, 128);
                JSONObject v = new JSONObject();
                v.put("مستوى", current * 100 / 255);
                v.put("تلقائي", true);
                sendDeviceValue(s, v);
            } else {
                Settings.System.putInt(getContentResolver(),
                        Settings.System.SCREEN_BRIGHTNESS_MODE,
                        Settings.System.SCREEN_BRIGHTNESS_MODE_MANUAL);
                int value = level * 255 / 100;
                Settings.System.putInt(getContentResolver(),
                        Settings.System.SCREEN_BRIGHTNESS, value);
                JSONObject v = new JSONObject();
                v.put("مستوى", level);
                v.put("تلقائي", false);
                sendDeviceValue(s, v);
            }
        } catch (Exception e) {
            sendDeviceError(s, "تعذّر ضبط السطوع");
        }
    }

    /** استقبال طلب توست (البند 106): رسالة عائمة
     * تختفي وحدها؛ وترجع صحيحا (المحرك يحوّلها إلى
     * لا شيء). قاعدة التوست 196-1: إلغاء السابق
     * قبل عرض الجديد — توست واحد ظاهر في كل مرة.
     * وملاحظة: المساعد الداخلي toast(String) يبقى
     * لرسائل التطبيق نفسه — بلا تغيير نطاق. */
    private void handleToastRequest(Session s, String payload) {
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب توست غير صالح");
            return;
        }
        String text = req.optString("نص", "");
        boolean longShow = req.optBoolean("طويل", false);
        if (text.isEmpty()) {
            sendDeviceError(s, "طلب توست غير صالح");
            return;
        }
        if (currentToast != null) {
            try {
                currentToast.cancel();
            } catch (Exception ignored) {
            }
        }
        currentToast = Toast.makeText(this, text,
                longShow ? Toast.LENGTH_LONG : Toast.LENGTH_SHORT);
        currentToast.show();
        sendDeviceValue(s, true);
    }

    /** استقبال طلب معلومات الصوت (البند 119): قاموس
     * {معدل_العينات، حجم_المخزن، بلوتوث،
     * سماعة_سلكية} — بلا إذن. */
    private void handleAudioDeviceInfoRequest(Session s, String payload) {
        try {
            new JSONObject(payload);
        } catch (JSONException e) {
            sendDeviceError(s, "طلب معلومات غير صالح");
            return;
        }
        AudioManager am = audioManager();
        if (am == null) {
            sendDeviceError(s, "الصوت غير متاح");
            return;
        }
        int sampleRate = AudioTrack.getNativeOutputSampleRate(
                AudioManager.STREAM_MUSIC);
        int frames = 0;
        String framesProp = am.getProperty(
                AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER);
        if (framesProp != null) {
            try {
                frames = Integer.parseInt(framesProp);
            } catch (NumberFormatException ignored) {
            }
        }
        try {
            JSONObject v = new JSONObject();
            v.put("معدل_العينات", sampleRate);
            v.put("حجم_المخزن", frames);
            v.put("بلوتوث", am.isBluetoothA2dpOn());
            v.put("سماعة_سلكية", am.isWiredHeadsetOn());
            sendDeviceValue(s, v);
        } catch (JSONException ignored) {
        }
    }

    // ===== ملفات في شجرة ممنوحة SAF (المهمة 28، CATALOG-19) =====
    // المحرك يطلب والتطبيق ينفّذ — شجرة SAF عناوين
    // `content://` لا يفتحها `std::fs` إطلاقا. بادئة
    // واحدة (`ملف-ممنوح:`) والتفريع على حقل `عملية`.
    // والنتيجة سطر واحد `نيفا-نتيجة:` كما الحوار
    // والتنبيه (البادئة نفسها). والمنحة ملك التطبيق:
    // معرّفات رتيبة من 1، سقف 64، والإبطال من إعدادات
    // النظام فقط. والطلب لجلسة خلفية يُحفَظ ويُنفَّذ
    // عند التبديل إليها (نمط الحوار). وإلغاء قراءة
    // SAF جارية بزر «ألغِ الأمر» يجهض انتظار المحرك
    // (انتظار SAF قابل للإلغاء في المحرك) — ونتيجة
    // تصل بعده تُسقَط صامتا (علم safCancelArmed)
    // فلا يبقى سطر نتيجة شارد يفسد القراءة التالية.

    /** خطأ SAF عربي واحد — يحمله `SafFailure` من
     * العمق إلى المرسل. */
    private static class SafFailure extends Exception {
        SafFailure(String message) {
            super(message);
        }
    }

    /** معلومات مستند واحدة (نوع وحجم وتاريخ). */
    private static class SafInfo {
        String mime;
        long size;
        long lastModified;
    }

    /** استقبال طلب SAF من قناة التحكم. */
    private void handleSafControl(Session s, String payload) {
        if (isFinishing() || isDestroyed()) {
            sendSafError(s, "التطبيق غير متاح");
            return;
        }
        if (s.process == null || s.ended) {
            // بلا محرك حي فلا طالب — لا نتيجة تُرسَل.
            return;
        }
        if (s != active()) {
            // جلسة خلفية: يُحفَظ حتى تصير نشطة.
            s.pendingSaf = payload;
            return;
        }
        runSafRequest(s, payload);
    }

    /** تنفيذ طلب SAF معلّق حين تصير جلسته نشطة. */
    private void maybeShowPendingSaf(Session s) {
        if (s.pendingSaf != null && s.process != null && !s.ended) {
            String payload = s.pendingSaf;
            s.pendingSaf = null;
            runSafRequest(s, payload);
        }
    }

    /** تنفيذ طلب SAF واحد: إلغاء مسلّح يُسقِط الطلب
     * بلا نتيجة (المحرك أجهض الانتظار على سطر
     * الإلغاء)؛ وإلا يُنفَّذ وتُرسَل نتيجة واحدة. */
    private void runSafRequest(Session s, String payload) {
        if (s.safCancelArmed) {
            // ضغطة إلغاء سبقت الطلب — المحرك أجهض
            // انتظاره؛ فلا نتيجة تُرسَل أبدا (سطر
            // شارد)، ويُمسح العلم.
            s.safCancelArmed = false;
            s.safWaitPending = false;
            return;
        }
        s.safWaitPending = true;
        try {
            JSONObject req = new JSONObject(payload);
            String op = req.optString("عملية", "");
            switch (op) {
                case "امنح_مجلد":
                    handleSafGrantTree(s);
                    break;
                case "سرد_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    Uri doc = safResolveDocument(g, req.optString("مسار", ""));
                    sendSafValue(s, safListChildren(g, doc));
                    break;
                }
                case "اقرأ_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    Uri doc = safResolveDocument(g, path);
                    sendSafValue(s, safReadText(doc, path));
                    break;
                }
                case "اكتب_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    Uri doc = safResolveDocument(g, path);
                    safWriteText(doc, req.optString("نص", ""), path);
                    sendSafValue(s, true);
                    break;
                }
                case "أنشئ_ملف_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    safCreateDocument(g, path, false);
                    sendSafValue(s, true);
                    break;
                }
                case "أنشئ_مجلد_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    safCreateDocument(g, path, true);
                    sendSafValue(s, true);
                    break;
                }
                case "تفاصيل_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    Uri doc = safResolveDocument(g, path);
                    sendSafValue(s, safStatDict(doc, path));
                    break;
                }
                case "احذف_ممنوح": {
                    Grant g = safLiveGrant(req.getLong("معرف"));
                    String path = req.optString("مسار", "");
                    safDeleteDocument(g, path);
                    sendSafValue(s, true);
                    break;
                }
                case "الممنوح":
                    sendSafValue(s, safListGrantsJson());
                    break;
                default:
                    sendSafError(s, "عملية غير معروفة: " + op);
                    break;
            }
        } catch (JSONException e) {
            sendSafError(s, "طلب ملف-ممنوح غير صالح");
        } catch (SafFailure e) {
            sendSafError(s, e.getMessage());
        }
    }

    /** إرسال نتيجة SAF واحدة إلى المحرك عبر الطابور
     * المحدود — البادئة نفسها. ونتيجة تصل بعد إلغاء
     * مسلّح تُسقَط صامتا (لا سطر شارد). */
    private void sendSafResult(Session s, JSONObject result) {
        if (s.safCancelArmed) {
            s.safCancelArmed = false;
            s.safWaitPending = false;
            return;
        }
        s.safWaitPending = false;
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendSafValue(Session s, Object value) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", value);
            sendSafResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendSafError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendSafResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    // ----- سجل المنح -----

    /** تحميل سجل المنح من الإعدادات عند البدء، مع
     * تقليم المنح الميتة بصمت (CATALOG-19 §3). */
    private void loadGrants() {
        if (prefs == null) return;
        nextGrantId = prefs.getLong("منحة_أخير", 1);
        if (nextGrantId < 1) nextGrantId = 1;
        grants.clear();
        for (long id = 1; id < nextGrantId; id++) {
            String uriStr = prefs.getString("منحة_عنوان_" + id, null);
            String name = prefs.getString("منحة_اسم_" + id, null);
            if (uriStr == null || name == null) continue;
            grants.add(new Grant(id, name, Uri.parse(uriStr)));
        }
        pruneGrantsAtStart();
    }

    /** حفظ سجل المنح: تُمسح مفاتيح المنح القديمة كلها
     * ثم تُكتب الحية — فالمحذوفة لا تبقى شبحا. */
    private void saveGrants() {
        if (prefs == null) return;
        SharedPreferences.Editor e = prefs.edit();
        e.putLong("منحة_أخير", nextGrantId);
        for (long id = 1; id < nextGrantId; id++) {
            e.remove("منحة_عنوان_" + id);
            e.remove("منحة_اسم_" + id);
        }
        for (Grant g : grants) {
            e.putString("منحة_عنوان_" + g.id, g.uri.toString());
            e.putString("منحة_اسم_" + g.id, g.name);
        }
        e.apply();
    }

    /** تقليم المنح الميتة عند البدء (CATALOG-19 §3). */
    private void pruneGrantsAtStart() {
        boolean changed = false;
        java.util.Iterator<Grant> it = grants.iterator();
        while (it.hasNext()) {
            if (!safGrantAlive(it.next())) {
                it.remove();
                changed = true;
            }
        }
        if (changed) saveGrants();
    }

    /** هل المنحة ما زالت ممنوحة من النظام (قراءة
     * وكتابة)؟ */
    private boolean safGrantAlive(Grant g) {
        try {
            for (UriPermission p
                    : getContentResolver().getPersistedUriPermissions()) {
                if (p.getUri().equals(g.uri) && p.isReadPermission()
                        && p.isWritePermission()) {
                    return true;
                }
            }
        } catch (Exception ignored) {
        }
        return false;
    }

    /** منحة حية بالمعرف — أو رفض عربي (غير معروفة /
     * مبطلة من إعدادات النظام). والمبطلة تُقلم من
     * السجل بصمت عند اكتشافها. */
    private Grant safLiveGrant(long id) throws SafFailure {
        Grant found = null;
        for (Grant g : grants) {
            if (g.id == id) {
                found = g;
                break;
            }
        }
        if (found == null) {
            throw new SafFailure("الشجرة غير معروفة: " + id);
        }
        if (!safGrantAlive(found)) {
            grants.remove(found);
            saveGrants();
            throw new SafFailure("الشجرة غير متاحة — أُبطلت المنحة من إعدادات النظام");
        }
        return found;
    }

    /** قائمة المنح الحية بصيغة JSON (تُقلم الميتة
     * بصمت قبل السرد). */
    private JSONArray safListGrantsJson() {
        pruneGrantsAtStart();
        JSONArray arr = new JSONArray();
        try {
            for (Grant g : grants) {
                JSONObject d = new JSONObject();
                d.put("معرف", g.id);
                d.put("اسم", g.name);
                arr.put(d);
            }
        } catch (JSONException ignored) {
        }
        return arr;
    }

    // ----- منتقي المنح -----

    /** طلب منح مجلد: يفتح منتقي النظام؛ والنتيجة في
     * onActivityResult (معرف جديد أو إلغاء). */
    private void handleSafGrantTree(Session s) {
        if (grants.size() >= MAX_GRANTS) {
            sendSafError(s, "بلغت الحد الأقصى للشجرات الممنوحة (٦٤)");
            return;
        }
        safGrantSession = s;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        try {
            startActivityForResult(intent, REQ_SAF_GRANT);
        } catch (Exception e) {
            safGrantSession = null;
            sendSafError(s, "تعذّر فتح منتقي المجلدات");
        }
    }

    /** اسم الشجرة للعرض (DISPLAY_NAME للمستند الجذر،
     * أو آخر مقطع من العنوان عند الفشل). */
    private String safTreeDisplayName(Uri treeUri) {
        try {
            String docId = DocumentsContract.getTreeDocumentId(treeUri);
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId);
            try (Cursor c = getContentResolver().query(docUri,
                    new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},
                    null, null, null)) {
                if (c != null && c.moveToFirst()) {
                    String name = c.getString(0);
                    if (name != null && !name.isEmpty()) return name;
                }
            }
        } catch (Exception ignored) {
        }
        String last = treeUri.getLastPathSegment();
        return last != null ? last : "مجلد ممنوح";
    }

    // ----- حل المسارات -----

    /** مقاطع المسار بعد الفحص المعجمي: `..` والفارغ
     * (مسار مطلق أو //) مرفوضة «خارج الشجرة»؛ و`.`
     * تُتخطى؛ والفارغ هو الجذر نفسه. */
    private List<String> safPathSegments(String path) throws SafFailure {
        if (path == null) throw new SafFailure("المسار غير صالح");
        if (path.isEmpty()) return new ArrayList<>();
        String p = path;
        if (p.endsWith("/")) p = p.substring(0, p.length() - 1);
        if (p.isEmpty()) return new ArrayList<>();
        String[] parts = p.split("/", -1);
        List<String> out = new ArrayList<>();
        for (String part : parts) {
            if (part.isEmpty() || "..".equals(part)) {
                throw new SafFailure("المسار خارج الشجرة الممنوحة");
            }
            if (".".equals(part)) continue;
            out.add(part);
        }
        return out;
    }

    /** عنوان مستند ابن بالاسم تحت أب — أو null إن لم
     * يوجد. */
    private Uri safChildUri(Uri treeUri, String parentDocId, String name) {
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(
                treeUri, parentDocId);
        try (Cursor c = getContentResolver().query(children,
                new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_DISPLAY_NAME},
                DocumentsContract.Document.COLUMN_DISPLAY_NAME + "=?",
                new String[]{name}, null)) {
            if (c != null) {
                while (c.moveToNext()) {
                    if (name.equals(c.getString(1))) {
                        return DocumentsContract.buildDocumentUriUsingTree(
                                treeUri, c.getString(0));
                    }
                }
            }
        } catch (Exception ignored) {
        }
        return null;
    }

    /** عنوان مستند المسار داخل الشجرة — أو رفض «غير
     * موجود». */
    private Uri safResolveDocument(Grant g, String path) throws SafFailure {
        List<String> segments = safPathSegments(path);
        String docId = DocumentsContract.getTreeDocumentId(g.uri);
        Uri doc = DocumentsContract.buildDocumentUriUsingTree(g.uri, docId);
        for (String seg : segments) {
            Uri child = safChildUri(g.uri, docId, seg);
            if (child == null) {
                throw new SafFailure("المسار غير موجود: " + path);
            }
            docId = DocumentsContract.getDocumentId(child);
            doc = child;
        }
        return doc;
    }

    /** معلومات مستند (نوع وحجم وتاريخ) باستعلام واحد. */
    private SafInfo safDocInfo(Uri doc) throws SafFailure {
        try (Cursor c = getContentResolver().query(doc,
                new String[]{DocumentsContract.Document.COLUMN_MIME_TYPE,
                        DocumentsContract.Document.COLUMN_SIZE,
                        DocumentsContract.Document.COLUMN_LAST_MODIFIED},
                null, null, null)) {
            if (c != null && c.moveToFirst()) {
                SafInfo info = new SafInfo();
                info.mime = c.getString(0);
                info.size = c.isNull(1) ? 0 : c.getLong(1);
                info.lastModified = c.isNull(2) ? 0 : c.getLong(2);
                return info;
            }
        } catch (Exception ignored) {
        }
        throw new SafFailure("تعذّر قراءة معلومات المسار");
    }

    private static boolean safIsDir(SafInfo info) {
        return DocumentsContract.Document.MIME_TYPE_DIR.equals(info.mime);
    }

    // ----- العمليات -----

    /** سرد أسماء أبناء مجلد. */
    private JSONArray safListChildren(Grant g, Uri doc) throws SafFailure {
        SafInfo info = safDocInfo(doc);
        if (!safIsDir(info)) {
            throw new SafFailure("المسار ليس مجلدا");
        }
        String docId = DocumentsContract.getDocumentId(doc);
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(
                g.uri, docId);
        JSONArray names = new JSONArray();
        try (Cursor c = getContentResolver().query(children,
                new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},
                null, null, null)) {
            if (c != null) {
                while (c.moveToNext()) {
                    String name = c.getString(0);
                    if (name != null) names.put(name);
                }
            }
        } catch (Exception e) {
            throw new SafFailure("تعذّر سرد المجلد");
        }
        return names;
    }

    /** قراءة نص ملف (UTF-8 صارم، سقف 5 ميغابايت). */
    private String safReadText(Uri doc, String path) throws SafFailure {
        SafInfo info = safDocInfo(doc);
        if (safIsDir(info)) {
            throw new SafFailure("المسار ليس ملفا: " + path);
        }
        byte[] bytes;
        try (InputStream in = getContentResolver().openInputStream(doc)) {
            if (in == null) throw new SafFailure("تعذّر قراءة الملف");
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            int total = 0;
            while ((n = in.read(buf)) != -1) {
                total += n;
                if (total > SAF_READ_CAP) {
                    throw new SafFailure("الملف أكبر من الحد الأقصى (5 ميغابايت)");
                }
                out.write(buf, 0, n);
            }
            bytes = out.toByteArray();
        } catch (SafFailure e) {
            throw e;
        } catch (java.io.FileNotFoundException e) {
            throw new SafFailure("المسار غير موجود: " + path);
        } catch (Exception e) {
            throw new SafFailure("تعذّر قراءة الملف");
        }
        try {
            return StandardCharsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(bytes)).toString();
        } catch (CharacterCodingException e) {
            throw new SafFailure("الملف ليس نصا صالحا");
        }
    }

    /** كتابة نص ملف (استبدال كامل — بلا إلحاق في v1). */
    private void safWriteText(Uri doc, String text, String path) throws SafFailure {
        SafInfo info = safDocInfo(doc);
        if (safIsDir(info)) {
            throw new SafFailure("المسار ليس ملفا: " + path);
        }
        try (OutputStream out = getContentResolver().openOutputStream(doc, "wt")) {
            if (out == null) throw new SafFailure("تعذّر كتابة الملف");
            out.write(text.getBytes(StandardCharsets.UTF_8));
        } catch (SafFailure e) {
            throw e;
        } catch (java.io.FileNotFoundException e) {
            throw new SafFailure("المسار غير موجود: " + path);
        } catch (Exception e) {
            throw new SafFailure("تعذّر كتابة الملف");
        }
    }

    /** نوع MIME من الامتداد (`.نيفا` نص عادي). */
    private static String safMimeForName(String name) {
        String lower = name.toLowerCase(Locale.US);
        if (lower.endsWith(".json")) return "application/json";
        if (lower.endsWith(".md")) return "text/markdown";
        if (lower.endsWith(".html") || lower.endsWith(".htm")) return "text/html";
        if (lower.endsWith(".xml")) return "text/xml";
        if (lower.endsWith(".csv")) return "text/csv";
        return "text/plain";
    }

    /** إنشاء ملف نصي فارغ أو مجلد تحت أبيه. */
    private void safCreateDocument(Grant g, String path, boolean dir)
            throws SafFailure {
        List<String> segments = safPathSegments(path);
        if (segments.isEmpty()) {
            throw new SafFailure("المسار غير صالح");
        }
        String name = segments.get(segments.size() - 1);
        List<String> parentSegs = segments.subList(0, segments.size() - 1);
        String parentPath = String.join("/", parentSegs);
        Uri parent = safResolveDocument(g, parentPath);
        SafInfo parentInfo = safDocInfo(parent);
        if (!safIsDir(parentInfo)) {
            throw new SafFailure("المسار الأب ليس مجلدا");
        }
        String parentId = DocumentsContract.getDocumentId(parent);
        if (safChildUri(g.uri, parentId, name) != null) {
            throw new SafFailure("الملف موجود أصلا: " + path);
        }
        String mime = dir ? DocumentsContract.Document.MIME_TYPE_DIR
                : safMimeForName(name);
        Uri created;
        try {
            created = DocumentsContract.createDocument(
                    getContentResolver(), parent, mime, name);
        } catch (Exception e) {
            throw new SafFailure("تعذّر إنشاء الملف");
        }
        if (created == null) {
            throw new SafFailure("تعذّر إنشاء الملف");
        }
    }

    /** تفاصيل مستند بصيغة قاموس {نوع، حجم، تاريخ UTC}. */
    private JSONObject safStatDict(Uri doc, String path) throws SafFailure {
        SafInfo info = safDocInfo(doc);
        boolean dir = safIsDir(info);
        SimpleDateFormat fmt = new SimpleDateFormat(
                "yyyy-MM-dd HH:mm:ss", Locale.US);
        fmt.setTimeZone(TimeZone.getTimeZone("UTC"));
        try {
            JSONObject d = new JSONObject();
            d.put("نوع", dir ? "مجلد" : "ملف");
            d.put("حجم", dir ? 0 : info.size);
            d.put("تاريخ", fmt.format(new Date(info.lastModified)));
            return d;
        } catch (JSONException e) {
            throw new SafFailure("تعذّر قراءة معلومات المسار");
        }
    }

    /** حذف ملف (ومجلد فارغ) مع التحقق بعد الحذف. */
    private void safDeleteDocument(Grant g, String path) throws SafFailure {
        List<String> segments = safPathSegments(path);
        if (segments.isEmpty()) {
            throw new SafFailure("لا يمكن حذف جذر الشجرة");
        }
        String name = segments.get(segments.size() - 1);
        String parentPath = String.join("/",
                segments.subList(0, segments.size() - 1));
        Uri doc = safResolveDocument(g, path);
        SafInfo info = safDocInfo(doc);
        if (safIsDir(info)) {
            JSONArray children = safListChildren(g, doc);
            if (children.length() > 0) {
                throw new SafFailure("المجلد غير فارغ — احذف محتوياته أولا");
            }
        }
        boolean deleted;
        try {
            deleted = DocumentsContract.deleteDocument(
                    getContentResolver(), doc);
        } catch (Exception e) {
            throw new SafFailure("تعذّر حذف الملف");
        }
        if (!deleted) {
            throw new SafFailure("تعذّر حذف الملف");
        }
        // التحقق بعد الحذف: ما زال موجودا = فشل.
        Uri parent = safResolveDocument(g, parentPath);
        String parentId = DocumentsContract.getDocumentId(parent);
        if (safChildUri(g.uri, parentId, name) != null) {
            throw new SafFailure("تعذّر حذف الملف — ما زال موجودا");
        }
    }

    // ----- المؤقت وتشغيل ملف .نيفا (CATALOG-19 §8) -----

    /** مجلد المؤقت (getFilesDir()/tmp — لا getCacheDir). */
    private File tempDir() {
        File tmp = new File(getFilesDir(), TEMP_DIR_NAME);
        if (!tmp.isDirectory()) tmp.mkdirs();
        return tmp;
    }

    /** ملاحظة داخل جلسة (سطر + موجّه جديد — نمط
     * فشل بدء المحرك). */
    private void noteInSession(Session s, String msg) {
        String full = msg + "\n" + s.lastPrompt;
        appendSessionText(s, full);
        if (s == active()) {
            appendToTerminal(full);
            s.inputStart = s.text.length();
            terminalView.inputStart = s.inputStart;
            paintPrompt();
            scrollToBottomIfAuto();
        } else {
            s.inputStart = s.text.length();
        }
    }

    /** فشل تشغيل: تنبيه + سطر في الجلسة الحالية
     * (CATALOG-19 §8) — بلا جلسة جديدة وبلا نسخ. */
    private void noteSafFailure(String msg) {
        toast(msg);
        if (!sessions.isEmpty()) noteInSession(active(), msg);
    }

    /** اسم مستند للعرض (DISPLAY_NAME أو آخر مقطع). */
    private String safDocumentDisplayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri,
                new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},
                null, null, null)) {
            if (c != null && c.moveToFirst()) {
                String name = c.getString(0);
                if (name != null && !name.isEmpty()) return name;
            }
        } catch (Exception ignored) {
        }
        return uri.getLastPathSegment();
    }

    /** هل المستند مجلد (MIME_TYPE_DIR)؟ */
    private boolean safDocumentIsDir(Uri uri) {
        try (Cursor c = getContentResolver().query(uri,
                new String[]{DocumentsContract.Document.COLUMN_MIME_TYPE},
                null, null, null)) {
            if (c != null && c.moveToFirst()) {
                return DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(0));
            }
        } catch (Exception ignored) {
        }
        return false;
    }

    /** ملف مؤقت فريد: تعارض الاسم → لاحقة رقمية
     * (بلا كتابة فوق الأصل أبدا). */
    private File safUniqueTempFile(File dir, String name) {
        File dest = new File(dir, name);
        if (!dest.exists()) return dest;
        int dot = name.lastIndexOf('.');
        String base = dot >= 0 ? name.substring(0, dot) : name;
        String ext = dot >= 0 ? name.substring(dot) : "";
        for (int i = 2; i < 1000; i++) {
            File cand = new File(dir, base + "-" + arabicDigits(i) + ext);
            if (!cand.exists()) return cand;
        }
        return null;
    }

    /** فتح منتقي تشغيل ملف .نيفا (StorageGet). */
    private void launchRunPicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("text/plain");
        try {
            startActivityForResult(intent, REQ_SAF_RUN);
        } catch (Exception e) {
            toast("تعذّر فتح منتقي الملفات");
        }
    }

    /** تشغيل ملف .نيفا مختار/مستقبَل (CATALOG-19 §8):
     * تحقق قبل التشغيل، نسخ إلى المؤقت، جلسة جديدة
     * مجلدها المؤقت — والأصل لا يُمس أبدا. */
    private void runChosenNivaFile(Uri uri) {
        String name = safDocumentDisplayName(uri);
        if (name == null || name.isEmpty()) {
            noteSafFailure("تعذّر تشغيل الملف: تعذّر قراءة اسم الملف");
            return;
        }
        if (!name.endsWith(".نيفا")) {
            noteSafFailure("تعذّر تشغيل الملف: الملف ليس ملف نيفا");
            return;
        }
        if (safDocumentIsDir(uri)) {
            noteSafFailure("تعذّر تشغيل الملف: الملف ليس ملفا");
            return;
        }
        // الجلسات قبل أي نسخ — فلا ملف يتيم عند الامتلاء.
        if (sessions.size() >= MAX_SESSIONS) {
            showSessionCapDialog();
            return;
        }
        byte[] bytes;
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null) {
                noteSafFailure("تعذّر قراءة الملف المختار");
                return;
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            int total = 0;
            while ((n = in.read(buf)) != -1) {
                total += n;
                if (total > SAF_READ_CAP) {
                    noteSafFailure("تعذّر تشغيل الملف: الملف أكبر من الحد الأقصى (5 ميغابايت)");
                    return;
                }
                out.write(buf, 0, n);
            }
            bytes = out.toByteArray();
        } catch (Exception e) {
            noteSafFailure("تعذّر قراءة الملف المختار");
            return;
        }
        try {
            StandardCharsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(bytes));
        } catch (CharacterCodingException e) {
            noteSafFailure("تعذّر تشغيل الملف: الملف ليس نصا صالحا");
            return;
        }
        File dest = safUniqueTempFile(tempDir(), name);
        if (dest == null) {
            noteSafFailure("تعذّر تشغيل الملف: تعذّر إنشاء نسخة مؤقتة");
            return;
        }
        try (FileOutputStream fos = new FileOutputStream(dest)) {
            fos.write(bytes);
        } catch (Exception e) {
            dest.delete();
            noteSafFailure("تعذّر تشغيل الملف: تعذّر نسخ الملف");
            return;
        }
        if (!createSession(name, dest, name)) {
            dest.delete();
            showSessionCapDialog();
        }
    }

    // ----- متصفح الشجرات (CATALOG-19 §10) -----

    /** قائمة «ملفات ممنوحة»: بلا منح → تنبيه؛ واحدة
     * → متصفحها؛ عدة → منتقي منح أولا. */
    private void showGrantedFilesMenu() {
        pruneGrantsAtStart();
        if (grants.isEmpty()) {
            toast("لا توجد شجرات ممنوحة — امنح مجلدا من برنامج نيفا أولا");
            return;
        }
        if (grants.size() == 1) {
            showSafBrowser(grants.get(0), "");
            return;
        }
        String[] names = new String[grants.size()];
        for (int i = 0; i < grants.size(); i++) names[i] = grants.get(i).name;
        new AlertDialog.Builder(this)
                .setTitle("اختر شجرة")
                .setItems(names, (d, which) -> showSafBrowser(grants.get(which), ""))
                .setNegativeButton("إغلاق", null)
                .show();
    }

    /** متصفح شجرة (قراءة فقط — بلا فتح وبلا تشغيل
     * من داخله في v1): النقر على مجلد يتنقل فيه،
     * وعلى ملف يبلّغ بعدم الإتاحة؛ والضغطة الطويلة
     * على ملف تحذفه بحوار تأكيد، وعلى مجلد تبلّغ
     * بعدم الإتاحة (حذف المجلدات برمجيا فقط). */
    private void showSafBrowser(final Grant g, final String path) {
        JSONArray children;
        try {
            Uri doc = safResolveDocument(g, path);
            children = safListChildren(g, doc);
        } catch (SafFailure e) {
            toast(e.getMessage());
            return;
        }
        int count = children.length();
        String[] names = new String[count];
        for (int i = 0; i < count; i++) names[i] = children.optString(i, "");
        String title = g.name + (path.isEmpty() ? "" : " / " + path);
        AlertDialog.Builder b = new AlertDialog.Builder(this)
                .setTitle(title)
                .setItems(names, (d, which) -> {
                    String child = names[which];
                    String childPath = path.isEmpty() ? child : path + "/" + child;
                    try {
                        Uri doc = safResolveDocument(g, childPath);
                        if (safIsDir(safDocInfo(doc))) {
                            showSafBrowser(g, childPath);
                        } else {
                            toast("فتح الملفات من المتصفح غير متاح في v1");
                        }
                    } catch (SafFailure e) {
                        toast(e.getMessage());
                    }
                })
                .setNegativeButton("إغلاق", null);
        if (!path.isEmpty()) {
            String parent = path.contains("/")
                    ? path.substring(0, path.lastIndexOf('/')) : "";
            b.setNeutralButton("رجوع", (d, w) -> showSafBrowser(g, parent));
        }
        AlertDialog dialog = b.show();
        ListView list = dialog.getListView();
        if (list != null) {
            list.setOnItemLongClickListener((parent, view, pos, id) -> {
                String child = names[pos];
                String childPath = path.isEmpty() ? child : path + "/" + child;
                try {
                    Uri doc = safResolveDocument(g, childPath);
                    if (safIsDir(safDocInfo(doc))) {
                        toast("حذف المجلدات من المتصفح غير متاح — احذفها من البرنامج");
                    } else {
                        showSafDeleteDialog(g, childPath, child, path);
                    }
                } catch (SafFailure e) {
                    toast(e.getMessage());
                }
                return true;
            });
        }
    }

    /** حوار تأكيد الحذف من المتصفح + تحقق بعد الحذف. */
    private void showSafDeleteDialog(final Grant g, final String childPath,
            final String name, final String parentPath) {
        new AlertDialog.Builder(this)
                .setTitle("احذف الملف؟")
                .setMessage("سيُحذف «" + name + "» نهائيا — لا تراجع.")
                .setPositiveButton("احذف", (d, w) -> {
                    try {
                        safDeleteDocument(g, childPath);
                        toast("حُذف الملف");
                        showSafBrowser(g, parentPath);
                    } catch (SafFailure e) {
                        toast(e.getMessage());
                    }
                })
                .setNegativeButton("إلغاء", null)
                .setCancelable(false)
                .show();
    }

    // ----- استقبال ملفات .نيفا (CATALOG-19 §13) -----

    /** حوار الاستقبال المعطّل (من niva.properties). */
    private void showReceiveDisabledDialog() {
        new AlertDialog.Builder(this)
                .setTitle("استقبال الملفات")
                .setMessage("استقبال الملفات معطّل — فعّله من `niva.properties`.")
                .setPositiveButton("حسنا", null)
                .show();
    }

    /** اسم التطبيق المرسل (تسمية الحزمة أو «تطبيق
     * آخر» عند الجهل) — لا يُخمن أبدا. */
    private String senderName() {
        try {
            Uri ref = getReferrer();
            if (ref != null) {
                String pkg = ref.getAuthority();
                if (pkg != null && !pkg.isEmpty()) {
                    PackageInfo info = getPackageManager().getPackageInfo(pkg, 0);
                    CharSequence label = getPackageManager()
                            .getApplicationLabel(info.applicationInfo);
                    if (label != null) return label.toString();
                    return pkg;
                }
            }
        } catch (Exception ignored) {
        }
        return "تطبيق آخر";
    }

    /** استقبال ملف .نيفا (ACTION_SEND / ACTION_VIEW):
     * لا تشغيل صامت أبدا — حوار تأكيد أولا، والتجاهل
     * لا ينسخ شيئا. */
    private void handleReceivedFile(Intent intent) {
        String action = intent.getAction();
        boolean isShare = Intent.ACTION_SEND.equals(action);
        boolean isView = Intent.ACTION_VIEW.equals(action);
        if (!isShare && !isView) return;
        if (isShare && disableReceiveShare) {
            showReceiveDisabledDialog();
            return;
        }
        if (isView && disableReceiveView) {
            showReceiveDisabledDialog();
            return;
        }
        Uri uri = isShare
                ? (Uri) intent.getParcelableExtra(Intent.EXTRA_STREAM)
                : intent.getData();
        if (uri == null) {
            // مشاركة بلا ملف (الدفعة 4أ — البندان 26 و27):
            // نص عادي → يُحفظ ملفا نصيا بعد تأكيد؛
            // ورابط وحده → يُفتح في المتصفح بعد تأكيد.
            // وبلا برامج يكتبها المستخدم أبدا — الفعل
            // بعد الاستقبال ثابت في التطبيق.
            if (!isShare) {
                toast("لم يصل ملف");
                return;
            }
            String text = intent.getStringExtra(Intent.EXTRA_TEXT);
            if (text == null || text.trim().isEmpty()) {
                toast("لم يصل ملف");
                return;
            }
            final String sharedText = text;
            if (looksLikeSingleUrl(sharedText)) {
                final String url = sharedText.trim();
                new AlertDialog.Builder(this)
                        .setTitle("افتح هذا الرابط؟")
                        .setMessage("وصل رابط «" + url + "» من "
                                + senderName() + ".")
                        .setPositiveButton("افتح", (d, w) -> {
                            Intent browser = new Intent(
                                    Intent.ACTION_VIEW, Uri.parse(url));
                            try {
                                startActivity(browser);
                            } catch (ActivityNotFoundException e) {
                                toast("لا يوجد متصفح لفتح الرابط");
                            }
                        })
                        .setNegativeButton("تجاهل", null)
                        .setCancelable(false)
                        .show();
            } else {
                // معاينة أول 200 حرف في الحوار — المستخدم
                // يرى ما سيحفظه قبل الموافقة (H2).
                String preview = sharedText.length() > 200
                        ? sharedText.substring(0, 200) + "…"
                        : sharedText;
                new AlertDialog.Builder(this)
                        .setTitle("احفظ هذا النص؟")
                        .setMessage("وصل نص من " + senderName()
                                + " — يُحفظ ملفا نصيا في مجلد البرنامج.\n\n«"
                                + preview + "»")
                        .setPositiveButton("احفظ", (d, w) ->
                                saveSharedText(sharedText))
                        .setNegativeButton("تجاهل", null)
                        .setCancelable(false)
                        .show();
            }
            return;
        }
        String name = safDocumentDisplayName(uri);
        if (name == null || name.isEmpty()) {
            String last = uri.getLastPathSegment();
            name = last != null ? last : "ملف";
        }
        if (!name.endsWith(".نيفا")) {
            new AlertDialog.Builder(this)
                    .setTitle("ملف غير مدعوم")
                    .setMessage("وصل ملف «" + name + "» — التطبيق يستقبل ملفات .نيفا فقط.")
                    .setPositiveButton("حسنا", null)
                    .show();
            return;
        }
        final String fileName = name;
        final Uri fileUri = uri;
        new AlertDialog.Builder(this)
                .setTitle("شغّل هذا الملف؟")
                .setMessage("وصل ملف نيفا «" + fileName + "» من " + senderName() + ".")
                .setPositiveButton("شغّل", (d, w) -> runChosenNivaFile(fileUri))
                .setNegativeButton("تجاهل", null)
                .setCancelable(false)
                .show();
    }

    /** هل النص رابط واحد وحده (البند 27)؟ — يبدأ بـ
     * http:// أو https:// أو magnet: وبلا فراغات
     * ولا أسطر؛ وغير ذلك نص عادي (البند 26). */
    private boolean looksLikeSingleUrl(String text) {
        String t = text.trim();
        if (t.indexOf(' ') >= 0 || t.indexOf('\n') >= 0
                || t.indexOf('\t') >= 0) {
            return false;
        }
        return t.startsWith("http://") || t.startsWith("https://")
                || t.startsWith("magnet:");
    }

    /** حفظ نص مشارك ملفا نصيا في مجلد البرنامج
     * (البند 26): الاسم تلقائي من التاريخ — بلا سؤال
     * عن اسم أولا؛ ومن أراد اسما آخر ينقله بـ`انقل`. */
    private void saveSharedText(String text) {
        String stamp = new SimpleDateFormat("yyyyMMdd-HHmmss",
                Locale.getDefault()).format(new Date());
        String fileName = "نص-" + stamp + ".txt";
        File dest = new File(getFilesDir(), fileName);
        try (FileOutputStream out = new FileOutputStream(dest)) {
            out.write(text.getBytes(StandardCharsets.UTF_8));
            toast("حُفظ النص في " + fileName);
        } catch (IOException e) {
            toast("تعذّر حفظ النص");
        }
    }

    // ===== حوار جهاز.حوار (المهمة 20) =====
    // المحرك يطلب عبر قناة التحكم (stderr) وينتظر سطر
    // نتيجة واحدا على stdin: {"قيمة": ...} أو
    // {"ألغي": true} أو {"خطأ": "..."}. كل حوار غير
    // قابل للإلغاء بزر الرجوع (setCancelable(false))
    // وله زر «إلغاء» صريح؛ وكل مسار زر يرسل نتيجة
    // واحدة بالضبط — فلا يعلق المحرك أبدا في انتظار.
    // الطلب لجلسة خلفية يُحفَظ ويُعرض عند التبديل إليها.

    /** استقبال طلب حوار من قناة التحكم. */
    private void handleDialogRequest(Session s, String payload) {
        if (isFinishing() || isDestroyed()) {
            sendDialogError(s, "التطبيق غير متاح");
            return;
        }
        if (s != active()) {
            // جلسة خلفية: يُحفَظ حتى تصير نشطة.
            s.pendingDialog = payload;
            return;
        }
        showDialogRequest(s, payload);
    }

    /** عرض طلب معلّق حين تصير جلسته نشطة. */
    private void maybeShowPendingDialog(Session s) {
        if (s.pendingDialog != null && s.dialog == null
                && s.process != null && !s.ended) {
            String payload = s.pendingDialog;
            s.pendingDialog = null;
            showDialogRequest(s, payload);
        }
    }

    /** إغلاق حوار الجلسة المفتوح ومسح المعلّق — عند
     * إعادة التشغيل أو موت العملية (المحرك ميت فلا
     * نتيجة تُرسل). */
    private void dismissSessionDialog(Session s) {
        if (s.dialog != null) {
            try {
                s.dialog.dismiss();
            } catch (Exception ignored) {
            }
            s.dialog = null;
        }
        s.pendingDialog = null;
        // وطلب SAF معلّق (المهمة 28) — كالحوار: المحرك
        // ميت فلا نتيجة تُرسَل.
        s.pendingSaf = null;
        s.safWaitPending = false;
        s.safCancelArmed = false;
    }

    /** توزيع طلب الحوار حسب النوع (v1 = 9 أنواع؛
     * «صوتي» يرفضه المحرك نفسه قبل الوصول هنا). */
    private void showDialogRequest(Session s, String payload) {
        if (s.dialog != null) {
            // حوار آخر مفتوح (لا يصل عمليا — المحرك
            // ينتظر إجابة الأول) — خطأ بدلا من التعليق.
            sendDialogError(s, "حوار آخر مفتوح");
            return;
        }
        JSONObject req;
        try {
            req = new JSONObject(payload);
        } catch (JSONException e) {
            sendDialogError(s, "طلب حوار غير صالح");
            return;
        }
        String kind = req.optString("نوع", "");
        String title = req.optString("عنوان", "");
        try {
            switch (kind) {
                case "تأكيد":
                    showConfirmDialog(s, title);
                    break;
                case "نص":
                    showTextDialog(s, title);
                    break;
                case "رقم":
                    showNumberDialog(s, title);
                    break;
                case "اختيار واحد":
                    showSingleChoiceDialog(s, title,
                            jsonStringArray(req.getJSONArray("خيارات")));
                    break;
                case "اختيارات":
                    showMultiChoiceDialog(s, title,
                            jsonStringArray(req.getJSONArray("خيارات")));
                    break;
                case "قائمة":
                    showListDialog(s, title,
                            jsonStringArray(req.getJSONArray("خيارات")));
                    break;
                case "عداد":
                    showCounterDialog(s, title, req.getLong("أدنى"),
                            req.getLong("أقصى"),
                            req.optLong("افتراضي", req.getLong("أدنى")));
                    break;
                case "تاريخ":
                    showDateDialog(s, title);
                    break;
                case "وقت":
                    showTimeDialog(s, title);
                    break;
                default:
                    sendDialogError(s, "نوع حوار غير معروف: " + kind);
                    break;
            }
        } catch (JSONException e) {
            sendDialogError(s, "طلب حوار غير صالح");
        } catch (IllegalArgumentException e) {
            sendDialogError(s, "نطاق الحوار غير صالح");
        }
    }

    /** تسجيل الحوار المفتوح في الجلسة وعرضه. */
    private void trackDialog(Session s, AlertDialog d) {
        s.dialog = d;
        d.setOnDismissListener(dlg -> {
            if (s.dialog == dlg) s.dialog = null;
        });
        d.show();
    }

    /** إرسال نتيجة حوار واحدة إلى المحرك عبر طابور
     * الإدخال المحدود (ليست أمر جلب — تمر مباشرة). */
    private void sendDialogResult(Session s, JSONObject result) {
        sendLineToEngine(s, DIALOG_RESULT_PREFIX + result.toString());
    }

    private void sendDialogValue(Session s, Object value) {
        try {
            JSONObject r = new JSONObject();
            r.put("قيمة", value);
            sendDialogResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendDialogCancel(Session s) {
        try {
            JSONObject r = new JSONObject();
            r.put("ألغي", true);
            sendDialogResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private void sendDialogError(Session s, String message) {
        try {
            JSONObject r = new JSONObject();
            r.put("خطأ", message);
            sendDialogResult(s, r);
        } catch (JSONException ignored) {
        }
    }

    private static String[] jsonStringArray(JSONArray arr) throws JSONException {
        String[] out = new String[arr.length()];
        for (int i = 0; i < arr.length(); i++) out[i] = arr.getString(i);
        return out;
    }

    /** تأكيد: نعم / لا / إلغاء. */
    private void showConfirmDialog(Session s, String title) {
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setPositiveButton("نعم",
                        (dlg, w) -> sendDialogValue(s, true))
                .setNegativeButton("لا",
                        (dlg, w) -> sendDialogValue(s, false))
                .setNeutralButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** نص: سطر واحد. */
    private void showTextDialog(Session s, String title) {
        EditText input = new EditText(this);
        input.setSingleLine(true);
        input.setInputType(InputType.TYPE_CLASS_TEXT);
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setView(input)
                .setPositiveButton("موافق",
                        (dlg, w) -> sendDialogValue(s,
                                input.getText().toString()))
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** رقم: إدخال عشري بإشارة؛ الأرقام العربية
     * (٠-٩) والفاصلة العربية (٫) تُحوَّل قبل التحليل،
     * والفارغ أو غير الرقم خطأ عربي يصل البرنامج. */
    private void showNumberDialog(Session s, String title) {
        EditText input = new EditText(this);
        input.setSingleLine(true);
        input.setInputType(InputType.TYPE_CLASS_NUMBER
                | InputType.TYPE_NUMBER_FLAG_DECIMAL
                | InputType.TYPE_NUMBER_FLAG_SIGNED);
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setView(input)
                .setPositiveButton("موافق", (dlg, w) -> {
                    Double v = parseArabicNumber(
                            input.getText().toString());
                    if (v == null) {
                        sendDialogError(s, "أدخل رقما");
                    } else {
                        sendDialogValue(s, v);
                    }
                })
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** تحليل رقم يقبل الأرقام العربية والفاصلة
     * العربية؛ null = ليس رقما. */
    private static Double parseArabicNumber(String text) {
        if (text == null) return null;
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c >= '٠' && c <= '٩') {
                sb.append((char) ('0' + (c - '٠')));
            } else if (c == '٫') {
                sb.append('.');
            } else if (c == '٬') {
                // فاصل آلاف عربي — يُتجاهل.
            } else {
                sb.append(c);
            }
        }
        String t = sb.toString().trim();
        if (t.isEmpty()) return null;
        try {
            return Double.parseDouble(t);
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /** اختيار واحد: قائمة radio + موافق/إلغاء؛ بلا
     * اختيار عند «موافق» خطأ عربي. */
    private void showSingleChoiceDialog(Session s, String title,
            String[] options) {
        final int[] sel = {-1};
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setSingleChoiceItems(options, -1,
                        (dlg, which) -> sel[0] = which)
                .setPositiveButton("موافق", (dlg, w) -> {
                    if (sel[0] < 0) {
                        sendDialogError(s, "اختر خيارا");
                    } else {
                        sendDialogValue(s, options[sel[0]]);
                    }
                })
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** اختيارات: قائمة multi-choice؛ قد ترجع قائمة
     * فارغة (اختيار لا شيء اختيار صالح). */
    private void showMultiChoiceDialog(Session s, String title,
            String[] options) {
        final boolean[] checked = new boolean[options.length];
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setMultiChoiceItems(options, null,
                        (dlg, which, isChecked) -> checked[which] = isChecked)
                .setPositiveButton("موافق", (dlg, w) -> {
                    JSONArray arr = new JSONArray();
                    for (int i = 0; i < options.length; i++) {
                        if (checked[i]) arr.put(options[i]);
                    }
                    sendDialogValue(s, arr);
                })
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** قائمة: النقر على عنصر يرجع فورا بلا زر موافق. */
    private void showListDialog(Session s, String title,
            String[] options) {
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setItems(options,
                        (dlg, which) -> sendDialogValue(s, options[which]))
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** عدّاد: NumberPicker بلا التفاف؛ نطاق خارج int
     * خطأ عربي (المحرك يقبل أعدادا أكبر من int).
     * التحقق `أدنى <= افتراضي <= أقصى` قبل البناء
     * (H7): منتقي أندرويد يرمي إن كان الأدنى أكبر
     * من الأقصى الحالي؛ والترتيب أقصى ثم أدنى ثم
     * قيمة؛ والتقاط احتياطي حول البناء كله. */
    private void showCounterDialog(Session s, String title,
            long min, long max, long def) {
        if (min < Integer.MIN_VALUE || max > Integer.MAX_VALUE) {
            sendDialogError(s, "نطاق العداد كبير جدا");
            return;
        }
        if (min > max || def < min || def > max) {
            sendDialogError(s, "نطاق العداد غير صالح");
            return;
        }
        try {
            NumberPicker picker = new NumberPicker(this);
            picker.setMaxValue((int) max);
            picker.setMinValue((int) min);
            picker.setValue((int) def);
            picker.setWrapSelectorWheel(false);
            AlertDialog d = new AlertDialog.Builder(this)
                    .setTitle(title)
                    .setView(picker)
                    .setPositiveButton("موافق",
                            (dlg, w) -> sendDialogValue(s, picker.getValue()))
                    .setNegativeButton("إلغاء",
                            (dlg, w) -> sendDialogCancel(s))
                    .setCancelable(false)
                    .create();
            trackDialog(s, d);
        } catch (IllegalArgumentException e) {
            sendDialogError(s, "نطاق العداد غير صالح");
        }
    }

    /** تاريخ: منتقي تاريخ؛ الشهر يُرجع 1-12 (أندرويد
     * 0-11) في {سنة، شهر، يوم}. */
    private void showDateDialog(Session s, String title) {
        Calendar now = Calendar.getInstance();
        DatePicker picker = new DatePicker(this);
        picker.init(now.get(Calendar.YEAR), now.get(Calendar.MONTH),
                now.get(Calendar.DAY_OF_MONTH), null);
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setView(picker)
                .setPositiveButton("موافق", (dlg, w) -> {
                    try {
                        JSONObject r = new JSONObject();
                        r.put("سنة", picker.getYear());
                        r.put("شهر", picker.getMonth() + 1);
                        r.put("يوم", picker.getDayOfMonth());
                        sendDialogValue(s, r);
                    } catch (JSONException ignored) {
                    }
                })
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
    }

    /** وقت: منتقي وقت 24 ساعة؛ يرجع {ساعة، دقيقة}. */
    private void showTimeDialog(Session s, String title) {
        Calendar now = Calendar.getInstance();
        TimePicker picker = new TimePicker(this);
        picker.setIs24HourView(true);
        picker.setHour(now.get(Calendar.HOUR_OF_DAY));
        picker.setMinute(now.get(Calendar.MINUTE));
        AlertDialog d = new AlertDialog.Builder(this)
                .setTitle(title)
                .setView(picker)
                .setPositiveButton("موافق", (dlg, w) -> {
                    try {
                        JSONObject r = new JSONObject();
                        r.put("ساعة", picker.getHour());
                        r.put("دقيقة", picker.getMinute());
                        sendDialogValue(s, r);
                    } catch (JSONException ignored) {
                    }
                })
                .setNegativeButton("إلغاء",
                        (dlg, w) -> sendDialogCancel(s))
                .setCancelable(false)
                .create();
        trackDialog(s, d);
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
        if (s.programFile != null && !s.programFile.isFile()) {
            // جلسة برنامج ذهب ملفها المؤقت (المهمة 28):
            // لا إعادة تشغيل — خطأ عربي داخل الجلسة،
            // بلا مسار داخلي أبدا.
            String prompt = CONT_PROMPT_TEXT.equals(s.lastPrompt) ? "~ $ " : s.lastPrompt;
            s.lastPrompt = prompt;
            String msg = "تعذّر إعادة تشغيل البرنامج — الملف المؤقت غير موجود.\n" + prompt;
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
            return;
        }
        // إعادة التشغيل تقتل أي حوار مفتوح أو معلّق
        // (المهمة 20) — المحرك الجديد يبدأ بلا انتظار.
        dismissSessionDialog(s);
        // وتُسقَط تنبيهات الجلسة المعلّقة بإذن وتُمسح
        // خريطتها (المهمة 21) — والمنشور يبقى عمدا.
        dropPendingNotifies(s);
        // ويُوقَف نطق الجلسة الجاري وتُمسح معلّقاته
        // (المهمة 23) — المحرك مات فلا نتيجة تُرسَل.
        stopSpeakForSession(s);
        // ويُوقَف صوت الجلسة الجاري (المهمة 24) —
        // المحرك مات فلا نتيجة تُرسَل.
        stopAudioForSession(s);
        // وتُلغى قراءات حساس الجلسة المعلقة (المهمة 25) —
        // المحرك مات فلا نتيجة تُرسَل.
        stopSensorForSession(s);
        // ويُوقَف استماع الجلسة الجاري ويُغلق حوار
        // الإفصاح إن عُرض (المهمة 26) — المحرك مات
        // فلا نتيجة تُرسَل.
        stopListenForSession(s);
        // وتُلغى طلبات جهاز الجلسة المعلقة (الدفعة 4أ) —
        // المحرك مات فلا نتيجة تُرسَل.
        cancelDeviceRequestsForSession(s);
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
        // موت العملية يغلق أي حوار مفتوح أو معلّق
        // (المهمة 20) — فلا يبقى حوار بلا محرك.
        dismissSessionDialog(s);
        // وتُسقَط تنبيهات الجلسة المعلّقة بإذن وتُمسح
        // خريطتها (المهمة 21) — والمنشور يبقى عمدا.
        dropPendingNotifies(s);
        // ويُوقَف نطق الجلسة الجاري وتُمسح معلّقاته
        // (المهمة 23) — المحرك مات فلا نتيجة تُرسَل.
        stopSpeakForSession(s);
        // ويُوقَف صوت الجلسة الجاري (المهمة 24) —
        // المحرك مات فلا نتيجة تُرسَل.
        stopAudioForSession(s);
        // وتُلغى قراءات حساس الجلسة المعلقة (المهمة 25) —
        // المحرك مات فلا نتيجة تُرسَل.
        stopSensorForSession(s);
        // ويُوقَف استماع الجلسة الجاري ويُغلق حوار
        // الإفصاح إن عُرض (المهمة 26) — المحرك مات
        // فلا نتيجة تُرسَل.
        stopListenForSession(s);
        // وتُلغى طلبات جهاز الجلسة المعلقة (الدفعة 4أ) —
        // المحرك مات فلا نتيجة تُرسَل.
        cancelDeviceRequestsForSession(s);
        // رمز الخروج: -1 تعني «من مسار EOF، غير معروف» —
        // يُحلّ عبر exitValue إن أمكن. (صعد قبل فحص
        // «أثناء الخروج» — اجتماع 39: الإزالة الفورية
        // يجب أن تتم حتى أثناء التدمير.)
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
        // نموذج ترمكس (اجتماع 39): الخروج النظيف (0 أو
        // 130) يزيل الجلسة العادية فورا من القائمة
        // والقرص — بلا خيار وبلا رجعة؛ وجلسة البرنامج
        // مستثناة (مخرجاتها هي قيمتها).
        if ((code == 0 || code == 130) && s.programName == null) {
            int idx = sessions.indexOf(s);
            if (idx >= 0) {
                if (isFinishing() || isDestroyed()) {
                    // إزالة صامتة أثناء التدمير: قائمة +
                    // ملفات + حفظ — بلا واجهة وبلا إشعار
                    // (إعادة النشر هنا كانت تعيد تشغيل
                    // الخدمة بعد إيقافها).
                    sessions.remove(idx);
                    deleteSessionFiles(idx);
                    if (activeIdx >= sessions.size()) {
                        activeIdx = Math.max(0, sessions.size() - 1);
                    }
                    saveAllSessions();
                } else {
                    deleteSession(idx, true);
                    toast("حُذفت الجلسة المنتهية");
                }
            }
            return;
        }
        // حفظ فوري (اجتماع 37): حالة «منتهية» تُكتب
        // الآن في ملف العلامة — فلا تُبعث الجلسة حية
        // إن قُتل التطبيق قبل onPause.
        saveAllSessions();
        // أثناء الخروج/التدمير: التنظيف أعلاه يتم،
        // لكن بلا واجهة وبلا إعادة نشر إشعار — فإعادة
        // النشر هنا كانت تعيد تشغيل الخدمة بعد إيقافها
        // (خلل إعادة الفتح من درج الإشعارات).
        if (isFinishing() || isDestroyed()) return;
        // موجّه الاستكمال لا يصلح لمحرك ميت (الكتلة ذهبت
        // معه) — الموجّه الوهمي موجّه عادي.
        String prompt = CONT_PROMPT_TEXT.equals(s.lastPrompt) ? "~ $ " : s.lastPrompt;
        s.lastPrompt = prompt;
        // الدفعة 3، البند 54: صياغة موحدة لسطر نهاية
        // البرنامج — الرمز يُذكر فقط إن لم يكن صفرا.
        String endedLine = "[انتهى البرنامج"
                + (code != 0 ? " (الرمز " + arabicDigits(code) + ")" : "")
                + " — اضغط إدخال]";
        String msg = "\n" + endedLine + "\n" + prompt;
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
        // اهتزاز عند انتهاء برنامج (v1.1.6 بند 26):
        // المدة الافتراضية 1000ms.
        vibrateDefault();
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
     * معروضا (فلا يُفقد نص مكتوب)، ومن الرأس إلى حدود سطر.
     * السقف حماية للأداء (الدفعة 3، البند 73): سجل بلا
     * حد يبطئ العرض والحفظ مع طول الجلسة. */
    private void trimTranscript(Session s) {
        if (s.inputStart >= 0) return;
        while ((s.lineCount > maxTranscriptLines || s.text.length() > MAX_CHARS) && s.lineCount > 100) {
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
        int chunkStart = terminalView.getText().length();
        terminalView.getText().append(chunk);
        colorOutputLines(chunkStart, true);
        terminalView.programmatic = false;
    }

    /** لون سطور المخرجات المكتملة بعد إلحاق قطعة (أمر المالك
     * 2026-11-08): سطور الأخطاء بالأحمر وسطور نجاح التثبيت
     * بالأخضر. القطع تصل مجزأة: يُفحص من بداية السطر الذي
     * بدأت فيه القطعة (بالرجوع إلى \n السابقة)، ولا يُلوَّن
     * سطر ناقص (بلا \n في نهايته) حتى يكتمل بقطعة لاحقة.
     * كل سطر يُلوَّن مرة واحدة فقط: القطعة التالية تبدأ
     * فحصها من سطر لم يُلوَّن بعد. التلوين في terminalView
     * فقط؛ ونص الجلسة المخزّن يبقى نصا عاديا بلا spans.
     * والجرس (الدفعة 3، البند 57) يرنّ فقط على القطع
     * الجديدة (ringBell) لا على إعادة التلوين. */
    private void colorOutputLines(int chunkStart, boolean ringBell) {
        Editable text = terminalView.getText();
        int len = text.length();
        if (len == 0) return;
        int start = chunkStart;
        if (start > len) start = len;
        while (start > 0 && text.charAt(start - 1) != '\n') start--;
        int pos = start;
        while (pos < len) {
            int eol = -1;
            for (int i = pos; i < len; i++) {
                if (text.charAt(i) == '\n') { eol = i; break; }
            }
            if (eol < 0) break;
            int color = lineColor(text, pos, eol);
            if (color != 0) {
                text.setSpan(new ForegroundColorSpan(color), pos, eol,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                if (ringBell && color == errorColor) bellOnErrorLine();
            }
            pos = eol + 1;
        }
    }

    /** لون سطر واحد حسب محتواه: أحمر للخطأ، أخضر لنجاح
     * التثبيت، وسماوي لسطر فيه رابط (الدفعة 3، البند 21)،
     * وألوان عرض الملفات (`ات`): سطر ينتهي بـ`/`
     * فهو مجلد (أزرق)، وبـ`*` فهو تنفيذي (أخضر)، وبـ`@`
     * فهو رابط (سماوي)؛ و0 بلا لون. المقارنة على النص دون
     * \n النهائية. سطر الموجّه («~ $ ...») لا يُلوَّن أبدا:
     * موجّهه الأخضر من paintPrompt، فيبقى كما هو. لون
     * الخطأ والنجاح من niva.properties (الدفعة 3، البند 63). */
    private int lineColor(Editable text, int start, int end) {
        String line = text.subSequence(start, end).toString();
        if (line.startsWith("~")) return 0;
        if (line.startsWith(ERROR_PREFIX)
                || line.contains(CANNOT_WITH_SHADDA)
                || line.contains(CANNOT_PLAIN)) {
            return errorColor;
        }
        for (String marker : FETCH_ERROR_MARKERS) {
            if (line.contains(marker)) return errorColor;
        }
        if (line.startsWith(INSTALL_PREFIX)) {
            return successColor != 0 ? successColor : promptColor;
        }
        if (URL_PATTERN.matcher(line).find()) return LS_LINK_CYAN;
        if (line.endsWith("/")) return LS_DIR_BLUE;
        if (line.endsWith("*")) return promptColor;
        if (line.endsWith("@")) return LS_LINK_CYAN;
        return 0;
    }

    /** إعادة تلوين النص الكامل (عند استعادة الجلسة): setText
     * يمحو كل الـspans، فيُعاد التلوين على السطور المكتملة. */
    private void colorAllOutputLines() {
        colorOutputLines(0, false);
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
        // حارس السجل (v1.1.5): إن كانت بداية سطر
        // الإدخال قديمة (المنطقة منها إلى نهاية النص
        // تحوي سطرا جديدا غير أخير) فالاستبدال حينها
        // يأكل ردودا — لا يُستبدل شيء، وتُعاد البداية
        // إلى نهاية النص (شفاء ذاتي، بلا حذف).
        String region = text.toString().substring(safe);
        String probe = region.endsWith("\n")
                ? region.substring(0, region.length() - 1) : region;
        if (probe.contains("\n")) {
            terminalView.inputStart = text.length();
            if (!sessions.isEmpty()) active().inputStart = text.length();
            terminalView.programmatic = false;
            return;
        }
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
        // نموذج ترمكس (اجتماع 39): Enter على سطر فارغ
        // في جلسة منتهية يصرفها نهائيا من القائمة
        // والقرص؛ والسطر غير الفارغ يبقي سلوك الإحياء
        // الحالي (اع / إعادة التشغيل).
        if (s.ended && line.trim().isEmpty()) {
            int idx = sessions.indexOf(s);
            if (idx >= 0) deleteSession(idx, true);
            return;
        }
        // سقف سطر الإدخال (م-10).
        if (line.length() > MAX_LINE_CHARS) {
            appLine(s, "السطر أطول من الحد الأقصى (64 كيلوبايت)");
            return;
        }
        // نبضة تنفيذ خفيفة (v1.1.6 بند 26).
        vibrate(60);
        dispatchLine(s, line, true);
    }

    /** تطبيع عربي خفيف لمطابقة الأوامر (v1.1.5 —
     * تقوية دفاعية): تجريد التشكيل والتطويل وتوحيد
     * الهمزات، كما يفعل محرك نيفا للكلمات المفتاحية.
     * الحكم على الاعتراض: (أ) — المالك كان على 1.1.3
     * حين وصلت «تحديث نيفا» إلى المحرك؛ وهذا التطبيع
     * يمنع فقط فشل مطابقة مستقبليا (تشكيل عارض من
     * لوحة المفاتيح مثلا)، وليس إصلاحا لثغرة قائمة. */
    private static String normalizeArabicForMatch(String s) {
        if (s == null) return "";
        StringBuilder out = new StringBuilder(s.length());
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c >= '\u064B' && c <= '\u0652') continue;
            if (c == '\u0670') continue;
            if (c == '\u0640') continue;
            switch (c) {
                case '\u0623': case '\u0625': case '\u0622': case '\u0671':
                    out.append('\u0627');
                    break;
                case '\u0624':
                    out.append('\u0648');
                    break;
                case '\u0626':
                    out.append('\u064A');
                    break;
                case '\u0629':
                    out.append('\u0647');
                    break;
                case '\u0649':
                    out.append('\u064A');
                    break;
                default:
                    out.append(c);
            }
        }
        return out.toString();
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
        // المطابقة بعد التطبيع (v1.1.5) فلا يكسرها
        // تشكيل عارض من لوحة المفاتيح.
        String trimmed = line.trim();
        String norm = normalizeArabicForMatch(trimmed);
        String normFirst = norm.split("\\s+")[0];
        // العبارات الكاملة للتطبيق (2026-11-08): قبل
        // أكواد التطبيق — مطابقة السطر كله بعد التطبيع.
        String appPhrase = appPhraseFor(norm);
        if (appPhrase != null) {
            handleAppPhrase(s, appPhrase, norm);
            return;
        }
        if (isAppCode(normFirst)) {
            handleAppShortcut(s, normFirst, norm);
            return;
        }
        // عبارة التحديث الذاتي (v1.1.4): لا تصل إلى
        // المحرك أبدا — فحص إصدارات جيت هاب من التطبيق.
        if (normalizeArabicForMatch(UPDATE_PHRASE).equals(norm)) {
            runSelfUpdate(s);
            return;
        }
        // مسح محفوظات الأوامر دون مساس بالمتغيرات
        // (v1.1.6 بند 4): يمسح محفوظات الأسهم في
        // التطبيق. أما سطور :اعرض في المحرك فلا يملك
        // التطبيق أمرا لمسحها وحدها (:مسح يعيد ضبط
        // المفسر كله) — فمسحها يحتاج أمر محرك مستقبلا
        // (موثق في التقرير كمتابعة).
        if (":امسح-المحفوظات".equals(norm)) {
            s.history.clear();
            s.histIdx = -1;
            s.draft = "";
            appLine(s, "تم مسح محفوظات الأسهم");
            toast("تم مسح محفوظات الأسهم");
            return;
        }
        // مسح بالاسم الكامل (مهمة 12): المحرك لا يعرف
        // إلا `:مسح`؛ أما `مسح` عارية فيرد عليها
        // «متغير غير معرف» (مُتحقق حيا). تُمرَّر كـ`:مسح`
        // فيعيد ضبط الجلسة كلها. بلا تعديل
        // في المحرك: التوجيه هنا في التطبيق فقط.
        if ("مسح".equals(norm) && "مسح".equals(normFirst)) {
            proceedLineToEngine(s, ":مسح");
            return;
        }
        // تأكيد قبل حذف ملف/مجلد (v1.1.6 بند 5): حف
        // وحم بلا تأكيد اليوم.
        if ("حف".equals(normFirst) || "حم".equals(normFirst)) {
            confirmDeleteFromTerminal(s, line, trimmed);
            return;
        }
        proceedLineToEngine(s, line);
    }

    /** إرسال السطر إلى المحرك مع معالجة توقفه
     * (مستخرجة من dispatchLine ليُعاد استخدامها بعد
     * حوار تأكيد الحذف — بند 5). */
    private void proceedLineToEngine(Session s, String line) {
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

    /** حوار تأكيد قبل حذف ملف/مجلد من الترمنال
     * (v1.1.6 بند 5): يسمّي الهدف؛ والمجلد غير الفارغ
     * يُعلن أنه سيُحذف كل ما فيه. الهدف غير الموجود
     * يُرسَل للمحرك مباشرة ليبلّغ هو بالخطأ. */
    private void confirmDeleteFromTerminal(Session s, String line, String trimmed) {
        String[] parts = trimmed.split("\\s+");
        if (parts.length < 2) {
            proceedLineToEngine(s, line);
            return;
        }
        String target = parts[1];
        File f = resolveEnginePath(target);
        if (f == null || !f.exists()) {
            proceedLineToEngine(s, line);
            return;
        }
        String msg = "احذف «" + target + "»؟";
        if (f.isDirectory()) {
            String[] kids = f.list();
            if (kids != null && kids.length > 0) {
                msg += "\nالمجلد غير فارغ — سيُحذف كل ما فيه.";
            }
        }
        new AlertDialog.Builder(this)
                .setMessage(msg)
                .setPositiveButton("حذف", (d, w) -> proceedLineToEngine(s, line))
                .setNegativeButton("إلغاء", (d, w) -> appLine(s, "أُلغي الحذف"))
                .show();
    }

    /** حلّ مسار من سطر اختصار (~/… أو نسبي) داخل
     * مجلد ملفات المحرك (مجلد برنامج المحرك نفسه). */
    private File resolveEnginePath(String target) {
        if (target == null || target.isEmpty()) return null;
        String t = target;
        if (t.equals("~")) return getFilesDir();
        if (t.startsWith("~/")) t = t.substring(2);
        else if (t.startsWith("/")) return new File(t);
        return new File(getFilesDir(), t);
    }

    /** إرسال سطر إلى المحرك عبر الطابور المحدود (بند 15)،
     * مع فحص الشبكة وقفل الاستيقاظ للجلب (بند 11 + 12). */
    private void sendLineToEngine(Session s, String line) {
        if (!"\u0001نيفا-إلغاء".equals(line)) {
            // أي سطر غير الإلغاء يضمن استهلاك سطر إلغاء
            // سابق (المهمة 28): حلقة الإدخال تتخطاه،
            // وانتظار التحكم يتخطاه أو يجهض عليه —
            // فيُمسح علم الإلغاء المسلّح ولا تُسقَط
            // نتيجة SAF لاحقة ظلما.
            s.safCancelArmed = false;
        }
        if (isFetchCommand(line.trim())) {
            if (!isOnline()) {
                // دون اتصال: لا إرسال ولا انتظار مهلة DNS —
                // رسالة واضحة وموجّه جديد فورا.
                vibrate(200);
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

    // ---------- اختصارات التطبيق (3 من الـ37) ----------

    private static boolean isAppCode(String word) {
        for (String code : APP_CODES) {
            if (code.equals(word)) return true;
        }
        return false;
    }

    /** العبارات الكاملة للتطبيق (2026-11-08، أمر المالك
     * «اكتب دول كاملين»): بدلا من أكوادها المحذوفة
     * (مش/شب/سش) — مطابقة السطر كله بعد التطبيع
     * (فالتشكيل العارض من لوحة المفاتيح لا يكسرها،
     * كـ«تحديث نيفا»)، أو بدايته مع معاملات زائدة
     * فيرد خطأ «بدون معاملات» بدلا من الوصول إلى المحرك.
     * «امسح الجلسة» ليست هنا: محرك لا تطبيق — التطبيق
     * يمررها إلى المحرك كما هي ولا يعترضها. */
    private static String appPhraseFor(String norm) {
        String clear = normalizeArabicForMatch("امسح الشاشة");
        String state = normalizeArabicForMatch("حالة الشبكة");
        String allow = normalizeArabicForMatch("اسمح بالشبكة");
        String storage = normalizeArabicForMatch("اكشف التخزين");
        if (norm.equals(clear) || norm.startsWith(clear + " ")) return "امسح الشاشة";
        if (norm.equals(state) || norm.startsWith(state + " ")) return "حالة الشبكة";
        if (norm.equals(allow) || norm.startsWith(allow + " ")) return "اسمح بالشبكة";
        if (norm.equals(storage) || norm.startsWith(storage + " ")) return "اكشف التخزين";
        return null;
    }

    /** تنفيذ عبارة تطبيق كاملة — نفس سلوك كودها المحذوف
     * تماما (مش/شب/سش): بلا معاملات، وإلا خطأ قصير
     * بنفس أسلوب handleAppShortcut. */
    private void handleAppPhrase(Session s, String phrase, String norm) {
        if (!normalizeArabicForMatch(phrase).equals(norm)) {
            appLine(s, "خطأ: " + phrase + " بدون معاملات");
            return;
        }
        switch (phrase) {
            case "امسح الشاشة":
                // مسح الشاشة فقط (مثل clear في أي ترمنال):
                // السجل يختفي من العرض، لكن محفوظات المحرك
                // (:اعرض) والمتغيرات تبقى كما هي — فـ
                // «امسح الشاشة» ليست «امسح الجلسة».
                // موثّق في SHORTCUTS-31.md (م-11).
                clearDisplayOnly(s);
                break;
            case "حالة الشبكة":
                appLine(s, "الشبكة: " + (s.networkAllowed ? "مفتوحة" : "مغلقة"));
                break;
            case "اسمح بالشبكة":
                // السماح بالشبكة بلا إعادة تشغيل (م-2): أمر
                // تحكم داخل العملية نفسها، فتبقى كل متغيرات
                // الجلسة. الموجّه التالي من المحرك هو الرد.
                // والشبكة تلقائية أصلا (أمر المالك
                // 2026-11-06) فهذا تأكيد غير ضار فقط.
                if (s.networkAllowed) {
                    appLine(s, "الشبكة: مفتوحة بالفعل");
                } else {
                    s.networkAllowed = true;
                    updateNetState();
                    sendToEngine(s, ALLOW_NETWORK_COMMAND);
                }
                break;
            case "اكشف التخزين":
                // الدفعة 5 (البندان 94 و96): كشف مجلد
                // التنزيلات للقراءة فقط بلا إعادة تشغيل —
                // أمر تحكم داخل العملية نفسها، فتبقى كل
                // متغيرات الجلسة. والإذن يُطلب عند أول
                // استعمال فقط (من شاشة الإعدادات — لا
                // حوار إذن عادي له)، لا عند الفتح ولا
                // عند بدء جلسة.
                if (storageManagerGranted()) {
                    sendToEngine(s, SET_DOWNLOADS_PREFIX + downloadsPath());
                    appLine(s, "التخزين مكشوف: تخزين/تنزيلات");
                } else if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
                    appLine(s, "التخزين المشترك غير مدعوم على هذا الإصدار");
                } else if (isFinishing() || isDestroyed()) {
                    appLine(s, "التطبيق غير متاح");
                } else {
                    pendingStorageSession = s;
                    Intent intent = new Intent(
                            Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION);
                    try {
                        startActivityForResult(intent, REQ_STORAGE_SETTINGS);
                    } catch (ActivityNotFoundException e) {
                        pendingStorageSession = null;
                        appLine(s, "تعذّر فتح إعدادات التخزين");
                    }
                }
                break;
            default:
                break;
        }
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
            case "تنظيف":
                // أمر التنظيف بالاسم الكامل (مهمة 12 — أمر
                // المالك): نفس «امسح الشاشة» تماما — مسح الشاشة
                // فقط، والمحرك ومحفوظات الأسهم و:اعرض تبقى.
                // سلوك Termux في clear (CSI 2J) يمسح
                // الشاشة الظاهرة ويُبقي السجل؛ أما عندنا
                // فالشاشة والسجل حقل نص واحد، فمسح العرض
                // هو مسح السجل المعروض — والفرق موثّق في
                // التقرير. `مس` هي التي تعيد ضبط المحرك.
                clearDisplayOnly(s);
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

    /** مسح العرض فقط («امسح الشاشة») — مستخرجة من handleAppShortcut
     * ليُعاد استخدامها في «امسح كل شيء» (v1.1.6 بند 3).
     * إصلاح عطل مسح v1.1.2 (بند 0): لا setText هنا أبدا
     * — استبدال النص كله تحت لوحة مفاتيح نشطة يتركها
     * تطلب تحديدا بموضع قديم فيقتل التطبيق؛ clear() على
     * النص نفسه مع مؤشر مصفَّر صراحة هو الآمن. */
    private void clearDisplayOnly(Session s) {
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
        colorAllOutputLines();
        scrollToBottomIfAuto();
    }

    /** حوار تأكيد «امسح كل شيء» (v1.1.6 بند 3): يسمّي
     * الجلسة، ويعلن أن العرض وملف السجل المحفوظ
     * سيُمسحان معا — مقابل «امسح الشاشة» الذي يمسح العرض فقط.
     * المتغيرات لا تُمسّ (لا إعادة ضبط للمفسر). */
    private void showClearAllDialog() {
        Session s = active();
        String name = s.customName != null ? s.customName
                : "جلسة " + arabicDigits(activeIdx + 1);
        new AlertDialog.Builder(this)
                .setMessage("امسح كل شيء في «" + name + "»؟\nسيُمسح العرض وملف السجل المحفوظ معا.")
                .setPositiveButton("امسح كل شيء", (d, w) -> clearAllSession(s))
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** «امسح كل شيء» (v1.1.6 بند 3): مسح العرض
     * كـ«امسح الشاشة» ثم حذف ملف السجل المحفوظ؛ والحفظ التالي
     * يعيد كتابته فارغا من النص الجديد (الموجّه فقط). */
    private void clearAllSession(Session s) {
        int idx = sessions.indexOf(s);
        if (idx < 0) return;
        clearDisplayOnly(s);
        new File(sessionsDir(), "session-" + idx + ".txt").delete();
        saveAllSessions();
        toast("تم مسح كل شيء");
    }

    /** حفظ نص الجلسة ملفا داخل ~ (v1.1.6 بند 8): ملف
     * نصي في مجلد ملفات التطبيق نفسه (لا إذن تخزين)؛
     * الاسم يُقترح من تاريخ بدء الجلسة. */
    private void saveSessionTextToFile() {
        Session s = active();
        SimpleDateFormat fmt = new SimpleDateFormat("yyyy-MM-dd_HH-mm", Locale.US);
        String name = "جلسة-" + fmt.format(new Date(s.startTime)) + ".txt";
        try {
            writeFile(new File(getFilesDir(), name), s.text.toString());
            toast("تم الحفظ: " + name);
        } catch (IOException e) {
            toast("تعذّر الحفظ");
        }
    }

    // ---------- الشبكة ----------

    // (حوار إذن الشبكة أُزيل 2026-11-06: الشبكة تلقائية
    // بأمر المالك — كما في Termux، بلا سؤال إذن أبدا.
    // انظر handleControl: إشارة «شبكة» تُثبَّت تلقائيا.)

    private void updateNetState() {
        Session s = active();
        netStateView.setText("الشبكة: " + (s.networkAllowed ? "مفتوحة" : "مغلقة"));
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

    /** سطر النتيجة الأخير لفشل تحديث (الدفعة 5 —
     * البند 107): سطر الترمنال نفسه، زائد تقرير
     * عطل صامت — فقط حين يكون التطبيق في الخلفية
     * والإذن ممنوحا؛ فالفشل لا يضيع بصمت إن فات
     * المستخدم سطر الترمنال. */
    private void finishUpdateFailed(Session s, String msg) {
        finishUpdate(s, msg);
        if (!appVisible && notifyPermissionGranted()) {
            postCrashNotification(msg);
        }
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
                finishUpdateFailed(s, "تعذّر التحقق من التحديث (خطأ " + code + ")");
                return;
            }
            String body = readAllCapped(conn.getInputStream(), 1024 * 1024);
            String tag = jsonString(body, "tag_name");
            int[] remote = tag == null ? null : parseVersion(tag);
            if (remote == null) {
                finishUpdateFailed(s, "تعذّر قراءة إصدار التحديث");
                return;
            }
            int[] local = parseVersion(localVersionName());
            if (local == null) {
                finishUpdateFailed(s, "تعذّر قراءة إصدار التحديث");
                return;
            }
            if (compareVersion(remote, local) <= 0) {
                finishUpdate(s, "لا يوجد تحديث جديد");
                return;
            }
            String assetUrl = findApkAsset(body);
            if (assetUrl == null) {
                finishUpdateFailed(s, "لا يوجد ملف تثبيت في الإصدار");
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
            finishUpdateFailed(s, "تعذّر التحقق من التحديث");
        } finally {
            if (conn != null) conn.disconnect();
        }
    }

    /** تنزيل ملف التثبيت إلى ملف تجهيز خاص (خيط
     * عامل): تقدّم كل 25٪، ثم تحقق وتبديل، ثم فتح
     * المثبّت. التثبيت نفسه يؤكده المستخدم على شاشة
     * النظام — لا تثبيت صامت أبدا. (53ب: التجهيز
     * والتحقق والتبديل — فتنزيل فاشل لا يمس ملف
     * تثبيت صالحا منزَّلا من قبل أبدا.) */
    private void downloadUpdate(Session s, String url, String tag) {
        postUpdateLine(s, "جارٍ تنزيل التحديث " + tag + "…");
        HttpURLConnection conn = null;
        File staging = new File(getCacheDir(), UPDATE_APK_NAME + UPDATE_STAGING_SUFFIX);
        File out = new File(getCacheDir(), UPDATE_APK_NAME);
        try {
            conn = (HttpURLConnection) new URL(url).openConnection();
            conn.setRequestProperty("User-Agent", "NivaTerminal");
            conn.setRequestProperty("Accept", "application/octet-stream");
            conn.setConnectTimeout(15000);
            conn.setReadTimeout(60000);
            int code = conn.getResponseCode();
            if (code != 200) {
                finishUpdateFailed(s, "تعذّر تنزيل التحديث (خطأ " + code + ")");
                return;
            }
            long total = conn.getContentLengthLong();
            if (total > MAX_UPDATE_BYTES) {
                finishUpdateFailed(s, "حجم التحديث أكبر من المتوقع");
                return;
            }
            int nextMilestone = 25;
            try (InputStream in = new BufferedInputStream(conn.getInputStream());
                 OutputStream os = new FileOutputStream(staging)) {
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
            if (staging.length() == 0 || (total > 0 && staging.length() != total)
                    || !isApkFile(staging)) {
                staging.delete();
                finishUpdateFailed(s, "تعذّر تنزيل التحديث");
                return;
            }
            // التبديل: إعادة التسمية في المجلد نفسه تحل
            // محل النهائي ذريا — والفشل هنا يبقي القديم
            // الصالح كما هو.
            if (!staging.renameTo(out)) {
                staging.delete();
                finishUpdateFailed(s, "تعذّر تنزيل التحديث");
                return;
            }
            launchInstaller(s, out);
        } catch (Exception e) {
            staging.delete();
            finishUpdateFailed(s, "تعذّر تنزيل التحديث");
        } finally {
            if (conn != null) conn.disconnect();
        }
    }

    /** تحقق سريع من ملف التثبيت: أول 4 بايتات سحر ZIP
     * (PK\x03\x04) — فملف مقطوع أو صفحة خطأ لا يمر. */
    private static boolean isApkFile(File f) {
        try (FileInputStream in = new FileInputStream(f)) {
            byte[] magic = new byte[4];
            if (in.read(magic) != 4) return false;
            return magic[0] == 'P' && magic[1] == 'K' && magic[2] == 3 && magic[3] == 4;
        } catch (IOException e) {
            return false;
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
        // إيقاف محرك النطق مع التطبيق (المهمة 23).
        if (tts != null) {
            try {
                tts.stop();
                tts.shutdown();
            } catch (Exception ignored) {
            }
            tts = null;
        }
        // إيقاف الصوت الجاري مع التطبيق (المهمة 24).
        releaseAudio();
        // إلغاء قراءات الحساس المعلقة مع التطبيق (المهمة 25).
        stopAllSensors();
        // إيقاف أي استماع معلق مع التطبيق (المهمة 26).
        stopAllListens();
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
        // المظهر (v1.1.6 بند 10): داكن/فاتح/النظام.
        lightTheme = themeMode == 1 || (themeMode == 2 && !isSystemNight());
        applyThemeFields();
        applyKeepScreenFlag();
    }

    /** هل وضع الجهاز الليلي مفعّل؟ (v1.1.6 بند 10). */
    private boolean isSystemNight() {
        int night = getResources().getConfiguration().uiMode
                & Configuration.UI_MODE_NIGHT_MASK;
        return night == Configuration.UI_MODE_NIGHT_YES;
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
        } else if (themeMode == 3) {
            // أسود خالص (v1.1.7 بند 45): خلفية ولوحة
            // #000000؛ النص والموجّه كما في الداكن.
            colorBg = BLACK_BG;
            colorText = BLACK_TEXT;
            colorPanel = BLACK_PANEL;
            colorBtn = BLACK_BTN;
            colorActive = BLACK_ACTIVE;
            promptColor = BLACK_PROMPT;
            colorSubtitle = SUBTITLE_GRAY_DARK;
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

    /** هوامش الترمنال (v1.1.6 بند 11): من niva.properties. */
    private void applyMargins() {
        if (terminalView != null) {
            terminalView.setPadding(marginH, marginV, marginH, marginV);
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
        applyCursorColor();
    }

    /** السطوع المدرك للون (الدفعة 3، البند 64): الجذر
     * التربيعي الموزون للأحمر والأخضر والأزرق. */
    private static int perceivedBrightness(int color) {
        int r = Color.red(color);
        int g = Color.green(color);
        int b = Color.blue(color);
        return (int) Math.sqrt(0.299 * r * r + 0.587 * g * g + 0.114 * b * b);
    }

    /** لون المؤشر التلقائي لخلفية (الدفعة 3، البند 64):
     * خلفية داكنة (سطوع أقل من 130) = مؤشر أبيض. */
    private static int cursorColorForBackground(int bg) {
        return perceivedBrightness(bg) < 130 ? Color.WHITE : Color.BLACK;
    }

    /** لون المؤشر (الدفعة 3، البند 64): صريح من الإعدادات
     * إن وُجد، وإلا أبيض/أسود حسب سطوع الخلفية بعتبة 130 —
     * فيبقى المؤشر مرئيا على أي سمة. */
    private void applyCursorColor() {
        if (terminalView == null) return;
        if (Build.VERSION.SDK_INT < 29) return;
        int color = cursorColor != 0 ? cursorColor : cursorColorForBackground(colorBg);
        try {
            float density = getResources().getDisplayMetrics().density;
            GradientDrawable d = new GradientDrawable();
            d.setShape(GradientDrawable.RECTANGLE);
            d.setColor(color);
            d.setSize(Math.max(1, (int) (2 * density + 0.5f)),
                    Math.max(1, terminalView.getLineHeight()));
            terminalView.setTextCursorDrawable(d);
        } catch (Exception ignored) {
        }
    }

    /** قراءة niva.properties (مفتاح=قيمة، UTF-8، مفاتيح
     * عربية): حجم_الخط، ابقاء_الشاشة، اظهار_صف_المفاتيح،
     * المظهر. أسطر # تعليقات؛ المفاتيح المجهولة تُحفظ
     * عند الكتابة ولا تُفسَّر. */
    /** مجلد الإعدادات: مجلد البرنامج/home/إعدادات
     * (اجتماع 45 §59 — ظاهر لأن الإعدادات يحررها
     * المستخدم بنفسه). */
    private File settingsDir() {
        return new File(new File(getFilesDir(), HOME_DIR_NAME), SETTINGS_DIR_NAME);
    }

    /** ملف الإعدادات الجديد؛ وإن غاب ووُجد القديم في
     * جذر مجلد البرنامج نُقل إليه (بلا كتابة فوق ملف
     * جديد موجود أبدا). النقل إعادة تسمية، وإن فشلت
     * نُسخ المحتوى ثم حُذف القديم. */
    private File nivaPropertiesFile() {
        File dir = settingsDir();
        File f = new File(dir, NIVA_PROPS_FILE);
        if (!f.isFile()) {
            File old = new File(getFilesDir(), NIVA_PROPS_FILE);
            if (old.isFile()) {
                dir.mkdirs();
                if (!old.renameTo(f)) {
                    try {
                        copyFileBytes(old, f);
                        old.delete();
                    } catch (IOException ignored) {
                    }
                }
            }
        }
        return f;
    }

    /** نسخ بايتات ملف إلى آخر (للنقل الاحتياطي). */
    private static void copyFileBytes(File from, File to) throws IOException {
        try (FileInputStream in = new FileInputStream(from);
             FileOutputStream out = new FileOutputStream(to)) {
            byte[] buf = new byte[8192];
            int n;
            while ((n = in.read(buf)) >= 0) out.write(buf, 0, n);
        }
    }

    /** مفاتيح متقاعدة: القديم → الجديد (القاعدة 75).
     * فارغة بالتصميم — لم يُعَد تسمية أي مفتاح بعد؛
     * وأول إعادة تسمية تضيف سطرا هنا. */
    private static final String[][] RETIRED_KEYS = {};

    /** تحويل المفاتيح المتقاعدة عند القراءة (القاعدة
     * 75): القديم يصير الجديد، ويُعاد كتابة الملف ذريا
     * بالمفتاح الجديد. الفشل صامت — القراءة تكمل من
     * الملف كما هو، وتُعاد المحاولة لاحقا. */
    private void migrateRetiredKeys(File f) {
        if (RETIRED_KEYS.length == 0 || !f.isFile()) return;
        try {
            List<String> lines = new ArrayList<>();
            boolean changed = false;
            try (BufferedReader br = new BufferedReader(
                    new InputStreamReader(new FileInputStream(f), StandardCharsets.UTF_8))) {
                String line;
                while ((line = br.readLine()) != null) {
                    String t = line.trim();
                    int eq = t.indexOf('=');
                    if (eq > 0) {
                        String key = t.substring(0, eq).trim();
                        for (String[] pair : RETIRED_KEYS) {
                            if (pair[0].equals(key)) {
                                line = pair[1] + line.substring(eq);
                                changed = true;
                                break;
                            }
                        }
                    }
                    lines.add(line);
                }
            }
            if (changed) {
                StringBuilder sb = new StringBuilder();
                for (String l : lines) sb.append(l).append('\n');
                writeFileAtomic(f, sb.toString());
            }
        } catch (IOException ignored) {
        }
    }

    /** الإعدادات الافتراضية (اجتماع 45 §59: البند 48)
     * — مضمّنة في التطبيق وتُنسخ إلى ~/إعدادات/ عند
     * أول تشغيل إن غاب الملف، ولا تُكتب فوق ملف موجود
     * أبدا؛ والمحرك لا ينشئها. */
    private static final String DEFAULT_NIVA_PROPERTIES =
            "# إعدادات ترمنال نيفا\n"
            + "حجم_الخط=١٦\n"
            + "ابقاء_الشاشة=لا\n"
            + "اظهار_صف_المفاتيح=نعم\n"
            + "المظهر=داكن\n"
            + "إخفاء_لوحة_المفاتيح_عند_البدء=لا\n"
            + "لوحة_المفاتيح_البرمجية_مفعلة=نعم\n"
            + "هامش_أفقي=١٦\n"
            + "هامش_عمودي=٨\n"
            + "# أسطر_السجل: سقف أسطر الجلسة المحفوظة في العرض — حماية للأداء؛ والقيم الكبيرة قد تبطئ التطبيق\n"
            + "أسطر_السجل=٢٠٠٠\n"
            + "ملء_الشاشة=لا\n"
            + "بديل_ملء_الشاشة=لا\n"
            + "خط_مخصص=\n"
            + "تنبيه_تبديل_الجلسة=نعم\n"
            + "مفاتيح_الصوت=لا شيء\n"
            + "تعطيل_استقبال_المشاركة=لا\n"
            + "تعطيل_استقبال_العرض=لا\n"
            + "حذف_المؤقت_الأقدم_من=٠\n"
            + "تعطيل_اختصارات_لوحة_المفاتيح=لا\n"
            + "لون_الخطأ=#FF5252\n"
            + "لون_النجاح=\n"
            + "لون_المؤشر=\n";

    /** نسخ الإعدادات الافتراضية عند أول تشغيل إن غاب
     * الملف (القديم والجديد معا) — بلا كتابة فوق ملف
     * موجود أبدا. */
    private void ensureDefaultProperties() {
        File f = nivaPropertiesFile();
        if (f.isFile()) return;
        try {
            f.getParentFile().mkdirs();
            writeFile(f, DEFAULT_NIVA_PROPERTIES);
        } catch (IOException ignored) {
        }
    }

    /** كتابة ذرية (البند 47): ملف مؤقت في المجلد نفسه
     * ثم إعادة تسمية — فانقطاع الكتابة لا يترك ملفا
     * نصف مكتوب أبدا. */
    private static void writeFileAtomic(File f, String content) throws IOException {
        File parent = f.getParentFile();
        if (parent != null) parent.mkdirs();
        File tmp = new File(parent, f.getName() + ".جديد");
        writeFile(tmp, content);
        if (!tmp.renameTo(f)) {
            tmp.delete();
            writeFile(f, content);
        }
    }

    private void loadNivaProperties() {
        File f = nivaPropertiesFile();
        if (!f.isFile()) return;
        migrateRetiredKeys(f);
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
                    // v1.1.6 بند 10: قيمة ثالثة «النظام»؛
                    // v1.1.7 بند 45: قيمة رابعة «أسود خالص».
                    if ("فاتح".equals(value)) themeMode = 1;
                    else if ("النظام".equals(value)) themeMode = 2;
                    else if ("أسود خالص".equals(value)) themeMode = 3;
                    else themeMode = 0;
                } else if ("إخفاء_لوحة_المفاتيح_عند_البدء".equals(key)) {
                    hideKeyboardOnStartup = "نعم".equals(value);
                } else if ("لوحة_المفاتيح_البرمجية_مفعلة".equals(key)) {
                    softKeyboardEnabled = !"لا".equals(value);
                } else if ("هامش_أفقي".equals(key)) {
                    int v = arabicToInt(value);
                    if (v >= 0 && v <= 100) marginH = v;
                } else if ("هامش_عمودي".equals(key)) {
                    int v = arabicToInt(value);
                    if (v >= 0 && v <= 100) marginV = v;
                } else if ("أسطر_السجل".equals(key)) {
                    int v = arabicToInt(value);
                    if (v >= MIN_TRANSCRIPT_LINES && v <= MAX_TRANSCRIPT_LINES) {
                        maxTranscriptLines = v;
                    }
                } else if ("ملء_الشاشة".equals(key)) {
                    // v1.1.7 بند 30.
                    fullscreen = "نعم".equals(value);
                } else if ("بديل_ملء_الشاشة".equals(key)) {
                    // v1.1.7 بند 51.
                    fullscreenWorkaroundAlt = "نعم".equals(value);
                } else if ("خط_مخصص".equals(key)) {
                    // v1.1.7 بند 32: مسار ملف الخط؛ فارغ =
                    // خط النظام.
                    customFontPath = value;
                } else if ("تنبيه_تبديل_الجلسة".equals(key)) {
                    // v1.1.7 بند 50: الافتراضي مفعّل.
                    sessionChangeToast = !"لا".equals(value);
                } else if ("مفاتيح_الصوت".equals(key)) {
                    // مهمة 12: «تحكم» تجعل خفض الصوت =
                    // تحكم (كما في Termux)؛ أي قيمة أخرى =
                    // مفاتيح صوت عادية (الافتراضي).
                    volumeKeysCtrl = "تحكم".equals(value);
                } else if ("تعطيل_استقبال_المشاركة".equals(key)) {
                    // المهمة 28: نعم = استقبال المشاركة معطّل.
                    disableReceiveShare = "نعم".equals(value);
                } else if ("تعطيل_استقبال_العرض".equals(key)) {
                    // المهمة 28: نعم = استقبال العرض معطّل.
                    disableReceiveView = "نعم".equals(value);
                } else if ("حذف_المؤقت_الأقدم_من".equals(key)) {
                    // المهمة 28: سالب = لا حذف أبدا، 0 =
                    // الكل (الافتراضي)، موجب N = الأقدم
                    // من N أيام.
                    tempAgeDays = parseTempAgeDays(value);
                } else if ("تعطيل_اختصارات_لوحة_المفاتيح".equals(key)) {
                    // الدفعة 3، البند 74: نعم = كل اختصارات
                    // لوحة المفاتيح المادية معطّلة.
                    disableHardwareShortcuts = "نعم".equals(value);
                } else if ("لون_الخطأ".equals(key)) {
                    // الدفعة 3، البند 63: سداسي عشري؛
                    // فارغ أو فاسد = اللون الحالي.
                    int c = parseHexColor(value);
                    if (c != 0) errorColor = c;
                } else if ("لون_النجاح".equals(key)) {
                    // الدفعة 3، البند 63: فارغ = لون
                    // الموجّه (حسب المظهر).
                    successColor = parseHexColor(value);
                } else if ("لون_المؤشر".equals(key)) {
                    // الدفعة 3، البند 64: فارغ = تلقائي
                    // من سطوع الخلفية.
                    cursorColor = parseHexColor(value);
                }
            }
        } catch (IOException ignored) {
        }
    }

    /** تحليل لون سداسي عشري `#RRGGBB` (الدفعة 3، البند 63):
     * 0 = فارغ أو فاسد — فيسقط على القيمة الحالية. */
    private static int parseHexColor(String value) {
        if (value == null) return 0;
        String v = value.trim();
        if (v.startsWith("#")) v = v.substring(1);
        if (v.length() != 6) return 0;
        try {
            int rgb = (int) Long.parseLong(v, 16);
            return 0xFF000000 | rgb;
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    /** كتابة لون سداسيا عشريا `#RRGGBB` (الدفعة 3، البند 63). */
    private static String hexColorText(int color) {
        return String.format("#%06X", color & 0xFFFFFF);
    }

    /** كتابة niva.properties: تُحفَظ الأسطر المجهولة
     * والتعليقات كما هي، وتُحدَّث المفاتيح الأربعة. */
    private void saveNivaProperties() {
        File f = nivaPropertiesFile();
        List<String> lines = new ArrayList<>();
        boolean hasSize = false, hasKeep = false, hasRow = false, hasTheme = false;
        boolean hasHideKb = false, hasSoftKb = false;
        boolean hasMarginH = false, hasMarginV = false;
        boolean hasRows = false;
        // v1.1.7: ملء الشاشة + بديله + الخط المخصص +
        // تنبيه تبديل الجلسة.
        boolean hasFull = false, hasFullAlt = false;
        boolean hasFont = false, hasSessToast = false;
        // مهمة 12: مفاتيح الصوت كتحكم.
        boolean hasVolKeys = false;
        // المهمة 28: استقبال الملفات + عمر المؤقت.
        boolean hasRecvShare = false, hasRecvView = false, hasTempAge = false;
        // الدفعة 3: تعطيل اختصارات لوحة المفاتيح (بند 74)
        // + ألوان الخطأ والنجاح والمؤشر (بندا 63 و64).
        boolean hasHwKeys = false, hasErrColor = false;
        boolean hasOkColor = false, hasCursorColor = false;
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
                        // v1.1.6 بند 10: القيمة الثالثة «النظام»؛
                        // v1.1.7 بند 45: القيمة الرابعة «أسود خالص».
                        String mode = themeMode == 1 ? "فاتح"
                                : themeMode == 2 ? "النظام"
                                : themeMode == 3 ? "أسود خالص" : "داكن";
                        lines.add("المظهر=" + mode);
                        hasTheme = true;
                    } else if ("إخفاء_لوحة_المفاتيح_عند_البدء".equals(key)) {
                        lines.add("إخفاء_لوحة_المفاتيح_عند_البدء=" + (hideKeyboardOnStartup ? "نعم" : "لا"));
                        hasHideKb = true;
                    } else if ("لوحة_المفاتيح_البرمجية_مفعلة".equals(key)) {
                        lines.add("لوحة_المفاتيح_البرمجية_مفعلة=" + (softKeyboardEnabled ? "نعم" : "لا"));
                        hasSoftKb = true;
                    } else if ("هامش_أفقي".equals(key)) {
                        lines.add("هامش_أفقي=" + arabicDigits(marginH));
                        hasMarginH = true;
                    } else if ("هامش_عمودي".equals(key)) {
                        lines.add("هامش_عمودي=" + arabicDigits(marginV));
                        hasMarginV = true;
                    } else if ("أسطر_السجل".equals(key)) {
                        lines.add("أسطر_السجل=" + arabicDigits(maxTranscriptLines));
                        hasRows = true;
                    } else if ("ملء_الشاشة".equals(key)) {
                        // v1.1.7 بند 30.
                        lines.add("ملء_الشاشة=" + (fullscreen ? "نعم" : "لا"));
                        hasFull = true;
                    } else if ("بديل_ملء_الشاشة".equals(key)) {
                        // v1.1.7 بند 51.
                        lines.add("بديل_ملء_الشاشة=" + (fullscreenWorkaroundAlt ? "نعم" : "لا"));
                        hasFullAlt = true;
                    } else if ("خط_مخصص".equals(key)) {
                        // v1.1.7 بند 32.
                        lines.add("خط_مخصص=" + customFontPath);
                        hasFont = true;
                    } else if ("تنبيه_تبديل_الجلسة".equals(key)) {
                        // v1.1.7 بند 50.
                        lines.add("تنبيه_تبديل_الجلسة=" + (sessionChangeToast ? "نعم" : "لا"));
                        hasSessToast = true;
                    } else if ("مفاتيح_الصوت".equals(key)) {
                        // مهمة 12.
                        lines.add("مفاتيح_الصوت=" + (volumeKeysCtrl ? "تحكم" : "لا شيء"));
                        hasVolKeys = true;
                    } else if ("تعطيل_استقبال_المشاركة".equals(key)) {
                        // المهمة 28.
                        lines.add("تعطيل_استقبال_المشاركة=" + (disableReceiveShare ? "نعم" : "لا"));
                        hasRecvShare = true;
                    } else if ("تعطيل_استقبال_العرض".equals(key)) {
                        // المهمة 28.
                        lines.add("تعطيل_استقبال_العرض=" + (disableReceiveView ? "نعم" : "لا"));
                        hasRecvView = true;
                    } else if ("حذف_المؤقت_الأقدم_من".equals(key)) {
                        // المهمة 28.
                        lines.add("حذف_المؤقت_الأقدم_من=" + tempAgeDaysText());
                        hasTempAge = true;
                    } else if ("تعطيل_اختصارات_لوحة_المفاتيح".equals(key)) {
                        // الدفعة 3، البند 74.
                        lines.add("تعطيل_اختصارات_لوحة_المفاتيح=" + (disableHardwareShortcuts ? "نعم" : "لا"));
                        hasHwKeys = true;
                    } else if ("لون_الخطأ".equals(key)) {
                        // الدفعة 3، البند 63.
                        lines.add("لون_الخطأ=" + hexColorText(errorColor));
                        hasErrColor = true;
                    } else if ("لون_النجاح".equals(key)) {
                        // الدفعة 3، البند 63: فارغ = لون الموجّه.
                        lines.add("لون_النجاح=" + (successColor == 0 ? "" : hexColorText(successColor)));
                        hasOkColor = true;
                    } else if ("لون_المؤشر".equals(key)) {
                        // الدفعة 3، البند 64: فارغ = تلقائي.
                        lines.add("لون_المؤشر=" + (cursorColor == 0 ? "" : hexColorText(cursorColor)));
                        hasCursorColor = true;
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
        if (!hasTheme) lines.add("المظهر=" + (themeMode == 1 ? "فاتح" : themeMode == 2 ? "النظام" : themeMode == 3 ? "أسود خالص" : "داكن"));
        if (!hasHideKb) lines.add("إخفاء_لوحة_المفاتيح_عند_البدء=" + (hideKeyboardOnStartup ? "نعم" : "لا"));
        if (!hasSoftKb) lines.add("لوحة_المفاتيح_البرمجية_مفعلة=" + (softKeyboardEnabled ? "نعم" : "لا"));
        if (!hasMarginH) lines.add("هامش_أفقي=" + arabicDigits(marginH));
        if (!hasMarginV) lines.add("هامش_عمودي=" + arabicDigits(marginV));
        if (!hasRows) lines.add("أسطر_السجل=" + arabicDigits(maxTranscriptLines));
        if (!hasFull) lines.add("ملء_الشاشة=" + (fullscreen ? "نعم" : "لا"));
        if (!hasFullAlt) lines.add("بديل_ملء_الشاشة=" + (fullscreenWorkaroundAlt ? "نعم" : "لا"));
        if (!hasFont) lines.add("خط_مخصص=" + customFontPath);
        if (!hasSessToast) lines.add("تنبيه_تبديل_الجلسة=" + (sessionChangeToast ? "نعم" : "لا"));
        if (!hasVolKeys) lines.add("مفاتيح_الصوت=" + (volumeKeysCtrl ? "تحكم" : "لا شيء"));
        if (!hasRecvShare) lines.add("تعطيل_استقبال_المشاركة=" + (disableReceiveShare ? "نعم" : "لا"));
        if (!hasRecvView) lines.add("تعطيل_استقبال_العرض=" + (disableReceiveView ? "نعم" : "لا"));
        if (!hasTempAge) lines.add("حذف_المؤقت_الأقدم_من=" + tempAgeDaysText());
        if (!hasHwKeys) lines.add("تعطيل_اختصارات_لوحة_المفاتيح=" + (disableHardwareShortcuts ? "نعم" : "لا"));
        if (!hasErrColor) lines.add("لون_الخطأ=" + hexColorText(errorColor));
        if (!hasOkColor) lines.add("لون_النجاح=" + (successColor == 0 ? "" : hexColorText(successColor)));
        if (!hasCursorColor) lines.add("لون_المؤشر=" + (cursorColor == 0 ? "" : hexColorText(cursorColor)));
        StringBuilder sb = new StringBuilder();
        for (String l : lines) sb.append(l).append('\n');
        try {
            writeFileAtomic(f, sb.toString());
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

    /** عدد صحيح بإشارة من نص عربي أو غربي («-٥» أو
     * «-5» أو «٧»): arabicToInt يسقط الإشارة فلا يصلح
     * هنا — سالب = لا حذف أبدا في مفتاح عمر المؤقت
     * (المهمة 28). وبلا أرقام = الافتراضي 0. عام
     * لتقرأه الخدمة عند الخروج أيضا. */
    public static int parseTempAgeDays(String value) {
        if (value == null) return 0;
        String v = value.trim();
        boolean negative = v.startsWith("-") || v.startsWith("−");
        if (negative) v = v.substring(1);
        int n = arabicToInt(v);
        if (n < 0) return 0;
        return negative ? -n : n;
    }

    /** عمر المؤقت للكتابة في niva.properties (سالب =
     * شرطة لاتينية + أرقام عربية — فـarabicDigits لا
     * تعرف السالب). */
    private String tempAgeDaysText() {
        return tempAgeDays < 0
                ? "-" + arabicDigits(-tempAgeDays)
                : arabicDigits(tempAgeDays);
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
     * وحفظ في الإعدادات والملف معا. بلا تغيير يُحفَظ
     * شيء (الدفعة 3، البند 20): القرص يستدعيها مرات
     * كثيرة فلا تُكتب الإعدادات بلا داعٍ. */
    private void setFontSize(float size) {
        float next = Math.max(MIN_FONT_SP, Math.min(MAX_FONT_SP, size));
        if (next == fontSizeSp) return;
        fontSizeSp = next;
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
        if (keyRow2 != null) {
            keyRow2.setVisibility((show && isKeyboardVisible())
                    ? View.VISIBLE : View.GONE);
        }
        updateSearchRowVisibility();
        updateOutputSearchRowVisibility();
        saveSettings();
    }

    /** ضبط المظهر (v1.1.6 بند 10 + v1.1.7 بند 45): 0
     * داكن، 1 فاتح، 2 يتبع الجهاز، 3 أسود خالص. */
    private void setThemeMode(int mode) {
        themeMode = mode;
        lightTheme = themeMode == 1 || (themeMode == 2 && !isSystemNight());
        applyThemeColors();
        saveSettings();
    }

    /** تدوير المظهر من القائمة: داكن → فاتح → النظام →
     * أسود خالص. */
    private void cycleTheme() {
        int next = (themeMode + 1) % 4;
        setThemeMode(next);
        toast("المظهر: " + themeModeName());
    }

    /** اسم المظهر الحالي لعرضه في القائمة. */
    private String themeModeName() {
        return themeMode == 0 ? "داكن"
                : themeMode == 1 ? "فاتح"
                : themeMode == 2 ? "النظام" : "أسود خالص";
    }

    // ---------- v1.1.7: ملء الشاشة + الخط المخصص + حول ----------

    /** ملء الشاشة (بند 30): تبديل + حفظ. */
    private void setFullscreen(boolean on) {
        fullscreen = on;
        applyFullscreen();
        saveSettings();
    }

    /** تطبيق ملء الشاشة: إخفاء الأشرطة + تركيب حلّ
     * إعادة القياس، أو إظهارها وإزالته. */
    private void applyFullscreen() {
        if (rootLayout == null) return;
        if (fullscreen) {
            hideSystemBars();
            installFullscreenWorkaround();
        } else {
            showSystemBars();
            removeFullscreenWorkaround();
        }
    }

    /** إخفاء شريطي الحالة والتنقل (API 30+ بالمتحكم،
     * وقبله بالأعلام القديمة). */
    private void hideSystemBars() {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                c.hide(WindowInsets.Type.statusBars()
                        | WindowInsets.Type.navigationBars());
                c.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        }
    }

    /** إظهار شريطي الحالة والتنقل. */
    private void showSystemBars() {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                c.show(WindowInsets.Type.statusBars()
                        | WindowInsets.Type.navigationBars());
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_VISIBLE);
        }
    }

    /** حلّ ملء الشاشة (بند 30 + بديله بند 51): في ملء
     * الشاشة لا يقلّص النظام النافذة عند ظهور الكيبورد،
     * فيختفي صف المفاتيح خلفها. المستمع يعيد قياس الجذر
     * على الإطار المرئي كلما تغيّر — الأساسي: ارتفاع
     * الجذر = ارتفاع الإطار؛ والبديل: إبقاء الارتفاع
     * كاملا مع حشوة سفلية بقدر ما حجبه الكيبورد.
     * إعادة تنفيذ من التصميم الموثق في الدراسة، لا
     * نسخ كود (GPLv3). */
    private void installFullscreenWorkaround() {
        if (fullscreenLayoutListener != null || rootLayout == null) return;
        lastVisibleHeight = -1;
        fullscreenLayoutListener = new ViewTreeObserver.OnGlobalLayoutListener() {
            @Override
            public void onGlobalLayout() {
                if (!fullscreen || rootLayout == null) return;
                Rect r = new Rect();
                rootLayout.getWindowVisibleDisplayFrame(r);
                int visibleHeight = r.height();
                if (visibleHeight <= 0 || visibleHeight == lastVisibleHeight) return;
                lastVisibleHeight = visibleHeight;
                if (fullscreenWorkaroundAlt) {
                    int screenH = rootLayout.getRootView().getHeight();
                    int padBottom = Math.max(0, screenH - r.bottom);
                    rootLayout.setPadding(marginH, marginV,
                            marginH, marginV + padBottom);
                } else {
                    ViewGroup.LayoutParams lp = rootLayout.getLayoutParams();
                    if (lp.height != visibleHeight) {
                        lp.height = visibleHeight;
                        rootLayout.setLayoutParams(lp);
                    }
                }
            }
        };
        rootLayout.getViewTreeObserver()
                .addOnGlobalLayoutListener(fullscreenLayoutListener);
    }

    /** إزالة حلّ ملء الشاشة واستعادة الجذر كاملا. */
    private void removeFullscreenWorkaround() {
        if (fullscreenLayoutListener != null && rootLayout != null) {
            rootLayout.getViewTreeObserver()
                    .removeOnGlobalLayoutListener(fullscreenLayoutListener);
            fullscreenLayoutListener = null;
        }
        lastVisibleHeight = -1;
        if (rootLayout != null) {
            ViewGroup.LayoutParams lp = rootLayout.getLayoutParams();
            if (lp.height != ViewGroup.LayoutParams.MATCH_PARENT) {
                lp.height = ViewGroup.LayoutParams.MATCH_PARENT;
                rootLayout.setLayoutParams(lp);
            }
            applyMargins();
        }
    }

    /** شاشة «حول» (بند 31): حقائق عن التطبيق المثبت
     * فقط — لا شرح ولا درس. نص Markdown محلي في حوار
     * قابل للتمرير والنسخ، مع زر حفظ ملفا. */
    private void showAboutDialog() {
        String md = aboutMarkdown();
        TextView tv = new TextView(this);
        tv.setText(md);
        tv.setTextDirection(View.TEXT_DIRECTION_RTL);
        tv.setTextIsSelectable(true);
        tv.setTextSize(14);
        tv.setPadding(32, 24, 32, 24);
        ScrollView sv = new ScrollView(this);
        sv.addView(tv);
        new AlertDialog.Builder(this)
                .setTitle("حول")
                .setView(sv)
                .setPositiveButton("حفظ ملفا", (d, w) -> {
                    try {
                        writeFile(new File(getFilesDir(), "حول-نيفا.md"), md);
                        toast("تم الحفظ: حول-نيفا.md");
                    } catch (IOException e) {
                        toast("تعذّر الحفظ");
                    }
                })
                .setNegativeButton("إغلاق", null)
                .show();
    }

    /** نص «حول» بصيغة Markdown: إصدار التطبيق +
     * إصدار المحرك + معلومات الجهاز + الروابط. */
    private String aboutMarkdown() {
        String appVer = localVersionName();
        if (appVer == null) appVer = "؟";
        long vc = localVersionCode();
        String vcText = vc >= 0 ? arabicDigits((int) vc) : "؟";
        StringBuilder sb = new StringBuilder();
        sb.append("# نيفا — وحدة التحكم\n\n");
        sb.append("## التطبيق\n\n");
        sb.append("- الإصدار: ").append(appVer)
                .append(" (").append(vcText).append(")\n");
        sb.append("- الحزمة: com.rio4140.niva\n\n");
        sb.append("## المحرك\n\n");
        sb.append("- محرك نيفا ١٫٠٫٠\n\n");
        sb.append("## الجهاز\n\n");
        sb.append("- الطراز: ").append(Build.MANUFACTURER)
                .append(" ").append(Build.MODEL).append("\n");
        sb.append("- أندرويد: ").append(Build.VERSION.RELEASE)
                .append(" (المستوى ")
                .append(arabicDigits(Build.VERSION.SDK_INT)).append(")\n\n");
        sb.append("## الروابط\n\n");
        sb.append("- https://github.com/RIO4140/niva\n");
        sb.append("- https://rio4140.github.io/niva/\n");
        return sb.toString();
    }

    /** رقم نسخة التطبيق المثبتة (versionCode). */
    private long localVersionCode() {
        try {
            PackageInfo info;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                info = getPackageManager().getPackageInfo(getPackageName(),
                        PackageManager.PackageInfoFlags.of(0));
            } else {
                info = getPackageManager().getPackageInfo(getPackageName(), 0);
            }
            if (Build.VERSION.SDK_INT >= 28) return info.getLongVersionCode();
            return info.versionCode;
        } catch (Exception e) {
            return -1;
        }
    }

    /** خط مخصص من ملف (بند 32): حوار إدخال لاسم ملف
     * الخط داخل ~ (أو مسار مطلق داخل مجلد التطبيق).
     * خط تالف أو ناقص التشكيل يرجع للخط الافتراضي مع
     * تنبيه — فلا يكسر عرض العربية أبدا. */
    private void showCustomFontDialog() {
        final EditText input = new EditText(this);
        input.setText(customFontPath);
        input.setTextDirection(View.TEXT_DIRECTION_RTL);
        input.setHint("اسم ملف الخط داخل ~");
        new AlertDialog.Builder(this)
                .setTitle("خط مخصص")
                .setView(input)
                .setPositiveButton("تطبيق", (d, w) -> {
                    String path = input.getText().toString().trim();
                    if (path.isEmpty()) {
                        customFontPath = "";
                        applyCustomFont();
                        saveSettings();
                        toast("عاد الخط الافتراضي");
                        return;
                    }
                    customFontPath = path;
                    if (applyCustomFont()) {
                        saveSettings();
                        toast("تم تطبيق الخط");
                    } else {
                        customFontPath = "";
                        applyCustomFont();
                        toast("تعذّر تحميل الخط — عاد الخط الافتراضي");
                    }
                })
                .setNeutralButton("الخط الافتراضي", (d, w) -> {
                    customFontPath = "";
                    applyCustomFont();
                    saveSettings();
                    toast("عاد الخط الافتراضي");
                })
                .setNegativeButton("إلغاء", null)
                .show();
    }

    /** تطبيق الخط المخصص على الترمنال؛ يعيد false إن
     * تعذّر التحميل (فيرجع للخط الافتراضي). */
    private boolean applyCustomFont() {
        if (terminalView == null) return false;
        if (customFontPath == null || customFontPath.trim().isEmpty()) {
            customTypeface = null;
            terminalView.setTypeface(Typeface.DEFAULT);
            return true;
        }
        File f = resolveFontFile(customFontPath.trim());
        if (f == null) {
            customTypeface = null;
            terminalView.setTypeface(Typeface.DEFAULT);
            return false;
        }
        try {
            Typeface tf = Typeface.createFromFile(f);
            if (tf == null) throw new IOException("خط غير صالح");
            customTypeface = tf;
            terminalView.setTypeface(tf);
            return true;
        } catch (Exception e) {
            customTypeface = null;
            terminalView.setTypeface(Typeface.DEFAULT);
            return false;
        }
    }

    /** حلّ مسار ملف الخط: ~/… أو نسبي داخل ~ أو مطلق
     * — ويُقبل فقط ملف موجود داخل مجلد التطبيق. */
    private File resolveFontFile(String path) {
        File f;
        if (path.startsWith("~/")) {
            f = new File(getFilesDir(), path.substring(2));
        } else if (path.startsWith("/")) {
            f = new File(path);
        } else {
            f = new File(getFilesDir(), path);
        }
        try {
            File base = getFilesDir().getCanonicalFile();
            File canon = f.getCanonicalFile();
            if (!canon.getPath().startsWith(base.getPath())) return null;
            if (!canon.isFile()) return null;
            return canon;
        } catch (IOException e) {
            return null;
        }
    }

    /** إعادة الإعدادات للافتراضي (v1.1.6 بند 7): يحذف
     * ملف niva.properties ويمسح الإعدادات المحفوظة
     * ويعيد تحميل القيم الافتراضية فورا — بلا إعادة
     * كتابة الملف (الملف المفقود = الافتراضي). */
    private void resetSettingsToDefaults() {
        nivaPropertiesFile().delete();
        new File(getFilesDir(), NIVA_PROPS_FILE).delete();
        if (prefs != null) prefs.edit().clear().apply();
        fontSizeSp = DEFAULT_FONT_SP;
        keepScreenOn = false;
        showKeyRowPref = true;
        themeMode = 0;
        lightTheme = false;
        hideKeyboardOnStartup = false;
        softKeyboardEnabled = true;
        marginH = 16;
        marginV = 8;
        maxTranscriptLines = MAX_LINES;
        // v1.1.7: ملء الشاشة + بديله + الخط المخصص +
        // تنبيه تبديل الجلسة — كلها للافتراضي.
        fullscreen = false;
        fullscreenWorkaroundAlt = false;
        customFontPath = "";
        customTypeface = null;
        sessionChangeToast = true;
        if (terminalView != null) terminalView.setTextSize(fontSizeSp);
        applyThemeColors();
        applyKeepScreenFlag();
        applyMargins();
        applyFullscreen();
        applyCustomFont();
        if (terminalView != null) {
            terminalView.setShowSoftInputOnFocus(softKeyboardEnabled);
        }
        if (keyRow != null) {
            keyRow.setVisibility((showKeyRowPref && isKeyboardVisible())
                    ? View.VISIBLE : View.GONE);
        }
        toast("تمت إعادة الإعدادات للافتراضي");
    }

    // ---------- قائمة العنوان (الضغطة الطويلة) ----------

    /** قائمة الإعدادات والإجراءات من الضغطة الطويلة على
     * عنوان الشريط العلوي (بند 4 + 5 + 7 + 9 + 10،
     * وحفظ الجلسة ملفا و«امسح كل شيء» وإعادة الإعدادات
     * — v1.1.6 بنود 3 و7 و8). */
    private void showTopMenu() {
        if (titleView == null) return;
        PopupMenu popup = new PopupMenu(this, titleView);
        Menu menu = popup.getMenu();
        MenuItem keep = menu.add(Menu.NONE, 1, Menu.NONE, "إبقاء الشاشة مضاءة");
        keep.setCheckable(true);
        keep.setChecked(keepScreenOn);
        menu.add(Menu.NONE, 2, Menu.NONE, "المظهر: " + themeModeName());
        MenuItem row = menu.add(Menu.NONE, 3, Menu.NONE, "إظهار صف المفاتيح");
        row.setCheckable(true);
        row.setChecked(showKeyRowPref);
        menu.add(Menu.NONE, 4, Menu.NONE, "تكبير الخط");
        menu.add(Menu.NONE, 5, Menu.NONE, "تصغير الخط");
        menu.add(Menu.NONE, 6, Menu.NONE, "مشاركة نص الجلسة");
        menu.add(Menu.NONE, 7, Menu.NONE, "اختر رابطا");
        menu.add(Menu.NONE, 8, Menu.NONE, "كتابة برنامج");
        // v1.1.6: حفظ نص الجلسة ملفا (بند 8)، «امسح كل
        // شيء» (بند 3)، إعادة الإعدادات (بند 7).
        menu.add(Menu.NONE, 9, Menu.NONE, "حفظ نص الجلسة ملفا");
        menu.add(Menu.NONE, 10, Menu.NONE, "امسح كل شيء");
        menu.add(Menu.NONE, 11, Menu.NONE, "أعد الإعدادات للافتراضي");
        // v1.1.7: ملء الشاشة (بند 30)، حول (بند 31)،
        // خط مخصص (بند 32).
        MenuItem full = menu.add(Menu.NONE, 12, Menu.NONE, "ملء الشاشة");
        full.setCheckable(true);
        full.setChecked(fullscreen);
        menu.add(Menu.NONE, 13, Menu.NONE, "حول");
        menu.add(Menu.NONE, 14, Menu.NONE, "خط مخصص…");
        // المهمة 28: تشغيل ملف .نيفا + ملفات ممنوحة.
        menu.add(Menu.NONE, 15, Menu.NONE, "شغّل ملف .نيفا");
        menu.add(Menu.NONE, 16, Menu.NONE, "ملفات ممنوحة");
        // الدفعة 3، البند 60: بحث في المخرجات.
        menu.add(Menu.NONE, 17, Menu.NONE, "بحث في المخرجات");
        popup.setOnMenuItemClickListener(item -> {
            int id = item.getItemId();
            if (id == 1) {
                setKeepScreenOn(!keepScreenOn);
                return true;
            }
            if (id == 2) {
                cycleTheme();
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
            if (id == 9) {
                saveSessionTextToFile();
                return true;
            }
            if (id == 10) {
                showClearAllDialog();
                return true;
            }
            if (id == 11) {
                resetSettingsToDefaults();
                return true;
            }
            if (id == 12) {
                setFullscreen(!fullscreen);
                return true;
            }
            if (id == 13) {
                showAboutDialog();
                return true;
            }
            if (id == 14) {
                showCustomFontDialog();
                return true;
            }
            if (id == 15) {
                launchRunPicker();
                return true;
            }
            if (id == 16) {
                showGrantedFilesMenu();
                return true;
            }
            if (id == 17) {
                toggleOutputSearch();
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
        if (sel.isEmpty()) {
            // مهمة 12: لا صمت عند فشل النسخ — المستخدم
            // يعرف أن شيئا لم يُنسخ، بدلا من ظن أن النسخ
            // تم والحافظة فارغة.
            toast("لا يوجد نص محدد");
            return;
        }
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null) return;
        cm.setPrimaryClip(ClipData.newPlainText("نيفا", sel));
        Toast.makeText(this, "تم النسخ", Toast.LENGTH_SHORT).show();
    }

    /** نسخ السطر (مهمة 12): السطر كله — أو كل الأسطر
     * التي يمسها التحديد — بلا الموجّه البادئ، فيُلصق
     * أمرا جاهزا للتنفيذ. فراغات نهاية السطر تُقصّ
     * كما في نسخ Termux. */
    private void copyLineAtSelection() {
        Editable text = terminalView.getText();
        if (text == null) return;
        int start = terminalView.getSelectionStart();
        int end = terminalView.getSelectionEnd();
        if (start < 0 || end < 0) return;
        int lo = Math.min(start, end);
        int hi = Math.max(start, end);
        String full = text.toString();
        int lineStart = full.lastIndexOf('\n', Math.max(0, lo - 1)) + 1;
        int lineEnd = full.indexOf('\n', hi);
        if (lineEnd < 0) lineEnd = full.length();
        if (lineStart >= lineEnd) {
            toast("لا يوجد نص محدد");
            return;
        }
        String region = full.substring(lineStart, lineEnd);
        String[] lines = region.split("\n", -1);
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < lines.length; i++) {
            String l = lines[i];
            if (i == 0) l = stripPromptPrefix(l);
            int e = l.length();
            while (e > 0 && l.charAt(e - 1) == ' ') e--;
            if (i > 0) out.append('\n');
            out.append(l, 0, e);
        }
        String result = out.toString();
        if (result.trim().isEmpty()) {
            toast("لا يوجد نص محدد");
            return;
        }
        ClipboardManager cm = (ClipboardManager)
                getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null) return;
        cm.setPrimaryClip(ClipData.newPlainText("نيفا", result));
        Toast.makeText(this, "تم نسخ السطر", Toast.LENGTH_SHORT).show();
    }

    /** إزالة بادئة الموجّه من سطر («~ $ » أو «> » أو
     * «~/مجلد $ ») — نفس منطق isPrompt، للنسخ فقط. */
    private static String stripPromptPrefix(String line) {
        if (line.startsWith("~ $ ")) return line.substring(4);
        if (line.startsWith("> ")) return line.substring(2);
        if (line.startsWith("~/")) {
            int dollar = line.indexOf(" $ ");
            if (dollar > 2) {
                String mid = line.substring(2, dollar);
                if (!mid.isEmpty() && !mid.contains(" ")
                        && !mid.contains("\n")) {
                    return line.substring(dollar + 3);
                }
            }
        }
        return line;
    }

    /** حذف النص المحدد من العرض ومن السجل المحفوظ
     * (v1.1.6 بند 12): يزيل المحدد من النص المعروض،
     * ثم يزامن نص الجلسة (مصدر ملف السجل) ويحفظ فورا
     * — فيُحذف المحدد من الملف أيضا. سطر الإدخال
     * الحالي لا يُحذف أبدا؛ المحدد الذي يطاله يُرفَض
     * بهدوء مع تنبيه. */
    private void deleteSelectedText() {
        Editable text = terminalView.getText();
        if (text == null) return;
        int start = terminalView.getSelectionStart();
        int end = terminalView.getSelectionEnd();
        if (start < 0 || end < 0 || start == end) return;
        int lo = Math.max(0, Math.min(start, end));
        int hi = Math.min(text.length(), Math.max(start, end));
        if (lo >= hi) return;
        if (terminalView.inputStart >= 0 && hi > terminalView.inputStart) {
            toast("لا يمكن حذف سطر الإدخال");
            return;
        }
        int removed = hi - lo;
        terminalView.programmatic = true;
        text.delete(lo, hi);
        if (terminalView.inputStart >= 0) {
            terminalView.inputStart = Math.max(0, terminalView.inputStart - removed);
        }
        terminalView.programmatic = false;
        saveTerminalToSession();
        terminalView.setSelection(terminalView.inputStart, terminalView.inputStart);
        saveAllSessions();
        toast("تم حذف المحدد");
    }

    /** مشاركة النص المحدد فقط (بند 1 — تأتي مع النسخ). */
    private void shareSelection() {
        String sel = selectedText();
        if (sel.isEmpty()) {
            toast("لا يوجد نص محدد");
            return;
        }
        shareText(sel);
    }

    /** مشاركة نص الجلسة كله (بند 5) — بعد حوار
     * معاينة وتحذير (H6). */
    private void shareTranscript() {
        Session s = active();
        String text = s.text.toString();
        confirmShareText(text, () -> shareText(text), null);
    }

    /** حوار معاينة قبل مشاركة نص جلسة (H6): يبيّن
     * الحجم وأول 300 حرف ويحذّر أن النص قد يحوي
     * أسرارا كُتبت في الجلسة؛ فالمستخدم يقرر.
     * onCancel فارغ = لا فعل عند الإلغاء (مسار
     * القائمة)؛ ومسار البرنامج يمرر خطأ لئلا يعلق
     * المحرك في انتظار النتيجة. */
    private void confirmShareText(String text, Runnable onConfirm,
            Runnable onCancel) {
        if (text == null || text.isEmpty()) return;
        String preview = text.length() > 300
                ? text.substring(0, 300) + "…"
                : text;
        new AlertDialog.Builder(this)
                .setTitle("مشاركة نص الجلسة؟")
                .setMessage("ستُشارك " + text.length()
                        + " حرفا من نص الجلسة — وقد يحوي أسرارا كُتبت فيها.\n\n«"
                        + preview + "»")
                .setCancelable(false)
                .setPositiveButton("شارك", (d, w) -> onConfirm.run())
                .setNegativeButton("إلغاء", (d, w) -> {
                    if (onCancel != null) onCancel.run();
                })
                .show();
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
        // أثناء الخروج/التدمير: لا تعيد نشر الإشعار —
        // فإعادة النشر هنا كانت تعيد تشغيل الخدمة بعد
        // إيقافها (خلل إعادة الفتح من درج الإشعارات).
        if (isFinishing() || isDestroyed()) return;
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
