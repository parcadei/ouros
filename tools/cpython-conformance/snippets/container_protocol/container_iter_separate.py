# conformance: container_protocol
# description: __iter__ returns a separate iterator object
# tags: iter,next,separate_iterator
# ---
class Range3:
    def __iter__(self):
        return Range3Iter()

class Range3Iter:
    def __init__(self):
        self.i = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self.i >= 3:
            raise StopIteration
        val = self.i
        self.i += 1
        return val

r = Range3()
for x in r:
    print(x)
# Should be reusable since __iter__ creates new iterator each time
for x in r:
    print(x)
