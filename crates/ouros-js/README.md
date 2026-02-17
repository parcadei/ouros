# ouros

JavaScript/TypeScript bindings for the Ouros sandboxed Python interpreter.

## Installation

```bash
npm install ouros
```

## Basic Usage

```ts
import { Sandbox } from 'ouros'

// Create interpreter and run code
const m = new Sandbox('1 + 2')
const result = m.run() // returns 3
```

## Input Variables

```ts
const m = new Sandbox('x + y', { inputs: ['x', 'y'] })
const result = m.run({ inputs: { x: 10, y: 20 } }) // returns 30
```

## External Functions

For synchronous external functions, pass them directly to `run()`:

```ts
const m = new Sandbox('add(2, 3)', { externalFunctions: ['add'] })

const result = m.run({
  externalFunctions: {
    add: (a: number, b: number) => a + b,
  },
}) // returns 5
```

For async external functions, use `runSandboxAsync()`:

```ts
import { Sandbox, runSandboxAsync } from 'ouros'

const m = new Sandbox('fetch_data(url)', {
  inputs: ['url'],
  externalFunctions: ['fetch_data'],
})

const result = await runSandboxAsync(m, {
  inputs: { url: 'https://example.com' },
  externalFunctions: {
    fetch_data: async (url: string) => {
      const response = await fetch(url)
      return response.text()
    },
  },
})
```

## Iterative Execution

For fine-grained control over external function calls, use `start()` and `resume()`:

```ts
const m = new Sandbox('a() + b()', { externalFunctions: ['a', 'b'] })

let progress = m.start()
while (progress instanceof Snapshot) {
  console.log(`Calling: ${progress.functionName}`)
  console.log(`Args: ${progress.args}`)
  // Provide the return value and resume
  progress = progress.resume({ returnValue: 10 })
}
// progress is now Complete
console.log(progress.output) // 20
```

## Error Handling

```ts
import { Sandbox, OurosSyntaxError, OurosRuntimeError, SandboxTypingError } from 'ouros'

try {
  const m = new Sandbox('1 / 0')
  m.run()
} catch (error) {
  if (error instanceof OurosSyntaxError) {
    console.log('Syntax error:', error.message)
  } else if (error instanceof OurosRuntimeError) {
    console.log('Runtime error:', error.message)
    console.log('Traceback:', error.traceback())
  } else if (error instanceof SandboxTypingError) {
    console.log('Type error:', error.displayDiagnostics())
  }
}
```

## Type Checking

```ts
const m = new Sandbox('"hello" + 1')
try {
  m.typeCheck()
} catch (error) {
  if (error instanceof SandboxTypingError) {
    console.log(error.displayDiagnostics('concise'))
  }
}

// Or enable during construction
const m2 = new Sandbox('1 + 1', { typeCheck: true })
```

## Resource Limits

```ts
const m = new Sandbox('1 + 1')
const result = m.run({
  limits: {
    maxAllocations: 10000,
    maxDurationSecs: 5,
    maxMemory: 1024 * 1024, // 1MB
    maxRecursionDepth: 100,
  },
})
```

## Serialization

```ts
// Save parsed code to avoid re-parsing
const m = new Sandbox('complex_code()')
const data = m.dump()

// Later, restore without re-parsing
const m2 = Sandbox.load(data)
const result = m2.run()

// Snapshots can also be serialized
const snapshot = m.start()
if (snapshot instanceof Snapshot) {
  const snapshotData = snapshot.dump()
  // Later, restore and resume
  const restored = Snapshot.load(snapshotData)
  const result = restored.resume({ returnValue: 42 })
}
```

## API Reference

### `Sandbox` Class

- `constructor(code: string, options?: SandboxOptions)` - Parse Python code
- `run(options?: RunOptions)` - Execute and return the result
- `start(options?: StartOptions)` - Start iterative execution
- `typeCheck(prefixCode?: string)` - Perform static type checking
- `dump()` - Serialize to binary format
- `Sandbox.load(data)` - Deserialize from binary format
- `scriptName` - The script name (default: `'main.py'`)
- `inputs` - Declared input variable names
- `externalFunctions` - Declared external function names

### `SandboxOptions`

- `scriptName?: string` - Name used in tracebacks (default: `'main.py'`)
- `inputs?: string[]` - Input variable names
- `externalFunctions?: string[]` - External function names
- `typeCheck?: boolean` - Enable type checking on construction
- `typeCheckPrefixCode?: string` - Code to prepend for type checking

### `RunOptions`

- `inputs?: object` - Input variable values
- `limits?: ResourceLimits` - Resource limits
- `externalFunctions?: object` - External function callbacks

### `ResourceLimits`

- `maxAllocations?: number` - Maximum heap allocations
- `maxDurationSecs?: number` - Maximum execution time in seconds
- `maxMemory?: number` - Maximum heap memory in bytes
- `gcInterval?: number` - Run GC every N allocations
- `maxRecursionDepth?: number` - Maximum call stack depth (default: 1000)

### `Snapshot` Class

Returned by `start()` when execution pauses at an external function call.

- `scriptName` - The script being executed
- `functionName` - The external function being called
- `args` - Positional arguments
- `kwargs` - Keyword arguments
- `resume(options: ResumeOptions)` - Resume with return value or exception
- `dump()` / `Snapshot.load(data)` - Serialization

### `Complete` Class

Returned by `start()` or `resume()` when execution completes.

- `output` - The final result value

### Error Classes

- `OurosError` - Base class for all Ouros errors
- `OurosSyntaxError` - Syntax/parsing errors
- `OurosRuntimeError` - Runtime exceptions (with `traceback()`)
- `SandboxTypingError` - Type checking errors (with `displayDiagnostics()`)
