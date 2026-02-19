# conformance: container_protocol
# description: __iter__ + __next__ protocol for custom iterator
# tags: iter,next,iterator
# ---
class CountUp:
    def __init__(self, limit):
        self.limit = limit
        self.current = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self.current >= self.limit:
            raise StopIteration
        val = self.current
        self.current += 1
        return val

for x in CountUp(5):
    print(x)
