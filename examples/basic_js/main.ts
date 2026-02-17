import { Sandbox, Snapshot, Complete, runSandboxAsync } from 'ouros'

// --- Basic execution ---

const add = new Sandbox('x + y', { inputs: ['x', 'y'] })
const result = add.run({ inputs: { x: 10, y: 20 } })
console.log('Basic:', result) // 30

// --- External functions ---

const code = `
data = fetch(url)
len(data)
`

const m = new Sandbox(code, {
  inputs: ['url'],
  externalFunctions: ['fetch'],
})

let progress = m.start({ inputs: { url: 'https://example.com' } })

// Execution paused at fetch() — provide the return value
if (progress instanceof Snapshot) {
  console.log('External call:', progress.functionName) // 'fetch'
  console.log('Args:', progress.args) // ['https://example.com']

  const resumed = progress.resume({ returnValue: 'hello world' })
  if (resumed instanceof Complete) {
    console.log('Result:', resumed.output) // 11
  }
}

// --- Async external functions ---

const agent = new Sandbox(
  `
async def run():
    result = await llm(prompt)
    return result.upper()

await run()
`,
  {
    inputs: ['prompt'],
    externalFunctions: ['llm'],
  },
)

async function llm(prompt: string): Promise<string> {
  return `response to: ${prompt}`
}

const output = await runSandboxAsync(agent, {
  inputs: { prompt: 'hello' },
  externalFunctions: { llm },
})
console.log('Async:', output) // 'RESPONSE TO: HELLO'

// --- Serialization ---

const fib = new Sandbox(
  `
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
fib(x)
`,
  { inputs: ['x'] },
)

// Serialize to bytes
const bytes = fib.dump()
console.log('Serialized size:', bytes.length, 'bytes')

// Restore and run
const restored = Sandbox.load(bytes)
const fibResult = restored.run({ inputs: { x: 10 } })
console.log('Fibonacci(10):', fibResult) // 55
