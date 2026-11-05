#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Rio4140 — Arabic programming language, transpiled to real Python.
Standalone personal tool, built from scratch. NOT a network, NOT a hub.
Design decisions (2026-11-05, discussion):
- Formal language with fixed Arabic keywords (MSA), NOT natural Arabic sentences.
- Forgiving parser: diacritics stripped, hamza unified for KEYWORDS ONLY,
  Arabic-Indic digits and Arabic punctuation accepted.
- Identifiers are preserved exactly except diacritics (so مدرسة != مدرسه).
- String literals and comments are NEVER translated.
- Output is real, standalone Python: it runs outside Rio4140, forever.
- NO server execution: the transpiler runs locally; the browser playground
  runs code only in the learner's own browser (Pyodide) or locally.
Usage:
    python3 rio4140.py program.rio            -> writes program.py
    python3 rio4140.py program.rio --run      -> writes program.py and runs it
    python3 rio4140.py program.rio --stdout   -> prints the Python code
    python3 rio4140.py --keywords             -> prints the keyword table
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

__version__ = "1.0.0"

# ---------------------------------------------------------------------------
# Normalisation
# ---------------------------------------------------------------------------
_DIACRITICS = re.compile(r"[\u064b-\u0652\u0670\u0640]")
_ARABIC_DIGITS = str.maketrans("٠١٢٣٤٥٦٧٨٩", "0123456789")
_ARABIC_PUNCT = {"،": ",", "؛": ";"}


def strip_diacritics(text: str) -> str:
    """Remove tashkeel and tatweel only. Hamza and ة/ه are kept."""
    return _DIACRITICS.sub("", text)


def normalize_keyword(word: str) -> str:
    """Aggressive normalisation, used ONLY to recognise keywords."""
    w = strip_diacritics(word)
    w = w.replace("أ", "ا").replace("إ", "ا").replace("آ", "ا")
    w = w.replace("ؤ", "و").replace("ئ", "ي")
    w = w.replace("ة", "ه").replace("ى", "ي")
    return w


# Keyword table: Arabic (raw) -> Python. Keys are normalised at load time.
_RAW_KEYWORDS = {
    "اطبع": "print",
    "إذا": "if",
    "وإلا": "else",
    "بينما": "while",
    "لكل": "for",
    "دالة": "def",
    "ارجع": "return",
    "فئة": "class",
    "استورد": "import",
    "من": "from",
    "حاول": "try",
    "ماعدا": "except",
    "أخيرا": "finally",
    "صحيح": "True",
    "خطأ": "False",
    "لاشيء": "None",
    "و": "and",
    "أو": "or",
    "ليس": "not",
    "في": "in",
    "مرر": "pass",
    "اكسر": "break",
    "تابع": "continue",
    "احذف": "del",
    "أكد": "assert",
    "مع": "with",
    "باسم": "as",
    "أثير": "raise",
    "عالمي": "global",
    "مدى": "range",
    "طول": "len",
    "مجموع": "sum",
    "أدنى": "min",
    "أقصى": "max",
    "مرتب": "sorted",
    "مطلق": "abs",
    "افتح": "open",
    "ادخل": "input",
    "نوع": "type",
}
KEYWORDS = {normalize_keyword(k): v for k, v in _RAW_KEYWORDS.items()}

# ---------------------------------------------------------------------------
# String / comment handling: strings and comments are never translated.
# ---------------------------------------------------------------------------


def split_strings_and_comment(line: str):
    """Return (code_with_placeholders, strings, comment).

    Strings are replaced by __RIO_STR_<n>__ placeholders.
    The comment (from # outside a string) is returned unchanged.
    """
    strings = []
    out = []
    i = 0
    n = len(line)
    while i < n:
        ch = line[i]
        # Triple-quoted strings
        if line.startswith('"""', i) or line.startswith("'''", i):
            quote = line[i : i + 3]
            j = line.find(quote, i + 3)
            if j == -1:
                lit = line[i:]
                i = n
            else:
                lit = line[i : j + 3]
                i = j + 3
            ph = f"__RIO_STR_{len(strings)}__"
            strings.append(lit)
            out.append(ph)
            continue
        if ch in ("'", '"'):
            j = i + 1
            while j < n:
                if line[j] == "\\":
                    j += 2
                    continue
                if line[j] == ch:
                    break
                j += 1
            if j < n:
                lit = line[i : j + 1]
                i = j + 1
            else:  # unterminated: keep the rest, Python will report it
                lit = line[i:]
                i = n
            ph = f"__RIO_STR_{len(strings)}__"
            strings.append(lit)
            out.append(ph)
            continue
        if ch == "#":
            # Everything from here is a comment.
            return "".join(out), strings, line[i:]
        out.append(ch)
        i += 1
    return "".join(out), strings, ""


_ARABIC_LETTERS = r"\u0621-\u064A\u066E-\u06D3"
_TOKEN_RE = re.compile(
    r"[0-9٠-٩]+|[" + _ARABIC_LETTERS + r"]+|[A-Za-z_][A-Za-z0-9_]*|.",
    re.DOTALL,
)


def _is_word(tok: str) -> bool:
    return bool(
        re.match(r"^[" + _ARABIC_LETTERS + r"]+$|^[_A-Za-z][_A-Za-z0-9]*$", tok)
    )


def _is_space(tok: str) -> bool:
    return tok.strip() == ""


def translate_code(code: str) -> str:
    """Translate one code line (no strings, no comment) to Python."""
    tokens = _TOKEN_RE.findall(code)
    out = []
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if _is_word(tok) and re.search("[" + _ARABIC_LETTERS + "]", tok):
            norm = normalize_keyword(tok)
            # Two-word phrases, checked before single words.
            if norm == normalize_keyword("وإلا"):
                j = i + 1
                while j < len(tokens) and _is_space(tokens[j]):
                    j += 1
                if j < len(tokens) and normalize_keyword(tokens[j]) == normalize_keyword(
                    "إذا"
                ):
                    out.append("elif")
                    i = j + 1
                    continue
            if norm == normalize_keyword("ليس"):
                j = i + 1
                while j < len(tokens) and _is_space(tokens[j]):
                    j += 1
                if j < len(tokens) and normalize_keyword(tokens[j]) == normalize_keyword(
                    "في"
                ):
                    out.append("not in")
                    i = j + 1
                    continue
            if norm in KEYWORDS:
                out.append(KEYWORDS[norm])
            else:
                # Identifier: strip diacritics only, keep hamza/ة/ه distinct.
                out.append(strip_diacritics(tok))
            i += 1
            continue
        if re.fullmatch(r"[0-9٠-٩]+", tok):
            out.append(tok.translate(_ARABIC_DIGITS))
            i += 1
            continue
        if tok in _ARABIC_PUNCT:
            out.append(_ARABIC_PUNCT[tok])
            i += 1
            continue
        out.append(tok)
        i += 1
    return "".join(out)


def translate_line(line: str) -> str:
    code, strings, comment = split_strings_and_comment(line.rstrip("\n"))
    py = translate_code(code)
    # اطبع without parentheses:  اطبع "مرحبا"  ->  print("مرحبا")
    m = re.match(r"^(\s*)print\s+(.+)$", py)
    if m and not m.group(2).lstrip().startswith("("):
        py = f"{m.group(1)}print({m.group(2).rstrip()})"
    for idx, lit in enumerate(strings):
        py = py.replace(f"__RIO_STR_{idx}__", lit)
    return py + comment


def transpile(source: str) -> str:
    return "\n".join(translate_line(line) for line in source.split("\n"))


def keyword_table() -> str:
    rows = ["Rio4140 -> Python"]
    for arabic, py in _RAW_KEYWORDS.items():
        rows.append(f"  {arabic} -> {py}")
    rows.append("  وإلا إذا -> elif")
    rows.append("  ليس في -> not in")
    return "\n".join(rows)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        prog="rio4140",
        description="Rio4140: Arabic code -> real Python.",
    )
    ap.add_argument("source", nargs="?", help=".rio source file")
    ap.add_argument("-o", "--output", help="output .py file (default: same name, .py)")
    ap.add_argument("--run", action="store_true", help="run the generated Python")
    ap.add_argument("--stdout", action="store_true", help="print Python, do not write a file")
    ap.add_argument("--keywords", action="store_true", help="print the keyword table and exit")
    ap.add_argument("--version", action="store_true", help="print version and exit")
    args = ap.parse_args(argv)

    if args.version:
        print(f"Rio4140 {__version__}")
        return 0
    if args.keywords:
        print(keyword_table())
        return 0
    if not args.source:
        ap.print_help()
        return 2

    src_path = Path(args.source)
    if not src_path.exists():
        print(f"خطأ: الملف غير موجود: {args.source}", file=sys.stderr)
        return 1
    source = src_path.read_text(encoding="utf-8")
    try:
        python_code = transpile(source)
    except Exception as exc:  # never hide a transpiler crash
        print(f"خطأ في التحويل: {exc}", file=sys.stderr)
        return 1

    if args.stdout:
        print(python_code)
    else:
        out_path = Path(args.output) if args.output else src_path.with_suffix(".py")
        out_path.write_text(python_code + "\n", encoding="utf-8")
        print(f"تم التحويل إلى: {out_path}")

    if args.run:
        out_path = (
            Path(args.output).resolve()
            if args.output
            else src_path.with_suffix(".py").resolve()
        )
        if args.stdout:
            out_path.write_text(python_code + "\n", encoding="utf-8")
        proc = subprocess.run(
            [sys.executable, str(out_path)],
            cwd=str(src_path.parent.resolve()),
            capture_output=True,
            text=True,
        )
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        return proc.returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
