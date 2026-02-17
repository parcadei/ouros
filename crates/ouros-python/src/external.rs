//! External function callback support.
//!
//! Allows Python code running in Ouros to call back to host Python functions.
//! External functions are registered by name and called when Ouros execution
//! reaches a call to that function.

use ::ouros::{ExternalResult, Object};
use pyo3::{
    exceptions::PyKeyError,
    prelude::*,
    types::{PyDict, PyTuple},
};

use crate::{
    convert::{ouros_to_py, py_to_ouros},
    exceptions::exc_py_to_ouros,
};

/// Registry that maps external function names to Python callables.
///
/// Passed to the execution loop and used to dispatch calls when Ouros
/// execution pauses at an external function.
pub struct ExternalFunctionRegistry<'py> {
    py: Python<'py>,
    functions: &'py Bound<'py, PyDict>,
    dc_registry: &'py Bound<'py, PyDict>,
}

impl<'py> ExternalFunctionRegistry<'py> {
    /// Creates a new registry from a Python dict of `name -> callable`.
    pub fn new(py: Python<'py>, functions: &'py Bound<'py, PyDict>, dc_registry: &'py Bound<'py, PyDict>) -> Self {
        Self {
            py,
            functions,
            dc_registry,
        }
    }

    /// Calls an external function by name with Ouros arguments.
    ///
    /// Converts args/kwargs from Ouros format, calls the Python callable
    /// with unpacked `*args, **kwargs`, and converts the result back to Ouros format.
    ///
    /// If the Python function raises an exception, it's converted to a Ouros
    /// exception that will be raised inside Ouros execution.
    pub fn call(&self, function_name: &str, args: &[Object], kwargs: &[(Object, Object)]) -> ExternalResult {
        match self.call_inner(function_name, args, kwargs) {
            Ok(result) => ExternalResult::Return(result),
            Err(err) => ExternalResult::Error(exc_py_to_ouros(self.py, &err)),
        }
    }

    /// Inner implementation that returns `PyResult` for error handling.
    fn call_inner(&self, function_name: &str, args: &[Object], kwargs: &[(Object, Object)]) -> PyResult<Object> {
        // Look up the callable
        let callable = self
            .functions
            .get_item(function_name)?
            .ok_or_else(|| PyKeyError::new_err(format!("External function '{function_name}' not found")))?;

        // Convert positional arguments to Python objects
        let py_args: PyResult<Vec<Py<PyAny>>> = args
            .iter()
            .map(|arg| ouros_to_py(self.py, arg, self.dc_registry))
            .collect();
        let py_args_tuple = PyTuple::new(self.py, py_args?)?;

        // Convert keyword arguments to Python dict
        let py_kwargs = PyDict::new(self.py);
        for (key, value) in kwargs {
            // Keys in kwargs should be strings
            let py_key = ouros_to_py(self.py, key, self.dc_registry)?;
            let py_value = ouros_to_py(self.py, value, self.dc_registry)?;
            py_kwargs.set_item(py_key, py_value)?;
        }

        // Call the function with unpacked *args, **kwargs
        let result = if py_kwargs.is_empty() {
            callable.call1(&py_args_tuple)?
        } else {
            callable.call(&py_args_tuple, Some(&py_kwargs))?
        };

        // Convert result back to Ouros format
        py_to_ouros(&result)
    }
}
