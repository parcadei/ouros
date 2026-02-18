# conformance: container_protocol
# description: __len__ for len() builtin
# tags: len,builtin
# ---
class Sized:
    def __init__(self, n):
        self.n = n
    def __len__(self):
        return self.n

print(len(Sized(0)))
print(len(Sized(5)))
print(len(Sized(100)))
