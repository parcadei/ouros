# === super().__init__ reaches object for explicit object subclasses ===
class ObjectLeaf(object):
    def __init__(self):
        super().__init__()


leaf = ObjectLeaf()
assert isinstance(leaf, ObjectLeaf), 'super().__init__() should resolve object.__init__ for object subclasses'


# === cooperative super() chain ending at object ===
class ChainA:
    def __init__(self):
        self.chain = ['A']
        super().__init__()


class ChainB(ChainA):
    def __init__(self):
        super().__init__()
        self.chain.append('B')


chain = ChainB()
assert chain.chain == ['A', 'B'], 'cooperative super() chain should execute and terminate at object.__init__'


# === diamond super() chain ending at object ===
class DiamondBase:
    def __init__(self, log):
        log.append('base')
        super().__init__()


class DiamondLeft(DiamondBase):
    def __init__(self, log):
        log.append('left')
        super().__init__(log)


class DiamondRight(DiamondBase):
    def __init__(self, log):
        log.append('right')
        super().__init__(log)


class DiamondBottom(DiamondLeft, DiamondRight):
    def __init__(self, log):
        log.append('bottom')
        super().__init__(log)


mro_log = []
DiamondBottom(mro_log)
assert mro_log == ['bottom', 'left', 'right', 'base'], 'diamond super() chain should resolve all __init__ calls'


# === super().__init__ on builtin container subclasses ===
class MyList(list):
    def __init__(self, *args):
        super().__init__(*args)
        self.tag = 'list-ok'


class MyDict(dict):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.tag = 'dict-ok'


class MySet(set):
    def __init__(self, *args):
        super().__init__(*args)
        self.tag = 'set-ok'


my_list = MyList([1, 2])
my_dict = MyDict(a=1)
my_set = MySet([1, 2])

assert my_list.tag == 'list-ok', 'list subclass super().__init__ should execute without AttributeError'
assert my_dict.tag == 'dict-ok', 'dict subclass super().__init__ should execute without AttributeError'
assert my_set.tag == 'set-ok', 'set subclass super().__init__ should execute without AttributeError'


# === exception subclasses still resolve super().__init__ ===
class MyExc(Exception):
    def __init__(self, message):
        super().__init__(message)


try:
    raise MyExc('boom')
except MyExc as exc:
    assert str(exc) == 'boom', 'exception super().__init__ should preserve message'
