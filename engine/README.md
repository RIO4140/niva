# Rio4140 Engine — محرك مستقل بلغة Rust

محرك تنفيذ مستقل تماما عن Python: Lexer + Parser + AST +
Tree-Walking Interpreter، مكتوب بـ Rust، ولا يحوّل برامج Rio4140
إلى Python، ولا إلى لغة آلة في v1.

## البناء والتشغيل (دون Python)

البناء يحتاج أدوات Rust فقط (cargo/rustc). التشغيل لا يحتاج شيئا
آخر: الملف التنفيذي الناتج مستقل بذاته.

```bash
cargo build --release
./target/release/rio examples/01-hello.rio
./target/release/rio examples/08-full-program.rio
./target/release/rio program.rio --ast   # اطبع شجرة AST ثم شغّل
cargo test                                # اختبارات الوحدة والتكامل
```

## أمثلة

انظر مجلد `examples/`: طباعة، متغيرات، شروط، حلقات
(بما فيها `كرر N مرات` و`كرر طالما`)، دوال، قوائم وقواميس،
محاولة وأخطاء وملفات، وبرنامج درجات كامل.

## التصميم

انظر `DESIGN.md` لقواعد AST وScope وType System وError Handling
بالعربي، بما فيها: متغير الدالة محلي فقط، ومتغير الحلقة متاح
بعدها، والشروط يجب أن تكون منطقية، والتحويل الصريح فقط
عبر `عدد()` و`نص()`، ورسائل الأخطاء بالسطر والعمود والنوع.

## الأنظمة المستهدفة

الكود std فقط ومحمول، فيمكن بناؤه لـ Windows وLinux وAndroid.
النسخة المبنية هنا Linux. بناء Windows (mingw) وAndroid (NDK)
خطوة إصدار لاحقة.
