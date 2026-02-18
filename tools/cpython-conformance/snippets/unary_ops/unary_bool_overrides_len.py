# conformance: unary_ops
# description: __bool__ takes priority over __len__
# tags: bool,len,priority
# ---
class BoolOverLen:
    def __bool__(self):
        return False
    def __len__(self):
        return 100

print(bool(BoolOverLen()))
