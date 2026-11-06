// Node LSP driver: talks to niva-lsp over stdio (Content-Length
// framing) and checks diagnostics, hover, symbols, completion.
// Replaces the old Python driver (test-lsp.py) — Niva has zero
// Python dependence anywhere, including tests.
// Usage: node test-lsp.mjs
import { spawn } from 'node:child_process';

const BIN = 'target/release/niva-lsp';
const URI = 'file:///tmp/test.نيفا';

const proc = spawn(BIN, [], { stdio: ['pipe', 'pipe', 'inherit'] });
let buf = Buffer.alloc(0);
let nextId = 1;
let failures = 0;
const pending = new Map();
const notifications = [];

function check(label, actual, expected) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  console.log(`${ok ? 'PASS' : 'FAIL'} ${label}: ${JSON.stringify(actual)}`);
  if (!ok) {
    console.log(`      expected: ${JSON.stringify(expected)}`);
    failures++;
  }
}

proc.stdout.on('data', (d) => {
  buf = Buffer.concat([buf, d]);
  for (;;) {
    const headEnd = buf.indexOf('\r\n\r\n');
    if (headEnd < 0) break;
    const head = buf.slice(0, headEnd).toString('utf8');
    const m = head.match(/Content-Length: (\d+)/);
    if (!m) break;
    const len = parseInt(m[1], 10);
    if (buf.length < headEnd + 4 + len) break;
    const body = buf.slice(headEnd + 4, headEnd + 4 + len).toString('utf8');
    buf = buf.slice(headEnd + 4 + len);
    const msg = JSON.parse(body);
    if (msg.id !== undefined && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.method) {
      notifications.push(msg);
    }
  }
});

function send(obj) {
  const body = Buffer.from(JSON.stringify(obj), 'utf8');
  proc.stdin.write(Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, 'utf8'),
    body,
  ]));
}

function request(method, params) {
  const id = nextId++;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    send({ jsonrpc: '2.0', id, method, params });
  });
}

function notify(method, params) {
  send({ jsonrpc: '2.0', method, params });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function diagnosticsAfterOpen(text) {
  notifications.length = 0;
  notify('textDocument/didOpen', {
    textDocument: { uri: URI, languageId: 'niva', version: 1, text },
  });
  await sleep(300);
  const pub = notifications.find((n) => n.method === 'textDocument/publishDiagnostics');
  return pub ? pub.params.diagnostics : null;
}

// --- initialize ---
const init = await request('initialize', {
  processId: null,
  rootUri: null,
  capabilities: {},
});
check('initialize ok', !!init.result, true);
check('hover capability', !!init.result.capabilities.hoverProvider, true);
check('symbols capability', !!init.result.capabilities.documentSymbolProvider, true);
check('completion capability', !!init.result.capabilities.completionProvider, true);
notify('initialized', {});

// --- diagnostics: undefined variable (semantic analyzer's exact path) ---
const d1 = await diagnosticsAfterOpen('اطبع س\n');
check('undefined var: 1 diagnostic', d1 ? d1.length : null, 1);
check(
  'diagnostic message is Arabic',
  d1 && d1[0].message.includes('غير معرف'),
  true,
);
check('diagnostic source', d1 && d1[0].source, 'نيفا');

// --- diagnostics: valid program (compiler-era features) ---
const valid = [
  'دالة اصنع_عداد():',
  '    ع = ٠',
  '    دالة زد():',
  '        ع = ع + ١',
  '        ارجع ع',
  '    ارجع زد',
  'أ = اصنع_عداد()',
  'اطبع أ()',
  'اطبع أ()',
  'استورد رياضيات',
  'اطبع رياضيات.جذر(16)',
  'اطبع رياضيات.ه',
  'س = ٥',
  'اطبع س',
].join('\n');
const d2 = await diagnosticsAfterOpen(valid);
check('valid program: 0 diagnostics', d2 ? d2.length : null, 0);

// --- diagnostics: plain صدر keyword (no diacritics) ---
const d3 = await diagnosticsAfterOpen('استورد رياضيات\nاطبع رياضيات.جذر(16)\n');
check('صدر-free program: 0 diagnostics', d3 ? d3.length : null, 0);

// --- hover: keyword, new file builtin, شبكة ---
await diagnosticsAfterOpen('اطبع اعرض_الملفات(".")\nاستورد شبكة\n');
const hover1 = await request('textDocument/hover', {
  textDocument: { uri: URI },
  position: { line: 0, character: 1 },
});
check('hover اطبع', hover1.result && hover1.result.contents.value.includes('يطبع'), true);
const hover2 = await request('textDocument/hover', {
  textDocument: { uri: URI },
  position: { line: 0, character: 8 },
});
check(
  'hover اعرض_الملفات (new builtin)',
  hover2.result && hover2.result.contents.value.includes('اعرض_الملفات'),
  true,
);
const hover3 = await request('textDocument/hover', {
  textDocument: { uri: URI },
  position: { line: 1, character: 9 },
});
check(
  'hover شبكة (new module)',
  hover3.result && hover3.result.contents.value.includes('اجلب'),
  true,
);

// --- symbols: function + class ---
await diagnosticsAfterOpen('دالة جمع(أ، ب):\n    ارجع أ + ب\nفئة نقطة:\n    دالة أنشئ(س، ص):\n        نفس.س = س\n');
const syms = await request('textDocument/documentSymbol', {
  textDocument: { uri: URI },
});
const names = (syms.result || []).map((s) => s.name);
check('symbols include جمع', names.includes('جمع'), true);
check('symbols include نقطة', names.includes('نقطة'), true);

// --- completion: includes شبكة + new builtins + plain صدر ---
const comp = await request('textDocument/completion', {
  textDocument: { uri: URI },
  position: { line: 0, character: 0 },
});
const labels = (comp.result || []).map((c) => c.label);
check('completion has شبكة', labels.includes('شبكة'), true);
check('completion has اعرض_الملفات', labels.includes('اعرض_الملفات'), true);
check('completion has صدر (plain)', labels.includes('صدر'), true);
check('completion has no diacritic forms', labels.includes('صدّر') || labels.includes('هـ'), false);

await request('shutdown', null);
notify('exit', null);
await sleep(200);
proc.kill();

console.log(failures === 0 ? '=== LSP: ALL GREEN ===' : `=== LSP: ${failures} FAILURES ===`);
process.exit(failures === 0 ? 0 : 1);
