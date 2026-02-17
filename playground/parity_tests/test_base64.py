import base64

# === b64encode ===
try:
    # basic encoding
    print('b64encode_basic', base64.b64encode(b'hello'))
    print('b64encode_empty', base64.b64encode(b''))
    print('b64encode_binary', base64.b64encode(b'\x00\x01\x02\xff'))
    print('b64encode_long', base64.b64encode(b'hello world this is a longer string to encode'))
except Exception as e:
    print('SKIP_b64encode', type(e).__name__, e)

# === b64decode ===
try:
    # basic decoding
    print('b64decode_basic', base64.b64decode(b'aGVsbG8='))
    print('b64decode_empty', base64.b64decode(b''))
    print('b64decode_binary', base64.b64decode(b'AAEC/w=='))
    print('b64decode_string', base64.b64decode('aGVsbG8='))
except Exception as e:
    print('SKIP_b64decode', type(e).__name__, e)

# === standard_b64encode ===
try:
    print('standard_b64encode_basic', base64.standard_b64encode(b'hello'))
    print('standard_b64encode_empty', base64.standard_b64encode(b''))
    print('standard_b64encode_binary', base64.standard_b64encode(b'\x00\x01\x02\xff'))
except Exception as e:
    print('SKIP_standard_b64encode', type(e).__name__, e)

# === standard_b64decode ===
try:
    print('standard_b64decode_basic', base64.standard_b64decode(b'aGVsbG8='))
    print('standard_b64decode_empty', base64.standard_b64decode(b''))
    print('standard_b64decode_string', base64.standard_b64decode('aGVsbG8='))
except Exception as e:
    print('SKIP_standard_b64decode', type(e).__name__, e)

# === urlsafe_b64encode ===
try:
    print('urlsafe_b64encode_basic', base64.urlsafe_b64encode(b'hello'))
    print('urlsafe_b64encode_plus_slash', base64.urlsafe_b64encode(b'\xfb\xff'))
    print('urlsafe_b64encode_empty', base64.urlsafe_b64encode(b''))
except Exception as e:
    print('SKIP_urlsafe_b64encode', type(e).__name__, e)

# === urlsafe_b64decode ===
try:
    print('urlsafe_b64decode_basic', base64.urlsafe_b64decode(b'aGVsbG8='))
    print('urlsafe_b64decode_minus_underscore', base64.urlsafe_b64decode(b'--8='))
    print('urlsafe_b64decode_empty', base64.urlsafe_b64decode(b''))
except Exception as e:
    print('SKIP_urlsafe_b64decode', type(e).__name__, e)

# === b32encode ===
try:
    print('b32encode_basic', base64.b32encode(b'hello'))
    print('b32encode_empty', base64.b32encode(b''))
    print('b32encode_binary', base64.b32encode(b'\x00\x01\x02\xff'))
except Exception as e:
    print('SKIP_b32encode', type(e).__name__, e)

# === b32decode ===
try:
    print('b32decode_basic', base64.b32decode(b'NBSWY3DP'))
    print('b32decode_empty', base64.b32decode(b''))
    print('b32decode_casefold', base64.b32decode('nbswy3dp', casefold=True))
    print('b32decode_map01', base64.b32decode(b'LO234231', map01=b'I'))
except Exception as e:
    print('SKIP_b32decode', type(e).__name__, e)

# === b32hexencode ===
try:
    print('b32hexencode_basic', base64.b32hexencode(b'hello'))
    print('b32hexencode_empty', base64.b32hexencode(b''))
    print('b32hexencode_binary', base64.b32hexencode(b'\x00\x01\x02\xff'))
except Exception as e:
    print('SKIP_b32hexencode', type(e).__name__, e)

# === b32hexdecode ===
try:
    print('b32hexdecode_basic', base64.b32hexdecode(b'91IMOR3F'))
    print('b32hexdecode_empty', base64.b32hexdecode(b''))
    print('b32hexdecode_casefold', base64.b32hexdecode('91imor3f', casefold=True))
except Exception as e:
    print('SKIP_b32hexdecode', type(e).__name__, e)

# === b16encode ===
try:
    print('b16encode_basic', base64.b16encode(b'hello'))
    print('b16encode_empty', base64.b16encode(b''))
    print('b16encode_binary', base64.b16encode(b'\x00\x01\x02\xff'))
except Exception as e:
    print('SKIP_b16encode', type(e).__name__, e)

# === b16decode ===
try:
    print('b16decode_basic', base64.b16decode(b'68656C6C6F'))
    print('b16decode_empty', base64.b16decode(b''))
    print('b16decode_casefold', base64.b16decode('68656c6c6f', casefold=True))
except Exception as e:
    print('SKIP_b16decode', type(e).__name__, e)

# === a85encode ===
try:
    print('a85encode_basic', base64.a85encode(b'hello'))
    print('a85encode_empty', base64.a85encode(b''))
    print('a85encode_binary', base64.a85encode(b'\x00\x01\x02\xff'))
    print('a85encode_all_zeros', base64.a85encode(b'\x00\x00\x00\x00'))
    print('a85encode_foldspaces', base64.a85encode(b'    ', foldspaces=True))
    print('a85encode_wrap', base64.a85encode(b'hello world ' * 5, wrapcol=20))
    print('a85encode_adobe', base64.a85encode(b'hello', adobe=True))
except Exception as e:
    print('SKIP_a85encode', type(e).__name__, e)

# === a85decode ===
try:
    print('a85decode_basic', base64.a85decode(b'BOu!rDZ'))
    print('a85decode_empty', base64.a85decode(b''))
    print('a85decode_all_z', base64.a85decode(b'z'))
    print('a85decode_foldspaces', base64.a85decode(b'y', foldspaces=True))
    print('a85decode_adobe', base64.a85decode(b'<~BOu!rDZ~>', adobe=True))
except Exception as e:
    print('SKIP_a85decode', type(e).__name__, e)

# === b85encode ===
try:
    print('b85encode_basic', base64.b85encode(b'hello'))
    print('b85encode_empty', base64.b85encode(b''))
    print('b85encode_binary', base64.b85encode(b'\x00\x01\x02\xff'))
    print('b85encode_pad', base64.b85encode(b'h', pad=True))
except Exception as e:
    print('SKIP_b85encode', type(e).__name__, e)

# === b85decode ===
try:
    print('b85decode_basic', base64.b85decode(b'Wq)SV='))
    print('b85decode_empty', base64.b85decode(b''))
except Exception as e:
    print('SKIP_b85decode', type(e).__name__, e)

# === z85encode ===
try:
    print('z85encode_basic', base64.z85encode(b'hello!'))
    print('z85encode_empty', base64.z85encode(b''))
    print('z85encode_8bytes', base64.z85encode(b'\x00\x00\x00\x00\x00\x00\x00\x00'))
except Exception as e:
    print('SKIP_z85encode', type(e).__name__, e)

# === z85decode ===
try:
    print('z85decode_basic', base64.z85decode(b'nm=QNz'))
    print('z85decode_empty', base64.z85decode(b''))
except Exception as e:
    print('SKIP_z85decode', type(e).__name__, e)

# === encodebytes ===
try:
    print('encodebytes_basic', base64.encodebytes(b'hello'))
    print('encodebytes_empty', base64.encodebytes(b''))
    print('encodebytes_multiline', base64.encodebytes(b'hello world\nthis is line 2\n'))
except Exception as e:
    print('SKIP_encodebytes', type(e).__name__, e)

# === decodebytes ===
try:
    print('decodebytes_basic', base64.decodebytes(b'aGVsbG8=\n'))
    print('decodebytes_empty', base64.decodebytes(b''))
    print('decodebytes_multiline', base64.decodebytes(b'aGVsbG8gd29ybGQK\ndGhpcyBpcyBsaW5lIDIK\n'))
except Exception as e:
    print('SKIP_decodebytes', type(e).__name__, e)

# === encode (legacy interface) ===
try:
    import io
    out = io.BytesIO()
    base64.encode(io.BytesIO(b'hello'), out)
    print('encode_basic', out.getvalue())

    out2 = io.BytesIO()
    base64.encode(io.BytesIO(b''), out2)
    print('encode_empty', out2.getvalue())
except Exception as e:
    print('SKIP_encode (legacy interface)', type(e).__name__, e)

# === decode (legacy interface) ===
try:
    out3 = io.BytesIO()
    base64.decode(io.BytesIO(b'aGVsbG8=\n'), out3)
    print('decode_basic', out3.getvalue())

    out4 = io.BytesIO()
    base64.decode(io.BytesIO(b''), out4)
    print('decode_empty', out4.getvalue())
except Exception as e:
    print('SKIP_decode (legacy interface)', type(e).__name__, e)

# === b64encode with altchars ===
try:
    print('b64encode_altchars', base64.b64encode(b'\xfb\xff\xfc\xfd', altchars=b'_-'))
except Exception as e:
    print('SKIP_b64encode with altchars', type(e).__name__, e)

# === b64decode with altchars ===
try:
    print('b64decode_altchars', base64.b64decode(b'----8P39', altchars=b'_-'))
except Exception as e:
    print('SKIP_b64decode with altchars', type(e).__name__, e)

# === b64decode with validate ===
try:
    print('b64decode_validate_true', base64.b64decode(b'aGVsbG8=', validate=True))
    print('b64decode_validate_false', base64.b64decode(b'aGVsbG8='))
except Exception as e:
    print('SKIP_b64decode with validate', type(e).__name__, e)

# === constants ===
try:
    print('MAXBINSIZE', base64.MAXBINSIZE)
    print('MAXLINESIZE', base64.MAXLINESIZE)
except Exception as e:
    print('SKIP_constants', type(e).__name__, e)
