import tokenize

# === untokenize ===
try:
    print('untokenize_default', tokenize.untokenize([]))
except Exception as e:
    print('SKIP_untokenize', type(e).__name__, e)
