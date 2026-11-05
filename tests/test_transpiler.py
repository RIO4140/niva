#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Formal unit tests for the Rio4140 transpiler.

Run from the project root (no third-party packages needed):
    python3 -m unittest discover -s tests -v
    python3 tests/run_tests.py          # integration: the 10 examples

These tests are the contract: if a test fails, the transpiler is wrong,
not the test. They cover the design decisions from 2026-11-05:
- fixed MSA keywords, not natural Arabic sentences;
- forgiving parser for keywords only (diacritics, hamza, digits, punctuation);
- identifiers stay distinct (diacritics stripped only, hamza kept);
- strings and comments are never translated;
- output is real, standalone Python.
"""
import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rio4140 import transpile  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


class TestKeywords(unittest.TestCase):
    def test_print(self):
        self.assertEqual(transpile('اطبع "مرحبا"'), 'print("مرحبا")')
        self.assertEqual(transpile('اطبع("مرحبا")'), 'print("مرحبا")')

    def test_conditions(self):
        self.assertEqual(
            transpile("إذا س > ٥:\n    اطبع س"),
            "if س > 5:\n    print(س)",
        )
        self.assertEqual(transpile("وإلا إذا س == ١:"), "elif س == 1:")
        self.assertEqual(transpile("وإلا:"), "else:")

    def test_loops_and_functions(self):
        self.assertEqual(
            transpile("لكل رقم في مدى(٣):"), "for رقم in range(3):"
        )
        self.assertEqual(transpile("بينما س < ٣:"), "while س < 3:")
        self.assertEqual(
            transpile("دالة جمع(أ، ب):\n    ارجع أ + ب"),
            "def جمع(أ, ب):\n    return أ + ب",
        )

    def test_classes_imports_errors(self):
        self.assertEqual(transpile("فئة سيارة:"), "class سيارة:")
        self.assertEqual(transpile("استورد math"), "import math")
        self.assertEqual(transpile("من math استورد sqrt"), "from math import sqrt")
        self.assertEqual(transpile("استورد math باسم رياضيات"), "import math as رياضيات")
        self.assertEqual(transpile("حاول:"), "try:")
        self.assertEqual(transpile("ماعدا:"), "except:")
        self.assertEqual(transpile("أخيرا:"), "finally:")

    def test_booleans_and_operators(self):
        self.assertEqual(transpile("س = صحيح"), "س = True")
        self.assertEqual(transpile("س = خطأ"), "س = False")
        self.assertEqual(transpile("س = لاشيء"), "س = None")
        self.assertEqual(transpile("إذا أ و ب أو ليس ج:"), "if أ and ب or not ج:")
        self.assertEqual(transpile("إذا س ليس في قائمة:"), "if س not in قائمة:")

    def test_builtins(self):
        self.assertEqual(transpile("اطبع طول(قائمة)"), "print(len(قائمة))")
        self.assertEqual(transpile("اطبع مجموع(قائمة)"), "print(sum(قائمة))")
        self.assertEqual(transpile("اطبع مدى(٥)"), "print(range(5))")


class TestForgivingParser(unittest.TestCase):
    def test_hamza_variants(self):
        # اذا without hamza must work exactly like إذا.
        self.assertEqual(transpile("اذا س:"), "if س:")
        self.assertEqual(transpile("إذا س:"), "if س:")

    def test_diacritics_on_keyword(self):
        # A diacritic inside a keyword must not break tokenisation.
        self.assertEqual(transpile('اِطبع "مرحبا"'), 'print("مرحبا")')

    def test_arabic_digits_and_punctuation(self):
        self.assertEqual(transpile("س = ١٢٣"), "س = 123")
        self.assertEqual(transpile("قائمة = [١، ٢، ٣]"), "قائمة = [1, 2, 3]")
        self.assertEqual(transpile("دالة جمع(أ؛ ب):"), "def جمع(أ; ب):")

    def test_identifiers_stay_distinct(self):
        # مدرسة (ة) and مدرسه (ه) are DIFFERENT names; never merge them.
        out = transpile("مدرسة = ١\nمدرسه = ٢")
        self.assertEqual(out, "مدرسة = 1\nمدرسه = 2")


class TestStringsAndComments(unittest.TestCase):
    def test_string_never_translated(self):
        self.assertEqual(transpile('اطبع "إذا ١٢٣"'), 'print("إذا ١٢٣")')

    def test_comment_never_translated(self):
        self.assertEqual(transpile("# إذا تعليق"), "# إذا تعليق")
        self.assertEqual(
            transpile('اطبع س  # إذا تعليق'), 'print(س) # إذا تعليق'
        )

    def test_hash_inside_string_is_not_comment(self):
        self.assertEqual(transpile('اطبع "a#b"'), 'print("a#b")')


class TestTranspiledCodeRuns(unittest.TestCase):
    def _run(self, rio_source):
        py = transpile(rio_source)
        proc = subprocess.run(
            [sys.executable, "-c", py], capture_output=True, text=True
        )
        self.assertEqual(proc.returncode, 0, f"Python failed: {proc.stderr}\n{py}")
        return proc.stdout

    def test_full_program_runs(self):
        out = self._run(
            "درجات = [٨٥، ٩٢، ٤٠]\n"
            "اطبع مجموع(درجات)\n"
            "اطبع طول(درجات)\n"
        )
        self.assertEqual(out, "217\n3\n")

    def test_blank_lines_and_empty_input(self):
        self.assertEqual(transpile(""), "")
        self.assertEqual(transpile("\n\n"), "\n\n")


class TestCLI(unittest.TestCase):
    def _cli(self, *args):
        return subprocess.run(
            [sys.executable, str(ROOT / "rio4140.py"), *args],
            capture_output=True, text=True, cwd=str(ROOT),
        )

    def test_stdout(self):
        proc = self._cli("examples/01-hello.rio", "--stdout")
        self.assertEqual(proc.returncode, 0)
        self.assertIn('print("أهلا بك في Rio4140")', proc.stdout)

    def test_keywords_and_version(self):
        self.assertEqual(self._cli("--keywords").returncode, 0)
        self.assertIn("اطبع -> print", self._cli("--keywords").stdout)
        self.assertIn("Rio4140", self._cli("--version").stdout)

    def test_missing_file_fails(self):
        proc = self._cli("no-such-file.rio")
        self.assertNotEqual(proc.returncode, 0)


class TestPlayground(unittest.TestCase):
    def test_playground_mirrors_transpiler(self):
        html = (ROOT / "playground" / "index.html").read_text(encoding="utf-8")
        # The browser transpiler must carry the same core keywords.
        for keyword in ("اطبع", "إذا", "لكل", "دالة", "وإلا"):
            self.assertIn(keyword, html)
        self.assertIn("Pyodide", html)
        # No server execution: the playground must say so.
        self.assertIn("لا يرسل", html)


if __name__ == "__main__":
    unittest.main()
