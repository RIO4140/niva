// Niva playground UI: loads the WASM engine, runs the code in
// the textarea, and shows printed output (plus the Arabic error
// when the program fails). A WASM stack trap surfaces as a JS
// exception and is shown as a friendly message, not a blank page.
import init, { run_niva } from './pkg/niva_playground.js';

const EXAMPLES = {
  hello: `# الدرس الأول: الطباعة
اطبع "أهلا بك في نيفا"
اطبع ٢ + ٣
اطبع "النتيجة:"، ١٠ * ٢`,
  functions: `# الدوال والنطاق
دالة جمع(أ، ب):
    ارجع أ + ب
اطبع جمع(٣، ٤)
دالة اصنع_عداد():
    ع = ٠
    دالة زد():
        ع = ع + ١
        ارجع ع
    ارجع زد
أ = اصنع_عداد()
اطبع أ()
اطبع أ()
اطبع أ()`,
  stdlib: `# المكتبة القياسية
استورد رياضيات
استورد عشوائي
اطبع رياضيات.جذر(16)
اطبع رياضيات.قوة(2، 10)
اطبع رياضيات.باي
اطبع عشوائي.صحيح_عشوائي(1، 6)`,
  classes: `# الفئات
فئة نقطة:
    دالة أنشئ(س، ص):
        نفس.س = س
        نفس.ص = ص
    دالة مجموع():
        ارجع نفس.س + نفس.ص
ك = أنشئ نقطة(٣، ٤)
اطبع ك.مجموع()`,
};

const codeEl = document.getElementById('code');
const outEl = document.getElementById('output');
let ready = false;

function show(text, isError) {
  outEl.textContent = text;
  outEl.classList.toggle('error', !!isError);
}

function run() {
  if (!ready) return;
  let result;
  try {
    result = JSON.parse(run_niva(codeEl.value));
  } catch (e) {
    // A WASM trap (stack exhaustion, or an engine bug) reaches
    // here as a JS exception; do not guess its cause.
    show('حدث خطأ غير متوقع داخل المحرك. جرّب تبسيط البرنامج وحاول مجددا.\n' + e, true);
    return;
  }
  show(result.output + (result.ok ? '' : result.error), !result.ok);
}

init()
  .then(() => {
    ready = true;
    run();
  })
  .catch((e) => {
    show('تعذر تحميل المحرك: ' + e, true);
  });

document.getElementById('run').addEventListener('click', run);
codeEl.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
    e.preventDefault();
    run();
  }
});
document.querySelectorAll('[data-example]').forEach((b) => {
  b.addEventListener('click', () => {
    codeEl.value = EXAMPLES[b.dataset.example];
    run();
  });
});
