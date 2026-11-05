//! WASM entry for the Niva playground (Meeting 17, step 3).
//!
//! One exported function: `run_niva(source)` returns a JSON
//! string: `{"ok":true,"output":"...","error":""}` on success,
//! or `{"ok":false,"output":"<partial>","error":"<Arabic error>"}`
//! on failure. Panics are caught and reported as a clean JSON
//! error — an engine bug never shows as a blank crash, and a
//! WASM stack trap surfaces as a JS exception the page catches.
//!
//! The entry never spawns a thread (single-threaded WASM) and
//! never touches the filesystem: file/input builtins are inert
//! on wasm32 (see the engine), and modules load from memory
//! only (native stdlib) or fail as "module not found".

use niva_engine::repl::{banner as repl_banner, Repl};
use niva_engine::run_source;
use serde::Serialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct RunResult {
    ok: bool,
    output: String,
    error: String,
}

#[derive(Serialize)]
struct ConsoleResult {
    output: String,
    prompt: String,
    ended: bool,
    /// True when this output is (or contains) the network
    /// permission error — the console page uses it to offer
    /// the first-use permission prompt (Meeting 22 §3.3).
    needs_network: bool,
}

fn run_inner(source: &str) -> RunResult {
    let (output, res) = run_source(source);
    match res {
        Ok(()) => RunResult {
            ok: true,
            output,
            error: String::new(),
        },
        Err(e) => RunResult {
            ok: false,
            output,
            error: e.to_string(),
        },
    }
}

#[wasm_bindgen]
pub fn run_niva(source: &str) -> String {
    let result =
        catch_unwind(AssertUnwindSafe(|| run_inner(source))).unwrap_or_else(|_| RunResult {
            ok: false,
            output: String::new(),
            error: "خطأ داخلي غير متوقع في المحرك".to_string(),
        });
    serde_json::to_string(&result)
        .unwrap_or_else(|_| "{\"ok\":false,\"output\":\"\",\"error\":\"خطأ داخلي\"}".to_string())
}

/// Stateful REPL console for the Android app (and the console
/// page): one `NivaConsole` holds one `Repl`, and `eval` feeds
/// it a single line, returning a JSON string:
/// `{"output":"...","prompt":"~ $ " | "> ","ended":false}`.
/// This is the PUBLIC language build: the reversed-assignment
/// prototype is not in it (the engine is built without the
/// `prototype-rtl` feature), so `٥ = س` is rejected with the
/// normal parse error, exactly like the public repo.
#[wasm_bindgen]
pub struct NivaConsole {
    repl: Repl,
}

#[wasm_bindgen]
impl NivaConsole {
    #[wasm_bindgen(constructor)]
    pub fn new() -> NivaConsole {
        NivaConsole { repl: Repl::new() }
    }

    /// The greeting shown when a console session starts.
    pub fn banner() -> String {
        repl_banner().to_string()
    }

    pub fn eval(&mut self, line: &str) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let out = self.repl.feed(line);
            ConsoleResult {
                output: out.output,
                prompt: out.prompt.to_string(),
                ended: out.ended,
                needs_network: out.needs_network,
            }
        }))
        .unwrap_or_else(|_| ConsoleResult {
            output: "خطأ داخلي غير متوقع في المحرك\n".to_string(),
            prompt: "~ $ ".to_string(),
            ended: false,
            needs_network: false,
        });
        serde_json::to_string(&result).unwrap_or_else(|_| {
            "{\"output\":\"خطأ داخلي\",\"prompt\":\"~ $ \",\"ended\":false,\"needs_network\":false}".to_string()
        })
    }

    /// Meeting 22: this console's own network permission.
    /// Per-instance (sessions, owner's order 2026-11-05
    /// ~21:03): the page calls this on the session the user
    /// approved, after the Arabic first-use ask. The choice
    /// lasts for that session only — a fresh console starts
    /// OFF, and one session's approval never leaks into
    /// another. Network stays OFF by default.
    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.repl.set_network_allowed(allowed);
    }

    /// This console's own network permission, for the UI state.
    pub fn is_network_allowed(&self) -> bool {
        self.repl.network_allowed()
    }
}
