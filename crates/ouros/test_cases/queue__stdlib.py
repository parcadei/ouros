import queue

# === module attributes ===
assert hasattr(queue, 'Queue'), 'queue.Queue should exist'
assert hasattr(queue, 'LifoQueue'), 'queue.LifoQueue should exist'
assert hasattr(queue, 'PriorityQueue'), 'queue.PriorityQueue should exist'
assert hasattr(queue, 'SimpleQueue'), 'queue.SimpleQueue should exist'
assert hasattr(queue, 'Empty'), 'queue.Empty should exist'
assert hasattr(queue, 'Full'), 'queue.Full should exist'

# === Queue FIFO behavior ===
q = queue.Queue()
assert q.qsize() == 0, 'Queue should start empty'
assert q.empty() is True, 'Queue.empty() should be true initially'
assert q.full() is False, 'Queue.full() should be false for maxsize=0'

q.put(10)
q.put(20)
assert q.qsize() == 2, 'Queue size should increase on put'
assert q.get() == 10, 'Queue.get() should return FIFO first item'
assert q.get_nowait() == 20, 'Queue.get_nowait() should return next FIFO item'
assert q.empty() is True, 'Queue should be empty after removing all items'

# task_done/join should be callable without deadlocking when bookkeeping is balanced
assert q.task_done() is None, 'Queue.task_done() should exist and return None'
assert q.task_done() is None, 'Queue.task_done() should be callable per retrieved item'
assert q.join() is None, 'Queue.join() should return when unfinished task count is zero'

# === Queue(maxsize) full behavior ===
q2 = queue.Queue(1)
q2.put('x')
assert q2.full() is True, 'Queue.full() should report true when maxsize reached'

try:
    q2.put_nowait('y')
    assert False, 'Queue.put_nowait on full queue should raise queue.Full'
except queue.Full:
    pass

# put() accepts block/timeout signature even though Ouros never blocks
try:
    q2.put('y', block=False, timeout=1)
    assert False, 'Queue.put on full queue should raise queue.Full in single-threaded mode'
except queue.Full:
    pass

try:
    q2.put('y', timeout=-1)
    assert False, 'Queue.put timeout<0 should raise ValueError'
except ValueError as exc:
    assert str(exc) == "'timeout' must be a non-negative number", 'timeout validation message should match'

# === Empty behavior ===
q3 = queue.Queue()
try:
    q3.get_nowait()
    assert False, 'Queue.get_nowait on empty queue should raise queue.Empty'
except queue.Empty:
    pass

try:
    q3.get(block=False, timeout=1)
    assert False, 'Queue.get on empty queue should raise queue.Empty'
except queue.Empty:
    pass

try:
    q3.get(timeout=-1)
    assert False, 'Queue.get timeout<0 should raise ValueError'
except ValueError as exc:
    assert str(exc) == "'timeout' must be a non-negative number", 'timeout validation message should match'

# === LifoQueue behavior ===
lifo = queue.LifoQueue()
lifo.put(1)
lifo.put(2)
lifo.put(3)
assert lifo.get() == 3, 'LifoQueue should pop latest item first'
assert lifo.get_nowait() == 2, 'LifoQueue get_nowait should be LIFO'
assert lifo.get() == 1, 'LifoQueue should eventually return oldest item'

# === PriorityQueue behavior ===
pq = queue.PriorityQueue()
pq.put((2, 'b'))
pq.put((1, 'a'))
pq.put((3, 'c'))
assert pq.get() == (1, 'a'), 'PriorityQueue should return smallest item first'
assert pq.get_nowait() == (2, 'b'), 'PriorityQueue should keep min-heap order'
assert pq.get() == (3, 'c'), 'PriorityQueue should return remaining item'

# === SimpleQueue behavior ===
sq = queue.SimpleQueue()
assert sq.empty() is True, 'SimpleQueue should start empty'
assert sq.qsize() == 0, 'SimpleQueue initial size should be 0'

# SimpleQueue.put accepts block/timeout for compatibility but stays unbounded
assert sq.put('a') is None, 'SimpleQueue.put should return None'
assert sq.put('b', block=False, timeout=123) is None, 'SimpleQueue.put should accept block/timeout kwargs'
assert sq.qsize() == 2, 'SimpleQueue size should track puts'
assert sq.get() == 'a', 'SimpleQueue should be FIFO'
assert sq.get_nowait() == 'b', 'SimpleQueue.get_nowait should return next FIFO item'
assert sq.empty() is True, 'SimpleQueue should be empty after gets'

try:
    sq.get_nowait()
    assert False, 'SimpleQueue.get_nowait on empty queue should raise queue.Empty'
except queue.Empty:
    pass

# === from import ===
from queue import LifoQueue, PriorityQueue, Queue, SimpleQueue

assert Queue is not None, 'from queue import Queue should work'
assert LifoQueue is not None, 'from queue import LifoQueue should work'
assert PriorityQueue is not None, 'from queue import PriorityQueue should work'
assert SimpleQueue is not None, 'from queue import SimpleQueue should work'
