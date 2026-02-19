# conformance: descriptor_protocol
# description: Function as non-data descriptor for method binding
# tags: method,binding,descriptor
# ---
class C:
    def greet(self):
        return "hello"

c = C()
# Function accessed through instance becomes a bound method
print(c.greet())
# Function accessed through class is unbound
print(C.greet(c))
