class AppError(Exception):
    pass

class ValidationError(AppError):
    def __init__(self, field, message):
        self.field = field
        self.message = message
        super().__init__(f'{field}: {message}')

class NotFoundError(AppError):
    def __init__(self, resource, id_):
        self.resource = resource
        self.id_ = id_
        super().__init__(f'{resource} with id {id_} not found')

# Catch subclass via parent, check isinstance
try:
    raise ValidationError('email', 'invalid')
except AppError as e:
    assert isinstance(e, ValidationError), f'expected ValidationError, got {type(e).__name__}'
    assert isinstance(e, AppError), f'should also be AppError'
    assert isinstance(e, Exception), f'should also be Exception'
    assert e.field == 'email'
    assert e.message == 'invalid'
    assert type(e).__name__ == 'ValidationError'
    assert str(e) == 'email: invalid', f'unexpected str: {str(e)!r}'
    assert repr(e) == "ValidationError('email: invalid')", f'unexpected repr: {repr(e)!r}'

# Catch different subclass
try:
    raise NotFoundError('User', 42)
except AppError as e:
    assert isinstance(e, NotFoundError), f'expected NotFoundError, got {type(e).__name__}'
    assert not isinstance(e, ValidationError), f'should NOT be ValidationError'
    assert e.resource == 'User'
    assert e.id_ == 42
    assert type(e).__name__ == 'NotFoundError'
    assert str(e) == 'User with id 42 not found', f'unexpected str: {str(e)!r}'
    assert repr(e) == "NotFoundError('User with id 42 not found')", f'unexpected repr: {repr(e)!r}'

# Direct catch preserves type too
try:
    raise ValidationError('name', 'too short')
except ValidationError as e:
    assert isinstance(e, ValidationError)
    assert type(e).__name__ == 'ValidationError'
    assert str(e) == 'name: too short', f'unexpected str: {str(e)!r}'
    assert repr(e) == "ValidationError('name: too short')", f'unexpected repr: {repr(e)!r}'

print('ALL PASSED')
