"""
Parity tests for Python's io module.
Tests StringIO, BytesIO, and other io functionality.
"""
import io

# === Module constants ===
try:
    print('default_buffer_size', io.DEFAULT_BUFFER_SIZE)
    print('seek_set', io.SEEK_SET)
    print('seek_cur', io.SEEK_CUR)
    print('seek_end', io.SEEK_END)
except Exception as e:
    print('SKIP_Module constants', type(e).__name__, e)

# === StringIO: Basic construction ===
try:
    s = io.StringIO()
    print('stringio_empty_construction', s.getvalue())

    s = io.StringIO('initial value')
    print('stringio_initial_construction', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: Basic construction', type(e).__name__, e)

# === StringIO: write() and getvalue() ===
try:
    s = io.StringIO()
    s.write('hello')
    print('stringio_write', s.getvalue())

    s.write(' world')
    print('stringio_multiple_write', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: write() and getvalue()', type(e).__name__, e)

# === StringIO: writelines() ===
try:
    s = io.StringIO()
    s.writelines(['line1\n', 'line2\n', 'line3\n'])
    print('stringio_writelines', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: writelines()', type(e).__name__, e)

# === StringIO: read() after seek ===
try:
    s = io.StringIO('hello world')
    print('stringio_read_all', s.read())

    s.seek(0)
    print('stringio_read_after_seek', s.read())

    s.seek(0)
    print('stringio_read_size', s.read(5))
except Exception as e:
    print('SKIP_StringIO: read() after seek', type(e).__name__, e)

# === StringIO: readline() ===
try:
    s = io.StringIO('line1\nline2\nline3')
    print('stringio_readline_1', s.readline())
    print('stringio_readline_2', s.readline())

    s.seek(0)
    print('stringio_readline_size', s.readline(4))
except Exception as e:
    print('SKIP_StringIO: readline()', type(e).__name__, e)

# === StringIO: readlines() ===
try:
    s = io.StringIO('line1\nline2\nline3')
    lines = s.readlines()
    print('stringio_readlines', len(lines))
except Exception as e:
    print('SKIP_StringIO: readlines()', type(e).__name__, e)

# === StringIO: tell() position tracking ===
try:
    s = io.StringIO()
    print('stringio_tell_empty', s.tell())

    s.write('hello')
    print('stringio_tell_after_write', s.tell())

    s.read()
    print('stringio_tell_after_read_end', s.tell())
except Exception as e:
    print('SKIP_StringIO: tell() position tracking', type(e).__name__, e)

# === StringIO: with initial value ===
try:
    s = io.StringIO('initial content')
    print('stringio_initial_read', s.read())

    s.seek(0)
    s.write('modified')
    print('stringio_initial_append', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: with initial value', type(e).__name__, e)

# === StringIO: newline handling ===
try:
    s = io.StringIO(newline='\n')
    s.write('line1\nline2')
    print('stringio_newline_n', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: newline handling', type(e).__name__, e)

# === StringIO: seek() operations ===
try:
    s = io.StringIO('0123456789')
    print('stringio_seek_start_tell', s.tell())

    s.seek(5)
    print('stringio_seek_n', s.tell())

    s.seek(0)
    print('stringio_seek_zero', s.tell())

    s.seek(3)
    print('stringio_read_after_seek_n', s.read())
except Exception as e:
    print('SKIP_StringIO: seek() operations', type(e).__name__, e)

# === StringIO: seek with whence ===
try:
    s = io.StringIO('0123456789')
    s.seek(0, io.SEEK_END)
    print('stringio_seek_end', s.tell())

    # Note: StringIO doesn't support negative seeks from end
    # Just seek to absolute position and read
    s.seek(7)
    print('stringio_seek_end_offset', s.read())

    s.seek(5)
    s.seek(3)  # Absolute seek
    print('stringio_seek_absolute', s.tell())
except Exception as e:
    print('SKIP_StringIO: seek with whence', type(e).__name__, e)

# === StringIO: truncate() ===
try:
    s = io.StringIO('hello world')
    s.truncate(5)
    print('stringio_truncate_size', s.getvalue())

    s = io.StringIO('hello world')
    s.truncate()
    print('stringio_truncate_empty', s.getvalue())

    s = io.StringIO('test')
    s.truncate(10)
    print('stringio_truncate_expand', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: truncate()', type(e).__name__, e)

# === StringIO: readable(), writable(), seekable() ===
try:
    s = io.StringIO()
    print('stringio_readable', s.readable())
    print('stringio_writable', s.writable())
    print('stringio_seekable', s.seekable())
except Exception as e:
    print('SKIP_StringIO: readable(), writable(), seekable()', type(e).__name__, e)

# === StringIO: isatty() ===
try:
    s = io.StringIO()
    print('stringio_isatty', s.isatty())
except Exception as e:
    print('SKIP_StringIO: isatty()', type(e).__name__, e)

# === StringIO: flush() ===
try:
    s = io.StringIO()
    s.write('test')
    result = s.flush()
    print('stringio_flush_returns_none', result is None)
except Exception as e:
    print('SKIP_StringIO: flush()', type(e).__name__, e)

# === StringIO: close() ===
try:
    s = io.StringIO('test')
    s.close()
    print('stringio_closed', s.closed)

    # Test operations on closed StringIO raise ValueError
    try:
        s.read()
        print('stringio_read_closed_raises', False)
    except ValueError:
        print('stringio_read_closed_raises', True)
except Exception as e:
    print('SKIP_StringIO: close()', type(e).__name__, e)

# === StringIO: detach() raises UnsupportedOperation ===
try:
    s = io.StringIO()
    try:
        s.detach()
        print('stringio_detach_raises', False)
    except io.UnsupportedOperation:
        print('stringio_detach_raises', True)
except Exception as e:
    print('SKIP_StringIO: detach() raises UnsupportedOperation', type(e).__name__, e)

# === StringIO: fileno() raises UnsupportedOperation ===
try:
    s = io.StringIO()
    try:
        s.fileno()
        print('stringio_fileno_raises', False)
    except io.UnsupportedOperation:
        print('stringio_fileno_raises', True)
except Exception as e:
    print('SKIP_StringIO: fileno() raises UnsupportedOperation', type(e).__name__, e)

# === StringIO: context manager ===
try:
    with io.StringIO() as s:
        s.write('inside context')
        print('stringio_ctx_inside', s.getvalue())

    print('stringio_ctx_after_closed', s.closed)

    with io.StringIO('initial') as s:
        print('stringio_ctx_initial', s.read())
except Exception as e:
    print('SKIP_StringIO: context manager', type(e).__name__, e)

# === BytesIO: Basic construction ===
try:
    b = io.BytesIO()
    print('bytesio_empty_construction', b.getvalue())

    b = io.BytesIO(b'initial bytes')
    print('bytesio_initial_construction', b.getvalue())
except Exception as e:
    print('SKIP_BytesIO: Basic construction', type(e).__name__, e)

# === BytesIO: write() and getvalue() ===
try:
    b = io.BytesIO()
    b.write(b'hello')
    print('bytesio_write', b.getvalue())

    b.write(b' world')
    print('bytesio_multiple_write', b.getvalue())
except Exception as e:
    print('SKIP_BytesIO: write() and getvalue()', type(e).__name__, e)

# === BytesIO: writelines() ===
try:
    b = io.BytesIO()
    b.writelines([b'line1\n', b'line2\n', b'line3\n'])
    print('bytesio_writelines', b.getvalue())
except Exception as e:
    print('SKIP_BytesIO: writelines()', type(e).__name__, e)

# === BytesIO: read() after seek ===
try:
    b = io.BytesIO(b'hello world')
    print('bytesio_read_all', b.read())

    b.seek(0)
    print('bytesio_read_after_seek', b.read())

    b.seek(0)
    print('bytesio_read_size', b.read(5))
except Exception as e:
    print('SKIP_BytesIO: read() after seek', type(e).__name__, e)

# === BytesIO: readline() ===
try:
    b = io.BytesIO(b'line1\nline2\nline3')
    print('bytesio_readline_1', b.readline())
    print('bytesio_readline_2', b.readline())
except Exception as e:
    print('SKIP_BytesIO: readline()', type(e).__name__, e)

# === BytesIO: readlines() ===
try:
    b = io.BytesIO(b'line1\nline2\nline3')
    lines = b.readlines()
    print('bytesio_readlines', len(lines))
except Exception as e:
    print('SKIP_BytesIO: readlines()', type(e).__name__, e)

# === BytesIO: tell() and seek() ===
try:
    b = io.BytesIO()
    print('bytesio_tell_empty', b.tell())

    b.write(b'data')
    print('bytesio_tell_after_write', b.tell())

    b.seek(2)
    print('bytesio_tell_after_seek', b.tell())

    b.seek(0)
    print('bytesio_read_after_seek_zero', b.read())
except Exception as e:
    print('SKIP_BytesIO: tell() and seek()', type(e).__name__, e)

# === BytesIO: seek with whence ===
try:
    b = io.BytesIO(b'0123456789')
    b.seek(0, io.SEEK_END)
    print('bytesio_seek_end', b.tell())

    b.seek(-3, io.SEEK_END)
    print('bytesio_seek_end_offset', b.read())
except Exception as e:
    print('SKIP_BytesIO: seek with whence', type(e).__name__, e)

# === BytesIO: with initial value ===
try:
    b = io.BytesIO(b'initial content')
    print('bytesio_initial_read', b.read())

    b.seek(len(b.getvalue()))
    b.write(b' appended')
    print('bytesio_initial_append', b.getvalue())
except Exception as e:
    print('SKIP_BytesIO: with initial value', type(e).__name__, e)

# === BytesIO: truncate() ===
try:
    b = io.BytesIO(b'hello world')
    b.truncate(5)
    print('bytesio_truncate_size', b.getvalue())

    b = io.BytesIO(b'hello world')
    b.truncate()
    print('bytesio_truncate_empty', b.getvalue())
except Exception as e:
    print('SKIP_BytesIO: truncate()', type(e).__name__, e)

# === BytesIO: readable(), writable(), seekable() ===
try:
    b = io.BytesIO()
    print('bytesio_readable', b.readable())
    print('bytesio_writable', b.writable())
    print('bytesio_seekable', b.seekable())
except Exception as e:
    print('SKIP_BytesIO: readable(), writable(), seekable()', type(e).__name__, e)

# === BytesIO: isatty() ===
try:
    b = io.BytesIO()
    print('bytesio_isatty', b.isatty())
except Exception as e:
    print('SKIP_BytesIO: isatty()', type(e).__name__, e)

# === BytesIO: flush() ===
try:
    b = io.BytesIO()
    result = b.flush()
    print('bytesio_flush_returns_none', result is None)
except Exception as e:
    print('SKIP_BytesIO: flush()', type(e).__name__, e)

# === BytesIO: getbuffer() ===
try:
    b = io.BytesIO(b'hello world')
    buf = b.getbuffer()
    print('bytesio_getbuffer_len', len(buf))
    print('bytesio_getbuffer_slice', bytes(buf[:5]))
    del buf  # Release the buffer
except Exception as e:
    print('SKIP_BytesIO: getbuffer()', type(e).__name__, e)

# === BytesIO: read1() ===
try:
    b = io.BytesIO(b'hello world')
    print('bytesio_read1', b.read1(5))
except Exception as e:
    print('SKIP_BytesIO: read1()', type(e).__name__, e)

# === BytesIO: readinto() ===
try:
    b = io.BytesIO(b'hello')
    buf = bytearray(10)
    n = b.readinto(buf)
    print('bytesio_readinto', n)
    print('bytesio_readinto_buf', bytes(buf[:n]))
except Exception as e:
    print('SKIP_BytesIO: readinto()', type(e).__name__, e)

# === BytesIO: readinto1() ===
try:
    b = io.BytesIO(b'hello')
    buf = bytearray(10)
    n = b.readinto1(buf)
    print('bytesio_readinto1', n)
except Exception as e:
    print('SKIP_BytesIO: readinto1()', type(e).__name__, e)

# === BytesIO: close() ===
try:
    b = io.BytesIO(b'test')
    b.close()
    print('bytesio_closed', b.closed)

    # Test operations on closed BytesIO raise ValueError
    try:
        b.read()
        print('bytesio_read_closed_raises', False)
    except ValueError:
        print('bytesio_read_closed_raises', True)
except Exception as e:
    print('SKIP_BytesIO: close()', type(e).__name__, e)

# === BytesIO: detach() raises UnsupportedOperation ===
try:
    b = io.BytesIO()
    try:
        b.detach()
        print('bytesio_detach_raises', False)
    except io.UnsupportedOperation:
        print('bytesio_detach_raises', True)
except Exception as e:
    print('SKIP_BytesIO: detach() raises UnsupportedOperation', type(e).__name__, e)

# === BytesIO: fileno() raises UnsupportedOperation ===
try:
    b = io.BytesIO()
    try:
        b.fileno()
        print('bytesio_fileno_raises', False)
    except io.UnsupportedOperation:
        print('bytesio_fileno_raises', True)
except Exception as e:
    print('SKIP_BytesIO: fileno() raises UnsupportedOperation', type(e).__name__, e)

# === BytesIO: context manager ===
try:
    with io.BytesIO() as b:
        b.write(b'inside context')
        print('bytesio_ctx_inside', b.getvalue())

    print('bytesio_ctx_after_closed', b.closed)

    with io.BytesIO(b'initial') as b:
        print('bytesio_ctx_initial', b.read())
except Exception as e:
    print('SKIP_BytesIO: context manager', type(e).__name__, e)

# === StringIO: mixed operations ===
try:
    s = io.StringIO()
    s.write('first line\n')
    s.write('second line\n')
    print('stringio_mixed_lines', s.getvalue())

    s.seek(0)
    print('stringio_mixed_read', s.read())

    s.seek(6)
    s.write('MODIFIED')
    print('stringio_mixed_overwrite', s.getvalue())
except Exception as e:
    print('SKIP_StringIO: mixed operations', type(e).__name__, e)

# === BytesIO: mixed operations ===
try:
    b = io.BytesIO()
    b.write(b'\x00\x01\x02')
    b.write(b'\x03\x04\x05')
    print('bytesio_mixed_write', b.getvalue())

    b.seek(2)
    print('bytesio_mixed_read_partial', b.read(2))

    b.seek(0)
    print('bytesio_mixed_read_all', b.read())
except Exception as e:
    print('SKIP_BytesIO: mixed operations', type(e).__name__, e)

# === StringIO: empty read ===
try:
    s = io.StringIO()
    print('stringio_empty_read', repr(s.read()))

    s.write('data')
    print('stringio_read_after_write_no_seek', repr(s.read()))

    s.seek(0)
    print('stringio_empty_read_size_zero', repr(s.read(0)))
except Exception as e:
    print('SKIP_StringIO: empty read', type(e).__name__, e)

# === BytesIO: empty read ===
try:
    b = io.BytesIO()
    print('bytesio_empty_read', b.read())

    b.write(b'data')
    print('bytesio_read_after_write_no_seek', b.read())

    b.seek(0)
    print('bytesio_empty_read_size_zero', b.read(0))
except Exception as e:
    print('SKIP_BytesIO: empty read', type(e).__name__, e)

# === Exceptions ===
try:
    print('unsupported_operation_exists', hasattr(io, 'UnsupportedOperation'))
    print('unsupported_operation_is_io_error', issubclass(io.UnsupportedOperation, Exception))
except Exception as e:
    print('SKIP_Exceptions', type(e).__name__, e)
