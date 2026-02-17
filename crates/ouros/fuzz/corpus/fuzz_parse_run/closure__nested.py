# === Nested closure - variable from 2 levels up ===
def outer(prefix):
    def middle(suffix):
        def inner(cls):
            cls.name = prefix + suffix
            return cls

        return inner

    return middle


@outer('pre_')('_suf')
class Named:
    pass


assert Named.name == 'pre__suf', 'nested closure captures var from 2 levels up'


# === Single-level closure decorator ===
def make_decorator(prefix):
    def decorator(cls):
        cls.prefix = prefix
        return cls

    return decorator


@make_decorator('hello')
class Greeter:
    pass


assert Greeter.prefix == 'hello', 'single-level closure decorator'
