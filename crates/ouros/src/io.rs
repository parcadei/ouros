use std::{
    borrow::Cow,
    cell::RefCell,
    io::{self, Write as _},
};

use crate::{exception_public::Exception, heap::HeapId};

/// Trait for handling output from the `print()` builtin function.
///
/// Implement this trait to capture or redirect print output from sandboxed Python code.
/// The default implementation `StdPrint` writes to stdout.
pub trait PrintWriter {
    /// Called once for each formatted argument passed to `print()`.
    ///
    /// This method is responsible for writing only the given argument's text, and must
    /// not add separators or a trailing newline. Separators (such as spaces) and the
    /// final terminator (such as a newline) are emitted via [`stdout_push`].
    ///
    /// # Arguments
    /// * `output` - The formatted output string for a single argument (without
    ///   separators or trailing newline).
    fn stdout_write(&mut self, output: Cow<'_, str>) -> Result<(), Exception>;

    /// Add a single character to stdout.
    ///
    /// Generally called to add spaces and newlines within print output.
    ///
    /// # Arguments
    /// * `end` - The character to print after the formatted output.
    fn stdout_push(&mut self, end: char) -> Result<(), Exception>;
}

/// Default `PrintWriter` that writes to stdout.
///
/// This is the default writer used when no custom writer is provided.
#[derive(Debug)]
pub struct StdPrint;

thread_local! {
    /// Thread-local stdout buffer for `StdPrint`.
    ///
    /// Buffering stdout matches CPython behavior when stdout is redirected: stderr output
    /// (such as warnings) appears before buffered stdout lines.
    static STDOUT_BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    /// Thread-local redirect stack for `contextlib.redirect_stdout`.
    static STDOUT_REDIRECT_STACK: RefCell<Vec<RedirectTarget>> = const { RefCell::new(Vec::new()) };
    /// Thread-local redirect stack for `contextlib.redirect_stderr`.
    static STDERR_REDIRECT_STACK: RefCell<Vec<RedirectTarget>> = const { RefCell::new(Vec::new()) };
}

/// Active stream redirection target for `print()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedirectTarget {
    /// Suppress output for the redirected stream.
    Sink,
    /// Route output to a heap-backed file-like object.
    Heap(HeapId),
}

/// Pushes a stdout redirection target onto the thread-local stack.
pub(crate) fn push_stdout_redirect(target: RedirectTarget) {
    STDOUT_REDIRECT_STACK.with(|stack| stack.borrow_mut().push(target));
}

/// Pops the most recent stdout redirection target.
pub(crate) fn pop_stdout_redirect() -> Option<RedirectTarget> {
    STDOUT_REDIRECT_STACK.with(|stack| stack.borrow_mut().pop())
}

/// Returns the current stdout redirection target, if any.
#[must_use]
pub(crate) fn current_stdout_redirect() -> Option<RedirectTarget> {
    STDOUT_REDIRECT_STACK.with(|stack| stack.borrow().last().copied())
}

/// Pushes a stderr redirection target onto the thread-local stack.
pub(crate) fn push_stderr_redirect(target: RedirectTarget) {
    STDERR_REDIRECT_STACK.with(|stack| stack.borrow_mut().push(target));
}

/// Pops the most recent stderr redirection target.
pub(crate) fn pop_stderr_redirect() -> Option<RedirectTarget> {
    STDERR_REDIRECT_STACK.with(|stack| stack.borrow_mut().pop())
}

/// Returns the current stderr redirection target, if any.
#[must_use]
pub(crate) fn current_stderr_redirect() -> Option<RedirectTarget> {
    STDERR_REDIRECT_STACK.with(|stack| stack.borrow().last().copied())
}

impl PrintWriter for StdPrint {
    fn stdout_write(&mut self, output: Cow<'_, str>) -> Result<(), Exception> {
        STDOUT_BUFFER.with(|buffer| buffer.borrow_mut().push_str(&output));
        Ok(())
    }

    fn stdout_push(&mut self, end: char) -> Result<(), Exception> {
        STDOUT_BUFFER.with(|buffer| buffer.borrow_mut().push(end));
        Ok(())
    }
}

impl Drop for StdPrint {
    fn drop(&mut self) {
        STDOUT_BUFFER.with(|buffer| {
            let mut buffer = buffer.borrow_mut();
            if buffer.is_empty() {
                return;
            }
            let _ = io::stdout().write_all(buffer.as_bytes());
            let _ = io::stdout().flush();
            buffer.clear();
        });
    }
}

/// A `PrintWriter` that collects all output into a string.
///
/// Uses interior mutability via `RefCell` to allow collecting output
/// while being passed as a shared reference through the execution stack.
///
/// Useful for testing or capturing print output programmatically.
#[derive(Debug, Default)]
pub struct CollectStringPrint(String);

impl CollectStringPrint {
    /// Creates a new empty `CollectStringPrint`.
    #[must_use]
    pub fn new() -> Self {
        Self(String::new())
    }

    /// Returns the collected output as a string slice.
    ///
    /// # Panics
    /// Panics if the internal RefCell is currently borrowed mutably.
    #[must_use]
    pub fn output(&self) -> &str {
        self.0.as_str()
    }

    /// Consumes the writer and returns the collected output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.0
    }
}

impl PrintWriter for CollectStringPrint {
    fn stdout_write(&mut self, output: Cow<'_, str>) -> Result<(), Exception> {
        self.0.push_str(&output);
        Ok(())
    }

    fn stdout_push(&mut self, end: char) -> Result<(), Exception> {
        self.0.push(end);
        Ok(())
    }
}

/// `PrintWriter` that ignores all output.
///
/// Useful for suppressing print output during testing or benchmarking.
#[derive(Debug, Default)]
pub struct NoPrint;

impl PrintWriter for NoPrint {
    fn stdout_write(&mut self, _output: Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }

    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}
