import gc

# === collect / enabled ===
try:
    print('collect_type', type(gc.collect()).__name__)
    print('disable_result', gc.disable())
    print('isenabled_after_disable', gc.isenabled())
    print('enable_result', gc.enable())
    print('isenabled_after_enable', gc.isenabled())
except Exception as e:
    print('SKIP_collect_enabled', type(e).__name__, e)

# === thresholds/debug ===
try:
    thresholds = gc.get_threshold()
    print('thresholds_type', type(thresholds).__name__)
    print('thresholds_len', len(thresholds))
    print('thresholds_items_type', [type(x).__name__ for x in thresholds])
    print('set_threshold_result', gc.set_threshold(*thresholds))
    print('thresholds_roundtrip', gc.get_threshold() == thresholds)

    debug = gc.get_debug()
    print('get_debug_type', type(debug).__name__)
    print('set_debug_result', gc.set_debug(debug))
except Exception as e:
    print('SKIP_thresholds_debug', type(e).__name__, e)

# === stats/object helpers ===
try:
    counts = gc.get_count()
    print('get_count_type', type(counts).__name__)
    print('get_count_len', len(counts))
    print('get_count_items_type', [type(x).__name__ for x in counts])

    stats = gc.get_stats()
    print('get_stats_type', type(stats).__name__)
    print('get_stats_row_types', [type(row).__name__ for row in stats])
    print('get_stats_keys', [sorted(row.keys()) for row in stats])

    print('get_referents', gc.get_referents())
    print('get_referrers', gc.get_referrers())
    print('is_tracked_type', type(gc.is_tracked([])).__name__)
    print('is_finalized_type', type(gc.is_finalized(object())).__name__)
except Exception as e:
    print('SKIP_stats_helpers', type(e).__name__, e)

# === freeze hooks ===
try:
    print('freeze_result', gc.freeze())
    print('get_freeze_count_type', type(gc.get_freeze_count()).__name__)
    print('unfreeze_result', gc.unfreeze())
except Exception as e:
    print('SKIP_freeze', type(e).__name__, e)
