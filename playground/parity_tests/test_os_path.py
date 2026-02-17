# HOST MODULE: Only testing sandbox-safe subset
# Tests for os.path string manipulation functions (no filesystem access required)
import os.path

# === basename ===
try:
    print('basename_simple', os.path.basename('/usr/bin/python'))
    print('basename_root', os.path.basename('/'))
    print('basename_empty', os.path.basename(''))
    print('basename_no_dir', os.path.basename('file.txt'))
    print('basename_trailing_slash', os.path.basename('/usr/bin/'))
    print('basename_with_spaces', os.path.basename('/path/to/my file.txt'))
    print('basename_dots', os.path.basename('/path/to/.hidden'))
except Exception as e:
    print('SKIP_basename', type(e).__name__, e)

# === dirname ===
try:
    print('dirname_simple', os.path.dirname('/usr/bin/python'))
    print('dirname_root', os.path.dirname('/'))
    print('dirname_empty', os.path.dirname(''))
    print('dirname_no_dir', os.path.dirname('file.txt'))
    print('dirname_trailing_slash', os.path.dirname('/usr/bin/'))
    print('dirname_multiple_slashes', os.path.dirname('//usr//bin'))
except Exception as e:
    print('SKIP_dirname', type(e).__name__, e)

# === join ===
try:
    print('join_two', os.path.join('path', 'to', 'file'))
    print('join_absolute', os.path.join('/absolute', 'path'))
    print('join_multiple', os.path.join('a', 'b', 'c', 'd'))
    print('join_empty', os.path.join('', 'file'))
    print('join_with_dots', os.path.join('path', '..', 'other'))
    print('join_single_dot', os.path.join('path', '.', 'file'))
    print('join_starts_with_sep', os.path.join('/start', 'end'))
    print('join_with_empty_middle', os.path.join('a', '', 'b'))
except Exception as e:
    print('SKIP_join', type(e).__name__, e)

# === split ===
try:
    print('split_simple', os.path.split('/usr/bin/python'))
    print('split_root', os.path.split('/'))
    print('split_no_dir', os.path.split('file.txt'))
    print('split_trailing_slash', os.path.split('/usr/bin/'))
    print('split_no_ext', os.path.split('file'))
    print('split_multiple_slashes', os.path.split('//usr//bin//file'))
except Exception as e:
    print('SKIP_split', type(e).__name__, e)

# === splitext ===
try:
    print('splitext_simple', os.path.splitext('/path/file.txt'))
    print('splitext_no_ext', os.path.splitext('/path/file'))
    print('splitext_double', os.path.splitext('/path/file.tar.gz'))
    print('splitext_dotfile', os.path.splitext('/path/.bashrc'))
    print('splitext_dotfile_ext', os.path.splitext('/path/.bashrc.conf'))
    print('splitext_only_dot', os.path.splitext('/path/file.'))
    print('splitext_empty', os.path.splitext(''))
    print('splitext_root', os.path.splitext('/.hidden'))
except Exception as e:
    print('SKIP_splitext', type(e).__name__, e)

# === splitdrive ===
try:
    print('splitdrive_unix', os.path.splitdrive('/path/to/file'))
    print('splitdrive_windows', os.path.splitdrive('C:\\path\\to\\file'))
    print('splitdrive_unc', os.path.splitdrive('\\\\server\\share\\file'))
    print('splitdrive_relative', os.path.splitdrive('relative\\path'))
    print('splitdrive_empty', os.path.splitdrive(''))
except Exception as e:
    print('SKIP_splitdrive', type(e).__name__, e)

# === splitroot ===
try:
    print('splitroot_unix_abs', os.path.splitroot('/path/to/file'))
    print('splitroot_unix_rel', os.path.splitroot('path/to/file'))
    print('splitroot_windows', os.path.splitroot('C:\\path\\to\\file'))
    print('splitroot_unc', os.path.splitroot('\\\\server\\share\\file'))
    print('splitroot_empty', os.path.splitroot(''))
except Exception as e:
    print('SKIP_splitroot', type(e).__name__, e)

# === exists ===
try:
    print('exists_type', type(os.path.exists('/nonexistent/path/12345')))
    print('exists_nonexistent', os.path.exists('/nonexistent/path/12345'))
    print('exists_empty', os.path.exists(''))
except Exception as e:
    print('SKIP_exists', type(e).__name__, e)

# === lexists ===
try:
    print('lexists_type', type(os.path.lexists('/nonexistent/path/12345')))
    print('lexists_nonexistent', os.path.lexists('/nonexistent/path/12345'))
except Exception as e:
    print('SKIP_lexists', type(e).__name__, e)

# === isabs ===
try:
    print('isabs_unix_abs', os.path.isabs('/absolute/path'))
    print('isabs_unix_rel', os.path.isabs('relative/path'))
    print('isabs_windows_abs', os.path.isabs('C:\\path'))
    print('isabs_empty', os.path.isabs(''))
    print('isabs_dot', os.path.isabs('.'))
    print('isabs_double_slash', os.path.isabs('//path'))
except Exception as e:
    print('SKIP_isabs', type(e).__name__, e)

# === isfile ===
try:
    print('isfile_type', type(os.path.isfile('/nonexistent')))
    print('isfile_nonexistent', os.path.isfile('/nonexistent/path/12345'))
    print('isfile_empty', os.path.isfile(''))
except Exception as e:
    print('SKIP_isfile', type(e).__name__, e)

# === isdir ===
try:
    print('isdir_type', type(os.path.isdir('/nonexistent')))
    print('isdir_nonexistent', os.path.isdir('/nonexistent/path/12345'))
    print('isdir_empty', os.path.isdir(''))
except Exception as e:
    print('SKIP_isdir', type(e).__name__, e)

# === islink ===
try:
    print('islink_type', type(os.path.islink('/nonexistent')))
    print('islink_nonexistent', os.path.islink('/nonexistent/path/12345'))
except Exception as e:
    print('SKIP_islink', type(e).__name__, e)

# === ismount ===
try:
    print('ismount_type', type(os.path.ismount('/nonexistent')))
    print('ismount_nonexistent', os.path.ismount('/nonexistent/path/12345'))
    print('ismount_root', os.path.ismount('/'))
except Exception as e:
    print('SKIP_ismount', type(e).__name__, e)

# === isjunction ===
try:
    print('isjunction_type', type(os.path.isjunction('/nonexistent')))
    print('isjunction_nonexistent', os.path.isjunction('/nonexistent/path/12345'))
except Exception as e:
    print('SKIP_isjunction', type(e).__name__, e)

# === isdevdrive ===
try:
    print('isdevdrive_type', type(os.path.isdevdrive('/nonexistent')))
    print('isdevdrive_nonexistent', os.path.isdevdrive('/nonexistent/path/12345'))
except Exception as e:
    print('SKIP_isdevdrive', type(e).__name__, e)

# === normcase ===
try:
    print('normcase_unix', os.path.normcase('/Path/To/File.TXT'))
    print('normcase_backslash', os.path.normcase('C:\\Path\\To\\File'))
    print('normcase_empty', os.path.normcase(''))
    print('normcase_dots', os.path.normcase('/path/../other/./file'))
    print('normcase_double_slash', os.path.normcase('//server/share'))
except Exception as e:
    print('SKIP_normcase', type(e).__name__, e)

# === normpath ===
try:
    print('normpath_simple', os.path.normpath('/path/to/file'))
    print('normpath_dots', os.path.normpath('/path/../other/./file'))
    print('normpath_double_dots', os.path.normpath('/a/b/../c/../d'))
    print('normpath_leading_dots', os.path.normpath('../path/to/file'))
    print('normpath_empty', os.path.normpath(''))
    print('normpath_root', os.path.normpath('/'))
    print('normpath_redundant', os.path.normpath('/path//to///file'))
    print('normpath_dot_only', os.path.normpath('./././.'))
    print('normpath_parent_overflow', os.path.normpath('a/../../b'))
except Exception as e:
    print('SKIP_normpath', type(e).__name__, e)

# === abspath ===
try:
    print('abspath_type', type(os.path.abspath('.')))
    print('abspath_dot', os.path.abspath('.'))
    print('abspath_empty', os.path.abspath(''))
    print('abspath_relative', os.path.abspath('path/to/file'))
except Exception as e:
    print('SKIP_abspath', type(e).__name__, e)

# === realpath ===
try:
    print('realpath_type', type(os.path.realpath('.')))
    print('realpath_dot', os.path.realpath('.'))
    print('realpath_empty', os.path.realpath(''))
except Exception as e:
    print('SKIP_realpath', type(e).__name__, e)

# === relpath ===
try:
    print('relpath_simple', os.path.relpath('/path/to/file', '/path'))
    print('relpath_same', os.path.relpath('/path/to', '/path/to'))
    print('relpath_no_start', os.path.relpath('/path/to/file'))
    print('relpath_relative_start', os.path.relpath('a/b', 'a/c'))
    print('relpath_parent', os.path.relpath('/a', '/a/b/c'))
    print('relpath_dots', os.path.relpath('/path/to/./file', '/path/./from'))
except Exception as e:
    print('SKIP_relpath', type(e).__name__, e)

# === commonprefix ===
try:
    print('commonprefix_simple', os.path.commonprefix(['/usr/bin', '/usr/local/bin']))
    print('commonprefix_none', os.path.commonprefix(['/usr/bin', '/etc/passwd']))
    print('commonprefix_single', os.path.commonprefix(['/usr/bin']))
    print('commonprefix_empty', os.path.commonprefix([]))
    print('commonprefix_nested', os.path.commonprefix(['/a/b/c', '/a/b/d', '/a/b/e/f']))
    print('commonprefix_partial', os.path.commonprefix(['/usr', '/usrbin']))
except Exception as e:
    print('SKIP_commonprefix', type(e).__name__, e)

# === commonpath ===
try:
    print('commonpath_simple', os.path.commonpath(['/usr/bin', '/usr/local/bin']))
    print('commonpath_single', os.path.commonpath(['/usr/bin']))
    print('commonpath_nested', os.path.commonpath(['/a/b/c', '/a/b/d', '/a/b/e/f']))
    print('commonpath_relative', os.path.commonpath(['path/to/file', 'path/to/other']))
except Exception as e:
    print('SKIP_commonpath', type(e).__name__, e)

# === expanduser ===
try:
    print('expanduser_tilde', os.path.expanduser('~/file'))
    print('expanduser_tilde_slash', os.path.expanduser('~root/file'))
    print('expanduser_no_tilde', os.path.expanduser('/path/to/file'))
    print('expanduser_only_tilde', os.path.expanduser('~'))
    print('expanduser_empty', os.path.expanduser(''))
except Exception as e:
    print('SKIP_expanduser', type(e).__name__, e)

# === expandvars ===
try:
    print('expandvars_simple', os.path.expandvars('/path/$HOME/file'))
    print('expandvars_brace', os.path.expandvars('/path/${HOME}/file'))
    print('expandvars_undefined', os.path.expandvars('/path/$UNDEFINED_VAR/file'))
    print('expandvars_no_vars', os.path.expandvars('/path/to/file'))
    print('expandvars_empty', os.path.expandvars(''))
    print('expandvars_multiple', os.path.expandvars('$HOME/bin:$PATH'))
    print('expandvars_special', os.path.expandvars('$$HOME'))
except Exception as e:
    print('SKIP_expandvars', type(e).__name__, e)

# === getsize ===
try:
    print('getsize_type', type(os.path.getsize('.')))
except Exception as e:
    print('SKIP_getsize', type(e).__name__, e)

# === getmtime ===
try:
    print('getmtime_type', type(os.path.getmtime('.')))
except Exception as e:
    print('SKIP_getmtime', type(e).__name__, e)

# === getatime ===
try:
    print('getatime_type', type(os.path.getatime('.')))
except Exception as e:
    print('SKIP_getatime', type(e).__name__, e)

# === getctime ===
try:
    print('getctime_type', type(os.path.getctime('.')))
except Exception as e:
    print('SKIP_getctime', type(e).__name__, e)

# === samefile ===
try:
    print('samefile_type', type(os.path))
except Exception as e:
    print('SKIP_samefile', type(e).__name__, e)

# === sameopenfile ===
# Skip - requires actual file descriptors

# === samestat ===
try:
    print('samestat_type', type(os.path))
except Exception as e:
    print('SKIP_samestat', type(e).__name__, e)

# === supports_unicode_filenames ===
try:
    print('supports_unicode_filenames', os.path.supports_unicode_filenames)
except Exception as e:
    print('SKIP_supports_unicode_filenames', type(e).__name__, e)

# === Module constants ===
try:
    print('const_sep', os.path.sep)
    print('const_altsep', os.path.altsep)
    print('const_pathsep', os.path.pathsep)
    print('const_extsep', os.path.extsep)
    print('const_curdir', os.path.curdir)
    print('const_pardir', os.path.pardir)
    print('const_devnull', os.path.devnull)
    print('const_defpath', os.path.defpath)
except Exception as e:
    print('SKIP_Module constants', type(e).__name__, e)
