# === Swap list elements via unpack assignment to subscript targets ===
arr = [1, 2, 3, 4, 5]
i = 1
j = 3
arr[i], arr[j] = arr[j], arr[i]
assert arr == [1, 4, 3, 2, 5], f'expected [1, 4, 3, 2, 5], got {arr}'

# === Assign multiple dict entries via unpack assignment to subscripts ===
d = {}
d['a'], d['b'] = 1, 2
assert d['a'] == 1, 'expected d["a"] to be 1, got {}'.format(d['a'])
assert d['b'] == 2, 'expected d["b"] to be 2, got {}'.format(d['b'])
