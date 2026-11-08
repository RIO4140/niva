//! أمر `شغّل` التفاعلي على مستوى الثنائي — اجتماع 43
//! §58 + تجاوز المالك «بلا تعقيد»: يغذّي الموجّه بسطور
//! عبر stdin ويقارن المخرج ورمز الخروج.
//!
//! كل اختبار يشغل الثنائي الحقيقي (CARGO_BIN_EXE_niva)
//! في مجلد مؤقت فريد، بلا شبكة وبلا حالة مشتركة.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

fn bin() -> String {
    env!("CARGO_BIN_EXE_niva").to_string()
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "niva-run-cmd-{}-{}",
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// شغّل الموجّه بسطور عبر stdin، وأعد (المخرج، رمز
/// الخروج). إغلاق stdin يخرج نظيفا كما يفعل Ctrl-D.
fn run_repl(dir: &PathBuf, input: &str) -> (String, i32) {
    let mut child = Command::new(bin())
        .args(["--تفاعلي"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn نيفا");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait نيفا");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn write_dashed_example(dir: &PathBuf) {
    fs::write(dir.join("قاعدة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n").unwrap();
    fs::write(
        dir.join("مثال-أول.نيفا"),
        "استورد قاعدة\nاطبع قاعدة.قيمة\n",
    )
    .unwrap();
}

#[test]
fn run_command_dashed_name_both_forms() {
    let dir = fresh_dir();
    write_dashed_example(&dir);
    let (out, code) = run_repl(
        &dir,
        "شغّل مثال-أول\nشغّل مثال-أول.نيفا\nشغل مثال-أول\n١ + ١\n",
    );
    assert_eq!(code, 0, "out: {}", out);
    // الاسم العاري والاسم الكامل والصيغة بلا تشكيل —
    // ثلاث مرات «٤٢»، ثم التفاعلي حي.
    assert_eq!(out.matches("٤٢\n").count(), 3, "out: {}", out);
    assert!(out.contains("٢\n"), "out: {}", out);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_command_missing_file_returns_to_prompt() {
    let dir = fresh_dir();
    write_dashed_example(&dir);
    let (out, code) = run_repl(&dir, "شغّل غائب\n١ + ١\n");
    assert_eq!(code, 0, "out: {}", out);
    assert!(out.contains("غير موجود"), "out: {}", out);
    assert!(out.contains("٢\n"), "out: {}", out);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_command_runtime_error_returns_to_prompt() {
    let dir = fresh_dir();
    fs::write(dir.join("خاطئ.نيفا"), "اطبع ١\nس = ١ / ٠\n").unwrap();
    let (out, code) = run_repl(&dir, "شغّل خاطئ\n٢ + ٢\n");
    assert_eq!(code, 0, "out: {}", out);
    assert!(out.contains("١\n"), "out: {}", out);
    assert!(out.contains("خطأ"), "out: {}", out);
    assert!(out.contains("٤\n"), "out: {}", out);
    let _ = fs::remove_dir_all(&dir);
}
