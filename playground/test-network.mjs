// Node verification of the Niva WASM network path (Meeting 22).
// Usage: node test-network.mjs
//
// The WASM engine fetches through a synchronous XMLHttpRequest
// (web-sys). Node has no XMLHttpRequest, so this test installs
// a minimal synchronous polyfill backed by `curl` (spawnSync)
// BEFORE importing the WASM module. The polyfill is test-only:
// it mimics the browser's redirect-following (curl -L) and
// exposes status / responseText / responseURL, so the engine's
// own checks (final-host, HTTPS downgrade, 5 MB cap) are what
// is actually exercised. The local test server sends
// Access-Control-Allow-Origin: * — a real browser would need
// that CORS header too (documented in NETWORK.md).
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';

// --- Local test HTTP server (separate child process) ---
// See test-server.mjs for why: the XHR polyfill below calls
// curl via spawnSync, which blocks this process's event loop,
// so an in-process server could never answer a fetch.
import { spawn } from 'node:child_process';
const serverProc = spawn(process.execPath, ['test-server.mjs'], { stdio: ['ignore', 'pipe', 'inherit'] });
const port = await new Promise((resolve, reject) => {
  let buf = '';
  serverProc.stdout.on('data', (d) => {
    buf += d.toString();
    const m = buf.match(/READY (\d+)/);
    if (m) resolve(parseInt(m[1], 10));
  });
  serverProc.on('exit', (c) => reject(new Error(`test server exited (${c})`)));
});
const base = `http://127.0.0.1:${port}`;

// --- Synchronous XMLHttpRequest polyfill (test-only, curl) ---
class NodeXHR {
  constructor() {
    this._timeout = 0;
    this._status = 0;
    this._text = '';
    this._url = '';
  }
  set timeout(ms) { this._timeout = ms; }
  get timeout() { return this._timeout; }
  open(method, url, async) {
    if (async !== false) throw new Error('polyfill is sync-only');
    this._method = method;
    this._url = url;
  }
  send() {
    const maxTime = Math.max(1, Math.ceil(this._timeout / 1000));
    const r = spawnSync('curl', [
      '-s', '-L', '--max-time', String(maxTime),
      '-w', '\n__NIVA_TRAILER__ %{http_code} %{url_effective}',
      this._url,
    ], { encoding: 'utf-8', maxBuffer: 64 * 1024 * 1024 });
    if (r.status !== 0) throw new Error(`curl failed (${r.status})`);
    const out = r.stdout || '';
    const idx = out.lastIndexOf('\n__NIVA_TRAILER__ ');
    if (idx < 0) throw new Error('curl trailer missing');
    this._text = out.slice(0, idx);
    const [code, url] = out.slice(idx + '\n__NIVA_TRAILER__ '.length).trim().split(' ');
    this._status = parseInt(code, 10);
    this._url = url;
  }
  get status() { return this._status; }
  get responseText() { return this._text; }
  get responseURL() { return this._url; }
}
globalThis.XMLHttpRequest = NodeXHR;

// --- WASM ---
const { default: init, run_niva, NivaConsole } = await import('./pkg/niva_playground.js');
await init(fs.readFileSync('./pkg/niva_playground_bg.wasm'));

let failures = 0;
function check(label, actual, expected) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  console.log(`${ok ? 'PASS' : 'FAIL'} ${label}: ${JSON.stringify(actual)}`);
  if (!ok) {
    console.log(`      expected: ${JSON.stringify(expected)}`);
    failures++;
  }
}

// run_niva (one-shot): network is OFF, no flag exists there.
const r0 = JSON.parse(run_niva('استورد شبكة\nاطبع شبكة.اجلب("http://127.0.0.1:9/")'));
check('run_niva gate off', [r0.ok, r0.error.includes('الشبكة مغلقة افتراضيا')], [false, true]);

const c = new NivaConsole();
function feed(line) {
  return JSON.parse(c.eval(line));
}
check('isNetworkAllowed default false', c.is_network_allowed(), false);
check('استورد شبكة', feed('استورد شبكة'), { output: '', prompt: '~ $ ', ended: false, needs_network: false });

// Gate: permission error + needs_network flag.
const gated = feed(`شبكة.اجلب("${base}/hello")`);
check('gate error text', gated.output.includes('الشبكة مغلقة افتراضيا'), true);
check('gate needs_network flag', gated.needs_network, true);
check('gate prompt back to normal', gated.prompt, '~ $ ');

// Approve (what the console page does after the Arabic ask).
c.set_network_allowed(true);
check('isNetworkAllowed after set', c.is_network_allowed(), true);

// Real fetch through the polyfilled sync XHR.
check('fetch hello', feed(`شبكة.اجلب("${base}/hello")`), { output: 'مرحبا من الخادم\n', prompt: '~ $ ', ended: false, needs_network: false });
check('fetch chunked', feed(`شبكة.اجلب("${base}/chunked")`), { output: 'chunked-ok\n', prompt: '~ $ ', ended: false, needs_network: false });
check('fetch same-host redirect', feed(`شبكة.اجلب("${base}/redirect")`), { output: 'مرحبا من الخادم\n', prompt: '~ $ ', ended: false, needs_network: false });
check('fetch cross-host redirect refused', feed(`شبكة.اجلب("${base}/cross")`).output.includes('مضيف مختلف'), true);
check('fetch 500 refused', feed(`شبكة.اجلب("${base}/status500")`).output.includes('500'), true);
check('fetch too big refused', feed(`شبكة.اجلب("${base}/big")`).output.includes('5 ميغابايت'), true);
// HTTPS in WASM goes to the browser fetch (curl here) — it
// is NOT refused like on native. Whether the sandbox egress
// proxy reaches example.com or not, the result must be a clean
// Arabic outcome (the page, or a fetch error) — never a trap.
const https = feed('شبكة.اجلب("https://example.com/")');
check('fetch https clean Arabic outcome (no trap)', https.output.includes('Example Domain') || https.output.includes('تعذر الوصول'), true);

// Prototype still absent in the public WASM build.
const rev = feed('٥ = س');
check('٥ = س rejected', rev.output.includes('هدف التعيين غير صالح'), true);

serverProc.kill();
console.log(failures === 0 ? '=== NETWORK (WASM): ALL GREEN ===' : `=== NETWORK (WASM): ${failures} FAILURES ===`);
process.exit(failures === 0 ? 0 : 1);
