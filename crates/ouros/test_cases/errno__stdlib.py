import errno
import sys

# POSIX-standard values (same on all platforms)
assert errno.EPERM == 1
assert errno.ENOENT == 2
assert errno.ESRCH == 3
assert errno.EINTR == 4
assert errno.EIO == 5
assert errno.ENXIO == 6
assert errno.E2BIG == 7
assert errno.ENOEXEC == 8
assert errno.EBADF == 9
assert errno.ECHILD == 10
assert errno.EACCES == 13
assert errno.EBUSY == 16
assert errno.EEXIST == 17
assert errno.EINVAL == 22
assert errno.ENOSPC == 28
assert errno.EPIPE == 32

# Platform-specific values
if sys.platform == 'darwin':
    assert errno.ENAMETOOLONG == 63
    assert errno.ENOSYS == 78
    assert errno.ENOTEMPTY == 66
    assert errno.EWOULDBLOCK == errno.EAGAIN == 35
    assert errno.ENOTSUP == 45
    assert errno.EOPNOTSUPP == 102
else:
    # Linux
    assert errno.ENAMETOOLONG == 36
    assert errno.ENOSYS == 38
    assert errno.ENOTEMPTY == 39
    assert errno.EWOULDBLOCK == errno.EAGAIN == 11
    assert errno.ENOTSUP == 95
    assert errno.EOPNOTSUPP == 95

# errorcode should be a dict mapping int -> str
assert isinstance(errno.errorcode, dict)
assert errno.errorcode[1] == 'EPERM'
assert errno.errorcode[2] == 'ENOENT'
assert errno.errorcode[13] == 'EACCES'

# Alias names are constants but not canonical errorcode dict values.
assert 'EWOULDBLOCK' not in errno.errorcode.values()
