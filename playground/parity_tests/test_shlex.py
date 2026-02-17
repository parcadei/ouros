import shlex

# === quote ===
try:
    print('quote_default', shlex.quote(''))
    print('quote_combo_req_2', shlex.quote('hello'))
except Exception as e:
    print('SKIP_quote', type(e).__name__, e)

# === split ===
try:
    print('split_default', shlex.split(''))
    print('split_combo_req_2', shlex.split('hello'))
except Exception as e:
    print('SKIP_split', type(e).__name__, e)
