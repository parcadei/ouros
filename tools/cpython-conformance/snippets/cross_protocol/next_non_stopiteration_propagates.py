# conformance: cross_protocol
# description: Non-StopIteration exception from __next__ propagates through for-loop
# tags: next,stopiteration,exception,for_loop,propagation,cross_protocol
# ---
class BrokenIter:
    def __init__(self):
        self.count = 0
    def __iter__(self):
        return self
    def __next__(self):
        self.count += 1
        if self.count <= 2:
            return self.count
        raise ValueError("iteration broke at 3")

# For-loop catches StopIteration but NOT other exceptions
try:
    for x in BrokenIter():
        print(x)
except ValueError as e:
    print(f"ValueError propagated: {e}")

# StopIteration properly ends the loop (no exception visible)
class CleanIter:
    def __init__(self):
        self.count = 0
    def __iter__(self):
        return self
    def __next__(self):
        self.count += 1
        if self.count > 3:
            raise StopIteration
        return self.count

for x in CleanIter():
    print(x)
print("loop ended cleanly")

# RuntimeError during iteration also propagates
class RuntimeIter:
    def __iter__(self):
        return self
    def __next__(self):
        raise RuntimeError("runtime boom")

try:
    for x in RuntimeIter():
        pass
except RuntimeError as e:
    print(f"RuntimeError: {e}")
