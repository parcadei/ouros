import csv
import sys

try:
    import io
except ModuleNotFoundError:
    io = None

# === field_size_limit ===
default_limit = csv.field_size_limit()
assert default_limit == 131072, 'field_size_limit default'
old_limit = csv.field_size_limit(1024)
assert old_limit == default_limit, 'field_size_limit returns previous value'
assert csv.field_size_limit() == 1024, 'field_size_limit updated'

# === dialect registry ===
csv.register_dialect('pipe', delimiter='|')
assert csv.list_dialects().count('pipe') == 1, 'list_dialects includes pipe'
pipe_dialect = csv.get_dialect('pipe')
pipe_delim = pipe_dialect.delimiter if hasattr(pipe_dialect, 'delimiter') else pipe_dialect['delimiter']
assert pipe_delim == '|', 'get_dialect returns delimiter'
csv.unregister_dialect('pipe')
assert csv.list_dialects().count('pipe') == 0, 'unregister_dialect removes entry'

csv.register_dialect('semi', delimiter=';')
semi_dialect = csv.get_dialect('semi')
semi_delim = semi_dialect.delimiter if hasattr(semi_dialect, 'delimiter') else semi_dialect['delimiter']
assert semi_delim == ';', 'register_dialect accepts delimiter kw'
csv.unregister_dialect('semi')

# === constants and exception class ===
try:
    raise csv.Error('csv boom')
except Exception as exc:
    assert exc is not None, 'csv.Error should behave like an exception class'

assert csv.QUOTE_MINIMAL == 0, 'QUOTE_MINIMAL constant'
assert csv.QUOTE_ALL == 1, 'QUOTE_ALL constant'
assert csv.QUOTE_NONNUMERIC == 2, 'QUOTE_NONNUMERIC constant'
assert csv.QUOTE_NONE == 3, 'QUOTE_NONE constant'
assert csv.QUOTE_STRINGS == 4, 'QUOTE_STRINGS constant'
assert csv.QUOTE_NOTNULL == 5, 'QUOTE_NOTNULL constant'

# === built-in dialect objects ===
excel_delim = csv.excel.delimiter if hasattr(csv.excel, 'delimiter') else csv.excel['delimiter']
excel_tab_delim = csv.excel_tab.delimiter if hasattr(csv.excel_tab, 'delimiter') else csv.excel_tab['delimiter']
unix_delim = csv.unix_dialect.delimiter if hasattr(csv.unix_dialect, 'delimiter') else csv.unix_dialect['delimiter']
assert excel_delim == ',', 'excel dialect delimiter'
assert excel_tab_delim == '\t', 'excel_tab dialect delimiter'
assert unix_delim == ',', 'unix_dialect delimiter'
assert csv.list_dialects().count('excel') == 1, 'excel should be registered'
assert csv.list_dialects().count('excel-tab') == 1, 'excel-tab should be registered'
assert csv.list_dialects().count('unix') == 1, 'unix should be registered'

# === DictReader ===
rows = ['a,b,c', '1,2,3', '4,5,6']
result = list(csv.DictReader(rows))
assert len(result) == 2, 'DictReader returns data rows only'
assert result[0]['a'] == '1', 'DictReader row 1 value'
assert result[1]['c'] == '6', 'DictReader row 2 value'

result = list(csv.DictReader(['1,2', '3,4'], ['x', 'y']))
assert result[0]['x'] == '1', 'DictReader uses provided fieldnames'
assert result[1]['y'] == '4', 'DictReader uses provided fieldnames row 2'

# === DictWriter ===
rows = [{'a': '1', 'b': '2'}, {'a': '3', 'b': '4'}]
missing_rows = [{'a': '1'}]
is_ouros = 'Ouros' in sys.version
if is_ouros:
    output = csv.DictWriter(rows, ['a', 'b'])
    assert output == ['1,2', '3,4'], 'DictWriter writes rows in fieldname order'
    output = csv.DictWriter(missing_rows, ['a', 'b'])
    assert output[0] == '1,', 'DictWriter fills missing fields with empty string'
else:
    buf = io.StringIO()
    writer = csv.DictWriter(buf, fieldnames=['a', 'b'])
    writer.writerows(rows)
    assert buf.getvalue().splitlines() == ['1,2', '3,4'], 'DictWriter writes rows in fieldname order'
    buf = io.StringIO()
    writer = csv.DictWriter(buf, fieldnames=['a', 'b'])
    writer.writerows(missing_rows)
    assert buf.getvalue().splitlines() == ['1,'], 'DictWriter fills missing fields with empty string'

# === Sniffer ===
if is_ouros:
    dialect = csv.Sniffer.sniff('a;b;c')
else:
    dialect = csv.Sniffer().sniff('a;b;c')
delim = dialect.delimiter if hasattr(dialect, 'delimiter') else dialect['delimiter']
assert delim == ';', 'Sniffer detects semicolon'

if is_ouros:
    dialect = csv.Sniffer.sniff('a\tb\tc')
else:
    dialect = csv.Sniffer().sniff('a\tb\tc')
delim = dialect.delimiter if hasattr(dialect, 'delimiter') else dialect['delimiter']
assert delim == '\t', 'Sniffer detects tab'

# reset field size limit to avoid cross-test contamination
csv.field_size_limit(default_limit)
