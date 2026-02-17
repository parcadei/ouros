# === Path constructor ===
from pathlib import Path, PurePath, PurePosixPath, PureWindowsPath

p = Path('/usr/local/bin/python')
assert str(p) == '/usr/local/bin/python', 'Path str should match input'

# Constructor with multiple arguments
assert str(Path('folder', 'file.txt')) == 'folder/file.txt', 'Path with two args joins'
assert str(Path('/usr', 'local', 'bin')) == '/usr/local/bin', 'Path with three args joins'
assert str(Path('start', '/absolute', 'end')) == '/absolute/end', 'absolute in middle replaces'

# Constructor with no arguments
assert str(Path()) == '.', 'Path() returns current dir'

# === name property ===
assert p.name == 'python', 'name should be final component'
assert Path('/usr/local/bin/').name == 'bin', 'name should handle trailing slash'
assert Path('/').name == '', 'root path should have empty name'
assert Path('file.txt').name == 'file.txt', 'relative path name'

# === parent property ===
assert str(p.parent) == '/usr/local/bin', 'parent should remove last component'
assert str(Path('/usr').parent) == '/', 'parent of first-level should be root'
assert str(Path('/').parent) == '/', 'parent of root is root'
assert str(Path('file.txt').parent) == '.', 'parent of relative without dir is .'

# === stem property ===
assert Path('/path/file.tar.gz').stem == 'file.tar', 'stem removes last extension'
assert Path('/path/file.txt').stem == 'file', 'stem removes single extension'
assert Path('/path/.bashrc').stem == '.bashrc', 'stem preserves hidden files'
assert Path('/path/file').stem == 'file', 'stem without extension'

# === suffix property ===
assert Path('/path/file.tar.gz').suffix == '.gz', 'suffix is last extension'
assert Path('/path/file.txt').suffix == '.txt', 'suffix with single extension'
assert Path('/path/.bashrc').suffix == '', 'hidden file has no suffix'
assert Path('/path/file').suffix == '', 'no extension means empty suffix'

# === suffixes property ===
assert Path('/path/file.tar.gz').suffixes == ['.tar', '.gz'], 'suffixes list'
assert Path('/path/file.txt').suffixes == ['.txt'], 'single suffix as list'
assert Path('/path/.bashrc').suffixes == [], 'hidden file has no suffixes'

# === parts property ===
assert Path('/usr/local/bin').parts == ('/', 'usr', 'local', 'bin'), 'absolute path parts'
assert Path('usr/local').parts == ('usr', 'local'), 'relative path parts'
assert Path('/').parts == ('/',), 'root path parts'

# === is_absolute method ===
assert Path('/usr/bin').is_absolute() == True, 'absolute path'
assert Path('usr/bin').is_absolute() == False, 'relative path not absolute'
assert Path('').is_absolute() == False, 'empty path not absolute'

# === joinpath method ===
assert str(Path('/usr').joinpath('local')) == '/usr/local', 'joinpath with one arg'
assert str(Path('/usr').joinpath('local', 'bin')) == '/usr/local/bin', 'joinpath with two args'
assert str(Path('/usr').joinpath('/etc')) == '/etc', 'joinpath with absolute replaces'
assert str(Path('.').joinpath('file')) == 'file', 'joinpath from dot'

# === with_name method ===
assert str(Path('/path/file.txt').with_name('other.py')) == '/path/other.py', 'with_name replaces name'
assert str(Path('file.txt').with_name('other.py')) == 'other.py', 'with_name on relative'

# === with_suffix method ===
assert str(Path('/path/file.txt').with_suffix('.py')) == '/path/file.py', 'with_suffix replaces'
assert str(Path('/path/file.txt').with_suffix('')) == '/path/file', 'with_suffix removes'
assert str(Path('/path/file').with_suffix('.txt')) == '/path/file.txt', 'with_suffix adds'

# === / operator ===
assert str(Path('/usr') / 'local') == '/usr/local', '/ operator joins'
assert str(Path('/usr') / 'local' / 'bin') == '/usr/local/bin', '/ operator chains'

# === as_posix method ===
assert Path('/usr/bin').as_posix() == '/usr/bin', 'as_posix returns string'

# === __fspath__ method (os.PathLike protocol) ===
assert Path('/usr/bin').__fspath__() == '/usr/bin', '__fspath__ returns string'

# === repr ===
r = repr(Path('/usr/bin'))
assert r == "PosixPath('/usr/bin')", f'repr should be PosixPath, got {r}'

# === PurePath constructors ===
pure = PurePath('/usr/local/bin.py')
assert str(pure) == '/usr/local/bin.py', 'PurePath str'
assert repr(pure) == "PurePosixPath('/usr/local/bin.py')", 'PurePath repr uses PurePosixPath'
assert str(PurePosixPath('/a/b')) == '/a/b', 'PurePosixPath basic constructor'
assert isinstance(Path('/a'), PurePath) == True, 'Path is instance of PurePath'
assert isinstance(PurePosixPath('/a'), PurePath) == True, 'PurePosixPath is instance of PurePath'
assert isinstance(PureWindowsPath('C:/a'), PurePath) == True, 'PureWindowsPath is instance of PurePath'

# === root / anchor / drive properties ===
assert Path('/usr').root == '/', 'Path root for absolute path'
assert Path('usr').root == '', 'Path root for relative path'
assert Path('/usr').anchor == '/', 'Path anchor for absolute path'
assert Path('/usr').drive == '', 'Path drive is empty on posix'
assert PureWindowsPath('C:/Users/Alice').root == '\\', 'PureWindowsPath root'
assert PureWindowsPath('C:/Users/Alice').drive == 'C:', 'PureWindowsPath drive'
assert PureWindowsPath('C:/Users/Alice').anchor == 'C:\\', 'PureWindowsPath anchor'
assert PureWindowsPath('C:/Users/Alice').as_posix() == 'C:/Users/Alice', 'PureWindowsPath as_posix uses slashes'
assert PureWindowsPath('C:/Users/Alice').__fspath__() == 'C:\\Users\\Alice', (
    'PureWindowsPath __fspath__ uses backslashes'
)

# === parents property ===
assert tuple(str(x) for x in Path('/usr/local/bin').parents) == ('/usr/local', '/usr', '/'), 'Path parents sequence'
assert tuple(str(x) for x in PurePath('a/b').parents) == ('a', '.'), 'PurePath relative parents'
assert tuple(str(x) for x in PureWindowsPath('C:/a/b').parents) == ('C:\\a', 'C:\\'), 'PureWindowsPath parents'

# === with_stem method ===
assert str(Path('/path/file.txt').with_stem('other')) == '/path/other.txt', 'Path with_stem replaces stem'
assert str(PurePath('/path/.bashrc').with_stem('config')) == '/path/config', 'with_stem keeps empty suffix'

# === relative_to and is_relative_to ===
assert str(Path('/usr/local/bin').relative_to('/usr')) == 'local/bin', 'relative_to strips prefix'
assert Path('/usr/local/bin').is_relative_to('/usr') == True, 'is_relative_to true'
assert Path('/usr/local/bin').is_relative_to('/opt') == False, 'is_relative_to false'
assert str(PureWindowsPath('C:/Users/Alice/file.txt').relative_to('C:/Users')) == 'Alice\\file.txt', (
    'windows relative_to'
)
assert PureWindowsPath('C:/Users/Alice/file.txt').is_relative_to('D:/Users') == False, 'windows is_relative_to false'

# === match method ===
assert Path('/usr/local/bin.py').match('*.py') == True, 'Path match basename wildcard'
assert Path('/usr/local/bin.py').match('local/bin.py') == True, 'Path match suffix segments'
assert Path('/usr/local/bin.py').match('usr/*.py') == False, 'Path match requires right-aligned segments'
assert Path('/usr/local/bin.py').match('/usr/*/bin.py') == True, 'Path absolute pattern match'
assert PureWindowsPath('C:/Users/Alice/file.TXT').match('*.txt') == True, 'PureWindowsPath match is case-insensitive'

# === equality and hashing ===
assert Path('/x/y') == PurePosixPath('/x/y'), 'Path equals PurePosixPath with same path'
assert hash(Path('/x/y')) == hash(PurePosixPath('/x/y')), 'Path and PurePosixPath hash equally'
assert PureWindowsPath('C:/Users/Alice') == PureWindowsPath('c:/users/alice'), 'PureWindowsPath equality ignores case'
assert hash(PureWindowsPath('C:/Users/Alice')) == hash(PureWindowsPath('c:/users/alice')), (
    'PureWindowsPath hash ignores case'
)
assert PureWindowsPath('C:/Users/Alice') != PurePosixPath('C:/Users/Alice'), (
    'Windows and posix pure paths are not equal'
)
