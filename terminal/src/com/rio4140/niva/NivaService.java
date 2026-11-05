package com.rio4140.niva;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;

/**
 * نيفا — الخدمة الأمامية (بند 13).
 *
 * إشعار عربي مستمر بعدد الجلسات («نيفا — N جلسات»)
 * وحالة قفل الاستيقاظ، مع زرّي خروج وتبديل قفل
 * الاستيقاظ اليدوي. START_NOT_STICKY: لا تعود بعد
 * موت العملية — والخدمة لا تعني استعادة الجلسات
 * (حتى Termux يفقد الجلسات عند موت العملية)؛
 * التطبيق يحتفظ بنسخة السجل فقط (م-9)، والمتغيرات
 * لا تعود أبدا.
 */
public class NivaService extends Service {

    public static final String ACTION_EXIT = "com.rio4140.niva.EXIT";
    public static final String ACTION_TOGGLE_WAKE =
            "com.rio4140.niva.TOGGLE_WAKE";
    public static final String ACTION_REFRESH = "com.rio4140.niva.REFRESH";

    private static final int NOTIF_ID = 1;
    private static final String CHANNEL_ID = "niva";

    /** قفل الاستيقاظ اليدوي (من زر الإشعار) — يبقى
     * ممسوكا حتى يبدّله المستخدم أو تموت الخدمة. */
    private static boolean manualWakeHeld = false;
    private PowerManager.WakeLock manualWakeLock;

    private int sessionCount = 1;
    private boolean wakeHeld = false;

    public static boolean isManualWakeHeld() {
        return manualWakeHeld;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        createChannel();
        startForegroundWithType(buildNotification());
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && intent.getAction() != null) {
            String action = intent.getAction();
            if (ACTION_EXIT.equals(action)) {
                // الخروج: إبلاغ النشاط لينهي نفسه (فيقتل
                // العمليات ويوقف الخدمة في onDestroy).
                sendBroadcast(new Intent(ACTION_EXIT));
                stopSelf();
                return START_NOT_STICKY;
            }
            if (ACTION_TOGGLE_WAKE.equals(action)) {
                toggleManualWake();
            } else if (ACTION_REFRESH.equals(action)) {
                sessionCount = intent.getIntExtra("sessions", sessionCount);
                wakeHeld = intent.getBooleanExtra("wake_held", wakeHeld);
            }
            startForegroundWithType(buildNotification());
        }
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        if (manualWakeLock != null && manualWakeLock.isHeld()) {
            try {
                manualWakeLock.release();
            } catch (Exception ignored) {
            }
        }
        manualWakeHeld = false;
        stopForeground(true);
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void toggleManualWake() {
        try {
            PowerManager pm = (PowerManager)
                    getSystemService(Context.POWER_SERVICE);
            if (pm == null) return;
            if (manualWakeLock == null) {
                manualWakeLock = pm.newWakeLock(
                        PowerManager.PARTIAL_WAKE_LOCK, "niva:manual");
            }
            if (manualWakeHeld) {
                if (manualWakeLock.isHeld()) manualWakeLock.release();
                manualWakeHeld = false;
            } else {
                if (!manualWakeLock.isHeld()) manualWakeLock.acquire();
                manualWakeHeld = true;
            }
        } catch (Exception ignored) {
        }
    }

    private void createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        NotificationManager nm = (NotificationManager)
                getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm == null) return;
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID, "نيفا", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("إبقاء جلسات نيفا حية");
        nm.createNotificationChannel(channel);
    }

    private void startForegroundWithType(Notification notif) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notif,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        } else {
            startForeground(NOTIF_ID, notif);
        }
    }

    private Notification buildNotification() {
        String text = "نيفا — " + arabicDigits(sessionCount) + " جلسات";
        if (wakeHeld || manualWakeHeld) {
            text += " (قفل الاستيقاظ ممسوك)";
        }
        Intent open = new Intent(this, MainActivity.class);
        open.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP
                | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent openPi = PendingIntent.getActivity(this, 0, open,
                PendingIntent.FLAG_UPDATE_CURRENT
                        | PendingIntent.FLAG_IMMUTABLE);
        Intent exit = new Intent(this, NivaService.class);
        exit.setAction(ACTION_EXIT);
        PendingIntent exitPi = PendingIntent.getService(this, 1, exit,
                PendingIntent.FLAG_UPDATE_CURRENT
                        | PendingIntent.FLAG_IMMUTABLE);
        Intent wake = new Intent(this, NivaService.class);
        wake.setAction(ACTION_TOGGLE_WAKE);
        PendingIntent wakePi = PendingIntent.getService(this, 2, wake,
                PendingIntent.FLAG_UPDATE_CURRENT
                        | PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, CHANNEL_ID);
        } else {
            builder = new Notification.Builder(this);
            builder.setPriority(Notification.PRIORITY_LOW);
        }
        builder.setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle("نيفا")
                .setContentText(text)
                .setContentIntent(openPi)
                .setOngoing(true)
                .addAction(0, "خروج", exitPi)
                .addAction(0, "قفل الاستيقاظ", wakePi);
        return builder.build();
    }

    private static String arabicDigits(int n) {
        StringBuilder sb = new StringBuilder();
        for (char c : String.valueOf(n).toCharArray()) {
            sb.append((char) ('٠' + (c - '0')));
        }
        return sb.toString();
    }
}
