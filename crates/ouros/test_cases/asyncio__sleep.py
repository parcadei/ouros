import asyncio

async def test_sleep():
    await asyncio.sleep(0)
    return 'done'

result = asyncio.run(test_sleep())
assert result == 'done', f'basic: {result}'

async def test_sleep_return():
    val = await asyncio.sleep(0)
    return val

result = asyncio.run(test_sleep_return())
assert result is None, f'return: {result}'

async def test_multiple():
    await asyncio.sleep(0)
    x = 1
    await asyncio.sleep(0)
    x += 1
    await asyncio.sleep(0)
    x += 1
    return x

result = asyncio.run(test_multiple())
assert result == 3, f'multiple: {result}'

print('ALL PASSED')
