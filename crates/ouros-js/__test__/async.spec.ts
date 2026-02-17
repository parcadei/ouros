import test from 'ava'

import { Sandbox, OurosRuntimeError, runSandboxAsync } from '../wrapper'

// =============================================================================
// Basic async external function tests
// =============================================================================

test('runSandboxAsync with sync external function', async (t) => {
  const m = new Sandbox('get_value()', { externalFunctions: ['get_value'] })

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      get_value: () => 42,
    },
  })

  t.is(result, 42)
})

test('runSandboxAsync with async external function', async (t) => {
  const m = new Sandbox('fetch_data()', { externalFunctions: ['fetch_data'] })

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      fetch_data: async () => {
        // Simulate async operation
        await new Promise((resolve) => setTimeout(resolve, 10))
        return 'async result'
      },
    },
  })

  t.is(result, 'async result')
})

test('runSandboxAsync with multiple async calls', async (t) => {
  const m = new Sandbox(
    `
a = fetch_a()
b = fetch_b()
a + b
`,
    { externalFunctions: ['fetch_a', 'fetch_b'] },
  )

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      fetch_a: async () => {
        await new Promise((resolve) => setTimeout(resolve, 5))
        return 10
      },
      fetch_b: async () => {
        await new Promise((resolve) => setTimeout(resolve, 5))
        return 20
      },
    },
  })

  t.is(result, 30)
})

test('runSandboxAsync with inputs', async (t) => {
  const m = new Sandbox('multiply(x)', { inputs: ['x'], externalFunctions: ['multiply'] })

  const result = await runSandboxAsync(m, {
    inputs: { x: 5 },
    externalFunctions: {
      multiply: async (n: number) => n * 2,
    },
  })

  t.is(result, 10)
})

test('runSandboxAsync with args and kwargs', async (t) => {
  const m = new Sandbox('process(1, 2, name="test")', { externalFunctions: ['process'] })

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      process: async (a: number, b: number, kwargs: { name: string }) => {
        return `${kwargs.name}: ${a + b}`
      },
    },
  })

  t.is(result, 'test: 3')
})

// =============================================================================
// Error handling tests
// =============================================================================

test('runSandboxAsync sync function throws exception', async (t) => {
  const m = new Sandbox('fail_sync()', { externalFunctions: ['fail_sync'] })

  class ValueError extends Error {
    override name = 'ValueError'
  }

  const error = await t.throwsAsync(
    runSandboxAsync(m, {
      externalFunctions: {
        fail_sync: () => {
          throw new ValueError('sync error')
        },
      },
    }),
  )

  t.true(error instanceof OurosRuntimeError)
})

test('runSandboxAsync async function throws exception', async (t) => {
  const m = new Sandbox('fail_async()', { externalFunctions: ['fail_async'] })

  class ValueError extends Error {
    override name = 'ValueError'
  }

  const error = await t.throwsAsync(
    runSandboxAsync(m, {
      externalFunctions: {
        fail_async: async () => {
          await new Promise((resolve) => setTimeout(resolve, 5))
          throw new ValueError('async error')
        },
      },
    }),
  )

  t.true(error instanceof OurosRuntimeError)
})

test('runSandboxAsync exception caught in try/except', async (t) => {
  const m = new Sandbox(
    `
try:
    might_fail()
except ValueError:
    result = 'caught'
result
`,
    { externalFunctions: ['might_fail'] },
  )

  class ValueError extends Error {
    override name = 'ValueError'
  }

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      might_fail: async () => {
        throw new ValueError('expected error')
      },
    },
  })

  t.is(result, 'caught')
})

test('runSandboxAsync missing external function', async (t) => {
  const m = new Sandbox('missing_func()', { externalFunctions: ['missing_func'] })

  const error = await t.throwsAsync(runSandboxAsync(m, { externalFunctions: {} }))

  t.true(error instanceof OurosRuntimeError)
})

test('runSandboxAsync missing function caught in try/except', async (t) => {
  const m = new Sandbox(
    `
try:
    missing()
except KeyError:
    result = 'key error caught'
result
`,
    { externalFunctions: ['missing'] },
  )

  const result = await runSandboxAsync(m, { externalFunctions: {} })

  t.is(result, 'key error caught')
})

// =============================================================================
// Complex type tests
// =============================================================================

test('runSandboxAsync returns complex types', async (t) => {
  const m = new Sandbox('get_data()', { externalFunctions: ['get_data'] })

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      get_data: async () => {
        return [1, 2, { key: 'value' }]
      },
    },
  })

  t.true(Array.isArray(result))
  t.is(result[0], 1)
  t.is(result[1], 2)
  t.true(result[2] instanceof Map)
  t.is(result[2].get('key'), 'value')
})

test('runSandboxAsync with list input', async (t) => {
  const m = new Sandbox('sum_list(items)', { inputs: ['items'], externalFunctions: ['sum_list'] })

  const result = await runSandboxAsync(m, {
    inputs: { items: [1, 2, 3, 4, 5] },
    externalFunctions: {
      sum_list: async (items: number[]) => {
        return items.reduce((a, b) => a + b, 0)
      },
    },
  })

  t.is(result, 15)
})

// =============================================================================
// Mixed sync/async tests
// =============================================================================

test('runSandboxAsync mixed sync and async functions', async (t) => {
  const m = new Sandbox(
    `
sync_result = sync_func()
async_result = async_func()
sync_result + async_result
`,
    { externalFunctions: ['sync_func', 'async_func'] },
  )

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      sync_func: () => 100,
      async_func: async () => {
        await new Promise((resolve) => setTimeout(resolve, 5))
        return 200
      },
    },
  })

  t.is(result, 300)
})

test('runSandboxAsync chained async calls', async (t) => {
  const m = new Sandbox(
    `
first = get_first()
second = process(first)
finalize(second)
`,
    { externalFunctions: ['get_first', 'process', 'finalize'] },
  )

  const result = await runSandboxAsync(m, {
    externalFunctions: {
      get_first: async () => 'hello',
      process: async (s: string) => s.toUpperCase(),
      finalize: async (s: string) => `${s}!`,
    },
  })

  t.is(result, 'HELLO!')
})

// =============================================================================
// No external functions tests
// =============================================================================

test('runSandboxAsync without external functions', async (t) => {
  const m = new Sandbox('1 + 2')

  const result = await runSandboxAsync(m, {})

  t.is(result, 3)
})

test('runSandboxAsync pure computation', async (t) => {
  const m = new Sandbox(
    `
def factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)
factorial(5)
`,
  )

  const result = await runSandboxAsync(m)

  t.is(result, 120)
})
