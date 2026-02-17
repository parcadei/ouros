import shlex

# === quote ===
assert shlex.quote('') == "''", 'quote_default'
assert shlex.quote(s='') == "''", 'quote_required_keyword_form'
assert shlex.quote('hello') == 'hello', 'quote_combo_req_2'
assert shlex.quote('<b>hi</b>') == "'<b>hi</b>'", 'quote_combo_req_3'

# === split ===
assert shlex.split('') == [], 'split_default'
assert shlex.split(s='') == [], 'split_required_keyword_form'
assert shlex.split('hello') == ['hello'], 'split_combo_req_2'
assert shlex.split('<b>hi</b>') == ['<b>hi</b>'], 'split_combo_req_3'
assert shlex.split('', comments=False) == [], 'split_opt_comments_1'
assert shlex.split('', comments=0) == [], 'split_opt_comments_2'
assert shlex.split('', posix=True) == [], 'split_opt_posix_1'
assert shlex.split('', posix=0) == [], 'split_opt_posix_2'
assert shlex.split('', comments=False, posix=True) == [], 'split_optpair_comments_posix'
assert shlex.split('', comments=False, posix=0) == [], 'split_optpair_comments_posix_2'
assert shlex.split('', comments=0, posix=True) == [], 'split_optpair_comments_posix_3'
assert shlex.split('', comments=0, posix=0) == [], 'split_optpair_comments_posix_4'
