#!/bin/bash
# مثبّت لغة نيفا — كل شيء بالعربي
#
# الاستخدام من داخل المستودع:
#   ./install.sh
# أو بأمر واحد (بعد دفع الملف للمستودع العام):
#   curl -fsSL https://raw.githubusercontent.com/RIO4140/niva/main/install.sh | bash
#
# ما الذي يفعله:
#   1. يتأكد أن النظام Linux أو macOS وأن أدوات البناء موجودة (أو يعرض تثبيت Rust)
#   2. يبني المحرك من المصدر (cargo build --release)
#   3. يثبّت الأمر نيفا في ‎~/.local/bin
#   4. يتأكد أن ‎~/.local/bin في PATH (ويعرض إضافته)
#   5. يتحقق من التثبيت بتشغيل مثال
#
# على Windows: نزّل الثنائي الجاهز من صفحة الإصدارات،
# أو ابنِ من المصدر بـ cargo build --release.
#
# ملاحظة: أسماء المتغيرات والدوال في السكربت لاتينية لأن bash
# لا يقبل معرفات عربية؛ أما كل ما يراه المستخدم فبالعربي.
set -e

# سؤال بنعم/لا بالعربي. الافتراضي: نعم عند Enter.
# يقرأ من الطرفية مباشرة حتى عند تمرير السكربت عبر curl | bash.
ask_yes() {
    local q="$1"
    local ans=""
    # إن لم توجد طرفية (مثال: تشغيل غير تفاعلي)، الافتراضي نعم بصمت.
    # ترتيب التوجيه مهم: ‎2>/dev/null أولا حتى لا يظهر خطأ فتح ‎/dev/tty.
    if printf "%s [نعم/لا، الافتراضي نعم]: " "$q" 2>/dev/null > /dev/tty; then
        IFS= read -r ans 2>/dev/null < /dev/tty || ans=""
    fi
    case "$ans" in
        ""|"نعم"|"ن"|"yes"|"y"|"Y") return 0 ;;
        *) return 1 ;;
    esac
}

echo "=============================="
echo "  تثبيت لغة البرمجة نيفا"
echo "=============================="
echo ""

# 1. النظام: Linux أو macOS (خطوات يونكس واحدة)
OS="$(uname -s)"
if [ "$OS" != "Linux" ] && [ "$OS" != "Darwin" ]; then
    echo "خطأ: المثبّت الحالي يدعم Linux وmacOS فقط."
    echo "على Windows: نزّل الثنائي الجاهز من صفحة الإصدارات"
    echo "أو ابنِ من المصدر بـ cargo build --release."
    exit 1
fi
echo "النظام: $OS — حسن."

# 2. مصدر الكود: المستودع المحلي إن وجد، وإلا استنساخه
SRC=""
if [ -f "engine/Cargo.toml" ]; then
    SRC="$PWD"
    echo "وجدت مصدر نيفا هنا: $SRC"
else
    if ! command -v git > /dev/null 2>&1; then
        echo "خطأ: git غير موجود، وهو مطلوب لتنزيل مصدر نيفا."
        echo "ثبّت git أولا ثم أعد تشغيل المثبّت."
        exit 1
    fi
    SRC="$HOME/.local/share/نيفا"
    if [ -d "$SRC/engine" ]; then
        echo "وجدت مصدرا سابقا في: $SRC — سأحدثه."
        git -C "$SRC" pull --ff-only || echo "تعذر التحديث، سأكمل بالمصدر الموجود."
    else
        echo "أنزّل مصدر نيفا إلى: $SRC"
        git clone --depth 1 https://github.com/RIO4140/niva "$SRC"
    fi
fi

# 3. أدوات Rust: يجب أن يعمل cargo فعلا، لا أن يوجد اسمه فقط
# (قد يوجد عبر rustup بلا toolchain افتراضية).
if command -v cargo > /dev/null 2>&1 && cargo --version > /dev/null 2>&1; then
    echo "أدوات Rust موجودة: $(cargo --version)"
elif command -v rustup > /dev/null 2>&1; then
    echo "وجدت rustup لكن بلا toolchain افتراضية — أضبط stable كافتراضي..."
    rustup default stable
    echo "أدوات Rust موجودة: $(cargo --version)"
else
    echo ""
    echo "أدوات Rust (cargo) غير موجودة على جهازك."
    if ask_yes "هل أثبّت Rust الآن عبر rustup؟"; then
        if ! command -v curl > /dev/null 2>&1; then
            echo "خطأ: curl غير موجود، وهو مطلوب لتثبيت Rust."
            echo "ثبّت curl أولا ثم أعد تشغيل المثبّت."
            exit 1
        fi
        echo "أثبّت Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1091
        [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    else
        echo "أوقفت التثبيت: Rust مطلوب لبناء نيفا من المصدر."
        echo "ثبّت Rust بنفسك من https://rustup.rs ثم أعد تشغيل المثبّت."
        exit 1
    fi
    echo "أدوات Rust موجودة: $(cargo --version)"
fi

# 3.5 مترجم C (cc) — مطلوب فقط للبناء الأصلي (نيفا ابن --أصلي).
# غيابه لا يوقف التثبيت: الوضع الافتراضي (المصنوع) يعمل بدونه،
# وأمر البناء الأصلي نفسه يشرح التثبيت بالعربي عند الحاجة.
if command -v cc > /dev/null 2>&1; then
    echo "مترجم C موجود: $(cc --version | head -1)"
else
    echo ""
    echo "تنبيه: مترجم C (cc) غير موجود — البناء الأصلي (نيفا ابن --أصلي) لن يعمل حتى تثبّته."
    if [ -n "$PREFIX" ] && case "$PREFIX" in *com.termux*) true;; *) false;; esac; then
        echo "في Termux ثبّته بالأمر: pkg install clang"
    elif [ "$OS" = "Darwin" ]; then
        echo "على macOS ثبّته بالأمر: xcode-select --install"
    elif command -v apt > /dev/null 2>&1; then
        echo "على Debian/Ubuntu ثبّته بالأمر: apt install gcc"
    else
        echo "ثبّت مترجم C (gcc أو clang) من مدير حزم نظامك."
        echo "على Windows: ثبّت MinGW."
    fi
    echo "التثبيت مستمر — الوضع الافتراضي يعمل بلا مترجم C."
fi

# 4. البناء
echo ""
echo "أبني المحرك (قد يستغرق هذا دقيقة في أول مرة)..."
cargo build --release --manifest-path "$SRC/engine/Cargo.toml"

# 5. التثبيت في ‎~/.local/bin — الأمر المثبّت اسمه نيفا.
# (cargo يبني الملف التنفيذي باسم niva لأن أسماء ثنائيات
# cargo لاتينية فقط؛ المثبّت ينسخه إلى الاسم العربي.)
mkdir -p "$HOME/.local/bin"
cp "$SRC/engine/target/release/niva" "$HOME/.local/bin/نيفا"
chmod +x "$HOME/.local/bin/نيفا"
echo "ثبّت الأمر نيفا في: $HOME/.local/bin/نيفا"

# 6. PATH — ملف الإعداد حسب النظام والقشرة (zsh افتراضي macOS)
RC="$HOME/.bashrc"
if [ "$OS" = "Darwin" ]; then
    case "$SHELL" in
        *zsh*) RC="$HOME/.zshrc" ;;
    esac
fi
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$HOME/.local/bin"; then
    echo ""
    echo "المجلد ‎~/.local/bin غير موجود في PATH، فلن يعمل الأمر نيفا من أي مكان."
    if ask_yes "هل أضيفه تلقائيا إلى $RC؟"; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$RC"
        echo "أضفته إلى $RC — افتح طرفية جديدة أو نفذ: source $RC"
    else
        echo "أضف هذا السطر بنفسك إلى $RC:"
        echo '  export PATH="$HOME/.local/bin:$PATH"'
    fi
    export PATH="$HOME/.local/bin:$PATH"
fi

# 7. التحقق
echo ""
echo "أتحقق من التثبيت..."
"$HOME/.local/bin/نيفا" --نسخة
echo "--- مثال سريع ---"
"$HOME/.local/bin/نيفا" "$SRC/engine/examples/01-hello.نيفا"

echo ""
echo "=============================="
echo "  تم التثبيت بنجاح!"
echo "=============================="
echo "جرّب الآن:"
echo "  نيفا                       (دخول الوضع التفاعلي)"
echo "  نيفا برنامج.نيفا          (تشغيل برنامج)"
echo "  نيفا --مساعدة              (عرض المساعدة)"
