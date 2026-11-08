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
        // Session commands (اس، عر) are control, not
        // content — like the :commands, they are not recorded.
        if !matches!(def.kind, ShortcutKind::Command) {
            self.history.push(line.to_string());
        }
        match def.kind {
            ShortcutKind::Command => Some(self.handle_command(def.key)),
            ShortcutKind::Echo => Some(ReplOutput {
                output: format!("{}\n", args.join(" ")),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            }),
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
    /// 37-code table (the table stays 37, and the app has no
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

    /// اقرأ ملف `.نيفا` من مجلد البرنامج وشغّله كبرنامج
    /// في جلسة جديدة على محرك التفاعلي الحالي (افتراضي
    /// أو مفسر) — جلسة التفاعلي نفسها لا تُمس ولا يتسرب
    /// شيء في أي اتجاه. المخرجات تُبث حية (أو تُجمع في
    /// ناتج الإرجاع حين يكون البث معطلا كما في
    /// الاختبارات)؛ وخطأ وقت التشغيل يُطبع بالعربية
    /// ويعود التحكم إلى الموجه بلا خروج من التفاعلي.
    fn run_program_file(&mut self, name: &str) -> ReplOutput {
        let path = self.program_dir.join(name);
        let is_file = std::fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            return ReplOutput {
                output: format!("الملف غير موجود: {}\n", name),
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
    /// مجموع/أدنى/أقصى/مدى تكفي في الكود. مطابقة بادئة
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

    /// The exact logic of the old `run_repl` loop, moved into
    /// the struct so the native loop and the WASM console share
    /// it: an empty line while a block is pending cancels it; a
    /// command line never belongs to the block; an indented line
    /// only waits; only a line at indent 0 can finish the block.
    fn feed_inner(&mut self, line: &str) -> ReplOutput {
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
                Ok(v) => ReplEval::Done {
                    output: format!("{}{}\n", printed, v.fmt_value()),
                },
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

/// `:مساعدة` output: a BARE command list — terminal convention,
/// not teaching. No explanations, no examples, no notes.
pub fn help_text() -> &'static str {
    ":مساعدة\n:اعرض\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:خروج\n"
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
    /// Print the arguments joined with spaces (echo).
    Echo,
    /// Evaluate a constructed stdlib snippet (رياضيات /
    /// عشوائي / وقت / شبكة / عمليات).
    Snippet,
    /// A REPL command (:مساعدة / :اعرض / :مسح / :خروج).
    Command,
}

/// One row of the 37-shortcut table
/// (~/workspace/android-app/SHORTCUTS-37.md): the code, the
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
        // --- ملفات ومجلدات (22) ---
        "ات" => ShortcutDef { key: "اعرض_الملفات", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "مح" => ShortcutDef { key: "المجلد_الحالي", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "نم" => ShortcutDef { key: "انتقل", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شم" => ShortcutDef { key: "أنشئ_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حم" => ShortcutDef { key: "احذف_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حف" => ShortcutDef { key: "احذف_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "سم" => ShortcutDef { key: "انسخ", min: 2, max: 2, kind: ShortcutKind::StrArgs },
        "قم" => ShortcutDef { key: "انقل", min: 2, max: 2, kind: ShortcutKind::StrArgs },
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
        "وج" => ShortcutDef { key: "موجود", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "ام" => ShortcutDef { key: "نوع_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "تم" => ShortcutDef { key: "تفاصيل_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عم" => ShortcutDef { key: "عد_الملفات", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        // --- أوامر الجلسة (2) ---
        "اس" => ShortcutDef { key: ":مساعدة", min: 0, max: 0, kind: ShortcutKind::Command },
        "عر" => ShortcutDef { key: ":اعرض", min: 0, max: 0, kind: ShortcutKind::Command },
        // --- إصدار المحرك (1) ---
        "اص" => ShortcutDef { key: "النسخة", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        // --- نص وأعداد (1) ---
        "رع" => ShortcutDef { key: "عشوائي", min: 0, max: 0, kind: ShortcutKind::Snippet },
        // --- وقت وشبكة (4) ---
        "تا" => ShortcutDef { key: "التاريخ_الآن", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "وق" => ShortcutDef { key: "وقت", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نو" => ShortcutDef { key: "نوم", min: 1, max: 1, kind: ShortcutKind::Snippet },
        "اج" => ShortcutDef { key: "اجلب", min: 1, max: 1, kind: ShortcutKind::Snippet },
        // --- طباعة (1) ---
        "اط" => ShortcutDef { key: "", min: 0, max: usize::MAX, kind: ShortcutKind::Echo },
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
    println!("{}", banner());
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
            "ألغيت الكتلة غير المكتملة.\n:مساعدة\n:اعرض\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:خروج\n"
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
        let mut r = Repl::new();
        let o = r.feed("المجلد_الحالي()");
        assert!(!o.ended);
        // Home displays as `~` (owner's order 2026-11-05
        // ~22:38), never the absolute internal path.
        assert_eq!(o.output.trim(), "~", "got: {}", o.output);
        let o = r.feed("اعرض_الملفات()");
        assert!(!o.ended);
        assert!(o.output.contains("Cargo.toml"), "got: {}", o.output);
    }

    // Short terminal commands (owner's order 2026-11-05 ~22:02):
    // two-letter codes, space-separated args, no comma ever.
    fn shortcut_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("niva-shortcut-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn repl_shortcut_list_and_pwd() {
        let dir = shortcut_dir();
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(dir.join("فرعي")).unwrap();
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_cd_mkdir_rmdir() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شم جديد");
        assert!(!o.ended);
        assert_eq!(o.output, "");
        assert!(dir.join("جديد").is_dir());
        let o = r.feed("نم جديد");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("جديد"), "got: {}", o.output);
        let o = r.feed("نم ..");
        assert_eq!(o.output, "");
        let o = r.feed("حم جديد");
        assert_eq!(o.output, "");
        assert!(!dir.join("جديد").exists());
        // Escaping the base is rejected.
        let o = r.feed("نم ..");
        assert!(o.output.contains("خارج مجلد البرنامج"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_show_file_and_rm_file() {
        let dir = shortcut_dir();
        std::fs::write(dir.join("أ.txt"), "نص الملف").unwrap();
        std::fs::create_dir(dir.join("مجلد")).unwrap();
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
        assert!(!dir.join("أ.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_copy_move_space_separated() {
        let dir = shortcut_dir();
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("سم أ.txt ب.txt");
        assert!(!o.ended);
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(dir.join("ب.txt")).unwrap(), "نص");
        let o = r.feed("قم ب.txt ج.txt");
        assert_eq!(o.output, "");
        assert!(!dir.join("ب.txt").exists());
        assert!(dir.join("ج.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_wrong_args_and_not_a_shortcut() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        // Wrong arg counts are short Arabic errors; the session survives.
        let o = r.feed("نم");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("سم أ.txt");
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

    // The 37-shortcut table (owner's correction 2026-11-05
    // ~22:11, superseding the 100): exactly 37 codes, all
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
    #[test]
    fn shortcut_table_is_thirty_seven_unique_codes() {
        let codes = [
            "ات", "مح", "نم", "شم", "حم", "حف", "سم", "قم", "اف", "شف", "عب", "عن", "عس",
            "عك", "عش", "حج", "شر", "اك", "وج", "ام", "تم", "عم", "اس", "عر",
            "اص", "رع", "تا", "وق", "نو", "اج", "اط",
            "عع", "نب", "اب",
        ];
        assert_eq!(codes.len(), 34, "engine codes (37 total - 3 app-level)");
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(shortcut_def(code).is_some(), "missing code: {}", code);
            assert!(seen.insert(code), "duplicate code: {}", code);
        }
        // The 9 original codes stay inside the 37.
        for code in ["ات", "مح", "نم", "شم", "حم", "حف", "سم", "قم", "اف"] {
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
        std::fs::write(dir.join("ملف.txt"), "ألف\nباء\nجيم\nدال\nهاء\nواو").unwrap();
        std::fs::create_dir(dir.join("فرعي")).unwrap();
        std::fs::write(dir.join("فرعي/ملف-داخلي.txt"), "داخل").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شف جديد.txt");
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(dir.join("جديد.txt")).unwrap(), "");
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
        let expected = std::fs::metadata(dir.join("ملف.txt")).unwrap().len() as f64;
        let o = r.feed("حج ملف.txt");
        assert_eq!(parse_shortcut_number(o.output.trim()), Some(expected));
        let o = r.feed("وج ملف.txt");
        assert_eq!(o.output, "صحيح\n");
        let o = r.feed("وج غائب.txt");
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
            std::fs::read_to_string(dir.join("رابط.txt")).unwrap(),
            "ألف\nباء\nجيم\nدال\nهاء\nواو"
        );
        // اك: path + the rest joined with spaces.
        let o = r.feed("اك ملف٢.txt مرحبا بالعالم");
        assert_eq!(o.output, "");
        assert_eq!(
            std::fs::read_to_string(dir.join("ملف٢.txt")).unwrap(),
            "مرحبا بالعالم"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Text and numbers: random and echo stay shortcuts; sum,
    // min, max, convert and range left the table 2026-11-08
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
        assert_eq!(r.feed("اط مرحبا بالعالم").output, "مرحبا بالعالم\n");
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
        std::fs::create_dir(dir.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("نم فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("ارجع للبيت");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        // تم: details of a file, a dir, and a missing path.
        std::fs::write(dir.join("ملف.txt"), "abc").unwrap();
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
        assert_eq!(r.feed("اط مرحبا").output, "مرحبا\n");
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
            std::fs::create_dir(dir.join("مصادر")).unwrap();
            std::fs::write(dir.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n")
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
        std::fs::create_dir(dir.join("مصادر")).unwrap();
        std::fs::write(dir.join("مصادر/نص.txt"), "نص").unwrap();
        std::fs::write(dir.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ١\n").unwrap();
        std::fs::write(dir.join("مصادر/رياضيات.نيفا"), "صدر س\nس = ١\n").unwrap();
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
        let o = r.feed("تثبيت ../خارج.نيفا");
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
        let o = r.feed("عر");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("امسح الجلسة");
        assert!(!o.ended);
        let o = r.feed("عر");
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
        std::fs::create_dir(dir.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("س = ٥");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("نم فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("نم ..");
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
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
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
        std::fs::write(dir.join("ملف.txt"), "ألف\nباء\n").unwrap();
        std::fs::create_dir(dir.join("فرعي")).unwrap();
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
        let o = r.feed("نم فرعي");
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
        assert_eq!(r.feed("اط مرحبا").output, "مرحبا\n");
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

    // اجتماع 43 §58 + تجاوز المالك: الأخطاء العربية
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
}
