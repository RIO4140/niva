// Node verification of command history in the Niva console
// page (owner's approval 2026-11-05 ~21:24: up/down arrows,
// per session). Usage: node test-history.mjs
//
// The real page script (console/console.js) is loaded in
// Node behind a minimal DOM shim; its WASM import is
// redirected by test-history-loader.mjs to the stub that
// loads the real WASM from disk. What is proven:
// - up/down cycling, Down past newest returns empty;
// - empty and whitespace-only lines are not stored;
// - typing after navigating resets to the newest;
// - history is per session and survives switching;
// - continuation lines are stored one entry per line and
//   the > prompt still works;
// - the reversed-assignment prototype is still absent.
import { register } from 'node:module';
register('./test-history-loader.mjs', import.meta.url);
import { NivaConsole } from './pkg/niva_playground.js';

// --- Minimal DOM shim (only what console.js touches) ---
class El {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.listeners = {};
    this.style = {};
    this._classes = new Set();
    this.classList = {
      add: (c) => { this._classes.add(c); },
      remove: (c) => { this._classes.delete(c); },
      toggle: (c, force) => {
        if (force === undefined) {
          if (this._classes.has(c)) { this._classes.delete(c); return false; }
          this._classes.add(c); return true;
        }
        if (force) this._classes.add(c); else this._classes.delete(c);
        return !!force;
      },
      contains: (c) => this._classes.has(c),
    };
    this.textContent = '';
    this.value = '';
    this.disabled = false;
    this.scrollTop = 0;
    this.scrollHeight = 0;
    this._innerHTML = '';
  }
  get innerHTML() { return this._innerHTML; }
  set innerHTML(v) {
    this._innerHTML = v;
    if (v === '') this.children = [];
  }
  appendChild(child) { this.children.push(child); return child; }
  append(...kids) { for (const k of kids) this.children.push(k); }
  addEventListener(type, fn) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(fn);
  }
  focus() { document.activeElement = this; }
  blur() { if (document.activeElement === this) document.activeElement = null; }
}

const elements = {};
const bodyEl = new El('body');
globalThis.document = {
  getElementById: (id) => {
    if (!elements[id]) elements[id] = new El();
    return elements[id];
  },
  createElement: (tag) => new El(tag),
  body: bodyEl,
  activeElement: null,
};

function fire(el, type, event = {}) {
  const e = { type, preventDefault() {}, ...event };
  for (const fn of (el.listeners[type] || [])) fn(e);
}

// Load the real page script (top-level await runs init +
// creates session 1).
await import('./console/console.js');

const input = document.getElementById('input');
const promptEl = document.getElementById('prompt');

let failures = 0;
function check(label, actual, expected) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  console.log(`${ok ? 'PASS' : 'FAIL'} ${label}: ${JSON.stringify(actual)}`);
  if (!ok) {
    console.log(`      expected: ${JSON.stringify(expected)}`);
    failures++;
  }
}

// Drive the input exactly like a user: set the value, fire
// the key. Setting value from code fires no 'input' event,
// matching the browser.
function type(line) {
  input.value = line;
  fire(input, 'keydown', { key: 'Enter' });
}
function typeText(text) {
  input.value = text;
  fire(input, 'input', {});
}
function up() { fire(input, 'keydown', { key: 'ArrowUp' }); return input.value; }
function down() { fire(input, 'keydown', { key: 'ArrowDown' }); return input.value; }
function addSession() { fire(document.getElementById('add-session'), 'click', {}); }
function switchTo(index) {
  const rows = document.getElementById('session-list').children;
  fire(rows[index], 'click', {});
}
function scrollText() {
  const wrap = document.getElementById('scrollwrap');
  const visible = wrap.children.find((el) => el.style.display === 'block');
  if (!visible) return '';
  return visible.children.map((c) => c.textContent).join('\n');
}

check('session 1 prompt fresh', promptEl.textContent, '~ $ ');
check('up with empty history does nothing', up(), '');

type('س = ٥');
type('اطبع س');
type('ص = ٧');
check('up newest', up(), 'ص = ٧');
check('up middle', up(), 'اطبع س');
check('up oldest', up(), 'س = ٥');
check('up stays at oldest', up(), 'س = ٥');
check('down middle', down(), 'اطبع س');
check('down newest', down(), 'ص = ٧');
check('down past newest is empty', down(), '');
check('down at fresh stays empty', down(), '');
check('up after fresh returns newest', up(), 'ص = ٧');

// Empty and whitespace-only lines are not stored.
type('');
check('up after empty submit still newest', up(), 'ص = ٧');
type('   ');
check('up after whitespace submit still newest', up(), 'ص = ٧');

// Typing after navigating resets to the newest.
up(); // newest: ص = ٧
up(); // middle: اطبع س
typeText('ا');
check('up after typing restarts at newest', up(), 'ص = ٧');

// History is per session and survives switching.
addSession(); // session 2, switched to it
check('session 2 prompt fresh', promptEl.textContent, '~ $ ');
check('session 2 up with empty history', up(), '');
type('أ = ١');
check('session 2 up', up(), 'أ = ١');
switchTo(0); // back to session 1
check('session 1 history survives switch', up(), 'ص = ٧');
switchTo(1); // session 2 again
check('session 2 history survives switch', up(), 'أ = ١');

// Continuation lines are stored one entry per line, and
// the > prompt still works.
type('إذا أ أكبر من ٠:');
check('continuation prompt', promptEl.textContent, '> ');
type('    اطبع "كبير"');
check('still continuation', promptEl.textContent, '> ');
type('ب = ٢');
check('block evaluated, prompt back', promptEl.textContent, '~ $ ');
check('block output reached scrollback', scrollText().includes('كبير'), true);
check('up is the last line', up(), 'ب = ٢');
check('up is the indented line', up(), '    اطبع "كبير"');
check('up is the if line', up(), 'إذا أ أكبر من ٠:');
check('up is the older session line', up(), 'أ = ١');
check('up stays at oldest', up(), 'أ = ١');

// The input draft is per session too: a half-typed line
// stays with its session across switches.
typeText('س = ');
switchTo(0);
check('session 1 draft restored', input.value, 'ص = ٧');
switchTo(1);
check('session 2 draft restored', input.value, 'س = ');

// Prototype still absent in the public WASM build.
const c = new NivaConsole();
check('٥ = س rejected', JSON.parse(c.eval('٥ = س')).output.includes('هدف التعيين غير صالح'), true);

console.log(failures === 0 ? '=== HISTORY: ALL GREEN ===' : `=== HISTORY: ${failures} FAILURES ===`);
process.exit(failures === 0 ? 0 : 1);
