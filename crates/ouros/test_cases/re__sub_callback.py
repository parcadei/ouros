import re

# === re.sub with function callback ===
def upper_match(match):
    return match.group(0).upper()

result = re.sub(r'\b\w+\b', upper_match, 'hello world')
assert result == 'HELLO WORLD', f'expected HELLO WORLD, got {result}'

# === re.sub with lambda callback ===
result = re.sub(r'(\d+)', lambda m: str(int(m.group(1)) * 2), 'a1b2c3')
assert result == 'a2b4c6', f'expected a2b4c6, got {result}'

# === re.sub callback accessing groups ===
def swap_groups(m):
    return m.group(2) + '=' + m.group(1)

result = re.sub(r'(\w+):(\w+)', swap_groups, 'key:value')
assert result == 'value=key', f'expected value=key, got {result}'

# === re.sub callback with closure ===
env = {'NAME': 'World', 'GREETING': 'Hello'}
def replacer(m):
    key = m.group(1)
    return env.get(key, m.group(0))

result = re.sub(r'\$\{(\w+)\}', replacer, '${GREETING}, ${NAME}!')
assert result == 'Hello, World!', f'expected Hello, World!, got {result}'

# === re.sub callback with count limit ===
result = re.sub(r'\d+', lambda m: 'X', 'a1b2c3', count=2)
assert result == 'aXbXc3', f'expected aXbXc3, got {result}'

# === re.subn with callback ===
result, count = re.subn(r'\d+', lambda m: str(int(m.group(0)) + 10), 'a1b2c3')
assert result == 'a11b12c13', f'expected a11b12c13, got {result}'
assert count == 3, f'expected 3 substitutions, got {count}'

# === Pattern.sub with callback ===
pat = re.compile(r'\d+')
result = pat.sub(lambda m: str(int(m.group(0)) * 3), '1-2-3')
assert result == '3-6-9', f'expected 3-6-9, got {result}'

# === Pattern.subn with callback ===
result, count = pat.subn(lambda m: str(int(m.group(0)) * 3), '1-2-3')
assert result == '3-6-9', f'expected 3-6-9, got {result}'
assert count == 3, f'expected 3 substitutions, got {count}'

# === re.sub callback returning empty string ===
result = re.sub(r'\d', lambda m: '', 'a1b2c3')
assert result == 'abc', f'expected abc, got {result}'

# === re.sub callback with no matches ===
result = re.sub(r'\d', lambda m: 'X', 'abc')
assert result == 'abc', f'expected abc, got {result}'
