// Node verification of Termux-style sessions in the Niva
// console (owner's order 2026-11-05 ~21:03, overruling
// Meeting 23 on the multiplexer core). Usage:
// node test-sessions.mjs
//
// What is proven here, at the WASM level (the page itself
// renders these same records into per-session scroll divs):
// - two consoles are independent (variables, network flag);
// - switching restores a session's scrollback exactly;
// - a new session starts fresh with the banner;
// - the reversed-assignment prototype is still absent.
//
// The network checks need the local test server + the sync
// XHR polyfill, copied from test-network.mjs (same reason:
// curl via spawnSync blocks the event loop, so the server
// must run as a separate child process).
import fs from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';

// --- Local test HTTP server (separate child process) ---
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
const { default: init, NivaConsole } = await import('./pkg/niva_playground.js');
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

// A session record, mirroring the console page exactly: its
// own NivaConsole, its own scrollback lines, its own prompt
// and its own network decision.
class Session {
  constructor() {
    this.niva = new NivaConsole();
    this.lines = [];
    this.netChoice = null;
    this.prompt = '~ $ ';
    for (const line of NivaConsole.banner().split('\n')) this.lines.push(line);
  }
  feed(line) {
    this.lines.push(`~ $ ${line}`);
    const r = JSON.parse(this.niva.eval(line));
    if (r.output) for (const l of r.output.replace(/\n$/, '').split('\n')) this.lines.push(l);
    this.prompt = r.prompt;
    return r;
  }
}

// A new session starts fresh with the minimal banner.
check('banner', NivaConsole.banner(), 'محرك نيفا ١٫٠٫٠');
const s1 = new Session();
check('fresh session lines = banner only', s1.lines, ['محرك نيفا ١٫٠٫٠']);
check('fresh session prompt', s1.prompt, '~ $ ');
check('fresh session network off', s1.niva.is_network_allowed(), false);

// Two consoles are independent: a variable defined in
// session 1 does not exist in session 2.
s1.feed('س = ٥');
s1.feed('اطبع س');
const s2 = new Session();
check('session 2 starts fresh', s2.lines, ['محرك نيفا ١٫٠٫٠']);
const s2out = s2.feed('اطبع س');
check('session 2 does not see session 1 variable', s2out.output.includes('غير معرف'), true);
check('session 1 scrollback still has its own output', s1.lines.includes('٥'), true);
check('session 2 scrollback has no ٥ result', s2.lines.includes('٥'), false);

// Switching restores scrollback: back in session 1, the old
// output is still there, and new output appends after it.
s1.feed('اطبع س + ١');
check('session 1 scrollback restored + appended', s1.lines, [
  'محرك نيفا ١٫٠٫٠',
  '~ $ س = ٥',
  '~ $ اطبع س',
  '٥',
  '~ $ اطبع س + ١',
  '٦',
]);
check('session 2 scrollback untouched by session 1', s2.lines[0], 'محرك نيفا ١٫٠٫٠');

// Network decision is per-session: session 1 allows and
// fetches; session 2 is still gated at its own first اجلب.
s1.feed('استورد شبكة');
s2.feed('استورد شبكة');
s1.niva.set_network_allowed(true);
s1.netChoice = true;
check('session 1 network on', s1.niva.is_network_allowed(), true);
check('session 1 fetch works', s1.feed(`شبكة.اجلب("${base}/hello")`).output, 'مرحبا من الخادم\n');
check('session 2 network still off', s2.niva.is_network_allowed(), false);
const gated = s2.feed(`شبكة.اجلب("${base}/hello")`);
check('session 2 first اجلب still gated', gated.output.includes('الشبكة مغلقة افتراضيا'), true);
check('session 2 gated needs_network', gated.needs_network, true);
// Session 2 allows too; switching back to session 1 keeps
// its approval without asking again.
s2.niva.set_network_allowed(true);
s2.netChoice = true;
check('session 2 fetch works after allow', s2.feed(`شبكة.اجلب("${base}/hello")`).output, 'مرحبا من الخادم\n');
check('session 1 still allowed after switch back', s1.feed(`شبكة.اجلب("${base}/hello")`).output, 'مرحبا من الخادم\n');

// Prototype still absent in the public WASM build.
const rev = s1.feed('٥ = س');
check('٥ = س rejected', rev.output.includes('هدف التعيين غير صالح'), true);

serverProc.kill();
console.log(failures === 0 ? '=== SESSIONS: ALL GREEN ===' : `=== SESSIONS: ${failures} FAILURES ===`);
process.exit(failures === 0 ? 0 : 1);
