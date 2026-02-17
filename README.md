<div align="center">
  <img src="logo.jpeg" alt="Ouros" width="600">
</div>
<div align="center">
  <h3>A sandboxed Python runtime for AI agents, written in Rust.</h3>
</div>

---

Ouros is more than an interpreter and more than a REPL. It's a **stateful, sandboxed Python runtime** designed for AI agents that need to execute code across multiple turns, fork execution paths, rewind mistakes, and call out to external services — all without any access to the host system.

Run LLM-generated code safely with startup times under 1 microsecond. No containers, no VMs, no network access, no filesystem — just fast, isolated Python execution with full control over what the code can do.

## Key Features

**Sandboxed execution** — No filesystem, network, subprocess, or environment access. The only way sandbox code communicates with the outside world is through external functions you explicitly provide.

**Persistent REPL sessions** — `SessionManager` keeps variables alive across multiple `execute()` calls. Fork sessions, rewind history, transfer variables between sessions, save/restore to disk.

**Snapshot & resume** — Execution pauses at external function calls and produces a serializable `Snapshot`. Store it in a database, send it over the network, resume in a different process.

**Type checking included** — Full Python type checking via [ty](https://docs.astral.sh/ty/) (from Astral/Ruff), bundled in a single binary. Catch errors before execution.

**72 stdlib modules** — `json`, `re`, `datetime`, `collections`, `dataclasses`, `math`, `itertools`, `decimal`, `pathlib`, `statistics`, `csv`, `hashlib`, `uuid`, `asyncio`, and many more.

**Resource limits** — Cap memory, allocations, stack depth, and execution time. Kill runaway code before it causes problems.

**Multi-language bindings** — Call from Python, JavaScript/TypeScript, or Rust.

## Usage

### Python

```bash
uv add ouros
```

```python
import ouros

# Basic execution
m = ouros.Sandbox('x + y', inputs=['x', 'y'])
result = m.run(inputs={'x': 10, 'y': 20})  # returns 30
```

#### External Functions

Sandbox code can call functions on the host — but only the ones you allow:

```python
import ouros

code = """
data = fetch(url)
len(data)
"""

m = ouros.Sandbox(code, inputs=['url'], external_functions=['fetch'])

# start() pauses when fetch() is called
result = m.start(inputs={'url': 'https://example.com'})

print(type(result))  # <class 'ouros.Snapshot'>
#> <class 'ouros.Snapshot'>
print(result.function_name)  # 'fetch'
#> fetch
print(result.args)  # ('https://example.com',)
#> ('https://example.com',)

# Perform the real fetch, then resume
result = result.resume(return_value='hello world')
print(result.output)  # 11
#> 11
```

#### Async External Functions

```python {test="skip - requires async runtime"}
import ouros

code = """
async def agent(prompt, messages):
    while True:
        output = await call_llm(prompt, messages)
        if isinstance(output, str):
            return output
        messages.extend(output)

await agent(prompt, [])
"""

m = ouros.Sandbox(
    code,
    inputs=['prompt'],
    external_functions=['call_llm'],
    type_check=True,
)


async def call_llm(prompt, messages):
    # Your LLM call here
    return f'Done after {len(messages)} messages'


output = await ouros.run_async(  # noqa: F704
    m,
    inputs={'prompt': 'testing'},
    external_functions={'call_llm': call_llm},
)
```

#### Persistent Sessions (REPL)

Variables survive across calls. Fork sessions. Rewind history. Transfer variables between sessions.

```python
from ouros import SessionManager

mgr = SessionManager()
session = mgr.create_session('analysis', external_functions=['llm_query'])

# Execute code — variables persist
session.execute('data = [1, 2, 3, 4, 5]')
session.execute('total = sum(data)')

# Inspect state
session.get_variable('total')  # {'json_value': 15, 'repr': '15'}
session.list_variables()  # [{'name': 'data', ...}, {'name': 'total', ...}]

# Fork a session for exploration
branch = session.fork('experiment')
branch.execute('data.append(100)')
branch.get_variable('data')  # [1, 2, 3, 4, 5, 100]
session.get_variable('data')  # [1, 2, 3, 4, 5] — original unchanged

# Rewind mistakes
session.execute('data = "oops"')
session.rewind(steps=1)
session.get_variable('data')  # [1, 2, 3, 4, 5] — restored

# Save and restore
mgr.set_storage_dir('/tmp/ouros-sessions')
session.save(name='checkpoint-1')
# Later, or in another process:
mgr.load_session('checkpoint-1')
```

#### Serialization

Both `Sandbox` and `Snapshot` can be serialized to bytes, stored, and restored later:

```python
import ouros

# Cache parsed code
m = ouros.Sandbox('x + 1', inputs=['x'])
data = m.dump()
m2 = ouros.Sandbox.load(data)
print(m2.run(inputs={'x': 41}))  # 42
#> 42

# Suspend execution mid-flight
m = ouros.Sandbox('fetch(url)', inputs=['url'], external_functions=['fetch'])
snapshot = m.start(inputs={'url': 'https://example.com'})
state = snapshot.dump()

# Resume later, even in a different process
snapshot2 = ouros.Snapshot.load(state)
result = snapshot2.resume(return_value='response data')
```

### JavaScript / TypeScript

```bash
npm install ouros
```

```ts
import { Sandbox, Snapshot, runSandboxAsync } from 'ouros'

// Basic
const m = new Sandbox('x + y', { inputs: ['x', 'y'] })
const result = m.run({ inputs: { x: 10, y: 20 } }) // 30

// Iterative execution with external functions
const m2 = new Sandbox('a() + b()', { externalFunctions: ['a', 'b'] })
let progress = m2.start()
while (progress instanceof Snapshot) {
  progress = progress.resume({ returnValue: 10 })
}
console.log(progress.output) // 20
```

### Rust

```rust
use ouros::{Runner, Object, NoLimitTracker, StdPrint};

let code = r#"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)
"#;

let runner = Runner::new(code.to_owned(), "fib.py", vec!["x".to_owned()], vec![]).unwrap();
let result = runner.run(vec![Object::Int(10)], NoLimitTracker, &mut StdPrint).unwrap();
assert_eq!(result, Object::Int(55));
```

### MCP Server

Ouros ships an MCP (Model Context Protocol) server that exposes the full `SessionManager` API as tools. Any coding agent or IDE that supports MCP can use Ouros as a sandboxed Python runtime.

```bash
cargo install ouros-mcp
```

The server speaks JSON-RPC over stdin/stdout with Content-Length framing (standard MCP transport).

#### Available Tools

| Category | Tools |
|----------|-------|
| **Execution** | `execute`, `resume`, `resume_as_pending`, `resume_futures` |
| **Variables** | `list_variables`, `get_variable`, `set_variable`, `delete_variable`, `eval_variable`, `transfer_variable`, `call_session` |
| **Sessions** | `create_session`, `destroy_session`, `list_sessions`, `fork_session`, `reset` |
| **Persistence** | `save_session`, `load_session`, `list_saved_sessions` |
| **History** | `rewind`, `history`, `set_history_depth` |
| **Heap introspection** | `heap_stats`, `snapshot_heap`, `diff_heap` |

All tools accept an optional `session_id` parameter. When omitted, the `"default"` session is used.

#### Adding to Claude Code

Add to your project's `.claude/settings.json`:

```json
{
  "mcpServers": {
    "ouros": {
      "command": "ouros-mcp",
      "args": ["--storage-dir", "/tmp/ouros-sessions"]
    }
  }
}
```

#### Adding to Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "ouros": {
      "command": "ouros-mcp",
      "args": ["--storage-dir", "/tmp/ouros-sessions"]
    }
  }
}
```

#### Adding to any MCP client

```json
{
  "command": "ouros-mcp",
  "args": ["--storage-dir", "/tmp/ouros-sessions"]
}
```

The `--storage-dir` flag is optional. It defaults to `$OUROS_STORAGE_DIR` if set, otherwise `~/.ouros/sessions/`. When configured, `save_session` and `load_session` tools persist session state to disk.

#### Example: Agent workflow over MCP

```text
Agent → execute(code: "data = [1, 2, 3]\nsum(data)")
     ← {status: "ok", result: 6, variables: [{name: "data", type: "list"}]}

Agent → execute(code: "fetch(url)", session_id: "default")
     ← {status: "external_call", call_id: 1, function: "fetch", args: ["https://..."]}

Agent → resume(call_id: 1, result: "response body")
     ← {status: "ok", result: "response body"}

Agent → fork_session(session_id: "default", new_session_id: "experiment")
     ← {status: "ok"}

Agent → execute(code: "data.append(100)", session_id: "experiment")
     ← {status: "ok"}

Agent → get_variable(name: "data", session_id: "default")
     ← {name: "data", value: [1, 2, 3]}  // original unchanged
```

## Use Cases

### AI Agent Code Execution

The primary use case. LLMs generate Python code, Ouros runs it safely:

- **Tool calling via code** — Instead of JSON tool schemas, let the model write Python that calls your functions
- **Data processing** — Let agents write analysis code that runs on your data without exposing your filesystem
- **Multi-step reasoning** — Persistent sessions let agents build up state across multiple code generations

### RLM (Recursive Language Model) Runtime

Ouros's `SessionManager` is a natural fit for the [RLM pattern](https://arxiv.org/abs/2512.24601) — a root LLM generates code that runs in a persistent REPL, calling `llm_query()` for sub-tasks. Variables persist in Ouros's heap (not in the LLM's context window), enabling processing of arbitrarily large inputs with a bounded model context.

See [`examples/rlm_orchestrator/`](examples/rlm_orchestrator/) for a working implementation.

### Sandboxed Computation

Any scenario where you need to run untrusted Python safely: user-submitted code, plugin systems, educational platforms, competitive programming judges.

## Examples

See [`examples/`](examples/) for complete, runnable examples:

| Example | Language | Description |
|---------|----------|-------------|
| [`basic_js/`](examples/basic_js/) | TypeScript | Basic execution, external functions, async, serialization |
| [`basic_rust/`](examples/basic_rust/) | Rust | Basic execution, fibonacci, external functions |
| [`expense_analysis/`](examples/expense_analysis/) | Python | Async external functions for team expense analysis |
| [`rlm_orchestrator/`](examples/rlm_orchestrator/) | Python | Recursive Language Model pattern with persistent REPL sessions |
| [`sql_playground/`](examples/sql_playground/) | Python | Cross-format data joining with SQL, JSON, and sentiment analysis |

## Stdlib Modules

72 modules implemented natively in Rust:

`abc`, `argparse`, `array`, `asyncio`, `atexit`, `base64`, `binascii`, `bisect`, `builtins`, `cmath`, `codecs`, `collections`, `collections.abc`, `concurrent`, `concurrent.futures`, `contextlib`, `copy`, `csv`, `dataclasses`, `datetime`, `decimal`, `difflib`, `enum`, `errno`, `fnmatch`, `fractions`, `functools`, `gc`, `hashlib`, `heapq`, `html`, `inspect`, `io`, `ipaddress`, `itertools`, `json`, `linecache`, `logging`, `math`, `numbers`, `operator`, `os`, `os.path`, `pathlib`, `pickle`, `pprint`, `queue`, `random`, `re`, `secrets`, `shelve`, `shlex`, `statistics`, `string`, `struct`, `sys`, `textwrap`, `threading`, `time`, `token`, `tokenize`, `tomllib`, `traceback`, `types`, `typing`, `typing_extensions`, `urllib`, `urllib.parse`, `uuid`, `warnings`, `weakref`, `zlib`

## What Ouros Cannot Do

- **Full CPython compatibility** — Ouros implements a substantial subset of Python, not all of it
- **Third-party libraries** — No pip, no numpy, no pandas. This is by design
- **Direct I/O** — No filesystem, network, or subprocess access from sandbox code. All external communication goes through external functions you control

## Performance

Benchmarks comparing Ouros vs CPython 3.14 (`make bench`):

| Benchmark | Ouros | CPython | Ratio |
|-----------|------|---------|-------|
| End-to-end (parse + run) | 1.2 µs | 8.3 µs | **6.9x faster** |
| Loop + modulo (1k iter) | 42 µs | 27 µs | 1.5x slower |
| List comprehension | 47 µs | 27 µs | 1.7x slower |
| Dict comprehension | 100 µs | 45 µs | 2.2x slower |
| Fibonacci (recursive) | 21.7 ms | 9.8 ms | 2.2x slower |
| List append (10k strings) | 13.9 ms | 6.1 ms | 2.3x slower |
| Tuple creation (10k pairs) | 11.2 ms | 8.9 ms | 1.3x slower |

End-to-end startup is where Ouros shines — no interpreter boot, no module imports, just parse and go. Runtime is typically 1.3–2.3x slower than CPython, which is fast enough for agent-generated code where the bottleneck is the LLM call, not the computation.

## Concepts

| Term | What it is |
|------|-----------|
| **Sandbox** | A compiled Python program. Parse once, run many times with different inputs. No access to the host. |
| **External function** | A host function that sandbox code can call by name. Execution pauses until the host provides a return value. This is how sandbox code talks to the outside world. |
| **Snapshot** | A frozen execution state, captured when an external function is called. Serializable to bytes — store in a database, send over the wire, resume in another process. |
| **SessionManager** | A persistent multi-session runtime. Variables survive across `execute()` calls. Think Jupyter kernel, but sandboxed. |
| **Session** | A single named environment within a SessionManager. Has its own variables, history, and heap. |
| **Fork** | Copy a session into an independent branch. The original is unchanged. Use for speculative execution or tree-of-thought. |
| **Rewind** | Undo the last N `execute()` calls in a session. Variables revert to their previous state. |
| **Heap introspection** | `heap_stats()`, `snapshot_heap()`, `diff_heap()` — inspect and compare memory state across executions. |
| **Resource limits** | Cap allocations, memory, stack depth, and wall-clock time. Execution terminates with `ResourceError` if exceeded. |
| **Type checking** | Static analysis via [ty](https://docs.astral.sh/ty/) before execution. Optional, zero runtime cost. |

## Acknowledgments

Ouros is forked from [Monty](https://github.com/pydantic/monty) by [Pydantic](https://pydantic.dev), originally created by [Samuel Colvin](https://github.com/samuelcolvin). The core interpreter, bytecode VM, and parser integration were developed as part of that project. Ouros extends the original with persistent sessions, session forking, rewind/history, heap introspection, cross-session variable transfer, and session serialization.

## License

MIT
