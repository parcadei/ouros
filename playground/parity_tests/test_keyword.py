import keyword

# === iskeyword ===
try:
    print('iskeyword_default', keyword.iskeyword(0))
    print('iskeyword_combo_req_2', keyword.iskeyword(1))
except Exception as e:
    print('SKIP_iskeyword', type(e).__name__, e)

# === issoftkeyword ===
try:
    print('issoftkeyword_default', keyword.issoftkeyword(0))
    print('issoftkeyword_combo_req_2', keyword.issoftkeyword(1))
except Exception as e:
    print('SKIP_issoftkeyword', type(e).__name__, e)
