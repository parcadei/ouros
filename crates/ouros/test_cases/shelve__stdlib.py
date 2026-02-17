import pickle
import shelve
import sys


if 'Ouros' not in sys.version:
    # CPython shelve imports a large dbm stack in this harness environment.
    # This case validates Ouros's in-memory shelve implementation only.
    pass
else:

    def exercise_shelf(filename):
        db = shelve.open(filename)

        db['alpha'] = 1
        db['beta'] = ['x', 2]
        db['gamma'] = {'k': True}

        assert db['alpha'] == 1, '__getitem__ returns stored int'
        assert db['beta'] == ['x', 2], '__getitem__ returns stored list'
        assert 'alpha' in db, '__contains__ true for existing key'
        assert 'missing' not in db, '__contains__ false for missing key'

        keys = db.keys()
        assert 'alpha' in keys and 'beta' in keys and 'gamma' in keys, 'keys includes stored keys'

        values = db.values()
        assert 1 in values, 'values includes int entry'
        assert ['x', 2] in values, 'values includes list entry'
        assert {'k': True} in values, 'values includes dict entry'

        items = dict(db.items())
        assert items['alpha'] == 1, 'items includes alpha value'
        assert items['beta'] == ['x', 2], 'items includes beta value'

        del db['alpha']
        assert 'alpha' not in db, '__delitem__ removes key'

        try:
            db['alpha']
            assert False, 'missing key read should raise KeyError'
        except KeyError:
            pass

        try:
            del db['alpha']
            assert False, 'missing key delete should raise KeyError'
        except KeyError:
            pass

        db.sync()
        db.close()

        try:
            db['beta']
            assert False, 'closed shelf operations should raise ValueError'
        except ValueError:
            pass

        try:
            db.sync()
            assert False, 'sync on closed shelf should raise ValueError'
        except ValueError:
            pass

        reopened = shelve.open(filename, flag='c', protocol=pickle.HIGHEST_PROTOCOL, writeback=False)
        assert reopened['beta'] == ['x', 2], 'reopened shelf preserves stored values by filename'
        reopened.close()


    # === module shape ===
    assert shelve.Shelf is not None, 'Shelf class should exist'

    # deterministic in-memory key used by Ouros shelve backend
    exercise_shelf('__ouros_shelve_stdlib__')
