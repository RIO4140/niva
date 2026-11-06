// Node verification of the Niva WASM console (NivaConsole).
// Public-language build: the reversed-assignment prototype is
// NOT in it — feeding `٥ = س` must produce the normal parse
// error, not an assignment. Usage: node test-console.mjs
import init, { run_niva, NivaConsole } from './pkg/niva_playground.js';
import fs from 'node:fs';

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

// run_niva (the playground export) still works.
const r1 = JSON.parse(run_niva('اطبع ٢ + ٣'));
check('run_niva still works', [r1.ok, r1.output], [true, '٥\n']);

// The banner comes from the engine.
check('banner', NivaConsole.banner(), 'محرك نيفا ١٫٠٫٠');

// The public-language session (normal form), line by line.
const c = new NivaConsole();
function feed(line) {
  return JSON.parse(c.eval(line));
}
check('س = ٥', feed('س = ٥'), { output: '', prompt: '~ $ ', ended: false, needs_network: false });
check('ص = ٣', feed('ص = ٣'), { output: '', prompt: '~ $ ', ended: false, needs_network: false });
check('اطبع س + ص', feed('اطبع س + ص'), { output: '٨\n', prompt: '~ $ ', ended: false, needs_network: false });
check('إذا line -> continuation', feed('إذا س أكبر من ص:'), { output: '', prompt: '> ', ended: false, needs_network: false });
check('indented line -> still waiting', feed('    اطبع "س أكبر"'), { output: '', prompt: '> ', ended: false, needs_network: false });
check('ع = ٧ evaluates block', feed('ع = ٧'), { output: 'س أكبر\n', prompt: '~ $ ', ended: false, needs_network: false });
check('اطبع ع * ٢', feed('اطبع ع * ٢'), { output: '١٤\n', prompt: '~ $ ', ended: false, needs_network: false });

// PROTOTYPE ABSENT: the reversed form must be rejected with
// the normal parse error, not performed as an assignment.
const rev = feed('٥ = س');
check('٥ = س rejected', rev.output.includes('هدف التعيين غير صالح'), true);
check('٥ = س prompt back to normal', rev.prompt, '~ $ ');
check('٥ = س is not a network ask', rev.needs_network, false);
check('٥ = س not ended', rev.ended, false);
// And س is still ٥ afterwards (no assignment happened).
check('س unchanged after rejection', feed('س'), { output: '٥\n', prompt: '~ $ ', ended: false, needs_network: false });

// Word operators and Arabic digits (public) still work.
check('word operator', feed('اطبع ٣ أكبر من ٢'), { output: 'صحيح\n', prompt: '~ $ ', ended: false, needs_network: false });

// :خروج ends the session; a fresh console starts clean.
check(':خروج ends', feed(':خروج'), { output: '', prompt: '~ $ ', ended: true, needs_network: false });
const c2 = new NivaConsole();
check('fresh console is empty', JSON.parse(c2.eval('س')).output.includes('غير معرف'), true);

console.log(failures === 0 ? '=== CONSOLE: ALL GREEN ===' : `=== CONSOLE: ${failures} FAILURES ===`);
process.exit(failures === 0 ? 0 : 1);
