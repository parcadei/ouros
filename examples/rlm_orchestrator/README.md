# RLM Orchestrator

Demonstrates using Ouros as the sandboxed REPL runtime for a [Recursive Language Model](https://arxiv.org/abs/2512.24601) (Zhang et al., 2025).

## How it works

A root LLM generates Python code. Ouros executes it in a sandboxed session where variables persist across turns. When the code calls `llm_query(prompt)`, execution pauses and the host performs the actual LLM call, then resumes execution with the result.

The root model's context stays tiny (~10-20k tokens) regardless of how much data is being processed -- all heavy data lives as Python variables in Ouros's heap.

## Usage

```bash
# Mock mode (no API key needed)
python main.py --mock "What are the first 10 prime numbers?"

# With a real LLM
export OPENAI_API_KEY=sk-...
python main.py "Summarize the key themes in this document" --context large_doc.txt

# With Anthropic
export ANTHROPIC_API_KEY=sk-ant-...
python main.py "Analyze this codebase for security issues" --context repo_dump.txt
```

## Architecture

```
Root LLM (bounded context)
    |
    | generates Python code
    v
Ouros SessionManager (persistent REPL)
    |
    | executes code, pauses on llm_query()
    v
Host loop (this script)
    |
    | calls real LLM, resumes execution
    v
Variables persist in Ouros heap
```

## Why Ouros for RLMs

- **Sandboxed**: No filesystem, network, or subprocess access -- safe to run untrusted model-generated code
- **Persistent variables**: `SessionManager` keeps variables alive across turns
- **External functions**: `llm_query()` pauses execution cleanly via snapshots
- **Fork/rewind**: `fork_session()` enables tree-of-thought; `rewind()` enables backtracking
- **Serializable**: `dump()`/`load()` can suspend execution to disk and resume later
- **Resource limits**: Cap memory, allocations, and execution time
