import io
import pickle
import sys


payload = {
    'none': None,
    'bool': True,
    'int': 42,
    'bigint': 2**80,
    'float': 3.5,
    'str': 'hello',
    'bytes': b'xyz',
    'list': [1, 2, 3],
    'tuple': ('a', 5),
    'dict': {'a': 1, 2: 'b'},
    'set': {1, 2, 3},
    'frozenset': frozenset({4, 5}),
}

# === constants ===
assert isinstance(pickle.HIGHEST_PROTOCOL, int), 'HIGHEST_PROTOCOL should be int'
assert isinstance(pickle.DEFAULT_PROTOCOL, int), 'DEFAULT_PROTOCOL should be int'
assert pickle.HIGHEST_PROTOCOL >= pickle.DEFAULT_PROTOCOL, 'HIGHEST_PROTOCOL >= DEFAULT_PROTOCOL'

# === exception classes ===
assert issubclass(pickle.PicklingError, Exception), 'PicklingError should derive from Exception'
assert issubclass(pickle.UnpicklingError, Exception), 'UnpicklingError should derive from Exception'

# === dumps / loads ===
blob = pickle.dumps(payload)
assert isinstance(blob, bytes), 'dumps returns bytes'
loaded = pickle.loads(blob)
assert loaded == payload, 'loads round-trips supported payload'

# explicit protocol and negative aliases to highest
blob_high = pickle.dumps(payload, protocol=pickle.HIGHEST_PROTOCOL)
assert pickle.loads(blob_high) == payload, 'dumps with highest protocol round-trips'
blob_alias = pickle.dumps(payload, protocol=-1)
assert pickle.loads(blob_alias) == payload, 'dumps with protocol=-1 uses highest protocol'
blob_alias2 = pickle.dumps(payload, protocol=-2)
assert pickle.loads(blob_alias2) == payload, 'dumps with protocol<-1 uses highest protocol'

# protocol upper bound validation
try:
    pickle.dumps(payload, protocol=pickle.HIGHEST_PROTOCOL + 1)
    assert False, 'protocol above highest should raise ValueError'
except ValueError:
    pass

# === dump / load using file-like ===
buf = io.BytesIO()
result = pickle.dump(payload, buf)
assert result is None, 'dump returns None'
buf.seek(0)
reloaded = pickle.load(buf)
assert reloaded == payload, 'load reads from file-like read() output'

# === unsupported type ===
gen = (x for x in [1, 2, 3])
try:
    pickle.dumps(gen)
    assert False, 'unsupported type should raise pickling error'
except pickle.PicklingError:
    pass
except TypeError:
    assert 'Ouros' not in sys.version, 'Ouros should not raise TypeError for unsupported pickling type'

# === invalid payload ===
try:
    pickle.loads(b'not-an-ouros-pickle')
    assert False, 'invalid bytes should raise UnpicklingError'
except pickle.UnpicklingError:
    pass
