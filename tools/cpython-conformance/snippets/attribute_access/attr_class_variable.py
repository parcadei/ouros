# conformance: attribute_access
# description: Instance attr shadows class attr; class attr accessible via type
# tags: class_variable,instance,shadow
# ---
class C:
    x = "class"

c = C()
print(c.x)
c.x = "instance"
print(c.x)
print(C.x)
