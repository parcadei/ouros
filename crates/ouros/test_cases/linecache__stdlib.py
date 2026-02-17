import linecache


# === Module API ===
for name in ["getline", "getlines", "clearcache", "checkcache", "lazycache", "cache"]:
    assert hasattr(linecache, name), f"missing linecache.{name}"

assert isinstance(linecache.cache, dict), "linecache.cache must be a dict"


# === Missing files / sandbox fallback ===
linecache.clearcache()
assert linecache.cache == {}, "clearcache should empty cache"
assert linecache.getline("definitely_missing.py", 1) == "", "missing line should be empty"
assert linecache.getline("definitely_missing.py", 0) == "", "lineno <= 0 should be empty"
assert linecache.getlines("definitely_missing.py") == [], "missing file should return empty list"
assert linecache.checkcache() is None, "checkcache() should return None"
assert linecache.checkcache("definitely_missing.py") is None, "checkcache(filename) should return None"

lazy_result = linecache.lazycache("definitely_missing.py", {})
assert isinstance(lazy_result, bool), "lazycache should return bool"


# === Cache dictionary behavior ===
linecache.cache["virtual.py"] = (0, None, ["first\n", "second\n"], "virtual.py")
assert linecache.getline("virtual.py", 1) == "first\n", "getline should read cached first line"
assert linecache.getline("virtual.py", 2) == "second\n", "getline should read cached second line"
assert linecache.getline("virtual.py", 3) == "", "getline should return empty when out of range"
assert linecache.getlines("virtual.py") == ["first\n", "second\n"], "getlines should read cached list"


# === Keyword argument handling ===
assert linecache.getline(filename="virtual.py", lineno=2, module_globals=None) == "second\n"
assert linecache.getlines(filename="virtual.py", module_globals=None) == ["first\n", "second\n"]
assert linecache.checkcache(filename="virtual.py") is None
assert isinstance(linecache.lazycache(filename="virtual.py", module_globals={}), bool)


# === Clear cache ===
linecache.clearcache()
assert linecache.cache == {}, "clearcache should clear all cached entries"
assert linecache.getline("virtual.py", 1) == "", "cleared cache should no longer return virtual lines"
