import string

# === capwords: basic usage ===
assert string.capwords('hello world') == 'Hello World', 'capwords basic'
assert string.capwords('HELLO WORLD') == 'Hello World', 'capwords all caps'
assert string.capwords('hello') == 'Hello', 'capwords single word'
assert string.capwords('') == '', 'capwords empty string'

# === capwords: whitespace handling ===
assert string.capwords('  hello   world  ') == 'Hello World', 'capwords collapses whitespace'
assert string.capwords('already Capitalized Words') == 'Already Capitalized Words', 'capwords mixed case'

# === capwords: with sep ===
assert string.capwords('hello-world', '-') == 'Hello-World', 'capwords with sep'

# === Formatter ===
formatter = string.Formatter()
assert formatter is not None, 'Formatter() returns an object'
assert formatter.format('Hello {} {name}', 'world', name='!') == 'Hello world !', 'Formatter.format basic usage'
assert formatter.format('{{x}} {}', 7) == '{x} 7', 'Formatter.format escaped braces'

# === Template ===
template = string.Template('Hello $name $$ ${thing}')
assert template.template == 'Hello $name $$ ${thing}', 'Template exposes template source'
assert template.substitute(name='Ouros', thing='World') == 'Hello Ouros $ World', (
    'Template.substitute replaces placeholders'
)
assert template.safe_substitute(thing='World') == 'Hello $name $ World', (
    'safe_substitute preserves missing placeholders'
)
