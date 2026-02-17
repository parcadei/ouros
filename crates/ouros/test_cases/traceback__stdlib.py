import traceback


# === Module API ===
for name in [
    'format_exc',
    'format_exception',
    'format_tb',
    'format_stack',
    'print_exc',
    'print_exception',
    'print_tb',
    'print_stack',
    'extract_tb',
    'extract_stack',
    'format_list',
    'FrameSummary',
    'StackSummary',
    'TracebackException',
]:
    assert hasattr(traceback, name), f'missing traceback.{name}'


# === FrameSummary ===
frame = traceback.FrameSummary('demo.py', 10, 'func', line='x = 1')
assert frame.filename == 'demo.py', 'FrameSummary.filename'
assert frame.lineno == 10, 'FrameSummary.lineno'
assert frame.name == 'func', 'FrameSummary.name'
assert frame.line == 'x = 1', 'FrameSummary.line'


# === format_list ===
formatted_list = traceback.format_list([('demo.py', 10, 'func', 'x = 1')])
assert formatted_list == ['  File "demo.py", line 10, in func\n    x = 1\n'], 'format_list tuple input'


# === format helpers ===
assert traceback.format_exc() == 'NoneType: None\n', 'format_exc with no active exception'
assert traceback.format_tb(None) == [], 'format_tb(None)'

formatted_exception = traceback.format_exception(ValueError, ValueError('bad'), None)
assert isinstance(formatted_exception, list), 'format_exception returns list'
assert formatted_exception[-1] == 'ValueError: bad\n', 'format_exception line'

stack_lines = traceback.format_stack(limit=0)
assert isinstance(stack_lines, list), 'format_stack returns list'


# === extraction helpers ===
summary_from_tb = traceback.extract_tb(None)
assert hasattr(summary_from_tb, 'format'), 'extract_tb returns StackSummary-like object'
assert summary_from_tb.format() == [], 'extract_tb(None) empty'

summary_from_extract = traceback.StackSummary.extract([])
assert hasattr(summary_from_extract, 'format'), 'StackSummary.extract returns object with format'
assert summary_from_extract.format() == [], 'StackSummary.extract([]) empty'


# === TracebackException ===
tb_exc = traceback.TracebackException(ValueError, ValueError('bad'), None)
assert list(tb_exc.format_exception_only()) == ['ValueError: bad\n'], 'TracebackException.format_exception_only'
assert list(tb_exc.format())[-1] == 'ValueError: bad\n', 'TracebackException.format'


# === print helpers ===
traceback.print_exception(ValueError, ValueError('bad'), None, file=None)
