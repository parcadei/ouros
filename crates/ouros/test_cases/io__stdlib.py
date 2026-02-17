import io


# === Module-level parity ===
assert io.open is open
assert isinstance(io.DEFAULT_BUFFER_SIZE, int)
assert io.DEFAULT_BUFFER_SIZE == 131072
assert isinstance(io.TextIOBase, type)
assert isinstance(io.RawIOBase, type)
assert isinstance(io.BufferedIOBase, type)

missing = "__ouros_io_missing_file__"
try:
    io.open(missing)
    assert False, "io.open should behave like builtin open"
except FileNotFoundError as exc:
    assert missing in str(exc)


# === StringIO constructor/type validation ===
s = io.StringIO(None)
assert s.getvalue() == ""
s.close()

try:
    io.StringIO(123)
    assert False, "StringIO should reject non-str/non-None initial value"
except TypeError as exc:
    assert "initial_value must be str or None" in str(exc)

try:
    io.StringIO("a", initial_value="b")
    assert False, "duplicate initial_value should fail"
except TypeError as exc:
    msg = str(exc)
    assert ("multiple values" in msg) or ("position (1)" in msg)

for newline in (None, "", "\n", "\r", "\r\n"):
    s = io.StringIO("x", newline=newline)
    assert s.getvalue() == "x"
    s.close()

try:
    io.StringIO("", newline=1)
    assert False, "newline must be str or None"
except TypeError as exc:
    assert "newline must be str or None" in str(exc)

try:
    io.StringIO("", newline="bad")
    assert False, "invalid newline must raise ValueError"
except ValueError as exc:
    assert "illegal newline value" in str(exc)


# === StringIO semantics ===
s = io.StringIO("abc")
s.seek(5)
assert s.write("Z") == 1
assert s.tell() == 6
assert s.getvalue() == "abc\0\0Z"
s.close()

s = io.StringIO("abc")
s.seek(5)
assert s.write("") == 0
assert s.tell() == 5
assert s.getvalue() == "abc"
s.close()

s = io.StringIO("caf\u00e9")
assert s.read(3) == "caf"
assert s.read() == "\u00e9"
s.seek(3)
assert s.write("X") == 1
assert s.getvalue() == "cafX"
s.close()

s = io.StringIO("a\nb\nccc\n")
assert s.readlines(1) == ["a\n"]
s.seek(0)
assert s.readlines(2) == ["a\n", "b\n"]
s.seek(0)
assert s.readlines(4) == ["a\n", "b\n", "ccc\n"]
s.close()

try:
    io.StringIO().write(1)
    assert False, "write() must only accept str"
except TypeError as exc:
    assert "string argument expected" in str(exc)

try:
    io.StringIO().writelines([1])
    assert False, "writelines() must only accept str elements"
except TypeError as exc:
    assert "string argument expected" in str(exc)

s = io.StringIO("line1\nline2")
assert s.__iter__() is s
assert next(s) == "line1\n"
assert next(s) == "line2"
try:
    next(s)
    assert False
except StopIteration:
    pass

s = io.StringIO("x")
assert s.__exit__(None, None, None) is None
assert s.closed


# === BytesIO constructor/type validation ===
b = io.BytesIO(bytearray(b"abc"))
assert b.getvalue() == b"abc"
b.close()

try:
    io.BytesIO(123)
    assert False, "BytesIO should reject non-bytes-like initial value"
except TypeError as exc:
    assert "bytes-like object" in str(exc)

try:
    io.BytesIO(b"a", initial_bytes=b"b")
    assert False, "duplicate initial_bytes should fail"
except TypeError as exc:
    msg = str(exc)
    assert ("multiple values" in msg) or ("at most 1 argument" in msg)


# === BytesIO semantics ===
b = io.BytesIO(b"abc")
b.seek(5)
assert b.write(b"Z") == 1
assert b.tell() == 6
assert b.getvalue() == b"abc\x00\x00Z"
b.close()

b = io.BytesIO(b"abc")
b.seek(5)
assert b.write(b"") == 0
assert b.tell() == 5
assert b.getvalue() == b"abc"
b.close()

b = io.BytesIO(b"a\nb\nccc\n")
assert b.readlines(1) == [b"a\n"]
b.seek(0)
assert b.readlines(2) == [b"a\n"]
b.seek(0)
assert b.readlines(4) == [b"a\n", b"b\n"]
b.close()

b = io.BytesIO()
assert b.write(bytearray(b"xy")) == 2
assert b.getvalue() == b"xy"
b.close()

try:
    io.BytesIO().write(1)
    assert False, "BytesIO.write() must only accept bytes-like values"
except TypeError as exc:
    assert "bytes-like object" in str(exc)

try:
    io.BytesIO().writelines([1])
    assert False, "BytesIO.writelines() must only accept bytes-like elements"
except TypeError as exc:
    assert "bytes-like object" in str(exc)

b = io.BytesIO(b"line1\nline2")
assert b.__iter__() is b
assert next(b) == b"line1\n"
assert next(b) == b"line2"
try:
    next(b)
    assert False
except StopIteration:
    pass

b = io.BytesIO(b"abc")
view = b.getbuffer()
assert bytes(view) == b"abc"
try:
    b.close()
    assert False, "close should fail while exported buffer is live"
except BufferError:
    pass
del view
b.close()
assert b.closed

b = io.BytesIO(b"x")
assert b.__exit__(None, None, None) is None
assert b.closed
