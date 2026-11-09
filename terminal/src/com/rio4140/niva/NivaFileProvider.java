package com.rio4140.niva;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;

/**
 * مزوّد ملفات عام (الدفعة 4أ — البنود 22 و23 و32):
 * يخدم ملفات مجلد البرنامج (getFilesDir) لتطبيقات
 * خارجية — مشاركة ملف، فتح ملف، وكتابة صورة من
 * تطبيق الكاميرا — قراءة فقط، عدا كتابة الصورة
 * الملتقطة بمنح لمرة واحدة. بلا مكتبات خارجية:
 * بديل إطار العمل عن FileProvider.
 *
 * قواعد الأمان:
 * - المسار المحلول قانونيا يجب أن يبقى داخل مجلد
 *   البرنامج؛ أي خروج عنه مرفوض (بلا .. وبلا روابط
 *   رمزية هاربة).
 * - ملف الخصائص niva.properties مرفوض دائما — لا
 *   يغادر التطبيق أبدا، لا قراءة ولا كتابة.
 * - مجلدات لا تُخدم؛ ملفات فقط.
 * - الملفات المشتركة تُمنح للقراءة فقط؛ الكتابة
 *   لملف صورة واحد يحدده التطبيق فقط.
 */
public class NivaFileProvider extends ContentProvider {

    static final String AUTHORITY = "com.rio4140.niva.files";
    static final String PROPERTIES_NAME = "niva.properties";

    @Override
    public boolean onCreate() {
        return true;
    }

    /** يبني رابط المحتوى لملف داخل مجلد البرنامج. */
    static Uri getUriForFile(Context context, File file) {
        File base = context.getFilesDir();
        String basePath = base.getAbsolutePath();
        String filePath = file.getAbsolutePath();
        String rel = filePath.startsWith(basePath + File.separator)
                ? filePath.substring(basePath.length() + 1)
                : file.getName();
        return Uri.parse("content://" + AUTHORITY + "/"
                + Uri.encode(rel, "/"));
    }

    /** يحوّل رابط المحتوى إلى ملف داخل مجلد
     * البرنامج، أو يرمي FileNotFoundException. */
    private File fileForUri(Uri uri) throws FileNotFoundException {
        String rel = uri.getEncodedPath();
        if (rel == null || rel.length() <= 1) {
            throw new FileNotFoundException("مسار فارغ");
        }
        rel = Uri.decode(rel.substring(1));
        File base = getContext().getFilesDir();
        File file = new File(base, rel);
        try {
            File canonBase = base.getCanonicalFile();
            File canon = file.getCanonicalFile();
            String basePath = canonBase.getAbsolutePath() + File.separator;
            if (!canon.getAbsolutePath().startsWith(basePath)) {
                throw new FileNotFoundException("مسار خارج مجلد البرنامج");
            }
            if (PROPERTIES_NAME.equals(canon.getName())) {
                throw new FileNotFoundException("ملف خصائص محمي");
            }
            return canon;
        } catch (IOException e) {
            throw new FileNotFoundException("تعذّر حل المسار");
        }
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode)
            throws FileNotFoundException {
        File file = fileForUri(uri);
        boolean write = "w".equals(mode) || "wt".equals(mode)
                || "wa".equals(mode) || "rw".equals(mode);
        if (write) {
            // كتابة الصورة الملتقطة: الملف يُنشأ داخل
            // مجلد البرنامج فقط، ووالده موجود مسبقا.
            if (file.isDirectory()) {
                throw new FileNotFoundException("مجلد لا يُكتب");
            }
            return ParcelFileDescriptor.open(
                    file,
                    ParcelFileDescriptor.MODE_WRITE_ONLY
                            | ParcelFileDescriptor.MODE_CREATE
                            | ParcelFileDescriptor.MODE_TRUNCATE);
        }
        if (!file.isFile()) {
            throw new FileNotFoundException("الملف غير موجود");
        }
        return ParcelFileDescriptor.open(
                file, ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
                        String[] selectionArgs, String sortOrder) {
        File file;
        try {
            file = fileForUri(uri);
        } catch (FileNotFoundException e) {
            return null;
        }
        if (!file.isFile()) {
            return null;
        }
        MatrixCursor cursor = new MatrixCursor(new String[]{
                OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE});
        cursor.addRow(new Object[]{file.getName(), file.length()});
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        File file;
        try {
            file = fileForUri(uri);
        } catch (FileNotFoundException e) {
            return "application/octet-stream";
        }
        String name = file.getName();
        int dot = name.lastIndexOf('.');
        if (dot >= 0 && dot + 1 < name.length()) {
            String mime = MimeTypeMap.getSingleton()
                    .getMimeTypeFromExtension(
                            name.substring(dot + 1).toLowerCase());
            if (mime != null) {
                return mime;
            }
        }
        return "application/octet-stream";
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
