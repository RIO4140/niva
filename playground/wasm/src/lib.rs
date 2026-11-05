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
