import hashlib
import io

# === Module Attributes ===
try:
    print('algorithms_guaranteed_count', len(hashlib.algorithms_guaranteed))
    print('algorithms_guaranteed_contains_md5', 'md5' in hashlib.algorithms_guaranteed)
    print('algorithms_guaranteed_contains_sha1', 'sha1' in hashlib.algorithms_guaranteed)
    print('algorithms_guaranteed_contains_sha256', 'sha256' in hashlib.algorithms_guaranteed)
    print('algorithms_guaranteed_contains_sha3_256', 'sha3_256' in hashlib.algorithms_guaranteed)
    print('algorithms_guaranteed_contains_blake2b', 'blake2b' in hashlib.algorithms_guaranteed)
    print('algorithms_available_count', len(hashlib.algorithms_available))
    print('algorithms_available_is_superset', hashlib.algorithms_guaranteed.issubset(hashlib.algorithms_available))
except Exception as e:
    print('SKIP_Module_Attributes', type(e).__name__, e)

# === md5 ===
try:
    print('md5_hex_empty', hashlib.md5(b'').hexdigest())
    print('md5_hex_hello', hashlib.md5(b'hello').hexdigest())
    print('md5_hex_unicode', hashlib.md5('hello world'.encode('utf-8')).hexdigest())
    print('md5_digest_size', hashlib.md5().digest_size)
    print('md5_block_size', hashlib.md5().block_size)
    print('md5_name', hashlib.md5().name)
except Exception as e:
    print('SKIP_md5', type(e).__name__, e)

# === sha1 ===
try:
    print('sha1_hex_empty', hashlib.sha1(b'').hexdigest())
    print('sha1_hex_hello', hashlib.sha1(b'hello').hexdigest())
    print('sha1_digest_size', hashlib.sha1().digest_size)
    print('sha1_block_size', hashlib.sha1().block_size)
    print('sha1_name', hashlib.sha1().name)
except Exception as e:
    print('SKIP_sha1', type(e).__name__, e)

# === sha224 ===
try:
    print('sha224_hex_empty', hashlib.sha224(b'').hexdigest())
    print('sha224_hex_hello', hashlib.sha224(b'hello').hexdigest())
    print('sha224_digest_size', hashlib.sha224().digest_size)
    print('sha224_block_size', hashlib.sha224().block_size)
    print('sha224_name', hashlib.sha224().name)
except Exception as e:
    print('SKIP_sha224', type(e).__name__, e)

# === sha256 ===
try:
    print('sha256_hex_empty', hashlib.sha256(b'').hexdigest())
    print('sha256_hex_hello', hashlib.sha256(b'hello').hexdigest())
    print('sha256_hex_long', hashlib.sha256(b'Nobody inspects the spammish repetition').hexdigest())
    print('sha256_digest_size', hashlib.sha256().digest_size)
    print('sha256_block_size', hashlib.sha256().block_size)
    print('sha256_name', hashlib.sha256().name)
except Exception as e:
    print('SKIP_sha256', type(e).__name__, e)

# === sha384 ===
try:
    print('sha384_hex_empty', hashlib.sha384(b'').hexdigest())
    print('sha384_hex_hello', hashlib.sha384(b'hello').hexdigest())
    print('sha384_digest_size', hashlib.sha384().digest_size)
    print('sha384_block_size', hashlib.sha384().block_size)
    print('sha384_name', hashlib.sha384().name)
except Exception as e:
    print('SKIP_sha384', type(e).__name__, e)

# === sha512 ===
try:
    print('sha512_hex_empty', hashlib.sha512(b'').hexdigest())
    print('sha512_hex_hello', hashlib.sha512(b'hello').hexdigest())
    print('sha512_digest_size', hashlib.sha512().digest_size)
    print('sha512_block_size', hashlib.sha512().block_size)
    print('sha512_name', hashlib.sha512().name)
except Exception as e:
    print('SKIP_sha512', type(e).__name__, e)

# === sha3_224 ===
try:
    print('sha3_224_hex_empty', hashlib.sha3_224(b'').hexdigest())
    print('sha3_224_hex_hello', hashlib.sha3_224(b'hello').hexdigest())
    print('sha3_224_digest_size', hashlib.sha3_224().digest_size)
    print('sha3_224_block_size', hashlib.sha3_224().block_size)
    print('sha3_224_name', hashlib.sha3_224().name)
except Exception as e:
    print('SKIP_sha3_224', type(e).__name__, e)

# === sha3_256 ===
try:
    print('sha3_256_hex_empty', hashlib.sha3_256(b'').hexdigest())
    print('sha3_256_hex_hello', hashlib.sha3_256(b'hello').hexdigest())
    print('sha3_256_digest_size', hashlib.sha3_256().digest_size)
    print('sha3_256_block_size', hashlib.sha3_256().block_size)
    print('sha3_256_name', hashlib.sha3_256().name)
except Exception as e:
    print('SKIP_sha3_256', type(e).__name__, e)

# === sha3_384 ===
try:
    print('sha3_384_hex_empty', hashlib.sha3_384(b'').hexdigest())
    print('sha3_384_hex_hello', hashlib.sha3_384(b'hello').hexdigest())
    print('sha3_384_digest_size', hashlib.sha3_384().digest_size)
    print('sha3_384_block_size', hashlib.sha3_384().block_size)
    print('sha3_384_name', hashlib.sha3_384().name)
except Exception as e:
    print('SKIP_sha3_384', type(e).__name__, e)

# === sha3_512 ===
try:
    print('sha3_512_hex_empty', hashlib.sha3_512(b'').hexdigest())
    print('sha3_512_hex_hello', hashlib.sha3_512(b'hello').hexdigest())
    print('sha3_512_digest_size', hashlib.sha3_512().digest_size)
    print('sha3_512_block_size', hashlib.sha3_512().block_size)
    print('sha3_512_name', hashlib.sha3_512().name)
except Exception as e:
    print('SKIP_sha3_512', type(e).__name__, e)

# === shake_128 ===
try:
    print('shake_128_hex_16', hashlib.shake_128(b'hello').hexdigest(16))
    print('shake_128_hex_32', hashlib.shake_128(b'hello').hexdigest(32))
    print('shake_128_hex_64', hashlib.shake_128(b'hello').hexdigest(64))
    print('shake_128_digest_16', hashlib.shake_128(b'hello').digest(16).hex())
    print('shake_128_block_size', hashlib.shake_128().block_size)
    print('shake_128_name', hashlib.shake_128().name)
except Exception as e:
    print('SKIP_shake_128', type(e).__name__, e)

# === shake_256 ===
try:
    print('shake_256_hex_16', hashlib.shake_256(b'hello').hexdigest(16))
    print('shake_256_hex_32', hashlib.shake_256(b'hello').hexdigest(32))
    print('shake_256_hex_64', hashlib.shake_256(b'hello').hexdigest(64))
    print('shake_256_hex_128', hashlib.shake_256(b'hello').hexdigest(128))
    print('shake_256_digest_32', hashlib.shake_256(b'hello').digest(32).hex())
    print('shake_256_block_size', hashlib.shake_256().block_size)
    print('shake_256_name', hashlib.shake_256().name)
except Exception as e:
    print('SKIP_shake_256', type(e).__name__, e)

# === blake2b ===
try:
    print('blake2b_hex_empty', hashlib.blake2b(b'').hexdigest())
    print('blake2b_hex_hello', hashlib.blake2b(b'hello').hexdigest())
    print('blake2b_digest_size', hashlib.blake2b().digest_size)
    print('blake2b_block_size', hashlib.blake2b().block_size)
    print('blake2b_name', hashlib.blake2b().name)
    print('blake2b_digest_size_32', hashlib.blake2b(b'hello', digest_size=32).hexdigest())
    print('blake2b_digest_size_64', hashlib.blake2b(b'hello', digest_size=64).hexdigest())
except Exception as e:
    print('SKIP_blake2b', type(e).__name__, e)

# === blake2s ===
try:
    print('blake2s_hex_empty', hashlib.blake2s(b'').hexdigest())
    print('blake2s_hex_hello', hashlib.blake2s(b'hello').hexdigest())
    print('blake2s_digest_size', hashlib.blake2s().digest_size)
    print('blake2s_block_size', hashlib.blake2s().block_size)
    print('blake2s_name', hashlib.blake2s().name)
    print('blake2s_digest_size_16', hashlib.blake2s(b'hello', digest_size=16).hexdigest())
    print('blake2s_digest_size_32', hashlib.blake2s(b'hello', digest_size=32).hexdigest())
except Exception as e:
    print('SKIP_blake2s', type(e).__name__, e)

# === new() constructor ===
try:
    print('new_sha256_hex', hashlib.new('sha256', b'hello').hexdigest())
    print('new_md5_hex', hashlib.new('md5', b'hello').hexdigest())
    print('new_sha1_hex', hashlib.new('sha1', b'hello').hexdigest())
    print('new_sha3_256_hex', hashlib.new('sha3_256', b'hello').hexdigest())
    print('new_blake2b_hex', hashlib.new('blake2b', b'hello').hexdigest())
except Exception as e:
    print('SKIP_new()_constructor', type(e).__name__, e)

# === update() method ===
try:
    m = hashlib.sha256()
    m.update(b'hello')
    print('update_single', m.hexdigest())
    m = hashlib.sha256()
    m.update(b'hel')
    m.update(b'lo')
    print('update_multiple', m.hexdigest())
    m = hashlib.sha256()
    m.update(b'a' * 2048)
    print('update_large', m.hexdigest())
except Exception as e:
    print('SKIP_update()_method', type(e).__name__, e)

# === digest() method ===
try:
    m = hashlib.sha256(b'hello')
    print('digest_bytes', m.digest().hex())
except Exception as e:
    print('SKIP_digest()_method', type(e).__name__, e)

# === copy() method ===
try:
    m1 = hashlib.sha256(b'hello')
    m2 = m1.copy()
    m1.update(b' world')
    print('copy_original', m1.hexdigest())
    print('copy_clone', m2.hexdigest())
except Exception as e:
    print('SKIP_copy()_method', type(e).__name__, e)

# === file_digest ===
try:
    fileobj = io.BytesIO(b'hello world')
    result = hashlib.file_digest(fileobj, 'sha256')
    print('file_digest_sha256', result.hexdigest())

    fileobj = io.BytesIO(b'test content for hashing')
    result = hashlib.file_digest(fileobj, 'md5')
    print('file_digest_md5', result.hexdigest())

    fileobj = io.BytesIO(b'')
    result = hashlib.file_digest(fileobj, 'sha256')
    print('file_digest_empty', result.hexdigest())
except Exception as e:
    print('SKIP_file_digest', type(e).__name__, e)

# === pbkdf2_hmac ===
try:
    key = hashlib.pbkdf2_hmac('sha256', b'password', b'salt', 100000)
    print('pbkdf2_hmac_sha256', key.hex())
    key = hashlib.pbkdf2_hmac('sha256', b'password', b'salt', 100000, dklen=32)
    print('pbkdf2_hmac_dklen_32', key.hex())
    key = hashlib.pbkdf2_hmac('sha256', b'password', b'salt', 100000, dklen=64)
    print('pbkdf2_hmac_dklen_64', key.hex())
    key = hashlib.pbkdf2_hmac('sha512', b'password', b'salt', 100000)
    print('pbkdf2_hmac_sha512', key.hex())
    key = hashlib.pbkdf2_hmac('sha1', b'password', b'salt', 1000)
    print('pbkdf2_hmac_sha1', key.hex())
except Exception as e:
    print('SKIP_pbkdf2_hmac', type(e).__name__, e)

# === scrypt ===
try:
    key = hashlib.scrypt(b'password', salt=b'salt', n=2, r=8, p=1)
    print('scrypt_basic', key.hex())
    key = hashlib.scrypt(b'password', salt=b'salt', n=2, r=8, p=1, dklen=32)
    print('scrypt_dklen_32', key.hex())
    key = hashlib.scrypt(b'password', salt=b'salt', n=2, r=8, p=1, dklen=64)
    print('scrypt_dklen_64', key.hex())
except Exception as e:
    print('SKIP_scrypt', type(e).__name__, e)

# === usedforsecurity parameter ===
try:
    print('usedforsecurity_md5', hashlib.md5(b'hello', usedforsecurity=False).hexdigest())
    print('usedforsecurity_sha1', hashlib.sha1(b'hello', usedforsecurity=False).hexdigest())
    print('usedforsecurity_sha256', hashlib.sha256(b'hello', usedforsecurity=True).hexdigest())
    print('usedforsecurity_new', hashlib.new('md5', b'hello', usedforsecurity=False).hexdigest())
except Exception as e:
    print('SKIP_usedforsecurity_parameter', type(e).__name__, e)

# === Edge cases: empty input ===
try:
    print('edge_md5_empty', hashlib.md5(b'').hexdigest())
    print('edge_sha256_empty', hashlib.sha256(b'').hexdigest())
    print('edge_sha512_empty', hashlib.sha512(b'').hexdigest())
    print('edge_blake2b_empty', hashlib.blake2b(b'').hexdigest())
except Exception as e:
    print('SKIP_Edge_cases:_empty_input', type(e).__name__, e)

# === Edge cases: binary data ===
try:
    print('edge_md5_binary', hashlib.md5(bytes(range(256))).hexdigest())
    print('edge_sha256_binary', hashlib.sha256(bytes(range(256))).hexdigest())
except Exception as e:
    print('SKIP_Edge_cases:_binary_data', type(e).__name__, e)

# === Edge cases: large data ===
try:
    large_data = b'a' * 10000
    print('edge_sha256_large', hashlib.sha256(large_data).hexdigest())
except Exception as e:
    print('SKIP_Edge_cases:_large_data', type(e).__name__, e)

# === Algorithm availability checks ===
try:
    for algo in sorted(hashlib.algorithms_guaranteed):
        try:
            h = hashlib.new(algo)
            print(f'algo_available_{algo}', True)
        except Exception as e:
            print(f'algo_available_{algo}', False)
except Exception as e:
    print('SKIP_Algorithm_availability_checks', type(e).__name__, e)

# === Additional algorithms from algorithms_available ===
try:
    extra_algos = ['sha512_224', 'sha512_256', 'sm3', 'ripemd160', 'md5-sha1']
    for algo in extra_algos:
        if algo in hashlib.algorithms_available:
            try:
                h = hashlib.new(algo, b'test')
                print(f'extra_algo_{algo.replace("-", "_")}', h.hexdigest()[:16] + '...')
            except Exception as e:
                print(f'extra_algo_{algo.replace("-", "_")}', f'error: {e}')
        else:
            print(f'extra_algo_{algo.replace("-", "_")}', 'not_available')
except Exception as e:
    print('SKIP_Additional_algorithms_from_algorithms_available', type(e).__name__, e)
