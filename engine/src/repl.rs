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
//! - File policy is the same as file mode: restricted to the
//!   current working directory, unless --allow-all was given.
//! - Commands (top-level only): :مساعدة :اعرض :مسح :خروج
//!   (plus :help :show :clear :quit, and plain خروج / exit).

use crate::ast::StmtKind;
use crate::interpreter::Interpreter;
use crate::lexer::tokenize;
use crate::parser::Parser;
use crate::semantic::SemanticAnalyzer;
use std::io::{BufRead, Write};
use std::process::ExitCode;

/// Parser errors that mean "this input is incomplete, read more".
const INCOMPLETE_MARKERS: [&str; 2] =
    ["نهاية غير متوقعة داخل كتلة", "توقع كتلة بمسافة بادئة بعد النقطتين"];

/// Unicode bidi isolates, used by :اعرض for RTL display.
const RLI: char = '\u{2066}';
const PDI: char = '\u{2069}';

pub enum ReplEval {
    Done { output: String },
    Failed { output: String, error: String },
    Incomplete,
}

pub struct Repl {
    interp: Interpreter,
    analyzer: SemanticAnalyzer,
    history: Vec<String>,
    // Remembered because Interpreter does not expose its file
    // policy for reading; reset() must restore the same flag.
    trusted: bool,
}

impl Repl {
    pub fn new() -> Self {
        Self {
            interp: Interpreter::new(),
            analyzer: SemanticAnalyzer::new(),
            history: Vec::new(),
            trusted: false,
        }
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        self.interp.set_trusted(trusted);
    }

    pub fn history(&self) -> &[String] {
        &self.history
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
        self.interp = Interpreter::new();
        self.interp.set_trusted(trusted);
        self.analyzer = SemanticAnalyzer::new();
        self.history.clear();
    }
}

pub fn help_text() -> &'static str {
    "أوامر REPL نيفا:\n  :مساعدة  عرض هذه المساعدة\n  :اعرض    عرض ما كتبته في هذه الجلسة (بعرض RTL معزول)\n  :مسح     إعادة بدء الجلسة (مسح كل المتغيرات والدوال)\n  :خروج    الخروج من REPL\nكود نيفا:\n  اكتب جملة في سطر؛ التعبير وحده يطبع قيمته (مثال: ١ + ٢)\n  الكتل (دالة، إذا، بينما، لكل، كرر، فئة، حاول) تكمل بسطور إضافية بمسافة بادئة\n  سطر فارغ أثناء الكتلة يلغيها\n  الملفات مقيدة بمجلد العمل الحالي (أو --allow-all للموثوقة)\n  ملاحظة RTL: شكل الحروف العربية يعتمد على الترمنال نفسه؛ :اعرض يعيد طباعة ما كتبته بعوازل اتجاه صريحة\n"
}

pub enum ReplCommand {
    Help,
    Show,
    Clear,
    Quit,
}

pub fn parse_command(line: &str) -> Option<ReplCommand> {
    match line.trim() {
        ":مساعدة" | ":help" => Some(ReplCommand::Help),
        ":اعرض" | ":show" => Some(ReplCommand::Show),
        ":مسح" | ":clear" => Some(ReplCommand::Clear),
        ":خروج" | ":quit" | ":q" | "خروج" | "exit" => Some(ReplCommand::Quit),
        _ => None,
    }
}

/// The interactive loop: read lines from stdin, evaluate, print.
/// Returns SUCCESS on :خروج or EOF.
pub fn run_repl(trusted: bool) -> ExitCode {
    let mut repl = Repl::new();
    repl.set_trusted(trusted);
    println!("مرحبا في REPL نيفا — Niva engine 1.0.0");
    println!("اكتب كود نيفا؛ الكتل مثل دالة وإذا وبينما تكمل بسطور إضافية (سطر فارغ يلغي الكتلة).");
    println!("الأوامر: :مساعدة للمساعدة، :اعرض لعرض ما كتبته، :مسح لإعادة البدء، :خروج للخروج.");
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut pending = String::new();
    loop {
        if pending.is_empty() {
            print!("نيفا> ");
        } else {
            print!(".... ");
        }
        if out.flush().is_err() {
            return ExitCode::FAILURE;
        }
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => {
                // EOF (Ctrl-D): exit cleanly.
                println!();
                return ExitCode::SUCCESS;
            }
            Ok(_) => {}
            Err(_) => return ExitCode::FAILURE,
        }
        let line = line.trim_end_matches(['\n', '\r']).to_string();
        if !pending.is_empty() {
            if line.trim().is_empty() {
                pending.clear();
                println!("ألغيت الكتلة غير المكتملة.");
                continue;
            }
            // A command line never belongs to the block: finish the
            // pending block first (or drop it if still incomplete),
            // then fall through to command handling below.
            if parse_command(&line).is_some() {
                match repl.eval(&pending) {
                    ReplEval::Done { output } => print!("{}", output),
                    ReplEval::Failed { output, error } => {
                        print!("{}", output);
                        eprintln!("{}", error);
                    }
                    ReplEval::Incomplete => {
                        println!("ألغيت الكتلة غير المكتملة.");
                    }
                }
                pending.clear();
            } else if line.starts_with([' ', '\t']) {
                // Indented line: still inside the block. Do NOT
                // evaluate yet: the lexer closes blocks at EOF, so
                // a prefix ending inside a nested block parses as
                // complete and would run too early — a nested
                // function definition would be cut off from the
                // rest of its outer body. Only a line at indent 0
                // can finish the block.
                pending.push('\n');
                pending.push_str(&line);
                continue;
            } else {
                pending.push('\n');
                pending.push_str(&line);
                match repl.eval(&pending) {
                    ReplEval::Incomplete => continue,
                    ReplEval::Done { output } => {
                        print!("{}", output);
                        pending.clear();
                    }
                    ReplEval::Failed { output, error } => {
                        print!("{}", output);
                        eprintln!("{}", error);
                        pending.clear();
                    }
                }
                continue;
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(cmd) = parse_command(&line) {
            match cmd {
                ReplCommand::Help => println!("{}", help_text()),
                ReplCommand::Show => println!("{}", repl.show_history()),
                ReplCommand::Clear => {
                    repl.reset();
                    println!("بدأت جلسة جديدة.");
                }
                ReplCommand::Quit => return ExitCode::SUCCESS,
            }
            continue;
        }
        match repl.eval(&line) {
            ReplEval::Incomplete => {
                pending.push_str(&line);
            }
            ReplEval::Done { output } => {
                print!("{}", output);
            }
            ReplEval::Failed { output, error } => {
                print!("{}", output);
                eprintln!("{}", error);
            }
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
        assert_eq!(done_out(r.eval("١ + ٢")), "3\n");
        assert_eq!(done_out(r.eval("\"مرحبا\"")), "مرحبا\n");
        assert_eq!(done_out(r.eval("[١، ٢]")), "[1, 2]\n");
    }

    #[test]
    fn repl_trailing_expression_prints_after_block() {
        let mut r = Repl::new();
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب\nجمع(٢، ٣)")),
            "5\n"
        );
        // A trailing call after a definition prints its value.
        assert_eq!(done_out(r.eval("س = ٧\nس * ٢")), "14\n");
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
        // The definition block runs silently; the two calls print 1, 2.
        assert_eq!(outs, vec!["", "1\n", "2\n"]);
    }

    #[test]
    fn repl_lambda_define_and_call() {
        // Meeting 13: a lambda is a single-line expression, so
        // the REPL needs no block continuation for it.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("مضاعف = دالة_مجهولة(س): س * ٢")), "");
        assert_eq!(done_out(r.eval("مضاعف(٥)")), "10\n");
        assert_eq!(done_out(r.eval("مضاعف")), "<دالة مجهولة>\n");
    }

    #[test]
    fn repl_variables_and_functions_persist() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        assert_eq!(done_out(r.eval("س * ٢")), "10\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "5\n");
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
        assert_eq!(done_out(r.eval("س")), "1\n");
    }

    #[test]
    fn repl_print_output_and_reset() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("اطبع ٥")), "5\n");
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
        assert!(matches!(parse_command("exit"), Some(ReplCommand::Quit)));
        assert!(parse_command("س = ٥").is_none());
    }
}
