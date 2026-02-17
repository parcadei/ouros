// Custom error classes that extend Error for proper JavaScript error handling.
// These wrap the native Rust classes to provide instanceof support.

import type {
  SandboxOptions,
  RunOptions,
  ResourceLimits,
  Frame,
  ExceptionInfo,
  StartOptions,
  ResumeOptions,
  ExceptionInput,
  SnapshotLoadOptions,
  FutureResultInput,
  JsObject,
} from './index.js'

import {
  Sandbox as NativeSandbox,
  Snapshot as NativeSnapshot,
  FutureSnapshot as NativeFutureSnapshot,
  Complete as NativeComplete,
  SandboxException as NativeSandboxException,
  SandboxTypingError as NativeSandboxTypingError,
} from './index.js'

export type {
  SandboxOptions,
  RunOptions,
  ResourceLimits,
  Frame,
  ExceptionInfo,
  StartOptions,
  ResumeOptions,
  ExceptionInput,
  SnapshotLoadOptions,
  FutureResultInput,
  JsObject,
}


/**
 * Alias for ResourceLimits (deprecated name).
 */
export type JsResourceLimits = ResourceLimits

/**
 * Base class for all Ouros interpreter errors.
 *
 * This is the parent class for `OurosSyntaxError`, `OurosRuntimeError`, and `SandboxTypingError`.
 * Catching `OurosError` will catch any exception raised by the sandbox.
 */
export class OurosError extends Error {
  protected _typeName: string
  protected _message: string

  constructor(typeName: string, message: string) {
    super(message ? `${typeName}: ${message}` : typeName)
    this.name = 'OurosError'
    this._typeName = typeName
    this._message = message
    // Maintains proper stack trace for where our error was thrown (only available on V8)
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, OurosError)
    }
  }

  /**
   * Returns information about the inner Python exception.
   */
  get exception(): ExceptionInfo {
    return {
      typeName: this._typeName,
      message: this._message,
    }
  }

  /**
   * Returns formatted exception string.
   * @param format - 'type-msg' for 'ExceptionType: message', 'msg' for just the message
   */
  display(format: 'type-msg' | 'msg' = 'msg'): string {
    switch (format) {
      case 'msg':
        return this._message
      case 'type-msg':
        return this._message ? `${this._typeName}: ${this._message}` : this._typeName
      default:
        throw new Error(`Invalid display format: '${format}'. Expected 'type-msg' or 'msg'`)
    }
  }
}

/**
 * Raised when Python code has syntax errors or cannot be parsed by Ouros.
 *
 * The inner exception is always a `SyntaxError`. Use `display()` to get
 * formatted error output.
 */
export class OurosSyntaxError extends OurosError {
  private _native: NativeSandboxException | null

  constructor(messageOrNative: string | NativeSandboxException) {
    if (typeof messageOrNative === 'string') {
      super('SyntaxError', messageOrNative)
      this._native = null
    } else {
      const exc = messageOrNative.exception
      super('SyntaxError', exc.message)
      this._native = messageOrNative
    }
    this.name = 'OurosSyntaxError'
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, OurosSyntaxError)
    }
  }

  /**
   * Returns formatted exception string.
   * @param format - 'type-msg' for 'SyntaxError: message', 'msg' for just the message
   */
  override display(format: 'type-msg' | 'msg' = 'msg'): string {
    if (this._native && typeof this._native.display === 'function') {
      return this._native.display(format)
    }
    return super.display(format)
  }
}

/**
 * Raised when Ouros code fails during execution.
 *
 * Provides access to the traceback frames where the error occurred via `traceback()`,
 * and formatted output via `display()`.
 */
export class OurosRuntimeError extends OurosError {
  private _native: NativeSandboxException | null
  private _tracebackString: string | null
  private _frames: Frame[] | null

  constructor(
    nativeOrTypeName: NativeSandboxException | string,
    message?: string,
    tracebackString?: string,
    frames?: Frame[],
  ) {
    if (typeof nativeOrTypeName === 'string') {
      // Legacy constructor: (typeName, message, tracebackString, frames)
      super(nativeOrTypeName, message!)
      this._native = null
      this._tracebackString = tracebackString ?? null
      this._frames = frames ?? null
    } else {
      // New constructor: (nativeException)
      const exc = nativeOrTypeName.exception
      super(exc.typeName, exc.message)
      this._native = nativeOrTypeName
      this._tracebackString = null
      this._frames = null
    }
    this.name = 'OurosRuntimeError'
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, OurosRuntimeError)
    }
  }

  /**
   * Returns the Ouros traceback as an array of Frame objects.
   */
  traceback(): Frame[] {
    if (this._native) {
      return this._native.traceback()
    }
    return this._frames || []
  }

  /**
   * Returns formatted exception string.
   * @param format - 'traceback' for full traceback, 'type-msg' for 'ExceptionType: message', 'msg' for just the message
   */
  display(format: 'traceback' | 'type-msg' | 'msg' = 'traceback'): string {
    if (this._native && typeof this._native.display === 'function') {
      return this._native.display(format)
    }
    // Fallback for legacy constructor
    switch (format) {
      case 'traceback':
        return this._tracebackString || this.message
      case 'type-msg':
        return this._message ? `${this._typeName}: ${this._message}` : this._typeName
      case 'msg':
        return this._message
      default:
        throw new Error(`Invalid display format: '${format}'. Expected 'traceback', 'type-msg', or 'msg'`)
    }
  }
}

export type TypingDisplayFormat =
  | 'full'
  | 'concise'
  | 'azure'
  | 'json'
  | 'jsonlines'
  | 'rdjson'
  | 'pylint'
  | 'gitlab'
  | 'github'

/**
 * Raised when type checking finds errors in the code.
 *
 * This exception is raised when static type analysis detects type errors.
 * Use `displayDiagnostics()` to render rich diagnostics in various formats for tooling integration.
 * Use `display()` (inherited) for simple 'type-msg' or 'msg' formats.
 */
export class SandboxTypingError extends OurosError {
  private _native: NativeSandboxTypingError | null

  constructor(messageOrNative: string | NativeSandboxTypingError, nativeError: NativeSandboxTypingError | null = null) {
    if (typeof messageOrNative === 'string') {
      super('TypeError', messageOrNative)
      this._native = nativeError
    } else {
      const exc = messageOrNative.exception
      super('TypeError', exc.message)
      this._native = messageOrNative
    }
    this.name = 'SandboxTypingError'
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, SandboxTypingError)
    }
  }

  /**
   * Renders rich type error diagnostics for tooling integration.
   *
   * @param format - Output format (default: 'full')
   * @param color - Include ANSI color codes (default: false)
   */
  displayDiagnostics(format: TypingDisplayFormat = 'full', color: boolean = false): string {
    if (this._native && typeof this._native.display === 'function') {
      return this._native.display(format, color)
    }
    return this._message
  }
}

/**
 * Wrapped Sandbox class that throws proper Error subclasses.
 */
export class Sandbox {
  private _native: NativeSandbox

  /**
   * Creates a new Sandbox interpreter by parsing the given code.
   *
   * @param code - Python code to execute
   * @param options - Configuration options
   * @throws {OurosSyntaxError} If the code has syntax errors
   * @throws {SandboxTypingError} If type checking is enabled and finds errors
   */
  constructor(code: string, options?: SandboxOptions) {
    const result = NativeSandbox.create(code, options)

    if (result instanceof NativeSandboxException) {
      // Check typeName to distinguish syntax errors from other exceptions
      if (result.exception.typeName === 'SyntaxError') {
        throw new OurosSyntaxError(result)
      }
      throw new OurosRuntimeError(result)
    }
    if (result instanceof NativeSandboxTypingError) {
      throw new SandboxTypingError(result)
    }

    this._native = result
  }

  /**
   * Performs static type checking on the code.
   *
   * @param prefixCode - Optional code to prepend before type checking
   * @throws {SandboxTypingError} If type checking finds errors
   */
  typeCheck(prefixCode?: string): void {
    const result = this._native.typeCheck(prefixCode)
    if (result instanceof NativeSandboxTypingError) {
      throw new SandboxTypingError(result)
    }
  }

  /**
   * Executes the code and returns the result.
   *
   * @param options - Execution options (inputs, limits)
   * @returns The result of the last expression
   * @throws {OurosRuntimeError} If the code raises an exception
   */
  run(options?: RunOptions): JsObject {
    const result = this._native.run(options)
    if (result instanceof NativeSandboxException) {
      throw new OurosRuntimeError(result)
    }
    return result
  }

  /**
   * Starts execution and returns either a snapshot (paused at external call),
   * a future snapshot (paused waiting for async futures), or completion.
   *
   * @param options - Execution options (inputs, limits)
   * @returns Snapshot if an external function call is pending,
   *          FutureSnapshot if async futures need resolution,
   *          Complete if done
   * @throws {OurosRuntimeError} If the code raises an exception
   */
  start(options?: StartOptions): Snapshot | FutureSnapshot | Complete {
    const result = this._native.start(options)
    return wrapStartResult(result)
  }

  /**
   * Serializes the Sandbox instance to a binary format.
   */
  dump(): Buffer {
    return this._native.dump()
  }

  /**
   * Deserializes a Sandbox instance from binary format.
   */
  static load(data: Buffer): Sandbox {
    const instance = Object.create(Sandbox.prototype) as Sandbox
    instance._native = NativeSandbox.load(data)
    return instance
  }

  /** Returns the script name. */
  get scriptName(): string {
    return this._native.scriptName
  }

  /** Returns the input variable names. */
  get inputs(): string[] {
    return this._native.inputs
  }

  /** Returns the external function names. */
  get externalFunctions(): string[] {
    return this._native.externalFunctions
  }

  /** Returns a string representation of the Sandbox instance. */
  repr(): string {
    return this._native.repr()
  }
}

/**
 * Helper to wrap native start/resume results, throwing errors as needed.
 */
function wrapStartResult(
  result: NativeSnapshot | NativeFutureSnapshot | NativeComplete | NativeSandboxException,
): Snapshot | FutureSnapshot | Complete {
  if (result instanceof NativeSandboxException) {
    throw new OurosRuntimeError(result)
  }
  if (result instanceof NativeSnapshot) {
    return new Snapshot(result)
  }
  if (result instanceof NativeFutureSnapshot) {
    return new FutureSnapshot(result)
  }
  if (result instanceof NativeComplete) {
    return new Complete(result)
  }
  throw new Error(`Unexpected result type from native binding: ${result}`)
}

/**
 * Represents paused execution waiting for an external function call return value.
 *
 * Contains information about the pending external function call and allows
 * resuming execution with the return value or an exception.
 */
export class Snapshot {
  private _native: NativeSnapshot

  constructor(nativeSnapshot: NativeSnapshot) {
    this._native = nativeSnapshot
  }

  /** Returns the name of the script being executed. */
  get scriptName(): string {
    return this._native.scriptName
  }

  /** Returns the name of the external function being called. */
  get functionName(): string {
    return this._native.functionName
  }

  /**
   * Returns the unique call ID for this external invocation.
   *
   * This ID is used to correlate Promise results with `FutureSnapshot.pendingCallIds`.
   */
  get callId(): number {
    return this._native.callId
  }

  /** Returns the positional arguments passed to the external function. */
  get args(): JsObject[] {
    return this._native.args
  }

  /** Returns the keyword arguments passed to the external function as an object. */
  get kwargs(): Record<string, JsObject> {
    return this._native.kwargs as Record<string, JsObject>
  }

  /**
   * Resumes execution with either a return value or an exception.
   *
   * @param options - Object with either `returnValue` or `exception`
   * @returns Snapshot if another external call is pending,
   *          FutureSnapshot if async futures need resolution,
   *          Complete if done
   * @throws {OurosRuntimeError} If the code raises an exception
   */
  resume(options: ResumeOptions): Snapshot | FutureSnapshot | Complete {
    const result = this._native.resume(options)
    return wrapStartResult(result)
  }

  /**
   * Serializes the Snapshot to a binary format.
   */
  dump(): Buffer {
    return this._native.dump()
  }

  /**
   * Deserializes a Snapshot from binary format.
   */
  static load(data: Buffer, options?: SnapshotLoadOptions): Snapshot {
    const nativeSnapshot = NativeSnapshot.load(data, options)
    return new Snapshot(nativeSnapshot)
  }

  /** Returns a string representation of the Snapshot. */
  repr(): string {
    return this._native.repr()
  }
}

/**
 * Represents paused execution waiting for one or more async futures to resolve.
 *
 * This is returned when code `await`s an external future that hasn't been resolved yet.
 * The host must provide results for the pending call IDs using `resumeFutures()`.
 *
 * Supports incremental resolution -- you can resolve a subset of pending calls,
 * and the interpreter will continue running until it blocks again on remaining futures.
 */
export class FutureSnapshot {
  private _native: NativeFutureSnapshot

  constructor(nativeFutureSnapshot: NativeFutureSnapshot) {
    this._native = nativeFutureSnapshot
  }

  /** Returns the name of the script being executed. */
  get scriptName(): string {
    return this._native.scriptName
  }

  /** Returns the call IDs of pending futures that need resolution. */
  get pendingCallIds(): number[] {
    return this._native.pendingCallIds
  }

  /**
   * Resumes execution by providing results for pending futures.
   *
   * Each entry in `results` maps a `callId` to either a `returnValue` or an `exception`.
   * You may provide a subset of pending calls for incremental resolution.
   *
   * @param results - Array of {callId, returnValue} or {callId, exception} objects
   * @returns Snapshot if a new external call is hit,
   *          FutureSnapshot if more futures need resolution,
   *          Complete if done
   * @throws {OurosRuntimeError} If the code raises an exception
   */
  resumeFutures(results: FutureResultInput[]): Snapshot | FutureSnapshot | Complete {
    const result = this._native.resumeFutures(results)
    return wrapStartResult(result)
  }

  /** Returns a string representation of the FutureSnapshot. */
  repr(): string {
    return this._native.repr()
  }
}

/**
 * Represents completed execution with a final output value.
 */
export class Complete {
  private _native: NativeComplete

  constructor(nativeComplete: NativeComplete) {
    this._native = nativeComplete
  }

  /** Returns the final output value from the executed code. */
  get output(): JsObject {
    return this._native.output
  }

  /** Returns a string representation of the Complete. */
  repr(): string {
    return this._native.repr()
  }
}

/**
 * Options for `runSandboxAsync`.
 */
export interface RunSandboxAsyncOptions {
  /** Input values for the script. */
  inputs?: Record<string, JsObject>
  /** External function implementations (sync or async). */
  externalFunctions?: Record<string, (...args: unknown[]) => unknown>
  /** Resource limits. */
  limits?: ResourceLimits
}


/**
 * Runs a Sandbox script with async external function support.
 *
 * This function handles both synchronous and asynchronous external functions.
 * Promise-returning callbacks are resumed using the future protocol so sandbox
 * code can `await` them correctly.
 *
 * @param sandboxRunner - The Sandbox runner instance to execute
 * @param options - Execution options
 * @returns The output of the Sandbox script
 * @throws {OurosRuntimeError} If the code raises an exception
 * @throws {OurosSyntaxError} If the code has syntax errors
 *
 * @example
 * const m = new Sandbox('result = await fetch_data(url)', {
 *   inputs: ['url'],
 *   externalFunctions: ['fetch_data']
 * });
 *
 * const result = await runSandboxAsync(m, {
 *   inputs: { url: 'https://example.com' },
 *   externalFunctions: {
 *     fetch_data: async (url) => {
 *       const response = await fetch(url);
 *       return response.text();
 *     }
 *   }
 * });
 */
export async function runSandboxAsync(sandboxRunner: Sandbox, options: RunSandboxAsyncOptions = {}): Promise<JsObject> {
  const { inputs, externalFunctions = {}, limits } = options

  type SettledFuture = { callId: number; result: unknown } | { callId: number; error: Error }

  // Track pending async calls: callId -> Promise<settled future result>
  const pendingFutures = new Map<number, Promise<SettledFuture>>()

  let progress: Snapshot | FutureSnapshot | Complete = sandboxRunner.start({ inputs, limits })

  while (!(progress instanceof Complete)) {
    if (progress instanceof Snapshot) {
      const snapshot = progress
      const funcName = snapshot.functionName
      const extFunction = externalFunctions[funcName]

      if (!extFunction) {
        // Function not found - resume with a KeyError exception
        progress = snapshot.resume({
          exception: {
            type: 'KeyError',
            message: `"External function '${funcName}' not found"`,
          },
        })
        continue
      }

      let result: unknown
      try {
        result = extFunction(...snapshot.args, snapshot.kwargs)
      } catch (error) {
        // External function threw synchronously - convert to Ouros exception
        const err = normalizeError(error)
        const excType = err.name || 'RuntimeError'
        const excMessage = err.message || String(error)
        progress = snapshot.resume({
          exception: {
            type: excType,
            message: excMessage,
          },
        })
        continue
      }

      // Promise-returning functions default to eager resolution for backwards compatibility.
      // If Ouros rejects the resolved value as non-awaitable, retry this call using the
      // future protocol so Python `await` works correctly.
      if (isPromiseLike(result)) {
        const callId = snapshot.callId
        const snapshotDump = snapshot.dump()

        const settled: SettledFuture = await result.then(
          (value) => ({ callId, result: value }),
          (error) => ({ callId, error: normalizeError(error) }),
        )

        if ('error' in settled) {
          progress = snapshot.resume({
            exception: {
              type: settled.error.name || 'RuntimeError',
              message: settled.error.message || String(settled.error),
            },
          })
          continue
        }

        try {
          progress = snapshot.resume({ returnValue: settled.result })
        } catch (resumeError) {
          if (!isAwaitProtocolMismatch(resumeError)) {
            throw resumeError
          }
          pendingFutures.set(callId, Promise.resolve(settled))
          progress = Snapshot.load(snapshotDump).resume({ future: true })
        }
      } else {
        progress = snapshot.resume({ returnValue: result })
      }
    } else if (progress instanceof FutureSnapshot) {
      // Resolve whichever requested future completes first.
      const pendingIds = progress.pendingCallIds
      const requested = pendingIds
        .map((callId) => pendingFutures.get(callId))
        .filter((pending): pending is Promise<SettledFuture> => pending !== undefined)

      if (requested.length === 0) {
        throw new Error(`FutureSnapshot: no results available for pending call IDs: [${pendingIds.join(', ')}]`)
      }

      const resolved = await Promise.race(requested)
      pendingFutures.delete(resolved.callId)

      if ('error' in resolved) {
        progress = progress.resumeFutures([
          {
            callId: resolved.callId,
            exception: {
              type: resolved.error.name || 'RuntimeError',
              message: resolved.error.message || String(resolved.error),
            },
          },
        ])
      } else {
        progress = progress.resumeFutures([{ callId: resolved.callId, returnValue: resolved.result }])
      }
    }
  }

  return progress.output
}

/**
 * Returns true when a value behaves like a Promise.
 */
function isPromiseLike(value: unknown): value is Promise<unknown> {
  return value !== null && (typeof value === 'object' || typeof value === 'function')
    && typeof (value as Promise<unknown>).then === 'function'
}

/**
 * Normalizes arbitrary thrown values into Error instances.
 */
function normalizeError(error: unknown): Error {
  if (error instanceof Error) {
    return error
  }
  return new Error(String(error))
}

/**
 * Detects errors that indicate Ouros expected an awaitable, not an eager value.
 */
function isAwaitProtocolMismatch(error: unknown): boolean {
  if (!(error instanceof OurosRuntimeError)) {
    return false
  }
  if (error.exception.typeName !== 'TypeError') {
    return false
  }
  return /await/i.test(error.exception.message)
}

// Re-export session management types and classes
export { Session, SessionManager } from './session.js'
export type {
  ExecuteResult,
  VariableInfo,
  VariableValue,
  EvalResult,
  SessionInfo,
  RewindResult,
  HistoryInfo,
  HeapStats,
  HeapDiff,
  ChangedVariable,
  VariableDiff,
  HeapDiffResult,
  SaveResult,
  SavedSessionInfo,
  SessionManagerOptions,
} from './session.js'
