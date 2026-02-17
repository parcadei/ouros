# === Basic ExceptionGroup split ===
try:
    raise ExceptionGroup('errors', [ValueError('bad'), TypeError('wrong')])
except* ValueError as eg:
    assert len(eg.exceptions) == 1, 'should catch ValueError subgroup'
except* TypeError as eg:
    assert len(eg.exceptions) == 1, 'should catch TypeError subgroup'

print('except* basic passed')

# === ExceptionGroup with single type ===
try:
    raise ExceptionGroup('test', [ValueError('a'), ValueError('b')])
except* ValueError as eg:
    assert len(eg.exceptions) == 2, 'should catch both ValueErrors'

print('except* multi passed')
