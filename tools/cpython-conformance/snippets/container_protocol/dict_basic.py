# conformance: container_protocol
# description: Basic dict operations
# tags: dict,container,getitem,setitem,len
# ---
d = {"a": 1, "b": 2, "c": 3}
print(d["a"])
print(d["c"])
print(len(d))
d["d"] = 4
print(d["d"])
print("b" in d)
print("z" in d)
del d["a"]
print(len(d))
print(sorted(d.keys()))
print(sorted(d.values()))
