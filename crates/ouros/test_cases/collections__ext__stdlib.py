import collections


# === Counter extended behavior ===
c = collections.Counter("abca")
assert c.__missing__("z") == 0, "Counter.__missing__ returns 0"
assert c.total() == 4, "Counter.total sums counts"

c.update("bb")
assert c["b"] == 3, "Counter.update adds iterable counts"

c.subtract({"a": 1, "d": 2})
assert c["a"] == 1 and c["d"] == -2, "Counter.subtract handles mapping and negatives"

mc = c.most_common(2)
assert mc[0][0] == "b" and mc[0][1] == 3, "Counter.most_common returns highest count first"
assert sorted(list(c.elements())) == ["a", "b", "b", "b", "c"], "Counter.elements omits non-positive counts"

c1 = collections.Counter({"a": 3, "b": 1})
c2 = collections.Counter({"a": 1, "b": 2, "c": 4})
assert c1 + c2 == collections.Counter({"a": 4, "b": 3, "c": 4}), "Counter +"
assert c1 - c2 == collections.Counter({"a": 2}), "Counter - keeps positives only"
assert (c1 & c2) == collections.Counter({"a": 1, "b": 1}), "Counter &"
assert (c1 | c2) == collections.Counter({"a": 3, "b": 2, "c": 4}), "Counter |"


# === OrderedDict equality and reversed ===
od1 = collections.OrderedDict([("a", 1), ("b", 2)])
od2 = collections.OrderedDict([("b", 2), ("a", 1)])
assert od1 != od2, "OrderedDict equality is order-sensitive"
assert list(od1.__reversed__()) == ["b", "a"], "OrderedDict.__reversed__ yields reverse key order"


# === defaultdict default_factory + __missing__ ===
dd = collections.defaultdict(list)
assert dd.default_factory is list, "default_factory is readable"
dd["items"].append(1)
assert dd["items"] == [1], "default_factory list is invoked on missing key"

dd.default_factory = int
assert dd.default_factory is int, "default_factory assignment works"
assert dd["count"] == 0, "updated default_factory is used"

setattr(dd, "default_factory", str)
assert dd.default_factory is str, "setattr works for default_factory"
assert dd["name"] == "", "dynamic setattr default_factory is used"

setattr(dd, "default_factory", None)
assert dd.default_factory is None, "default_factory can be set to None"
try:
    dd.__missing__("boom")
    raise AssertionError("defaultdict.__missing__ with None factory should raise KeyError")
except KeyError:
    pass


# === deque extended behavior ===
d = collections.deque([1, 2, 3], maxlen=3)
del d[0]
assert list(d) == [2, 3], "deque supports indexed deletion"

d += [4, 5]
assert list(d) == [3, 4, 5] and d.maxlen == 3, "deque __iadd__ accepts iterables and preserves maxlen"

plus = collections.deque([1, 2], maxlen=3) + collections.deque([3, 4])
assert list(plus) == [2, 3, 4] and plus.maxlen == 3, "deque __add__ keeps lhs maxlen semantics"

mul = collections.deque([1, 2], maxlen=3) * 2
assert list(mul) == [2, 1, 2] and mul.maxlen == 3, "deque __mul__ preserves maxlen trimming"

imul = collections.deque([9], maxlen=2)
imul *= 3
assert list(imul) == [9, 9] and imul.maxlen == 2, "deque __imul__ path works through repetition"

assert collections.deque([1, 2]) < collections.deque([1, 3]), "deque supports lexicographic <"
assert collections.deque([1, 2]) <= collections.deque([1, 2]), "deque supports <="
assert collections.deque([2]) > collections.deque([1, 9]), "deque supports >"
assert list(collections.deque([1, 2, 3]).__reversed__()) == [3, 2, 1], "deque.__reversed__"


# === ChainMap extras ===
cm = collections.ChainMap({"a": 1}, {"a": 2, "b": 3})
assert cm.get("a") == 1 and cm.get("missing", 7) == 7, "ChainMap.get"
assert len(cm) == 2 and bool(cm), "ChainMap len/bool"

popped = cm.pop("a")
assert popped == 1, "ChainMap.pop pops from first mapping"
assert cm["a"] == 2, "After pop from first map, lower-priority mapping is visible"

assert cm.pop("missing", 11) == 11, "ChainMap.pop default"

try:
    cm.pop("missing")
    raise AssertionError("ChainMap.pop without default should raise KeyError")
except KeyError:
    pass

child = cm.new_child({"x": 10}, y=20)
assert child["x"] == 10 and child["y"] == 20 and child["b"] == 3, "ChainMap.new_child"
assert isinstance(cm.maps, list) and len(cm.maps) >= 1, "ChainMap.maps attribute is present"
parents = cm.parents
assert parents["a"] == 2 and parents["b"] == 3, "ChainMap.parents drops first mapping"


# === namedtuple helpers ===
Point = collections.namedtuple("Point", ["x", "y"], defaults=[10])
p = Point(1)
assert Point._fields == ("x", "y"), "namedtuple factory _fields"
assert p._fields == ("x", "y"), "namedtuple instance _fields"
assert Point._field_defaults == {"y": 10}, "namedtuple factory _field_defaults"

m = Point._make([2, 3])
assert m == Point(2, 3), "namedtuple _make"

r = m._replace(y=9)
assert r == Point(2, 9), "namedtuple _replace"

ad = r._asdict()
assert type(ad).__name__ == "dict", "namedtuple _asdict returns dict on CPython 3.14"
assert ad["x"] == 2 and ad["y"] == 9, "namedtuple _asdict contents"
