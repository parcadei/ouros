import textwrap

# === wrap ===
try:
    text = "This is a long text that needs wrapping into multiple lines."
    print('wrap_basic', textwrap.wrap(text, width=20))
    print('wrap_default_width', textwrap.wrap(text))
    print('wrap_narrow', textwrap.wrap(text, width=10))
    print('wrap_wide', textwrap.wrap(text, width=100))
    print('wrap_empty', textwrap.wrap(''))
    print('wrap_single_word', textwrap.wrap('Hello'))
    print('wrap_long_word', textwrap.wrap('supercalifragilisticexpialidocious', width=10))
    print('wrap_multiple_paragraphs', textwrap.wrap('Hello world.\n\nThis is another paragraph.', width=20))
    print('wrap_with_initial_indent', textwrap.wrap(text, width=20, initial_indent='  '))
    print('wrap_with_subsequent_indent', textwrap.wrap(text, width=20, subsequent_indent='  '))
    print('wrap_with_both_indents', textwrap.wrap(text, width=20, initial_indent='* ', subsequent_indent='  '))
    print('wrap_expand_tabs_true', textwrap.wrap('Hello\tworld', width=20, expand_tabs=True))
    print('wrap_expand_tabs_false', textwrap.wrap('Hello\tworld', width=20, expand_tabs=False))
    print('wrap_replace_whitespace_true', textwrap.wrap('Hello\nworld\ttest', width=20, replace_whitespace=True))
    print('wrap_replace_whitespace_false', textwrap.wrap('Hello\nworld\ttest', width=20, replace_whitespace=False))
    print('wrap_fix_sentence_endings_true', textwrap.wrap('Hello. World. Test.', width=20, fix_sentence_endings=True))
    print('wrap_break_long_words_true', textwrap.wrap('supercalifragilistic', width=10, break_long_words=True))
    print('wrap_break_long_words_false', textwrap.wrap('supercalifragilistic', width=10, break_long_words=False))
    print('wrap_drop_whitespace_true', textwrap.wrap('  hello   world  ', width=20, drop_whitespace=True))
    print('wrap_drop_whitespace_false', textwrap.wrap('  hello   world  ', width=20, drop_whitespace=False))
    print('wrap_break_on_hyphens_true', textwrap.wrap('hyphenated-word-test', width=10, break_on_hyphens=True))
    print('wrap_break_on_hyphens_false', textwrap.wrap('hyphenated-word-test', width=10, break_on_hyphens=False))
    print('wrap_tabsize', textwrap.wrap('Hello\tworld', width=20, tabsize=4))
    print('wrap_max_lines', textwrap.wrap(text, width=20, max_lines=2))
    print('wrap_max_lines_with_placeholder', textwrap.wrap(text, width=20, max_lines=2, placeholder='...'))
    print('wrap_placeholder', textwrap.wrap(text, width=20, max_lines=1, placeholder='[more]'))
except Exception as e:
    print('SKIP_wrap', type(e).__name__, e)

# === fill ===
try:
    text = "This is a long text that needs wrapping into multiple lines."
    print('fill_basic', textwrap.fill(text, width=20))
    print('fill_default_width', textwrap.fill(text))
    print('fill_narrow', textwrap.fill(text, width=10))
    print('fill_empty', textwrap.fill(''))
    print('fill_single_word', textwrap.fill('Hello'))
    print('fill_with_initial_indent', textwrap.fill(text, width=20, initial_indent='  '))
    print('fill_with_subsequent_indent', textwrap.fill(text, width=20, subsequent_indent='  '))
    print('fill_with_both_indents', textwrap.fill(text, width=20, initial_indent='* ', subsequent_indent='  '))
    print('fill_max_lines', textwrap.fill(text, width=20, max_lines=2))
    print('fill_max_lines_placeholder', textwrap.fill(text, width=20, max_lines=2, placeholder='...'))
except Exception as e:
    print('SKIP_fill', type(e).__name__, e)

# === shorten ===
try:
    print('shorten_basic', textwrap.shorten('Hello  world!', width=12))
    print('shorten_truncate', textwrap.shorten('Hello  world!', width=11))
    print('shorten_custom_placeholder', textwrap.shorten('Hello world', width=10, placeholder='...'))
    print('shorten_fits', textwrap.shorten('Hello', width=10))
    print('shorten_exact', textwrap.shorten('Hello world', width=11))
    print('shorten_long', textwrap.shorten('This is a very long text that needs shortening', width=20))
    print('shorten_single_word', textwrap.shorten('supercalifragilistic', width=10))
    print('shorten_break_long_words', textwrap.shorten('supercalifragilistic', width=10, break_long_words=True))
    print('shorten_no_break_long_words', textwrap.shorten('supercalifragilistic', width=10, break_long_words=False))
    print('shorten_break_on_hyphens', textwrap.shorten('well-known phrase here', width=15, break_on_hyphens=True))
    print('shorten_no_break_on_hyphens', textwrap.shorten('well-known phrase here', width=15, break_on_hyphens=False))
    print('shorten_custom_placeholder_long', textwrap.shorten('Hello world this is long', width=15, placeholder='[...]'))
except Exception as e:
    print('SKIP_shorten', type(e).__name__, e)

# === dedent ===
try:
    print('dedent_basic', textwrap.dedent('    hello\n    world'))
    print('dedent_partial', textwrap.dedent('  hello\n    world'))
    print('dedent_mixed', textwrap.dedent('\thello\n    world'))
    print('dedent_empty', textwrap.dedent(''))
    print('dedent_no_common', textwrap.dedent('hello\n  world'))
    print('dedent_blank_lines', textwrap.dedent('    hello\n\n    world'))
    print('dedent_whitespace_only', textwrap.dedent('    hello\n   \n    world'))
    print('dedent_single_line', textwrap.dedent('    hello'))
    print('dedent_already_left', textwrap.dedent('hello\nworld'))
    print('dedent_tabs', textwrap.dedent('\thello\n\tworld'))
    print('dedent_mixed_tabs_spaces', textwrap.dedent('    hello\n\tworld'))
except Exception as e:
    print('SKIP_dedent', type(e).__name__, e)

# === indent ===
try:
    print('indent_basic', textwrap.indent('hello\nworld', '  '))
    print('indent_empty', textwrap.indent('', '  '))
    print('indent_single', textwrap.indent('hello', '  '))
    print('indent_empty_lines', textwrap.indent('hello\n\nworld', '  '))
    print('indent_whitespace_lines', textwrap.indent('hello\n  \nworld', '  '))
    print('indent_prefix_string', textwrap.indent('hello\nworld', '> '))
    print('indent_all_lines', textwrap.indent('hello\n\nworld', '+ ', lambda line: True))
    print('indent_no_lines', textwrap.indent('hello\nworld', '+ ', lambda line: False))
    print('indent_even_lines', textwrap.indent('a\nb\nc\nd', '  ', lambda line: len(line) == 1 and line in 'bd'))
    print('indent_multichar_prefix', textwrap.indent('hello\nworld', '----'))
    print('indent_with_tabs', textwrap.indent('hello\nworld', '\t'))
except Exception as e:
    print('SKIP_indent', type(e).__name__, e)

# === TextWrapper class ===
try:
    text = "This is a long text that needs wrapping into multiple lines."

    # TextWrapper basic usage
    tw = textwrap.TextWrapper()
    print('tw_default_wrap', tw.wrap('Hello world this is a test'))
    print('tw_default_fill', tw.fill('Hello world this is a test'))

    # TextWrapper with custom width
    tw_width = textwrap.TextWrapper(width=20)
    print('tw_width_20_wrap', tw_width.wrap(text))
    print('tw_width_20_fill', tw_width.fill(text))

    # TextWrapper with initial_indent
    tw_init = textwrap.TextWrapper(width=30, initial_indent='* ')
    print('tw_initial_indent_wrap', tw_init.wrap(text))
    print('tw_initial_indent_fill', tw_init.fill(text))

    # TextWrapper with subsequent_indent
    tw_sub = textwrap.TextWrapper(width=30, subsequent_indent='  ')
    print('tw_subsequent_indent_wrap', tw_sub.wrap(text))
    print('tw_subsequent_indent_fill', tw_sub.fill(text))

    # TextWrapper with both indents
    tw_both = textwrap.TextWrapper(width=30, initial_indent='>> ', subsequent_indent='   ')
    print('tw_both_indents_wrap', tw_both.wrap(text))
    print('tw_both_indents_fill', tw_both.fill(text))

    # TextWrapper expand_tabs
    tw_expand = textwrap.TextWrapper(width=20, expand_tabs=True, tabsize=4)
    print('tw_expand_tabs', tw_expand.wrap('Col1\tCol2\tCol3'))
    tw_no_expand = textwrap.TextWrapper(width=20, expand_tabs=False)
    print('tw_no_expand_tabs', tw_no_expand.wrap('Col1\tCol2\tCol3'))

    # TextWrapper tabsize
    tw_tab4 = textwrap.TextWrapper(width=30, expand_tabs=True, tabsize=4)
    print('tw_tabsize_4', tw_tab4.wrap('a\tb\tc'))
    tw_tab8 = textwrap.TextWrapper(width=30, expand_tabs=True, tabsize=8)
    print('tw_tabsize_8', tw_tab8.wrap('a\tb\tc'))

    # TextWrapper replace_whitespace
    tw_replace = textwrap.TextWrapper(width=20, replace_whitespace=True)
    print('tw_replace_whitespace', tw_replace.wrap('Line1\nLine2\tTab'))
    tw_no_replace = textwrap.TextWrapper(width=20, replace_whitespace=False)
    print('tw_no_replace_whitespace', tw_no_replace.wrap('Line1\nLine2\tTab'))

    # TextWrapper fix_sentence_endings
    tw_fix = textwrap.TextWrapper(width=30, fix_sentence_endings=True)
    print('tw_fix_sentence_endings', tw_fix.wrap('Hello. World. Test. Here.'))
    tw_no_fix = textwrap.TextWrapper(width=30, fix_sentence_endings=False)
    print('tw_no_fix_sentence_endings', tw_no_fix.wrap('Hello. World. Test. Here.'))

    # TextWrapper break_long_words
    tw_break = textwrap.TextWrapper(width=10, break_long_words=True)
    print('tw_break_long_words_true', tw_break.wrap('supercalifragilistic'))
    tw_no_break = textwrap.TextWrapper(width=10, break_long_words=False)
    print('tw_break_long_words_false', tw_no_break.wrap('supercalifragilistic'))

    # TextWrapper drop_whitespace
    tw_drop = textwrap.TextWrapper(width=20, drop_whitespace=True)
    print('tw_drop_whitespace_true', tw_drop.wrap('  hello   world  '))
    tw_no_drop = textwrap.TextWrapper(width=20, drop_whitespace=False)
    print('tw_drop_whitespace_false', tw_no_drop.wrap('  hello   world  '))

    # TextWrapper break_on_hyphens
    tw_hyphen = textwrap.TextWrapper(width=10, break_on_hyphens=True)
    print('tw_break_on_hyphens_true', tw_hyphen.wrap('hyphenated-word'))
    tw_no_hyphen = textwrap.TextWrapper(width=10, break_on_hyphens=False)
    print('tw_break_on_hyphens_false', tw_no_hyphen.wrap('hyphenated-word'))

    # TextWrapper max_lines
    tw_max2 = textwrap.TextWrapper(width=20, max_lines=2)
    print('tw_max_lines_2_wrap', tw_max2.wrap(text))
    print('tw_max_lines_2_fill', tw_max2.fill(text))

    # TextWrapper max_lines with placeholder
    tw_max2_ph = textwrap.TextWrapper(width=20, max_lines=2, placeholder='...')
    print('tw_max_lines_placeholder', tw_max2_ph.wrap(text))
    print('tw_max_lines_placeholder_fill', tw_max2_ph.fill(text))

    # TextWrapper custom placeholder
    tw_custom_ph = textwrap.TextWrapper(width=20, max_lines=1, placeholder=' [more]')
    print('tw_custom_placeholder', tw_custom_ph.wrap(text))

    # TextWrapper attribute access
    tw_attrs = textwrap.TextWrapper()
    print('tw_attr_width', tw_attrs.width)
    print('tw_attr_initial_indent', repr(tw_attrs.initial_indent))
    print('tw_attr_subsequent_indent', repr(tw_attrs.subsequent_indent))
    print('tw_attr_expand_tabs', tw_attrs.expand_tabs)
    print('tw_attr_replace_whitespace', tw_attrs.replace_whitespace)
    print('tw_attr_fix_sentence_endings', tw_attrs.fix_sentence_endings)
    print('tw_attr_break_long_words', tw_attrs.break_long_words)
    print('tw_attr_drop_whitespace', tw_attrs.drop_whitespace)
    print('tw_attr_break_on_hyphens', tw_attrs.break_on_hyphens)
    print('tw_attr_tabsize', tw_attrs.tabsize)
    print('tw_attr_max_lines', tw_attrs.max_lines)
    print('tw_attr_placeholder', repr(tw_attrs.placeholder))

    # TextWrapper attribute modification
    tw_mod = textwrap.TextWrapper()
    tw_mod.width = 10
    tw_mod.initial_indent = '> '
    tw_mod.subsequent_indent = '  '
    tw_mod.expand_tabs = False
    tw_mod.replace_whitespace = False
    tw_mod.fix_sentence_endings = True
    tw_mod.break_long_words = False
    tw_mod.drop_whitespace = False
    tw_mod.break_on_hyphens = False
    tw_mod.tabsize = 4
    tw_mod.max_lines = 1
    tw_mod.placeholder = '...'
    print('tw_modified_wrap', tw_mod.wrap('Hello world'))
    print('tw_modified_fill', tw_mod.fill('Hello world'))

    # TextWrapper edge cases
    print('tw_empty_wrap', textwrap.TextWrapper().wrap(''))
    print('tw_empty_fill', textwrap.TextWrapper().fill(''))
    print('tw_whitespace_only_wrap', textwrap.TextWrapper().wrap('   \n\t  '))
    print('tw_newlines_only_wrap', textwrap.TextWrapper().wrap('\n\n\n'))
    print('tw_width_1', textwrap.TextWrapper(width=1).wrap('a b c'))
    print('tw_width_exact', textwrap.TextWrapper(width=5).wrap('hello'))
    print('tw_long_single_word', textwrap.TextWrapper(width=5).wrap('supercalifragilistic'))

    # TextWrapper reusing instance
    tw_reuse = textwrap.TextWrapper(width=20)
    result1 = tw_reuse.wrap('First text to wrap here')
    tw_reuse.width = 30
    tw_reuse.initial_indent = '* '
    result2 = tw_reuse.wrap('Second text to wrap here')
    print('tw_reuse_first', result1)
    print('tw_reuse_second', result2)

    # Combined kwargs test
    print('wrap_all_kwargs', textwrap.wrap(
        'Test text with\ttabs and\nnewlines and verylongwordhere',
        width=15,
        initial_indent='> ',
        subsequent_indent='  ',
        expand_tabs=True,
        replace_whitespace=True,
        fix_sentence_endings=True,
        break_long_words=True,
        drop_whitespace=True,
        break_on_hyphens=True,
        tabsize=4,
        max_lines=2,
        placeholder='...'
    ))

    print('fill_all_kwargs', textwrap.fill(
        'Test text with\ttabs and\nnewlines and verylongwordhere',
        width=15,
        initial_indent='> ',
        subsequent_indent='  ',
        expand_tabs=True,
        replace_whitespace=True,
        fix_sentence_endings=True,
        break_long_words=True,
        drop_whitespace=True,
        break_on_hyphens=True,
        tabsize=4,
        max_lines=2,
        placeholder='...'
    ))

    # Special characters and unicode
    print('wrap_unicode', textwrap.wrap('Héllo wörld ñoël', width=10))
    print('wrap_em_dash', textwrap.wrap('word--word--test', width=10))
    print('wrap_punctuation', textwrap.wrap('Hello, world! How are you?', width=15))
    print('wrap_quotes', textwrap.wrap('"Hello world" test', width=15))
    print('wrap_apostrophe', textwrap.wrap("It's a test", width=10))

    # Indent with special predicate
    print('indent_nonempty', textwrap.indent('line1\n\nline2', '+ ', predicate=lambda line: line.strip()))
    print('indent_starts_with', textwrap.indent('apple\nbanana\napricot', '- ', predicate=lambda line: line.startswith('a')))

    # Shorten edge cases
    print('shorten_exact_fit', textwrap.shorten('Hello world', width=11, placeholder=''))
    print('shorten_no_placeholder', textwrap.shorten('Hello world test', width=10, placeholder=''))
    print('shorten_just_placeholder', textwrap.shorten('Hello world', width=5, placeholder='[...]'))
except Exception as e:
    print('SKIP_TextWrapper class', type(e).__name__, e)
