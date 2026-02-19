# conformance: container_protocol
# description: 'not in' negates __contains__
# tags: not_in,contains,negation
# ---
class HasItems:
    def __contains__(self, item):
        return item in [1, 2, 3]

h = HasItems()
print(1 not in h)
print(4 not in h)
