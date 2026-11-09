// Niva console page: a terminal-styled REPL over the WASM
// engine (NivaConsole). Offline: no webfonts, no CDN — the
// WASM module loads from ../pkg/ with relative paths.
//
// Behavior mirrors the native `نيفا --تفاعلي` exactly, because
// both run on the same stateful Repl::feed: one line at a
// time, block continuation with the > prompt, :خروج ends
// the session (tap the restart bar for a fresh console).
//
// Sessions (owner's order 2026-11-05 ~21:03, terminal-style):
// numbered sessions in a drawer, tap to switch, + to add.
// Each session is its own NivaConsole with its own scrollback,
// prompt and network decision — switching never leaks state
// between sessions. Local only: no panes, no remote, no
// detach. No renaming, no closing in v1: only add + switch.
//
// Command history (owner's approval 2026-11-05 ~21:24):
// up/down arrows on the existing input, per session, in
// memory only. No new visible UI at all — just the arrow
// keys. Up = older line, Down = newer line, Down past the
// newest returns to a fresh empty input. Every submitted
// non-empty line is stored as typed, one entry per line
// (continuation lines included). Typing anything resets the
// navigation to the newest. The input line itself is also
// per session: a half-typed draft stays with its session
// when switching away and is restored on switching back.
//
// No demo, no tutorial, no docs links: this is a terminal
// only, like terminal/Linux (owner's no-explanation rule).
import init, { NivaConsole } from '../pkg/niva_playground.js';

const scrollwrap = document.getElementById('scrollwrap');
const input = document.getElementById('input');
const promptEl = document.getElementById('prompt');
const restartEl = document.getElementById('restart');
const netbarEl = document.getElementById('netbar');
const netstateEl = document.getElementById('netstate');
const drawerEl = document.getElementById('drawer');
const backdropEl = document.getElementById('backdrop');
const sessionListEl = document.getElementById('session-list');

// Arabic-Indic digits for session labels (جلسة ١، جلسة ٢…).
const AR_DIGITS = '٠١٢٣٤٥٦٧٨٩';
function arDigits(n) {
  return String(n).replace(/\d/g, (d) => AR_DIGITS[+d]);
}

let sessions = [];
let active = null;
let nextNum = 1;

function setNetState() {
  netstateEl.textContent =
    active.netChoice === true ? 'الشبكة: مسموحة' :
    active.netChoice === false ? 'الشبكة: مرفوضة' : 'الشبكة: مغلقة';
}

function addLine(text, cls, session) {
  const target = session || active;
  if (!target) return;
  const div = document.createElement('div');
  if (cls) div.className = cls;
  div.textContent = text;
  target.scrollEl.appendChild(div);
  if (target === active) target.scrollEl.scrollTop = target.scrollEl.scrollHeight;
}

function setPrompt(prompt) {
  active.prompt = prompt;
  promptEl.textContent = prompt;
  promptEl.classList.toggle('cont', prompt.trim() === '>');
}

function showBanner(session) {
  for (const line of NivaConsole.banner().split('\n')) {
    addLine(line, 'banner', session);
  }
}

function createSession() {
  const scrollEl = document.createElement('div');
  scrollEl.className = 'scroll';
  scrollwrap.appendChild(scrollEl);
  const session = {
    num: nextNum++,
    niva: new NivaConsole(),
    netChoice: null,
    pendingNetLine: null,
    ended: false,
    prompt: '~ $ ',
    scrollEl,
    history: [],
    histIdx: null,
    draft: '',
  };
  sessions.push(session);
  showBanner(session);
  renderSessionList();
  return session;
}

function renderSessionList() {
  sessionListEl.innerHTML = '';
  for (const s of sessions) {
    const row = document.createElement('div');
    row.className = 'session-row' + (s === active ? ' active' : '');
    row.textContent = `جلسة ${arDigits(s.num)}`;
    row.addEventListener('click', () => {
      switchSession(s);
      closeDrawer();
    });
    sessionListEl.appendChild(row);
  }
}

function switchSession(session) {
  // The input line belongs to the session being left: keep
  // its draft so switching back restores exactly what the
  // user had typed (a fresh session's draft is empty).
  if (active) active.draft = input.value;
  active = session;
  // Fresh navigation position for this session; its own
  // history list is untouched, so Up starts at its newest.
  session.histIdx = null;
  input.value = session.draft;
  for (const s of sessions) {
    s.scrollEl.style.display = s === session ? 'block' : 'none';
  }
  promptEl.textContent = session.prompt;
  promptEl.classList.toggle('cont', session.prompt.trim() === '>');
  setNetState();
  // The network ask belongs to this session only: show it
  // again if this session never answered it.
  netbarEl.style.display =
    session.pendingNetLine !== null && session.netChoice === null ? 'block' : 'none';
  restartEl.style.display = session.ended ? 'block' : 'none';
  input.disabled = session.ended;
  renderSessionList();
  session.scrollEl.scrollTop = session.scrollEl.scrollHeight;
  input.focus();
}

function restartActiveSession() {
  // Only the active session restarts; the others keep their
  // state untouched.
  active.niva = new NivaConsole();
  active.netChoice = null;
  active.pendingNetLine = null;
  active.ended = false;
  active.history = [];
  active.histIdx = null;
  active.draft = '';
  input.value = '';
  active.prompt = '~ $ ';
  active.scrollEl.innerHTML = '';
  netbarEl.style.display = 'none';
  restartEl.style.display = 'none';
  input.disabled = false;
  setNetState();
  showBanner(active);
  setPrompt('~ $ ');
  input.focus();
}

function openDrawer() {
  renderSessionList();
  drawerEl.classList.add('open');
  backdropEl.style.display = 'block';
}

function closeDrawer() {
  drawerEl.classList.remove('open');
  backdropEl.style.display = 'none';
}

function submitLine(line) {
  if (!active || active.ended) return;
  // Echo the typed line with the prompt that was showing.
  const wasCont = promptEl.textContent.trim() === '>';
  const echo = document.createElement('div');
  const p = document.createElement('span');
  p.className = wasCont ? 'cont' : 'prompt';
  p.textContent = promptEl.textContent;
  const t = document.createElement('span');
  t.textContent = line;
  echo.append(p, t);
  active.scrollEl.appendChild(echo);
  let result;
  try {
    result = JSON.parse(active.niva.eval(line));
  } catch (e) {
    addLine('حدث خطأ غير متوقع داخل المحرك', 'err');
    active.scrollEl.scrollTop = active.scrollEl.scrollHeight;
    return;
  }
  if (result.output) {
    // Output may be multi-line; keep each line as its own div
    // so RTL rendering stays clean.
    for (const outLine of result.output.replace(/\n$/, '').split('\n')) {
      addLine(outLine);
    }
  }
  setPrompt(result.prompt);
  // First use of اجلب without permission: offer the Arabic
  // permission prompt (once per session). On approval the
  // line is re-run with the network open; on denial the
  // error stands and the bar never asks again this session.
  if (result.needs_network && active.netChoice === null) {
    active.pendingNetLine = line;
    netbarEl.style.display = 'block';
  }
  if (result.ended) {
    active.ended = true;
    input.disabled = true;
    restartEl.style.display = 'block';
  }
  active.scrollEl.scrollTop = active.scrollEl.scrollHeight;
}

document.getElementById('net-allow').addEventListener('click', () => {
  if (!active) return;
  active.niva.set_network_allowed(true);
  active.netChoice = true;
  const line = active.pendingNetLine;
  active.pendingNetLine = null;
  netbarEl.style.display = 'none';
  setNetState();
  addLine('تم السماح بالشبكة.', 'banner');
  if (line !== null && !active.ended) submitLine(line);
});

document.getElementById('net-deny').addEventListener('click', () => {
  if (!active) return;
  active.netChoice = false;
  active.pendingNetLine = null;
  netbarEl.style.display = 'none';
  setNetState();
  addLine('الشبكة مغلقة لهذه الجلسة.', 'banner');
});

document.getElementById('sessions-btn').addEventListener('click', () => {
  if (drawerEl.classList.contains('open')) closeDrawer();
  else openDrawer();
});

backdropEl.addEventListener('click', closeDrawer);

document.getElementById('add-session').addEventListener('click', () => {
  switchSession(createSession());
  closeDrawer();
});

document.getElementById('keyboard-toggle').addEventListener('click', () => {
  // terminal-style keyboard toggle: hide the soft keyboard when
  // it is up, show it when it is down.
  if (document.activeElement === input) input.blur();
  else input.focus();
});

input.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    const line = input.value;
    input.value = '';
    // Store the submission here, not in submitLine: the
    // network-allow re-run calls submitLine again for the
    // same line, and it must not be stored twice.
    if (line.trim() !== '') active.history.push(line);
    active.histIdx = null;
    submitLine(line);
    return;
  }
  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    e.preventDefault();
    if (!active || active.ended) return;
    const h = active.history;
    if (e.key === 'ArrowUp') {
      if (h.length === 0) return;
      if (active.histIdx === null) active.histIdx = h.length - 1;
      else if (active.histIdx > 0) active.histIdx -= 1;
      input.value = h[active.histIdx];
    } else {
      if (active.histIdx === null) return;
      active.histIdx += 1;
      if (active.histIdx >= h.length) {
        // Past the newest: back to a fresh empty input.
        active.histIdx = null;
        input.value = '';
      } else {
        input.value = h[active.histIdx];
      }
    }
  }
});

// Typing anything after navigating resets the position to
// the newest. (Setting input.value from code does not fire
// this event, so navigation itself never resets it.)
input.addEventListener('input', () => {
  if (active) active.histIdx = null;
});

restartEl.addEventListener('click', restartActiveSession);
document.body.addEventListener('click', () => {
  if (active && !active.ended) input.focus();
});

try {
  await init();
  switchSession(createSession());
} catch (e) {
  const div = document.createElement('div');
  div.className = 'err';
  div.textContent = 'تعذّر تحميل محرك نيفا — أعد تشغيل التطبيق.';
  scrollwrap.appendChild(div);
}
