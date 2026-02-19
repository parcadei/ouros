"""Tests for the CPython conformance differential test harness.

Tests the core logic: snippet parsing, output normalization, result comparison,
and report generation. Uses mock subprocesses to avoid needing real ouros binary.
"""
import json
import os
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Add parent dir to path so we can import harness
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import harness


class TestSnippetParsing(unittest.TestCase):
    """Test parsing of snippet header comments."""

    def test_parse_basic_header(self):
        """Parse a snippet with all header fields."""
        content = textwrap.dedent("""\
            # conformance: arithmetic
            # description: Integer addition
            # expect: pass
            # tags: int,operator,addition
            # ---
            print(1 + 2)
        """)
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
            f.write(content)
            f.flush()
            try:
                meta = harness.parse_snippet_metadata(Path(f.name))
            finally:
                os.unlink(f.name)

        self.assertEqual(meta["conformance"], "arithmetic")
        self.assertEqual(meta["description"], "Integer addition")
        self.assertEqual(meta["expect"], "pass")
        self.assertEqual(meta["tags"], ["int", "operator", "addition"])

    def test_parse_minimal_header(self):
        """Parse a snippet with only required fields."""
        content = textwrap.dedent("""\
            # conformance: strings
            # description: String concatenation
            # ---
            print("hello" + " world")
        """)
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
            f.write(content)
            f.flush()
            try:
                meta = harness.parse_snippet_metadata(Path(f.name))
            finally:
                os.unlink(f.name)

        self.assertEqual(meta["conformance"], "strings")
        self.assertEqual(meta["description"], "String concatenation")
        self.assertEqual(meta["expect"], "pass")  # default
        self.assertEqual(meta["tags"], [])  # default

    def test_parse_skip_snippet(self):
        """Parse a snippet marked as skip."""
        content = textwrap.dedent("""\
            # conformance: edge_cases
            # description: Known crash
            # expect: skip
            # ---
            import something_unavailable
        """)
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
            f.write(content)
            f.flush()
            try:
                meta = harness.parse_snippet_metadata(Path(f.name))
            finally:
                os.unlink(f.name)

        self.assertEqual(meta["expect"], "skip")

    def test_parse_no_header(self):
        """Snippet without header gets defaults."""
        content = "print(42)\n"
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
            f.write(content)
            f.flush()
            try:
                meta = harness.parse_snippet_metadata(Path(f.name))
            finally:
                os.unlink(f.name)

        self.assertEqual(meta["conformance"], "unknown")
        self.assertEqual(meta["description"], "")
        self.assertEqual(meta["expect"], "pass")


class TestNormalization(unittest.TestCase):
    """Test output normalization for fair comparison."""

    def test_strip_trailing_whitespace(self):
        result = harness.normalize_output("hello   \nworld  \n")
        self.assertEqual(result, "hello\nworld")

    def test_strip_trailing_empty_lines(self):
        result = harness.normalize_output("hello\nworld\n\n\n")
        self.assertEqual(result, "hello\nworld")

    def test_normalize_crlf(self):
        result = harness.normalize_output("hello\r\nworld\r\n")
        self.assertEqual(result, "hello\nworld")

    def test_normalize_memory_addresses(self):
        result = harness.normalize_output(
            "<object at 0x7f1234abcdef>\n<Foo at 0x104a8c3d0>"
        )
        self.assertEqual(result, "<object at 0xADDR>\n<Foo at 0xADDR>")

    def test_normalize_object_ids(self):
        result = harness.normalize_output("Widget(id=139842374)")
        self.assertEqual(result, "Widget(id=ID)")

    def test_normalize_class_repr(self):
        """Normalize <class '__main__.Foo'> vs <class 'Foo'>."""
        cpython = harness.normalize_output("<class '__main__.Foo'>")
        ouros = harness.normalize_output("<class 'Foo'>")
        self.assertEqual(cpython, ouros)

    def test_empty_string(self):
        result = harness.normalize_output("")
        self.assertEqual(result, "")

    def test_only_whitespace(self):
        result = harness.normalize_output("   \n  \n")
        self.assertEqual(result, "")


class TestCapturedResult(unittest.TestCase):
    """Test the CapturedResult data class."""

    def test_create_result(self):
        r = harness.CapturedResult(
            stdout="hello\n",
            stderr="",
            exit_code=0,
            duration_ms=45.0,
            timed_out=False,
        )
        self.assertEqual(r.stdout, "hello\n")
        self.assertEqual(r.exit_code, 0)
        self.assertFalse(r.timed_out)

    def test_timed_out_result(self):
        r = harness.CapturedResult(
            stdout="",
            stderr="",
            exit_code=-1,
            duration_ms=10000.0,
            timed_out=True,
        )
        self.assertTrue(r.timed_out)


class TestVerdictComparison(unittest.TestCase):
    """Test the comparison logic between CPython and ouros results."""

    def _make_result(self, stdout="", stderr="", exit_code=0,
                     duration_ms=50.0, timed_out=False):
        return harness.CapturedResult(
            stdout=stdout, stderr=stderr, exit_code=exit_code,
            duration_ms=duration_ms, timed_out=timed_out,
        )

    def test_pass_identical_output(self):
        cp = self._make_result(stdout="3\n")
        ou = self._make_result(stdout="3\n")
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "PASS")

    def test_fail_different_stdout(self):
        cp = self._make_result(stdout="3\n")
        ou = self._make_result(stdout="4\n")
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "FAIL")

    def test_error_ouros_crashed(self):
        """Ouros killed by signal (negative exit code)."""
        cp = self._make_result(stdout="hello\n", exit_code=0)
        ou = self._make_result(stdout="", exit_code=-11)  # SIGSEGV
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "ERROR")

    def test_error_ouros_failed_cpython_passed(self):
        cp = self._make_result(stdout="42\n", exit_code=0)
        ou = self._make_result(stdout="", exit_code=1)
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "ERROR")

    def test_pass_both_error_same_exit(self):
        """Both interpreters error on the same snippet (expect: error)."""
        cp = self._make_result(stdout="", exit_code=1)
        ou = self._make_result(stdout="", exit_code=1)
        meta = {"expect": "error"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "PASS")

    def test_pass_both_error_different_exit(self):
        """expect: error, both fail but different codes -- still PASS."""
        cp = self._make_result(stdout="", exit_code=1)
        ou = self._make_result(stdout="", exit_code=2)
        meta = {"expect": "error"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "PASS")

    def test_skip_verdict(self):
        """Snippets marked skip should return SKIP."""
        cp = self._make_result()
        ou = self._make_result()
        meta = {"expect": "skip"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "SKIP")

    def test_timeout_verdict(self):
        """If ouros timed out, verdict is TIMEOUT."""
        cp = self._make_result(stdout="ok\n")
        ou = self._make_result(timed_out=True)
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "TIMEOUT")

    def test_bad_snippet_cpython_fails(self):
        """If CPython itself fails, verdict is BAD_SNIPPET."""
        cp = self._make_result(exit_code=1, stderr="SyntaxError")
        ou = self._make_result(exit_code=1)
        meta = {"expect": "pass"}  # not expected to error
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "BAD_SNIPPET")

    def test_pass_with_trailing_whitespace_diff(self):
        """Trailing whitespace differences should not cause FAIL."""
        cp = self._make_result(stdout="hello   \nworld  \n")
        ou = self._make_result(stdout="hello\nworld\n")
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "PASS")

    def test_pass_with_class_repr_diff(self):
        """__main__.Foo vs Foo in class repr should not cause FAIL."""
        cp = self._make_result(stdout="<class '__main__.Foo'>\n")
        ou = self._make_result(stdout="<class 'Foo'>\n")
        meta = {"expect": "pass"}
        verdict = harness.compare_results(cp, ou, meta)
        self.assertEqual(verdict.verdict, "PASS")


class TestSnippetDiscovery(unittest.TestCase):
    """Test finding snippets in a directory tree."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        # Create structure: category/snippet.py
        cat1 = Path(self.tmpdir) / "arithmetic"
        cat1.mkdir()
        (cat1 / "int_add.py").write_text(
            "# conformance: arithmetic\n# description: add\n# ---\nprint(1+2)\n"
        )
        (cat1 / "int_sub.py").write_text(
            "# conformance: arithmetic\n# description: sub\n# ---\nprint(3-1)\n"
        )
        cat2 = Path(self.tmpdir) / "strings"
        cat2.mkdir()
        (cat2 / "concat.py").write_text(
            "# conformance: strings\n# description: concat\n# ---\nprint('a'+'b')\n"
        )
        # Non-py file should be ignored
        (cat2 / "README.md").write_text("ignore me\n")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_discover_all_snippets(self):
        snippets = harness.discover_snippets(Path(self.tmpdir))
        self.assertEqual(len(snippets), 3)

    def test_discover_sorted(self):
        """Snippets should be sorted by full path for deterministic ordering."""
        snippets = harness.discover_snippets(Path(self.tmpdir))
        paths = [str(s) for s in snippets]
        self.assertEqual(paths, sorted(paths))

    def test_discover_with_category_filter(self):
        snippets = harness.discover_snippets(
            Path(self.tmpdir), category="arithmetic"
        )
        self.assertEqual(len(snippets), 2)
        for s in snippets:
            self.assertIn("arithmetic", str(s))

    def test_discover_with_pattern_filter(self):
        snippets = harness.discover_snippets(
            Path(self.tmpdir), pattern="*add*"
        )
        self.assertEqual(len(snippets), 1)
        self.assertIn("int_add", snippets[0].name)


class TestCategoryGrouping(unittest.TestCase):
    """Test grouping results by category."""

    def test_group_by_category(self):
        results = [
            {"category": "arithmetic", "verdict": "PASS"},
            {"category": "arithmetic", "verdict": "PASS"},
            {"category": "strings", "verdict": "FAIL"},
            {"category": "strings", "verdict": "PASS"},
        ]
        grouped = harness.group_by_category(results)
        self.assertEqual(grouped["arithmetic"]["total"], 2)
        self.assertEqual(grouped["arithmetic"]["passed"], 2)
        self.assertEqual(grouped["strings"]["total"], 2)
        self.assertEqual(grouped["strings"]["passed"], 1)


class TestJsonReport(unittest.TestCase):
    """Test JSON report generation."""

    def test_json_report_structure(self):
        results = [
            {
                "snippet": "arithmetic/int_add.py",
                "category": "arithmetic",
                "description": "add",
                "verdict": "PASS",
                "duration_cpython_ms": 45.0,
                "duration_ouros_ms": 120.0,
                "cpython_exit": 0,
                "ouros_exit": 0,
            }
        ]
        report = harness.build_json_report(
            results,
            ouros_binary="/path/ouros",
            cpython_binary="python3",
        )
        self.assertIn("timestamp", report)
        self.assertEqual(report["total"], 1)
        self.assertEqual(report["passed"], 1)
        self.assertEqual(report["failed"], 0)
        self.assertEqual(report["pass_rate"], 1.0)
        self.assertEqual(len(report["results"]), 1)

    def test_json_report_pass_rate(self):
        results = [
            {"verdict": "PASS", "category": "a"},
            {"verdict": "PASS", "category": "a"},
            {"verdict": "FAIL", "category": "b"},
            {"verdict": "ERROR", "category": "b"},
            {"verdict": "SKIP", "category": "c"},
        ]
        # Fill in other fields
        for r in results:
            r.setdefault("snippet", "test.py")
            r.setdefault("description", "")
            r.setdefault("duration_cpython_ms", 0)
            r.setdefault("duration_ouros_ms", 0)
            r.setdefault("cpython_exit", 0)
            r.setdefault("ouros_exit", 0)

        report = harness.build_json_report(results, "/ouros", "python3")
        # SKIP should not count in total for pass_rate
        # total testable = 4 (excluding skips), passed = 2
        self.assertEqual(report["total"], 5)
        self.assertEqual(report["passed"], 2)
        self.assertEqual(report["skipped"], 1)
        # pass_rate = passed / (total - skipped) = 2/4 = 0.5
        self.assertAlmostEqual(report["pass_rate"], 0.5)


class TestJunitReport(unittest.TestCase):
    """Test JUnit XML report generation."""

    def test_junit_xml_valid(self):
        results = [
            {
                "snippet": "arithmetic/int_add.py",
                "category": "arithmetic",
                "description": "add",
                "verdict": "PASS",
                "duration_cpython_ms": 45.0,
                "duration_ouros_ms": 120.0,
                "cpython_exit": 0,
                "ouros_exit": 0,
            },
            {
                "snippet": "strings/concat.py",
                "category": "strings",
                "description": "concat",
                "verdict": "FAIL",
                "duration_cpython_ms": 30.0,
                "duration_ouros_ms": 80.0,
                "cpython_exit": 0,
                "ouros_exit": 0,
                "diff": {"stdout": "- cpython: hello\n+ ouros: helo"},
            },
        ]
        xml = harness.build_junit_xml(results)
        self.assertIn('<?xml version=', xml)
        self.assertIn('<testsuite', xml)
        self.assertIn('name="arithmetic/int_add.py"', xml)
        self.assertIn('<failure', xml)
        self.assertIn('</testsuite>', xml)


class TestDiffGeneration(unittest.TestCase):
    """Test unified diff generation for FAIL verdicts."""

    def test_diff_output(self):
        diff = harness.generate_diff("hello\nworld\n", "hello\nworl\n")
        self.assertIn("---", diff)
        self.assertIn("+++", diff)
        self.assertIn("-world", diff)
        self.assertIn("+worl", diff)


if __name__ == "__main__":
    unittest.main()
