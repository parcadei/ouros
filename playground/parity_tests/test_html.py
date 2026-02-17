import html

# === escape ===
try:
    print('escape_default', html.escape(''))
    print('escape_combo_req_2', html.escape('hello'))
except Exception as e:
    print('SKIP_escape', type(e).__name__, e)

# === unescape ===
try:
    print('unescape_default', html.unescape(''))
    print('unescape_combo_req_2', html.unescape('hello'))
except Exception as e:
    print('SKIP_unescape', type(e).__name__, e)
