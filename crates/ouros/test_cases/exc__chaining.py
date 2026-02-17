# === raise ... from ... sets __cause__ and suppresses context ===
try:
    raise RuntimeError('outer') from ValueError('inner')
except RuntimeError as e:
    assert isinstance(e.__cause__, ValueError), '__cause__ should be ValueError'
    assert str(e.__cause__) == 'inner', '__cause__ message'
    assert e.__suppress_context__ is True, '__suppress_context__ set by explicit cause'

# === raise ... from None suppresses context with no cause ===
try:
    raise RuntimeError('clean') from None
except RuntimeError as e:
    assert e.__cause__ is None, '__cause__ is None for from None'
    assert e.__suppress_context__ is True, '__suppress_context__ is True for from None'

# === implicit chaining sets __context__ ===
try:
    try:
        raise ValueError('first')
    except:
        raise RuntimeError('second')
except RuntimeError as e:
    assert isinstance(e.__context__, ValueError), '__context__ should capture previous exception'
    assert str(e.__context__) == 'first', '__context__ message'
    assert e.__cause__ is None, '__cause__ remains None for implicit chaining'
    assert e.__suppress_context__ is False, '__suppress_context__ is False for implicit chaining'

# === default values on plain exceptions ===
try:
    raise RuntimeError('plain')
except RuntimeError as e:
    assert e.__cause__ is None, 'plain __cause__ defaults to None'
    assert e.__context__ is None, 'plain __context__ defaults to None'
    assert e.__suppress_context__ is False, 'plain __suppress_context__ defaults to False'

# === direct attribute access works ===
try:
    raise ValueError('attrs')
except ValueError as e:
    _ = e.__cause__
    _ = e.__context__
    _ = e.__suppress_context__
