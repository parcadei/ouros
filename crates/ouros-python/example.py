"""Example usage of the Ouros Python bindings."""

import ouros

# Basic execution - simple expression
m = ouros.Ouros('1 + 2 * 3')
print(f'Basic: {m.run()!r}')  # 7

# Using input variables
m = ouros.Ouros('x + y', inputs=['x', 'y'])
print(f'Inputs: {m.run(inputs={"x": 10, "y": 20})}')  # 30

# Reusing the same parsed code with different values
print(f'Reuse: {m.run(inputs={"x": 100, "y": 200})}')  # 300

# With resource limits
limits = ouros.ResourceLimits(max_duration_secs=5.0, max_memory=1024 * 1024)
m = ouros.Ouros('x * y * z', inputs=['x', 'y', 'z'])
print(f'With limits: {m.run(inputs={"x": 2, "y": 3, "z": 4}, limits=limits)}')  # 24

# External function callbacks
m = ouros.Ouros('fetch("https://example.com")', external_functions=['fetch'])


def fetch(url: str) -> str:
    return f'Fetched: {url}'


print(f'External: {m.run(external_functions={"fetch": fetch})}')

# Print output is forwarded to Python stdout
m = ouros.Ouros('print("Hello from Ouros!")')
m.run()

# Exception handling
m = ouros.Ouros('1 / 0')
try:
    m.run()
except ZeroDivisionError as e:
    print(f'Caught: {type(e).__name__}')
