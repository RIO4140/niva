//! Niva REPL (interactive terminal), Meeting 7.
//!
//! A REPL with RTL display, built on the compiler (VM) by
//! default since phase ك-2 (Meeting 25, task 7): every block
//! compiles to bytecode and runs on one persistent VM session
//! whose globals table survives across `feed()` calls
//! (DESIGN §39.3). The tree-walking interpreter stays only
//! as a hidden diagnostic mode (`Repl::new_interpreter`,
//! CLI `--مفسر`) — same session behavior, for comparison
//! and diagnosis, never documented as a way to run programs.
//!
//! Design:
//! - One engine (VM session or interpreter) + one
//!   SemanticAnalyzer persist across lines,
//!   so variables and functions defined earlier stay defined.
//!   The analyzer defines a name only after its value analyzes
//!   cleanly, so a failed line does not pollute later lines.
//! - Multi-line blocks are parser-driven: if parsing fails with
//!   "نهاية غير متوقعة داخل كتلة" or "توقع كتلة بمسافة بادئة
//!   بعد النقطتين", the input is incomplete and the REPL reads
//!   another line. An empty line while a block is open cancels it.
//! - A bare expression prints its value (fmt_value), like a
//!   Python REPL. Other statements run normally; their اطبع
//!   output is printed.
//! - Arabic prompt, Arabic help and errors. RTL display aid:
//!   :اعرض re-prints the session's lines wrapped in Unicode
//!   bidi isolates (RLI ... PDI), so a terminal renders each
//!   line as one isolated run. Full glyph shaping still depends
//!   on the terminal itself; the REPL does not pretend otherwise.
//! - std-only (no external crates): no arrow-key history in v1.
//! - Stateful line API: `Repl::feed` takes one line and returns
//!   `ReplOutput` (display text, next prompt, ended flag), holding
//!   the pending multi-line block inside the struct. The native
//!   `run_repl` loop and the WASM console both run on top of it,
//!   so the interactive behavior is identical everywhere.
//! - File policy is the same as file mode: restricted to the
//!   current working directory, unless --اسمح-بالكل was given.
//! - Commands (top-level only): :مساعدة :اعرض :امسح-المحفوظات
//!   :مسح :خروج (and plain خروج). Arabic only — no Latin
//!   aliases. :امسح-المحفوظات (task 2 of the Meeting 25 plan)
//!   clears only the :اعرض history lines — variables and
//!   functions stay — while :مسح resets the whole interpreter.
//! - Short terminal commands (owner's order 2026-11-05 ~22:02,
//!   corrected ~22:05): two-letter codes typed bare — like the
//!   :commands but WITHOUT the colon — with space-separated
//!   arguments and never a comma: ات (ls) مح (pwd) نم (cd)
//!   شم (mkdir) حم (rmdir) حف (rm) سم (cp) قم (mv) اف (cat).
//!   A REPL layer only: the line is intercepted before it is
//!   parsed as Niva code, and each code calls the file builtin
//!   that implements it. Full-word builtins (اعرض_الملفات...)
//!   stay as the implementation; the codes are what the
//!   terminal user types. Code rule: one easy letter from the
//!   first word + one easy letter from the second word.
//! - The prompt is Arabic-only punctuation: `نيفا: ` — the old
//!   `نيفا> ` mirrored to `نيفا<` in RTL display (bidi), and the
//!   continuation `.... ` was Latin dots; it is now `أكمل: `.

use crate::ast::{Expr, Stmt, StmtKind};
use crate::error::RioResult;
use crate::interpreter::Interpreter;
use crate::lexer::{normalize_cli_word, tokenize};
use crate::parser::Parser;
use crate::semantic::SemanticAnalyzer;
use crate::value::Value;
use crate::vm::VmSession;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Parser errors that mean "this input is incomplete, read more".
const INCOMPLETE_MARKERS: [&str; 2] =
    ["نهاية غير متوقعة داخل كتلة", "توقع كتلة بمسافة بادئة بعد النقطتين"];

/// Unicode bidi isolates, used by :اعرض for RTL display.
const RLI: char = '\u{2066}';
const PDI: char = '\u{2069}';

/// Block-continuation prompt: `> `, like any terminal
/// (owner's order 2026-11-05 ~23:02). The normal prompt is
/// dynamic — the current directory + ` $ ` (see
/// `Repl::prompt`): `~ $ ` at home, `~/dir $ ` below it,
/// RTL (owner's orders 2026-11-05 ~22:58 and ~23:03: the
/// prompt is never `نيفا: `).
pub const CONT_PROMPT: &str = "> ";

/// Structured control signals the REPL returns alongside its
/// display text (audit م-6): a host that wants them reads them
/// on a separate channel (stderr, only when the
/// `NIVA_CONTROL_CHANNEL=stderr` environment variable is set)
/// instead of matching visible text — a user program printing
/// the same words must never trigger them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplControl {
    None,
    /// This output asks for network permission (first use of
    /// اجلب with the gate closed).
    NetworkNeeded,
    /// This output asks for server permission (first use of
    /// اخدم with the gate closed). The network flag alone is
    /// never enough for the server (Meeting 27 §42.4).
    ServerNeeded,
    /// The session was cleared (:مسح) — the host clears its
    /// arrow-key history.
    SessionCleared,
    /// The settings were re-read (أعد-الإعدادات) — the host
    /// re-applies them live (Meeting 45 §59, item 19).
    SettingsReload,
}

/// The result of feeding one line to the REPL: the text to
/// display (stdout output, with any error text appended), the
/// prompt to show next, and whether the session ended (on
/// :خروج — EOF is reported by the caller's loop via `eof()`).
pub struct ReplOutput {
    pub output: String,
    pub prompt: String,
    pub ended: bool,
    /// True when this output is (or contains) the network
    /// permission error — the interactive console uses it to
    /// offer the first-use permission prompt (Meeting 22 §3.3).
    pub needs_network: bool,
}


pub enum ReplEval {
    Done { output: String },
    Failed { output: String, error: String },
    Incomplete,
}

/// ك-2 (مهمة 7): محرك الجلسة — VM افتراضيا، والمفسر وضع
/// تشخيص خفيا فقط. الواجهة مرآة واجهة المفسر التي يستخدمها
/// REPL، فيبقى منطق الجلسة واحدا في الوضعين.
enum Engine {
    Vm(VmSession),
    Interp(Interpreter),
}

impl Engine {
    fn run(&mut self, program: &[Stmt]) -> RioResult<()> {
        match self {
            Engine::Vm(s) => s.run(program),
            Engine::Interp(i) => i.run(program),
        }
    }

    fn eval_one(&mut self, expr: &Expr) -> RioResult<Value> {
        match self {
            Engine::Vm(s) => s.eval_one(expr),
            Engine::Interp(i) => i.eval_one(expr),
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        match self {
            Engine::Vm(s) => s.call_builtin(name, args, line, col),
            Engine::Interp(i) => i.call_builtin(name, args, line, col),
        }
    }

    fn current_dir_display(&self) -> String {
        match self {
            Engine::Vm(s) => s.current_dir_display(),
            Engine::Interp(i) => i.current_dir_display(),
        }
    }

    fn take_output(&mut self) -> String {
        match self {
            Engine::Vm(s) => s.take_output(),
            Engine::Interp(i) => std::mem::take(&mut i.output),
        }
    }

    fn set_trusted(&mut self, trusted: bool) {
        match self {
            Engine::Vm(s) => s.set_trusted(trusted),
            Engine::Interp(i) => i.set_trusted(trusted),
        }
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// — مرآة set_trusted حرفيا.
    fn set_live_output(&mut self, live: bool) {
        match self {
            Engine::Vm(s) => s.set_live_output(live),
            Engine::Interp(i) => i.set_live_output(live),
        }
    }

    fn set_network_allowed(&mut self, allowed: bool) {
        match self {
            Engine::Vm(s) => s.set_network_allowed(allowed),
            Engine::Interp(i) => i.set_network_allowed(allowed),
        }
    }

    fn network_allowed(&self) -> bool {
        match self {
            Engine::Vm(s) => s.network_allowed(),
            Engine::Interp(i) => i.network_allowed(),
        }
    }

    fn set_server_allowed(&mut self, allowed: bool) {
        match self {
            Engine::Vm(s) => s.set_server_allowed(allowed),
            Engine::Interp(i) => i.set_server_allowed(allowed),
        }
    }

    fn server_allowed(&self) -> bool {
        match self {
            Engine::Vm(s) => s.server_allowed(),
            Engine::Interp(i) => i.server_allowed(),
        }
    }

    fn allow_local_network_host(&mut self, host: &str) {
        match self {
            Engine::Vm(s) => s.allow_local_network_host(host),
            Engine::Interp(i) => i.allow_local_network_host(host),
        }
    }

    fn reset(&mut self, trusted: bool, network_allowed: bool, server_allowed: bool, live_output: bool) {
        match self {
            Engine::Vm(s) => {
                s.reset();
                s.set_live_output(live_output);
            }
            Engine::Interp(i) => {
                *i = Interpreter::new();
                i.set_trusted(trusted);
                i.set_network_allowed(network_allowed);
                i.set_server_allowed(server_allowed);
                i.set_live_output(live_output);
            }
        }
    }
}

pub struct Repl {
    engine: Engine,
    analyzer: SemanticAnalyzer,
    history: Vec<String>,
    // Accumulated lines of a multi-line block still being typed.
    pending: String,
    // Remembered because Interpreter does not expose its file
    // policy for reading; reset() must restore the same flag.
    trusted: bool,
    // Meeting 22: same pattern for the network flag — the
    // interpreter holds the process-wide gate, and reset()
    // must restore the same value.
    network_allowed: bool,
    // Meeting 27: same pattern for the server flag — the
    // engine holds it per instance, and reset() must restore
    // the same value (the network flag alone never opens it).
    server_allowed: bool,
    // اجتماع 32، السؤال الثامن: البث الحي لمخرجات
    // `اطبع` — النمط نفسه: المحرك يحمله لكل نسخة،
    // وreset() يعيد القيمة نفسها (وإلا ضاع عند :مسح).
    live_output: bool,
    // مجلد البرنامج (جذر الاستيراد والتشغيل) — يُحفظ
    // لأن `شغّل` يشغّل الملف منه؛ والجلسة نفسها تبنيه
    // من مجلد العمل الجاري (new) أو من المعطى (tests).
    program_dir: std::path::PathBuf,
    // Audit م-6: the control signal of the last fed line
    // (None / NetworkNeeded / SessionCleared), read by
    // `last_control()` — a side channel so run_repl can emit
    // it on stderr without touching the display text.
    last_control: ReplControl,
    // اجتماع 45 §59 (البند 19): تأكيد أعد-الضبط معلّق —
    // السطر التالي «نعم» حرفيا ينفّذه، وأي سطر آخر يلغيه.
    pending_reset: bool,
}

impl Repl {
    /// الجلسة الافتراضية: على المترجم (VM).
    pub fn new() -> Self {
        let program_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self {
            engine: Engine::Vm(VmSession::new(program_dir.clone())),
            analyzer: SemanticAnalyzer::new(),
            history: Vec::new(),
            pending: String::new(),
            trusted: false,
            network_allowed: false,
            server_allowed: false,
            live_output: false,
            program_dir,
            last_control: ReplControl::None,
            pending_reset: false,
        }
    }

    /// وضع التشخيص الخفي: الجلسة نفسها على مفسر الشجرة
    /// (CLI `--مفسر`) — للمقارنة والتشخيص فقط.
    pub fn new_interpreter() -> Self {
        let program_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self {
            engine: Engine::Interp(Interpreter::new()),
            analyzer: SemanticAnalyzer::new(),
            history: Vec::new(),
            pending: String::new(),
            trusted: false,
            network_allowed: false,
            server_allowed: false,
            live_output: false,
            program_dir,
            last_control: ReplControl::None,
            pending_reset: false,
        }
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        self.engine.set_trusted(trusted);
    }

    /// REPL rooted at a given directory (tests; the app and the
    /// CLI use the process working directory via new()).
    pub fn with_program_dir(program_dir: std::path::PathBuf) -> Self {
        let mut r = Self::new();
        r.engine = Engine::Vm(VmSession::new(program_dir.clone()));
        r.program_dir = program_dir;
        r
    }

    /// Like with_program_dir, on the interpreter (diagnostic).
    pub fn with_program_dir_interpreter(program_dir: std::path::PathBuf) -> Self {
        let mut r = Self::new_interpreter();
        r.engine = Engine::Interp(Interpreter::with_program_dir(program_dir.clone()));
        r.program_dir = program_dir;
        r
    }

    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.network_allowed = allowed;
        self.engine.set_network_allowed(allowed);
    }

    /// This session's own network permission (per-instance:
    /// one console's approval never leaks into another).
    pub fn network_allowed(&self) -> bool {
        self.engine.network_allowed()
    }

    /// Meeting 27: server capability (--اسمح-بالخادم).
    /// Default OFF; the network flag alone is not enough.
    pub fn set_server_allowed(&mut self, allowed: bool) {
        self.server_allowed = allowed;
        self.engine.set_server_allowed(allowed);
    }

    /// This session's own server permission (per-instance).
    pub fn server_allowed(&self) -> bool {
        self.engine.server_allowed()
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// (السطر يصل فور إنتاجه) — النمط نفسه كـ
    /// set_server_allowed؛ وreset() يعيده.
    pub fn set_live_output(&mut self, live: bool) {
        self.live_output = live;
        self.engine.set_live_output(live);
    }

    pub fn allow_local_network_host(&mut self, host: &str) {
        self.engine.allow_local_network_host(host);
    }

    /// رسالة الترحيب مفعّلة أم لا: تُقرأ من ملف الإعدادات
    /// الجديد (`~/إعدادات/niva.properties`، مع نقل القديم
    /// عند القراءة — اجتماع 45 §59، البند 36) — نفس صيغة
    /// التطبيق (سطور `مفتاح=قيمة`، `#` تعليق، فراغات
    /// تُهمل). المفتاح `الترحيب` بقيمة `لا` يوقفها؛
    /// وغياب الملف أو المفتاح أو أي قيمة أخرى يبقيها
    /// ظاهرة (الافتراضي: نعم). الملف قابل للتحرير من
    /// الترمنال نفسه.
    pub fn welcome_enabled(&self) -> bool {
        let (entries, _) = read_settings(&self.program_dir);
        for (key, value) in entries {
            if key == "الترحيب" {
                return value != "لا";
            }
        }
        true
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// The control signal of the last fed line (audit م-6).
    pub fn last_control(&self) -> ReplControl {
        self.last_control
    }

    /// The prompt to show now: the current directory,
    /// terminal style — `~ $ ` at home, `~/dir $ ` below it
    /// — or `> ` while a block is open (owner's orders
    /// 2026-11-05 ~22:58 and ~23:03: never `نيفا: `; `$`
    /// after the path, RTL display).
    pub fn prompt(&self) -> String {
        if self.pending.is_empty() {
            format!("{} $ ", self.engine.current_dir_display())
        } else {
            CONT_PROMPT.to_string()
        }
    }

    /// Evaluate a complete snippet and render its result as
    /// display text: stdout output, with any error appended. An
    /// Incomplete result means the block never completed, so it
    /// is dropped with the cancellation message.
    fn eval_display(&mut self, src: &str) -> String {
        match self.eval(src) {
            ReplEval::Done { output } => output,
            ReplEval::Failed { output, error } => format!("{}{}\n", output, error),
            ReplEval::Incomplete => "ألغيت الكتلة غير المكتملة.\n".to_string(),
        }
    }

    /// Run a parsed command line and return its display output.
    /// The line must already parse as a command.
    fn handle_command(&mut self, line: &str) -> ReplOutput {
        match parse_command(line) {
            Some(ReplCommand::Help) => ReplOutput {
                output: help_text().to_string(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
            Some(ReplCommand::Show) => ReplOutput {
                output: self.show_history(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
            Some(ReplCommand::ClearHistory) => {
                // Task 2 (Meeting 25 plan): clear ONLY the
                // :اعرض lines. The interpreter, the analyzer
                // and the variables all stay — unlike :مسح,
                // which resets the whole session. No control
                // signal: the app intercepts this command
                // itself and clears its own arrow history.
                self.history.clear();
                ReplOutput {
                    output: "مسحت محفوظات الجلسة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::Clear) => {
                self.reset();
                // Audit م-6: the host clears its arrow history.
                self.last_control = ReplControl::SessionCleared;
                ReplOutput {
                    output: "بدأت جلسة جديدة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::AllowNetwork) => {
                // Audit م-2: flip the network flag inside the
                // running process — no restart, no lost session
                // state. The app sends this line itself when the
                // user allows the network; it never appears in
                // the terminal text or in :اعرض.
                self.set_network_allowed(true);
                ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::AllowServer) => {
                // Meeting 27: the same in-process flip for the
                // server flag — the network flag alone is never
                // enough. The app sends this line itself when
                // the user allows the server.
                self.set_server_allowed(true);
                ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::Quit) => ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: true,
                needs_network: false,
            },
            None => ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
        }
    }

    /// Try a short terminal command (ات، مح، نم، شم، حم، حف،
    /// سم، قم، اف). Returns Some(output) when the first word is
    /// a shortcut code — including an arity error — and None
    /// when the line is not a shortcut, so it falls through
    /// to normal code. Only at the top level (never inside a
    /// pending block, where the line is block content).
    fn try_shortcut(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let code = words.next()?;
        let def = shortcut_def(code)?;
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        if args.len() < def.min || args.len() > def.max {
            let want = if def.min == def.max {
                if def.min == 0 {
                    "بدون معاملات".to_string()
                } else {
                    format!("{} معاملات", def.min)
                }
            } else if def.max == usize::MAX {
                format!("{} معاملات على الأقل", def.min)
            } else {
                format!("{} إلى {} معاملات", def.min, def.max)
            };
            return Some(ReplOutput {
                output: format!("{} تتوقع {} لكن أعطيت {}\n", code, want, args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        // Successful shortcut lines join the session history,
        // like code lines do, so :اعرض shows what was typed.
        // Session commands (اس) are control, not
        // content — like the :commands, they are not recorded.
        if !matches!(def.kind, ShortcutKind::Command) {
            self.history.push(line.to_string());
        }
        match def.kind {
            ShortcutKind::Command => Some(self.handle_command(def.key)),
            ShortcutKind::Snippet => {
                let snippet = match def.key {
                    "عشوائي" => "استورد عشوائي\nعشوائي.عدد_عشوائي()".to_string(),
                    "وقت" => "استورد وقت\nوقت.الآن()".to_string(),
                    "نوم" => match parse_shortcut_number(&args[0]) {
                        Some(n) => format!("استورد وقت\nوقت.نوم({})", n),
                        None => {
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع عددا، وليس «{}»\n", code, args[0]),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    },
                    "اجلب" => {
                        let escaped = args[0].replace('\\', "\\\\").replace('"', "\\\"");
                        format!("استورد شبكة\nشبكة.اجلب(\"{}\")", escaped)
                    }
                    "الحالية" => "استورد عمليات\nعمليات.الحالية()".to_string(),
                    // The program + its arguments join into the
                    // one string عمليات.أنشئ takes (split on
                    // spaces, no shell) — no comma anywhere.
                    "أنشئ_برنامج" => {
                        let joined = args.join(" ");
                        let escaped = joined.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("استورد عمليات\nعمليات.أنشئ(\"{}\")", escaped)
                    }
                    "أوقف_برنامج" => match parse_shortcut_number(&args[0]) {
                        Some(n) => format!("استورد عمليات\nعمليات.أوقف({})", n),
                        None => {
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع عددا، وليس «{}»\n", code, args[0]),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    },
                    _ => unreachable!("unknown snippet key"),
                };
                // A snippet whose value is لاشيء prints nothing,
                // like the other shortcuts do (Nil -> empty).
                let out = self.eval_display(&snippet);
                let out = if out == "لاشيء\n" { String::new() } else { out };
                Some(ReplOutput {
                    output: out,
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                })
            }
            ShortcutKind::RestJoined => {
                let values = vec![
                    Value::Str(args[0].clone()),
                    Value::Str(args[1..].join(" ")),
                ];
                match self.engine.call_builtin(def.key, values, 1, 1) {
                    Ok(value) => Some(ReplOutput {
                        output: format_shortcut_output(&value),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }),
                    Err(e) => Some(ReplOutput {
                        output: format!("{}\n", e),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }),
                }
            }
            ShortcutKind::StrArgs => {
                let values: Vec<Value> = args.into_iter().map(Value::Str).collect();
                match self.engine.call_builtin(def.key, values, 1, 1) {
                    Ok(value) => Some(ReplOutput {
                        output: format_shortcut_output(&value),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }),
                    Err(e) => Some(ReplOutput {
                        output: format!("{}\n", e),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }),
                }
            }
        }
    }

    /// Full-name terminal command (تثبيت): a whole Arabic
    /// word, not a two-letter code — so it never enters the
    /// 31-code table (the table stays 31, and the app has no
    /// code for it either). Like the Snippet shortcuts, it
    /// builds a builtin call and evaluates it; the typed line
    /// joins the history, like code lines do. Only at the top
    /// level, like try_shortcut — never inside a pending block.
    fn try_full_command(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let first = words.next()?;
        if first != "تثبيت" {
            return None;
        }
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        if args.len() != 1 {
            return Some(ReplOutput {
                output: format!("تثبيت تتوقع 1 معاملات لكن أعطيت {}\n", args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        self.history.push(line.to_string());
        let escaped = args[0].replace('\\', "\\\\").replace('"', "\\\"");
        let snippet = format!("تثبيت(\"{}\")", escaped);
        // A snippet whose value is لاشيء prints nothing,
        // like the other shortcuts do (Nil -> empty).
        let out = self.eval_display(&snippet);
        let out = if out == "لاشيء\n" { String::new() } else { out };
        Some(ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        })
    }

    /// أمر تفاعلي `شغّل <اسم>` (اجتماع 43 §58 + تجاوز
    /// المالك «بلا تعقيد»): يشغّل ملف أداة من مجلد
    /// البرنامج كبرنامج — الاسم العاري بلا امتداد يُلحق
    /// به `.نيفا` تلقائيا (`شغّل مثال-أول`)، والاسم
    /// الكامل بالامتداد كما هو (`شغّل مثال-أول.نيفا`)،
    /// ولا خطأ على الصيغتين. والاسم القانوني بلا تشكيل
    /// (`شغل`) كما في باقي أوامر CLI — والصيغة المشكولة
    /// (`شغّل`) مقبولة توافقا فقط عبر normalize_cli_word
    /// (قاعدة المحلل نفسها). تفاعلي فقط — لا دالة
    /// برنامجية (من استدعاه من الكود يأخذ خطأ الاسم غير
    /// المعرف المعتاد). فقط في المستوى الأعلى، كـ
    /// try_full_command.
    /// مصدر الفهرس الافتراضي (كما في حزم — مخزن.نيفا):
    /// سطر «مصدر الفهرس» في `معلومات`.
    const DEFAULT_INDEX_URL: &str =
        "https://raw.githubusercontent.com/RIO4140/niva-index/main/فهرس.حزمة";

    /// `معلومات` (اجتماع 45 §59، البندان 18 و78): خمسة
    /// أسطر بالضبط، بلا نسخ للحافظة.
    fn info_lines(&self) -> String {
        // الوحدات المثبتة: ملفات .نيفا في جذر مجلد
        // البرنامج (ما يثبّته `تثبيت`) — لا وحدات المخزن
        // (لحزم أمرها الخاص).
        let installed = std::fs::read_dir(&self.program_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|x| x == "نيفا")
                            .unwrap_or(false)
                            && e.metadata().map(|m| m.is_file()).unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        let terminal = match terminal_version() {
            Some(t) => eastern_version(&t),
            None => "غير معروف".to_string(),
        };
        let network = if self.network_connected() {
            "متصلة"
        } else {
            "غير متصلة"
        };
        format!(
            "محرك نيفا {}\nترمنال التطبيق: {}\nالوحدات المثبتة: {}\nالشبكة: {}\nمصدر الفهرس: {}\n",
            version(),
            terminal,
            Value::Number(installed as f64).fmt_value(),
            network,
            Self::DEFAULT_INDEX_URL
        )
    }

    /// الشبكة متصلة أم لا: بلا سماح بالشبكة فالجواب
    /// «غير متصلة» حتما (بلا فحص)؛ ومع السماح فحص خفيف:
    /// اتصال TCP قصير (3 ثوان) بمضيف مصدر الفهرس — أي
    /// اتصال ناجح يعني «متصلة»، والفشل يعني انقطاعا.
    fn network_connected(&self) -> bool {
        if !self.network_allowed() {
            return false;
        }
        use std::net::ToSocketAddrs;
        let Ok(mut addrs) = ("raw.githubusercontent.com", 443).to_socket_addrs() else {
            return false;
        };
        let Some(addr) = addrs.next() else {
            return false;
        };
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)).is_ok()
    }

    /// `أعد-الإعدادات` (اجتماع 45 §59، البند 19): أعد
    /// قراءة niva.properties بلا إعادة تشغيل، وأرسل إشارة
    /// `إعدادات` للتطبيق ليعيد التطبيق الحي؛ وسطر تأكيد
    /// واحد يسمّي المفاتيح التي تحتاج إعادة تشغيل.
    fn reload_settings(&mut self) -> ReplOutput {
        let (_, notices) = read_settings(&self.program_dir);
        self.last_control = ReplControl::SettingsReload;
        // المفاتيح بلا مسار تطبيق حي (من قراءة loadSettings):
        // تُسمّى في الرسالة بأنها تحتاج إعادة تشغيل؛
        // والترحيب يسري من الجلسة الجديدة.
        let mut out = String::from(
            "أُعيدت قراءة الإعدادات وطُبّقت — وتحتاج إعادة تشغيل: إخفاء_لوحة_المفاتيح_عند_البدء، خط_مخصص، ملء_الشاشة؛ والترحيب يسري من الجلسة الجديدة\n",
        );
        for n in notices {
            out.push_str(&n);
            out.push('\n');
        }
        ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// `أعد-الضبط` — الخطوة الأولى (اجتماع 45 §59، البند
    /// 19): رسالة تسمّي ما سيُمسح وما يبقى، وتطلب كتابة
    /// «نعم» حرفيا؛ وأي سطر آخر يلغي.
    fn ask_reset(&mut self) -> ReplOutput {
        self.pending_reset = true;
        // الجلسات المحفوظة إن كانت في مجلد البرنامج
        // (session-* في الجذر) — تُسمّى بعددها.
        let sessions = std::fs::read_dir(&self.program_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("session-"))
                    .count()
            })
            .unwrap_or(0);
        let sessions_part = if sessions > 0 {
            format!(
                "، ومنها {} جلسة محفوظة",
                Value::Number(sessions as f64).fmt_value()
            )
        } else {
            String::new()
        };
        ReplOutput {
            output: format!(
                "سيُمسح مجلد البرنامج كاملا ما عدا البيت (~) — الوحدات المثبتة والملفات الداخلية{}؛ وتبقى ملفاتك في البيت (~) والإعدادات في ~/إعدادات/. اكتب نعم حرفيا للتأكيد، وأي شيء آخر يلغي.\n",
                sessions_part
            ),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// `أعد-الضبط` — الخطوة الثانية بعد «نعم»: امسح مجلد
    /// البرنامج كاملا ما عدا home/، وأعد بناء المشتقات في
    /// الخطوة نفسها (البند 52)، وأعد مجلد الجلسة إلى البيت.
    /// الإعدادات تعيش في home/ فلا تُمس.
    fn do_reset(&mut self) -> ReplOutput {
        let policy = crate::permissions::FilePolicy::restricted(self.program_dir.clone());
        let mut failed: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_name() == "home" {
                    continue;
                }
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if res.is_err() {
                    failed.push(policy.display_path(&path));
                }
            }
        }
        // (52) إعادة بناء المشتقات في الخطوة نفسها.
        let _ = std::fs::create_dir_all(self.program_dir.join("tmp"));
        let _ = std::fs::create_dir_all(self.program_dir.join(".نيفا"));
        // الجلسة تعود إلى البيت (مجلدها قد يكون في
        // المنطقة الممسوحة).
        let _ = self
            .engine
            .call_builtin("انتقل", vec![Value::Str("~".to_string())], 1, 1);
        // سلامة الإعدادات بعد المسح (تعيش في home/):
        // الملف غائب (افتراضي) أو مقروء = سليم.
        let settings_ok = match std::fs::read_to_string(&settings_path(&self.program_dir)) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        let mut out = if failed.is_empty() {
            "أُعيد ضبط مجلد البرنامج — وبقي البيت (~) كما هو، ومجلد الجلسة الآن البيت.".to_string()
        } else {
            format!(
                "أُعيد ضبط مجلد البرنامج جزئيا — تعذّر مسح: {}.",
                failed.join("، ")
            )
        };
        if !settings_ok {
            out.push_str(" تنبيه: ملف الإعدادات غير قابل للقراءة بعد المسح.");
        }
        out.push('\n');
        ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    fn try_run_command(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let first = words.next()?;
        if normalize_cli_word(first) != "شغل" {
            return None;
        }
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        let bad = |msg: &str| ReplOutput {
            output: format!("{}\n", msg),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        };
        if args.len() != 1 {
            return Some(bad("شغّل تتوقع معاملا واحدا"));
        }
        let name = &args[0];
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Some(bad("شغّل يقبل اسم ملف بلا مسار"));
        }
        let file_name = if name.ends_with(".نيفا") {
            name.clone()
        } else {
            format!("{}.نيفا", name)
        };
        self.history.push(line.to_string());
        Some(self.run_program_file(&file_name))
    }

    /// اقرأ ملف `.نيفا` — من مجلد البرنامج أولا (المثبَّت)،
    /// ثم من البيت (اجتماع 45 §59، البند 35: المثبَّت
    /// يفوز عند التعارض) — وشغّله كبرنامج في جلسة جديدة
    /// على محرك التفاعلي الحالي (افتراضي أو مفسر) — جلسة
    /// التفاعلي نفسها لا تُمس ولا يتسرب شيء في أي اتجاه.
    /// المخرجات تُبث حية (أو تُجمع في ناتج الإرجاع حين
    /// يكون البث معطلا كما في الاختبارات)؛ وخطأ وقت
    /// التشغيل يُطبع بالعربية ويعود التحكم إلى الموجه
    /// بلا خروج من التفاعلي.
    fn run_program_file(&mut self, name: &str) -> ReplOutput {
        // مجلد البرنامج أولا، ثم البيت؛ ورسالة الغياب
        // تسمّي المكانين.
        let mut path = self.program_dir.join(name);
        if !std::fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
            let home_path = self.program_dir.join("home").join(name);
            if std::fs::metadata(&home_path)
                .map(|m| m.is_file())
                .unwrap_or(false)
            {
                path = home_path;
            }
        }
        let is_file = std::fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            return ReplOutput {
                output: format!(
                    "الملف {} غير موجود — بحثت في $نيفا وفي ~\n",
                    name
                ),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                return ReplOutput {
                    output: format!("تعذر قراءة الملف: {}\n", name),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
        };
        let fail = |e: String| ReplOutput {
            output: format!("{}\n", e),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        };
        let tokens = match tokenize(&source) {
            Ok(t) => t,
            Err(e) => return fail(e.to_string()),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return fail(e.to_string()),
        };
        // محلل دلالي جديد للبرنامج — لا محلل الجلسة —
        // فتعريفات البرنامج لا تتسرب إلى التفاعلي.
        let mut analyzer = SemanticAnalyzer::new();
        if let Err(e) = analyzer.analyze_program(&program) {
            return fail(e.to_string());
        }
        // جلسة جديدة على محرك التفاعلي الحالي — بأعلام
        // الجلسة نفسها (موثوق/شبكة/خادم/بث حي)، فالبرنامج
        // يعمل كما يعمل `نيفا <ملف>` بالأعلام نفسها.
        let output = match &self.engine {
            Engine::Vm(_) => {
                let mut session = VmSession::new(self.program_dir.clone());
                session.set_trusted(self.trusted);
                session.set_network_allowed(self.network_allowed);
                session.set_server_allowed(self.server_allowed);
                session.set_live_output(self.live_output);
                session.set_source_file(name);
                match session.run(&program) {
                    Ok(()) => session.take_output(),
                    Err(e) => {
                        let mut out = session.take_output();
                        out.push_str(&format!("{}\n", e));
                        out
                    }
                }
            }
            Engine::Interp(_) => {
                let mut interp =
                    Interpreter::with_program_dir(self.program_dir.clone());
                interp.set_trusted(self.trusted);
                interp.set_network_allowed(self.network_allowed);
                interp.set_server_allowed(self.server_allowed);
                interp.set_live_output(self.live_output);
                interp.set_source_file(name);
                match interp.run(&program) {
                    Ok(()) => std::mem::take(&mut interp.output),
                    Err(e) => {
                        let mut out = std::mem::take(&mut interp.output);
                        out.push_str(&format!("{}\n", e));
                        out
                    }
                }
            }
        };
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// ملف البدء والمقتطفات (اجتماع 45 §59، البنود 44 و45
    /// و46 و111): `~/بدء.نيفا` ثم `~/مقتطفات/*.نيفا`
    /// أبجديا، عند دخول التفاعلي فقط — لا في تشغيل ملف
    /// ولا في `شغّل`. الخطأ في ملف لا يمنع الدخول أبدا:
    /// يُطبع الخطأ باسم الملف، ويكمل الباقي، ثم يظهر
    /// الموجه. التعريفات تبقى في الجلسة.
    fn run_startup_files(&mut self) -> String {
        let home = self.program_dir.join("home");
        let mut out = String::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        let start = home.join("بدء.نيفا");
        if start.is_file() {
            files.push(("~/بدء.نيفا".to_string(), start));
        }
        let snippets = home.join("مقتطفات");
        if let Ok(entries) = std::fs::read_dir(&snippets) {
            let mut names: Vec<(String, PathBuf)> = entries
                .flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == "نيفا")
                        .unwrap_or(false)
                        && e.metadata().map(|m| m.is_file()).unwrap_or(false)
                })
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    (format!("~/مقتطفات/{}", name), e.path())
                })
                .collect();
            names.sort_by(|a, b| a.0.cmp(&b.0));
            files.extend(names);
        }
        for (display, path) in files {
            match std::fs::read_to_string(&path) {
                Ok(source) => match self.eval(&source) {
                    ReplEval::Done { output } => out.push_str(&output),
                    ReplEval::Failed { output, error } => {
                        out.push_str(&output);
                        out.push_str(&format!("خطأ في {}: {}\n", display, error));
                    }
                    ReplEval::Incomplete => {
                        out.push_str(&format!("خطأ في {}: الملف غير مكتمل\n", display));
                    }
                },
                Err(_) => {
                    out.push_str(&format!("تعذّر قراءة الملف: {}\n", display));
                }
            }
        }
        out
    }

    /// استدعاء دالة مدمجة بمعاملات نصية، لعبارات كاملة:
    /// مثل ذراع StrArgs في try_shortcut — بلا مقتطف
    /// وبلا فصلة في أي مكان.
    fn call_str_builtin(&mut self, key: &str, args: Vec<String>) -> ReplOutput {
        let values: Vec<Value> = args.into_iter().map(Value::Str).collect();
        match self.engine.call_builtin(key, values, 1, 1) {
            Ok(value) => ReplOutput {
                output: format_shortcut_output(&value),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
            Err(e) => ReplOutput {
                output: format!("{}\n", e),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
        }
    }

    /// استدعاء مدمج بمعامل واحد يُحلَّل عددا أولا (أرقام
    /// عربية + ٫) فيمرَّر عددا، وإلا مرَّر النص كما هو —
    /// سلوك حع القديم في «حول إلى عدد»: عدد(نص) لا تقبل
    /// ٫، فبدون هذا التحويل ينكسر «حول إلى عدد ٣٫٥»
    /// الذي كان يعمل. (إصلاح من التحقق المستقل 2026-11-08.)
    fn call_num_or_str_builtin(&mut self, key: &str, arg: String) -> ReplOutput {
        let value = match parse_shortcut_number(&arg) {
            Some(n) => Value::Number(n),
            None => Value::Str(arg),
        };
        match self.engine.call_builtin(key, vec![value], 1, 1) {
            Ok(value) => ReplOutput {
                output: format_shortcut_output(&value),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
            Err(e) => ReplOutput {
                output: format!("{}\n", e),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
        }
    }

    /// العبارات الكاملة (2026-11-08، أمر المالك «اكتب دول
    /// كاملين»): 6 عبارات محرك بدلا من أكوادها المحذوفة
    /// — بم/بع/مس/ال/كا هنا، وحع هنا («حول إلى عدد»)؛
    /// ومع/اد/اق/مى بلا عبارة: دوالها العادية
    /// مجموع/أدنى/أقصى/مدى تكفي في الكود. وأوامر كاملة
    /// بكلمة واحدة (2026-11-08 ~20:00، أمر المالك:
    /// كلمة واحدة 4–5 حروف بلا ألف في النصف لا تحتاج
    /// اختصارا): انتقل/انسخ/انقل/موجود بدلا من أكوادها
    /// المحذوفة نم/سم/قم/وج — وعر/اط خرجا بلا بديل هنا
    /// لأن :اعرض واطبع يعملان أصلا بصيغتهما الكاملة.
    /// مطابقة بادئة للسطر كله: الكلمات الأولى هي العبارة
    /// للسطر كله: الكلمات الأولى هي العبارة والباقي
    /// معاملات مفصولة بمسافة بلا فصلة أبدا، وعدد
    /// المعاملات ورسائل الخطأ بنفس أسلوب الاختصارات
    /// («تتوقع N معاملات لكن أعطيت M»). «حول إلى عدد»
    /// بلا شدة على الواو (قاعدة بلا-تشكيل). تُفحص قبل
    /// line_is_code (كـ parse_command) فلا يبتلعها اسم
    /// معرّف من المستخدم: «ارجع للبيت» لأن ارجع كلمة
    /// مفتاحية، و«كرر الأخير» لأن كرر اسم موجود.
    /// «امسح الجلسة» محرك لا تطبيق: التطبيق يمررها
    /// إلى المحرك كما هي ولا يعترضها.
    fn try_full_phrase(&mut self, line: &str) -> Option<ReplOutput> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (phrase, nwords, min, max) = match words.as_slice() {
            ["ابحث", "في", "ملف", ..] => ("ابحث في ملف", 3, 2, 2),
            ["ابحث", "عن", "ملف", ..] => ("ابحث عن ملف", 3, 1, 1),
            ["امسح", "الجلسة", ..] => ("امسح الجلسة", 2, 0, 0),
            ["حول", "إلى", "عدد", ..] => ("حول إلى عدد", 3, 1, 1),
            ["ارجع", "للبيت", ..] => ("ارجع للبيت", 2, 0, 0),
            ["كرر", "الأخير", ..] => ("كرر الأخير", 2, 0, 0),
            ["انتقل", ..] => ("انتقل", 1, 1, 1),
            ["انسخ", ..] => ("انسخ", 1, 2, 2),
            ["انقل", ..] => ("انقل", 1, 2, 2),
            ["موجود", ..] => ("موجود", 1, 1, 1),
            // اجتماع 45 §59 (البنود 18 و19 و78): أوامر
            // كاملة بكلمة واحدة، بلا معاملات.
            ["معلومات", ..] => ("معلومات", 1, 0, 0),
            ["أعد-الإعدادات", ..] => ("أعد-الإعدادات", 1, 0, 0),
            ["أعد-الضبط", ..] => ("أعد-الضبط", 1, 0, 0),
            _ => return None,
        };
        let args: Vec<String> = words[nwords..].iter().map(|w| w.to_string()).collect();
        if args.len() < min || args.len() > max {
            let want = if min == max {
                if min == 0 {
                    "بدون معاملات".to_string()
                } else {
                    format!("{} معاملات", min)
                }
            } else if max == usize::MAX {
                format!("{} معاملات على الأقل", min)
            } else {
                format!("{} إلى {} معاملات", min, max)
            };
            return Some(ReplOutput {
                output: format!("{} تتوقع {} لكن أعطيت {}\n", phrase, want, args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        match phrase {
            "ابحث في ملف" => {
                self.history.push(line.to_string());
                Some(self.call_str_builtin("ابحث_في_ملف", args))
            }
            "ابحث عن ملف" => {
                self.history.push(line.to_string());
                Some(self.call_str_builtin("ابحث_عن", args))
            }
            "امسح الجلسة" => Some(self.handle_command(":مسح")),
            "انتقل" | "انسخ" | "انقل" | "موجود" => {
                self.history.push(line.to_string());
                Some(self.call_str_builtin(phrase, args))
            }
            "معلومات" => {
                self.history.push(line.to_string());
                Some(ReplOutput {
                    output: self.info_lines(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                })
            }
            "أعد-الإعدادات" => {
                self.history.push(line.to_string());
                Some(self.reload_settings())
            }
            "أعد-الضبط" => {
                self.history.push(line.to_string());
                Some(self.ask_reset())
            }
            "حول إلى عدد" => {
                self.history.push(line.to_string());
                // المعامل الواحد مضمون بفحص العدد أعلاه.
                let arg = args.into_iter().next().unwrap();
                Some(self.call_num_or_str_builtin("عدد", arg))
            }
            "ارجع للبيت" => {
                self.history.push(line.to_string());
                // البيت "~" في كل مكان في نيفا (سياسة
                // الملفات تحله إلى مجلد البرنامج).
                Some(self.call_str_builtin("انتقل", vec!["~".to_string()]))
            }
            "كرر الأخير" => {
                // كرر الأخير (!!): إعادة تنفيذ آخر سطر في
                // المحفوظات — آخر سطر ليس «كرر الأخير»
                // نفسها أبدا، ولا سطر «كا» قديم من جلسة
                // قبل الحذف؛ وبلا سطر سابق: الخطأ العربي
                // القصير نفسه. العبارة نفسها لا تُسجَّل:
                // السطر المعاد تنفيذه يسجّل نفسه في مساره.
                let last = self
                    .history
                    .iter()
                    .rev()
                    .find(|l| {
                        let w: Vec<&str> = l.split_whitespace().collect();
                        w != ["كرر", "الأخير"] && w.first() != Some(&"كا")
                    })
                    .cloned();
                match last {
                    Some(prev) => Some(self.feed_inner(&prev)),
                    None => Some(ReplOutput {
                        output: "خطأ: لا يوجد سطر سابق لإعادة تنفيذه\n".to_string(),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }),
                }
            }
            _ => unreachable!("unknown full phrase"),
        }
    }

    /// Feed one typed line and get what to display next. The
    /// wrapper flags the network-permission error so an
    /// interactive console can offer the first-use prompt.
    pub fn feed(&mut self, line: &str) -> ReplOutput {
        self.last_control = ReplControl::None;
        let mut out = self.feed_inner(line);
        out.needs_network = out
            .output
            .contains(crate::network::PERMISSION_ERROR_MARKER);
        if out.needs_network {
            // Audit م-6: the host offers the permission prompt
            // on this structured signal, not on the text.
            self.last_control = ReplControl::NetworkNeeded;
        }
        // Meeting 27: the same structured signal for the
        // server-permission error — the host offers the
        // first-use server prompt on it, not on the text.
        if out
            .output
            .contains(crate::network::SERVER_PERMISSION_MARKER)
        {
            self.last_control = ReplControl::ServerNeeded;
        }
        out
    }

    /// Audit م-12: a line that parses as Niva code runs as
    /// code — shortcut codes must not shadow names. The check
    /// is a trial semantic analysis on a CLONE of the
    /// analyzer: parse-only is not enough (a shortcut-shaped
    /// line can parse as code yet fail analysis while its
    /// name is undefined). One line of code is normally one statement,
    /// so a multi-statement line is shortcut-shaped. A bare
    /// reference to a shortcut-named variable falls through to
    /// the shortcut only while no such variable exists; once
    /// defined, the name is code. The clone keeps the trial
    /// side-effect free (the real eval analyzes again).
    fn line_is_code(&self, line: &str) -> bool {
        let tokens = match tokenize(line) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(_) => return false,
        };
        // One line of Niva code is normally one statement. A
        // line that parses as several statements (e.g. "نم
        // جديد" — two bare names) is shortcut-shaped: let the
        // shortcut layer own it, and fall back to eval after.
        if program.len() != 1 {
            return false;
        }
        let mut trial = self.analyzer.clone();
        trial.analyze_program(&program).is_ok()
    }

    /// تأكيد أعد-الضبط المعلّق (اجتماع 45 §59، البند 19):
    /// السطر التالي «نعم» حرفيا ينفّذ المسح، وأي سطر آخر
    /// يلغي ويُعالج عاديا (مع سطر الإلغاء قبله).
    fn feed_inner(&mut self, line: &str) -> ReplOutput {
        if self.pending_reset {
            self.pending_reset = false;
            if line.trim() == "نعم" {
                self.history.push(line.to_string());
                return self.do_reset();
            }
            let mut out = self.feed_inner_impl(line);
            out.output = format!("أُلغي إعادة الضبط.\n{}", out.output);
            return out;
        }
        self.feed_inner_impl(line)
    }

    /// The exact logic of the old `run_repl` loop, moved into
    /// the struct so the native loop and the WASM console share
    /// it: an empty line while a block is pending cancels it; a
    /// command line never belongs to the block; an indented line
    /// only waits; only a line at indent 0 can finish the block.
    fn feed_inner_impl(&mut self, line: &str) -> ReplOutput {
        let line = line.trim_end_matches(['\n', '\r']).to_string();
        if !self.pending.is_empty() {
            if line.trim().is_empty() {
                self.pending.clear();
                return ReplOutput {
                    output: "ألغيت الكتلة غير المكتملة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            if parse_command(&line).is_some() {
                let pending = std::mem::take(&mut self.pending);
                let mut output = self.eval_display(&pending);
                let cmd = self.handle_command(&line);
                output.push_str(&cmd.output);
                return ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: cmd.ended,
                    needs_network: false,
                };
            }
            if line.starts_with([' ', '\t']) {
                self.pending.push('\n');
                self.pending.push_str(&line);
                return ReplOutput {
                    output: String::new(),
                    prompt: CONT_PROMPT.to_string(),
                    ended: false,
                    needs_network: false,
                };
            }
            self.pending.push('\n');
            self.pending.push_str(&line);
            let pending = std::mem::take(&mut self.pending);
            return match self.eval(&pending) {
                ReplEval::Incomplete => {
                    self.pending = pending;
                    ReplOutput {
                        output: String::new(),
                        prompt: CONT_PROMPT.to_string(),
                        ended: false,
                        needs_network: false,
                    }
                }
                ReplEval::Done { output } => ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                },
                ReplEval::Failed { output, error } => ReplOutput {
                    output: format!("{}{}\n", output, error),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                },
            };
        }
        if line.trim().is_empty() {
            return ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        if parse_command(&line).is_some() {
            return self.handle_command(&line);
        }
        // العبارات الكاملة (2026-11-08): قبل line_is_code
        // كـ parse_command — فلا يبتلعها اسم معرّف؛
        // وتثبيت يبقى في موضعه بعد line_is_code بلا تغيير.
        if let Some(out) = self.try_full_phrase(&line) {
            return out;
        }
        // Audit م-12 (code first): a line that parses as Niva
        // code runs as code — shortcut codes never shadow
        // names. Only when the line is not code does the
        // shortcut layer get its turn.
        if !self.line_is_code(&line) {
            if let Some(out) = self.try_shortcut(&line) {
                return out;
            }
            if let Some(out) = self.try_full_command(&line) {
                return out;
            }
            if let Some(out) = self.try_run_command(&line) {
                return out;
            }
        }
        match self.eval(&line) {
            ReplEval::Incomplete => {
                self.pending.push_str(&line);
                ReplOutput {
                    output: String::new(),
                    prompt: CONT_PROMPT.to_string(),
                    ended: false,
                    needs_network: false,
                }
            }
            ReplEval::Done { output } => ReplOutput {
                output,
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
            ReplEval::Failed { output, error } => ReplOutput {
                output: format!("{}{}\n", output, error),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
        }
    }

    /// EOF (Ctrl-D): the loop prints a newline and exits cleanly.
    pub fn eof(&mut self) -> ReplOutput {
        ReplOutput {
            output: "\n".to_string(),
            prompt: self.prompt(),
            ended: true,
            needs_network: false,
        }
    }

    /// Evaluate one complete snippet (possibly multi-line).
    /// Incomplete input returns Incomplete and changes nothing.
    pub fn eval(&mut self, src: &str) -> ReplEval {
        let tokens = match tokenize(src) {
            Ok(t) => t,
            Err(e) => {
                return ReplEval::Failed {
                    output: String::new(),
                    error: e.to_string(),
                }
            }
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                if INCOMPLETE_MARKERS.iter().any(|m| msg.contains(m)) {
                    return ReplEval::Incomplete;
                }
                return ReplEval::Failed {
                    output: String::new(),
                    error: msg,
                };
            }
        };
        if let Err(e) = self.analyzer.analyze_program(&program) {
            return ReplEval::Failed {
                output: String::new(),
                error: e.to_string(),
            };
        }
        // Complete input: record it before running, so a runtime
        // error does not erase it from the session history.
        self.history.push(src.to_string());
        // A bare expression prints its value — including a trailing
        // expression after other statements (a function definition
        // followed by its call, the common REPL flow). Run the
        // preceding statements first, then evaluate the trailing
        // expression once and print it.
        if let Some(StmtKind::ExprStmt { expr }) = program.last().map(|s| &s.kind) {
            let (head, _) = program.split_at(program.len() - 1);
            if let Err(e) = self.engine.run(head) {
                return ReplEval::Failed {
                    output: self.engine.take_output(),
                    error: e.to_string(),
                };
            }
            let printed = self.engine.take_output();
            return match self.engine.eval_one(expr) {
                Ok(v) => {
                    // قيمة فارغة (كقيمة تثبيت-الكل) لا تطبع
                    // سطرا فارغا بعدها — المخرج يبقى ما طُبع
                    // فقط، بلا `\n` زائدة.
                    let value = v.fmt_value();
                    let output = if value.is_empty() {
                        printed
                    } else {
                        format!("{}{}\n", printed, value)
                    };
                    ReplEval::Done { output }
                }
                Err(e) => ReplEval::Failed {
                    output: printed,
                    error: e.to_string(),
                },
            };
        }
        match self.engine.run(&program) {
            Ok(()) => ReplEval::Done {
                output: self.engine.take_output(),
            },
            Err(e) => ReplEval::Failed {
                output: self.engine.take_output(),
                error: e.to_string(),
            },
        }
    }

    /// Session history, each line wrapped in bidi isolates so a
    /// terminal renders it as one isolated run (RTL display aid).
    pub fn show_history(&self) -> String {
        let mut out = String::from("ما كتبته في هذه الجلسة:\n");
        for line in &self.history {
            for part in line.lines() {
                out.push(RLI);
                out.push_str(part);
                out.push(PDI);
                out.push('\n');
            }
        }
        out
    }

    pub fn reset(&mut self) {
        let trusted = self.trusted;
        let network_allowed = self.network_allowed;
        let server_allowed = self.server_allowed;
        let live_output = self.live_output;
        self.engine
            .reset(trusted, network_allowed, server_allowed, live_output);
        self.analyzer = SemanticAnalyzer::new();
        self.history.clear();
        self.pending.clear();
    }
}

/// نص الإصدار المجرد وحده (مثال «١٫٠٫٠») — المصدر
/// الوحيد لرقم الإصدار؛ واللافتة تُبنى منه، فلا يفترقان
/// أبدا (اجتماع 32، السؤال السادس: `إصدار_المحرك()`).
pub fn version() -> &'static str {
    "١٫٠٫٠"
}

/// The greeting printed when a REPL session starts — the same
/// text the WASM console shows on load. Minimal by the owner's
/// no-explanation rule (2026-11-05): name + version only, like
/// Termux/Linux — no teaching, no how-to, no docs links.
/// نسخة الترمنال المضيف (بلاغ 2026-11-08: `اص` يعرض
/// نسخة المحرك ١٫٠٫٠ لا نسخة الترمنال الحقيقية).
/// التطبيق يمرّر نسخته في `NIVA_TERMINAL_VERSION`
/// (مثلا "1.2.11") عند تشغيل المحرك؛ والمحرك يقبلها
/// أرقاما ونقاطا لاتينية فقط، وإلا تجاهلها.
fn terminal_version() -> Option<String> {
    let raw = std::env::var("NIVA_TERMINAL_VERSION").ok()?;
    let raw = raw.trim();
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some(raw.to_string())
}

/// أرقام لاتينية ونقطة عشرية إلى أرقام مشرقية وفاصلة
/// عشرية عربية (مثلا "1.2.11" ← «١٫٢٫١١»).
fn eastern_version(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '0'..='9' => {
                char::from_u32('٠' as u32 + (c as u32 - '0' as u32)).unwrap_or(c)
            }
            '.' => '٫',
            other => other,
        })
        .collect()
}

/// اللافتة لنسخة ترمنال معطاة (دالة خالصة للاختبار):
/// بلا ترمنال — لافتة المحرك كما هي؛ ومعه — «ترمنال
/// نيفا X (محرك Y)» فيبقى رقم المحرك ظاهرا ولا يضيع.
/// `إصدار_المحرك()` لا يمسّه هذا أبدا (نسخة اللغة
/// للتوافق، لا نسخة التطبيق).
pub fn banner_for(terminal: Option<&str>) -> String {
    match terminal {
        Some(t) => format!(
            "ترمنال نيفا {} (محرك {})",
            eastern_version(t),
            version()
        ),
        None => format!("محرك نيفا {}", version()),
    }
}

pub fn banner() -> String {
    banner_for(terminal_version().as_deref())
}

/// رسالة الترحيب: تُطبع بعد اللافتة عند بدء التفاعلي
/// فقط — لا في تشغيل ملف ولا مع `--نسخه`. عربية كاملة
/// مختصرة: الوثائق والإبلاغ والأوامر اليومية. بلا سطر
/// مجتمع (لا قناة مخصصة لنيفا بعد — يُضاف حين يحددها
/// المالك، ولا يُختلق رابط).
pub fn welcome_text() -> String {
    "أهلا بك في نيفا\n\nالوثائق: https://github.com/RIO4140/niva\nبلّغ عن مشكلة: https://github.com/RIO4140/niva/issues\n\nأوامر يومية:\n ات — اعرض الملفات\n شغل <اسم> — شغّل ملف\n تثبيت <رابط> — ثبّت أداة\n تن — حدّث التطبيق".to_string()
}

/// `:مساعدة` output: a BARE command list — terminal convention,
/// not teaching. No explanations, no examples, no notes.
pub fn help_text() -> &'static str {
    ":مساعدة\n:اعرض\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:خروج\nمعلومات\nأعد-الإعدادات\nأعد-الضبط\n"
}

pub enum ReplCommand {
    Help,
    Show,
    /// Task 2 (Meeting 25 plan): clear only the :اعرض
    /// history lines — variables and functions stay. The
    /// app's own :امسح-المحفوظات (v1.1.6) clears the app's
    /// arrow-key history; the engine honors the same name
    /// here for its own history (desktop / Termux / WASM).
    ClearHistory,
    Clear,
    /// Audit م-2: allow the network inside the running
    /// process (no restart). Sent by the host app on approval.
    AllowNetwork,
    /// Meeting 27: allow the server inside the running
    /// process (no restart). Sent by the host app on approval.
    AllowServer,
    Quit,
}

/// Short terminal commands: code -> (builtin, min args, max
/// args). The single place to change the codes if the owner
/// corrects them; the builtins are the implementation.
/// Rule (owner 2026-11-05 ~22:05): a code is one easy letter
/// from the first word + one easy letter from the second word
/// — ات = اعرض ملفات، حم = احذف مجلد.
/// How a shortcut's space-separated arguments are handled.
enum ShortcutKind {
    /// All arguments as strings (file builtins).
    StrArgs,
    /// First argument is a path, the rest joined with spaces
    /// as one string (اكتب_ملف content may hold spaces).
    RestJoined,
    /// Evaluate a constructed stdlib snippet (رياضيات /
    /// عشوائي / وقت / شبكة / عمليات).
    Snippet,
    /// A REPL command (:مساعدة / :اعرض / :مسح / :خروج).
    Command,
}

/// One row of the 31-shortcut table
/// (~/workspace/android-app/SHORTCUTS-31.md): the code, the
/// builtin or snippet key, the argument count range, and the
/// kind. App-level codes (جلسات، مسح الشاشة، ...) are NOT
/// here: the app intercepts them before sending the line to
/// the engine, so they never reach this table.
struct ShortcutDef {
    key: &'static str,
    min: usize,
    max: usize,
    kind: ShortcutKind,
}

fn shortcut_def(code: &str) -> Option<ShortcutDef> {
    Some(match code {
        // --- ملفات ومجلدات (18) ---
        "ات" => ShortcutDef { key: "اعرض_الملفات", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "مح" => ShortcutDef { key: "المجلد_الحالي", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "شم" => ShortcutDef { key: "أنشئ_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حم" => ShortcutDef { key: "احذف_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حف" => ShortcutDef { key: "احذف_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "اف" => ShortcutDef { key: "اقرأ_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شف" => ShortcutDef { key: "أنشئ_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عب" => ShortcutDef { key: "اعرض_البداية", min: 1, max: 2, kind: ShortcutKind::StrArgs },
        "عن" => ShortcutDef { key: "اعرض_النهاية", min: 1, max: 2, kind: ShortcutKind::StrArgs },
        "عس" => ShortcutDef { key: "عد_الأسطر", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عك" => ShortcutDef { key: "عد_الكلمات", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عش" => ShortcutDef { key: "اعرض_الشجرة", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        "حج" => ShortcutDef { key: "حجم_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شر" => ShortcutDef { key: "أنشئ_رابط", min: 2, max: 2, kind: ShortcutKind::StrArgs },
        "اك" => ShortcutDef { key: "اكتب_ملف", min: 2, max: usize::MAX, kind: ShortcutKind::RestJoined },
        "ام" => ShortcutDef { key: "نوع_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "تم" => ShortcutDef { key: "تفاصيل_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عم" => ShortcutDef { key: "عد_الملفات", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        // --- أوامر الجلسة (1) ---
        "اس" => ShortcutDef { key: ":مساعدة", min: 0, max: 0, kind: ShortcutKind::Command },
        // --- إصدار المحرك (1) ---
        "اص" => ShortcutDef { key: "النسخة", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        // --- نص وأعداد (1) ---
        "رع" => ShortcutDef { key: "عشوائي", min: 0, max: 0, kind: ShortcutKind::Snippet },
        // --- وقت وشبكة (4) ---
        "تا" => ShortcutDef { key: "التاريخ_الآن", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "وق" => ShortcutDef { key: "وقت", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نو" => ShortcutDef { key: "نوم", min: 1, max: 1, kind: ShortcutKind::Snippet },
        "اج" => ShortcutDef { key: "اجلب", min: 1, max: 1, kind: ShortcutKind::Snippet },
        // --- عمليات (3) ---
        "عع" => ShortcutDef { key: "الحالية", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نب" => ShortcutDef { key: "أنشئ_برنامج", min: 1, max: usize::MAX, kind: ShortcutKind::Snippet },
        "اب" => ShortcutDef { key: "أوقف_برنامج", min: 1, max: 1, kind: ShortcutKind::Snippet },
        _ => return None,
    })
}

/// Parse a shortcut argument as a number: Arabic digits
/// (٠-٩) and the Arabic decimal separator (٫) are cleaned
/// first, like the عدد builtin does.
fn parse_shortcut_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
            '٫' => '.',
            other => other,
        })
        .collect();
    cleaned.parse::<f64>().ok()
}

/// Display a shortcut result the terminal way: a file list
/// prints one name per line, a path prints alone, a لاشيء
/// result prints nothing.
fn format_shortcut_output(value: &Value) -> String {
    match value {
        Value::List(items) => {
            let mut out = String::new();
            for item in items {
                out.push_str(&item.fmt_value());
                out.push('\n');
            }
            out
        }
        Value::Nil => String::new(),
        other => format!("{}\n", other.fmt_value()),
    }
}

pub fn parse_command(line: &str) -> Option<ReplCommand> {
    match line.trim() {
        ":مساعدة" => Some(ReplCommand::Help),
        ":اعرض" => Some(ReplCommand::Show),
        ":امسح-المحفوظات" => Some(ReplCommand::ClearHistory),
        ":مسح" => Some(ReplCommand::Clear),
        ":اسمح-بالشبكة" => Some(ReplCommand::AllowNetwork),
        ":اسمح-بالخادم" => Some(ReplCommand::AllowServer),
        ":خروج" | "خروج" => Some(ReplCommand::Quit),
        _ => None,
    }
}

/// مجلد الإعدادات (اجتماع 45 §59، البندان 36 و37):
/// `~/إعدادات/` — مرئي في البيت، يحرره المستخدم،
/// لا مخفي.
pub(crate) fn settings_dir(program_dir: &Path) -> PathBuf {
    program_dir.join("home").join("إعدادات")
}

/// ملف الإعدادات: `~/إعدادات/niva.properties`.
pub(crate) fn settings_path(program_dir: &Path) -> PathBuf {
    settings_dir(program_dir).join("niva.properties")
}

/// المسار القديم قبل الفصل (في جذر مجلد البرنامج).
fn legacy_settings_path(program_dir: &Path) -> PathBuf {
    program_dir.join("niva.properties")
}

/// مفاتيح متقاعدة سبق تحويلها (البند 75): (قديم، جديد).
/// لا مفتاح أُعيدت تسميته بعد حتى الآن — الجدول فارغ
/// عمدا، والآلية (migrate_retired_keys + إعادة الكتابة)
/// جاهزة للتحويلة الأولى حين يأمر بها المالك.
const RETIRED_KEYS: &[(&str, &str)] = &[];

/// حوّل المفاتيح المتقاعدة في نص ملف إعدادات إلى
/// أسمائها الجديدة (البند 75): دالة خالصة للاختبار.
/// تعيد النص الجديد وسطور الإشعار («المفتاح القديم X
/// صار Y») — سطر واحد لكل تحويلة.
pub(crate) fn migrate_retired_keys(
    content: &str,
    table: &[(&str, &str)],
) -> (String, Vec<String>) {
    if table.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let mut out = String::new();
    let mut notices = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        let Some(eq) = line.find('=') else {
            out.push_str(raw);
            out.push('\n');
            continue;
        };
        let key = line[..eq].trim();
        match table.iter().find(|(old, _)| *old == key) {
            Some((old, new)) => {
                out.push_str(new);
                out.push('=');
                out.push_str(line[eq + 1..].trim());
                out.push('\n');
                notices.push(format!("المفتاح القديم {} صار {}", old, new));
            }
            None => {
                out.push_str(raw);
                out.push('\n');
            }
        }
    }
    (out, notices)
}

/// إدخالات ملف إعدادات: أزواج (مفتاح، قيمة) بلا
/// تعليقات ولا سطور تالفة.
fn parse_settings_entries(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        out.push((
            line[..eq].trim().to_string(),
            line[eq + 1..].trim().to_string(),
        ));
    }
    out
}

/// اقرأ ملف الإعدادات (اجتماع 45 §59، البندان 36 و75):
/// الجديد أولا (`~/إعدادات/niva.properties`)؛ وإن غاب
/// والقديم موجود نُقل إليه (وإن تعذّر النقل قُرئ القديم
/// مع إشعار بإعادة المحاولة لاحقا)؛ والمفاتيح المتقاعدة
/// تتحول عند القراءة ويُعاد كتابة الملف بها. تعيد
/// (الإدخالات، الإشعارات).
pub(crate) fn read_settings(program_dir: &Path) -> (Vec<(String, String)>, Vec<String>) {
    let mut notices = Vec::new();
    let path = settings_path(program_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => {
            let (migrated, mut n) = migrate_retired_keys(&t, RETIRED_KEYS);
            let converted = !n.is_empty();
            notices.append(&mut n);
            if converted {
                let _ = crate::install::write_atomic(&path, migrated.as_bytes());
            }
            migrated
        }
        Err(_) => {
            let legacy = legacy_settings_path(program_dir);
            match std::fs::read_to_string(&legacy) {
                Ok(old) => {
                    let (migrated, mut n) = migrate_retired_keys(&old, RETIRED_KEYS);
                    notices.append(&mut n);
                    let _ = std::fs::create_dir_all(settings_dir(program_dir));
                    match crate::install::write_atomic(&path, migrated.as_bytes()) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&legacy);
                            migrated
                        }
                        Err(_) => {
                            notices.push(
                                "تعذّر نقل ملف الإعدادات إلى ~/إعدادات/niva.properties — سأستخدم الملف القديم وأعيد المحاولة لاحقا"
                                    .to_string(),
                            );
                            old
                        }
                    }
                }
                Err(_) => return (Vec::new(), notices),
            }
        }
    };
    let mut entries = parse_settings_entries(&text);
    // (41) قيمة بلا بايت فارغ: القيمة غير الصالحة تُسقط
    // مع إشعار يسمّي المفتاح، بلا إيقاف للجلسة.
    entries.retain(|(key, value)| {
        if crate::env::valid_value(value) {
            true
        } else {
            notices.push(format!("قيمة المفتاح {} غير صالحة — أُسقطت", key));
            false
        }
    });
    (entries, notices)
}

/// قفل النقل (البند 50): ملف فارغ عادي باسم المهمة
/// تحت `<program_dir>/.نيفا/` — يُنشأ فقط بعد النجاح
/// الكامل للنقل.
const MOVE_LOCK_NAME: &str = "نقل-البيت.قفل";

/// (اجتماع 49 §63) علامات شجرة مصدر — مجلد ترمنال
/// لن يحمل أيا منها أبدا.
const SOURCE_CHECKOUT_MARKERS: &[&str] = &[
    "Cargo.toml",
    ".git",
    "package.json",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Makefile",
    "CMakeLists.txt",
];

/// (اجتماع 49 §63) هل هذا المجلد شجرة مصدر لا مجلد
/// ترمنال؟ — وجود أي علامة من القائمة يكفي.
fn is_source_checkout(program_dir: &Path) -> bool {
    SOURCE_CHECKOUT_MARKERS
        .iter()
        .any(|m| program_dir.join(m).exists())
}

/// أسماء الجذر التي لا تنتقل إلى البيت أبدا (اجتماع 45
/// §59، البند 42 + استثناءان موثقان في CATALOG-43):
/// - home/ و.نيفا/ وtmp/ وniva.properties وملفات
///   الجلسات (session-*) — استثناءات الاجتماع الأربعة.
/// - `إعدادات/` — مجلد الإعدادات الجديد نفسه.
/// - `*.نيفا` — ملفات البرامج المثبتة: `استورد` يبحث
///   في مجلد البرنامج وحده، فنقلها يكسر الاستيراد.
/// - `*.نيفاب` — مصنوع مبني: منتج بناء لا ملف قديم
///   سائب، فتشغيله لا يرحّله.
/// - `بيان.حزمة` و`قفل.حزمة` — بيان المشروع وقفله:
///   يُقرآن من جذر مجلد البرنامج (`اقرأ_مشروع` و
///   `locked_version`)، فنقلهما يكسر الاستيراد
///   المقيَّد (المفسر يأخذ الأحدث بدلا من المقيَّد).
/// - `حول-نيفا.md` — وثيقة يولّدها التطبيق ويعيد
///   كتابتها في الجذر.
fn stays_at_root(name: &str) -> bool {
    matches!(
        name,
        "home" | ".نيفا" | "tmp" | "إعدادات" | "niva.properties" | "حول-نيفا.md"
            | "بيان.حزمة" | "قفل.حزمة"
    ) || name.starts_with("session-")
        || name.ends_with(".نيفا")
        || name.ends_with(".نيفاب")
}

/// إنشاء مجلدات الجلسة وفحص الكتابة فقط (اجتماع 45
/// §59، البنود 42 و51): home/ وtmp/ و~/إعدادات/
/// و.نيفا/، ثم ملف اختبار في tmp/ (خطأ قاتل يسمّي
/// المجلد). بلا نقل وبلا قفل — وهذا ما يحتاجه
/// المستقل (مجلد الملف التنفيذي ليس مجلد ترمنال،
/// والنقل فيه يحرّك الملف التنفيذي نفسه).
pub fn ensure_dirs(program_dir: &Path) -> Result<(), String> {
    let policy = crate::permissions::FilePolicy::restricted(program_dir.to_path_buf());
    let home = program_dir.join("home");
    let tmp = program_dir.join("tmp");
    let internal = program_dir.join(".نيفا");
    // المجلدات أولا — بلا رسالة للمستخدم إن نجحت.
    for dir in [&home, &tmp, &internal, &settings_dir(program_dir)] {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!(
                "تعذّر إنشاء مجلد {}: {}",
                policy.display_path(dir),
                e
            )
        })?;
    }
    // (51) فحص الكتابة: ملف اختبار في tmp/ — والخطأ
    // قاتل يسمّي المجلد، ولا يبدأ الدخول.
    let probe = tmp.join(".نيفا-اختبار-كتابة");
    match std::fs::write(&probe, "اختبار") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(_) => {
            return Err(format!(
                "مجلد الملفات غير قابل للكتابة: {} — تحقق من صلاحيات المجلد ثم أعد المحاولة",
                policy.display_path(&tmp)
            ));
        }
    }
    Ok(())
}

/// تجهيز أول تشغيل (اجتماع 45 §59، البنود 35 و36 و42
/// و50 و51): أنشئ home/ وtmp/ و~/إعدادات/ و.نيفا/؛
/// وافحص قابلية كتابة مجلد الملفات بملف اختبار في tmp/
/// (خطأ قاتل يسمّي المجلد)؛ وانقل ملفات الجذر القديمة
/// إلى البيت مرة واحدة (بقفل)؛ وانقل الإعدادات القديمة
/// إلى ~/إعدادات/. تعيد إشعارات تُطبع بعد اللافتة، أو
/// رسالة قاتلة تمنع بدء الدخول.
pub fn ensure_first_run(program_dir: &Path) -> Result<Vec<String>, String> {
    let mut notices = Vec::new();
    let policy = crate::permissions::FilePolicy::restricted(program_dir.to_path_buf());
    let home = program_dir.join("home");
    // هل كان البيت موجودا قبل هذا التشغيل؟ — يُفحص
    // قبل إنشائه، لأن القفل مع بيت أُعيد إنشاؤه الآن
    // حالة غير متسقة (البند 50).
    let home_existed = home.is_dir();
    // المجلدات وفحص الكتابة أولا.
    ensure_dirs(program_dir)?;
    // (36) نقل الإعدادات القديمة — read_settings يفعله
    // عند القراءة؛ وهنا نفعله مبكرا ليظهر التخطيط
    // الجديد فورا.
    let (_, mut settings_notices) = read_settings(program_dir);
    notices.append(&mut settings_notices);
    // (35/42/50) نقل البيت مرة واحدة، بقفل.
    let lock = program_dir.join(".نيفا").join(MOVE_LOCK_NAME);
    if lock.exists() {
        // القفل موجود لكن البيت لم يكن موجودا قبل هذا
        // التشغيل (أُعيد إنشاؤه فارغا أعلاه): حالة غير
        // متسقة — الخطأ يسمّي القفل بصيغته القصيرة
        // ويطلب حذفه يدويا لإعادة المحاولة.
        if !home_existed {
            return Err(format!(
                "البيت غير موجود رغم وجود القفل {} — احذف الملف يدويا لإعادة المحاولة",
                format!(".نيفا/{}", MOVE_LOCK_NAME)
            ));
        }
        return Ok(notices);
    }
    // (اجتماع 46 §60) مجلد مشروع: محتوياته هي
    // المشروع نفسه لا ملفات قديمة سائبة — يُتخطى
    // النقل كاملا (نقل `مصدر/` أو `حزم/` يكسر
    // البرنامج الجاري واستيراده)، ويُنشأ القفل بصمت.
    let is_project = program_dir.join("بيان.حزمة").is_file()
        || program_dir.join("حزم").is_dir()
        || program_dir.join("مصدر/رئيسي.نيفا").is_file();
    if is_project {
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    // (اجتماع 49 §63) حارس مجلد المصدر: مجلد فيه علامة
    // شجرة مصدر ليس مجلد ترمنال أبدا — فالنقل يُتخطى
    // كاملا ويُنشأ القفل بصمت (الترحيل «غير منطبق»).
    // بلا هذا الحارس نقلت سلسلة batch2e شجرة مصدر
    // المحرك كاملة إلى home/ (اختبار REPL بلا مجلد
    // مؤقت ورث جذر المحرك فصار مجلد برنامج).
    if is_source_checkout(program_dir) {
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    let mut to_move: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(program_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if stays_at_root(&name) {
                continue;
            }
            to_move.push((entry.path(), name));
        }
    }
    if to_move.is_empty() {
        // لا شيء لينقل: القفل يثبّت الاكتمال.
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    let mut moved_names: Vec<String> = Vec::new();
    for (path, name) in &to_move {
        let dest = home.join(name);
        if dest.exists() {
            // هدف بنفس الاسم في البيت: لا نكتب فوقه —
            // الحالة غير متسقة، بلا قفل، وتُعاد المحاولة
            // تلقائيا في المرة التالية.
            return Err(format!(
                "تعذّر نقل {} إلى البيت — ملف بنفس الاسم موجود في ~ — ستعاد المحاولة تلقائيا في المرة التالية",
                policy.display_path(path)
            ));
        }
        if let Err(e) = std::fs::rename(path, &dest) {
            return Err(format!(
                "تعذّر نقل {} إلى البيت ({}) — ستعاد المحاولة تلقائيا في المرة التالية",
                policy.display_path(path),
                e
            ));
        }
        moved_names.push(name.clone());
    }
    // النجاح الكامل وحده ينشئ القفل.
    let _ = std::fs::write(&lock, "");
    // رسالة واحدة واضحة بأسماء المنقولات.
    let shown: Vec<&str> = moved_names.iter().take(5).map(|s| s.as_str()).collect();
    let mut msg = format!("نُقلت ملفاتك القديمة إلى البيت (~): {}", shown.join("، "));
    if moved_names.len() > 5 {
        msg.push_str(&format!("، و {} أخرى", moved_names.len() - 5));
    }
    notices.push(msg);
    Ok(notices)
}

/// The interactive loop: read lines from stdin, feed them to
/// the stateful `Repl`, print what it returns. Returns SUCCESS
/// on :خروج or EOF. The session runs on the compiler (VM) by
/// default; `use_interpreter` (CLI `--مفسر`) runs it on the
/// tree-walking interpreter instead, for diagnosis only.
pub fn run_repl(
    trusted: bool,
    network_allowed: bool,
    server_allowed: bool,
    local_hosts: &[String],
    use_interpreter: bool,
) -> ExitCode {
    let mut repl = if use_interpreter {
        Repl::new_interpreter()
    } else {
        Repl::new()
    };
    repl.set_trusted(trusted);
    repl.set_network_allowed(network_allowed);
    repl.set_server_allowed(server_allowed);
    // اجتماع 32، السؤال الثامن: REPL التفاعلي يبث
    // `اطبع` حية — السطر يصل فور إنتاجه لا بعد الخروج.
    // (WASM لا يفعّله أبدا: يعرض result.output كما كان.)
    repl.set_live_output(true);
    for host in local_hosts {
        repl.allow_local_network_host(host);
    }
    // اجتماع 45 §59: تجهيز أول تشغيل (مجلدات + نقل
    // البيت + الإعدادات) — والخطأ القاتل يمنع الدخول.
    let notices = match ensure_first_run(&repl.program_dir) {
        Ok(n) => n,
        Err(msg) => {
            eprintln!("{}", msg);
            return ExitCode::FAILURE;
        }
    };
    println!("{}", banner());
    for n in &notices {
        println!("{}", n);
    }
    if repl.welcome_enabled() {
        println!("{}", welcome_text());
    }
    // ملف البدء والمقتطفات (البنود 44–46 و111) — الخطأ
    // فيهما لا يمنع الدخول.
    let startup = repl.run_startup_files();
    if !startup.is_empty() {
        print!("{}", startup);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        print!("{}", repl.prompt());
        if out.flush().is_err() {
            return ExitCode::FAILURE;
        }
        // مهمة 22 (شبكة.نزّل): القراءة عبر مضخة stdin
        // لا مباشرة — خيط المضخة وحده يملك stdin،
        // والمستهلكون (ومنهم `ادخل` داخل feed وانتظار
        // الجلب) يأخذون الأسطر من قناتها. قفل المهمة 20
        // (لكل قراءة لا لكل الحلقة) صار غير ذي موضوع
        // هنا: لا قفل إطلاقا على هذا الخيط بعد اليوم.
        // وسطور الإلغاء تُتخطى هنا بصمت (بلا جلب جارٍ
        // لا أثر لها)؛ وEOF يخرج نظيفا كما كان.
        let line = match crate::stdin_pump::stdin_line() {
            Some(l) => l,
            None => {
                // EOF (Ctrl-D): exit cleanly.
                let result = repl.eof();
                print!("{}", result.output);
                return ExitCode::SUCCESS;
            }
        };
        let result = repl.feed(&line);
        // Audit م-6: structured control signals on stderr,
        // only when the host opts in (NIVA_CONTROL_CHANNEL
        // =stderr) — the desktop/Termux REPL never shows them.
        // The \u{0001} prefix keeps a control line distinct
        // from any other stderr text (panics included).
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            match repl.last_control() {
                ReplControl::NetworkNeeded => eprintln!("\u{0001}نيفا-تحكم:شبكة"),
                ReplControl::ServerNeeded => eprintln!("\u{0001}نيفا-تحكم:خادم"),
                ReplControl::SessionCleared => eprintln!("\u{0001}نيفا-تحكم:مسح"),
                // اجتماع 45 §59 (البند 19): إعادة قراءة
                // الإعدادات — التطبيق يعيد التطبيق الحي.
                ReplControl::SettingsReload => eprintln!("\u{0001}نيفا-تحكم:إعدادات"),
                ReplControl::None => {}
            }
        }
        print!("{}", result.output);
        if result.ended {
            return ExitCode::SUCCESS;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done_out(r: ReplEval) -> String {
        match r {
            ReplEval::Done { output } => output,
            ReplEval::Failed { output, error } => {
                panic!("expected Done, got Failed: output={} error={}", output, error)
            }
            ReplEval::Incomplete => panic!("expected Done, got Incomplete"),
        }
    }

    fn is_failed(r: ReplEval) -> bool {
        matches!(r, ReplEval::Failed { .. })
    }

    #[test]
    fn repl_bare_expression_prints_value() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("١ + ٢")), "٣\n");
        assert_eq!(done_out(r.eval("\"مرحبا\"")), "مرحبا\n");
        assert_eq!(done_out(r.eval("[١، ٢]")), "[١, ٢]\n");
    }

    #[test]
    fn repl_live_output_streams_and_survives_reset() {
        // اجتماع 32، السؤال الثامن: مع البث الحي لا
        // يُحتفظ بمخرجات `اطبع` في ناتج eval (وصلت
        // للمخرجات فورا)؛ و:مسح يعيد العلم نفسه.
        let mut r = Repl::new();
        r.set_live_output(true);
        assert_eq!(done_out(r.eval("اطبع ١")), "");
        r.reset();
        assert_eq!(done_out(r.eval("اطبع ٢")), "");
        // وبدونه تعود المخرجات كما كانت.
        r.set_live_output(false);
        assert_eq!(done_out(r.eval("اطبع ٣")), "٣\n");
    }

    #[test]
    fn repl_trailing_expression_prints_after_block() {
        let mut r = Repl::new();
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب\nجمع(٢، ٣)")),
            "٥\n"
        );
        // A trailing call after a definition prints its value.
        assert_eq!(done_out(r.eval("س = ٧\nس * ٢")), "١٤\n");
    }

    // Simulate run_repl's line feeding: an indented line while a
    // block is pending only waits; a line at indent 0 evaluates
    // the accumulated block. This is the regression test for
    // nested function definitions (closures) in the REPL: the
    // block must reach eval as one piece, not line by line.
    fn feed_lines(repl: &mut Repl, lines: &[&str]) -> Vec<String> {
        let mut pending = String::new();
        let mut outputs = Vec::new();
        for line in lines {
            if !pending.is_empty() {
                if line.starts_with([' ', '\t']) {
                    pending.push('\n');
                    pending.push_str(line);
                    continue;
                }
                pending.push('\n');
                pending.push_str(line);
                match repl.eval(&pending) {
                    ReplEval::Incomplete => continue,
                    ReplEval::Done { output } => outputs.push(output),
                    ReplEval::Failed { output, error } => {
                        outputs.push(format!("{}{}", output, error))
                    }
                }
                pending.clear();
                continue;
            }
            match repl.eval(line) {
                ReplEval::Incomplete => pending.push_str(line),
                ReplEval::Done { output } => outputs.push(output),
                ReplEval::Failed { output, error } => {
                    outputs.push(format!("{}{}", output, error))
                }
            }
        }
        outputs
    }

    #[test]
    fn repl_nested_function_definition_completes_as_one_block() {
        let mut r = Repl::new();
        let outs = feed_lines(
            &mut r,
            &[
                "دالة اصنع_عداد():",
                "    ع = ٠",
                "    دالة زد():",
                "        ع = ع + ١",
                "        ارجع ع",
                "    ارجع زد",
                "أ = اصنع_عداد()",
                "أ()",
                "أ()",
            ],
        );
        // The definition block runs silently; the two calls print ١, ٢.
        assert_eq!(outs, vec!["", "١\n", "٢\n"]);
    }

    #[test]
    fn repl_lambda_define_and_call() {
        // Meeting 13: a lambda is a single-line expression, so
        // the REPL needs no block continuation for it.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("مضاعف = دالة_مجهولة(س): س * ٢")), "");
        assert_eq!(done_out(r.eval("مضاعف(٥)")), "١٠\n");
        assert_eq!(done_out(r.eval("مضاعف")), "<دالة مجهولة>\n");
    }

    #[test]
    fn repl_list_methods_mutate_and_print() {
        // Meeting 14: method calls are expressions, so the REPL
        // handles them like any other line.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("ق = [١، ٢]")), "");
        assert_eq!(done_out(r.eval("ق.أضف(٣)")), "لاشيء\n");
        assert_eq!(done_out(r.eval("ق")), "[١, ٢, ٣]\n");
        assert_eq!(done_out(r.eval("\"مرحبا\".طول()")), "٥\n");
    }

    #[test]
    fn repl_stdlib_import_and_call() {
        // Meeting 16: native modules load in the REPL like any
        // other import — the REPL keeps the same interpreter.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("استورد رياضيات")), "");
        assert_eq!(done_out(r.eval("رياضيات.جذر(١٦)")), "٤\n");
        assert_eq!(done_out(r.eval("رياضيات.باي")), "٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n");
    }

    #[test]
    fn repl_variables_and_functions_persist() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        assert_eq!(done_out(r.eval("س * ٢")), "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
    }

    #[test]
    fn repl_incomplete_block_keeps_waiting() {
        let mut r = Repl::new();
        assert!(matches!(r.eval("دالة جمع(أ، ب):"), ReplEval::Incomplete));
        assert!(matches!(r.eval("إذا س:"), ReplEval::Incomplete));
        assert!(matches!(r.eval("س = ٥"), ReplEval::Done { .. }));
    }

    #[test]
    fn repl_error_does_not_kill_session() {
        let mut r = Repl::new();
        assert!(is_failed(r.eval("غيرمعرف")));
        assert_eq!(done_out(r.eval("س = ١")), "");
        assert_eq!(done_out(r.eval("س")), "١\n");
    }

    #[test]
    fn repl_print_output_and_reset() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("اطبع ٥")), "٥\n");
        assert_eq!(done_out(r.eval("س = ٥")), "");
        r.reset();
        assert!(is_failed(r.eval("س")));
        assert!(r.history().is_empty());
    }

    #[test]
    fn repl_history_shows_with_bidi_isolates() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        let shown = r.show_history();
        assert!(shown.contains("س = ٥"));
        assert!(shown.contains(RLI));
        assert!(shown.contains(PDI));
    }

    #[test]
    fn repl_commands_parse() {
        assert!(matches!(parse_command(":مساعدة"), Some(ReplCommand::Help)));
        assert!(matches!(parse_command(":اعرض"), Some(ReplCommand::Show)));
        assert!(matches!(
            parse_command(":امسح-المحفوظات"),
            Some(ReplCommand::ClearHistory)
        ));
        assert!(matches!(parse_command(":مسح"), Some(ReplCommand::Clear)));
        assert!(matches!(parse_command(":خروج"), Some(ReplCommand::Quit)));
        assert!(matches!(parse_command("خروج"), Some(ReplCommand::Quit)));
        // No Latin aliases: :help / :quit / exit are not commands.
        assert!(parse_command(":help").is_none());
        assert!(parse_command("exit").is_none());
        assert!(parse_command("س = ٥").is_none());
    }

    // Task 2 (Meeting 25 plan): :امسح-المحفوظات clears
    // only the :اعرض history lines — variables and functions
    // survive it, unlike :مسح which resets the interpreter.
    #[test]
    fn repl_clear_history_keeps_variables() {
        let mut r = Repl::new();
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("س * ٢").output, "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
        // :اعرض shows the session lines.
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(o.output.contains("س * ٢"), "got: {}", o.output);
        // The new command clears the history only.
        let o = r.feed(":امسح-المحفوظات");
        assert_eq!(o.output, "مسحت محفوظات الجلسة.\n");
        assert!(!o.ended);
        assert!(r.history().is_empty());
        // :اعرض is now just the header — the old lines are gone.
        let o = r.feed(":اعرض");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(!o.output.contains("س * ٢"), "got: {}", o.output);
        // Variables and functions survived the clear.
        assert_eq!(r.feed("اطبع س").output, "٥\n");
        assert_eq!(r.feed("س").output, "٥\n");
        assert_eq!(r.feed("جمع(٢، ٣)").output, "٥\n");
        // The command line itself is never recorded: a new
        // line appears in :اعرض, the command does not.
        assert_eq!(r.feed("ص = ٧").output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("ص = ٧"), "got: {}", o.output);
        assert!(!o.output.contains("امسح-المحفوظات"), "got: {}", o.output);
    }

    // The stateful feed API (console): the prototype session,
    // line by line, with the exact prompts at each step. Only
    // compiled with the prototype feature enabled.
    #[test]
    #[cfg(feature = "prototype-rtl")]
    fn repl_feed_prototype_session() {
        let mut r = Repl::new();
        let o = r.feed("٥ = س");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        let o = r.feed("٣ = ص");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("اطبع س + ص");
        assert_eq!(o.output, "٨\n");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("إذا س أكبر من ص:");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع \"س أكبر\"");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("٧ = ع");
        assert_eq!(o.output, "س أكبر\n");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("اطبع ع * ٢");
        assert_eq!(o.output, "١٤\n");
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
    }

    // Public build (no prototype feature): the reversed form is
    // rejected with the normal error, and the normal form runs
    // the same session. This is the behavior the console ships.
    #[test]
    #[cfg(not(feature = "prototype-rtl"))]
    fn repl_feed_reversed_assignment_rejected() {
        let mut r = Repl::new();
        let o = r.feed("٥ = س");
        assert!(o.output.contains("هدف التعيين غير صالح"), "got: {}", o.output);
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives; the normal form runs the session.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("ص = ٣");
        assert_eq!(o.output, "");
        let o = r.feed("اطبع س + ص");
        assert_eq!(o.output, "٨\n");
        let o = r.feed("إذا س أكبر من ص:");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع \"س أكبر\"");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("ع = ٧");
        assert_eq!(o.output, "س أكبر\n");
        let o = r.feed("اطبع ع * ٢");
        assert_eq!(o.output, "١٤\n");
    }

    #[test]
    fn repl_feed_empty_line_cancels_block() {
        let mut r = Repl::new();
        let o = r.feed("دالة جمع(أ، ب):");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("");
        assert_eq!(o.output, "ألغيت الكتلة غير المكتملة.\n");
        assert_eq!(o.prompt, "~ $ ");
        // The cancelled definition never took effect.
        assert!(is_failed(r.eval("جمع(٢، ٣)")));
    }

    #[test]
    fn repl_feed_command_while_block_pending() {
        let mut r = Repl::new();
        let o = r.feed("إذا س:");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed(":مساعدة");
        assert_eq!(
            o.output,
            "ألغيت الكتلة غير المكتملة.\n:مساعدة\n:اعرض\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:خروج\nمعلومات\nأعد-الإعدادات\nأعد-الضبط\n"
        );
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
    }

    #[test]
    fn repl_feed_quit_and_clear() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        let o = r.feed(":مسح");
        assert_eq!(o.output, "بدأت جلسة جديدة.\n");
        assert!(!o.ended);
        assert!(is_failed(r.eval("س")));
        let o = r.feed(":خروج");
        assert!(o.ended);
        assert_eq!(o.output, "");
        let mut r2 = Repl::new();
        let o = r2.feed("خروج");
        assert!(o.ended);
        // Latin `exit` is not a command any more.
        let mut r3 = Repl::new();
        let o = r3.feed("exit");
        assert!(!o.ended);
    }

    #[test]
    fn repl_feed_error_appears_in_output() {
        let mut r = Repl::new();
        let o = r.feed("غيرمعرف");
        assert!(o.output.contains("غير معرف"));
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives the error.
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        let o = r.feed("س");
        assert_eq!(o.output, "١\n");
    }

    // Meeting 22: the network flag in the REPL — off by
    // default, on with set_network_allowed; feed() flags the
    // permission error so a console can offer the first-use
    // prompt. The fetch itself is exercised against the local
    // test server in network.rs; here the gate and the flag
    // are what is tested (no server needed).
    #[test]
    fn repl_network_gate_off_by_default() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        assert!(!o.needs_network);
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(o.needs_network);
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives the refused fetch.
        assert_eq!(done_out(r.eval("س = ١")), "");
        assert_eq!(done_out(r.eval("س")), "١\n");
    }

    // Meeting 26: without the `https` feature, HTTPS is refused
    // before any traffic, even allowed. With the feature, the
    // same fetch attempts a real connection (see the variant
    // below) — so this test is the no-feature one.
    #[test]
    #[cfg(not(feature = "https"))]
    fn repl_network_allowed_flag_sticks_after_reset() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        r.set_network_allowed(true);
        // HTTPS is refused before any traffic, even allowed.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"https://example.com/\")");
        assert!(o.output.contains("HTTPS غير مدعوم"), "got: {}", o.output);
        assert!(!o.needs_network, "an HTTPS refusal is not a permission ask");
        r.reset();
        // reset() restores the flag: still allowed afterwards.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"https://example.com/\")");
        assert!(o.output.contains("HTTPS غير مدعوم"), "got: {}", o.output);
        crate::network::set_network_allowed(false);
    }

    // Meeting 26: with the `https` feature, the flag-sticks
    // check uses a plain HTTP fetch to a closed local port:
    // the connection is refused at once, so the error is a
    // connection error, not a permission ask.
    #[test]
    #[cfg(feature = "https")]
    fn repl_network_allowed_flag_sticks_after_reset() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        crate::network::allow_local_host("127.0.0.1");
        let mut r = Repl::new();
        r.set_network_allowed(true);
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("تعذر الاتصال"), "got: {}", o.output);
        assert!(!o.needs_network, "a connection error is not a permission ask");
        r.reset();
        // reset() restores the flag: still allowed afterwards.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("تعذر الاتصال"), "got: {}", o.output);
        crate::network::set_network_allowed(false);
    }

    // Sessions (owner's order 2026-11-05 ~21:03, overruling
    // Meeting 23 on the multiplexer core): the network flag is
    // per-instance — one console's approval never leaks into
    // another, and switching back to an approved console keeps
    // its approval without asking again.
    #[test]
    fn repl_network_flag_is_per_instance() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r1 = Repl::new();
        let mut r2 = Repl::new();
        // r1 allows; r2 is still gated.
        r1.set_network_allowed(true);
        assert!(r1.network_allowed());
        assert!(!r2.network_allowed());
        let o = r2.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r2.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(o.needs_network);
        // r1 passes the gate (the fetch itself fails on the
        // refused connection / loopback refusal — but NOT on
        // the permission error).
        let o = r1.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r1.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        // r2 allows too; switching back to r1 keeps its approval.
        r2.set_network_allowed(true);
        let o = r2.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        let o = r1.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        crate::network::set_network_allowed(false);
    }

    #[test]
    fn repl_banner_text() {
        // Minimal banner (owner's no-explanation rule): name +
        // version only, no teaching, no how-to.
        assert_eq!(banner(), "محرك نيفا ١٫٠٫٠");
    }

    // رسالة الترحيب: عربية مختصرة — الوثائق والإبلاغ
    // والأوامر اليومية فقط، وبلا سطر مجتمع (لا قناة بعد).
    #[test]
    fn repl_welcome_text() {
        let text = welcome_text();
        assert!(text.starts_with("أهلا بك في نيفا"), "got: {}", text);
        assert!(text.contains("https://github.com/RIO4140/niva"), "got: {}", text);
        assert!(
            text.contains("https://github.com/RIO4140/niva/issues"),
            "got: {}",
            text
        );
        for cmd in ["ات", "شغل", "تثبيت", "تن"] {
            assert!(text.contains(cmd), "got: {}", text);
        }
        assert!(!text.contains("المجتمع"), "got: {}", text);
    }

    // مفتاح `الترحيب` في niva.properties: `لا` يوقفها،
    // وغياب الملف أو المفتاح أو أي قيمة أخرى يبقيها.
    #[test]
    fn repl_welcome_enabled_by_properties() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        let settings_dir = home.join("إعدادات");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let r = Repl::with_program_dir(dir.clone());
        assert!(r.welcome_enabled());
        // المسار الجديد: ~/إعدادات/niva.properties.
        std::fs::write(settings_dir.join("niva.properties"), "# تعليق\nالترحيب=لا\n").unwrap();
        assert!(!r.welcome_enabled());
        std::fs::write(settings_dir.join("niva.properties"), "الترحيب = نعم\n").unwrap();
        assert!(r.welcome_enabled());
        std::fs::write(settings_dir.join("niva.properties"), "المظهر=فاتح\n").unwrap();
        assert!(r.welcome_enabled());
        // المسار القديم (جذر مجلد البرنامج) يُنقل عند
        // القراءة (اجتماع 45 §59، البند 36).
        let _ = std::fs::remove_file(settings_dir.join("niva.properties"));
        std::fs::write(dir.join("niva.properties"), "الترحيب=لا\n").unwrap();
        assert!(!r.welcome_enabled());
        assert!(settings_dir.join("niva.properties").is_file());
        assert!(!dir.join("niva.properties").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_banner_terminal_version() {
        // بلاغ 2026-11-08: `اص` في الترمنال يعرض نسخة
        // المحرك لا نسخة الترمنال — اللافتة مع نسخة
        // ترمنال تسمّي الترمنال أولا وتبقي المحرك.
        assert_eq!(
            banner_for(Some("1.2.11")),
            "ترمنال نيفا ١٫٢٫١١ (محرك ١٫٠٫٠)"
        );
        assert_eq!(
            banner_for(Some("2.0")),
            "ترمنال نيفا ٢٫٠ (محرك ١٫٠٫٠)"
        );
        assert_eq!(banner_for(None), "محرك نيفا ١٫٠٫٠");
    }

    // Arabic terminal shortcuts in the REPL (owner's order
    // 2026-11-05): they run like any other builtin here.
    // The test runs with the engine crate dir as the working
    // directory, so Cargo.toml is one of the listed files.
    // Full-word file builtins in the REPL (the shortcut
    // layer calls these as its implementation).
    #[test]
    fn repl_file_shortcuts_work() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("المجلد_الحالي()");
        assert!(!o.ended);
        // Home displays as `~` (owner's order 2026-11-05
        // ~22:38), never the absolute internal path.
        assert_eq!(o.output.trim(), "~", "got: {}", o.output);
        let o = r.feed("اعرض_الملفات()");
        assert!(!o.ended);
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Short terminal commands (owner's order 2026-11-05 ~22:02):
    // two-letter codes, space-separated args, no comma ever.
    fn shortcut_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("niva-shortcut-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // اجتماع 45 §59: الجلسة تبدأ في البيت — أنشئه
        // ليكون مجلد العمل موجودا للاختبارات.
        std::fs::create_dir_all(dir.join("home")).unwrap();
        dir
    }

    #[test]
    fn repl_shortcut_list_and_pwd() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        std::fs::write(home.join(".مخفي"), "سر").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe = home.join("شغّل");
            std::fs::write(&exe, "نص").unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink("أ.txt", home.join("رابط")).unwrap();
        }
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("مح");
        assert!(!o.ended);
        // Home displays as `~` (owner's order 2026-11-05
        // ~22:38), never the absolute internal path.
        assert_eq!(o.output.trim(), "~", "got: {}", o.output);
        let o = r.feed("ات");
        assert!(!o.ended);
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        assert!(o.output.contains("فرعي/"), "got: {}", o.output);
        // مثل `ls` الافتراضي: المخفي لا يظهر.
        assert!(!o.output.contains(".مخفي"), "got: {}", o.output);
        #[cfg(unix)]
        {
            // علامات `ls -F`: `*` للتنفيذي و`@` للرابط.
            assert!(o.output.contains("شغّل*"), "got: {}", o.output);
            assert!(o.output.contains("رابط@"), "got: {}", o.output);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // قيمة فارغة (كقيمة تثبيت-الكل) لا تطبع سطرا فارغا
    // بعدها — الموجّه يأتي مباشرة.
    #[test]
    fn repl_empty_value_prints_nothing() {
        let mut r = Repl::new();
        let o = r.feed("\"\"");
        assert!(!o.ended);
        assert_eq!(o.output, "", "got: {:?}", o.output);
        let o = r.feed("٢ + ٢");
        assert_eq!(o.output, "٤\n", "got: {:?}", o.output);
    }

    #[test]
    fn repl_shortcut_cd_mkdir_rmdir() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شم جديد");
        assert!(!o.ended);
        assert_eq!(o.output, "");
        assert!(home.join("جديد").is_dir());
        let o = r.feed("انتقل جديد");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("جديد"), "got: {}", o.output);
        let o = r.feed("انتقل ..");
        assert_eq!(o.output, "");
        let o = r.feed("حم جديد");
        assert_eq!(o.output, "");
        assert!(!home.join("جديد").exists());
        // اجتماع 45 §59: من البيت `..` تصل إلى مجلد
        // البرنامج ($نيفا) — مسموحة؛ والثانية تتجاوزه
        // فتُرفض.
        let o = r.feed("انتقل ..");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "$نيفا $ ");
        let o = r.feed("انتقل ..");
        assert!(o.output.contains("خارج مجلد البرنامج"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_show_file_and_rm_file() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص الملف").unwrap();
        std::fs::create_dir(home.join("مجلد")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // اف = اعرض ملف (cat).
        let o = r.feed("اف أ.txt");
        assert!(!o.ended);
        assert!(o.output.contains("نص الملف"), "got: {}", o.output);
        // حم is dir-only: a file is an error.
        let o = r.feed("حم أ.txt");
        assert!(o.output.contains("ليس مجلدا"), "got: {}", o.output);
        // حف is file-only: a dir is an error.
        let o = r.feed("حف مجلد");
        assert!(o.output.contains("ليس ملفا"), "got: {}", o.output);
        let o = r.feed("حف أ.txt");
        assert_eq!(o.output, "");
        assert!(!home.join("أ.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_copy_move_space_separated() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انسخ أ.txt ب.txt");
        assert!(!o.ended);
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "نص");
        let o = r.feed("انقل ب.txt ج.txt");
        assert_eq!(o.output, "");
        assert!(!home.join("ب.txt").exists());
        assert!(home.join("ج.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_wrong_args_and_not_a_shortcut() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        // Wrong arg counts are short Arabic errors; the session survives.
        let o = r.feed("انتقل");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("انسخ أ.txt");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("ات زيادة");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        // Not a shortcut: falls through to normal code.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("س");
        assert_eq!(o.output, "٥\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // أوامر كاملة بكلمة واحدة (2026-11-08 ~20:00، قاعدة
    // 4–5 حروف بلا ألف في النصف): انتقل/انسخ/انقل/موجود
    // بدلا من نم/سم/قم/وج — والأكواد المحذوفة نفسها لم
    // تعد تعمل (لا اختصار ولا أمر)؛ وعر/اط خرجا لأن
    // :اعرض واطبع يعملان أصلا بصيغتهما الكاملة.
    #[test]
    fn repl_full_word_commands_replace_removed_codes() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("انتقل ..");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("نم فرعي");
        assert!(!o.output.is_empty(), "removed code must not silently work");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("انسخ أ.txt ب.txt");
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "نص");
        let o = r.feed("انقل ب.txt ج.txt");
        assert_eq!(o.output, "");
        assert!(home.join("ج.txt").exists());
        let o = r.feed("موجود أ.txt");
        assert_eq!(o.output, "صحيح\n");
        let o = r.feed("موجود غائب.txt");
        assert_eq!(o.output, "خطأ\n");
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("عر");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        let o = r.feed("اط مرحبا");
        assert_ne!(o.output, "مرحبا\n", "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The 31-shortcut table (owner's correction 2026-11-05
    // ~22:11, superseding the 100): exactly 31 codes, all
    // present, no collisions. The 3 app-level codes (جل،
    // مد، اع) are NOT here — the app intercepts them before
    // the line reaches the engine.
    // Change 2026-11-06 (owner's whole-table no-alif rule):
    // خج/طن/طع/جذ/قو left the engine table and جد left the
    // app table; عع/نب/اب/تم/ال/كا joined the engine table
    // — 44 engine codes + 6 app codes = 50.
    // Change 2026-11-08 (owner's «اكتب دول كاملين»):
    // بم/بع/مس/مع/اد/اق/حع/مى/ال/كا left the engine table
    // and مش/شب/سش left the app table, to their full
    // phrases — 34 engine codes + 3 app codes = 37.
    // Change 2026-11-08 ~20:00 (owner's 4–5-letter rule):
    // نم/سم/قم/وج/عر/اط left the engine table, to their
    // full commands — 28 engine codes + 3 app codes = 31.
    #[test]
    fn shortcut_table_is_thirty_one_unique_codes() {
        let codes = [
            "ات", "مح", "شم", "حم", "حف", "اف", "شف", "عب", "عن", "عس",
            "عك", "عش", "حج", "شر", "اك", "ام", "تم", "عم", "اس",
            "اص", "رع", "تا", "وق", "نو", "اج",
            "عع", "نب", "اب",
        ];
        assert_eq!(codes.len(), 28, "engine codes (31 total - 3 app-level)");
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(shortcut_def(code).is_some(), "missing code: {}", code);
            assert!(seen.insert(code), "duplicate code: {}", code);
        }
        // The 9 original codes stay inside the 31 — except
        // نم/سم/قم, removed by the 4–5-letter rule.
        for code in ["ات", "مح", "شم", "حم", "حف", "اف"] {
            assert!(shortcut_def(code).is_some(), "original code lost: {}", code);
        }
        // App-level codes must NOT be engine shortcuts.
        for code in ["جل", "مد", "اع"] {
            assert!(shortcut_def(code).is_none(), "app code leaked into engine: {}", code);
        }
        // The removed codes are gone from both tables.
        for code in [
            "خج", "طن", "طع", "جذ", "قو", "جد",
            "بم", "بع", "مس", "مع", "اد", "اق", "حع", "مى", "ال", "كا",
            "مش", "شب", "سش",
            "نم", "سم", "قم", "وج", "عر", "اط",
        ] {
            assert!(shortcut_def(code).is_none(), "removed code still present: {}", code);
        }
    }

    // New file builtins through the shortcuts (space-separated,
    // no comma): touch / head / tail / wc / grep / find / tree /
    // du / ln / test / file-type / count / write.
    #[test]
    fn repl_shortcut_new_file_builtins() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "ألف\nباء\nجيم\nدال\nهاء\nواو").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        std::fs::write(home.join("فرعي/ملف-داخلي.txt"), "داخل").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شف جديد.txt");
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("جديد.txt")).unwrap(), "");
        let o = r.feed("عب ملف.txt");
        assert_eq!(o.output, "ألف\nباء\nجيم\nدال\nهاء\n");
        let o = r.feed("عب ملف.txt ٢");
        assert_eq!(o.output, "ألف\nباء\n");
        let o = r.feed("عن ملف.txt ٢");
        assert_eq!(o.output, "هاء\nواو\n");
        let o = r.feed("عس ملف.txt");
        assert_eq!(o.output, "٦\n");
        let o = r.feed("عك ملف.txt");
        assert_eq!(o.output, "٦\n");
        let o = r.feed("ابحث في ملف ألف ملف.txt");
        assert_eq!(o.output, "ألف\n");
        let o = r.feed("ابحث عن ملف ملف");
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        assert!(o.output.contains("فرعي/ملف-داخلي.txt"), "got: {}", o.output);
        let o = r.feed("عش");
        assert!(o.output.contains("فرعي/"), "got: {}", o.output);
        assert!(o.output.contains("  ملف-داخلي.txt"), "got: {}", o.output);
        let expected = std::fs::metadata(home.join("ملف.txt")).unwrap().len() as f64;
        let o = r.feed("حج ملف.txt");
        assert_eq!(parse_shortcut_number(o.output.trim()), Some(expected));
        let o = r.feed("موجود ملف.txt");
        assert_eq!(o.output, "صحيح\n");
        let o = r.feed("موجود غائب.txt");
        assert_eq!(o.output, "خطأ\n");
        let o = r.feed("ام فرعي");
        assert_eq!(o.output, "مجلد\n");
        let o = r.feed("ام غائب.txt");
        assert_eq!(o.output, "غير موجود\n");
        // عم before شر: ملف.txt + فرعي + جديد.txt = 3.
        let o = r.feed("عم");
        assert_eq!(o.output, "٣\n");
        let o = r.feed("شر ملف.txt رابط.txt");
        assert_eq!(o.output, "");
        assert_eq!(
            std::fs::read_to_string(home.join("رابط.txt")).unwrap(),
            "ألف\nباء\nجيم\nدال\nهاء\nواو"
        );
        // اك: path + the rest joined with spaces.
        let o = r.feed("اك ملف٢.txt مرحبا بالعالم");
        assert_eq!(o.output, "");
        assert_eq!(
            std::fs::read_to_string(home.join("ملف٢.txt")).unwrap(),
            "مرحبا بالعالم"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Text and numbers: random stays a shortcut; echo left
    // the table 2026-11-08 ~20:00 (owner's 4–5-letter
    // rule) — اطبع is the full command. Sum, min, max,
    // convert and range left the table 2026-11-08
    // (owner's «اكتب دول كاملين») — their plain builtins
    // مجموع/أدنى/أقصى/عدد/مدى take a list (مجموع/أدنى/
    // أقصى) or comma-separated args in normal code.
    // (طن/طع/جذ/قو left the table 2026-11-06 under the
    // no-alif rule; their builtins — طول/مطلق/
    // رياضيات.جذر/رياضيات.قوة — still work in programs.)
    #[test]
    fn repl_shortcut_numbers_and_text() {
        let mut r = Repl::new();
        assert_eq!(r.feed("مجموع([١، ٢، ٣])").output, "٦\n");
        assert_eq!(r.feed("أدنى([٥، ٢، ٨])").output, "٢\n");
        assert_eq!(r.feed("أقصى([٥، ٢، ٨])").output, "٨\n");
        assert_eq!(r.feed("عدد(\"٥\")").output, "٥\n");
        assert_eq!(r.feed("مدى(١، ٥)").output, "[١, ٢, ٣, ٤]\n");
        let o = r.feed("رع");
        let n = parse_shortcut_number(o.output.trim());
        assert!(matches!(n, Some(v) if (0.0..1.0).contains(&v)), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا بالعالم\"").output, "مرحبا بالعالم\n");
        // A non-number where a number is expected: short error.
        let o = r.feed("عدد(\"نص\")");
        assert!(o.output.contains("لا يمكن تحويل"), "got: {}", o.output);
    }

    // The 2026-11-06 replacements (owner's whole-table
    // no-alif rule): عع/نب/اب/تم — each wired to a
    // real action. (ال/كا left the table 2026-11-08 to
    // their full phrases «ارجع للبيت» و«كرر الأخير».)
    // The نب/اب error paths assume the serial
    // runner (CI runs --test-threads=1): the snippet's
    // `استورد عمليات` resets the global process policy
    // right before the spawn, so no other test can
    // interleave — exactly the guarantee the processes
    // tests get from their lock.
    #[test]
    fn repl_shortcut_replacements() {
        // عع: the current process id — a positive number.
        let mut r = Repl::new();
        let o = r.feed("عع");
        let pid = parse_shortcut_number(o.output.trim());
        assert!(matches!(pid, Some(v) if v > 0.0), "got: {}", o.output);
        // نب/اب: wiring only — no universal program exists
        // on all three CI runners for a happy path (the
        // spawn/kill happy paths are covered in
        // processes.rs). A missing program surfaces the
        // Arabic spawn error; an unknown id surfaces the
        // Arabic kill error.
        let o = r.feed("نب لاوجود");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        let o = r.feed("اب ٩٩٩٩٩٩");
        assert!(o.output.contains("عملية غير معروفة"), "got: {}", o.output);
        // ارجع للبيت: from a subdir back home — the prompt
        // returns to "~ $ ".
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("ارجع للبيت");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        // تم: details of a file, a dir, and a missing path.
        std::fs::write(home.join("ملف.txt"), "abc").unwrap();
        let o = r.feed("تم ملف.txt");
        assert!(o.output.contains("\"نوع\": \"ملف\""), "got: {}", o.output);
        assert!(o.output.contains("\"حجم\": ٣"), "got: {}", o.output);
        let o = r.feed("تم فرعي");
        assert!(o.output.contains("\"نوع\": \"مجلد\""), "got: {}", o.output);
        let o = r.feed("تم غائب.txt");
        assert!(o.output.contains("\"نوع\": \"غير موجود\""), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
        // كرر الأخير: rerun the last line (!!). Empty history
        // is a short Arabic error; it never reruns itself.
        let mut r = Repl::new();
        let o = r.feed("كرر الأخير");
        assert!(o.output.contains("لا يوجد سطر سابق"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("كرر الأخير").output, "");
        // The removed codes' builtins still work as code.
        assert_eq!(done_out(r.eval("طول(\"مرحبا\")")), "٥\n");
        assert_eq!(done_out(r.eval("مطلق(٠ - ٥)")), "٥\n");
        assert_eq!(done_out(r.eval("استورد رياضيات")), "");
        assert_eq!(done_out(r.eval("رياضيات.جذر(١٦)")), "٤\n");
        assert_eq!(done_out(r.eval("رياضيات.قوة(٢، ٣)")), "٨\n");
    }

    // تثبيت أدوات نيفا (أمر الاسم الكامل، بلا كود حرفين):
    // من مسار محلي إلى مجلد البرنامج، ثم الاستيراد —
    // في الوضعين (المترجم والمفسر التشخيصي).
    #[test]
    fn repl_full_command_install_local() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let home = dir.join("home");
            std::fs::create_dir(home.join("مصادر")).unwrap();
            std::fs::write(home.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n")
                .unwrap();
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            let o = r.feed("تثبيت مصادر/أداة.نيفا");
            assert!(!o.ended);
            assert_eq!(o.output, "ثُبّتت الوحدة أداة\n", "got: {}", o.output);
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٤٢\n"
            );
            // والوحدة المثبّتة تُستورد فورا.
            assert_eq!(r.feed("استورد أداة").output, "");
            assert_eq!(r.feed("أداة.قيمة").output, "٤٢\n");
            // الثانية مرفوضة: بلا كتابة صامتة فوق موجود.
            let o = r.feed("تثبيت مصادر/أداة.نيفا");
            assert!(
                o.output.contains("مثبّتة أصلا"),
                "got: {}",
                o.output
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٤٢\n"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // تثبيت: الرفوض — الوسائط، وليس .نيفا، والخروج عن
    // المجلد، والمجلد، والغائب، واسم الوحدة الأصلية،
    // والمخطط غير http/https، والشبكة المغلقة (البوابة
    // قبل الشبكة نفسها). والجدول يبقى 37: تثبيت ليس
    // كود اختصار أبدا.
    #[test]
    fn repl_full_command_install_rejections() {
        assert!(shortcut_def("تثبيت").is_none());
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("مصادر")).unwrap();
        std::fs::write(home.join("مصادر/نص.txt"), "نص").unwrap();
        std::fs::write(home.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ١\n").unwrap();
        std::fs::write(home.join("مصادر/رياضيات.نيفا"), "صدر س\nس = ١\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // العارية وحدها كود (اسم مدمج معروف تحليليا) —
        // فترد «متغير غير معرف» وقت التشغيل، كأي دالة
        // مدمجة عارية (طول) — لا خطأ وسائط.
        let o = r.feed("تثبيت");
        assert!(
            o.output.contains("متغير غير معرف: تثبيت"),
            "got: {}",
            o.output
        );
        let o = r.feed("تثبيت أ ب");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/نص.txt");
        assert!(o.output.contains(".نيفا"), "got: {}", o.output);
        // اجتماع 45 §59: من البيت الخروج من القاعدة
        // يلزم `../..` لا `..` وحده.
        let o = r.feed("تثبيت ../../خارج.نيفا");
        assert!(
            o.output.contains("خارج مجلد البرنامج"),
            "got: {}",
            o.output
        );
        let o = r.feed("تثبيت مصادر");
        assert!(o.output.contains("ليس ملفا"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/غائب.نيفا");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/رياضيات.نيفا");
        assert!(o.output.contains("وحدة أصلية"), "got: {}", o.output);
        let o = r.feed("تثبيت ftp://مثال.مصر/أداة.نيفا");
        assert!(o.output.contains("http وhttps"), "got: {}", o.output);
        // الشبكة مغلقة افتراضيا: البوابة، لا الشبكة.
        let o = r.feed("تثبيت http://127.0.0.1:9/أداة.نيفا");
        assert!(o.needs_network, "got: {}", o.output);
        assert!(
            o.output.contains(crate::network::PERMISSION_ERROR_MARKER),
            "got: {}",
            o.output
        );
        assert!(!dir.join("أداة.نيفا").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// خادم HTTP محلي صغير لاختبار التثبيت من رابط: يرد
    /// بمحتوى ثابت على أول طلب ثم ينتهي (اتصال واحد —
    /// بلا keep-alive، كقاعدة المحرك).
    fn install_test_server(body: &'static str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // يقبل حتى 4 طلبات (الاختبار ينزّل مرتين:
            // رابط خام + رابط مرمّز) ثم ينتهي.
            for _ in 0..4 {
                let Ok((mut s, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        port
    }

    // تثبيت من رابط: خادم محلي، شبكة مسموحة، مضيف محلي
    // مسموح — في الوضعين. يفترض التشغيل المتسلسل (CI
    // --test-threads=1): العلم العام يُعاد ضبطه في النهاية.
    #[test]
    fn repl_full_command_install_url() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let port = install_test_server("صدر قيمة\nقيمة = ٧\n");
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            r.set_network_allowed(true);
            r.allow_local_network_host("127.0.0.1");
            let o = r.feed(&format!(
                "تثبيت http://127.0.0.1:{}/أداة.نيفا",
                port
            ));
            assert!(!o.ended);
            assert_eq!(o.output, "ثُبّتت الوحدة أداة\nبصمة الملف: 3d5a2de9551ce2a171b674bd2360a7ed0d355a5d33e15b7893a4e361bd68678f\n", "got: {}", o.output);
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٧\n"
            );
            assert_eq!(r.feed("استورد أداة").output, "");
            assert_eq!(r.feed("أداة.قيمة").output, "٧\n");
            // والرابط المرمّز بالنسب المئوية (الصيغة التي
            // تصل بها الروابط العربية عادة) — الاسم بعد
            // الفك «أداة» أيضا، والتثبيت الثاني مرفوض.
            let o = r.feed(&format!(
                "تثبيت http://127.0.0.1:{}/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7",
                port
            ));
            assert!(o.output.contains("مثبّتة أصلا"), "got: {}", o.output);
            r.set_network_allowed(false);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // Time, version, and the session commands through the
    // shortcuts; fetch with the network closed (the default)
    // hits the permission gate, not the network.
    #[test]
    fn repl_shortcut_time_version_fetch_commands() {
        let mut r = Repl::new();
        let o = r.feed("تا");
        let date = o.output.trim_end().to_string();
        assert_eq!(date.len(), 19, "got: {}", date);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[10..11], " ");
        let o = r.feed("اص");
        assert_eq!(o.output, "محرك نيفا ١٫٠٫٠\n");
        let o = r.feed("وق");
        assert!(parse_shortcut_number(o.output.trim()).is_some(), "got: {}", o.output);
        let o = r.feed("نو ٠");
        assert_eq!(o.output, "");
        let o = r.feed("اج https://example.com");
        assert!(o.needs_network, "got: {}", o.output);
        assert!(
            o.output.contains(crate::network::PERMISSION_ERROR_MARKER),
            "got: {}",
            o.output
        );
        // Session commands: help, show history, clear, quit.
        let o = r.feed("اس");
        assert!(o.output.contains(":خروج"), "got: {}", o.output);
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("امسح الجلسة");
        assert!(!o.ended);
        let o = r.feed(":اعرض");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed(":خروج");
        assert!(o.ended);
    }

    // Owner's orders 2026-11-05 ~22:58 and ~23:03: the prompt
    // is the current directory, terminal style — `~ $ ` at
    // home, `~/dir $ ` below it, `> ` while a block is open —
    // never `نيفا: `.
    #[test]
    fn repl_prompt_is_directory_and_dollar() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("س = ٥");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("انتقل ..");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("دالة جمع(أ، ب):");
        assert_eq!(o.prompt, "> ");
        let o = r.feed("");
        assert_eq!(o.prompt, "~ $ ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-12: shortcut codes never shadow Niva names —
    // `ات = ٥` runs as code, and once defined `ات` prints its
    // value; in a fresh session `ات` is still the file list.
    #[test]
    fn repl_shortcut_names_do_not_shadow_code() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // Undefined: the shortcut owns the bare name.
        let o = r.feed("ات");
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        // An assignment to the same name is code, not a
        // shortcut (the old code errored here).
        let o = r.feed("ات = ٥");
        assert_eq!(o.output, "", "got: {}", o.output);
        // Now defined: the bare name prints its value.
        let o = r.feed("ات");
        assert_eq!(o.output, "٥\n", "got: {}", o.output);
        // The real listing is still reachable by its full name.
        let o = r.feed("اعرض_الملفات()");
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-2: `:اسمح-بالشبكة` flips the network flag
    // inside the running process — no restart, no lost state.
    #[test]
    fn repl_allow_network_command_flips_flag_in_process() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("الشبكة مغلقة افتراضيا"), "got: {}", o.output);
        assert!(o.needs_network);
        // The control command: no output, no restart.
        let o = r.feed(":اسمح-بالشبكة");
        assert_eq!(o.output, "");
        assert!(!o.ended);
        // The same fetch now passes the gate (it fails on the
        // refused connection instead) — and the session state
        // survived: س is still defined.
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        let o = r.feed("س");
        assert_eq!(o.output, "٥\n");
        crate::network::set_network_allowed(false);
    }

    // Audit م-6: the structured control signal of the last
    // line — NetworkNeeded on a refused fetch, SessionCleared
    // on :مسح, None otherwise.
    #[test]
    fn repl_last_control_signals() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let _ = r.feed("س = ٥");
        assert_eq!(r.last_control(), ReplControl::None);
        let _ = r.feed("استورد شبكة");
        let _ = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert_eq!(r.last_control(), ReplControl::NetworkNeeded);
        let _ = r.feed("امسح الجلسة");
        assert_eq!(r.last_control(), ReplControl::SessionCleared);
        let _ = r.feed("س = ٦");
        assert_eq!(r.last_control(), ReplControl::None);
        crate::network::set_network_allowed(false);
    }

    // Audit م-16: وقت.نوم caps at 60 seconds.
    #[test]
    fn repl_sleep_caps_at_sixty() {
        let mut r = Repl::new();
        let o = r.feed("نو ٦١");
        assert!(o.output.contains("60 ثانية"), "got: {}", o.output);
        let o = r.feed("نو ٠");
        assert_eq!(o.output, "");
    }

    // ك-2 (مهمة 7): وضع التشخيص الخفي — الجلسة نفسها على
    // مفسر الشجرة (Repl::new_interpreter): المتغيرات تبقى،
    // والتعبير الأخير يُطبع، والشبكة تُفتح من داخل الجلسة.
    #[test]
    fn repl_allow_server_command_flips_flag_in_process() {
        // (اجتماع 27، س4): :اسمح-بالخادم يقلب علم الخادم
        // للجلسة نفسها، بلا إعادة تشغيل؛ و:اسمح-بالخادم
        // لا يفتح الشبكة العامة (العلمان مستقلان).
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        let mut r = Repl::new();
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        // المعالج دالة مجهولة كمعامل — القيمة نفسها
        // لا تهم: فحص الإذن يأتي قبل أي شيء آخر.
        let line = "شبكة.اخدم(١٨٤٤٣، \"server-cert.pem\"، \"server-key.pem\"، دالة_مجهولة(طلب): \"x\")";
        let o = r.feed(line);
        assert!(
            o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::ServerNeeded);
        assert!(!o.needs_network, "the server gate must not set the network signal");
        let o = r.feed(":اسمح-بالخادم");
        assert_eq!(o.output, "");
        assert!(!o.ended);
        // الاستدعاء نفسه يتجاوز البوابة الآن — يفشل على
        // ملف الشهادة الغائب (مع الميزة) أو على غياب
        // دعم TLS (بلا الميزة) بدلا من رفض الإذن —
        // وبقيت الجلسة حية بعد الأمر.
        let o = r.feed(line);
        assert!(
            !o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        #[cfg(feature = "https")]
        let expected = "تعذر قراءة ملف الشهادة";
        #[cfg(not(feature = "https"))]
        let expected = "الخادم يحتاج HTTPS";
        assert!(
            o.output.contains(expected),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::None);
        let o = r.feed("اطبع ٥");
        assert_eq!(o.output, "٥\n");
    }

    #[test]
    fn repl_server_and_network_flags_are_independent() {
        // (اجتماع 27، س4): علم الخادم وحده لا يفتح
        // الشبكة العامة (العلمان مستقلان).
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        let mut r = Repl::new();
        let o = r.feed(":اسمح-بالخادم");
        assert_eq!(o.output, "");
        // الشبكة العامة ما زالت مغلقة: اجلب مرفوض.
        let o = r.feed("استورد شبكة\nاطبع شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::NetworkNeeded);
        // والخادم في الوقت نفسه مفتوح: الاستدعاء يتجاوز
        // البوابة ويفشل على ملف الشهادة الغائب (مع
        // الميزة) أو على غياب دعم TLS (بلا الميزة) فقط.
        let o = r.feed("شبكة.اخدم(١٨٤٤٣، \"server-cert.pem\"، \"server-key.pem\"، دالة_مجهولة(طلب): \"x\")");
        assert!(
            !o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        #[cfg(feature = "https")]
        let expected = "تعذر قراءة ملف الشهادة";
        #[cfg(not(feature = "https"))]
        let expected = "الخادم يحتاج HTTPS";
        assert!(
            o.output.contains(expected),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::None);
    }

    #[test]
    fn repl_interpreter_mode_session_works() {
        let mut r = Repl::new_interpreter();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        assert_eq!(done_out(r.eval("س * ٢")), "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
        assert!(!r.network_allowed());
        let _ = r.feed(":اسمح-بالشبكة");
        assert!(r.network_allowed());
        r.reset();
        assert!(is_failed(r.eval("س")));
    }

    // ك-2 (مهمة 7): الدوال العادية تعمل على محرك VM —
    // عدد/مجموع تمر عبر VmSession::call_builtin مباشرة.
    // (حع/مع left the table 2026-11-08 — plain calls now.)
    #[test]
    fn repl_vm_shortcut_calls_builtin() {
        let mut r = Repl::new();
        assert_eq!(r.feed("عدد(\"٥\")").output, "٥\n");
        assert_eq!(r.feed("مجموع([١، ٢، ٣])").output, "٦\n");
        assert_eq!(r.feed("مح").output, "~\n");
    }

    // العبارات الكاملة (2026-11-08، أمر المالك «اكتب دول
    // كاملين»): المحرك الست — بحثان ومسح جلسة وتحويل
    // ورجوع للبيت وتكرار — مسارات سعيدة ورفوض وسائط.
    #[test]
    fn repl_full_phrases() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "ألف\nباء\n").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // ابحث في ملف: نمط + اسم.
        let o = r.feed("ابحث في ملف ألف ملف.txt");
        assert_eq!(o.output, "ألف\n");
        // ابحث عن ملف: اسم.
        let o = r.feed("ابحث عن ملف ملف");
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        // حول إلى عدد: نص (بلا شدة على الواو).
        assert_eq!(r.feed("حول إلى عدد ٥").output, "٥\n");
        // الفاصلة العشرية العربية ٫ تعمل كما كانت في حع
        // القديم (المعامل يُحلَّل عددا أولا قبل التمرير).
        assert_eq!(r.feed("حول إلى عدد ٣٫٥").output, "٣٫٥\n");
        // نص ليس عددا: خطأ عربي قصير.
        let o = r.feed("حول إلى عدد نص");
        assert!(o.output.contains("لا يمكن تحويل"), "got: {}", o.output);
        // ارجع للبيت: من فرعي إلى البيت.
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("ارجع للبيت");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        // امسح الجلسة: تعيد ضبط المحرك كـ :مسح تماما.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("امسح الجلسة");
        assert!(!o.ended);
        let o = r.feed("س");
        assert!(o.output.contains("متغير غير معرف"), "got: {}", o.output);
        // كرر الأخير: بلا سطر سابق خطأ قصير؛ وبعده يعيد
        // تنفيذ آخر سطر، ولا يعيد تنفيذ نفسه أبدا.
        let o = r.feed("كرر الأخير");
        assert!(o.output.contains("لا يوجد سطر سابق"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        // رفوض الوسائط: بنفس أسلوب الاختصارات.
        let o = r.feed("ابحث في ملف ألف");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("ابحث في ملف ألف ملف.txt زيادة");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 3"), "got: {}", o.output);
        let o = r.feed("ابحث عن ملف");
        assert!(o.output.contains("تتوقع 1 معاملات لكن أعطيت 0"), "got: {}", o.output);
        let o = r.feed("حول إلى عدد");
        assert!(o.output.contains("تتوقع 1 معاملات لكن أعطيت 0"), "got: {}", o.output);
        let o = r.feed("امسح الجلسة زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("ارجع للبيت زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("كرر الأخير زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الأكواد المحذوفة (2026-11-08) لا تنفذ اختصارا أبدا
    // بعد الحذف: تسقط إلى الكود وتخطئ كاسم غير معرف،
    // بلا فعل — فـ«بم» لا تبحث، و«مس» لا تمسح الجلسة،
    // و«كا» لا تكرر.
    #[test]
    fn repl_removed_codes_do_not_execute() {
        let dir = shortcut_dir();
        std::fs::write(dir.join("ملف.txt"), "ألف\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        for line in [
            "بم ألف ملف.txt",
            "بع ملف",
            "مس",
            "مع ١ ٢",
            "اد ٥ ٢",
            "اق ٥ ٢",
            "حع ٥",
            "مى ١ ٥",
            "ال",
            "كا",
        ] {
            let o = r.feed(line);
            assert!(
                o.output.contains("متغير غير معرف"),
                "line {}: got: {}",
                line,
                o.output
            );
            assert!(!o.ended, "line {} ended the session", line);
        }
        // و«مس» لم تمسح الجلسة: س ما زالت ٥.
        assert_eq!(r.feed("س").output, "٥\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ك-2 (مهمة 7): إغلاق معرّف في سطر يبقى قابلا للاستدعاء
    // في سطر لاحق — الجلسة الدائمة على VM تحتفظ بالبيئة
    // وسجل الإغلاقات عبر التحميلات.
    #[test]
    fn repl_vm_closure_persists_across_feeds() {
        let mut r = Repl::new();
        assert_eq!(
            done_out(r.eval("دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد")),
            ""
        );
        assert_eq!(done_out(r.eval("أ = اصنع_عداد()")), "");
        assert_eq!(done_out(r.eval("أ()")), "١\n");
        assert_eq!(done_out(r.eval("أ()")), "٢\n");
    }

    /// مجلد مؤقت لاختبارات `شغّل` — ملفات فقط، ويُمسح
    /// في نهاية كل اختبار.
    fn run_test_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-run-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// برنامج مثال بشرطة في اسمه يستورد وحدة: العرض
    /// المصغر لتدفق NivaDB المستهدف.
    fn write_dashed_example(dir: &std::path::Path) {
        std::fs::write(dir.join("قاعدة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n").unwrap();
        std::fs::write(
            dir.join("مثال-أول.نيفا"),
            "استورد قاعدة\nاطبع قاعدة.قيمة\nمتغير_البرنامج = ٥\n",
        )
        .unwrap();
    }

    // اجتماع 43 §58 + تجاوز المالك «بلا تعقيد»: `شغّل`
    // بالاسم العاري وبالاسم الكامل، بملف بشرطة يستورد
    // وحدة — والجلسة معزولة (متغير البرنامج لا يتسرب)
    // والتفاعلي حي بعد التشغيل.
    #[test]
    fn repl_run_command_dashed_name_and_isolation() {
        let dir = run_test_dir("dash");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        // الاسم العاري بلا امتداد (الإلحاق التلقائي).
        let o = r.feed("شغّل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        assert!(!o.ended);
        // والصيغة القانونية بلا تشكيل تعمل أيضا.
        let o = r.feed("شغل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        // متغير البرنامج لا يتسرب إلى التفاعلي.
        let o = r.feed("متغير_البرنامج");
        assert!(
            o.output.contains("متغير غير معرف"),
            "got: {}",
            o.output
        );
        // والاسم الكامل بالامتداد يشغّل أيضا.
        let o = r.feed("شغّل مثال-أول.نيفا");
        assert_eq!(o.output, "٤٢\n");
        // والتفاعلي حي بعد التشغيلين.
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: `شغّل` على المفسر أيضا — جلسة
    // جديدة على محرك التفاعلي الحالي لا على الافتراضي.
    #[test]
    fn repl_run_command_interpreter_mode() {
        let dir = run_test_dir("interp");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir_interpreter(dir.clone());
        let o = r.feed("شغّل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 45 §59 + تجاوز المالك: الأخطاء العربية
    // القصيرة — ملف غائب، صفر معاملات، معاملات زائدة،
    // اسم فيه مسار — والتفاعلي حي بعد كل خطأ.
    #[test]
    fn repl_run_command_errors() {
        let dir = run_test_dir("err");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغّل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert!(o.output.contains("غائب.نيفا"), "got: {}", o.output);
        let o = r.feed("شغّل");
        assert!(
            o.output.contains("معاملا واحدا"),
            "got: {}",
            o.output
        );
        let o = r.feed("شغّل أ ب");
        assert!(
            o.output.contains("معاملا واحدا"),
            "got: {}",
            o.output
        );
        let o = r.feed("شغّل ../غائب.نيفا");
        assert!(o.output.contains("بلا مسار"), "got: {}", o.output);
        let o = r.feed("شغّل مجلد/ملف.نيفا");
        assert!(o.output.contains("بلا مسار"), "got: {}", o.output);
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: خطأ وقت التشغيل في البرنامج يُطبع
    // بالعربية (بعد مخرجاته الجزئية) ويعود التحكم إلى
    // الموجه بلا خروج من التفاعلي.
    #[test]
    fn repl_run_command_runtime_error_returns_to_prompt() {
        let dir = run_test_dir("rterr");
        std::fs::write(dir.join("خاطئ.نيفا"), "اطبع ١\nس = ١ / ٠\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغّل خاطئ");
        assert!(o.output.starts_with("١\n"), "got: {}", o.output);
        assert!(o.output.contains("خطأ"), "got: {}", o.output);
        assert!(!o.ended);
        assert_eq!(r.feed("٢ + ٢").output, "٤\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: `شغّل` تفاعلي فقط — لا دالة
    // برنامجية: من استدعاه من الكود يأخذ خطأ الاسم غير
    // المعرف المعتاد.
    #[test]
    fn repl_run_command_not_a_builtin() {
        let mut r = Repl::new();
        let failed = match r.eval("شغّل(\"x.نيفا\")") {
            ReplEval::Failed { error, .. } => error,
            ReplEval::Done { .. } => panic!("expected Failed, got Done"),
            ReplEval::Incomplete => panic!("expected Failed, got Incomplete"),
        };
        assert!(
            failed.contains("متغير غير معرف"),
            "got: {}",
            failed
        );
        // المحلل يجرّد الشدة فيبلغ الاسم بلا شدة (درس
        // المهمة 24): «شغل» لا «شغّل».
        assert!(failed.contains("شغل"), "got: {}", failed);
    }

    // اجتماع 45 §59 — الدفعة 2: معلومات، أعد-الإعدادات،
    // أعد-الضبط، النقل والقفل، الترحيل، البدء والمقتطفات،
    // والبحث في المكانين.

    #[test]
    fn batch2_info_is_exactly_five_lines() {
        let dir = shortcut_dir();
        // وحدتان مثبّتتان في جذر مجلد البرنامج.
        std::fs::write(dir.join("أداة.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("أداة٢.نيفا"), "صدر س\nس = ٢\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("معلومات");
        let lines: Vec<&str> = o.output.lines().collect();
        assert_eq!(lines.len(), 5, "got: {}", o.output);
        assert_eq!(lines[0], "محرك نيفا ١٫٠٫٠");
        assert_eq!(lines[1], "ترمنال التطبيق: غير معروف");
        assert_eq!(lines[2], "الوحدات المثبتة: ٢");
        // الشبكة مغلقة افتراضيا في الاختبار: غير متصلة.
        assert_eq!(lines[3], "الشبكة: غير متصلة");
        assert_eq!(
            lines[4],
            "مصدر الفهرس: https://raw.githubusercontent.com/RIO4140/niva-index/main/فهرس.حزمة"
        );
        // بلا معاملات.
        let o = r.feed("معلومات زيادة");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_reload_settings_signals_and_names_restart_keys() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(home.join("إعدادات/niva.properties"), "المظهر=فاتح\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.last_control(), ReplControl::None);
        let o = r.feed("أعد-الإعدادات");
        assert!(o.output.contains("أُعيدت قراءة الإعدادات"), "got: {}", o.output);
        assert!(o.output.contains("إخفاء_لوحة_المفاتيح_عند_البدء"), "got: {}", o.output);
        assert!(o.output.contains("خط_مخصص"), "got: {}", o.output);
        assert!(o.output.contains("ملء_الشاشة"), "got: {}", o.output);
        assert!(o.output.contains("الترحيب يسري من الجلسة الجديدة"), "got: {}", o.output);
        assert_eq!(r.last_control(), ReplControl::SettingsReload);
        // سطر التأكيد واحد (بلا إشعارات إضافية هنا).
        assert_eq!(o.output.lines().count(), 1, "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_reset_asks_confirms_and_keeps_home() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(home.join("إعدادات/niva.properties"), "المظهر=فاتح\n").unwrap();
        std::fs::write(home.join("ملف-مستخدم.txt"), "ملفي").unwrap();
        std::fs::write(dir.join("أداة.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("session-1.txt"), "جلسة").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // الطلب: رسالة تسمّي المسح والبقاء والجلسة.
        let o = r.feed("أعد-الضبط");
        assert!(o.output.contains("سيُمسح"), "got: {}", o.output);
        assert!(o.output.contains("جلسة محفوظة"), "got: {}", o.output);
        assert!(o.output.contains("تبقى ملفاتك في البيت"), "got: {}", o.output);
        // أي سطر آخر يلغي — والسطر نفسه يُعالج عاديا.
        let o = r.feed("١ + ١");
        assert!(o.output.starts_with("أُلغي إعادة الضبط."), "got: {}", o.output);
        assert!(o.output.contains("٢"), "got: {}", o.output);
        assert!(dir.join("أداة.نيفا").exists());
        // الطلب ثم «نعم»: المسح يتم.
        let _ = r.feed("أعد-الضبط");
        let o = r.feed("نعم");
        assert!(o.output.contains("أُعيد ضبط مجلد البرنامج"), "got: {}", o.output);
        assert!(!dir.join("أداة.نيفا").exists());
        assert!(!dir.join("session-1.txt").exists());
        // البيت والإعدادات باقيان، والمشتقات أُعيد بناؤها.
        assert!(home.join("ملف-مستخدم.txt").exists());
        assert!(home.join("إعدادات/niva.properties").exists());
        assert!(dir.join("tmp").is_dir());
        assert!(dir.join(".نيفا").is_dir());
        // مجلد الجلسة عاد إلى البيت.
        assert_eq!(r.feed("مح").output.trim(), "~");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_source_checkout_skips_home_move() {
        // اجتماع 49 §63: مجلد مصدر (Cargo.toml) لا يُنقل
        // منه شيء — الحارس يمنع كارثة batch2e (ترحيل
        // شجرة المصدر كاملة إلى home/).
        let dir = shortcut_dir();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/مهم.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        assert!(!notices.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices);
        assert!(dir.join("Cargo.toml").exists());
        assert!(dir.join("src/مهم.rs").exists());
        assert!(dir.join("ملاحظة.txt").exists());
        // القفل يُنشأ بصمت رغم التخطي.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_project_dir_skips_home_move() {
        // اجتماع 46 §60: مجلد مشروع لا يُنقل — لا
        // `مصدر/` ولا `حزم/` ولا `بيان.حزمة`.
        let dir = shortcut_dir();
        std::fs::create_dir_all(dir.join("مصدر")).unwrap();
        std::fs::write(dir.join("مصدر/رئيسي.نيفا"), "اطبع \"ناجح\"\n").unwrap();
        std::fs::create_dir_all(dir.join("حزم/حزمة_صغيرة")).unwrap();
        std::fs::write(dir.join("بيان.حزمة"), "اسم: مشروعي\n").unwrap();
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        assert!(!notices.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices);
        assert!(dir.join("مصدر/رئيسي.نيفا").exists());
        assert!(dir.join("حزم/حزمة_صغيرة").is_dir());
        assert!(dir.join("بيان.حزمة").exists());
        assert!(dir.join("ملاحظة.txt").exists());
        // القفل يُنشأ بصمت رغم التخطي.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_first_run_moves_old_files_with_exclusions() {
        let dir = shortcut_dir();
        // ملفات قديمة في الجذر (قبل الفصل).
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        std::fs::write(dir.join("قديم.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("niva.properties"), "الترحيب=لا\n").unwrap();
        std::fs::write(dir.join("session-1.txt"), "جلسة").unwrap();
        std::fs::create_dir(dir.join("tmp")).unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        // رسالة النقل تسمّي المنقول.
        assert!(notices.iter().any(|n| n.contains("ملاحظة.txt")), "got: {:?}", notices);
        assert!(dir.join("home/ملاحظة.txt").exists());
        // الاستثناءات بقيت في الجذر.
        assert!(dir.join("قديم.نيفا").exists());
        assert!(dir.join("session-1.txt").exists());
        assert!(dir.join("tmp").is_dir());
        // الإعدادات انتقلت إلى ~/إعدادات/.
        assert!(dir.join("home/إعدادات/niva.properties").exists());
        assert!(!dir.join("niva.properties").exists());
        // القفل وُجد، والتشغيل الثاني بلا نقل ولا رسالة نقل.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let notices2 = ensure_first_run(&dir).unwrap();
        assert!(!notices2.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_move_lock_inconsistent_names_lock() {
        // البند 50: قفل موجود والبيت غائب — الخطأ يسمّي
        // القفل بصيغته القصيرة ويطلب حذفه يدويا.
        let dir = shortcut_dir();
        let home = dir.join("home");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(dir.join(".نيفا")).unwrap();
        std::fs::write(dir.join(".نيفا/نقل-البيت.قفل"), "").unwrap();
        let err = ensure_first_run(&dir).unwrap_err();
        assert!(err.contains(".نيفا/نقل-البيت.قفل"), "got: {}", err);
        assert!(err.contains("احذف الملف يدويا"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_retired_key_migration_is_pure_and_named() {
        // البند 75: الآلية خالصة — بجدول عينة، إذ لا
        // مفتاح أُعيدت تسميته بعد حتى الآن.
        let (out, notices) = migrate_retired_keys(
            "# تعليق\nمفتاح_قديم=قيمة\nمفتاح_آخر=١\n",
            &[("مفتاح_قديم", "مفتاح_جديد")],
        );
        assert_eq!(out, "# تعليق\nمفتاح_جديد=قيمة\nمفتاح_آخر=١\n");
        assert_eq!(notices, vec!["المفتاح القديم مفتاح_قديم صار مفتاح_جديد"]);
        // بلا جدول: النص كما هو بلا إشعارات.
        let (out, notices) = migrate_retired_keys("أ=١\n", &[]);
        assert_eq!(out, "أ=١\n");
        assert!(notices.is_empty());
    }

    #[test]
    fn batch2_settings_value_validation_drops_nul() {
        // البند 41: قيمة بلا بايت فارغ — القيمة غير
        // الصالحة تُسقط مع إشعار يسمّي المفتاح.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(
            home.join("إعدادات/niva.properties"),
            "المظهر=فاتح\nتالف=قبل\0بعد\n",
        )
        .unwrap();
        let (entries, notices) = read_settings(&dir);
        assert!(entries.iter().any(|(k, v)| k == "المظهر" && v == "فاتح"));
        assert!(!entries.iter().any(|(k, _)| k == "تالف"));
        assert!(notices.iter().any(|n| n.contains("تالف")), "got: {:?}", notices);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_startup_and_snippets_run_but_never_block() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مقتطفات")).unwrap();
        std::fs::write(home.join("بدء.نيفا"), "س = ٤٢\nاطبع \"بدء\"\n").unwrap();
        std::fs::write(home.join("مقتطفات/أ.نيفا"), "اطبع \"أ\"\n").unwrap();
        std::fs::write(home.join("مقتطفات/ب.نيفا"), "س = ١ / ٠\n").unwrap();
        std::fs::write(home.join("مقتطفات/ملاحظة.txt"), "ليس برنامجا\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let out = r.run_startup_files();
        // بدء ثم المقتطفات أبجديا؛ والملف التالف لا يمنع
        // الدخول: خطؤه يُطبع باسمه ويكمل الباقي.
        assert!(out.contains("بدء\n"), "got: {}", out);
        assert!(out.contains("أ\n"), "got: {}", out);
        assert!(out.contains("خطأ في ~/مقتطفات/ب.نيفا"), "got: {}", out);
        assert!(!out.contains("ملاحظة"), "got: {}", out);
        // تعريفات البدء بقيت في الجلسة.
        assert_eq!(r.feed("س").output, "٤٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_run_searches_program_dir_then_home() {
        let dir = run_test_dir("twoplace");
        // في المكانين معا: المثبَّت (الجذر) يفوز.
        std::fs::write(dir.join("مثال.نيفا"), "اطبع \"جذر\"\n").unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home/مثال.نيفا"), "اطبع \"بيت\"\n").unwrap();
        // في البيت وحده: يُوجد.
        std::fs::write(dir.join("home/بيتي.نيفا"), "اطبع \"بيتي\"\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("شغّل مثال").output, "جذر\n");
        assert_eq!(r.feed("شغّل بيتي").output, "بيتي\n");
        // الغائب يسمّي المكانين.
        let o = r.feed("شغّل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert!(o.output.contains("$نيفا"), "got: {}", o.output);
        assert!(o.output.contains("~"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
