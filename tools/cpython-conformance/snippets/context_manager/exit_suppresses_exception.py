# conformance: context_manager
# description: __exit__ returning True suppresses the exception
# tags: exit,suppress,exception,context_manager
# ---
class Suppressor:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is not None:
            print(f"suppressing {exc_type.__name__}: {exc_val}")
        return True  # Suppress the exception

# Exception inside with block is suppressed
with Suppressor():
    raise ValueError("should be suppressed")

print("execution continues after suppressed exception")

# Non-suppressing: returning False / None
class NonSuppressor:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        print(f"not suppressing {exc_type.__name__}")
        return False  # Do NOT suppress

try:
    with NonSuppressor():
        raise ValueError("should propagate")
except ValueError as e:
    print(f"caught: {e}")

# Selective suppression
class SelectiveSuppressor:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        return exc_type is KeyError  # Only suppress KeyError

with SelectiveSuppressor():
    raise KeyError("suppressed")
print("after suppressed KeyError")

try:
    with SelectiveSuppressor():
        raise ValueError("not suppressed")
except ValueError:
    print("ValueError propagated correctly")
