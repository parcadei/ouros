import errno

# Core values required by parity task.
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
assert errno.ENAMETOOLONG == 63
assert errno.ENOSYS == 78
assert errno.ENOTEMPTY == 66

# Darwin alias behavior.
assert errno.EWOULDBLOCK == errno.EAGAIN == 35
assert errno.ENOTSUP == 45
assert errno.EOPNOTSUPP == 102

expected_errorcode = {
    1: 'EPERM',
    2: 'ENOENT',
    3: 'ESRCH',
    4: 'EINTR',
    5: 'EIO',
    6: 'ENXIO',
    7: 'E2BIG',
    8: 'ENOEXEC',
    9: 'EBADF',
    10: 'ECHILD',
    11: 'EDEADLK',
    12: 'ENOMEM',
    13: 'EACCES',
    14: 'EFAULT',
    15: 'ENOTBLK',
    16: 'EBUSY',
    17: 'EEXIST',
    18: 'EXDEV',
    19: 'ENODEV',
    20: 'ENOTDIR',
    21: 'EISDIR',
    22: 'EINVAL',
    23: 'ENFILE',
    24: 'EMFILE',
    25: 'ENOTTY',
    26: 'ETXTBSY',
    27: 'EFBIG',
    28: 'ENOSPC',
    29: 'ESPIPE',
    30: 'EROFS',
    31: 'EMLINK',
    32: 'EPIPE',
    33: 'EDOM',
    34: 'ERANGE',
    35: 'EAGAIN',
    36: 'EINPROGRESS',
    37: 'EALREADY',
    38: 'ENOTSOCK',
    39: 'EDESTADDRREQ',
    40: 'EMSGSIZE',
    41: 'EPROTOTYPE',
    42: 'ENOPROTOOPT',
    43: 'EPROTONOSUPPORT',
    44: 'ESOCKTNOSUPPORT',
    45: 'ENOTSUP',
    46: 'EPFNOSUPPORT',
    47: 'EAFNOSUPPORT',
    48: 'EADDRINUSE',
    49: 'EADDRNOTAVAIL',
    50: 'ENETDOWN',
    51: 'ENETUNREACH',
    52: 'ENETRESET',
    53: 'ECONNABORTED',
    54: 'ECONNRESET',
    55: 'ENOBUFS',
    56: 'EISCONN',
    57: 'ENOTCONN',
    58: 'ESHUTDOWN',
    59: 'ETOOMANYREFS',
    60: 'ETIMEDOUT',
    61: 'ECONNREFUSED',
    62: 'ELOOP',
    63: 'ENAMETOOLONG',
    64: 'EHOSTDOWN',
    65: 'EHOSTUNREACH',
    66: 'ENOTEMPTY',
    67: 'EPROCLIM',
    68: 'EUSERS',
    69: 'EDQUOT',
    70: 'ESTALE',
    71: 'EREMOTE',
    72: 'EBADRPC',
    73: 'ERPCMISMATCH',
    74: 'EPROGUNAVAIL',
    75: 'EPROGMISMATCH',
    76: 'EPROCUNAVAIL',
    77: 'ENOLCK',
    78: 'ENOSYS',
    79: 'EFTYPE',
    80: 'EAUTH',
    81: 'ENEEDAUTH',
    82: 'EPWROFF',
    83: 'EDEVERR',
    84: 'EOVERFLOW',
    85: 'EBADEXEC',
    86: 'EBADARCH',
    87: 'ESHLIBVERS',
    88: 'EBADMACHO',
    89: 'ECANCELED',
    90: 'EIDRM',
    91: 'ENOMSG',
    92: 'EILSEQ',
    93: 'ENOATTR',
    94: 'EBADMSG',
    95: 'EMULTIHOP',
    96: 'ENODATA',
    97: 'ENOLINK',
    98: 'ENOSR',
    99: 'ENOSTR',
    100: 'EPROTO',
    101: 'ETIME',
    102: 'EOPNOTSUPP',
    103: 'ENOPOLICY',
    104: 'ENOTRECOVERABLE',
    105: 'EOWNERDEAD',
    106: 'EQFULL',
    107: 'ENOTCAPABLE',
}

assert errno.errorcode == expected_errorcode
assert len(errno.errorcode) == 107

for code, name in expected_errorcode.items():
    assert getattr(errno, name) == code

# Alias names are constants but not canonical errorcode dict values.
assert 'EWOULDBLOCK' not in errno.errorcode.values()
assert errno.EWOULDBLOCK == 35
