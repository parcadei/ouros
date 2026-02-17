import re

# === match ===
try:
    print('match_basic', re.match(r'\d+', '123abc').group())
    print('match_no_match', re.match(r'\d+', 'abc123'))
    print('match_pos', re.compile(r'\d+').match('abc123', pos=3).group())
    print('match_endpos', re.compile(r'\d+').match('123abc456', endpos=3).group())
    print('match_flags', re.match(r'[a-z]+', 'ABC', re.IGNORECASE).group())
    print('match_groups', re.match(r'(\d)(\d)(\d)', '123').groups())
    print('match_groupdict', re.match(r'(?P<first>\d)(?P<second>\d)', '12').groupdict())
except Exception as e:
    print('SKIP_match', type(e).__name__, e)

# === search ===
try:
    print('search_basic', re.search(r'\d+', 'abc123def').group())
    print('search_no_match', re.search(r'xyz', 'abcdef'))
    print('search_pos', re.compile(r'\d+').search('123abc456', pos=3).group())
    print('search_endpos', re.compile(r'\d+').search('123abc456', endpos=6).group())
    print('search_flags', re.search(r'[A-Z]+', 'abc', re.IGNORECASE).group())
except Exception as e:
    print('SKIP_search', type(e).__name__, e)

# === fullmatch ===
try:
    print('fullmatch_basic', re.fullmatch(r'\d+', '123').group())
    print('fullmatch_no_match', re.fullmatch(r'\d+', '123abc'))
    print('fullmatch_flags', re.fullmatch(r'[a-z]+', 'ABC', re.IGNORECASE).group())
except Exception as e:
    print('SKIP_fullmatch', type(e).__name__, e)

# === findall ===
try:
    print('findall_basic', re.findall(r'\d+', '123abc456def789'))
    print('findall_empty', re.findall(r'\d+', 'abcdef'))
    print('findall_groups', re.findall(r'(\d)(\w)', '1a2b3c'))
    print('findall_overlap', re.findall(r'(?=(\d\d))', '12345'))
except Exception as e:
    print('SKIP_findall', type(e).__name__, e)

# === finditer ===
try:
    print('finditer_basic', [m.group() for m in re.finditer(r'\d+', '123abc456')])
    print('finditer_groups', [(m.group(1), m.group(2)) for m in re.finditer(r'(\d)(\w)', '1a2b')])
except Exception as e:
    print('SKIP_finditer', type(e).__name__, e)

# === sub ===
try:
    print('sub_basic', re.sub(r'\d+', '#', '123abc456'))
    print('sub_count', re.sub(r'\d+', '#', '123abc456', count=1))
    print('sub_groups', re.sub(r'(\d)(\w)', r'\2\1', '1a2b3c'))
    print('sub_func', re.sub(r'\d+', lambda m: str(len(m.group())), '123abc45'))
    print('sub_no_match', re.sub(r'xyz', '#', 'abcdef'))
    print('sub_empty', re.sub(r'\d*', '-', 'ab'))
except Exception as e:
    print('SKIP_sub', type(e).__name__, e)

# === subn ===
try:
    print('subn_basic', re.subn(r'\d+', '#', '123abc456'))
    print('subn_count', re.subn(r'\d+', '#', '123abc456', count=1))
    print('subn_no_match', re.subn(r'xyz', '#', 'abcdef'))
except Exception as e:
    print('SKIP_subn', type(e).__name__, e)

# === split ===
try:
    print('split_basic', re.split(r'\d+', 'abc123def456ghi'))
    print('split_maxsplit', re.split(r'\d+', 'abc1def2ghi3jkl', maxsplit=2))
    print('split_groups', re.split(r'(\d+)', 'abc123def'))
    print('split_empty', re.split(r'\d+', 'abcdef'))
except Exception as e:
    print('SKIP_split', type(e).__name__, e)

# === compile ===
try:
    print('compile_basic', re.compile(r'\d+').pattern)
    print('compile_flags', re.compile(r'[a-z]', re.IGNORECASE).flags)
    print('compile_groups', re.compile(r'(\d)(\w)').groups)
    print('compile_groupindex', re.compile(r'(?P<num>\d)').groupindex)
except Exception as e:
    print('SKIP_compile', type(e).__name__, e)

# === escape ===
try:
    print('escape_basic', re.escape(r'.*+?^${}()[]|\\'))
    print('escape_alnum', re.escape('abc123'))
    print('escape_mixed', re.escape('a.b*c?'))
except Exception as e:
    print('SKIP_escape', type(e).__name__, e)

# === purge ===
try:
    re.purge()
    print('purge_done', 'ok')
except Exception as e:
    print('SKIP_purge', type(e).__name__, e)

# === error ===
try:
    try:
        re.compile(r'[invalid')
    except re.error as e:
        print('error_basic', str(e))
        print('error_pos', e.pos if hasattr(e, 'pos') else 'none')
        print('error_lineno', e.lineno if hasattr(e, 'lineno') else 'none')
        print('error_colno', e.colno if hasattr(e, 'colno') else 'none')
except Exception as e:
    print('SKIP_error', type(e).__name__, e)

# === PatternError ===
try:
    try:
        re.compile(r'(?P<dup>\d)(?P<dup>\w)')
    except re.PatternError as e:
        print('pattern_error_caught', 'ok')
except Exception as e:
    print('SKIP_PatternError', type(e).__name__, e)

# === Flags: ASCII ===
try:
    print('flag_ascii', re.ASCII)
    print('flag_a', re.A)
    print('flag_ascii_match', re.match(r'\w+', 'café', re.ASCII).group() if re.match(r'\w+', 'café', re.ASCII) else None)
except Exception as e:
    print('SKIP_Flags: ASCII', type(e).__name__, e)

# === Flags: IGNORECASE ===
try:
    print('flag_ignorecase', re.IGNORECASE)
    print('flag_i', re.I)
    print('flag_ignorecase_match', re.match(r'abc', 'ABC', re.I).group())
except Exception as e:
    print('SKIP_Flags: IGNORECASE', type(e).__name__, e)

# === Flags: MULTILINE ===
try:
    print('flag_multiline', re.MULTILINE)
    print('flag_m', re.M)
    text = 'line1\nline2'
    print('flag_multiline_caret', re.findall(r'^\w+', text, re.M))
    print('flag_multiline_dollar', re.findall(r'\w+$', text, re.M))
except Exception as e:
    print('SKIP_Flags: MULTILINE', type(e).__name__, e)

# === Flags: DOTALL ===
try:
    print('flag_dotall', re.DOTALL)
    print('flag_s', re.S)
    print('flag_dotall_match', re.match(r'a.b', 'a\nb', re.S).group())
except Exception as e:
    print('SKIP_Flags: DOTALL', type(e).__name__, e)

# === Flags: VERBOSE ===
try:
    print('flag_verbose', re.VERBOSE)
    print('flag_x', re.X)
    pattern = r'''
        \d+     # digits
        \.      # dot
        \d+     # more digits
    '''
    print('flag_verbose_match', re.match(pattern, '123.456', re.X).group())
except Exception as e:
    print('SKIP_Flags: VERBOSE', type(e).__name__, e)

# === Flags: UNICODE ===
try:
    print('flag_unicode', re.UNICODE)
    print('flag_u', re.U)
    print('flag_unicode_match', re.match(r'\w+', 'café').group())
except Exception as e:
    print('SKIP_Flags: UNICODE', type(e).__name__, e)

# === Flags: LOCALE ===
try:
    print('flag_locale', re.LOCALE)
    print('flag_l', re.L)
except Exception as e:
    print('SKIP_Flags: LOCALE', type(e).__name__, e)

# === Flags: DEBUG ===
try:
    print('flag_debug', re.DEBUG)
except Exception as e:
    print('SKIP_Flags: DEBUG', type(e).__name__, e)

# === Flags: NOFLAG ===
try:
    print('flag_noflag', re.NOFLAG)
except Exception as e:
    print('SKIP_Flags: NOFLAG', type(e).__name__, e)

# === Flags: Combined ===
try:
    print('flag_combined', re.IGNORECASE | re.MULTILINE)
    print('flag_combined_match', re.findall(r'^\w+', 'ABC\nDEF', re.I | re.M))
except Exception as e:
    print('SKIP_Flags: Combined', type(e).__name__, e)

# === RegexFlag enum ===
try:
    print('regexflag_type', type(re.RegexFlag.ASCII))
    print('regexflag_ascii', re.RegexFlag.ASCII)
    print('regexflag_ignorecase', re.RegexFlag.IGNORECASE)
except Exception as e:
    print('SKIP_RegexFlag enum', type(e).__name__, e)

# === Pattern methods: match ===
try:
    p = re.compile(r'\d+')
    print('pattern_match_basic', p.match('123abc').group())
    print('pattern_match_none', p.match('abc123'))
    print('pattern_match_pos', p.match('abc123', pos=3).group())
except Exception as e:
    print('SKIP_Pattern methods: match', type(e).__name__, e)

# === Pattern methods: search ===
try:
    p = re.compile(r'\d+')
    print('pattern_search_basic', p.search('abc123def').group())
    print('pattern_search_none', p.search('abcdef'))
except Exception as e:
    print('SKIP_Pattern methods: search', type(e).__name__, e)

# === Pattern methods: fullmatch ===
try:
    p = re.compile(r'\d+')
    print('pattern_fullmatch_basic', p.fullmatch('123').group())
    print('pattern_fullmatch_none', p.fullmatch('123abc'))
except Exception as e:
    print('SKIP_Pattern methods: fullmatch', type(e).__name__, e)

# === Pattern methods: findall ===
try:
    p = re.compile(r'\d+')
    print('pattern_findall', p.findall('abc123def456'))
except Exception as e:
    print('SKIP_Pattern methods: findall', type(e).__name__, e)

# === Pattern methods: finditer ===
try:
    p = re.compile(r'\d+')
    print('pattern_finditer', [m.group() for m in p.finditer('abc123def456')])
except Exception as e:
    print('SKIP_Pattern methods: finditer', type(e).__name__, e)

# === Pattern methods: sub ===
try:
    p = re.compile(r'\d+')
    print('pattern_sub', p.sub('#', 'abc123def456'))
except Exception as e:
    print('SKIP_Pattern methods: sub', type(e).__name__, e)

# === Pattern methods: subn ===
try:
    p = re.compile(r'\d+')
    print('pattern_subn', p.subn('#', 'abc123def456'))
except Exception as e:
    print('SKIP_Pattern methods: subn', type(e).__name__, e)

# === Pattern methods: split ===
try:
    p = re.compile(r'\d+')
    print('pattern_split', p.split('abc123def456ghi'))
except Exception as e:
    print('SKIP_Pattern methods: split', type(e).__name__, e)

# === Pattern methods: pattern ===
try:
    p = re.compile(r'\d+')
    print('pattern_pattern', p.pattern)
except Exception as e:
    print('SKIP_Pattern methods: pattern', type(e).__name__, e)

# === Pattern methods: flags ===
try:
    print('pattern_flags_default', re.compile(r'\d+').flags)
    print('pattern_flags_ignorecase', re.compile(r'\d+', re.I).flags)
except Exception as e:
    print('SKIP_Pattern methods: flags', type(e).__name__, e)

# === Pattern methods: groups ===
try:
    print('pattern_groups_none', re.compile(r'\d+').groups)
    print('pattern_groups_some', re.compile(r'(\d)(\w)').groups)
except Exception as e:
    print('SKIP_Pattern methods: groups', type(e).__name__, e)

# === Pattern methods: groupindex ===
try:
    print('pattern_groupindex_none', re.compile(r'\d+').groupindex)
    print('pattern_groupindex_some', re.compile(r'(?P<num>\d)(?P<letter>\w)').groupindex)
except Exception as e:
    print('SKIP_Pattern methods: groupindex', type(e).__name__, e)

# === Pattern methods: scanner ===
try:
    p = re.compile(r'\d+')
    scanner = p.scanner('123abc456')
    print('pattern_scanner', type(scanner).__name__)
except Exception as e:
    print('SKIP_Pattern methods: scanner', type(e).__name__, e)

# === Match methods: group ===
try:
    m = re.match(r'(\d)(\w)', '1a')
    print('match_group_0', m.group(0))
    print('match_group_1', m.group(1))
    print('match_group_2', m.group(2))
    print('match_group_multi', m.group(1, 2))
    print('match_group_name', re.match(r'(?P<num>\d)', '1').group('num'))
except Exception as e:
    print('SKIP_Match methods: group', type(e).__name__, e)

# === Match methods: groups ===
try:
    m = re.match(r'(\d)(\w)', '1a')
    print('match_groups', m.groups())
    m = re.match(r'(\d)(\w)?', '1')
    print('match_groups_default', m.groups())
    print('match_groups_default_value', m.groups(default='X'))
except Exception as e:
    print('SKIP_Match methods: groups', type(e).__name__, e)

# === Match methods: groupdict ===
try:
    m = re.match(r'(?P<num>\d)(?P<letter>\w)', '1a')
    print('match_groupdict', m.groupdict())
    m = re.match(r'(?P<num>\d)(?P<letter>\w)?', '1')
    print('match_groupdict_default', m.groupdict(default='N/A'))
except Exception as e:
    print('SKIP_Match methods: groupdict', type(e).__name__, e)

# === Match methods: start ===
try:
    m = re.match(r'(\d)(\w)', '1a23')
    print('match_start_0', m.start())
    print('match_start_1', m.start(1))
    print('match_start_2', m.start(2))
except Exception as e:
    print('SKIP_Match methods: start', type(e).__name__, e)

# === Match methods: end ===
try:
    print('match_end_0', m.end())
    print('match_end_1', m.end(1))
    print('match_end_2', m.end(2))
except Exception as e:
    print('SKIP_Match methods: end', type(e).__name__, e)

# === Match methods: span ===
try:
    print('match_span_0', m.span())
    print('match_span_1', m.span(1))
    print('match_span_2', m.span(2))
except Exception as e:
    print('SKIP_Match methods: span', type(e).__name__, e)

# === Match methods: pos ===
try:
    print('match_pos', m.pos)
except Exception as e:
    print('SKIP_Match methods: pos', type(e).__name__, e)

# === Match methods: endpos ===
try:
    print('match_endpos', m.endpos)
except Exception as e:
    print('SKIP_Match methods: endpos', type(e).__name__, e)

# === Match methods: re ===
try:
    print('match_re', m.re.pattern)
except Exception as e:
    print('SKIP_Match methods: re', type(e).__name__, e)

# === Match methods: string ===
try:
    print('match_string', m.string)
except Exception as e:
    print('SKIP_Match methods: string', type(e).__name__, e)

# === Match methods: expand ===
try:
    m = re.match(r'(\d)(\w)', '1a')
    print('match_expand', m.expand(r'\2-\1'))
    print('match_expand_named', re.match(r'(?P<n>\d)', '1').expand(r'\g<n>'))
except Exception as e:
    print('SKIP_Match methods: expand', type(e).__name__, e)

# === Match methods: lastgroup ===
try:
    print('match_lastgroup', re.match(r'(?P<num>\d)', '1').lastgroup)
    print('match_lastgroup_none', re.match(r'\d', '1').lastgroup)
except Exception as e:
    print('SKIP_Match methods: lastgroup', type(e).__name__, e)

# === Match methods: lastindex ===
try:
    print('match_lastindex', re.match(r'(\d)(\w)', '1a').lastindex)
    print('match_lastindex_none', re.match(r'\d', '1').lastindex)
except Exception as e:
    print('SKIP_Match methods: lastindex', type(e).__name__, e)

# === Match methods: regs ===
try:
    m = re.match(r'(\d)(\w)', '1a')
    print('match_regs', m.regs)
except Exception as e:
    print('SKIP_Match methods: regs', type(e).__name__, e)

# === Special regex features ===
try:
    # Named groups
    print('special_named_group', re.match(r'(?P<name>\w+)', 'hello').group('name'))
    # Non-capturing groups
    print('special_noncapture', re.match(r'(?:\d+)(\w)', '123a').groups())
    # Lookahead
    print('special_lookahead', re.findall(r'\w+(?=\d)', 'abc123'))
    # Negative lookahead
    print('special_neg_lookahead', re.findall(r'\w+(?!\d)', 'abc 123'))
    # Lookbehind
    print('special_lookbehind', re.findall(r'(?<=\d)\w+', '123abc'))
    # Negative lookbehind
    print('special_neg_lookbehind', re.findall(r'(?<!\d)\w+', 'abc123'))
    # Word boundaries
    print('special_word_boundary', re.findall(r'\b\w+\b', 'hello world'))
    # Anchors
    print('special_anchor_start', re.match(r'^abc', 'abc').group())
    print('special_anchor_end', re.search(r'abc$', 'abc').group())

    # Greedy vs non-greedy
    print('special_greedy', re.match(r'<.+>', '<a>b<c>').group())
    print('special_nongreedy', re.match(r'<.+?>', '<a>b<c>').group())

    # Possessive quantifiers (Python 3.11+)
    try:
        print('special_possessive', re.match(r'a*+a', 'aaaa'))
    except:
        pass  # May not be available

    # Atomic groups
    try:
        print('special_atomic', re.match(r'(?>a*)a', 'aaaa'))
    except:
        pass  # May not be available

    # Conditional patterns
    print('special_conditional', re.match(r'(\d)?(?(1)\w|\s)', '1a').group())

    # Comments in verbose mode
    pattern = r'''
        \d+      # digits
        (        # group start
           \.    # decimal point
           \d+   # fraction
        )?       # optional
    '''
    print('special_verbose', re.match(pattern, '123.45', re.X).group())

    # Backreferences
    print('special_backref', re.match(r'(\w)\1', 'aa').group())
    print('special_backref_named', re.match(r'(?P<x>\w)(?P=x)', 'bb').group())

    # Character classes
    print('special_charclass_digit', re.findall(r'\d+', 'abc123'))
    print('special_charclass_word', re.findall(r'\w+', 'hello_world'))
    print('special_charclass_space', re.findall(r'\s+', 'a  b\tc'))
    print('special_charclass_nondigit', re.findall(r'\D+', '123abc'))
    print('special_charclass_nonword', re.findall(r'\W+', 'hello@world'))
    print('special_charclass_nonspace', re.findall(r'\S+', 'a b c'))

    # Character sets
    print('special_charset', re.findall(r'[aeiou]+', 'hello'))
    print('special_charset_neg', re.findall(r'[^aeiou]+', 'hello'))
    print('special_charset_range', re.findall(r'[a-z]+', 'hello'))
    print('special_charset_range2', re.findall(r'[0-9]+', '123'))

    # Alternation
    print('special_alternation', re.findall(r'cat|dog', 'I have a cat and a dog'))

    # Optional, zero or more, one or more
    print('special_optional', re.match(r'colou?r', 'color').group())
    print('special_optional2', re.match(r'colou?r', 'colour').group())
    print('special_zeromore', re.match(r'a*b', 'aaab').group())
    print('special_zeromore2', re.match(r'a*b', 'b').group())
    print('special_onemore', re.match(r'a+b', 'aaab').group())

    # Exact count and ranges
    print('special_exact', re.match(r'a{3}', 'aaa').group())
    print('special_range', re.match(r'a{2,4}', 'aaaa').group())
    print('special_range_min', re.match(r'a{2,}', 'aaaa').group())
except Exception as e:
    print('SKIP_Special regex features', type(e).__name__, e)

# === Special characters in strings ===
try:
    print('special_newline', re.match(r'a\nb', 'a\nb').group())
    print('special_tab', re.match(r'a\tb', 'a\tb').group())
    print('special_literal_dot', re.match(r'a\.b', 'a.b').group())
    print('special_literal_star', re.match(r'a\*b', 'a*b').group())
    print('special_literal_plus', re.match(r'a\+b', 'a+b').group())
    print('special_literal_question', re.match(r'a\?b', 'a?b').group())
    print('special_literal_bracket', re.match(r'a\[b', 'a[b').group())
    print('special_literal_paren', re.match(r'a\(b', 'a(b').group())
    print('special_literal_brace', re.match(r'a\{b', 'a{b').group())
    print('special_literal_backslash', re.match(r'a\\b', 'a\\b').group())
    print('special_literal_caret', re.match(r'a\^b', 'a^b').group())
    print('special_literal_dollar', re.match(r'a\$b', 'a$b').group())
    print('special_literal_pipe', re.match(r'a\|b', 'a|b').group())
except Exception as e:
    print('SKIP_Special characters in strings', type(e).__name__, e)

# === Empty matches ===
try:
    print('empty_match_start', re.match(r'^', 'abc').group())
    print('empty_match_end', re.search(r'$', 'abc').group())
    print('empty_match_word', re.findall(r'\b', 'a b c'))
except Exception as e:
    print('SKIP_Empty matches', type(e).__name__, e)

# === Bytes patterns ===
try:
    print('bytes_pattern', re.match(rb'\d+', b'123').group())
    print('bytes_search', re.search(rb'\w+', b'hello').group())
    print('bytes_findall', re.findall(rb'\d+', b'123abc456'))
    print('bytes_sub', re.sub(rb'\d+', b'#', b'123abc'))
except Exception as e:
    print('SKIP_Bytes patterns', type(e).__name__, e)

# === Scanner class ===
try:
    scanner = re.Scanner([
        (r'\d+', lambda s, tok: ('NUMBER', tok)),
        (r'[a-zA-Z]+', lambda s, tok: ('WORD', tok)),
        (r'\s+', None),
    ])
    result, remainder = scanner.scan('123 hello 456 world')
    print('scanner_result', result)
    print('scanner_remainder', remainder)
except Exception as e:
    print('SKIP_Scanner class', type(e).__name__, e)

# === Complex patterns ===
try:
    # Email-like pattern
    email_pattern = r'[\w.+-]+@[\w.-]+\.[a-zA-Z]{2,}'
    print('complex_email', re.findall(email_pattern, 'Contact me at test@example.com or foo@bar.org'))

    # URL-like pattern
    url_pattern = r'https?://[^\s]+'
    print('complex_url', re.findall(url_pattern, 'Visit https://example.com or http://test.org'))

    # Phone number pattern
    phone_pattern = r'\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}'
    print('complex_phone', re.findall(phone_pattern, 'Call (123) 456-7890 or 123.456.7890'))

    # Date pattern
    date_pattern = r'\d{4}-\d{2}-\d{2}'
    print('complex_date', re.findall(date_pattern, 'Date: 2024-01-15 or 2023-12-25'))
except Exception as e:
    print('SKIP_Complex patterns', type(e).__name__, e)

# === Edge cases ===
try:
    # Empty string
    print('edge_empty_pattern', re.findall(r'', 'abc'))
    print('edge_empty_string', re.findall(r'\d+', ''))

    # Very long match
    long_text = 'a' * 1000
    print('edge_long_match', len(re.match(r'a+', long_text).group()))

    # Nested groups
    m = re.match(r'((\d)(\w))', '1a')
    print('edge_nested_groups', m.groups())

    # Overlapping patterns
    print('edge_overlapping', re.findall(r'(?=\d\d\d)', '12345'))

    # Multiple matches with same pattern
    print('edge_multiple', re.findall(r'\w+', 'hello world foo bar'))

    # Unicode characters
    print('edge_unicode', re.match(r'\w+', 'héllo café').group())
    print('edge_emoji', re.findall(r'.', '😀😁😂'))

    # Case folding
    print('edge_casefold', re.sub(r'ss', 'ß', 'mass').lower())
    print('edge_casefold_match', re.match(r'ß', 'ss', re.IGNORECASE))

    # Maximum recursion depth protection (test with reasonable depth)
    print('edge_deep_nesting', re.match(r'(' * 10 + r'a' + r')' * 10, 'a').group())
except Exception as e:
    print('SKIP_Edge cases', type(e).__name__, e)

# === Template substitution ===
try:
    print('template_basic', re.sub(r'(\w+) (\w+)', r'\2, \1', 'John Smith'))
    print('template_named', re.sub(r'(?P<first>\w+) (?P<last>\w+)', r'\g<last>, \g<first>', 'John Smith'))
    print('template_escaped', re.sub(r'(\w+)', r'\\1', 'test'))
    print('template_literal_g', re.sub(r'(\w+)', r'\g<0>', 'test'))
except Exception as e:
    print('SKIP_Template substitution', type(e).__name__, e)

# === Match object comparisons ===
try:
    m1 = re.match(r'\d+', '123')
    m2 = re.match(r'\d+', '123')
    print('match_eq_same', m1 == m2)
    m3 = re.match(r'\d+', '456')
    print('match_eq_diff', m1 == m3)
except Exception as e:
    print('SKIP_Match object comparisons', type(e).__name__, e)

# === Pattern object comparisons ===
try:
    p1 = re.compile(r'\d+')
    p2 = re.compile(r'\d+')
    p3 = re.compile(r'\w+')
    print('pattern_eq_same', p1 == p2)
    print('pattern_eq_diff', p1 == p3)
    print('pattern_eq_same_flags', re.compile(r'\d+', re.I) == re.compile(r'\d+', re.I))
    print('pattern_eq_diff_flags', re.compile(r'\d+') == re.compile(r'\d+', re.I))
except Exception as e:
    print('SKIP_Pattern object comparisons', type(e).__name__, e)

# === Pattern hash ===
try:
    print('pattern_hash', hash(p1))
except Exception as e:
    print('SKIP_Pattern hash', type(e).__name__, e)

# === Copy/deepcopy ===
try:
    import copy
    p = re.compile(r'\d+')
    print('pattern_copy', copy.copy(p).pattern)
    print('pattern_deepcopy', copy.deepcopy(p).pattern)
except Exception as e:
    print('SKIP_Copy/deepcopy', type(e).__name__, e)

# === Pickle ===
try:
    import pickle
    p = re.compile(r'\d+')
    pickled = pickle.dumps(p)
    unpickled = pickle.loads(pickled)
    print('pattern_pickle', unpickled.pattern)
    print('pattern_pickle_match', unpickled.match('123').group())
except Exception as e:
    print('SKIP_Pickle', type(e).__name__, e)

# === Regex caching ===
try:
    re.purge()
    # This should add to cache
    p1 = re.compile(r'\d+')
    p2 = re.compile(r'\d+')  # Should be from cache
    print('cache_test', p1.pattern == p2.pattern)
except Exception as e:
    print('SKIP_Regex caching', type(e).__name__, e)
