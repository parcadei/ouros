# conformance: context_manager
# description: __exit__ receives (exc_type, exc_val, exc_tb) on exception, (None, None, None) on success
# tags: exit,exception,args,traceback,context_manager
# ---
class Inspector:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            print("clean exit: all None")
            print(f"  type={exc_type}, val={exc_val}, tb={exc_tb}")
        else:
            print(f"exception exit: type={exc_type.__name__}, val={exc_val}")
            print(f"  tb is None: {exc_tb is None}")
        return True  # Suppress to continue testing

# No exception: all three args are None
with Inspector():
    pass

# With exception: args are populated
with Inspector():
    raise RuntimeError("test error")

# With different exception types
with Inspector():
    raise KeyError("key missing")

# __exit__ is called even if __enter__ value is not used
class Tracker:
    def __enter__(self):
        return 42
    def __exit__(self, *args):
        print(f"exit called with {len(args)} args")
        return False

with Tracker():
    pass
