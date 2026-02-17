import uuid

# === SafeUUID enum values ===
try:
    print('safeuuid_safe', uuid.SafeUUID.safe)
    print('safeuuid_unsafe', uuid.SafeUUID.unsafe)
    print('safeuuid_unknown', uuid.SafeUUID.unknown)
except Exception as e:
    print('SKIP_SafeUUID enum values', type(e).__name__, e)

# === UUID class constructors ===
try:
    # From hex string with hyphens
    u_hex = uuid.UUID('12345678-1234-5678-1234-567812345678')
    print('uuid_from_hex', u_hex)

    # From hex string without hyphens
    u_hex_no_dash = uuid.UUID('12345678123456781234567812345678')
    print('uuid_from_hex_no_dash', u_hex_no_dash)

    # From hex with curly braces
    u_hex_braces = uuid.UUID('{12345678-1234-5678-1234-567812345678}')
    print('uuid_from_hex_braces', u_hex_braces)

    # From URN string
    u_urn = uuid.UUID('urn:uuid:12345678-1234-5678-1234-567812345678')
    print('uuid_from_urn', u_urn)

    # From bytes
    u_bytes = uuid.UUID(bytes=b'\x12\x34\x56\x78' * 4)
    print('uuid_from_bytes', u_bytes)

    # From bytes_le (little-endian)
    u_bytes_le = uuid.UUID(bytes_le=b'\x78\x56\x34\x12\x34\x12\x78\x56' +
                                  b'\x12\x34\x56\x78\x12\x34\x56\x78')
    print('uuid_from_bytes_le', u_bytes_le)

    # From fields tuple
    u_fields = uuid.UUID(fields=(0x12345678, 0x1234, 0x5678, 0x12, 0x34, 0x567812345678))
    print('uuid_from_fields', u_fields)

    # From int
    u_int = uuid.UUID(int=0x12345678123456781234567812345678)
    print('uuid_from_int', u_int)

    # With version override
    u_version = uuid.UUID('12345678-1234-5678-1234-567812345678', version=4)
    print('uuid_version_override', u_version.version)
except Exception as e:
    print('SKIP_UUID class constructors', type(e).__name__, e)

# === UUID attributes ===
try:
    u = uuid.UUID('12345678-1234-5678-1234-567812345678')
    print('uuid_bytes', u.bytes)
    print('uuid_bytes_le', u.bytes_le)
    print('uuid_hex', u.hex)
    print('uuid_int', u.int)
    print('uuid_urn', u.urn)
    print('uuid_variant', u.variant)
    print('uuid_version_none', u.version)
    print('uuid_is_safe', u.is_safe)

    # UUID fields
    print('uuid_fields', u.fields)
    print('uuid_time_low', u.time_low)
    print('uuid_time_mid', u.time_mid)
    print('uuid_time_hi_version', u.time_hi_version)
    print('uuid_clock_seq_hi_variant', u.clock_seq_hi_variant)
    print('uuid_clock_seq_low', u.clock_seq_low)
    print('uuid_node', u.node)
    print('uuid_time', u.time)
    print('uuid_clock_seq', u.clock_seq)

    # UUID string representation
    print('uuid_str', str(u))
    print('uuid_repr', repr(u))
except Exception as e:
    print('SKIP_UUID attributes', type(e).__name__, e)

# === UUID comparison ===
try:
    u1 = uuid.UUID('12345678-1234-5678-1234-567812345678')
    u2 = uuid.UUID('12345678-1234-5678-1234-567812345678')
    u3 = uuid.UUID('87654321-4321-8765-4321-876543218765')
    print('uuid_eq', u1 == u2)
    print('uuid_ne', u1 != u3)
    print('uuid_lt', u1 < u3)
    print('uuid_le', u1 <= u2)
    print('uuid_gt', u3 > u1)
    print('uuid_ge', u2 >= u1)
except Exception as e:
    print('SKIP_UUID comparison', type(e).__name__, e)

# === Hash ===
try:
    print('uuid_hash', hash(u1))
except Exception as e:
    print('SKIP_Hash', type(e).__name__, e)

# === NAMESPACE constants ===
try:
    print('namespace_dns', uuid.NAMESPACE_DNS)
    print('namespace_url', uuid.NAMESPACE_URL)
    print('namespace_oid', uuid.NAMESPACE_OID)
    print('namespace_x500', uuid.NAMESPACE_X500)
except Exception as e:
    print('SKIP_NAMESPACE constants', type(e).__name__, e)

# === NIL constant ===
try:
    print('nil_uuid', uuid.NIL)
    print('nil_uuid_hex', uuid.NIL.hex)
    print('nil_uuid_int', uuid.NIL.int)
except Exception as e:
    print('SKIP_NIL constant', type(e).__name__, e)

# === Variant constants ===
try:
    print('reserved_ncs', uuid.RESERVED_NCS)
    print('rfc_4122', uuid.RFC_4122)
    print('reserved_microsoft', uuid.RESERVED_MICROSOFT)
    print('reserved_future', uuid.RESERVED_FUTURE)
except Exception as e:
    print('SKIP_Variant constants', type(e).__name__, e)

# === getnode ===
try:
    node = uuid.getnode()
    print('getnode_type', type(node).__name__)
    print('getnode_positive', node > 0)
except Exception as e:
    print('SKIP_getnode', type(e).__name__, e)

# === uuid1 ===
try:
    u1_gen = uuid.uuid1()
    print('uuid1_version', u1_gen.version)
    print('uuid1_variant', u1_gen.variant)
    print('uuid1_node', u1_gen.node)
    print('uuid1_is_safe_type', type(u1_gen.is_safe).__name__)

    # uuid1 with explicit node and clock_seq
    u1_explicit = uuid.uuid1(node=0x1234567890ab, clock_seq=0x1234)
    print('uuid1_explicit_version', u1_explicit.version)
    print('uuid1_explicit_node', hex(u1_explicit.node))
except Exception as e:
    print('SKIP_uuid1', type(e).__name__, e)

# === uuid3 ===
try:
    u3_dns = uuid.uuid3(uuid.NAMESPACE_DNS, 'example.com')
    print('uuid3_version', u3_dns.version)
    print('uuid3_variant', u3_dns.variant)
    print('uuid3_deterministic', uuid.uuid3(uuid.NAMESPACE_DNS, 'example.com') == u3_dns)

    # uuid3 with bytes name
    u3_bytes = uuid.uuid3(uuid.NAMESPACE_DNS, b'example.com')
    print('uuid3_bytes_same', u3_bytes == u3_dns)
except Exception as e:
    print('SKIP_uuid3', type(e).__name__, e)

# === uuid4 ===
try:
    u4_gen = uuid.uuid4()
    print('uuid4_version', u4_gen.version)
    print('uuid4_variant', u4_gen.variant)
except Exception as e:
    print('SKIP_uuid4', type(e).__name__, e)

# === uuid5 ===
try:
    u5_dns = uuid.uuid5(uuid.NAMESPACE_DNS, 'example.com')
    print('uuid5_version', u5_dns.version)
    print('uuid5_variant', u5_dns.variant)
    print('uuid5_deterministic', uuid.uuid5(uuid.NAMESPACE_DNS, 'example.com') == u5_dns)

    # uuid5 with bytes name
    u5_bytes = uuid.uuid5(uuid.NAMESPACE_DNS, b'example.com')
    print('uuid5_bytes_same', u5_bytes == u5_dns)

    # uuid5 vs uuid3 different
    print('uuid3_vs_uuid5_different', u3_dns != u5_dns)
except Exception as e:
    print('SKIP_uuid5', type(e).__name__, e)

# === uuid6 (Python 3.14+) ===
try:
    u6_gen = uuid.uuid6()
    print('uuid6_version', u6_gen.version)
    print('uuid6_variant', u6_gen.variant)
    print('uuid6_node', u6_gen.node)

    # uuid6 with explicit node and clock_seq
    u6_explicit = uuid.uuid6(node=0x1234567890ab, clock_seq=0x1234)
    print('uuid6_explicit_version', u6_explicit.version)
    print('uuid6_explicit_node', hex(u6_explicit.node))
except Exception as e:
    print('SKIP_uuid6 (Python 3.14+)', type(e).__name__, e)

# === uuid7 (Python 3.14+) ===
try:
    u7_gen = uuid.uuid7()
    print('uuid7_version', u7_gen.version)
    print('uuid7_variant', u7_gen.variant)
    print('uuid7_time', u7_gen.time)
except Exception as e:
    print('SKIP_uuid7 (Python 3.14+)', type(e).__name__, e)

# === uuid8 (Python 3.14+) ===
try:
    u8_gen = uuid.uuid8()
    print('uuid8_version', u8_gen.version)
    print('uuid8_variant', u8_gen.variant)

    # uuid8 with explicit parameters
    u8_explicit = uuid.uuid8(a=0x1234567890ab, b=0x1234567890ab, c=0x1234567890ab)
    print('uuid8_explicit_version', u8_explicit.version)
except Exception as e:
    print('SKIP_uuid8 (Python 3.14+)', type(e).__name__, e)

# === Different namespaces produce different results ===
try:
    print('uuid3_dns_vs_url', uuid.uuid3(uuid.NAMESPACE_DNS, 'example.com') != uuid.uuid3(uuid.NAMESPACE_URL, 'example.com'))
    print('uuid5_dns_vs_url', uuid.uuid5(uuid.NAMESPACE_DNS, 'example.com') != uuid.uuid5(uuid.NAMESPACE_URL, 'example.com'))
except Exception as e:
    print('SKIP_Different namespaces produce different results', type(e).__name__, e)

# === UUID v3/v5 known test vectors ===
try:
    # Test with empty string
    u3_empty = uuid.uuid3(uuid.NAMESPACE_DNS, '')
    print('uuid3_empty_version', u3_empty.version)
    u5_empty = uuid.uuid5(uuid.NAMESPACE_DNS, '')
    print('uuid5_empty_version', u5_empty.version)

    # Test with unicode
    u3_unicode = uuid.uuid3(uuid.NAMESPACE_DNS, 'tëst.example.com')
    print('uuid3_unicode_version', u3_unicode.version)
    u5_unicode = uuid.uuid5(uuid.NAMESPACE_DNS, 'tëst.example.com')
    print('uuid5_unicode_version', u5_unicode.version)
except Exception as e:
    print('SKIP_UUID v3/v5 known test vectors', type(e).__name__, e)

# === Type checking ===
try:
    print('uuid_isinstance', isinstance(uuid.uuid4(), uuid.UUID))
except Exception as e:
    print('SKIP_Type checking', type(e).__name__, e)

# === MAX constant (Python 3.14+) ===
try:
    print('max_uuid', uuid.MAX)
    print('max_uuid_int', uuid.MAX.int)
    print('max_uuid_hex', uuid.MAX.hex)
except Exception as e:
    print('SKIP_MAX constant (Python 3.14+)', type(e).__name__, e)
