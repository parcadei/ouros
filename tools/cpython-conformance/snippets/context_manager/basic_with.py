# conformance: context_manager
# description: Basic with-statement dispatches __enter__ then __exit__
# tags: enter,exit,with,context_manager
# ---
class CM:
    def __enter__(self):
        print("entering")
        return "resource"
    def __exit__(self, exc_type, exc_val, exc_tb):
        print("exiting")
        return False

with CM() as val:
    print(f"inside: {val}")

# Verify __exit__ is called even without 'as'
class CM2:
    def __enter__(self):
        print("enter2")
        return self
    def __exit__(self, *args):
        print("exit2")

with CM2():
    print("body2")
