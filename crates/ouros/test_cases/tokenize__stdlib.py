import sys
import tokenize


# === public API ===
assert hasattr(tokenize, 'tokenize'), 'tokenize exports tokenize'
assert hasattr(tokenize, 'generate_tokens'), 'tokenize exports generate_tokens'
assert hasattr(tokenize, 'detect_encoding'), 'tokenize exports detect_encoding'
assert hasattr(tokenize, 'untokenize'), 'tokenize exports untokenize'
assert hasattr(tokenize, 'open'), 'tokenize exports open'
assert hasattr(tokenize, 'TokenInfo'), 'tokenize exports TokenInfo'
assert hasattr(tokenize, 'TokenError'), 'tokenize exports TokenError'


# === token constants and helper predicates ===
assert tokenize.ENDMARKER == 0, 'tokenize_endmarker'
assert tokenize.OP == 55, 'tokenize_op'
assert tokenize.ENCODING == 68, 'tokenize_encoding'
assert tokenize.N_TOKENS == 69, 'tokenize_n_tokens'
assert tokenize.ISTERMINAL(0) == True, 'isterminal_zero'
assert tokenize.ISTERMINAL(x=0) == True, 'isterminal_keyword'
assert tokenize.ISNONTERMINAL(0) == False, 'isnonterminal_zero'
assert tokenize.ISNONTERMINAL(256) == True, 'isnonterminal_offset'
assert tokenize.ISEOF(0) == True, 'iseof_zero'
assert tokenize.ISEOF(1) == False, 'iseof_one'


# === TokenInfo namedtuple ===
info = tokenize.TokenInfo(1, 'name', (1, 0), (1, 4), 'name\n')
assert tuple(info) == (1, 'name', (1, 0), (1, 4), 'name\n'), 'tokeninfo_tuple'
assert info.type == 1, 'tokeninfo_type'
assert info.string == 'name', 'tokeninfo_string'
assert info.start == (1, 0), 'tokeninfo_start'
assert info.end == (1, 4), 'tokeninfo_end'
assert info.line == 'name\n', 'tokeninfo_line'
assert tokenize.TokenInfo._fields == ('type', 'string', 'start', 'end', 'line'), 'tokeninfo_fields'
assert info._asdict() == {
    'type': 1,
    'string': 'name',
    'start': (1, 0),
    'end': (1, 4),
    'line': 'name\n',
}, 'tokeninfo_asdict'


def _readline_factory(text, *, as_bytes):
    lines = text.splitlines(keepends=True)
    idx = {'i': 0}

    def _readline():
        i = idx['i']
        if i >= len(lines):
            return b'' if as_bytes else ''
        idx['i'] = i + 1
        line = lines[i]
        return line.encode('utf-8') if as_bytes else line

    return _readline


# === detect_encoding ===
if sys.platform == 'ouros':
    enc, lines = tokenize.detect_encoding('x = 1\n')
    enc_cookie, lines_cookie = tokenize.detect_encoding('# coding: latin-1\nx = 1\n')
else:
    enc, lines = tokenize.detect_encoding(_readline_factory('x = 1\n', as_bytes=True))
    enc_cookie, lines_cookie = tokenize.detect_encoding(_readline_factory('# coding: latin-1\nx = 1\n', as_bytes=True))

assert enc == 'utf-8', 'detect_encoding_default'
assert isinstance(lines, list), 'detect_encoding_lines_type'
assert enc_cookie in ('latin-1', 'iso-8859-1'), 'detect_encoding_cookie'
assert len(lines_cookie) >= 1, 'detect_encoding_cookie_lines'


# === tokenize / generate_tokens ===
source = 'x = 1\n#comment\n'
if sys.platform == 'ouros':
    full_tokens = list(tokenize.tokenize(source))
    text_tokens = list(tokenize.generate_tokens(source))
else:
    full_tokens = list(tokenize.tokenize(_readline_factory(source, as_bytes=True)))
    text_tokens = list(tokenize.generate_tokens(_readline_factory(source, as_bytes=False)))

assert len(full_tokens) > 0, 'tokenize_non_empty'
assert full_tokens[0].type == tokenize.ENCODING, 'tokenize_includes_encoding'
assert full_tokens[-1].type == tokenize.ENDMARKER, 'tokenize_endmarker'
assert any(t.type == tokenize.NAME and t.string == 'x' for t in full_tokens), 'tokenize_name'
assert any(t.type == tokenize.COMMENT and t.string == '#comment' for t in full_tokens), 'tokenize_comment'
assert any(t.type == tokenize.NEWLINE for t in full_tokens), 'tokenize_newline'
assert any(t.type == tokenize.NL for t in full_tokens), 'tokenize_nl'

assert text_tokens[0].type != tokenize.ENCODING, 'generate_tokens_no_encoding'
assert text_tokens[-1].type == tokenize.ENDMARKER, 'generate_tokens_endmarker'


# === untokenize ===
reconstructed = tokenize.untokenize(full_tokens)
if isinstance(reconstructed, bytes):
    reconstructed_text = reconstructed.decode('utf-8')
else:
    reconstructed_text = reconstructed
assert 'x = 1' in reconstructed_text, 'untokenize_contains_code'
assert '#comment' in reconstructed_text, 'untokenize_contains_comment'


# === open() sandbox behavior ===
try:
    tokenize.open('file.py')
    assert False, 'tokenize.open should fail'
except OSError as exc:
    if sys.platform == 'ouros':
        assert 'sandbox' in str(exc), 'tokenize.open error message'
    else:
        assert 'No such file' in str(exc), 'tokenize.open missing file'
