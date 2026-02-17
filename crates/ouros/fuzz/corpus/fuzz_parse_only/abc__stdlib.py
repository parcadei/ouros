# Tests for abc module stdlib functions and markers

import abc

# === abc.ABC ===
# ABC should exist as a truthy sentinel
assert abc.ABC, 'ABC should be truthy'

# === abc.ABCMeta ===
# ABCMeta should exist as an accessible attribute
meta = abc.ABCMeta
assert meta is not None, 'ABCMeta should exist'


# === abc.abstractmethod ===
def my_method():
    return 42


decorated = abc.abstractmethod(my_method)
assert decorated is my_method, 'abstractmethod should return the function unchanged'

# === abc.abstractclassmethod ===
# Just verify it exists and can be called as a decorator
decorated2 = abc.abstractclassmethod(my_method)
assert decorated2 is not None, 'abstractclassmethod should return a value'

# === abc.abstractstaticmethod ===
decorated3 = abc.abstractstaticmethod(my_method)
assert decorated3 is not None, 'abstractstaticmethod should return a value'

# === abc.abstractproperty ===
decorated4 = abc.abstractproperty(my_method)
assert decorated4 is not None, 'abstractproperty should return a value'
