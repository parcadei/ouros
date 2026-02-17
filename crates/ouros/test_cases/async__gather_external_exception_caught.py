# call-external
# run-async
# External async errors inside gather sub-tasks should be catchable by each sub-task.
import asyncio


async def fetch(url):
    if url.startswith('fail'):
        return await async_raise('ValueError', f'err:{url}')  # pyright: ignore
    return await async_call(f'ok:{url}')  # pyright: ignore


async def safe_fetch(url):
    try:
        return await fetch(url)
    except ValueError as e:
        return str(e)


results = await asyncio.gather(  # pyright: ignore
    safe_fetch('good1'),
    safe_fetch('fail1'),
    safe_fetch('good2'),
    safe_fetch('fail2'),
)
assert results == ['ok:good1', 'err:fail1', 'ok:good2', 'err:fail2'], (
    'gather should preserve order and let sub-task try/except catch async external errors'
)
