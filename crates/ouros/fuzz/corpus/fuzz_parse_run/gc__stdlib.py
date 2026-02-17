import gc

# === collect / enabled state ===
assert isinstance(gc.collect(), int), 'collect_returns_int'
assert gc.disable() is None, 'disable_returns_none'
assert gc.isenabled() is False, 'isenabled_after_disable'
assert gc.enable() is None, 'enable_returns_none'
assert gc.isenabled() is True, 'isenabled_after_enable'

# === thresholds ===
thresholds = gc.get_threshold()
assert isinstance(thresholds, tuple), 'thresholds_type'
assert len(thresholds) == 3, 'thresholds_len'
assert all(isinstance(v, int) for v in thresholds), 'thresholds_ints'
assert gc.set_threshold(*thresholds) is None, 'set_threshold_roundtrip'
assert gc.get_threshold() == thresholds, 'thresholds_roundtrip'

# === debug flags ===
debug_before = gc.get_debug()
assert isinstance(debug_before, int), 'get_debug_type'
assert gc.set_debug(debug_before | 1) is None, 'set_debug_returns_none'
assert isinstance(gc.get_debug(), int), 'get_debug_after_set_type'
assert gc.set_debug(debug_before) is None, 'set_debug_restore'

# === stats and counts ===
counts = gc.get_count()
assert isinstance(counts, tuple), 'get_count_type'
assert len(counts) == 3, 'get_count_len'
assert all(isinstance(v, int) for v in counts), 'get_count_values'

stats = gc.get_stats()
assert isinstance(stats, list), 'get_stats_type'
for row in stats:
    assert isinstance(row, dict), 'get_stats_row_type'
    assert set(row.keys()) == {'collections', 'collected', 'uncollectable'}, 'get_stats_row_keys'

# === referents/referrers and object predicates ===
assert gc.get_referents() == [], 'get_referents_empty'
assert gc.get_referrers() == [], 'get_referrers_empty'
assert isinstance(gc.is_tracked([]), bool), 'is_tracked_bool'
assert isinstance(gc.is_finalized(object()), bool), 'is_finalized_bool'

# === freeze compatibility hooks ===
assert gc.freeze() is None, 'freeze_returns_none'
assert isinstance(gc.get_freeze_count(), int), 'get_freeze_count_type'
assert gc.unfreeze() is None, 'unfreeze_returns_none'
