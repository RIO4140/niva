//! Niva REPL (interactive terminal), Meeting 7.
//!
//! A REPL with RTL display, built on the tree-walking interpreter
//! (the stable default path), never on the experimental VM.
//!
//! Design:
//! - One Interpreter + one SemanticAnalyzer persist across lines,
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
//! - Commands (top-level only): :مساعدة :اعرض :مسح :خروج
//!   (and plain خروج). Arabic only — no Latin aliases.
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

use crate::ast::StmtKind;
use crate::interpreter::Interpreter;
use crate::lexer::tokenize;
use crate::parser::Parser;
use crate::semantic::SemanticAnalyzer;
use crate::value::Value;
use std::io::{BufRead, Write};
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

pub struct Repl {
    interp: Interpreter,
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
    // Audit م-6: the control signal of the last fed line
    // (None / NetworkNeeded / SessionCleared), read by
    // `last_control()` — a side channel so run_repl can emit
    // it on stderr without touching the display text.
    last_control: ReplControl,
}

impl Repl {
    pub fn new() -> Self {
        Self {
            interp: Interpreter::new(),
            analyzer: SemanticAnalyzer::new(),
            history: Vec::new(),
            pending: String::new(),
            trusted: false,
            network_allowed: false,
            last_control: ReplControl::None,
        }
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        self.interp.set_trusted(trusted);
    }

    /// REPL rooted at a given directory (tests; the app and the
    /// CLI use the process working directory via new()).
    pub fn with_program_dir(program_dir: std::path::PathBuf) -> Self {
        let mut r = Self::new();
        r.interp = Interpreter::with_program_dir(program_dir);
        r
    }

    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.network_allowed = allowed;
        self.interp.set_network_allowed(allowed);
    }

    /// This session's own network permission (per-instance:
    /// one console's approval never leaks into another).
    pub fn network_allowed(&self) -> bool {
        self.network_allowed
    }

    pub fn allow_local_network_host(&mut self, host: &str) {
        self.interp.allow_local_network_host(host);
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
            format!("{} $ ", self.interp.current_dir_display())
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
        // Session commands (اس، عر، مس، خج) are control, not
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
                    "جذر" => match parse_shortcut_number(&args[0]) {
                        Some(n) => format!("استورد رياضيات\nرياضيات.جذر({})", n),
                        None => {
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع عددا، وليس «{}»\n", code, args[0]),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    },
                    // NOTE: this one snippet uses a comma between
                    // the two arguments. If the comma-scope ruling
                    // ever bans commas in calls, this snippet must
                    // be rebuilt another way; the user never types
                    // a comma for a shortcut either way.
                    "قوة" => {
                        let a = match parse_shortcut_number(&args[0]) {
                            Some(n) => n,
                            None => {
                                return Some(ReplOutput {
                                    output: format!("خطأ: {} تتوقع أعدادا، وليس «{}»\n", code, args[0]),
                                    prompt: self.prompt(),
                                    ended: false,
                                    needs_network: false,
                                });
                            }
                        };
                        let b = match parse_shortcut_number(&args[1]) {
                            Some(n) => n,
                            None => {
                                return Some(ReplOutput {
                                    output: format!("خطأ: {} تتوقع أعدادا، وليس «{}»\n", code, args[1]),
                                    prompt: self.prompt(),
                                    ended: false,
                                    needs_network: false,
                                });
                            }
                        };
                        format!("استورد رياضيات\nرياضيات.قوة({}، {})", a, b)
                    }
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
            ShortcutKind::NumArgs | ShortcutKind::NumList => {
                let mut nums: Vec<Value> = Vec::new();
                for a in &args {
                    match parse_shortcut_number(a) {
                        Some(n) => nums.push(Value::Number(n)),
                        None => {
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع أعدادا، وليس «{}»\n", code, a),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    }
                }
                let values: Vec<Value> = if matches!(def.kind, ShortcutKind::NumList) {
                    vec![Value::List(nums)]
                } else {
                    nums
                };
                match self.interp.call_builtin(def.key, values, 1, 1) {
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
            ShortcutKind::RestJoined => {
                let values = vec![
                    Value::Str(args[0].clone()),
                    Value::Str(args[1..].join(" ")),
                ];
                match self.interp.call_builtin(def.key, values, 1, 1) {
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
                match self.interp.call_builtin(def.key, values, 1, 1) {
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
        out
    }

    /// Audit م-12: a line that parses as Niva code runs as
    /// code — shortcut codes must not shadow names. The check
    /// is a trial semantic analysis on a CLONE of the
    /// analyzer: parse-only is not enough ("طع -٥" parses as
    /// one binary expression but is not valid code while طع is
    /// undefined). One line of code is normally one statement,
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
        // Audit م-12 (code first): a line that parses as Niva
        // code runs as code — shortcut codes never shadow
        // names. Only when the line is not code does the
        // shortcut layer get its turn.
        if !self.line_is_code(&line) {
            if let Some(out) = self.try_shortcut(&line) {
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
            if let Err(e) = self.interp.run(head) {
                return ReplEval::Failed {
                    output: std::mem::take(&mut self.interp.output),
                    error: e.to_string(),
                };
            }
            let printed = std::mem::take(&mut self.interp.output);
            return match self.interp.eval_one(expr) {
                Ok(v) => ReplEval::Done {
                    output: format!("{}{}\n", printed, v.fmt_value()),
                },
                Err(e) => ReplEval::Failed {
                    output: printed,
                    error: e.to_string(),
                },
            };
        }
        match self.interp.run(&program) {
            Ok(()) => ReplEval::Done {
                output: std::mem::take(&mut self.interp.output),
            },
            Err(e) => ReplEval::Failed {
                output: std::mem::take(&mut self.interp.output),
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
        self.interp = Interpreter::new();
        self.interp.set_trusted(trusted);
        self.interp.set_network_allowed(network_allowed);
        self.analyzer = SemanticAnalyzer::new();
        self.history.clear();
        self.pending.clear();
    }
}

/// The greeting printed when a REPL session starts — the same
/// text the WASM console shows on load. Minimal by the owner's
/// no-explanation rule (2026-11-05): name + version only, like
/// Termux/Linux — no teaching, no how-to, no docs links.
pub fn banner() -> &'static str {
    "محرك نيفا ١٫٠٫٠"
}

/// `:مساعدة` output: a BARE command list — terminal convention,
/// not teaching. No explanations, no examples, no notes.
pub fn help_text() -> &'static str {
    ":مساعدة\n:اعرض\n:مسح\n:اسمح-بالشبكة\n:خروج\n"
}

pub enum ReplCommand {
    Help,
    Show,
    Clear,
    /// Audit م-2: allow the network inside the running
    /// process (no restart). Sent by the host app on approval.
    AllowNetwork,
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
    /// Arguments parsed as numbers, passed one by one.
    NumArgs,
    /// Arguments parsed as numbers, passed as one list.
    NumList,
    /// First argument is a path, the rest joined with spaces
    /// as one string (اكتب_ملف content may hold spaces).
    RestJoined,
    /// Print the arguments joined with spaces (echo).
    Echo,
    /// Evaluate a constructed stdlib snippet (رياضيات /
    /// عشوائي / وقت / شبكة).
    Snippet,
    /// A REPL command (:مساعدة / :اعرض / :مسح / :خروج).
    Command,
}

/// One row of the 50-shortcut table
/// (~/workspace/android-app/SHORTCUTS-50.md): the code, the
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
        // --- ملفات ومجلدات (19) ---
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
        "بم" => ShortcutDef { key: "ابحث_في_ملف", min: 2, max: 2, kind: ShortcutKind::StrArgs },
        "بع" => ShortcutDef { key: "ابحث_عن", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عش" => ShortcutDef { key: "اعرض_الشجرة", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        "حج" => ShortcutDef { key: "حجم_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شر" => ShortcutDef { key: "أنشئ_رابط", min: 2, max: 2, kind: ShortcutKind::StrArgs },
        "اك" => ShortcutDef { key: "اكتب_ملف", min: 2, max: usize::MAX, kind: ShortcutKind::RestJoined },
        "وج" => ShortcutDef { key: "موجود", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "ام" => ShortcutDef { key: "نوع_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عم" => ShortcutDef { key: "عد_الملفات", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        // --- أوامر الجلسة (4) ---
        "اس" => ShortcutDef { key: ":مساعدة", min: 0, max: 0, kind: ShortcutKind::Command },
        "عر" => ShortcutDef { key: ":اعرض", min: 0, max: 0, kind: ShortcutKind::Command },
        "مس" => ShortcutDef { key: ":مسح", min: 0, max: 0, kind: ShortcutKind::Command },
        "خج" => ShortcutDef { key: ":خروج", min: 0, max: 0, kind: ShortcutKind::Command },
        // --- إصدار المحرك (1) ---
        "اص" => ShortcutDef { key: "النسخة", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        // --- نص وأعداد (10) ---
        "طن" => ShortcutDef { key: "طول", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "مع" => ShortcutDef { key: "مجموع", min: 1, max: usize::MAX, kind: ShortcutKind::NumList },
        "اد" => ShortcutDef { key: "أدنى", min: 1, max: usize::MAX, kind: ShortcutKind::NumList },
        "اق" => ShortcutDef { key: "أقصى", min: 1, max: usize::MAX, kind: ShortcutKind::NumList },
        "طع" => ShortcutDef { key: "مطلق", min: 1, max: 1, kind: ShortcutKind::NumArgs },
        "حع" => ShortcutDef { key: "عدد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "مى" => ShortcutDef { key: "مدى", min: 1, max: 3, kind: ShortcutKind::NumArgs },
        "رع" => ShortcutDef { key: "عشوائي", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "جذ" => ShortcutDef { key: "جذر", min: 1, max: 1, kind: ShortcutKind::Snippet },
        "قو" => ShortcutDef { key: "قوة", min: 2, max: 2, kind: ShortcutKind::Snippet },
        // --- وقت وشبكة (4) ---
        "تا" => ShortcutDef { key: "التاريخ_الآن", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "وق" => ShortcutDef { key: "وقت", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نو" => ShortcutDef { key: "نوم", min: 1, max: 1, kind: ShortcutKind::Snippet },
        "اج" => ShortcutDef { key: "اجلب", min: 1, max: 1, kind: ShortcutKind::Snippet },
        // --- طباعة (1) ---
        "اط" => ShortcutDef { key: "", min: 0, max: usize::MAX, kind: ShortcutKind::Echo },
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
        ":مسح" => Some(ReplCommand::Clear),
        ":اسمح-بالشبكة" => Some(ReplCommand::AllowNetwork),
        ":خروج" | "خروج" => Some(ReplCommand::Quit),
        _ => None,
    }
}

/// The interactive loop: read lines from stdin, feed them to
/// the stateful `Repl`, print what it returns. Returns SUCCESS
/// on :خروج or EOF.
pub fn run_repl(trusted: bool, network_allowed: bool, local_hosts: &[String]) -> ExitCode {
    let mut repl = Repl::new();
    repl.set_trusted(trusted);
    repl.set_network_allowed(network_allowed);
    for host in local_hosts {
        repl.allow_local_network_host(host);
    }
    println!("{}", banner());
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        print!("{}", repl.prompt());
        if out.flush().is_err() {
            return ExitCode::FAILURE;
        }
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => {
                // EOF (Ctrl-D): exit cleanly.
                let result = repl.eof();
                print!("{}", result.output);
                return ExitCode::SUCCESS;
            }
            Ok(_) => {}
            Err(_) => return ExitCode::FAILURE,
        }
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
        assert!(matches!(parse_command(":مسح"), Some(ReplCommand::Clear)));
        assert!(matches!(parse_command(":خروج"), Some(ReplCommand::Quit)));
        assert!(matches!(parse_command("خروج"), Some(ReplCommand::Quit)));
        // No Latin aliases: :help / :quit / exit are not commands.
        assert!(parse_command(":help").is_none());
        assert!(parse_command("exit").is_none());
        assert!(parse_command("س = ٥").is_none());
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
            "ألغيت الكتلة غير المكتملة.\n:مساعدة\n:اعرض\n:مسح\n:اسمح-بالشبكة\n:خروج\n"
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

    #[test]
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

    // The 50-shortcut table (owner's correction 2026-11-05
    // ~22:11, superseding the 100): exactly 50 engine codes,
    // all present, no collisions. The 7 app-level codes (جل،
    // جد، مش، شب، سش، مد، اع) are NOT here — the app
    // intercepts them before the line reaches the engine.
    #[test]
    fn shortcut_table_is_fifty_unique_codes() {
        let codes = [
            "ات", "مح", "نم", "شم", "حم", "حف", "سم", "قم", "اف", "شف", "عب", "عن", "عس",
            "عك", "بم", "بع", "عش", "حج", "شر", "اك", "وج", "ام", "عم", "اس", "عر", "مس",
            "خج", "اص", "طن", "مع", "اد", "اق", "طع", "حع", "مى", "رع", "جذ", "قو", "تا",
            "وق", "نو", "اج", "اط",
        ];
        assert_eq!(codes.len(), 43, "engine codes (50 total - 7 app-level)");
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(shortcut_def(code).is_some(), "missing code: {}", code);
            assert!(seen.insert(code), "duplicate code: {}", code);
        }
        // The 9 original codes stay inside the 50.
        for code in ["ات", "مح", "نم", "شم", "حم", "حف", "سم", "قم", "اف"] {
            assert!(shortcut_def(code).is_some(), "original code lost: {}", code);
        }
        // App-level codes must NOT be engine shortcuts.
        for code in ["جل", "جد", "مش", "شب", "سش", "مد", "اع"] {
            assert!(shortcut_def(code).is_none(), "app code leaked into engine: {}", code);
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
        let o = r.feed("بم ألف ملف.txt");
        assert_eq!(o.output, "ألف\n");
        let o = r.feed("بع ملف");
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

    // Text and numbers through the shortcuts: length, sum,
    // min, max, abs, convert, range, random, sqrt, power, echo.
    #[test]
    fn repl_shortcut_numbers_and_text() {
        let mut r = Repl::new();
        assert_eq!(r.feed("طن مرحبا").output, "٥\n");
        assert_eq!(r.feed("مع ١ ٢ ٣").output, "٦\n");
        assert_eq!(r.feed("اد ٥ ٢ ٨").output, "٢\n");
        assert_eq!(r.feed("اق ٥ ٢ ٨").output, "٨\n");
        assert_eq!(r.feed("طع -٥").output, "٥\n");
        assert_eq!(r.feed("حع ٥").output, "٥\n");
        assert_eq!(r.feed("مى ١ ٥").output, "١\n٢\n٣\n٤\n");
        let o = r.feed("رع");
        let n = parse_shortcut_number(o.output.trim());
        assert!(matches!(n, Some(v) if (0.0..1.0).contains(&v)), "got: {}", o.output);
        assert_eq!(r.feed("جذ ١٦").output, "٤\n");
        assert_eq!(r.feed("قو ٢ ٣").output, "٨\n");
        assert_eq!(r.feed("اط مرحبا بالعالم").output, "مرحبا بالعالم\n");
        // A non-number where a number is expected: short error.
        let o = r.feed("مع ١ س");
        assert!(o.output.contains("تتوقع أعدادا"), "got: {}", o.output);
        let o = r.feed("جذ س");
        assert!(o.output.contains("تتوقع عددا"), "got: {}", o.output);
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
        let o = r.feed("مس");
        assert!(!o.ended);
        let o = r.feed("عر");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("خج");
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
        let _ = r.feed("مس");
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
}
