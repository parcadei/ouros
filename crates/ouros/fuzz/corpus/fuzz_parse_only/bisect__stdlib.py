import bisect

# === bisect aliases ===
values = [1, 2, 4, 4, 5]
assert bisect.bisect(values, 4) == 4, 'bisect alias uses bisect_right'

# === insort aliases ===
values = [1, 2, 4, 4, 5]
bisect.insort(values, 4)
assert values == [1, 2, 4, 4, 4, 5], 'insort alias uses insort_right'
