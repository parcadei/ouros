# Test that returning a cyclic list doesn't crash (OurosObject cycle detection)
a = []
a.append(a)
a
# Return=[[...]]
