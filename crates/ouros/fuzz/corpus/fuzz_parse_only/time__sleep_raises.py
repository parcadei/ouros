# xfail=cpython
# time.sleep() raises RuntimeError in sandboxed environment
import time
time.sleep(0.1)
"""
TRACEBACK:
Traceback (most recent call last):
  File "time__sleep_raises.py", line 4, in <module>
    time.sleep(0.1)
    ~~~~~~~~~~~~~~~
RuntimeError: time.sleep() is not supported in sandboxed environment
"""
