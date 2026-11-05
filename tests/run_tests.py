#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Rio4140 test suite: transpile every example, run it, compare output.

Run from the project root:  python3 tests/run_tests.py
Exit 0 only if every example passes. No example is silently skipped.
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = ROOT / "examples"

EXPECTED = {
    "01-hello": "أهلا بك في Rio4140\nهذه طباعة بأقواس أيضا\n5\n",
    "02-variables": "8\n15\nمحمود\nTrue\n",
    "03-conditions": "ناجح\nفي المدى\nليس ٩٩\n",
    "04-loops": "0\n1\n2\n0\n1\n2\nR\ni\no\n",
    "05-functions": "7\nأهلا محمود\n25\n",
    "06-lists-dicts": "1\n3\n6\n3\nمحمود\n23\nاسم\nعمر\n",
    "07-classes": "أحمر\n",
    "08-files-errors": "خطأ في القسمة\nمرحبا من Rio4140\nتم الإمساك بالخطأ\nانتهى\n",
    "09-imports": "4.0\n5.0\n3.141592653589793\n",
    "10-full-program": (
        "محمود: ناجح\nسارة: ممتاز\nعلي: راسب\nالمتوسط:\n"
        "72.33333333333333\nأعلى درجة:\n92\n"
    ),
}


def main() -> int:
    failures = 0
    for name, expected in EXPECTED.items():
        rio = EXAMPLES / f"{name}.rio"
        if not rio.exists():
            print(f"FAIL {name}: missing {rio}")
            failures += 1
            continue
        proc = subprocess.run(
            [sys.executable, str(ROOT / "rio4140.py"), str(rio), "--run"],
            capture_output=True,
            text=True,
        )
        # --run prints "تم التحويل إلى: ..." on stdout first; strip that line.
        out = proc.stdout
        if out.startswith("تم التحويل"):
            out = out.split("\n", 1)[1] if "\n" in out else ""
        if proc.returncode != 0 or out != expected:
            print(f"FAIL {name}: exit={proc.returncode}")
            print(f"  expected: {expected!r}")
            print(f"  got:      {out!r}")
            print(f"  stderr:   {proc.stderr!r}")
            failures += 1
        else:
            print(f"PASS {name}")
    # Clean the file example 08 writes (it is a test artefact, not a deliverable).
    test_file = EXAMPLES / "rio-test.txt"
    if test_file.exists():
        test_file.unlink()
    print(f"{len(EXPECTED) - failures}/{len(EXPECTED)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
