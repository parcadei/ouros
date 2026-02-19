# conformance: cross_protocol
# description: Full fallback chain for 'in': __contains__ -> __iter__ -> __getitem__ (sequential integers)
# tags: contains,iter,getitem,fallback,chain,cross_protocol
# ---
# Level 1: __contains__ defined - used directly
class HasContains:
    def __contains__(self, item):
        return item in [1, 2, 3]
    def __iter__(self):
        raise RuntimeError("should not be called")
    def __getitem__(self, idx):
        raise RuntimeError("should not be called")

print(2 in HasContains())
print(4 in HasContains())

# Level 2: No __contains__, has __iter__ - iter used
class HasIter:
    def __iter__(self):
        return iter([10, 20, 30])
    def __getitem__(self, idx):
        raise RuntimeError("should not be called")

print(20 in HasIter())
print(40 in HasIter())

# Level 3: No __contains__, no __iter__, has __getitem__ - sequential int index fallback
class HasGetItem:
    def __init__(self):
        self.data = [100, 200, 300]
    def __getitem__(self, idx):
        return self.data[idx]  # Will raise IndexError when idx >= 3, stopping iteration

print(200 in HasGetItem())
print(400 in HasGetItem())
