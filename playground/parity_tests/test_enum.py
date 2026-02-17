import enum
from enum import (
    Enum, IntEnum, Flag, IntFlag, StrEnum,
    auto, unique, member, nonmember,
    CONFORM, EJECT, KEEP, STRICT,
    EnumCheck, verify, ReprEnum,
    pickle_by_enum_name, pickle_by_global_name,
)

# === Sentinel imports ===
try:
    # These should be truthy sentinel values (not None)
    assert enum.Enum is not None, 'Enum should not be None'
    assert enum.IntEnum is not None, 'IntEnum should not be None'
    assert enum.Flag is not None, 'Flag should not be None'
    assert enum.IntFlag is not None, 'IntFlag should not be None'
    assert enum.StrEnum is not None, 'StrEnum should not be None'
except Exception as e:
    print('SKIP_Sentinel imports', type(e).__name__, e)

# === Existence checks ===
try:
    assert enum.unique is not None, 'unique should exist'
    assert enum.member is not None, 'member should exist'
    assert enum.nonmember is not None, 'nonmember should exist'
    assert enum.verify is not None, 'verify should exist'
    assert str(enum.CONFORM) == 'conform', 'CONFORM should stringify to conform'
    assert str(enum.EJECT) == 'eject', 'EJECT should stringify to eject'
    assert str(enum.KEEP) == 'keep', 'KEEP should stringify to keep'
    assert str(enum.STRICT) == 'strict', 'STRICT should stringify to strict'
except Exception as e:
    print('SKIP_Existence checks', type(e).__name__, e)

# === EnumCheck ===
try:
    assert EnumCheck is not None, 'EnumCheck should exist'
    # EnumCheck values may vary by Python version
    print('EnumCheck_EXISTS', True)
    if hasattr(EnumCheck, 'CONFORM'):
        print('EnumCheck_CONFORM', EnumCheck.CONFORM)
    if hasattr(EnumCheck, 'EJECT'):
        print('EnumCheck_EJECT', EnumCheck.EJECT)
    if hasattr(EnumCheck, 'STRICT'):
        print('EnumCheck_STRICT', EnumCheck.STRICT)
except Exception as e:
    print('SKIP_EnumCheck', type(e).__name__, e)

# === verify decorator ===
try:
    @verify(EnumCheck.UNIQUE)
    class VerifiedColor(Enum):
        RED = 1
        GREEN = 2
        BLUE = 3

    print('verify_unique_red', VerifiedColor.RED)
except Exception as e:
    print('SKIP_verify decorator', type(e).__name__, e)

# === enum.unique with actual Enum subclass ===
try:
    # Fix: Using actual Enum subclass instead of plain class
    @enum.unique
    class UniqueColor(enum.Enum):
        RED = 1
        GREEN = 2
        BLUE = 3


    print('unique_enum_red', UniqueColor.RED)
    print('unique_enum_green', UniqueColor.GREEN)
    print('unique_enum_blue', UniqueColor.BLUE)

    # Verify enum.unique returns the class when no duplicates
    got_class_back = enum.unique(UniqueColor) is UniqueColor
    print('unique_returns_class', got_class_back)


    # Test that enum.unique raises ValueError for duplicate values
    try:
        # This should raise ValueError because of duplicate values
        @enum.unique
        class DuplicateStatus(enum.Enum):
            OK = 200
            SUCCESS = 200  # Duplicate value
        print('duplicate_error_raised', False)
    except ValueError:
        print('duplicate_error_raised', True)
except Exception as e:
    print('SKIP_enum.unique with actual Enum subclass', type(e).__name__, e)

# === from enum import ... ===
try:
    from enum import Enum, Flag, IntEnum, IntFlag, StrEnum

    assert Enum is not None, 'from enum import Enum should work'
    assert IntEnum is not None, 'from enum import IntEnum should work'
    assert Flag is not None, 'from enum import Flag should work'
    assert IntFlag is not None, 'from enum import IntFlag should work'
    assert StrEnum is not None, 'from enum import StrEnum should work'
except Exception as e:
    print('SKIP_from enum import ...', type(e).__name__, e)

# === enum.auto ===
try:
    auto_a = enum.auto()
    auto_b = enum.auto()
    assert auto_a != auto_b, 'separate auto() calls should produce distinct values'
    print('auto_distinct', auto_a != auto_b)
except Exception as e:
    print('SKIP_enum.auto', type(e).__name__, e)

# === Basic Enum class syntax ===
try:
    class Color(Enum):
        RED = 1
        GREEN = 2
        BLUE = 3


    print('color_red', Color.RED)
    print('color_red_name', Color.RED.name)
    print('color_red_value', Color.RED.value)
    print('color_green', Color.GREEN)
    print('color_blue', Color.BLUE)
except Exception as e:
    print('SKIP_Basic Enum class syntax', type(e).__name__, e)

# === Enum properties ===
try:
    print('color_red_name_prop', Color.RED.name)
    print('color_red_value_prop', Color.RED.value)
except Exception as e:
    print('SKIP_Enum properties', type(e).__name__, e)

# === Enum.__members__ ===
try:
    print('color_members_dict', list(Color.__members__.keys()))
except Exception as e:
    print('SKIP_Enum.__members__', type(e).__name__, e)

# === Enum __getitem__ ===
try:
    print('color_getitem', Color['RED'])
except Exception as e:
    print('SKIP_Enum __getitem__', type(e).__name__, e)

# === Enum __contains__ ===
try:
    print('color_contains_true', Color.RED in Color)
except Exception as e:
    print('SKIP_Enum __contains__', type(e).__name__, e)

# === Enum __iter__ ===
try:
    print('color_iter', list(Color))
except Exception as e:
    print('SKIP_Enum __iter__', type(e).__name__, e)

# === Enum __len__ via len() on list ===
try:
    print('color_len', len(list(Color)))
except Exception as e:
    print('SKIP_Enum __len__ via len() on list', type(e).__name__, e)

# === IntEnum class syntax + int behavior ===
try:
    class Priority(IntEnum):
        LOW = 1
        MEDIUM = 2
        HIGH = 3


    print('priority_low', Priority.LOW)
    print('priority_is_int', isinstance(Priority.LOW, int))
    print('priority_int_compare', Priority.LOW == 1)
    print('priority_int_math', Priority.LOW + Priority.MEDIUM == 3)
except Exception as e:
    print('SKIP_IntEnum class syntax + int behavior', type(e).__name__, e)

# === IntEnum int methods ===
try:
    print('priority_bit_length', Priority.HIGH.bit_length())
except Exception as e:
    print('SKIP_IntEnum int methods', type(e).__name__, e)

# === Flag class syntax + bitwise operators ===
try:
    class Permission(enum.Flag):
        READ = enum.auto()
        WRITE = enum.auto()
        EXECUTE = enum.auto()


    print('permission_read', Permission.READ)
    print('permission_write', Permission.WRITE)
    print('permission_execute', Permission.EXECUTE)
    print('permission_distinct', Permission.READ != Permission.WRITE)

    permission_rw = Permission.READ | Permission.WRITE
    print('permission_rw', permission_rw)
    print('permission_rw_has_read', (permission_rw & Permission.READ) == Permission.READ)
    print('permission_xor', ((Permission.READ ^ Permission.WRITE) ^ Permission.WRITE) == Permission.READ)
    print('permission_invert', (~Permission.READ) != Permission.READ)
except Exception as e:
    print('SKIP_Flag class syntax + bitwise operators', type(e).__name__, e)

# === Flag methods ===
try:
    print('permission_rw_name', permission_rw.name)
    print('permission_rw_value', permission_rw.value)
except Exception as e:
    print('SKIP_Flag methods', type(e).__name__, e)

# === IntFlag class syntax + int behavior ===
try:
    class PermissionInt(enum.IntFlag):
        READ = enum.auto()
        WRITE = enum.auto()
        EXECUTE = enum.auto()


    print('permission_int_is_int', isinstance(PermissionInt.READ, int))
    permission_int_rw = PermissionInt.READ | PermissionInt.WRITE
    print('permission_int_rw', permission_int_rw)
    print('permission_int_rw_has_read', (permission_int_rw & PermissionInt.READ) == PermissionInt.READ)
    print('permission_int_xor', ((PermissionInt.READ ^ PermissionInt.WRITE) ^ PermissionInt.WRITE) == PermissionInt.READ)
    print('permission_int_invert', (~PermissionInt.READ) != PermissionInt.READ)
except Exception as e:
    print('SKIP_IntFlag class syntax + int behavior', type(e).__name__, e)

# === StrEnum class syntax ===
try:
    class Direction(StrEnum):
        NORTH = 'north'
        SOUTH = 'south'
        EAST = 'east'
        WEST = 'west'


    print('direction_north', Direction.NORTH)
    print('direction_is_str', isinstance(Direction.NORTH, str))
    print('direction_str_compare', Direction.NORTH == 'north')
    print('direction_upper', Direction.NORTH.upper())
except Exception as e:
    print('SKIP_StrEnum class syntax', type(e).__name__, e)

# === StrEnum str methods ===
try:
    print('direction_lower', Direction.NORTH.lower())
    print('direction_startswith', Direction.NORTH.startswith('nor'))
except Exception as e:
    print('SKIP_StrEnum str methods', type(e).__name__, e)

# === Enum iteration ===
try:
    print('color_members', list(Color))
    print('color_member_count', len(list(Color)))
except Exception as e:
    print('SKIP_Enum iteration', type(e).__name__, e)

# === Enum comparison ===
try:
    print('color_eq_same', Color.RED == Color.RED)
    print('color_eq_diff', Color.RED == Color.GREEN)
    print('color_is_same', Color.RED is Color.RED)
except Exception as e:
    print('SKIP_Enum comparison', type(e).__name__, e)

# === Enum value access ===
try:
    print('color_red_by_value', Color(1))
    print('color_green_by_value', Color(2))
except Exception as e:
    print('SKIP_Enum value access', type(e).__name__, e)

# === Enum hashability (can be used in dict/set) ===
try:
    color_dict = {Color.RED: 'red', Color.GREEN: 'green'}
    print('color_hashable', color_dict[Color.RED])
    color_set = {Color.RED, Color.GREEN}
    print('color_in_set', Color.RED in color_set)
except Exception as e:
    print('SKIP_Enum hashability (can be used in dict/set)', type(e).__name__, e)

# === ReprEnum ===
try:
    # ReprEnum must be mixed with a data type like int or str
    class ReprColor(int, ReprEnum):
        RED = 1
        GREEN = 2

    print('repr_enum', repr(ReprColor.RED))
except Exception as e:
    print('SKIP_ReprEnum', type(e).__name__, e)

# === member and nonmember ===
try:
    class MixedEnum(Enum):
        MEMBER = member(1)
        NONMEMBER = nonmember(2)

    print('member_is_enum', isinstance(MixedEnum.MEMBER, Enum))
except Exception as e:
    print('SKIP_member and nonmember', type(e).__name__, e)

# === pickle_by_enum_name ===
try:
    print('pickle_by_enum_name_exists', callable(pickle_by_enum_name))
except Exception as e:
    print('SKIP_pickle_by_enum_name', type(e).__name__, e)

# === pickle_by_global_name ===
try:
    print('pickle_by_global_name_exists', callable(pickle_by_global_name))
except Exception as e:
    print('SKIP_pickle_by_global_name', type(e).__name__, e)

# === show_flag_values ===
try:
    print('show_flag_values_exists', callable(enum.show_flag_values))
    if hasattr(enum, 'show_flag_values'):
        print('show_flag_values', list(enum.show_flag_values(Permission.READ)))
except Exception as e:
    print('SKIP_show_flag_values', type(e).__name__, e)
