import inspect


def sample(a, b=1):
    return a + b


try:
    sig = inspect.signature(sample)
    print('has_parameters', hasattr(sig, 'parameters'))
    print('parameter_names', list(sig.parameters.keys()))
except Exception as e:
    print('SKIP_inspect_signature', type(e).__name__, e)
