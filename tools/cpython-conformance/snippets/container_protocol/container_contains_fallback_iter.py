# conformance: container_protocol
# description: 'in' falls back to __iter__ when __contains__ is not defined
# tags: contains,iter,fallback,in
# ---
class IterOnly:
    def __init__(self, *args):
        self.data = list(args)
    def __iter__(self):
        return iter(self.data)

i = IterOnly(1, 2, 3)
print(1 in i)
print(4 in i)
print(2 in i)
