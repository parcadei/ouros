import string

# === Constants ===
try:
    print('ascii_letters', string.ascii_letters)
    print('ascii_lowercase', string.ascii_lowercase)
    print('ascii_uppercase', string.ascii_uppercase)
    print('digits', string.digits)
    print('hexdigits', string.hexdigits)
    print('octdigits', string.octdigits)
    print('printable', string.printable)
    print('punctuation', string.punctuation)
    print('whitespace', string.whitespace)
except Exception as e:
    print('SKIP_Constants', type(e).__name__, e)

# === capwords function ===
try:
    print('capwords_simple', string.capwords('hello world'))
    print('capwords_multiple_spaces', string.capwords('hello   world'))
    print('capwords_leading_trailing', string.capwords('  hello world  '))
    print('capwords_custom_sep', string.capwords('hello-world', sep='-'))
    print('capwords_empty', string.capwords(''))
    print('capwords_single_word', string.capwords('hello'))
    print('capwords_already_capitalized', string.capwords('Hello World'))
    print('capwords_mixed_case', string.capwords('hElLo WoRlD'))
except Exception as e:
    print('SKIP_capwords function', type(e).__name__, e)

# === Formatter class ===
try:
    formatter = string.Formatter()

    # format method
    print('formatter_format_positional', formatter.format('Hello, {}!', 'world'))
    print('formatter_format_multiple', formatter.format('{} + {} = {}', 1, 2, 3))
    print('formatter_format_named', formatter.format('Hello, {name}!', name='world'))
    print('formatter_format_mixed', formatter.format('{} {name} {}', 'a', 'b', name='c'))

    # vformat method
    print('formatter_vformat_args', formatter.vformat('{} + {}', (1, 2), {}))
    print('formatter_vformat_kwargs', formatter.vformat('{x} * {y}', (), {'x': 3, 'y': 4}))
    print('formatter_vformat_both', formatter.vformat('{} + {n}', (1,), {'n': 5}))

    # parse method
    print('formatter_parse_simple', list(formatter.parse('{}')))
    print('formatter_parse_named', list(formatter.parse('{name}')))
    print('formatter_parse_spec', list(formatter.parse('{:>10}')))
    print('formatter_parse_conversion', list(formatter.parse('{!r}')))
    print('formatter_parse_full', list(formatter.parse('{name!s:>10}')))
    print('formatter_parse_literal', list(formatter.parse('hello')))
    print('formatter_parse_multiple', list(formatter.parse('{} and {}')))
    print('formatter_parse_adjacent', list(formatter.parse('{}{}')))
    print('formatter_parse_empty', list(formatter.parse('')))

    # get_value method
    print('formatter_get_value_positional', formatter.get_value(0, ('a', 'b'), {}))
    print('formatter_get_value_positional_1', formatter.get_value(1, ('a', 'b'), {}))
    print('formatter_get_value_keyword', formatter.get_value('key', (), {'key': 'value'}))

    # get_field method - returns (obj, used_key)
    print('formatter_get_field_simple', formatter.get_field('0', ('a', 'b'), {}))
    class DummyObj:
        name = 'attr'
    print('formatter_get_field_attr', formatter.get_field('0.name', (DummyObj(),), {}))

    # format_field method
    print('formatter_format_field_str', formatter.format_field('hello', ''))
    print('formatter_format_field_int', formatter.format_field(42, '05d'))
    print('formatter_format_field_float', formatter.format_field(3.14159, '.2f'))
    print('formatter_format_field_align', formatter.format_field('hi', '>10'))

    # convert_field method
    print('formatter_convert_field_s', formatter.convert_field('hello', 's'))
    print('formatter_convert_field_r', formatter.convert_field('hello', 'r'))
    print('formatter_convert_field_a', formatter.convert_field('hello\x00', 'a'))
    print('formatter_convert_field_none', formatter.convert_field('hello', None))

    # check_unused_args method - default does nothing, just verify it exists
    print('formatter_check_unused_args_exists', hasattr(formatter, 'check_unused_args'))
except Exception as e:
    print('SKIP_Formatter class', type(e).__name__, e)

# === Template class ===
try:
    # Basic template
    tmpl_basic = string.Template('Hello, $name!')
    print('template_basic', tmpl_basic.template)
    print('template_substitute_basic', tmpl_basic.substitute(name='world'))

    # Template with multiple substitutions
    tmpl_multi = string.Template('$greeting, $name!')
    print('template_substitute_multi', tmpl_multi.substitute(greeting='Hi', name='Alice'))

    # Template with curly braces
    tmpl_braces = string.Template('Hello, ${name}!')
    print('template_substitute_braces', tmpl_braces.substitute(name='Bob'))

    # Template with adjacent text
    tmpl_adjacent = string.Template('${prefix}name')
    print('template_substitute_adjacent', tmpl_adjacent.substitute(prefix='user_'))

    # safe_substitute - doesn't raise on missing keys
    tmpl_safe = string.Template('$known and $unknown')
    print('template_safe_substitute', tmpl_safe.safe_substitute(known='value'))
    print('template_safe_substitute_all', tmpl_safe.safe_substitute(known='a', unknown='b'))

    # safe_substitute with braces
    tmpl_safe_braces = string.Template('${known} and ${unknown}')
    print('template_safe_substitute_braces', tmpl_safe_braces.safe_substitute(known='value'))

    # Template with escaped delimiter
    tmpl_escaped = string.Template('Price: $$10')
    print('template_escaped_dollar', tmpl_escaped.substitute())

    # Template with mixed escaped and substitution
    tmpl_mixed = string.Template('$$$name')
    print('template_mixed_escaped', tmpl_mixed.substitute(name='price'))

    # get_identifiers method
    tmpl_ids = string.Template('$a, $b, ${c.d}')
    print('template_get_identifiers', tmpl_ids.get_identifiers())
    tmpl_ids_empty = string.Template('no substitutions')
    print('template_get_identifiers_empty', tmpl_ids_empty.get_identifiers())
    tmpl_ids_escaped = string.Template('$$escaped $real')
    print('template_get_identifiers_escaped', tmpl_ids_escaped.get_identifiers())

    # is_valid method
    tmpl_valid = string.Template('$valid')
    print('template_is_valid_ok', tmpl_valid.is_valid())
    tmpl_invalid = string.Template('$')
    print('template_is_valid_invalid', tmpl_invalid.is_valid())
    tmpl_invalid_brace = string.Template('${')
    print('template_is_valid_invalid_brace', tmpl_invalid_brace.is_valid())
    tmpl_valid_escaped = string.Template('$$')
    print('template_is_valid_escaped', tmpl_valid_escaped.is_valid())

    # Template class attributes
    print('template_delimiter', string.Template.delimiter)
    print('template_idpattern', string.Template.idpattern)
    print('template_braceidpattern', string.Template.braceidpattern)
    print('template_flags', string.Template.flags)

    # Template with custom pattern (subclass)
    class CustomTemplate(string.Template):
        delimiter = '%'
        idpattern = '[A-Z]+'

    custom = CustomTemplate('Hello, %NAME!')
    print('template_custom_delimiter', custom.substitute(NAME='WORLD'))

    # Template with invalid identifiers - safe_substitute preserves them
    tmpl_invalid_id = string.Template('$123invalid')
    print('template_safe_substitute_invalid', tmpl_invalid_id.safe_substitute())

    # Template edge cases
    tmpl_empty = string.Template('')
    print('template_empty_substitute', tmpl_empty.substitute())

    tmpl_no_subs = string.Template('plain text')
    print('template_no_substitutions', tmpl_no_subs.substitute())

    tmpl_unicode = string.Template('Hello, $naïve!')
    print('template_unicode_identifier', tmpl_unicode.safe_substitute())

    tmpl_multiline = string.Template('Line 1: $a\nLine 2: $b')
    print('template_multiline', tmpl_multiline.substitute(a='first', b='second'))

    # Template substitute with dict
    print('template_substitute_dict', tmpl_multi.substitute({'greeting': 'Hey', 'name': 'Sam'}))

    # Template substitute with keyword override
    print('template_substitute_override', tmpl_multi.substitute({'greeting': 'Hey'}, name='Override'))
except Exception as e:
    print('SKIP_Template class', type(e).__name__, e)
