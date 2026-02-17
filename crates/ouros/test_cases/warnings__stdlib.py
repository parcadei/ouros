import sys
import warnings


# === Module API ===
public_names = [
    'WarningMessage',
    'catch_warnings',
    'defaultaction',
    'deprecated',
    'filters',
    'filterwarnings',
    'formatwarning',
    'onceregistry',
    'resetwarnings',
    'showwarning',
    'simplefilter',
    'sys',
    'warn',
    'warn_explicit',
]
for name in public_names:
    assert hasattr(warnings, name), f'missing warnings.{name}'

assert isinstance(warnings.defaultaction, str), 'defaultaction must be a string'
assert isinstance(warnings.filters, list), 'filters must be a list'
assert isinstance(warnings.onceregistry, dict), 'onceregistry must be a dict'
assert warnings.sys.version_info == sys.version_info, 'warnings.sys must alias sys'


# === WarningMessage ===
warning_message = warnings.WarningMessage('hello', Warning, 'demo.py', 12)
assert warning_message.message == 'hello', 'WarningMessage.message'
assert warning_message.category is Warning, 'WarningMessage.category'
assert warning_message.filename == 'demo.py', 'WarningMessage.filename'
assert warning_message.lineno == 12, 'WarningMessage.lineno'
assert warning_message.file is None, 'WarningMessage.file default'
assert warning_message.line is None, 'WarningMessage.line default'
assert warning_message.source is None, 'WarningMessage.source default'
warning_message_text = str(warning_message)
assert warning_message_text.startswith("{message : 'hello', category : "), 'WarningMessage.__str__ prefix'
assert warning_message_text.endswith(", filename : 'demo.py', lineno : 12, line : None}"), 'WarningMessage.__str__ suffix'


# === formatwarning / showwarning ===
formatted = warnings.formatwarning('hello', Warning, 'demo.py', 12)
assert formatted.startswith('demo.py:12: '), 'formatwarning prefix'
assert formatted.endswith(': hello\n'), 'formatwarning suffix'

formatted_with_line = warnings.formatwarning('hello', Warning, 'demo.py', 12, line='  x = 1  ')
assert formatted_with_line.startswith('demo.py:12: '), 'formatwarning with line prefix'
assert formatted_with_line.endswith(': hello\n  x = 1\n'), 'formatwarning includes stripped source line'


showwarning_result = warnings.showwarning('shown', Warning, 'file.py', 9)
assert showwarning_result is None, 'showwarning returns None'


# === filterwarnings / simplefilter / resetwarnings ===
warnings.resetwarnings()
assert warnings.filters == [], 'resetwarnings clears filters'

warnings.simplefilter('always')
assert len(warnings.filters) == 1, 'simplefilter inserts one filter'
assert warnings.filters[0][0] == 'always', 'simplefilter action stored'
assert warnings.filters[0][2] is Warning, 'simplefilter category default'
assert warnings.filters[0][4] == 0, 'simplefilter lineno default'

warnings.filterwarnings('ignore', message='^abc$', module='^mod$', lineno=7, append=True)
assert len(warnings.filters) == 2, 'filterwarnings append=True appends filter'
appended_filter = warnings.filters[1]
assert appended_filter[0] == 'ignore', 'filterwarnings action stored'
assert appended_filter[2] is Warning, 'filterwarnings category default'
assert appended_filter[4] == 7, 'filterwarnings lineno stored'
if hasattr(appended_filter[1], 'pattern'):
    assert appended_filter[1].pattern == '^abc$', 'filterwarnings message regex pattern'
else:
    assert appended_filter[1] == '^abc$', 'filterwarnings message matcher'
if hasattr(appended_filter[3], 'pattern'):
    assert appended_filter[3].pattern == '^mod$', 'filterwarnings module regex pattern'
else:
    assert appended_filter[3] == '^mod$', 'filterwarnings module matcher'

try:
    warnings.simplefilter('invalid')
    assert False, 'simplefilter must reject invalid actions'
except ValueError as exc:
    assert str(exc) == "invalid action: 'invalid'", 'simplefilter invalid action message'

try:
    warnings.filterwarnings('ignore', lineno=-1)
    assert False, 'filterwarnings must reject negative lineno'
except ValueError as exc:
    assert str(exc) == 'lineno must be an int >= 0', 'filterwarnings negative lineno message'


# === warn / warn_explicit ===
warnings.resetwarnings()
with warnings.catch_warnings(record=True) as captured:
    warnings.simplefilter('always')
    warnings.warn('first warning')
    warnings.warn('second warning')
    assert len(captured) == 2, 'warn emits two warnings under always filter'
    assert isinstance(captured[0], warnings.WarningMessage), 'warn capture entry type'
    assert str(captured[0].message) == 'first warning', 'warn captured message text'
    assert captured[0].category is not None, 'warn captured category is set'

warnings.resetwarnings()
registry = {}
with warnings.catch_warnings(record=True) as captured_explicit:
    warnings.simplefilter('always')
    warnings.warn_explicit(
        'explicit warning',
        Warning,
        'explicit.py',
        42,
        module='demo_mod',
        registry=registry,
        source=None,
    )
    assert len(captured_explicit) == 1, 'warn_explicit emits one warning'
    explicit = captured_explicit[0]
    assert str(explicit.message) == 'explicit warning', 'warn_explicit message text'
    assert explicit.filename == 'explicit.py', 'warn_explicit filename'
    assert explicit.lineno == 42, 'warn_explicit lineno'
assert registry.get('version') is not None, 'warn_explicit writes registry version'


# === once filter behavior ===
warnings.resetwarnings()
with warnings.catch_warnings(record=True) as captured_once:
    warnings.simplefilter('once')
    warnings.warn('only once')
    warnings.warn('only once')
    assert len(captured_once) == 1, 'once filter suppresses duplicate warning'


# === catch_warnings ===
context = warnings.catch_warnings(record=True)
assert repr(context) == 'catch_warnings(record=True)', 'catch_warnings repr with record=True'

context.__enter__()
try:
    try:
        context.__enter__()
        assert False, 'catch_warnings cannot be entered twice'
    except RuntimeError as exc:
        assert str(exc).startswith('Cannot enter '), 'double enter error prefix'
        assert str(exc).endswith(' twice'), 'double enter error suffix'
finally:
    context.__exit__(None, None, None)

context = warnings.catch_warnings()
try:
    context.__exit__(None, None, None)
    assert False, 'catch_warnings cannot exit before enter'
except RuntimeError as exc:
    assert str(exc).startswith('Cannot exit '), 'exit-before-enter error prefix'
    assert str(exc).endswith(' without entering first'), 'exit-before-enter error suffix'


# === deprecated ===
deprecated_function_decorator = warnings.deprecated('use replacement', category=None)


def decorated_function():
    return 'ok'


decorated_function = deprecated_function_decorator(decorated_function)

deprecated_class_decorator = warnings.deprecated('legacy class', category=None)


class DecoratedClass:
    pass


DecoratedClass = deprecated_class_decorator(DecoratedClass)

assert decorated_function() == 'ok', 'deprecated(category=None) leaves callable behavior unchanged'
assert decorated_function.__deprecated__ == 'use replacement', 'deprecated(category=None) sets __deprecated__ on function'
assert DecoratedClass.__deprecated__ == 'legacy class', 'deprecated(category=None) sets __deprecated__ on class'

try:
    bad_target_decorator = warnings.deprecated('bad target')
    bad_target_decorator(123)
    assert False, 'deprecated with non-None category rejects non-callable targets'
except TypeError as exc:
    expected = '@deprecated decorator with non-None category must be applied to a class or callable, not 123'
    assert str(exc) == expected, 'deprecated target type error message'


# === argument validation ===
try:
    warnings.warn()
    assert False, 'warn requires message argument'
except TypeError as exc:
    assert str(exc) == "warn() missing required argument 'message' (pos 1)", 'warn missing message error'

try:
    warnings.warn_explicit('only message')
    assert False, 'warn_explicit requires category/filename/lineno'
except TypeError as exc:
    assert str(exc) == "warn_explicit() missing required argument 'category' (pos 2)", 'warn_explicit missing argument error'
