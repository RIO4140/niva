package com.rio4140.niva;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;

import java.io.File;
import java.io.FileNotFoundException;

/**
 * مزوّد ملفات مصغّر للتحديث الذاتي (v1.1.4): يخدم ملف
 * تحديث واحدا فقط — niva-update.apk من ذاكرة التطبيق
 * المؤقتة — للمثبّت، ولا يخدم أي ملف آخر أبدا.
 * بلا مكتبات خارجية: بديل إطار العمل عن FileProvider.
 */
public class ApkProvider extends ContentProvider {

    static final String AUTHORITY = "com.rio4140.niva.fileprovider";
    static final String APK_NAME = "niva-update.apk";

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode)
            throws FileNotFoundException {
        // اسم واحد مسموح فقط؛ أي مسار آخر مرفوض — لا
        // وصول لملفات خارج ملف التحديث أبدا.
        if (!APK_NAME.equals(uri.getLastPathSegment())) {
            throw new FileNotFoundException("ملف غير مسموح");
        }
        File file = new File(getContext().getCacheDir(), APK_NAME);
        if (!file.isFile()) {
            throw new FileNotFoundException("لا يوجد ملف تحديث");
        }
        return ParcelFileDescriptor.open(
                file, ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
                        String[] selectionArgs, String sortOrder) {
        return null;
    }

    @Override
    public String getType(Uri uri) {
        return "application/vnd.android.package-archive";
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection,
                      String[] selectionArgs) {
        return 0;
    }
}
