import tomllib

# === loads ===
try:
    print('loads_default', tomllib.loads(''))
except Exception as e:
    print('SKIP_loads', type(e).__name__, e)
