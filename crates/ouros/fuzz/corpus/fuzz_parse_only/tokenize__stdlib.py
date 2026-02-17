import tokenize

# === ISEOF ===
assert tokenize.ISEOF(0) == True, 'iseof_default'
assert tokenize.ISEOF(x=0) == True, 'iseof_required_keyword_form'
assert tokenize.ISEOF(1) == False, 'iseof_combo_req_2'
assert tokenize.ISEOF(-1) == False, 'iseof_combo_req_3'

# === ISNONTERMINAL ===
assert tokenize.ISNONTERMINAL(0) == False, 'isnonterminal_default'
assert tokenize.ISNONTERMINAL(x=0) == False, 'isnonterminal_required_keyword_form'
assert tokenize.ISNONTERMINAL(1) == False, 'isnonterminal_combo_req_2'
assert tokenize.ISNONTERMINAL(-1) == False, 'isnonterminal_combo_req_3'

# === ISTERMINAL ===
assert tokenize.ISTERMINAL(0) == True, 'isterminal_default'
assert tokenize.ISTERMINAL(x=0) == True, 'isterminal_required_keyword_form'
assert tokenize.ISTERMINAL(1) == True, 'isterminal_combo_req_2'
assert tokenize.ISTERMINAL(-1) == True, 'isterminal_combo_req_3'

# === untokenize ===
assert tokenize.untokenize([]) == '', 'untokenize_default'
assert tokenize.untokenize(iterable=[]) == '', 'untokenize_required_keyword_form'
