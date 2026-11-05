# ترمنال نيفا — تطبيق أندرويد الرسمي

ترمنال حقيقي للغة نيفا على أندرويد — ليس صفحة ويب ولا وحدة تحكم ويب. يشغّل محرك نيفا
الحقيقي (Rust) كعملية فرعية، بواجهة أندرويد أصلية تعرض العربية وتكتبها من اليمين
لليسار بشكل صحيح.

## المتطلبات

- JDK 17
- Android SDK: `cmdline-tools` و`platform-tools` و`platforms;android-34` و`build-tools;34.0.0`
- Android NDK r30
- Rust مع الهدف `aarch64-linux-android`

## البناء

من جذر هذا المستودع، ترجم المحرك للأندرويد أولا:

```
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="<NDK>/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang" \
  cargo build --release --target aarch64-linux-android --manifest-path engine/Cargo.toml
```

ثم انسخ الثنائي الناتج إلى `terminal/lib/arm64-v8a/libniva.so` (وجرّده بـ `llvm-strip`
من الـNDK). ثم من داخل `terminal/`:

```
BT="$ANDROID_HOME/build-tools/34.0.0"
"$BT/aapt2" compile --dir res -o compiled.zip
"$BT/aapt2" link -o app-unsigned.apk --manifest AndroidManifest.xml \
  -I "$ANDROID_HOME/platforms/android-34/android.jar" --java build/gen compiled.zip
javac -encoding UTF-8 -cp "$ANDROID_HOME/platforms/android-34/android.jar" \
  -d build/classes -sourcepath src:build/gen src/com/rio4140/niva/*.java
"$BT/d8" --lib "$ANDROID_HOME/platforms/android-34/android.jar" \
  --output build/classes build/classes/com/rio4140/niva/*.class
cp app-unsigned.apk app-build.apk
zip -j app-build.apk build/classes/classes.dex
zip -0 app-build.apk lib/arm64-v8a/libniva.so
"$BT/zipalign" -f -p 4 app-build.apk app-aligned.apk
```

## التوقيع

مفتاح الإصدار غير موجود في المستودع. وقّع الـAPK بمفتاحك الخاص:

```
"$BT/apksigner" sign --ks <مفتاحك> --out niva-terminal.apk app-aligned.apk
```

الـAPK الرسمي يوزَّع عبر GitHub Releases بعد اعتماد المالك الكامل للتطبيق.

## ملاحظات

- الحزمة `com.rio4140.niva`، الحد الأدنى SDK 24، الهدف SDK 34.
- الأذونات: `INTERNET` و`WAKE_LOCK` و`ACCESS_NETWORK_STATE` فقط.
- كل نص يراه المستخدم عربي؛ لا شرح تعليمي داخل التطبيق — ترمنال للعمل فقط.
- الشبكة في المحرك `اجلب` فقط (HTTP عادي)؛ HTTPS مرفوض حتى قرار TLS.
- النموذج الأولي للكتابة المعكوسة (`٥ = س`) غير موجود في أي بناء — ميزة `prototype-rtl` معطّلة افتراضيا، ومصيرها معلّق على حكم المالك.
