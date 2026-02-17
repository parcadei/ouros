// Ergonomic TypeScript wrapper for the native SessionManager.
//
// Provides a `SessionManager` class that creates `Session` objects, each
// representing a named interpreter session with methods for execution,
// variable management, history/rewind, and heap inspection.

import type { NapiExecuteResult, NapiVariableValue, NapiEvalResult, NapiHeapDiffResult } from './index.js'

import { SessionManager as NativeSessionManager } from './index.js'

// =============================================================================
// Result types - ergonomic wrappers around napi output types
// =============================================================================

/**
 * Result of executing code in a session.
 *
 * Contains execution output (stdout, result value, completion status)
 * and progress state when paused at an external function call.
 */
export interface ExecuteResult {
  /** Standard output captured during execution. */
  stdout: string
  /** Whether execution completed (true) or paused at an external call (false). */
  isComplete: boolean
  /** The result value, parsed from JSON. `undefined` for None or non-complete progress. */
  result: unknown
  /** For FunctionCall progress: the external function name. */
  functionName?: string
  /** For FunctionCall progress: the call ID for resume. */
  callId?: number
  /** For FunctionCall progress: positional args parsed from JSON. */
  args?: unknown[]
  /** For ResolveFutures progress: pending call IDs. */
  pendingCallIds?: number[]
}

/**
 * Info about a single variable in a session.
 */
export interface VariableInfo {
  /** Variable name. */
  name: string
  /** Python type name (e.g. "int", "str", "list"). */
  typeName: string
}

/**
 * A variable's value with JSON and repr representations.
 */
export interface VariableValue {
  /** The value, parsed from JSON. */
  jsonValue: unknown
  /** Python `repr()` string, if available. */
  repr: string | null
}

/**
 * Result of evaluating an expression without side effects.
 */
export interface EvalResult {
  /** The computed value. */
  value: VariableValue
  /** Standard output captured during evaluation. */
  stdout: string
}

/**
 * Summary info for one active session.
 */
export interface SessionInfo {
  /** The session ID. */
  id: string
  /** Number of defined global variables. */
  variableCount: number
}

/**
 * Result of a rewind operation.
 */
export interface RewindResult {
  /** Number of steps actually rewound. */
  stepsRewound: number
  /** Number of history entries remaining after rewind. */
  historyRemaining: number
}

/**
 * History depth info for a session.
 */
export interface HistoryInfo {
  /** Current undo history depth. */
  current: number
  /** Maximum configured history depth. */
  max: number
}

/**
 * Heap statistics for a session.
 */
export interface HeapStats {
  /** Total number of live objects on the heap. */
  liveObjects: number
  /** Number of free (recycled) slots. */
  freeSlots: number
  /** Total heap capacity (live + free). */
  totalSlots: number
  /** Number of interned strings. */
  internedStrings: number
}

/**
 * Aggregate heap counter deltas between two snapshots.
 */
export interface HeapDiff {
  /** Change in live object count. */
  liveObjectsDelta: number
  /** Change in free slot count. */
  freeSlotsDelta: number
  /** Change in total slot count. */
  totalSlotsDelta: number
  /** Change in interned string count. */
  internedStringsDelta: number
}

/**
 * One variable that changed between two heap snapshots.
 */
export interface ChangedVariable {
  /** Variable name. */
  name: string
  /** Repr string in the "before" state. */
  before: string
  /** Repr string in the "after" state. */
  after: string
}

/**
 * Variable-level diff summary between two heap states.
 */
export interface VariableDiff {
  /** Variables present only in the "after" state. */
  added: string[]
  /** Variables present only in the "before" state. */
  removed: string[]
  /** Variables whose repr changed between states. */
  changed: ChangedVariable[]
  /** Variables with identical repr in both states. */
  unchanged: string[]
}

/**
 * Result of a heap diff operation.
 */
export interface HeapDiffResult {
  /** Aggregate heap counter deltas. */
  heapDiff: HeapDiff
  /** Variable-level changes between two states. */
  variableDiff: VariableDiff
}

/**
 * Result of a save operation.
 */
export interface SaveResult {
  /** The snapshot name used for the save. */
  name: string
  /** Size of the saved snapshot in bytes. */
  sizeBytes: number
}

/**
 * Info about a saved session snapshot on disk.
 */
export interface SavedSessionInfo {
  /** Snapshot name (filename without extension). */
  name: string
  /** Size of the snapshot file in bytes. */
  sizeBytes: number
}

/**
 * Options for creating a new SessionManager.
 */
export interface SessionManagerOptions {
  /** Script name for tracebacks and error messages. Defaults to "session.py". */
  scriptName?: string
  /** Directory for session persistence (save/load). If not set, persistence is disabled. */
  storageDir?: string
}

// =============================================================================
// Conversion helpers
// =============================================================================

/** Parses the napi execute result into an ergonomic `ExecuteResult`. */
function wrapExecuteResult(raw: NapiExecuteResult): ExecuteResult {
  let result: unknown = undefined
  if (raw.resultJson != null) {
    try {
      result = JSON.parse(raw.resultJson)
    } catch {
      result = raw.resultJson
    }
  }

  let args: unknown[] | undefined = undefined
  if (raw.argsJson != null) {
    try {
      args = JSON.parse(raw.argsJson) as unknown[]
    } catch {
      args = undefined
    }
  }

  return {
    stdout: raw.stdout,
    isComplete: raw.isComplete,
    result,
    functionName: raw.functionName ?? undefined,
    callId: raw.callId ?? undefined,
    args,
    pendingCallIds: raw.pendingCallIds ?? undefined,
  }
}

/** Converts a napi variable value (with JSON string) to the ergonomic form. */
function wrapVariableValue(raw: NapiVariableValue): VariableValue {
  let jsonValue: unknown
  try {
    jsonValue = JSON.parse(raw.jsonValue)
  } catch {
    jsonValue = raw.jsonValue
  }
  return {
    jsonValue,
    repr: raw.repr ?? null,
  }
}

/** Converts a napi eval result to the ergonomic form. */
function wrapEvalResult(raw: NapiEvalResult): EvalResult {
  return {
    value: wrapVariableValue(raw.value),
    stdout: raw.stdout,
  }
}

/** Converts a napi heap diff result to the ergonomic form. */
function wrapHeapDiffResult(raw: NapiHeapDiffResult): HeapDiffResult {
  return {
    heapDiff: {
      liveObjectsDelta: raw.heapDiff.liveObjectsDelta,
      freeSlotsDelta: raw.heapDiff.freeSlotsDelta,
      totalSlotsDelta: raw.heapDiff.totalSlotsDelta,
      internedStringsDelta: raw.heapDiff.internedStringsDelta,
    },
    variableDiff: {
      added: raw.variableDiff.added,
      removed: raw.variableDiff.removed,
      changed: raw.variableDiff.changed.map((c) => ({
        name: c.name,
        before: c.before,
        after: c.after,
      })),
      unchanged: raw.variableDiff.unchanged,
    },
  }
}

// =============================================================================
// Session - A handle to one named session within a SessionManager
// =============================================================================

/**
 * Represents a single named interpreter session within a `SessionManager`.
 *
 * This is a lightweight handle that delegates all operations to the underlying
 * native `SessionManager`, tagging each call with this session's ID.
 *
 * Do not construct directly -- use `SessionManager.createSession()` or
 * `SessionManager.getSession()`.
 */
export class Session {
  private _manager: SessionManager
  private _id: string

  /** @internal */
  constructor(manager: SessionManager, sessionId: string) {
    this._manager = manager
    this._id = sessionId
  }

  /** The session ID. */
  get id(): string {
    return this._id
  }

  // -------------------------------------------------------------------------
  // Execution
  // -------------------------------------------------------------------------

  /**
   * Executes Python code in this session.
   *
   * @param code - Python code to execute
   * @returns Execution result with stdout, completion status, and value
   * @throws When the code has syntax errors or raises a runtime exception
   */
  execute(code: string): ExecuteResult {
    return wrapExecuteResult(this._manager._native.execute(code, this._id))
  }

  // -------------------------------------------------------------------------
  // Variables
  // -------------------------------------------------------------------------

  /**
   * Lists defined global variables and their types.
   */
  listVariables(): VariableInfo[] {
    return this._manager._native.listVariables(this._id).map((v) => ({
      name: v.name,
      typeName: v.typeName,
    }))
  }

  /**
   * Gets one variable's value from the session namespace.
   *
   * @param name - Variable name
   * @returns The variable value with JSON and repr representations
   * @throws When the variable does not exist
   */
  getVariable(name: string): VariableValue {
    return wrapVariableValue(this._manager._native.getVariable(this._id, name))
  }

  /**
   * Sets or creates a global variable by evaluating a Python expression.
   *
   * @param name - Variable name to set
   * @param valueExpr - Python expression to evaluate (e.g. "[1, 2, 3]", "'hello'")
   */
  setVariable(name: string, valueExpr: string): void {
    this._manager._native.setVariable(this._id, name, valueExpr)
  }

  /**
   * Deletes a global variable from the session.
   *
   * @param name - Variable name to delete
   * @returns true if the variable existed and was removed, false otherwise
   */
  deleteVariable(name: string): boolean {
    return this._manager._native.deleteVariable(this._id, name)
  }

  /**
   * Evaluates a Python expression without modifying session state.
   *
   * The expression is executed in a forked copy of the session, so no
   * side effects are persisted.
   *
   * @param expression - Python expression to evaluate
   * @returns The computed value and any captured stdout
   */
  evalVariable(expression: string): EvalResult {
    return wrapEvalResult(this._manager._native.evalVariable(this._id, expression))
  }

  // -------------------------------------------------------------------------
  // Fork
  // -------------------------------------------------------------------------

  /**
   * Forks this session into a new independent copy.
   *
   * The forked session starts with the same state (variables, heap) but
   * subsequent modifications are independent.
   *
   * @param newId - ID for the forked session
   * @returns A new Session handle for the fork
   */
  fork(newId: string): Session {
    this._manager._native.forkSession(this._id, newId)
    return new Session(this._manager, newId)
  }

  // -------------------------------------------------------------------------
  // History / rewind
  // -------------------------------------------------------------------------

  /**
   * Rewinds this session by N steps, restoring a previous state.
   *
   * @param steps - Number of steps to rewind (default: 1)
   * @returns Information about the rewind (steps rewound, remaining history)
   */
  rewind(steps: number = 1): RewindResult {
    const raw = this._manager._native.rewind(this._id, steps)
    return { stepsRewound: raw.stepsRewound, historyRemaining: raw.historyRemaining }
  }

  /**
   * Returns the current undo history depth and configured maximum.
   */
  history(): HistoryInfo {
    const raw = this._manager._native.history(this._id)
    return { current: raw.current, max: raw.max }
  }

  /**
   * Configures the maximum undo history depth for this session.
   *
   * If the new maximum is less than the current depth, the oldest entries
   * are trimmed. Returns the number of entries that were trimmed.
   *
   * @param maxDepth - New maximum history depth
   * @returns Number of entries that were trimmed
   */
  setHistoryDepth(maxDepth: number): number {
    return this._manager._native.setHistoryDepth(this._id, maxDepth)
  }

  // -------------------------------------------------------------------------
  // Heap introspection
  // -------------------------------------------------------------------------

  /**
   * Returns heap statistics for this session.
   */
  heapStats(): HeapStats {
    const raw = this._manager._native.heapStats(this._id)
    return {
      liveObjects: raw.liveObjects,
      freeSlots: raw.freeSlots,
      totalSlots: raw.totalSlots,
      internedStrings: raw.internedStrings,
    }
  }

  // -------------------------------------------------------------------------
  // Reset
  // -------------------------------------------------------------------------

  /**
   * Resets this session, replacing it with a fresh interpreter instance.
   *
   * The session's history is cleared. Pass external function names if the
   * session needs them after reset.
   *
   * @param externalFunctions - External function names for the reset session
   */
  reset(externalFunctions?: string[]): void {
    this._manager._native.reset(this._id, externalFunctions)
  }
}

// =============================================================================
// SessionManager - Multi-session interpreter manager
// =============================================================================

/**
 * Multi-session Python interpreter manager.
 *
 * Manages a registry of named interpreter sessions, each with its own
 * variables, heap, and execution history. A "default" session is always
 * present and is used when no session ID is specified.
 *
 * @example
 * ```typescript
 * const mgr = new SessionManager()
 * const session = mgr.getSession()
 * session.execute('x = 42')
 * console.log(session.getVariable('x').jsonValue) // 42
 *
 * const other = mgr.createSession('analysis')
 * other.execute('data = [1, 2, 3]')
 * mgr.transferVariable('analysis', 'default', 'data')
 * ```
 */
export class SessionManager {
  /** @internal -- the native napi session manager instance. */
  _native: NativeSessionManager

  /**
   * Creates a new session manager.
   *
   * @param options - Configuration options (script name, storage directory)
   */
  constructor(options?: SessionManagerOptions) {
    this._native = new NativeSessionManager(options?.scriptName, options?.storageDir)
  }

  // -------------------------------------------------------------------------
  // Session lifecycle
  // -------------------------------------------------------------------------

  /**
   * Creates a new named session.
   *
   * @param sessionId - Unique ID for the new session
   * @param options - Optional configuration (external function names)
   * @returns A Session handle for the new session
   * @throws When a session with the given ID already exists
   */
  createSession(sessionId: string, options?: { externalFunctions?: string[] }): Session {
    this._native.createSession(sessionId, options?.externalFunctions)
    return new Session(this, sessionId)
  }

  /**
   * Gets a Session handle for an existing session.
   *
   * @param sessionId - Session ID (default: the "default" session)
   * @returns A Session handle
   */
  getSession(sessionId?: string): Session {
    return new Session(this, sessionId ?? 'default')
  }

  /**
   * Destroys a named session. The default session cannot be destroyed.
   *
   * @param sessionId - ID of the session to destroy
   * @throws When the session is the default or does not exist
   */
  destroySession(sessionId: string): void {
    this._native.destroySession(sessionId)
  }

  /**
   * Lists all active sessions with their variable counts.
   */
  listSessions(): SessionInfo[] {
    return this._native.listSessions().map((s) => ({
      id: s.id,
      variableCount: s.variableCount,
    }))
  }

  // -------------------------------------------------------------------------
  // Cross-session operations
  // -------------------------------------------------------------------------

  /**
   * Transfers a variable from one session to another.
   *
   * Reads the variable from the source session and writes it into the
   * target session. The transfer is heap-independent (no raw references leak).
   *
   * @param source - Source session ID
   * @param target - Target session ID
   * @param name - Variable name in the source session
   * @param targetName - Optional different name in the target session
   */
  transferVariable(source: string, target: string, name: string, targetName?: string): void {
    this._native.transferVariable(source, target, name, targetName)
  }

  /**
   * Executes code in a source session and stores the result in a target session.
   *
   * @param source - Source session ID (null for default)
   * @param target - Target session ID
   * @param code - Python code to execute in the source
   * @param targetVariable - Variable name in the target to store the result
   * @returns Execution result
   */
  callSession(source: string | null, target: string, code: string, targetVariable: string): ExecuteResult {
    return wrapExecuteResult(this._native.callSession(source, target, code, targetVariable))
  }

  // -------------------------------------------------------------------------
  // Heap introspection
  // -------------------------------------------------------------------------

  /**
   * Saves the current heap stats as a named snapshot for later diff.
   *
   * @param name - Snapshot name
   * @param sessionId - Session ID (default: the "default" session)
   */
  snapshotHeap(name: string, sessionId?: string): void {
    this._native.snapshotHeap(sessionId ?? null, name)
  }

  /**
   * Compares two named heap snapshots and returns the diff.
   *
   * @param before - Name of the "before" snapshot
   * @param after - Name of the "after" snapshot
   * @returns Heap diff with aggregate deltas and variable-level changes
   */
  diffHeap(before: string, after: string): HeapDiffResult {
    return wrapHeapDiffResult(this._native.diffHeap(before, after))
  }

  // -------------------------------------------------------------------------
  // Persistence
  // -------------------------------------------------------------------------

  /**
   * Saves a session to disk as a named snapshot.
   *
   * Requires `storageDir` to be set in the constructor options.
   *
   * @param sessionId - Session ID (default: the "default" session)
   * @param name - Snapshot name (default: the session ID)
   * @returns Save result with name and size
   */
  saveSession(sessionId?: string, name?: string): SaveResult {
    const raw = this._native.saveSession(sessionId, name)
    return { name: raw.name, sizeBytes: raw.sizeBytes }
  }

  /**
   * Loads a previously saved session from disk.
   *
   * @param name - Snapshot name to load
   * @param sessionId - ID for the new session (default: the snapshot name)
   * @returns The session ID that was created
   */
  loadSession(name: string, sessionId?: string): string {
    return this._native.loadSession(name, sessionId)
  }

  /**
   * Lists all saved session snapshots on disk.
   */
  listSavedSessions(): SavedSessionInfo[] {
    return this._native.listSavedSessions().map((s) => ({
      name: s.name,
      sizeBytes: s.sizeBytes,
    }))
  }
}
