// Node verification of the Niva WASM playground (same V8 engine
// the Chromium browser uses). Loads pkg/niva_playground.js and
// runs the same 7 tests the browser task was given, plus a depth
// binary search. Usage: node test-wasm.mjs
import init, { run_niva } from './pkg/niva_playground.js';
import fs from 'node:fs';

await init(fs.readFileSync('./pkg/niva_playground_bg.wasm'));

function run(label, src) {
  try {
    const r = JSON.parse(run_niva(src));
    console.log(`--- ${label} ---\nok=${r.ok}\noutput=${JSON.stringify(r.output)}\nerror=${JSON.stringify(r.error)}`);
    return r;
  } catch (e) {
    console.log(`--- ${label} ---\nTRAP: ${e.constructor.name}: ${e.message}`);
    return null;
  }
}

run('T1 hello', `# الدرس الأول: الطباعة
اطبع "أهلا بك في نيفا"
اطبع ٢ + ٣
اطبع "النتيجة:"، ١٠ * ٢`);

run('T2 stdlib', `استورد رياضيات
استورد عشوائي
اطبع رياضيات.جذر(16)
اطبع رياضيات.قوة(2، 10)
اطبع رياضيات.باي`);

run('T3 undefined var', `اطبع س`);

run('T4 read file', `اطبع اقرأ_ملف("ملف.txt")`);

run('T5 sleep', `استورد وقت
وقت.نوم(1)
اطبع "بعد النوم"`);

run('T6 input', `اطبع ادخل()`);

run('T7a closures', `دالة اصنع_عداد():
    ع = ٠
    دالة زد():
        ع = ع + ١
        ارجع ع
    ارجع زد
أ = اصنع_عداد()
اطبع أ()
اطبع أ()
اطبع أ()`);

// Depth measurement: the Meeting-15 extreme shape (10 locals +
// a wide expression), max executing depth = N+1.
function depthProg(n) {
  return `دالة عد(ن):
    أ = ن + ١
    ب = أ * ٢
    ج = ب - ٣
    د = ج + أ
    ه = د * ب
    ز = ه - ج
    ح = ز + د
    ط = ح * ه
    ي = ط - ز
    ك = أ + ب + ج + د + ه + ز + ح + ط + ي + ن
    إذا ن <= ٠:
        ارجع ك
    ارجع عد(ن - ١)
اطبع عد(${n})`;
}
let lo = 0; // largest N known to complete
let hi = null; // smallest N known to trap (null = not found yet)
for (const n of [100, 200, 300, 400, 500, 700, 1000]) {
  const r = run(`depth ${n}`, depthProg(n));
  if (r && r.ok) lo = n;
  else { hi = n; break; }
}
if (hi !== null) {
  while (hi - lo > 50) {
    const mid = lo + Math.floor((hi - lo) / 2);
    const r = run(`depth ${mid}`, depthProg(mid));
    if (r && r.ok) lo = mid;
    else hi = mid;
  }
}
console.log(`=== DEPTH: largest completing N = ${lo}, smallest failing N = ${hi} (max executing depth = N+1) ===`);
