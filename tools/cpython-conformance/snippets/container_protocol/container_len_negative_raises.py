# conformance: container_protocol
# description: __len__ returning negative raises ValueError
# tags: len,negative,valueerror
# ---
class BadLen:
    def __len__(self):
        return -1

try:
    len(BadLen())
except ValueError as e:
    print("ValueError raised")
