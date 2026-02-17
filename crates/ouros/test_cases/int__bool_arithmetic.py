# bool is a subclass of int in Python arithmetic

# += with bool
x = 0
x += True
assert x == 1
x += False
assert x == 1

# + with bool
assert 1 + True == 2
assert True + 1 == 2
assert True + True == 2

# - with bool
assert 5 - True == 4
assert True - 1 == 0
assert True - True == 0

# * with bool
assert 3 * True == 3
assert True * 3 == 3
assert True * True == 1

# / with bool
assert 4 / True == 4.0
assert True / 2 == 0.5

# // with bool
assert 5 // True == 5
assert True // 2 == 0

# % with bool
assert 5 % True == 0
assert True % 2 == 1

# ** with bool
assert 2 ** True == 2
assert 2 ** False == 1
assert True ** 3 == 1

# in-place ops with bool RHS
v = 5
v -= True
assert v == 4

v = 6
v *= True
assert v == 6

v = 7
v //= True
assert v == 7

v = 8
v %= True
assert v == 0

v = 2
v **= True
assert v == 2
