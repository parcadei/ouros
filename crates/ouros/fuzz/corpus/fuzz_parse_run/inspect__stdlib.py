import inspect

# === signature ===
def sample(a, b=1):
    return a + b


sig = inspect.signature(sample)
params = sig.parameters
assert list(params.keys()) == ['a', 'b'], 'signature_parameter_names'
