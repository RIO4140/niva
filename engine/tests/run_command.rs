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
    run_repl_env(dir, input, &[], &[])
}

/// النسخة بوسائط ومتغيرات بيئة إضافية (للشبكة المحلية).
fn run_repl_env(
    dir: &PathBuf,
    input: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (String, i32) {
    let mut cmd = Command::new(bin());
    cmd.args(["--تفاعلي"]);
    for a in args {
        cmd.arg(a);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
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

/// خادم جيت هاب محاكى صارم: يرد 200 فقط للمسار
/// الدقيق (بلا اعتبار الاستعلام)، و404 لغيره.
/// يسرد ملفين بأحجامهما (32 بايتا لكل منهما).
fn strict_github_mock() -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut req: Vec<u8> = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if req.len() >= 4 && &req[req.len() - 4..] == b"\r\n\r\n" {
                            break;
                        }
                        if req.len() > 8192 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let req_text = String::from_utf8_lossy(&req).into_owned();
            let target = req_text
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let path = target.split('?').next().unwrap_or("/").to_string();
            let base = format!("http://127.0.0.1:{}", port);
            let file_body = "صدر قيمة\nقيمة = ٤٢\n";
            let (status, body): (&str, String) = match path.as_str() {
                "/repos/owner/repo/contents/sub" => (
                    "200 OK",
                    format!(
                        "[{{\"name\":\"a.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/a.نيفا\",\"size\":32}},{{\"name\":\"b.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/b.نيفا\",\"size\":32}}]",
                        base, base
                    ),
                ),
                _ if path.starts_with("/files/") => {
                    ("200 OK", file_body.to_string())
                }
                _ => (
                    "404 Not Found",
                    "{\"message\":\"Not Found\"}".to_string(),
                ),
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                )
                .as_bytes(),
            );
        }
    });
    port
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

#[test]
fn install_all_streams_live_in_repl() {
    // تثبيت-الكل في التفاعلي (البث الحي): السطور
    // تظهر لحظة إنتاجها وبالترتيب، ومرة واحدة فقط
    // (القيمة سلسلة فارغة فلا تطبع ثانية)، وبلا
    // بصمة؛ والموجّه حي بعدها.
    let dir = fresh_dir();
    let port = strict_github_mock();
    let api_base = format!("http://127.0.0.1:{}", port);
    let (out, code) = run_repl_env(
        &dir,
        "تثبيت https://github.com/owner/repo/tree/main/sub\n١ + ١\n",
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
        &[("NIVA_GITHUB_API_BASE", &api_base)],
    );
    assert_eq!(code, 0, "out: {}", out);
    let expected = [
        "جارٍ قراءة قائمة الملفات...\n",
        "سيُثبَّت ٢ ملفات (الحجم الإجمالي ٦٤ بايت)\n",
        "جارٍ تنزيل a (٣٢ بايت)... (١ من ٢)\n",
        "ثُبّتت الوحدة a\n",
        "جارٍ تنزيل b (٣٢ بايت)... (٢ من ٢)\n",
        "ثُبّتت الوحدة b\n",
        "ثُبّت ٢ من ٢ (تُخطَّى ٠)\n",
    ];
    let mut pos = 0usize;
    for line in expected {
        let at = out[pos..].find(line).unwrap_or_else(|| {
            panic!("missing {:?} in order in out: {}", line, out)
        });
        pos += at + line.len();
        assert_eq!(
            out.matches(line).count(),
            1,
            "line {:?} printed more than once in out: {}",
            line,
            out
        );
    }
    assert!(!out.contains("بصمة"), "out: {}", out);
    assert!(out.contains("٢\n"), "out: {}", out);
    assert!(dir.join("a.نيفا").exists());
    assert!(dir.join("b.نيفا").exists());
    let _ = fs::remove_dir_all(&dir);
}
