import string

# === string constants ===
assert string.ascii_lowercase == 'abcdefghijklmnopqrstuvwxyz', 'ascii_lowercase'
assert string.ascii_uppercase == 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'ascii_uppercase'
assert string.ascii_letters == 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ', 'ascii_letters'
assert string.digits == '0123456789', 'digits'
assert string.hexdigits == '0123456789abcdefABCDEF', 'hexdigits'
assert string.octdigits == '01234567', 'octdigits'
assert len(string.punctuation) == 32, 'punctuation length'
assert '!' in string.punctuation, 'punctuation has !'
assert '@' in string.punctuation, 'punctuation has @'
assert len(string.whitespace) == 6, 'whitespace length'
assert ' ' in string.whitespace, 'whitespace has space'
assert '\t' in string.whitespace, 'whitespace has tab'
assert '\n' in string.whitespace, 'whitespace has newline'

# === printable contains others ===
for c in string.digits:
    assert c in string.printable, 'printable contains digits'
for c in string.ascii_letters:
    assert c in string.printable, 'printable contains letters'

# === type checks ===
assert isinstance(string.ascii_lowercase, str), 'ascii_lowercase is str'
assert isinstance(string.digits, str), 'digits is str'

# === from import ===
from string import ascii_lowercase, digits

assert ascii_lowercase == 'abcdefghijklmnopqrstuvwxyz', 'from import ascii_lowercase'
assert digits == '0123456789', 'from import digits'
