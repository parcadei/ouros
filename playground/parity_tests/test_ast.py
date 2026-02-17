import ast

# === compare ===
try:
    print('compare_default', ast.compare([], 0))
    print('compare_combo_req_2', ast.compare([], 1))
except Exception as e:
    print('SKIP_compare', type(e).__name__, e)

# === get_source_segment ===
try:
    print('get_source_segment_default', ast.get_source_segment(0, 0))
    print('get_source_segment_combo_req_2', ast.get_source_segment(0, 1))
except Exception as e:
    print('SKIP_get_source_segment', type(e).__name__, e)
