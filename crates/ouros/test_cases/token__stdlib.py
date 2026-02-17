import token


# === constants ===
assert token.ENDMARKER == 0, 'ENDMARKER'
assert token.NAME == 1, 'NAME'
assert token.NUMBER == 2, 'NUMBER'
assert token.STRING == 3, 'STRING'
assert token.NEWLINE == 4, 'NEWLINE'
assert token.INDENT == 5, 'INDENT'
assert token.DEDENT == 6, 'DEDENT'
assert token.EXCLAMATION == 54, 'EXCLAMATION'
assert token.OP == 55, 'OP'
assert token.TYPE_IGNORE == 56, 'TYPE_IGNORE'
assert token.TYPE_COMMENT == 57, 'TYPE_COMMENT'
assert token.SOFT_KEYWORD == 58, 'SOFT_KEYWORD'
assert token.FSTRING_START == 59, 'FSTRING_START'
assert token.FSTRING_MIDDLE == 60, 'FSTRING_MIDDLE'
assert token.FSTRING_END == 61, 'FSTRING_END'
assert token.TSTRING_START == 62, 'TSTRING_START'
assert token.TSTRING_MIDDLE == 63, 'TSTRING_MIDDLE'
assert token.TSTRING_END == 64, 'TSTRING_END'
assert token.COMMENT == 65, 'COMMENT'
assert token.NL == 66, 'NL'
assert token.ERRORTOKEN == 67, 'ERRORTOKEN'
assert token.ENCODING == 68, 'ENCODING'
assert token.N_TOKENS == 69, 'N_TOKENS'
assert token.NT_OFFSET == 256, 'NT_OFFSET'


# === token names ===
assert token.tok_name[0] == 'ENDMARKER', 'tok_name_endmarker'
assert token.tok_name[token.NAME] == 'NAME', 'tok_name_name'
assert token.tok_name[token.COLONEQUAL] == 'COLONEQUAL', 'tok_name_colonequal'


# === exact token types ===
assert token.EXACT_TOKEN_TYPES['('] == token.LPAR, 'exact_lpar'
assert token.EXACT_TOKEN_TYPES[')'] == token.RPAR, 'exact_rpar'
assert token.EXACT_TOKEN_TYPES['//'] == token.DOUBLESLASH, 'exact_doubleslash'
assert token.EXACT_TOKEN_TYPES[':='] == token.COLONEQUAL, 'exact_walrus'
assert token.EXACT_TOKEN_TYPES['...'] == token.ELLIPSIS, 'exact_ellipsis'
assert token.EXACT_TOKEN_TYPES['!'] == token.EXCLAMATION, 'exact_exclamation'


# === helper predicates ===
assert token.ISTERMINAL(0) == True, 'isterminal_zero'
assert token.ISTERMINAL(1) == True, 'isterminal_name'
assert token.ISTERMINAL(-1) == True, 'isterminal_negative'
assert token.ISTERMINAL(x=0) == True, 'isterminal_keyword'

assert token.ISNONTERMINAL(0) == False, 'isnonterminal_zero'
assert token.ISNONTERMINAL(256) == True, 'isnonterminal_offset'
assert token.ISNONTERMINAL(-1) == False, 'isnonterminal_negative'
assert token.ISNONTERMINAL(x=0) == False, 'isnonterminal_keyword'

assert token.ISEOF(0) == True, 'iseof_zero'
assert token.ISEOF(1) == False, 'iseof_name'
assert token.ISEOF(-1) == False, 'iseof_negative'
assert token.ISEOF(x=0) == True, 'iseof_keyword'
