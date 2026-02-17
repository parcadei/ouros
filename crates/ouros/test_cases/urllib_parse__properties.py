from urllib.parse import urlparse

r = urlparse('https://api.example.com:8443/v2/users?page=3')
assert r.hostname == 'api.example.com', f'hostname: {r.hostname}'
assert r.port == 8443, f'port: {r.port}'

r = urlparse('https://example.com/path')
assert r.hostname == 'example.com'
assert r.port is None

r = urlparse('https://user:pass@host.com:443/path')
assert r.hostname == 'host.com'
assert r.port == 443
assert r.username == 'user'
assert r.password == 'pass'

r = urlparse('https://example.com/path')
assert r.username is None
assert r.password is None

r = urlparse('https://EXAMPLE.COM/path')
assert r.hostname == 'example.com'

r = urlparse('/relative/path')
assert r.hostname is None
assert r.port is None

print('ALL PASSED')
