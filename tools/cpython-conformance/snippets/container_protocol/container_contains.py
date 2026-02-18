# conformance: container_protocol
# description: __contains__ for 'in' operator
# tags: contains,in,membership
# ---
class EvenOnly:
    def __contains__(self, item):
        return item % 2 == 0

e = EvenOnly()
print(2 in e)
print(3 in e)
print(4 in e)
print(5 in e)
