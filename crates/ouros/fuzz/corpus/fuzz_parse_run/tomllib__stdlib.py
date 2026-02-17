import tomllib

# === loads ===
assert tomllib.loads('') == {}, 'loads_default'
assert tomllib.loads('', parse_float=0.0) == {}, 'loads_opt_parse_float_2'
