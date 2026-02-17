# HOST MODULE: Only testing sandbox-safe subset
# Comprehensive PurePath tests - no filesystem operations

from pathlib import PurePath, PurePosixPath, PureWindowsPath

# === PurePath class instantiation ===
try:
    print('purepath_empty', PurePath())
    print('purepath_single', PurePath('foo'))
    print('purepath_multiple', PurePath('foo', 'bar', 'baz'))
    print('purepath_absolute', PurePath('/usr/bin'))
    print('purepath_mixed', PurePath('foo', 'some/path', 'bar'))
except Exception as e:
    print('SKIP_PurePath_class_instantiation', type(e).__name__, e)

# === PurePosixPath class instantiation ===
try:
    print('pureposixpath_empty', PurePosixPath())
    print('pureposixpath_single', PurePosixPath('/etc/hosts'))
    print('pureposixpath_multiple', PurePosixPath('usr', 'bin', 'python'))
    print('pureposixpath_relative', PurePosixPath('foo/bar'))
except Exception as e:
    print('SKIP_PurePosixPath_class_instantiation', type(e).__name__, e)

# === PureWindowsPath class instantiation ===
try:
    print('purewindowspath_empty', PureWindowsPath())
    print('purewindowspath_single', PureWindowsPath('C:\\Windows'))
    print('purewindowspath_multiple', PureWindowsPath('C:', 'Users', 'file.txt'))
    print('purewindowspath_unc', PureWindowsPath('\\\\server\\share\\file'))
    print('purewindowspath_forward', PureWindowsPath('C:/Program Files'))
except Exception as e:
    print('SKIP_PureWindowsPath_class_instantiation', type(e).__name__, e)

# === PurePath parts property ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('parts_unix_abs', p.parts)

    p = PurePosixPath('usr/bin/python')
    print('parts_unix_rel', p.parts)

    p = PureWindowsPath('c:/Program Files/PSF')
    print('parts_windows', p.parts)

    p = PureWindowsPath('//server/share/file')
    print('parts_unc', p.parts)

    p = PurePosixPath('/')
    print('parts_root_only', p.parts)

    p = PurePosixPath('.')
    print('parts_dot', p.parts)
except Exception as e:
    print('SKIP_PurePath_parts_property', type(e).__name__, e)

# === drive property ===
try:
    print('drive_posix_abs', PurePosixPath('/usr/bin').drive)
    print('drive_posix_rel', PurePosixPath('usr/bin').drive)
    print('drive_windows_c', PureWindowsPath('c:/Windows').drive)
    print('drive_windows_d', PureWindowsPath('d:file.txt').drive)
    print('drive_windows_unc', PureWindowsPath('//server/share').drive)
    print('drive_windows_rel', PureWindowsPath('Windows').drive)
except Exception as e:
    print('SKIP_drive_property', type(e).__name__, e)

# === root property ===
try:
    print('root_posix_abs', PurePosixPath('/usr/bin').root)
    print('root_posix_rel', PurePosixPath('usr/bin').root)
    print('root_windows_abs', PureWindowsPath('c:/Windows').root)
    print('root_windows_unc', PureWindowsPath('//server/share').root)
    print('root_windows_rel', PureWindowsPath('Windows').root)
except Exception as e:
    print('SKIP_root_property', type(e).__name__, e)

# === anchor property ===
try:
    print('anchor_posix_abs', PurePosixPath('/usr/bin').anchor)
    print('anchor_posix_rel', PurePosixPath('usr/bin').anchor)
    print('anchor_windows_c', PureWindowsPath('c:/Windows').anchor)
    print('anchor_windows_d', PureWindowsPath('d:file.txt').anchor)
    print('anchor_windows_unc', PureWindowsPath('//server/share').anchor)
except Exception as e:
    print('SKIP_anchor_property', type(e).__name__, e)

# === name property ===
try:
    print('name_simple', PurePosixPath('file.txt').name)
    print('name_no_ext', PurePosixPath('file').name)
    print('name_with_path', PurePosixPath('/usr/bin/python').name)
    print('name_dot', PurePosixPath('.').name)
    print('name_dotdot', PurePosixPath('..').name)
    print('name_empty', PurePosixPath('').name)
    print('name_trailing_slash', PurePosixPath('/usr/bin/').name)
except Exception as e:
    print('SKIP_name_property', type(e).__name__, e)

# === suffix property ===
try:
    print('suffix_single', PurePosixPath('file.txt').suffix)
    print('suffix_multiple', PurePosixPath('file.tar.gz').suffix)
    print('suffix_no_ext', PurePosixPath('file').suffix)
    print('suffix_dotfile', PurePosixPath('.bashrc').suffix)
    print('suffix_dotdot', PurePosixPath('..').suffix)
    print('suffix_hidden', PurePosixPath('.profile').suffix)
except Exception as e:
    print('SKIP_suffix_property', type(e).__name__, e)

# === suffixes property ===
try:
    print('suffixes_single', PurePosixPath('file.txt').suffixes)
    print('suffixes_multiple', PurePosixPath('file.tar.gz').suffixes)
    print('suffixes_many', PurePosixPath('archive.tar.gz.bz2').suffixes)
    print('suffixes_no_ext', PurePosixPath('file').suffixes)
    print('suffixes_dotfile', PurePosixPath('.bashrc').suffixes)
except Exception as e:
    print('SKIP_suffixes_property', type(e).__name__, e)

# === stem property ===
try:
    print('stem_simple', PurePosixPath('file.txt').stem)
    print('stem_multiple', PurePosixPath('file.tar.gz').stem)
    print('stem_no_ext', PurePosixPath('file').stem)
    print('stem_dotfile', PurePosixPath('.bashrc').stem)
    print('stem_hidden', PurePosixPath('.profile').stem)
except Exception as e:
    print('SKIP_stem_property', type(e).__name__, e)

# === parent property ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('parent_single', p.parent)
    print('parent_double', p.parent.parent)
    print('parent_triple', p.parent.parent.parent)
    print('parent_rel', PurePosixPath('foo/bar/baz').parent)
    print('parent_root', PurePosixPath('/').parent)
    print('parent_dot', PurePosixPath('.').parent)
except Exception as e:
    print('SKIP_parent_property', type(e).__name__, e)

# === parents property ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('parents_list', list(p.parents))
    print('parents_index0', p.parents[0])
    print('parents_index1', p.parents[1])
    print('parents_index2', p.parents[2])
    print('parents_len', len(p.parents))

    p = PurePosixPath('foo/bar/baz')
    print('parents_rel', list(p.parents))
except Exception as e:
    print('SKIP_parents_property', type(e).__name__, e)

# === as_posix method ===
try:
    print('as_posix_unix', PurePosixPath('/usr/bin').as_posix())
    print('as_posix_windows', PureWindowsPath('C:\\Windows').as_posix())
    print('as_posix_mixed', PureWindowsPath('C:/Windows/System32').as_posix())
except Exception as e:
    print('SKIP_as_posix_method', type(e).__name__, e)

# === as_uri method ===
try:
    print('as_uri_unix_abs', PurePosixPath('/usr/bin').as_uri())
    print('as_uri_unix_nested', PurePosixPath('/etc/hosts').as_uri())

    # Windows paths with drive letters
    print('as_uri_windows_c', PureWindowsPath('C:\\Windows').as_uri())
    print('as_uri_windows_nested', PureWindowsPath('D:\\Users\\file.txt').as_uri())

    # UNC paths
    print('as_uri_unc', PureWindowsPath('\\\\server\\share\\file').as_uri())
except Exception as e:
    print('SKIP_as_uri_method', type(e).__name__, e)

# === is_absolute method ===
try:
    print('is_abs_unix_abs', PurePosixPath('/usr/bin').is_absolute())
    print('is_abs_unix_rel', PurePosixPath('usr/bin').is_absolute())
    print('is_abs_unix_dot', PurePosixPath('./file').is_absolute())
    print('is_abs_windows_c', PureWindowsPath('C:/Windows').is_absolute())
    print('is_abs_windows_rel', PureWindowsPath('Windows').is_absolute())
    print('is_abs_windows_drive_only', PureWindowsPath('C:').is_absolute())
except Exception as e:
    print('SKIP_is_absolute_method', type(e).__name__, e)

# === is_relative_to method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('is_rel_to_true1', p.is_relative_to('/usr'))
    print('is_rel_to_true2', p.is_relative_to('/usr/bin'))
    print('is_rel_to_false1', p.is_relative_to('/etc'))
    print('is_rel_to_false2', p.is_relative_to('/usr/local'))

    p = PurePosixPath('foo/bar/baz')
    print('is_rel_to_rel_true', p.is_relative_to('foo'))
    print('is_rel_to_rel_false', p.is_relative_to('bar'))
except Exception as e:
    print('SKIP_is_relative_to_method', type(e).__name__, e)

# === is_relative_to with multiple segments via join ===
try:
    p = PurePosixPath('/usr/local/bin')
    print('is_rel_to_path_join', p.is_relative_to(PurePosixPath('/usr') / 'local'))
except Exception as e:
    print('SKIP_is_relative_to_with_multiple_segments_via_join', type(e).__name__, e)

# === is_reserved method ===
try:
    print('is_reserved_unix_file', PurePosixPath('file.txt').is_reserved())
    print('is_reserved_unix_con', PurePosixPath('CON').is_reserved())
    print('is_reserved_unix_nul', PurePosixPath('NUL').is_reserved())

    print('is_reserved_windows_con', PureWindowsPath('CON').is_reserved())
    print('is_reserved_windows_prn', PureWindowsPath('PRN').is_reserved())
    print('is_reserved_windows_aux', PureWindowsPath('AUX').is_reserved())
    print('is_reserved_windows_nul', PureWindowsPath('NUL').is_reserved())
    print('is_reserved_windows_com1', PureWindowsPath('COM1').is_reserved())
    print('is_reserved_windows_lpt1', PureWindowsPath('LPT1').is_reserved())
    print('is_reserved_windows_file', PureWindowsPath('file.txt').is_reserved())
    print('is_reserved_windows_con_ext', PureWindowsPath('CON.txt').is_reserved())
    print('is_reserved_windows_lowercase', PureWindowsPath('con').is_reserved())
except Exception as e:
    print('SKIP_is_reserved_method', type(e).__name__, e)

# === joinpath method ===
try:
    p = PurePosixPath('/usr')
    print('joinpath_single', p.joinpath('bin'))
    print('joinpath_multiple', p.joinpath('local', 'bin'))
    print('joinpath_abs', p.joinpath('/etc'))
    print('joinpath_path_obj', p.joinpath(PurePosixPath('bin/python')))
except Exception as e:
    print('SKIP_joinpath_method', type(e).__name__, e)

# === slash operator ===
try:
    p = PurePosixPath('/etc')
    print('slash_str', p / 'init.d' / 'apache2')
    print('slash_path', p / PurePosixPath('nginx'))
    print('slash_left_str', '/usr' / PurePosixPath('bin'))
    print('slash_abs_override', p / '/absolute/path')

    p = PureWindowsPath('C:/Windows')
    print('slash_windows', p / 'System32')
except Exception as e:
    print('SKIP_slash_operator', type(e).__name__, e)

# === match method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('match_exact', p.match('/usr/bin/python'))
    print('match_wildcard1', p.match('/usr/bin/*'))
    print('match_wildcard2', p.match('/usr/*/python'))
    print('match_double', p.match('**/*.py'))
    print('match_false', p.match('/etc/*'))

    p = PurePosixPath('foo/bar/baz.py')
    print('match_rel_pattern', p.match('foo/bar/*.py'))
    print('match_rel_double', p.match('**/*.py'))
    print('match_partial_false', p.match('*.py'))

    # Case sensitivity
    print('match_case_posix', PurePosixPath('foo.py').match('*.PY'))
    print('match_case_windows', PureWindowsPath('foo.py').match('*.PY'))
except Exception as e:
    print('SKIP_match_method', type(e).__name__, e)

# === full_match method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('full_match_exact', p.full_match('/usr/bin/python'))
    print('full_match_wildcard', p.full_match('/usr/bin/*'))
    print('full_match_double', p.full_match('**/*'))
    print('full_match_false', p.full_match('usr/bin/python'))

    p = PurePosixPath('foo/bar/baz.py')
    print('full_match_rel_exact', p.full_match('foo/bar/baz.py'))
    print('full_match_rel_double', p.full_match('**/*.py'))
except Exception as e:
    print('SKIP_full_match_method', type(e).__name__, e)

# === relative_to method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('rel_to_single', p.relative_to('/usr'))
    print('rel_to_double', p.relative_to('/usr/bin'))
    p = PurePosixPath('foo/bar/baz')
    print('rel_to_rel', p.relative_to('foo'))

    # relative_to with single path that has multiple parts
    p = PurePosixPath('/usr/local/bin')
    print('rel_to_multi_path', p.relative_to(PurePosixPath('/') / 'usr' / 'local'))
except Exception as e:
    print('SKIP_relative_to_method', type(e).__name__, e)

# === with_name method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('with_name_basic', p.with_name('perl'))
    print('with_name_ext', p.with_name('python3'))
    print('with_name_rel', PurePosixPath('foo/bar.txt').with_name('baz.md'))

    # ValueError on no name
    # p = PurePosixPath('/').with_name('file')  # Raises ValueError
except Exception as e:
    print('SKIP_with_name_method', type(e).__name__, e)

# === with_stem method ===
try:
    p = PurePosixPath('/usr/bin/python.txt')
    print('with_stem_basic', p.with_stem('perl'))
    print('with_stem_multi', PurePosixPath('archive.tar.gz').with_stem('backup'))
    print('with_stem_no_ext', PurePosixPath('file').with_stem('document'))
except Exception as e:
    print('SKIP_with_stem_method', type(e).__name__, e)

# === with_suffix method ===
try:
    p = PurePosixPath('/usr/bin/python')
    print('with_suffix_add', p.with_suffix('.py'))
    print('with_suffix_replace', PurePosixPath('file.txt').with_suffix('.md'))
    print('with_suffix_remove', PurePosixPath('file.txt').with_suffix(''))
    print('with_suffix_multi', PurePosixPath('archive.tar.gz').with_suffix('.bz2'))
    print('with_suffix_empty', PurePosixPath('file').with_suffix('.txt'))
except Exception as e:
    print('SKIP_with_suffix_method', type(e).__name__, e)

# === with_segments method ===
try:
    p = PurePosixPath('/usr/bin')
    print('with_segments_basic', p.with_segments('foo', 'bar'))
    print('with_segments_abs', p.with_segments('/etc'))
    print('with_segments_rel', p.with_segments('a', 'b', 'c'))
except Exception as e:
    print('SKIP_with_segments_method', type(e).__name__, e)

# === parser property ===
try:
    print('parser_posix', PurePosixPath('/usr/bin').parser)
    print('parser_windows', PureWindowsPath('C:/Windows').parser)
except Exception as e:
    print('SKIP_parser_property', type(e).__name__, e)

# === String representation ===
try:
    p = PurePosixPath('/usr/bin')
    print('str_posix', str(p))
    print('repr_posix', repr(p))

    p = PureWindowsPath('C:/Windows')
    print('str_windows', str(p))
    print('repr_windows', repr(p))
except Exception as e:
    print('SKIP_String_representation', type(e).__name__, e)

# === Hash and equality ===
try:
    p1 = PurePosixPath('foo/bar')
    p2 = PurePosixPath('foo/bar')
    p3 = PurePosixPath('foo/baz')
    print('hash_equal', hash(p1) == hash(p2))
    print('eq_true', p1 == p2)
    print('eq_false', p1 == p3)

    # Cross-flavor equality
    print('eq_cross_flavor', PurePosixPath('foo') == PureWindowsPath('foo'))

    # Case sensitivity
    print('eq_case_posix', PurePosixPath('FOO') == PurePosixPath('foo'))
    print('eq_case_windows', PureWindowsPath('FOO') == PureWindowsPath('foo'))
except Exception as e:
    print('SKIP_Hash_and_equality', type(e).__name__, e)

# === Comparison ===
try:
    print('lt_posix', PurePosixPath('a') < PurePosixPath('b'))
    print('gt_posix', PurePosixPath('b') > PurePosixPath('a'))
except Exception as e:
    print('SKIP_Comparison', type(e).__name__, e)

# === Path normalization ===
try:
    print('norm_double_slash', PurePosixPath('foo//bar'))
    print('norm_dot', PurePosixPath('foo/./bar'))
    print('norm_preserve_double', PurePosixPath('//foo/bar'))
    print('norm_preserve_dotdot', PurePosixPath('foo/../bar'))
except Exception as e:
    print('SKIP_Path_normalization', type(e).__name__, e)

# === os.PathLike interface ===
try:
    import os
    p = PurePosixPath('/etc/hosts')
    print('fspath', os.fspath(p))
except Exception as e:
    print('SKIP_os_PathLike_interface', type(e).__name__, e)

# === bytes representation (Unix only) ===
try:
    p = PurePosixPath('/etc/hosts')
    print('bytes_posix', bytes(p))
except Exception as e:
    print('SKIP_bytes_representation_Unix_only', type(e).__name__, e)

# === Complex path manipulations ===
try:
    # Chaining operations
    p = PurePosixPath('/home/user/documents/report.txt')
    print('complex_chain', p.parent / p.stem / 'backup' / p.with_suffix('.bak').name)

    # Multiple suffix handling
    p = PurePosixPath('archive.tar.gz')
    print('complex_suffixes', p.suffixes)
    print('complex_stem', p.stem)
    print('complex_with_suffix', p.with_suffix('.bz2'))

    # Windows drive handling
    p = PureWindowsPath('c:/Users/pydantic/file.txt')
    print('windows_drive_parts', p.parts)
    print('windows_drive_anchor', p.anchor)
    print('windows_drive_parent', p.parent)
    print('windows_drive_name', p.name)

    # UNC path handling
    p = PureWindowsPath('//server/share/folder/file.txt')
    print('unc_parts', p.parts)
    print('unc_anchor', p.anchor)
    print('unc_drive', p.drive)
    print('unc_root', p.root)
except Exception as e:
    print('SKIP_Complex_path_manipulations', type(e).__name__, e)

# === Edge cases ===
try:
    print('edge_empty_str', PurePosixPath(''))
    print('edge_only_dots', PurePosixPath('...'))
    print('edge_many_dots', PurePosixPath('file....ext'))
    print('edge_spaces', PurePosixPath('my file.txt'))
    print('edge_special_chars', PurePosixPath('file@#$%.txt'))

    # Multiple slashes preservation
    print('edge_leading_doubleslash', PurePosixPath('//foo/bar'))
    print('edge_unc_like', PurePosixPath('//server'))

    # Empty segments are collapsed
    print('edge_empty_segments', PurePosixPath('foo', '', 'bar'))
except Exception as e:
    print('SKIP_Edge_cases', type(e).__name__, e)
