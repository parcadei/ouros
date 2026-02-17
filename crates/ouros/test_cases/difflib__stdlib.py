# === Module import and public API surface ===
import difflib

expected_public = [
    'Differ',
    'GenericAlias',
    'HtmlDiff',
    'IS_CHARACTER_JUNK',
    'IS_LINE_JUNK',
    'Match',
    'SequenceMatcher',
    'context_diff',
    'diff_bytes',
    'get_close_matches',
    'ndiff',
    'restore',
    'unified_diff',
]
for name in expected_public:
    assert hasattr(difflib, name), f'missing public name: {name}'

# === IS_* helpers ===
assert difflib.IS_CHARACTER_JUNK(' ') is True, 'space should be junk by default'
assert difflib.IS_CHARACTER_JUNK('x') is False, 'non-whitespace char should not be junk'
assert difflib.IS_LINE_JUNK('#\n') is True, 'comment-only line should be junk'
assert difflib.IS_LINE_JUNK('value\n') is False, 'non-empty content line should not be junk'

# === SequenceMatcher ===
sm = difflib.SequenceMatcher(None, 'abcd', 'abxd')
assert round(sm.ratio(), 6) == 0.75, 'ratio mismatch for basic substitution'
assert 0.0 <= sm.quick_ratio() <= 1.0, 'quick_ratio should be normalized'
assert 0.0 <= sm.real_quick_ratio() <= 1.0, 'real_quick_ratio should be normalized'

longest = sm.find_longest_match(0, 4, 0, 4)
assert (longest.a, longest.b, longest.size) == (0, 0, 2), 'find_longest_match mismatch'

blocks = sm.get_matching_blocks()
assert blocks[-1].size == 0, 'matching_blocks must end with zero-size sentinel'

opcodes = sm.get_opcodes()
assert opcodes[0][0] == 'equal', 'opcodes should begin with equal for common prefix'
assert any(code[0] == 'replace' for code in opcodes), 'opcodes should include replace for changed middle char'

groups = list(sm.get_grouped_opcodes())
assert len(groups) >= 1, 'grouped opcodes should return at least one group'

sm.set_seq1('abc')
sm.set_seq2('axc')
assert round(sm.ratio(), 6) == round(2 / 3, 6), 'set_seq1/set_seq2 should update matcher state'

sm.set_seqs('hello', 'yellow')
assert sm.find_longest_match(0, 5, 0, 6).size >= 4, 'set_seqs should update both sequences'

# === get_close_matches ===
close = difflib.get_close_matches('appel', ['apple', 'apply', 'ape', 'maple'], n=2, cutoff=0.6)
assert len(close) == 2, 'get_close_matches should honor n'
assert all(name in ['apple', 'apply', 'ape', 'maple'] for name in close), 'get_close_matches returned unknown candidate'

# === ndiff / restore ===
a_lines = ['one\n', 'two\n']
b_lines = ['one\n', 'too\n']
delta = list(difflib.ndiff(a_lines, b_lines))
assert delta[0].startswith('  '), 'ndiff first line should be unchanged marker'
assert any(line.startswith('- ') for line in delta), 'ndiff should mark deletions'
assert any(line.startswith('+ ') for line in delta), 'ndiff should mark insertions'

restored_a = ''.join(difflib.restore(delta, 1))
restored_b = ''.join(difflib.restore(delta, 2))
assert restored_a == ''.join(a_lines), 'restore(which=1) should reconstruct first input'
assert restored_b == ''.join(b_lines), 'restore(which=2) should reconstruct second input'

# === unified_diff / context_diff ===
unified = list(difflib.unified_diff(['a\n', 'b\n'], ['a\n', 'c\n'], fromfile='x', tofile='y'))
assert unified[0].startswith('--- '), 'unified diff must include --- header'
assert unified[1].startswith('+++ '), 'unified diff must include +++ header'
assert any(line.startswith('@@ ') for line in unified), 'unified diff must include hunk header'

context = list(difflib.context_diff(['a\n', 'b\n'], ['a\n', 'c\n'], fromfile='x', tofile='y'))
assert context[0].startswith('*** '), 'context diff must include *** header'
assert context[1].startswith('--- '), 'context diff must include --- header'
assert any(line.startswith('! ') or line.startswith('+ ') or line.startswith('- ') for line in context), (
    'context diff must include change markers'
)

# === diff_bytes ===
bytes_delta = list(
    difflib.diff_bytes(
        difflib.unified_diff,
        [b'a\n', b'b\n'],
        [b'a\n', b'c\n'],
        fromfile=b'x',
        tofile=b'y',
    )
)
assert bytes_delta[0].startswith(b'--- '), 'diff_bytes should preserve byte headers'
assert all(isinstance(line, bytes) for line in bytes_delta), 'diff_bytes should return bytes lines'

# === Differ ===
differ = difflib.Differ()
differ_delta = list(differ.compare(a_lines, b_lines))
assert differ_delta[0].startswith('  '), 'Differ.compare first line should be unchanged marker'
assert any(line.startswith('- ') for line in differ_delta), 'Differ.compare should mark deletions'
assert any(line.startswith('+ ') for line in differ_delta), 'Differ.compare should mark insertions'

# === HtmlDiff ===
html = difflib.HtmlDiff()
table = html.make_table(['a\n'], ['b\n'], fromdesc='left', todesc='right')
assert '<table' in table, 'HtmlDiff.make_table should render HTML table'
assert 'left' in table and 'right' in table, 'HtmlDiff.make_table should include headers'

file_html = html.make_file(['a\n'], ['b\n'])
assert '<html>' in file_html and '</html>' in file_html, 'HtmlDiff.make_file should render full HTML document'

# === Match namedtuple ===
match_obj = difflib.Match(1, 2, 3)
assert tuple(match_obj) == (1, 2, 3), 'Match should be tuple-compatible'
assert (match_obj.a, match_obj.b, match_obj.size) == (1, 2, 3), 'Match field access mismatch'
assert difflib.Match._fields == ('a', 'b', 'size'), 'Match._fields mismatch'
assert match_obj._asdict() == {'a': 1, 'b': 2, 'size': 3}, 'Match._asdict mismatch'
assert tuple(match_obj._replace(size=9)) == (1, 2, 9), 'Match._replace mismatch'

# === GenericAlias export ===
assert difflib.GenericAlias is not None, 'GenericAlias export should exist'
