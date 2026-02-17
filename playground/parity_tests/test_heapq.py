import heapq

# === heappush ===
try:
    heap = []
    heapq.heappush(heap, 3)
    print('heappush_basic', heap)
    heapq.heappush(heap, 1)
    print('heappush_add_smaller', heap)
    heapq.heappush(heap, 5)
    print('heappush_add_larger', heap)
    heapq.heappush(heap, 2)
    print('heappush_add_middle', heap)
except Exception as e:
    print('SKIP_heappush', type(e).__name__, e)

# === heappop ===
try:
    heap = [1, 3, 5, 7, 9, 2, 4, 6, 8, 0]
    heapq.heapify(heap)
    result = heapq.heappop(heap)
    print('heappop_basic', result, heap)
    result = heapq.heappop(heap)
    print('heappop_second', result, heap)
except Exception as e:
    print('SKIP_heappop', type(e).__name__, e)

# === heapify ===
try:
    heap = [3, 1, 4, 1, 5, 9, 2, 6]
    heapq.heapify(heap)
    print('heapify_unsorted', heap)
    heap = []
    heapq.heapify(heap)
    print('heapify_empty', heap)
    heap = [1]
    heapq.heapify(heap)
    print('heapify_single', heap)
    heap = [1, 2]
    heapq.heapify(heap)
    print('heapify_two_sorted', heap)
    heap = [2, 1]
    heapq.heapify(heap)
    print('heapify_two_unsorted', heap)
    heap = [5, 5, 5, 5]
    heapq.heapify(heap)
    print('heapify_all_same', heap)
except Exception as e:
    print('SKIP_heapify', type(e).__name__, e)

# === heappushpop ===
try:
    heap = [1, 3, 5, 7, 9]
    result = heapq.heappushpop(heap, 4)
    print('heappushpop_middle', result, heap)
    heap = [1, 3, 5, 7, 9]
    result = heapq.heappushpop(heap, 0)
    print('heappushpop_smaller', result, heap)
    heap = [1, 3, 5, 7, 9]
    result = heapq.heappushpop(heap, 10)
    print('heappushpop_larger', result, heap)
    heap = []
    result = heapq.heappushpop(heap, 5)
    print('heappushpop_empty', result, heap)
except Exception as e:
    print('SKIP_heappushpop', type(e).__name__, e)

# === heapreplace ===
try:
    heap = [1, 3, 5, 7, 9]
    result = heapq.heapreplace(heap, 4)
    print('heapreplace_basic', result, heap)
    heap = [1, 3, 5, 7, 9]
    result = heapq.heapreplace(heap, 0)
    print('heapreplace_smaller', result, heap)
    heap = [1, 3, 5, 7, 9]
    result = heapq.heapreplace(heap, 10)
    print('heapreplace_larger', result, heap)
    heap = [1]
    result = heapq.heapreplace(heap, 5)
    print('heapreplace_single', result, heap)
except Exception as e:
    print('SKIP_heapreplace', type(e).__name__, e)

# === heappush_max ===
try:
    heap = []
    heapq.heappush_max(heap, 3)
    print('heappush_max_basic', heap)
    heapq.heappush_max(heap, 5)
    print('heappush_max_add_larger', heap)
    heapq.heappush_max(heap, 1)
    print('heappush_max_add_smaller', heap)
except Exception as e:
    print('SKIP_heappush_max', type(e).__name__, e)

# === heappop_max ===
try:
    heap = [9, 7, 5, 3, 1, 8, 6, 4, 2, 0]
    heapq.heapify_max(heap)
    result = heapq.heappop_max(heap)
    print('heappop_max_basic', result, heap)
    result = heapq.heappop_max(heap)
    print('heappop_max_second', result, heap)
except Exception as e:
    print('SKIP_heappop_max', type(e).__name__, e)

# === heapify_max ===
try:
    heap = [3, 1, 4, 1, 5, 9, 2, 6]
    heapq.heapify_max(heap)
    print('heapify_max_unsorted', heap)
    heap = []
    heapq.heapify_max(heap)
    print('heapify_max_empty', heap)
    heap = [1]
    heapq.heapify_max(heap)
    print('heapify_max_single', heap)
    heap = [5, 5, 5, 5]
    heapq.heapify_max(heap)
    print('heapify_max_all_same', heap)
except Exception as e:
    print('SKIP_heapify_max', type(e).__name__, e)

# === heappushpop_max ===
try:
    heap = [9, 7, 5, 3, 1]
    result = heapq.heappushpop_max(heap, 6)
    print('heappushpop_max_middle', result, heap)
    heap = [9, 7, 5, 3, 1]
    result = heapq.heappushpop_max(heap, 10)
    print('heappushpop_max_larger', result, heap)
    heap = [9, 7, 5, 3, 1]
    result = heapq.heappushpop_max(heap, 0)
    print('heappushpop_max_smaller', result, heap)
except Exception as e:
    print('SKIP_heappushpop_max', type(e).__name__, e)

# === heapreplace_max ===
try:
    heap = [9, 7, 5, 3, 1]
    result = heapq.heapreplace_max(heap, 6)
    print('heapreplace_max_basic', result, heap)
    heap = [9, 7, 5, 3, 1]
    result = heapq.heapreplace_max(heap, 10)
    print('heapreplace_max_larger', result, heap)
    heap = [9, 7, 5, 3, 1]
    result = heapq.heapreplace_max(heap, 0)
    print('heapreplace_max_smaller', result, heap)
except Exception as e:
    print('SKIP_heapreplace_max', type(e).__name__, e)

# === merge ===
try:
    result = list(heapq.merge([1, 3, 5], [2, 4, 6]))
    print('merge_two_lists', result)
    result = list(heapq.merge([1, 2, 3], [4, 5, 6], [7, 8, 9]))
    print('merge_three_lists', result)
    result = list(heapq.merge([], [1, 2, 3]))
    print('merge_one_empty', result)
    result = list(heapq.merge([], []))
    print('merge_both_empty', result)
    result = list(heapq.merge([1, 3, 5], [2, 4, 6], reverse=True))
    print('merge_reverse', result)
    result = list(heapq.merge(['a', 'c', 'e'], ['b', 'd', 'f']))
    print('merge_strings', result)
    result = list(heapq.merge([3, 2, 1], [6, 5, 4], reverse=True))
    print('merge_reverse_descending', result)
    result = list(heapq.merge([1, 2], [1, 2]))
    print('merge_duplicates', result)
except Exception as e:
    print('SKIP_merge', type(e).__name__, e)

# === merge with key ===
try:
    result = list(heapq.merge(['aaa', 'b'], ['aa', 'bb'], key=len))
    print('merge_key_len', result)
    result = list(heapq.merge(['A', 'C'], ['b', 'd'], key=str.lower))
    print('merge_key_lower', result)
except Exception as e:
    print('SKIP_merge with key', type(e).__name__, e)

# === nlargest ===
try:
    result = heapq.nlargest(3, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nlargest_basic', result)
    result = heapq.nlargest(1, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nlargest_one', result)
    result = heapq.nlargest(10, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nlargest_more_than_len', result)
    result = heapq.nlargest(0, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nlargest_zero', result)
    result = heapq.nlargest(3, [3, 1, 4, 1, 5, 9, 2, 6], key=lambda x: -x)
    print('nlargest_key', result)
    result = heapq.nlargest(3, ['aaa', 'bb', 'c', 'dddd'], key=len)
    print('nlargest_key_len', result)
    result = heapq.nlargest(3, [5, 5, 5, 1, 2, 5, 3])
    print('nlargest_duplicates', result)
except Exception as e:
    print('SKIP_nlargest', type(e).__name__, e)

# === nsmallest ===
try:
    result = heapq.nsmallest(3, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nsmallest_basic', result)
    result = heapq.nsmallest(1, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nsmallest_one', result)
    result = heapq.nsmallest(10, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nsmallest_more_than_len', result)
    result = heapq.nsmallest(0, [3, 1, 4, 1, 5, 9, 2, 6])
    print('nsmallest_zero', result)
    result = heapq.nsmallest(3, [3, 1, 4, 1, 5, 9, 2, 6], key=lambda x: -x)
    print('nsmallest_key', result)
    result = heapq.nsmallest(3, ['aaa', 'bb', 'c', 'dddd'], key=len)
    print('nsmallest_key_len', result)
    result = heapq.nsmallest(3, [5, 5, 5, 1, 2, 5, 3])
    print('nsmallest_duplicates', result)
except Exception as e:
    print('SKIP_nsmallest', type(e).__name__, e)

# === heap with tuples ===
try:
    heap = []
    heapq.heappush(heap, (2, 'task2'))
    heapq.heappush(heap, (1, 'task1'))
    heapq.heappush(heap, (3, 'task3'))
    print('heappush_tuples', heap)
    result = heapq.heappop(heap)
    print('heappop_tuples', result, heap)
except Exception as e:
    print('SKIP_heap with tuples', type(e).__name__, e)

# === complex heap operations ===
try:
    heap = []
    for x in [3, 1, 4, 1, 5, 9, 2, 6]:
        heapq.heappush(heap, x)
    sorted_result = [heapq.heappop(heap) for _ in range(len(heap))]
    print('heapsort', sorted_result)
except Exception as e:
    print('SKIP_complex heap operations', type(e).__name__, e)

# === max-heap complex operations ===
try:
    heap = []
    for x in [3, 1, 4, 1, 5, 9, 2, 6]:
        heapq.heappush_max(heap, x)
    sorted_result = [heapq.heappop_max(heap) for _ in range(len(heap))]
    print('max_heapsort_descending', sorted_result)
except Exception as e:
    print('SKIP_max-heap complex operations', type(e).__name__, e)

# === merge generators ===
try:
    def gen1():
        yield 1
        yield 3
        yield 5

    def gen2():
        yield 2
        yield 4
        yield 6

    result = list(heapq.merge(gen1(), gen2()))
    print('merge_generators', result)
except Exception as e:
    print('SKIP_merge generators', type(e).__name__, e)
