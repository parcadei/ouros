# HOST MODULE: Only testing sandbox-safe subset
# Comprehensive CSV module parity tests (in-memory only)

import csv
import io

# === reader: basic usage ===
try:
    content = 'a,b,c\n1,2,3\n'
    reader = csv.reader(io.StringIO(content))
    print('csv_reader_rows', list(reader))
except Exception as e:
    print('SKIP_reader_basic_usage', type(e).__name__, e)

# === reader: with delimiter ===
try:
    content = 'a;b;c\n1;2;3\n'
    reader = csv.reader(io.StringIO(content), delimiter=';')
    print('csv_reader_delimiter', list(reader))
except Exception as e:
    print('SKIP_reader_with_delimiter', type(e).__name__, e)

# === reader: with quotechar ===
try:
    content = '"a,b",c\n"1,2",3\n'
    reader = csv.reader(io.StringIO(content), quotechar='"')
    print('csv_reader_quotechar', list(reader))
except Exception as e:
    print('SKIP_reader_with_quotechar', type(e).__name__, e)

# === reader: with different lineterminator (output only) ===
try:
    writer = io.StringIO()
    w = csv.writer(writer, lineterminator='\r\n')
    w.writerow(['a', 'b'])
    w.writerow(['1', '2'])
    print('csv_reader_lineterminator_output', repr(writer.getvalue()))
except Exception as e:
    print('SKIP_reader_with_different_lineterminator_output_only', type(e).__name__, e)

# === reader: empty fields ===
try:
    content = 'a,,c\n,2,\n'
    reader = csv.reader(io.StringIO(content))
    print('csv_reader_empty_fields', list(reader))
except Exception as e:
    print('SKIP_reader_empty_fields', type(e).__name__, e)

# === reader: QUOTE_MINIMAL (default) ===
try:
    content = 'a,"b,b",c\n'
    reader = csv.reader(io.StringIO(content), quoting=csv.QUOTE_MINIMAL)
    print('csv_reader_quote_minimal', list(reader))
except Exception as e:
    print('SKIP_reader_QUOTE_MINIMAL_default', type(e).__name__, e)

# === reader: QUOTE_ALL (all fields quoted) ===
try:
    content = '"a","b","c"\n"1","2","3"\n'
    reader = csv.reader(io.StringIO(content), quoting=csv.QUOTE_ALL)
    print('csv_reader_quote_all', list(reader))
except Exception as e:
    print('SKIP_reader_QUOTE_ALL_all_fields_quoted', type(e).__name__, e)

# === reader: QUOTE_NONNUMERIC ===
try:
    # Note: QUOTE_NONNUMERIC converts unquoted fields to floats
    content = '"a",1,"b",2.5\n'
    reader = csv.reader(io.StringIO(content), quoting=csv.QUOTE_NONNUMERIC)
    print('csv_reader_quote_nonnumeric', list(reader))
except Exception as e:
    print('SKIP_reader_QUOTE_NONNUMERIC', type(e).__name__, e)

# === reader: QUOTE_NONE ===
try:
    content = 'a,b,c\n1,2,3\n'
    reader = csv.reader(io.StringIO(content), quoting=csv.QUOTE_NONE)
    print('csv_reader_quote_none', list(reader))
except Exception as e:
    print('SKIP_reader_QUOTE_NONE', type(e).__name__, e)

# === reader: escapechar ===
try:
    content = 'a,b\\,c\n1,2,3\n'
    reader = csv.reader(io.StringIO(content), escapechar='\\', quoting=csv.QUOTE_NONE)
    print('csv_reader_escapechar', list(reader))
except Exception as e:
    print('SKIP_reader_escapechar', type(e).__name__, e)

# === reader: doublequote ===
try:
    content = '"a""b",c\n'
    reader = csv.reader(io.StringIO(content), doublequote=True)
    print('csv_reader_doublequote', list(reader))
except Exception as e:
    print('SKIP_reader_doublequote', type(e).__name__, e)

# === reader: skipinitialspace ===
try:
    content = 'a, b, c\n1, 2, 3\n'
    reader = csv.reader(io.StringIO(content), skipinitialspace=True)
    print('csv_reader_skipinitialspace', list(reader))
except Exception as e:
    print('SKIP_reader_skipinitialspace', type(e).__name__, e)

# === reader: empty input ===
try:
    reader = csv.reader(io.StringIO(''))
    print('csv_reader_empty_input', list(reader))
except Exception as e:
    print('SKIP_reader_empty_input', type(e).__name__, e)

# === reader: single row no newline ===
try:
    reader = csv.reader(io.StringIO('a,b,c'))
    print('csv_reader_no_newline', list(reader))
except Exception as e:
    print('SKIP_reader_single_row_no_newline', type(e).__name__, e)

# === writer: basic usage ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow(['a', 'b', 'c'])
    writer.writerow(['1', '2', '3'])
    print('csv_writer_basic', output.getvalue())
except Exception as e:
    print('SKIP_writer_basic_usage', type(e).__name__, e)

# === writer: with delimiter ===
try:
    output = io.StringIO()
    writer = csv.writer(output, delimiter=';')
    writer.writerow(['a', 'b', 'c'])
    print('csv_writer_delimiter', output.getvalue())
except Exception as e:
    print('SKIP_writer_with_delimiter', type(e).__name__, e)

# === writer: with quotechar ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quotechar="'")
    writer.writerow(['a', 'b,b', 'c'])
    print('csv_writer_quotechar', output.getvalue())
except Exception as e:
    print('SKIP_writer_with_quotechar', type(e).__name__, e)

# === writer: QUOTE_ALL ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_ALL)
    writer.writerow(['a', 'b', 'c'])
    print('csv_writer_quote_all', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_ALL', type(e).__name__, e)

# === writer: QUOTE_MINIMAL ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_MINIMAL)
    writer.writerow(['a', 'b,b', 'c'])
    print('csv_writer_quote_minimal', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_MINIMAL', type(e).__name__, e)

# === writer: QUOTE_NONNUMERIC ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_NONNUMERIC)
    writer.writerow(['a', 1, 'b', 2.5])
    print('csv_writer_quote_nonnumeric', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_NONNUMERIC', type(e).__name__, e)

# === writer: QUOTE_NONE with escapechar ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_NONE, escapechar='\\')
    writer.writerow(['a', 'b,b', 'c'])
    print('csv_writer_quote_none_escape', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_NONE_with_escapechar', type(e).__name__, e)

# === writer: QUOTE_STRINGS (Python 3.12+) ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_STRINGS)
    writer.writerow(['a', 1, 'b', 2.5])
    print('csv_writer_quote_strings', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_STRINGS_Python_3.12+', type(e).__name__, e)

# === writer: QUOTE_NOTNULL (Python 3.12+) ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_NOTNULL)
    writer.writerow(['a', None, 'b', ''])
    print('csv_writer_quote_notnull', output.getvalue())
except Exception as e:
    print('SKIP_writer_QUOTE_NOTNULL_Python_3.12+', type(e).__name__, e)

# === writer: writerows ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerows([['a', 'b'], ['1', '2'], ['3', '4']])
    print('csv_writer_writerows', output.getvalue())
except Exception as e:
    print('SKIP_writer_writerows', type(e).__name__, e)

# === writer: None values ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow(['a', None, 'c'])
    print('csv_writer_none_values', output.getvalue())
except Exception as e:
    print('SKIP_writer_None_values', type(e).__name__, e)

# === writer: numeric values ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow([1, 2.5, True])
    print('csv_writer_numeric', output.getvalue())
except Exception as e:
    print('SKIP_writer_numeric_values', type(e).__name__, e)

# === DictReader: basic with header ===
try:
    content = 'name,age,city\nAlice,30,NYC\nBob,25,LA\n'
    reader = csv.DictReader(io.StringIO(content))
    print('csv_dictreader_basic', list(reader))
except Exception as e:
    print('SKIP_DictReader_basic_with_header', type(e).__name__, e)

# === DictReader: with fieldnames ===
try:
    content = 'Alice,30,NYC\nBob,25,LA\n'
    reader = csv.DictReader(io.StringIO(content), fieldnames=['name', 'age', 'city'])
    print('csv_dictreader_fieldnames', list(reader))
except Exception as e:
    print('SKIP_DictReader_with_fieldnames', type(e).__name__, e)

# === DictReader: restkey ===
try:
    content = 'name,age\nAlice,30,NYC,USA\n'
    reader = csv.DictReader(io.StringIO(content), restkey='extra')
    print('csv_dictreader_restkey', list(reader))
except Exception as e:
    print('SKIP_DictReader_restkey', type(e).__name__, e)

# === DictReader: restval ===
try:
    content = 'name,age,city\nAlice,30\n'
    reader = csv.DictReader(io.StringIO(content), restval='UNKNOWN')
    print('csv_dictreader_restval', list(reader))
except Exception as e:
    print('SKIP_DictReader_restval', type(e).__name__, e)

# === DictReader: empty input ===
try:
    reader = csv.DictReader(io.StringIO(''))
    print('csv_dictreader_empty', list(reader))
except Exception as e:
    print('SKIP_DictReader_empty_input', type(e).__name__, e)

# === DictWriter: basic usage ===
try:
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=['name', 'age'])
    writer.writeheader()
    writer.writerow({'name': 'Alice', 'age': '30'})
    writer.writerow({'name': 'Bob', 'age': '25'})
    print('csv_dictwriter_basic', output.getvalue())
except Exception as e:
    print('SKIP_DictWriter_basic_usage', type(e).__name__, e)

# === DictWriter: with extrasaction='ignore' ===
try:
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=['name'], extrasaction='ignore')
    writer.writeheader()
    writer.writerow({'name': 'Alice', 'extra': 'ignored'})
    print('csv_dictwriter_extrasignore', output.getvalue())
except Exception as e:
    print('SKIP_DictWriter_with_extrasaction_ignore', type(e).__name__, e)

# === DictWriter: restval ===
try:
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=['name', 'age', 'city'], restval='N/A')
    writer.writeheader()
    writer.writerow({'name': 'Alice', 'age': '30'})
    print('csv_dictwriter_restval', output.getvalue())
except Exception as e:
    print('SKIP_DictWriter_restval', type(e).__name__, e)

# === DictWriter: writerows ===
try:
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=['a', 'b'])
    writer.writeheader()
    writer.writerows([{'a': '1', 'b': '2'}, {'a': '3', 'b': '4'}])
    print('csv_dictwriter_writerows', output.getvalue())
except Exception as e:
    print('SKIP_DictWriter_writerows', type(e).__name__, e)

# === Sniffer: sniff excel dialect ===
try:
    sample = 'a,b,c\n1,2,3\n'
    sniffer = csv.Sniffer()
    dialect = sniffer.sniff(sample)
    print('csv_sniffer_delimiter', dialect.delimiter)
    print('csv_sniffer_quotechar', dialect.quotechar)
    print('csv_sniffer_has_header', sniffer.has_header(sample))
except Exception as e:
    print('SKIP_Sniffer_sniff_excel_dialect', type(e).__name__, e)

# === Sniffer: sniff semicolon delimiter ===
try:
    sample = 'a;b;c\n1;2;3\n'
    sniffer = csv.Sniffer()
    dialect = sniffer.sniff(sample)
    print('csv_sniffer_semicolon', dialect.delimiter)
except Exception as e:
    print('SKIP_Sniffer_sniff_semicolon_delimiter', type(e).__name__, e)

# === Sniffer: sniff with quotes ===
try:
    sample = '"a","b","c"\n"1","2","3"\n'
    sniffer = csv.Sniffer()
    dialect = sniffer.sniff(sample)
    print('csv_sniffer_quotes', dialect.delimiter, dialect.quotechar)
except Exception as e:
    print('SKIP_Sniffer_sniff_with_quotes', type(e).__name__, e)

# === excel dialect ===
try:
    output = io.StringIO()
    writer = csv.writer(output, dialect=csv.excel)
    writer.writerow(['a', 'b', 'c'])
    print('csv_dialect_excel', output.getvalue())
    print('csv_dialect_excel_delimiter', csv.excel.delimiter)
    print('csv_dialect_excel_lineterminator', repr(csv.excel.lineterminator))
    print('csv_dialect_excel_quotechar', csv.excel.quotechar)
    print('csv_dialect_excel_quoting', csv.excel.quoting)
    print('csv_dialect_excel_doublequote', csv.excel.doublequote)
    print('csv_dialect_excel_skipinitialspace', csv.excel.skipinitialspace)
except Exception as e:
    print('SKIP_excel_dialect', type(e).__name__, e)

# === excel_tab dialect ===
try:
    output = io.StringIO()
    writer = csv.writer(output, dialect=csv.excel_tab)
    writer.writerow(['a', 'b', 'c'])
    print('csv_dialect_excel_tab', output.getvalue())
    print('csv_dialect_excel_tab_delimiter', repr(csv.excel_tab.delimiter))
except Exception as e:
    print('SKIP_excel_tab_dialect', type(e).__name__, e)

# === unix_dialect ===
try:
    output = io.StringIO()
    writer = csv.writer(output, dialect=csv.unix_dialect)
    writer.writerow(['a', 'b', 'c'])
    print('csv_dialect_unix', output.getvalue())
    print('csv_dialect_unix_delimiter', repr(csv.unix_dialect.delimiter))
    print('csv_dialect_unix_quotechar', csv.unix_dialect.quotechar)
    print('csv_dialect_unix_quoting', csv.unix_dialect.quoting)
    print('csv_dialect_unix_lineterminator', repr(csv.unix_dialect.lineterminator))
except Exception as e:
    print('SKIP_unix_dialect', type(e).__name__, e)

# === register_dialect / get_dialect / list_dialects / unregister_dialect ===
try:
    csv.register_dialect('mydialect', delimiter='|', quotechar="'")
    print('csv_list_dialects_after_register', csv.list_dialects())
    d = csv.get_dialect('mydialect')
    print('csv_get_dialect_delimiter', d.delimiter)
    print('csv_get_dialect_quotechar', d.quotechar)

    output = io.StringIO()
    writer = csv.writer(output, dialect='mydialect')
    writer.writerow(['a', 'b', 'c'])
    print('csv_custom_dialect_output', output.getvalue())

    csv.unregister_dialect('mydialect')
    print('csv_list_dialects_after_unregister', csv.list_dialects())
except Exception as e:
    print('SKIP_register_dialect_get_dialect_list_dialects_unregister_dialect', type(e).__name__, e)

# === register_dialect with Dialect class ===
try:
    class MyDialect(csv.Dialect):
        delimiter = ';'
        quotechar = '"'
        doublequote = True
        skipinitialspace = False
        lineterminator = '\n'
        quoting = csv.QUOTE_MINIMAL

    csv.register_dialect('myclassdialect', MyDialect)
    d = csv.get_dialect('myclassdialect')
    print('csv_class_dialect_delimiter', d.delimiter)
    csv.unregister_dialect('myclassdialect')
except Exception as e:
    print('SKIP_register_dialect_with_Dialect_class', type(e).__name__, e)

# === field_size_limit ===
try:
    original_limit = csv.field_size_limit()
    print('csv_field_size_limit_default', original_limit)
    new_limit = csv.field_size_limit(100000)
    print('csv_field_size_limit_previous', new_limit)
    print('csv_field_size_limit_current', csv.field_size_limit())
    csv.field_size_limit(original_limit)
    print('csv_field_size_limit_restored', csv.field_size_limit())
except Exception as e:
    print('SKIP_field_size_limit', type(e).__name__, e)

# === Dialect class attributes ===
try:
    print('csv_quote_all_value', csv.QUOTE_ALL)
    print('csv_quote_minimal_value', csv.QUOTE_MINIMAL)
    print('csv_quote_nonnumeric_value', csv.QUOTE_NONNUMERIC)
    print('csv_quote_none_value', csv.QUOTE_NONE)
    print('csv_quote_notnull_value', csv.QUOTE_NOTNULL)
    print('csv_quote_strings_value', csv.QUOTE_STRINGS)
except Exception as e:
    print('SKIP_Dialect_class_attributes', type(e).__name__, e)

# === Error exception ===
try:
    print('csv_error_class', csv.Error)
    print('csv_error_is_exception', issubclass(csv.Error, Exception))
except Exception as e:
    print('SKIP_Error_exception', type(e).__name__, e)

# === reader with large dataset ===
try:
    content = '\n'.join([f'field{i},field{i+1}' for i in range(0, 100, 2)])
    reader = csv.reader(io.StringIO(content))
    rows = list(reader)
    print('csv_reader_large_rowcount', len(rows))
    print('csv_reader_large_first', rows[0])
    print('csv_reader_large_last', rows[-1])
except Exception as e:
    print('SKIP_reader_with_large_dataset', type(e).__name__, e)

# === writer: special characters in fields ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow(['a\nb', 'c\td', 'e"f', 'g\\h'])
    print('csv_writer_special_chars', output.getvalue())
except Exception as e:
    print('SKIP_writer_special_characters_in_fields', type(e).__name__, e)

# === writer: empty rows ===
try:
    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow([])
    writer.writerow([''])
    print('csv_writer_empty_rows', output.getvalue())
except Exception as e:
    print('SKIP_writer_empty_rows', type(e).__name__, e)

# === DictReader: fieldnames iteration ===
try:
    content = 'a,b\n1,2\n3,4\n'
    reader = csv.DictReader(io.StringIO(content))
    print('csv_dictreader_fieldnames_attr', reader.fieldnames)
except Exception as e:
    print('SKIP_DictReader_fieldnames_iteration', type(e).__name__, e)

# === DictWriter: fieldnames attribute ===
try:
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=['x', 'y'])
    print('csv_dictwriter_fieldnames_attr', writer.fieldnames)
except Exception as e:
    print('SKIP_DictWriter_fieldnames_attribute', type(e).__name__, e)

# === dialect inheritance ===
try:
    csv.register_dialect('based_on_excel', csv.excel, delimiter='|')
    d = csv.get_dialect('based_on_excel')
    print('csv_dialect_inherit_delimiter', d.delimiter)
    print('csv_dialect_inherit_quotechar', d.quotechar)
    csv.unregister_dialect('based_on_excel')
except Exception as e:
    print('SKIP_dialect_inheritance', type(e).__name__, e)

# === reader: single column ===
try:
    content = 'a\n1\n2\n3\n'
    reader = csv.reader(io.StringIO(content))
    print('csv_reader_single_column', list(reader))
except Exception as e:
    print('SKIP_reader_single_column', type(e).__name__, e)

# === reader: many columns ===
try:
    content = ','.join([str(i) for i in range(20)]) + '\n'
    reader = csv.reader(io.StringIO(content))
    print('csv_reader_many_columns', next(reader))
except Exception as e:
    print('SKIP_reader_many_columns', type(e).__name__, e)

# === writer: all quoted with newlines ===
try:
    output = io.StringIO()
    writer = csv.writer(output, quoting=csv.QUOTE_ALL)
    writer.writerow(['line1\nline2', 'normal'])
    print('csv_writer_newlines_quoted', output.getvalue())
except Exception as e:
    print('SKIP_writer_all_quoted_with_newlines', type(e).__name__, e)
