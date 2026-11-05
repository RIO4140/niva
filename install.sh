#!/bin/bash
# مثبّت لغة Niva — كل شيء بالعربي
#
# الاستخدام من داخل المستودع:
#   ./install.sh
# أو بأمر واحد (بعد دفع الملف للمستودع العام):
#   curl -fsSL https://raw.githubusercontent.com/RIO4140/niva/main/install.sh | bash
#
# ما الذي يفعله:
#   1. يتأكد أن النظام Linux وأن أدوات البناء موجودة (أو يعرض تثبيت Rust)
#   2. يبني المحرك من المصدر (cargo build --release)
#   3. يثبّت الأمر niva في ‎~/.local/bin
#   4. يتأكد أن ‎~/.local/bin في PATH (ويعرض إضافته)
#   5. يتحقق من التثبيت بتشغيل مثال
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
echo "  تثبيت لغة البرمجة Niva"
echo "=============================="
echo ""

# 1. النظام
if [ "$(uname -s)" != "Linux" ]; then
    echo "خطأ: المثبّت الحالي يدعم Linux فقط."
    echo "بناء Windows وmacOS خطوة إصدار لاحقة."
    exit 1
fi
echo "النظام: Linux — حسن."

# 2. مصدر الكود: المستودع المحلي إن وجد، وإلا استنساخه
SRC=""
if [ -f "engine/Cargo.toml" ]; then
    SRC="$PWD"
    echo "وجدت مصدر Niva هنا: $SRC"
else
    if ! command -v git > /dev/null 2>&1; then
        echo "خطأ: git غير موجود، وهو مطلوب لتنزيل مصدر Niva."
        echo "ثبّت git أولا ثم أعد تشغيل المثبّت."
        exit 1
    fi
    SRC="$HOME/.local/share/niva"
    if [ -d "$SRC/engine" ]; then
        echo "وجدت مصدرا سابقا في: $SRC — سأحدثه."
        git -C "$SRC" pull --ff-only || echo "تعذر التحديث، سأكمل بالمصدر الموجود."
    else
        echo "أنزّل مصدر Niva إلى: $SRC"
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
        echo "أوقفت التثبيت: Rust مطلوب لبناء Niva من المصدر."
        echo "ثبّت Rust بنفسك من https://rustup.rs ثم أعد تشغيل المثبّت."
        exit 1
    fi
    echo "أدوات Rust موجودة: $(cargo --version)"
fi

# 4. البناء
echo ""
echo "أبني المحرك (قد يستغرق هذا دقيقة في أول مرة)..."
cargo build --release --manifest-path "$SRC/engine/Cargo.toml"

# 5. التثبيت في ‎~/.local/bin
mkdir -p "$HOME/.local/bin"
cp "$SRC/engine/target/release/niva" "$HOME/.local/bin/niva"
chmod +x "$HOME/.local/bin/niva"
echo "ثبّت الأمر niva في: $HOME/.local/bin/niva"

# 6. PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$HOME/.local/bin"; then
    echo ""
    echo "المجلد ‎~/.local/bin غير موجود في PATH، فلن يعمل الأمر niva من أي مكان."
    if ask_yes "هل أضيفه تلقائيا إلى ‎~/.bashrc؟"; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
        echo "أضفته إلى ‎~/.bashrc — افتح طرفية جديدة أو نفذ: source ~/.bashrc"
    else
        echo "أضف هذا السطر بنفسك إلى ‎~/.bashrc:"
        echo '  export PATH="$HOME/.local/bin:$PATH"'
    fi
    export PATH="$HOME/.local/bin:$PATH"
fi

# 7. التحقق
echo ""
echo "أتحقق من التثبيت..."
"$HOME/.local/bin/niva" --version
echo "--- مثال سريع ---"
"$HOME/.local/bin/niva" "$SRC/engine/examples/01-hello.niva"

echo ""
echo "=============================="
echo "  تم التثبيت بنجاح!"
echo "=============================="
echo "جرّب الآن:"
echo "  niva                       (دخول REPL التفاعلي)"
echo "  niva program.niva          (تشغيل برنامج)"
echo "  niva --help                (عرض المساعدة)"
