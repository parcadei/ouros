# Test that returning a cyclic dict doesn't crash (OurosObject cycle detection)
d = {}
d['self'] = d
d
# Return={'self': {...}}
