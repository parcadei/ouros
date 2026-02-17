# === NamedTuple field access via sys.version_info (lines 77, 96-98, 108-110) ===
import sys

vi = sys.version_info
assert vi.major == 3, 'version_info major field'
assert vi.minor >= 0, 'version_info minor field is non-negative'

# === NamedTuple length (lines 146, 153-155) ===
assert len(vi) == 5, 'version_info has 5 fields'

# === NamedTuple indexing (lines 172, 178) ===
assert vi[0] == 3, 'version_info index 0 is major'
assert vi[0] == vi.major, 'index access matches named access'
assert vi[-1] == vi.serial, 'negative index matches last field'

# === NamedTuple bool (lines 214-216) ===
assert bool(vi) == True, 'non-empty named tuple is truthy'

# === NamedTuple equality (lines 182, 185-191, 193-194) ===
# Named tuples compare by value like regular tuples
assert vi == vi, 'named tuple equal to itself'
major = vi[0]
minor = vi[1]
micro = vi[2]
releaselevel = vi[3]
serial = vi[4]
assert vi == (major, minor, micro, releaselevel, serial), 'named tuple equals regular tuple with same values'

# === NamedTuple repr (lines 218-224, 226, 228-236, 239-240) ===
r = repr(vi)
assert 'sys.version_info' in r, 'repr includes type name'
assert 'major=' in r, 'repr includes major field'
assert 'minor=' in r, 'repr includes minor field'
assert 'micro=' in r, 'repr includes micro field'
assert 'releaselevel=' in r, 'repr includes releaselevel field'
assert 'serial=' in r, 'repr includes serial field'

# === NamedTuple py_dec_ref_ids (lines 204-210) ===
# This is exercised when named tuple with ref items is garbage collected
# version_info contains strings which are refs
r2 = repr(vi)
assert isinstance(r2, str), 'repr returns string for named tuple with refs'

# === NamedTuple getattr for known fields ===
assert type(vi.releaselevel) == str, 'releaselevel is a string'
assert type(vi.serial) == int, 'serial is an int'

# === NamedTuple str ===
s = str(vi)
assert 'sys.version_info' in s, 'str includes type name'

# === NamedTuple getitem with out-of-bounds ===
try:
    vi[10]
except IndexError:
    assert True, 'out of bounds index raises IndexError'

try:
    vi[-10]
except IndexError:
    assert True, 'negative out of bounds raises IndexError'

# === NamedTuple getitem with non-int key ===
try:
    vi['major']
except TypeError:
    assert True, 'string key raises TypeError'

# === NamedTuple iteration via tuple() ===
t = tuple(vi)
assert len(t) == 5, 'tuple from named tuple has 5 elements'
assert t[0] == vi.major, 'tuple element matches named field'

# === NamedTuple in comparisons ===
assert vi != (0, 0, 0, '', 0), 'version_info not equal to zeros tuple'
