# conformance: container_protocol
# description: for-loop falls back to __getitem__ with sequential int indices when __iter__ is absent
# tags: getitem,iteration,fallback,old_style
# ---
class OldStyleSequence:
    def __init__(self, *items):
        self._items = list(items)
    def __getitem__(self, idx):
        # IndexError at end signals iteration stop (like StopIteration for __next__)
        return self._items[idx]

# for-loop calls __getitem__(0), __getitem__(1), ..., until IndexError
for x in OldStyleSequence(10, 20, 30):
    print(x)

# 'in' also uses this fallback (after __contains__ and __iter__)
print(20 in OldStyleSequence(10, 20, 30))
print(40 in OldStyleSequence(10, 20, 30))

# list() conversion uses this fallback
result = list(OldStyleSequence(1, 2, 3))
print(result)

# __getitem__ raising non-IndexError propagates
class BadSequence:
    def __getitem__(self, idx):
        raise RuntimeError(f"bad at {idx}")

try:
    for x in BadSequence():
        pass
except RuntimeError as e:
    print(f"RuntimeError: {e}")
