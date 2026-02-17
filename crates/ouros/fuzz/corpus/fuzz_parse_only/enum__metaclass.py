from enum import Enum


class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3


assert Color.RED.value == 1, 'enum value'
assert Color.RED.name == 'RED', 'enum name'
assert list(Color) == [Color.RED, Color.GREEN, Color.BLUE], 'enum iteration'
assert Color(1) is Color.RED, 'enum lookup by value'
