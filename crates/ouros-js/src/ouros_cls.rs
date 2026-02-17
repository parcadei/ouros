//! The main `Sandbox` class and iterative execution support for the TypeScript/JavaScript bindings.
//!
//! Provides a sandboxed Python interpreter that can be configured with inputs,
//! external functions, and resource limits. Supports both immediate execution
//! via `run()` and iterative execution via `start()`/`resume()`.
//!
//! ## Quick Start
//!
//! ```typescript
//! import { Sandbox } from 'ouros';
//!
//! // Simple execution
//! const m = new Sandbox('1 + 2');
//! const result = m.run(); // returns 3
//!
//! // With inputs
//! const m2 = new Sandbox('x + y', { inputs: ['x', 'y'] });
//! const result2 = m2.run({ inputs: { x: 10, y: 20 } }); // returns 30
//! ```
//!
//! ## Iterative Execution
//!
//! ```text
//! Sandbox.start() -> Snapshot | Complete
//!                       |
//!                       v
//! Snapshot.resume() -> Snapshot | Complete
//!                          |
//!                          v
//!                    (repeat until complete)
//! ```
//!
//! ```typescript
//! const m = new Sandbox('result = external_func(1, 2)', {
//!   externalFunctions: ['external_func']
//! });
//!
//! let progress = m.start();
//! while (progress instanceof Snapshot) {
//!   console.log(`Calling ${progress.functionName} with args:`, progress.args);
//!   progress = progress.resume({ returnValue: 42 });
//! }
//! console.log('Final result:', progress.output);
//! ```

use std::borrow::Cow;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use ouros::{
    CollectStringPrint, ExcType, Exception, ExternalResult, FutureSnapshot, LimitedTracker, NoLimitTracker,
    Object as OurosObject, ResourceTracker, RunProgress, Runner, Snapshot,
};
use ouros_type_checking::{type_check, SourceFile};

use crate::{
    convert::{js_to_ouros, ouros_to_js, JsOurosObject},
    exceptions::{JsOurosException, OurosTypingError},
    limits::JsResourceLimits,
};

// =============================================================================
// Ouros - Main interpreter class
// =============================================================================

/// A sandboxed Python interpreter instance.
///
/// Parses and compiles Python code on initialization, then can be run
/// multiple times with different input values. This separates the parsing
/// cost from execution, making repeated runs more efficient.
#[napi(js_name = "Sandbox")]
pub struct Ouros {
    /// The compiled code runner, ready to execute.
    runner: Runner,
    /// The artificial name of the python code "file".
    script_name: String,
    /// Names of input variables expected by the code.
    input_names: Vec<String>,
    /// Names of external functions the code can call.
    external_function_names: Vec<String>,
}

/// Options for creating a new Ouros instance.
#[napi(object, js_name = "SandboxOptions")]
pub struct OurosOptions {
    /// Name used in tracebacks and error messages. Default: 'main.py'
    pub script_name: Option<String>,
    /// List of input variable names available in the code.
    pub inputs: Option<Vec<String>>,
    /// List of external function names the code can call.
    pub external_functions: Option<Vec<String>>,
    /// Whether to perform type checking on the code. Default: false
    pub type_check: Option<bool>,
    /// Optional code to prepend before type checking.
    pub type_check_prefix_code: Option<String>,
}

/// Options for running code.
#[napi(object)]
#[derive(Clone)]
pub struct RunOptions<'env> {
    pub inputs: Option<Object<'env>>,
    /// Resource limits configuration.
    pub limits: Option<JsResourceLimits>,
    /// Dict of external function callbacks.
    /// Keys are function names, values are callable functions.
    pub external_functions: Option<Object<'env>>,
}

/// Options for starting execution.
#[napi(object)]
#[derive(Clone, Copy)]
pub struct StartOptions<'env> {
    /// Dict of input variable values.
    pub inputs: Option<Object<'env>>,
    /// Resource limits configuration.
    pub limits: Option<JsResourceLimits>,
}

#[napi]
impl Ouros {
    /// Creates a new Sandbox interpreter by parsing the given code.
    ///
    /// Returns either a Sandbox instance, an exception (for syntax errors), or a SandboxTypingError.
    /// The wrapper should check the result type and throw the appropriate error.
    ///
    /// @param code - Python code to execute
    /// @param options - Configuration options
    /// @returns Sandbox instance on success, or error object on failure
    #[napi]
    pub fn create(
        code: String,
        options: Option<OurosOptions>,
    ) -> Result<Either3<Self, JsOurosException, OurosTypingError>> {
        let options = options.unwrap_or(OurosOptions {
            script_name: None,
            inputs: None,
            external_functions: None,
            type_check: None,
            type_check_prefix_code: None,
        });

        let script_name = options.script_name.unwrap_or_else(|| "main.py".to_string());
        let input_names = options.inputs.unwrap_or_default();
        let external_function_names = options.external_functions.unwrap_or_default();
        let do_type_check = options.type_check.unwrap_or(false);

        // Perform type checking if requested
        if do_type_check {
            if let Some(error) = run_type_check_result(&code, &script_name, options.type_check_prefix_code.as_deref())?
            {
                return Ok(Either3::C(error));
            }
        }

        // Create the runner (parses the code)
        let runner = match Runner::new(code, &script_name, input_names.clone(), external_function_names.clone()) {
            Ok(r) => r,
            Err(exc) => return Ok(Either3::B(JsOurosException::new(exc))),
        };

        Ok(Either3::A(Self {
            runner,
            script_name,
            input_names,
            external_function_names,
        }))
    }

    /// Performs static type checking on the code.
    ///
    /// Returns either nothing (success) or a SandboxTypingError.
    ///
    /// @param prefixCode - Optional code to prepend before type checking
    /// @returns null on success, or SandboxTypingError on failure
    #[napi]
    pub fn type_check(&self, prefix_code: Option<String>) -> Result<Option<OurosTypingError>> {
        run_type_check_result(self.runner.code(), &self.script_name, prefix_code.as_deref())
    }

    /// Executes the code and returns the result, or an exception object if execution fails.
    ///
    /// @param options - Execution options (inputs, limits, externalFunctions)
    /// @returns The result of the last expression, or a SandboxException if execution fails
    #[napi]
    pub fn run<'env>(
        &self,
        env: &'env Env,
        options: Option<RunOptions<'env>>,
    ) -> Result<Either<JsOurosObject<'env>, JsOurosException>> {
        // Extract input values
        let input_values = self.extract_input_values(options.as_ref().and_then(|opts| opts.inputs), *env)?;

        let external_functions = options.as_ref().and_then(|opts| opts.external_functions);

        // If we have external functions declared, use the start/resume loop
        if !self.external_function_names.is_empty() {
            return self.run_with_external_functions(
                env,
                input_values,
                options.as_ref().and_then(|opts| opts.limits),
                external_functions,
            );
        }

        // No external functions - simple run
        let mut print_output = CollectStringPrint::default();

        let result = if let Some(limits) = options.as_ref().and_then(|opts| opts.limits) {
            let tracker = LimitedTracker::new(limits.into());
            self.runner.run(input_values, tracker, &mut print_output)
        } else {
            self.runner.run(input_values, NoLimitTracker, &mut print_output)
        };

        match result {
            Ok(value) => Ok(Either::A(ouros_to_js(&value, env)?)),
            Err(exc) => Ok(Either::B(JsOurosException::new(exc))),
        }
    }

    /// Internal helper to run code with external function callbacks.
    fn run_with_external_functions<'env>(
        &self,
        env: &'env Env,
        input_values: Vec<OurosObject>,
        limits: Option<JsResourceLimits>,
        external_functions: Option<Object<'env>>,
    ) -> Result<Either<JsOurosObject<'env>, JsOurosException>> {
        let mut print_output = CollectStringPrint::default();
        let runner = self.runner.clone();

        // Helper macro to handle the execution loop for both tracker types
        macro_rules! run_loop {
            ($tracker:expr) => {{
                let progress = runner.start(input_values, $tracker, &mut print_output);

                let mut progress = match progress {
                    Ok(p) => p,
                    Err(exc) => return Ok(Either::B(JsOurosException::new(exc))),
                };

                loop {
                    match progress {
                        RunProgress::Complete(result) => {
                            return Ok(Either::A(ouros_to_js(&result, env)?));
                        }
                        RunProgress::FunctionCall {
                            function_name,
                            args,
                            kwargs,
                            state,
                            ..
                        } => {
                            let return_value = call_external_function(
                                env,
                                external_functions.as_ref(),
                                &function_name,
                                &args,
                                &kwargs,
                            )?;

                            progress = match state.run(return_value, &mut print_output) {
                                Ok(p) => p,
                                Err(exc) => return Ok(Either::B(JsOurosException::new(exc))),
                            };
                        }
                        RunProgress::ResolveFutures(_) => {
                            return Err(Error::from_reason(
                                "Async futures are not supported in synchronous run(). Use start() for async execution.",
                            ));
                        }
                        RunProgress::OsCall { function, .. } => {
                            return Err(Error::from_reason(format!(
                                "OS calls are not supported: {function:?}",
                            )));
                        }
                    }
                }
            }};
        }

        if let Some(limits) = limits {
            let tracker = LimitedTracker::new(limits.into());
            run_loop!(tracker)
        } else {
            run_loop!(NoLimitTracker)
        }
    }

    /// Starts execution and returns either a snapshot (paused at external call), completion, or error.
    ///
    /// This method enables iterative execution where code pauses at external function
    /// calls, allowing the host to provide return values or exceptions before resuming.
    ///
    /// @param options - Execution options (inputs, limits)
    /// @returns Snapshot if paused, Complete if done, or SandboxException if failed
    #[napi]
    pub fn start<'env>(
        &self,
        env: &'env Env,
        options: Option<StartOptions<'env>>,
    ) -> Result<Either4<OurosSnapshot, OurosFutureSnapshot, OurosComplete, JsOurosException>> {
        // Extract input values
        let input_values = self.extract_input_values(options.and_then(|opts| opts.inputs), *env)?;

        // Clone the runner since start() consumes it - allows reuse of the parsed code
        let runner = self.runner.clone();
        let mut print_output = CollectStringPrint::default();

        // Start execution with appropriate tracker
        if let Some(limits) = options.and_then(|opts| opts.limits) {
            let tracker = LimitedTracker::new(limits.into());
            let progress = match runner.start(input_values, tracker, &mut print_output) {
                Ok(p) => p,
                Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
            };
            Ok(progress_to_result(progress, self.script_name.clone()))
        } else {
            let progress = match runner.start(input_values, NoLimitTracker, &mut print_output) {
                Ok(p) => p,
                Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
            };
            Ok(progress_to_result(progress, self.script_name.clone()))
        }
    }

    /// Serializes the Sandbox instance to a binary format.
    ///
    /// The serialized data can be stored and later restored with `Sandbox.load()`.
    /// This allows caching parsed code to avoid re-parsing on subsequent runs.
    ///
    /// @returns Buffer containing the serialized Sandbox instance
    #[napi]
    pub fn dump(&self) -> Result<Buffer> {
        let serialized = SerializedOuros {
            runner: self.runner.clone(),
            script_name: self.script_name.clone(),
            input_names: self.input_names.clone(),
            external_function_names: self.external_function_names.clone(),
        };
        let bytes =
            postcard::to_allocvec(&serialized).map_err(|e| Error::from_reason(format!("Serialization failed: {e}")))?;
        Ok(Buffer::from(bytes))
    }

    /// Deserializes a Sandbox instance from binary format.
    ///
    /// @param data - The serialized Sandbox data from `dump()`
    /// @returns A new Sandbox instance
    #[napi(factory)]
    pub fn load(data: Buffer) -> Result<Self> {
        let serialized: SerializedOuros =
            postcard::from_bytes(&data).map_err(|e| Error::from_reason(format!("Deserialization failed: {e}")))?;

        Ok(Self {
            runner: serialized.runner,
            script_name: serialized.script_name,
            input_names: serialized.input_names,
            external_function_names: serialized.external_function_names,
        })
    }

    /// Returns the script name.
    #[napi(getter)]
    pub fn script_name(&self) -> String {
        self.script_name.clone()
    }

    /// Returns the input variable names.
    #[napi(getter)]
    pub fn inputs(&self) -> Vec<String> {
        self.input_names.clone()
    }

    /// Returns the external function names.
    #[napi(getter)]
    pub fn external_functions(&self) -> Vec<String> {
        self.external_function_names.clone()
    }

    /// Returns a string representation of the Sandbox instance.
    #[napi]
    pub fn repr(&self) -> String {
        use std::fmt::Write;
        let lines = self.runner.code().lines().count();
        let mut s = format!(
            "Sandbox(<{} line{} of code>, scriptName='{}'",
            lines,
            if lines == 1 { "" } else { "s" },
            self.script_name
        );
        if !self.input_names.is_empty() {
            write!(s, ", inputs={:?}", self.input_names).unwrap();
        }
        if !self.external_function_names.is_empty() {
            write!(s, ", externalFunctions={:?}", self.external_function_names).unwrap();
        }
        s.push(')');
        s
    }

    /// Extracts input values from the JS Object in the order they were declared.
    fn extract_input_values(&self, inputs: Option<Object<'_>>, env: Env) -> Result<Vec<OurosObject>> {
        if self.input_names.is_empty() {
            if inputs.is_some() {
                return Err(Error::from_reason(
                    "No input variables declared but inputs object was provided",
                ));
            }
            return Ok(vec![]);
        }

        let Some(inputs) = inputs else {
            return Err(Error::from_reason(format!(
                "Missing required inputs: {:?}",
                self.input_names
            )));
        };

        // Extract values in declaration order
        self.input_names
            .iter()
            .map(|name| {
                if !inputs.has_named_property(name)? {
                    return Err(Error::from_reason(format!("Missing required input: '{name}'")));
                }
                let value: Unknown = inputs.get_named_property(name)?;
                js_to_ouros(value, env)
            })
            .collect()
    }
}

/// Performs type checking on the code and returns the error object if there are type errors.
///
/// Returns `None` if type checking passes, or `Some(OurosTypingError)` if there are errors.
fn run_type_check_result(code: &str, script_name: &str, prefix_code: Option<&str>) -> Result<Option<OurosTypingError>> {
    let source_code: Cow<str> = if let Some(prefix_code) = prefix_code {
        format!("{prefix_code}\n{code}").into()
    } else {
        code.into()
    };

    let source_file = SourceFile::new(&source_code, script_name);
    let result =
        type_check(&source_file, None).map_err(|e| Error::from_reason(format!("Type checking failed: {e}")))?;

    Ok(result.map(OurosTypingError::from_failure))
}

// =============================================================================
// EitherSnapshot - Internal enum to handle generic resource tracker types
// =============================================================================

/// Runtime execution snapshot, holds multiple resource tracker types since napi structs can't be generic.
///
/// Used internally by `Snapshot` to store execution state.
/// The `Done` variant indicates the snapshot has been consumed.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EitherSnapshot {
    NoLimit(Snapshot<NoLimitTracker>),
    Limited(Snapshot<LimitedTracker>),
    /// Done is used when taking the snapshot to run it.
    /// Should only be set after execution is complete.
    Done,
}

// =============================================================================
// OurosSnapshot - Paused execution at an external function call
// =============================================================================

/// Represents paused execution waiting for an external function call return value.
///
/// Contains information about the pending external function call and allows
/// resuming execution with the return value or an exception.
#[napi(js_name = "Snapshot")]
pub struct OurosSnapshot {
    /// The execution state that can be resumed.
    snapshot: EitherSnapshot,
    /// Name of the script being executed.
    script_name: String,
    /// The name of the external function being called.
    function_name: String,
    /// The positional arguments passed to the function (stored as OurosObject for serialization).
    args: Vec<OurosObject>,
    /// The keyword arguments passed to the function (stored as OurosObject pairs for serialization).
    kwargs: Vec<(OurosObject, OurosObject)>,
    /// The unique call ID for this external function invocation.
    ///
    /// This ID is stable across `Snapshot`/`FutureSnapshot` transitions and lets hosts
    /// correlate async results returned later via `resumeFutures()`.
    call_id: u32,
}

/// Options for resuming execution.
#[napi(object)]
pub struct ResumeOptions<'env> {
    /// The value to return from the external function call.
    pub return_value: Option<Unknown<'env>>,
    /// An exception to raise in the interpreter.
    /// Format: { type: string, message: string }
    pub exception: Option<ExceptionInput>,
    /// Marks this external call as a pending future instead of returning a concrete value.
    ///
    /// Hosts should set this to `true` when the JavaScript callback returns a Promise
    /// that will be resolved later through `FutureSnapshot::resume_futures()`.
    pub future: Option<bool>,
}

/// Input for raising an exception during resume.
#[napi(object)]
pub struct ExceptionInput {
    /// The exception type name (e.g., "ValueError").
    pub r#type: String,
    /// The exception message.
    pub message: String,
}

/// Options for loading a serialized snapshot.
#[napi(object)]
pub struct SnapshotLoadOptions {
    // Future: could add dataclass-like registry support
}

#[napi]
impl OurosSnapshot {
    /// Returns the name of the script being executed.
    #[napi(getter)]
    pub fn script_name(&self) -> String {
        self.script_name.clone()
    }

    /// Returns the name of the external function being called.
    #[napi(getter)]
    pub fn function_name(&self) -> String {
        self.function_name.clone()
    }

    /// Returns the unique call ID for this external function invocation.
    #[napi(getter)]
    pub fn call_id(&self) -> u32 {
        self.call_id
    }

    /// Returns the positional arguments passed to the external function.
    #[napi(getter)]
    pub fn args<'env>(&self, env: &'env Env) -> Result<Vec<JsOurosObject<'env>>> {
        self.args.iter().map(|obj| ouros_to_js(obj, env)).collect()
    }

    /// Returns the keyword arguments passed to the external function as an object.
    #[napi(getter)]
    pub fn kwargs<'env>(&self, env: &'env Env) -> Result<Object<'env>> {
        let mut obj = Object::new(env)?;
        for (k, v) in &self.kwargs {
            // Keys should be strings
            let key = match k {
                OurosObject::String(s) => s.clone(),
                _ => format!("{k:?}"),
            };
            let js_value = ouros_to_js(v, env)?;
            obj.set_named_property(&key, js_value)?;
        }
        Ok(obj)
    }

    /// Resumes execution with either a return value, an exception, or a pending future marker.
    ///
    /// Exactly one of `returnValue`, `exception`, or `future: true` must be provided.
    ///
    /// @param options - Object with either `returnValue`, `exception`, or `future: true`
    /// @returns Snapshot if paused, Complete if done, or SandboxException if failed
    #[napi]
    pub fn resume<'env>(
        &mut self,
        env: &'env Env,
        options: ResumeOptions<'env>,
    ) -> Result<Either4<Self, OurosFutureSnapshot, OurosComplete, JsOurosException>> {
        // Validate that exactly one of returnValue, exception, or future:true is provided.
        let external_result = match (options.return_value, options.exception, options.future) {
            (Some(value), None, None) => {
                let ouros_value = js_to_ouros(value, *env)?;
                ExternalResult::Return(ouros_value)
            }
            (None, Some(exc), None) => {
                let ouros_exc = Exception::new(string_to_exc_type(&exc.r#type)?, Some(exc.message));
                ExternalResult::Error(ouros_exc)
            }
            (None, None, Some(true)) => ExternalResult::Future,
            (None, None, Some(false)) => {
                return Err(Error::from_reason(
                    "resume() accepts returnValue or exception or future: true (future: false is invalid)",
                ));
            }
            _ => {
                return Err(Error::from_reason(
                    "resume() requires exactly one of returnValue or exception or future: true",
                ));
            }
        };

        // Take the snapshot, replacing with Done
        let snapshot = std::mem::replace(&mut self.snapshot, EitherSnapshot::Done);

        // Resume execution based on the snapshot type
        let mut print_output = CollectStringPrint::default();
        match snapshot {
            EitherSnapshot::NoLimit(state) => {
                let progress = match state.run(external_result, &mut print_output) {
                    Ok(p) => p,
                    Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
                };
                Ok(progress_to_result(progress, self.script_name.clone()))
            }
            EitherSnapshot::Limited(state) => {
                let progress = match state.run(external_result, &mut print_output) {
                    Ok(p) => p,
                    Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
                };
                Ok(progress_to_result(progress, self.script_name.clone()))
            }
            EitherSnapshot::Done => Err(Error::from_reason("Snapshot has already been resumed")),
        }
    }

    /// Serializes the Snapshot to a binary format.
    ///
    /// The serialized data can be stored and later restored with `Snapshot.load()`.
    /// This allows suspending execution and resuming later, potentially in a different process.
    ///
    /// @returns Buffer containing the serialized snapshot
    #[napi]
    pub fn dump(&self) -> Result<Buffer> {
        if matches!(self.snapshot, EitherSnapshot::Done) {
            return Err(Error::from_reason("Cannot dump snapshot that has already been resumed"));
        }

        let serialized = SerializedSnapshot {
            snapshot: &self.snapshot,
            script_name: &self.script_name,
            function_name: &self.function_name,
            args: &self.args,
            kwargs: &self.kwargs,
            call_id: self.call_id,
        };

        let bytes =
            postcard::to_allocvec(&serialized).map_err(|e| Error::from_reason(format!("Serialization failed: {e}")))?;
        Ok(Buffer::from(bytes))
    }

    /// Deserializes a Snapshot from binary format.
    ///
    /// @param data - The serialized snapshot data from `dump()`
    /// @param options - Optional load options (reserved for future use)
    /// @returns A new Snapshot instance
    #[napi(factory)]
    pub fn load(data: Buffer, _options: Option<SnapshotLoadOptions>) -> Result<Self> {
        let serialized: SerializedSnapshotOwned =
            postcard::from_bytes(&data).map_err(|e| Error::from_reason(format!("Deserialization failed: {e}")))?;

        Ok(Self {
            snapshot: serialized.snapshot,
            script_name: serialized.script_name,
            function_name: serialized.function_name,
            args: serialized.args,
            kwargs: serialized.kwargs,
            call_id: serialized.call_id,
        })
    }

    /// Returns a string representation of the Snapshot.
    #[napi]
    pub fn repr(&self) -> String {
        format!(
            "Snapshot(scriptName='{}', functionName='{}', args={:?}, kwargs={:?}, callId={})",
            self.script_name, self.function_name, self.args, self.kwargs, self.call_id
        )
    }
}

// =============================================================================
// OurosComplete - Completed execution
// =============================================================================

/// Represents completed execution with a final output value.
///
/// The output value is stored as an `OurosObject` internally and converted to JS on access.
#[napi(js_name = "Complete")]
pub struct OurosComplete {
    /// The final output value from the executed code.
    output_value: OurosObject,
}

#[napi]
impl OurosComplete {
    /// Returns the final output value from the executed code.
    #[napi(getter)]
    pub fn output<'env>(&self, env: &'env Env) -> Result<JsOurosObject<'env>> {
        ouros_to_js(&self.output_value, env)
    }

    /// Returns a string representation of the Complete.
    #[napi]
    #[must_use]
    pub fn repr(&self) -> String {
        format!("Complete(output={:?})", self.output_value)
    }
}

// =============================================================================
// OurosFutureSnapshot - Paused execution waiting for async futures
// =============================================================================

/// Internal enum to hold a `FutureSnapshot` with either resource tracker type.
///
/// Mirrors `EitherSnapshot` but for async future resolution state instead of
/// synchronous external function call state.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EitherFutureSnapshot {
    NoLimit(FutureSnapshot<NoLimitTracker>),
    Limited(FutureSnapshot<LimitedTracker>),
    /// Sentinel indicating this snapshot has already been consumed via `resumeFutures()`.
    Done,
}

/// Represents paused execution waiting for one or more async futures to resolve.
///
/// This is returned when code `await`s an external future that hasn't been resolved yet.
/// The host must provide results for the pending call IDs using `resumeFutures()`.
///
/// Supports incremental resolution -- you can resolve a subset of pending calls,
/// and the interpreter will continue running until it blocks again on remaining futures.
#[napi(js_name = "FutureSnapshot")]
pub struct OurosFutureSnapshot {
    /// The future execution state that can be resumed with results.
    future_snapshot: EitherFutureSnapshot,
    /// Name of the script being executed.
    script_name: String,
    /// The pending call IDs waiting for resolution.
    pending_call_ids: Vec<u32>,
}

/// A single future result: a call ID paired with either a return value or an exception.
///
/// Exactly one of `returnValue` or `exception` must be provided for each entry.
#[napi(object)]
pub struct FutureResultInput<'env> {
    /// The call ID this result corresponds to (from `pendingCallIds`).
    pub call_id: u32,
    /// The resolved value for this future.
    pub return_value: Option<Unknown<'env>>,
    /// An exception to raise for this future.
    pub exception: Option<ExceptionInput>,
}

#[napi]
impl OurosFutureSnapshot {
    /// Returns the call IDs of pending futures that need resolution.
    ///
    /// The host should provide results for some or all of these via `resumeFutures()`.
    #[napi(getter)]
    pub fn pending_call_ids(&self) -> Vec<u32> {
        self.pending_call_ids.clone()
    }

    /// Returns the name of the script being executed.
    #[napi(getter)]
    pub fn script_name(&self) -> String {
        self.script_name.clone()
    }

    /// Resumes execution by providing results for pending futures.
    ///
    /// Each entry in `results` maps a `callId` to either a `returnValue` or an `exception`.
    /// You may provide a subset of pending calls for incremental resolution.
    ///
    /// @param results - Array of {callId, returnValue} or {callId, exception} objects
    /// @returns Snapshot if a new external call is hit, FutureSnapshot if more futures
    ///          need resolution, Complete if done, or SandboxException if failed
    #[napi]
    pub fn resume_futures<'env>(
        &mut self,
        env: &'env Env,
        results: Vec<FutureResultInput<'env>>,
    ) -> Result<Either4<OurosSnapshot, Self, OurosComplete, JsOurosException>> {
        // Convert JS results to (u32, ExternalResult) pairs
        let ouros_results = results
            .into_iter()
            .map(|r| {
                let external_result = match (r.return_value, r.exception) {
                    (Some(value), None) => {
                        let ouros_value = js_to_ouros(value, *env)?;
                        Ok(ExternalResult::Return(ouros_value))
                    }
                    (None, Some(exc)) => {
                        let ouros_exc = Exception::new(string_to_exc_type(&exc.r#type)?, Some(exc.message));
                        Ok(ExternalResult::Error(ouros_exc))
                    }
                    (Some(_), Some(_)) => Err(Error::from_reason(format!(
                        "Future result for callId {} has both returnValue and exception",
                        r.call_id
                    ))),
                    (None, None) => Err(Error::from_reason(format!(
                        "Future result for callId {} has neither returnValue nor exception",
                        r.call_id
                    ))),
                }?;
                Ok((r.call_id, external_result))
            })
            .collect::<Result<Vec<_>>>()?;

        // Take the snapshot, replacing with Done
        let future_snapshot = std::mem::replace(&mut self.future_snapshot, EitherFutureSnapshot::Done);

        let mut print_output = CollectStringPrint::default();
        match future_snapshot {
            EitherFutureSnapshot::NoLimit(state) => {
                let progress = match state.resume(ouros_results, &mut print_output) {
                    Ok(p) => p,
                    Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
                };
                Ok(progress_to_result(progress, self.script_name.clone()))
            }
            EitherFutureSnapshot::Limited(state) => {
                let progress = match state.resume(ouros_results, &mut print_output) {
                    Ok(p) => p,
                    Err(exc) => return Ok(Either4::D(JsOurosException::new(exc))),
                };
                Ok(progress_to_result(progress, self.script_name.clone()))
            }
            EitherFutureSnapshot::Done => Err(Error::from_reason("FutureSnapshot has already been resumed")),
        }
    }

    /// Returns a string representation of the FutureSnapshot.
    #[napi]
    #[must_use]
    pub fn repr(&self) -> String {
        format!(
            "FutureSnapshot(scriptName='{}', pendingCallIds={:?})",
            self.script_name, self.pending_call_ids
        )
    }
}

/// Trait to convert a typed `FutureSnapshot` into `EitherFutureSnapshot`.
trait FromFutureSnapshot<T: ResourceTracker> {
    fn from_future_snapshot(snapshot: FutureSnapshot<T>) -> Self;
}

impl FromFutureSnapshot<NoLimitTracker> for EitherFutureSnapshot {
    fn from_future_snapshot(snapshot: FutureSnapshot<NoLimitTracker>) -> Self {
        Self::NoLimit(snapshot)
    }
}

impl FromFutureSnapshot<LimitedTracker> for EitherFutureSnapshot {
    fn from_future_snapshot(snapshot: FutureSnapshot<LimitedTracker>) -> Self {
        Self::Limited(snapshot)
    }
}

// =============================================================================
// Helper functions for progress conversion
// =============================================================================

/// Converts a `RunProgress` to a `Snapshot`, `FutureSnapshot`, `Complete`,
/// or `SandboxException`.
///
/// Maps each `RunProgress` variant to the appropriate JS-visible type:
/// - `FunctionCall` -> `Snapshot` (paused at sync external call)
/// - `ResolveFutures` -> `FutureSnapshot` (paused waiting for async futures)
/// - `Complete` -> `Complete` (execution finished)
/// - `OsCall` -> `SandboxException` (not yet supported)
fn progress_to_result<T>(
    progress: RunProgress<T>,
    script_name: String,
) -> Either4<OurosSnapshot, OurosFutureSnapshot, OurosComplete, JsOurosException>
where
    T: ResourceTracker + serde::Serialize + serde::de::DeserializeOwned,
    EitherSnapshot: FromSnapshot<T>,
    EitherFutureSnapshot: FromFutureSnapshot<T>,
{
    match progress {
        RunProgress::Complete(result) => Either4::C(OurosComplete { output_value: result }),
        RunProgress::FunctionCall {
            function_name,
            args,
            kwargs,
            call_id,
            state,
        } => {
            // Store args/kwargs as OurosObject directly for serialization
            Either4::A(OurosSnapshot {
                snapshot: EitherSnapshot::from_snapshot(state),
                script_name,
                function_name,
                args,
                kwargs,
                call_id,
            })
        }
        RunProgress::ResolveFutures(future_state) => {
            let pending_call_ids = future_state.pending_call_ids().to_vec();
            Either4::B(OurosFutureSnapshot {
                future_snapshot: EitherFutureSnapshot::from_future_snapshot(future_state),
                script_name,
                pending_call_ids,
            })
        }
        RunProgress::OsCall { function, .. } => {
            let exc = Exception::new(
                ExcType::NotImplementedError,
                Some(format!(
                    "OS calls are not yet supported in the JS bindings: {function:?}"
                )),
            );
            Either4::D(JsOurosException::new(exc))
        }
    }
}

/// Trait to convert a typed Snapshot into EitherSnapshot.
trait FromSnapshot<T: ResourceTracker> {
    fn from_snapshot(snapshot: Snapshot<T>) -> Self;
}

impl FromSnapshot<NoLimitTracker> for EitherSnapshot {
    fn from_snapshot(snapshot: Snapshot<NoLimitTracker>) -> Self {
        Self::NoLimit(snapshot)
    }
}

impl FromSnapshot<LimitedTracker> for EitherSnapshot {
    fn from_snapshot(snapshot: Snapshot<LimitedTracker>) -> Self {
        Self::Limited(snapshot)
    }
}

/// Converts a string exception type to `ExcType`.
fn string_to_exc_type(type_name: &str) -> Result<ExcType> {
    type_name
        .parse()
        .map_err(|_| Error::from_reason(format!("Invalid exception type: '{type_name}'")))
}

// =============================================================================
// Serialization types
// =============================================================================

/// Serialization wrapper for `Ouros` that includes all fields needed for reconstruction.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedOuros {
    runner: Runner,
    script_name: String,
    input_names: Vec<String>,
    external_function_names: Vec<String>,
}

/// Serialization wrapper for `OurosSnapshot` using borrowed references.
#[derive(serde::Serialize)]
struct SerializedSnapshot<'a> {
    snapshot: &'a EitherSnapshot,
    script_name: &'a str,
    function_name: &'a str,
    args: &'a [OurosObject],
    kwargs: &'a [(OurosObject, OurosObject)],
    call_id: u32,
}

/// Owned version of `SerializedSnapshot` for deserialization.
#[derive(serde::Deserialize)]
struct SerializedSnapshotOwned {
    snapshot: EitherSnapshot,
    script_name: String,
    function_name: String,
    args: Vec<OurosObject>,
    kwargs: Vec<(OurosObject, OurosObject)>,
    call_id: u32,
}

// =============================================================================
// External function support
// =============================================================================

/// Calls a JavaScript external function and returns the result.
///
/// Converts args/kwargs from Ouros format, calls the JS function,
/// and converts the result back to Ouros format (or an exception).
fn call_external_function(
    env: &Env,
    external_functions: Option<&Object<'_>>,
    function_name: &str,
    args: &[OurosObject],
    kwargs: &[(OurosObject, OurosObject)],
) -> Result<ExternalResult> {
    // Get the external functions dict, or error if not provided
    let functions = external_functions.ok_or_else(|| {
        Error::from_reason(format!(
            "External function '{function_name}' called but no externalFunctions provided"
        ))
    })?;

    // Look up the function by name
    if !functions.has_named_property(function_name)? {
        // Return a KeyError exception that will be raised in Ouros
        let exc = Exception::new(
            ExcType::KeyError,
            Some(format!("\"External function '{function_name}' not found\"")),
        );
        return Ok(ExternalResult::Error(exc));
    }

    let callable: Unknown = functions.get_named_property(function_name)?;

    // Convert positional arguments to JS
    let mut js_args: Vec<sys::napi_value> = Vec::with_capacity(args.len() + 1);
    for arg in args {
        js_args.push(ouros_to_js(arg, env)?.raw());
    }

    // If we have kwargs, add them as a final object argument
    if !kwargs.is_empty() {
        let mut kwargs_obj = Object::new(env)?;
        for (key, value) in kwargs {
            let key_str = match key {
                OurosObject::String(s) => s.clone(),
                _ => format!("{key:?}"),
            };
            kwargs_obj.set_named_property(&key_str, ouros_to_js(value, env)?)?;
        }
        js_args.push(kwargs_obj.raw());
    }

    // Get undefined for the 'this' argument
    let mut undefined_raw = std::ptr::null_mut();
    // SAFETY: [DH] - all arguments are valid and result is valid on success
    unsafe {
        sys::napi_get_undefined(env.raw(), &raw mut undefined_raw);
    }

    // Call the function using raw napi
    let mut result_raw = std::ptr::null_mut();
    // SAFETY: [DH] - all arguments are valid and result is valid on success
    let status = unsafe {
        sys::napi_call_function(
            env.raw(),
            undefined_raw, // this = undefined
            callable.raw(),
            js_args.len(),
            js_args.as_ptr(),
            &raw mut result_raw,
        )
    };

    if status != sys::Status::napi_ok {
        // An error occurred - get the pending exception
        let mut is_exception = false;
        // SAFETY: [DH] - all arguments are valid
        unsafe { sys::napi_is_exception_pending(env.raw(), &raw mut is_exception) };

        if is_exception {
            let mut exception_raw = std::ptr::null_mut();
            // SAFETY: [DH] - all arguments are valid and exception_raw is valid on success
            let status = unsafe { sys::napi_get_and_clear_last_exception(env.raw(), &raw mut exception_raw) };

            if status != sys::Status::napi_ok {
                // Failed to get the exception - return a generic error
                let exc = Exception::new(
                    ExcType::RuntimeError,
                    Some("External function call failed and exception could not be retrieved".to_string()),
                );
                return Ok(ExternalResult::Error(exc));
            }
            let exception_obj = Object::from_raw(env.raw(), exception_raw);
            let exc = extract_js_exception(exception_obj);
            return Ok(ExternalResult::Error(exc));
        }

        // Generic error
        let exc = Exception::new(ExcType::RuntimeError, Some("External function call failed".to_string()));
        return Ok(ExternalResult::Error(exc));
    }

    // Convert the result back to Ouros format
    // SAFETY: [DH] - result_raw is valid on success
    let result = unsafe { Unknown::from_raw_unchecked(env.raw(), result_raw) };
    let ouros_result = js_to_ouros(result, *env)?;
    Ok(ExternalResult::Return(ouros_result))
}

/// Extracts exception info from a JS exception object.
fn extract_js_exception(exception_obj: Object<'_>) -> Exception {
    // Try to get the 'name' property (e.g., "ValueError")
    let name: std::result::Result<String, _> = exception_obj.get_named_property("name");
    // Try to get the 'message' property
    let message: std::result::Result<String, _> = exception_obj.get_named_property("message");

    let exc_type = name
        .ok()
        .and_then(|n| string_to_exc_type(&n).ok())
        .unwrap_or(ExcType::RuntimeError);
    let msg = message.ok();

    Exception::new(exc_type, msg)
}
