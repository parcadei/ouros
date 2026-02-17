"""RLM (Recursive Language Model) orchestrator using Ouros as the sandboxed REPL.

Demonstrates the pattern from Zhang et al. (arXiv:2512.24601): a root LLM
generates Python code that runs inside a persistent, sandboxed REPL. The model
can call `llm_query()` to delegate sub-tasks to itself (or another model),
with all intermediate results stored as Python variables in the REPL -- never
in the LLM's context window.

The root model's prompt stays tiny and bounded (<20k tokens per turn) regardless
of how much data is being processed. The REPL (Ouros) holds everything in memory.

Architecture:
    Root LLM  -->  generates Python code
        |
        v
    Ouros SessionManager  -->  executes code, pauses on llm_query()
        |
        v
    Host loop  -->  intercepts llm_query(), calls real LLM, resumes execution
        |
        v
    Variables persist in Ouros heap across turns

Usage:
    # Set your LLM API key
    export OPENAI_API_KEY=sk-...
    # or export ANTHROPIC_API_KEY=sk-ant-...

    python main.py

    # Or use the mock mode (no API key needed):
    python main.py --mock
"""

from __future__ import annotations

import argparse
from typing import Any, Callable

from ouros import SessionManager


def create_rlm_session(
    manager: SessionManager,
    session_id: str = 'rlm',
) -> Any:
    """Create a session pre-configured for RLM use.

    The session declares `llm_query` as an external function so the model's
    generated code can call it. Execution will pause (snapshot) whenever
    llm_query() is called, letting the host perform the actual LLM call.
    """
    return manager.create_session(
        session_id,
        external_functions=['llm_query'],
    )


def build_root_prompt(
    user_query: str,
    variable_summaries: list[dict[str, Any]],
    last_stdout: str,
) -> str:
    """Build the prompt for the root LLM.

    Follows the RLM pattern: the root model sees only metadata about the REPL
    state, never the full variable contents. This keeps the prompt bounded.
    """
    var_info = ''
    if variable_summaries:
        lines = []
        for v in variable_summaries:
            name = v.get('name', '?')
            type_name = v.get('type', '?')
            repr_preview = v.get('repr', '')
            # Truncate repr to keep prompt small
            if len(repr_preview) > 200:
                repr_preview = repr_preview[:200] + '...'
            lines.append(f'  {name}: {type_name} = {repr_preview}')
        var_info = '\nCurrent REPL variables:\n' + '\n'.join(lines)

    stdout_info = ''
    if last_stdout:
        preview = last_stdout[:500]
        if len(last_stdout) > 500:
            preview += f'... ({len(last_stdout)} chars total)'
        stdout_info = f'\nLast stdout:\n  {preview}'

    return f"""You are an RLM (Recursive Language Model). You have access to a persistent
Python REPL where variables survive across turns. You can call llm_query(prompt)
to delegate sub-tasks to an LLM.

RULES:
- Write Python code to solve the task. Your code runs in a sandbox.
- Use llm_query(prompt) for any sub-task requiring reasoning or generation.
- Store results in variables -- they persist across turns.
- Print short summaries, not full data (the host only shows you a preview).
- When done, store your final answer in a variable called `answer`.
- Available stdlib: json, re, math, collections, itertools, datetime, statistics, etc.

Task: {user_query}
{var_info}{stdout_info}

Respond with ONLY a Python code block. No explanation."""


def mock_llm(prompt: str) -> str:
    """Mock LLM for demonstration -- echoes a summary back."""
    return f'[Mock LLM response to: {prompt[:100]}...]'


def call_real_llm(prompt: str) -> str:
    """Call a real LLM API. Tries OpenAI first, then Anthropic."""
    import os

    openai_key = os.environ.get('OPENAI_API_KEY')
    anthropic_key = os.environ.get('ANTHROPIC_API_KEY')

    if openai_key:
        import urllib.request

        data = {
            'model': 'gpt-4o-mini',
            'messages': [{'role': 'user', 'content': prompt}],
            'max_tokens': 2048,
        }
        import json as json_mod

        req = urllib.request.Request(
            'https://api.openai.com/v1/chat/completions',
            data=json_mod.dumps(data).encode(),
            headers={
                'Authorization': f'Bearer {openai_key}',
                'Content-Type': 'application/json',
            },
        )
        with urllib.request.urlopen(req) as resp:
            result = json_mod.loads(resp.read())
        return result['choices'][0]['message']['content']

    if anthropic_key:
        import urllib.request

        data = {
            'model': 'claude-sonnet-4-20250514',
            'max_tokens': 2048,
            'messages': [{'role': 'user', 'content': prompt}],
        }
        import json as json_mod

        req = urllib.request.Request(
            'https://api.anthropic.com/v1/messages',
            data=json_mod.dumps(data).encode(),
            headers={
                'x-api-key': anthropic_key,
                'anthropic-version': '2023-06-01',
                'Content-Type': 'application/json',
            },
        )
        with urllib.request.urlopen(req) as resp:
            result = json_mod.loads(resp.read())
        return result['content'][0]['text']

    raise RuntimeError(
        'No LLM API key found. Set OPENAI_API_KEY or ANTHROPIC_API_KEY, '
        'or use --mock mode.'
    )


def run_rlm(
    user_query: str,
    llm_fn: Callable[[str], str] = mock_llm,
    max_turns: int = 10,
    context: str | None = None,
    verbose: bool = True,
) -> str:
    """Run an RLM loop: root model generates code, Ouros executes it.

    Args:
        user_query: The task for the RLM to solve.
        llm_fn: Function that takes a prompt string and returns LLM output.
        max_turns: Maximum number of root model turns before stopping.
        context: Optional large context to inject as a variable.
        verbose: Print execution details.

    Returns:
        The final answer (contents of the `answer` variable).
    """
    manager = SessionManager()
    session = create_rlm_session(manager)

    # Inject large context if provided (this is the "10M tokens in RAM" pattern)
    if context:
        # Use repr() to safely inject the string as a Python literal
        session.set_variable('context', repr(context))
        if verbose:
            print(f'Injected context: {len(context):,} chars')

    last_stdout = ''

    for turn in range(max_turns):
        if verbose:
            print(f'\n--- Turn {turn + 1}/{max_turns} ---')

        # Build root prompt with REPL metadata (not full variable contents)
        variables = session.list_variables()
        root_prompt = build_root_prompt(user_query, variables, last_stdout)

        if verbose:
            print(f'Root prompt size: {len(root_prompt):,} chars')

        # Root model generates code
        code = llm_fn(root_prompt)

        # Strip markdown fences if present
        if code.startswith('```python'):
            code = code[len('```python'):]
        if code.startswith('```'):
            code = code[3:]
        if code.endswith('```'):
            code = code[:-3]
        code = code.strip()

        if verbose:
            print(f'Generated code:\n  {code[:200]}...' if len(code) > 200 else f'Generated code:\n  {code}')

        # Capture stdout
        stdout_lines = []

        def print_callback(_stream: str, text: str) -> None:
            stdout_lines.append(text)

        # Execute in Ouros -- this is the core RLM step
        session._manager._native.reset(
            session_id=session.id,
            external_functions=['llm_query'],
        )
        # Re-inject variables after reset... actually, let's use execute directly
        # The session preserves variables across execute() calls
        result = session.execute(code)

        # Handle external function calls (llm_query snapshots)
        while result.get('status') == 'snapshot':
            fn_name = result.get('function_name', '')
            args = result.get('args', ())
            call_id = result.get('call_id')

            if fn_name == 'llm_query' and args:
                sub_prompt = args[0] if isinstance(args[0], str) else str(args[0])
                if verbose:
                    print(f'  Sub-call: llm_query({sub_prompt[:80]}...)')
                sub_result = llm_fn(sub_prompt)
                result = session.resume(call_id, sub_result)
            else:
                # Unknown function -- resume with None
                result = session.resume(call_id, None)

        # Collect stdout
        last_stdout = result.get('stdout', '')
        if verbose and last_stdout:
            print(f'  stdout: {last_stdout[:200]}')

        # Check if the model set an `answer` variable
        try:
            answer_var = session.get_variable('answer')
            if verbose:
                print(f'\nFinal answer found: {answer_var}')
            return answer_var.get('json_value', answer_var.get('repr', str(answer_var)))
        except Exception:
            # No answer yet -- continue looping
            if verbose:
                print('  (no `answer` variable yet, continuing...)')

    return 'RLM did not produce an answer within max_turns'


def main():
    parser = argparse.ArgumentParser(description='RLM Orchestrator using Ouros')
    parser.add_argument('query', nargs='?', default='What are the first 10 prime numbers?',
                        help='Task for the RLM to solve')
    parser.add_argument('--mock', action='store_true', help='Use mock LLM (no API key needed)')
    parser.add_argument('--max-turns', type=int, default=5, help='Maximum turns')
    parser.add_argument('--context', type=str, default=None,
                        help='Path to a file to inject as context')
    parser.add_argument('--quiet', action='store_true', help='Suppress verbose output')
    args = parser.parse_args()

    llm_fn = mock_llm if args.mock else call_real_llm

    context = None
    if args.context:
        with open(args.context) as f:
            context = f.read()

    answer = run_rlm(
        user_query=args.query,
        llm_fn=llm_fn,
        max_turns=args.max_turns,
        context=context,
        verbose=not args.quiet,
    )
    print(f'\n=== Answer ===\n{answer}')


if __name__ == '__main__':
    main()
