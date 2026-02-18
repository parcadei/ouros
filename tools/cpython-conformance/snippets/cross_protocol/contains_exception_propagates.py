# conformance: cross_protocol
# description: Exception in __contains__ propagates; no fallback to __iter__ or __getitem__
# tags: contains,exception,propagation,no_fallback,cross_protocol
# ---
class ExplodingContains:
    def __contains__(self, item):
        raise RuntimeError("contains exploded")
    def __iter__(self):
        return iter([1, 2, 3])  # Should not be reached
    def __getitem__(self, idx):
        return [1, 2, 3][idx]  # Should not be reached

try:
    1 in ExplodingContains()
except RuntimeError as e:
    print(f"RuntimeError: {e}")

# Exception in __iter__ when used as __contains__ fallback
class ExplodingIter:
    def __iter__(self):
        raise ValueError("iter exploded")
    def __getitem__(self, idx):
        return [1, 2, 3][idx]  # Should not be reached (iter takes priority)

try:
    1 in ExplodingIter()
except ValueError as e:
    print(f"ValueError from __iter__: {e}")

# 'not in' with exception: exception propagates, negation not attempted
class ExplodingContains2:
    def __contains__(self, item):
        raise KeyError("nope")

try:
    1 not in ExplodingContains2()
except KeyError as e:
    print(f"KeyError from 'not in': {e}")
