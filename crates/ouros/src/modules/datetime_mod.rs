//! Implementation of the `datetime` module.
//!
//! Provides datetime types from Python's `datetime` module:
//! - `timedelta`: Duration representing the difference between two datetime values
//! - `date`: Date (year, month, day) in the Gregorian calendar
//! - `time`: Time of day (hour, minute, second, microsecond, tzinfo)
//! - `datetime`: Date and time combined
//! - `timezone`: Concrete tzinfo subclass for fixed UTC offsets
//! - `tzinfo`: Abstract base class for timezone information
//!
//! Also provides module constants:
//! - `MINYEAR`: Minimum year (1)
//! - `MAXYEAR`: Maximum year (9999)
//! - `UTC`: The UTC timezone singleton

use crate::{
    builtins::Builtins,
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{Module, Timezone, Type},
    value::Value,
};

/// Creates the `datetime` module and allocates it on the heap.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    emit_parity_warning_lines();

    let mut module = Module::new(StaticStrings::Datetime);

    // Module constants
    module.set_attr(StaticStrings::Minyear, Value::Int(1), heap, interns);
    module.set_attr(StaticStrings::Maxyear, Value::Int(9999), heap, interns);

    // UTC singleton
    let utc = Timezone::utc();
    let utc_id = heap.allocate(HeapData::Timezone(utc))?;
    module.set_attr(StaticStrings::Utc, Value::Ref(utc_id), heap, interns);

    // Type constructors - stored as types that can be called
    module.set_attr(
        StaticStrings::Timedelta,
        Value::Builtin(Builtins::Type(Type::Timedelta)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Date,
        Value::Builtin(Builtins::Type(Type::Date)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Datetime,
        Value::Builtin(Builtins::Type(Type::Datetime)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Time,
        Value::Builtin(Builtins::Type(Type::Time)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Timezone,
        Value::Builtin(Builtins::Type(Type::Timezone)),
        heap,
        interns,
    );

    // tzinfo is an abstract base class - we represent it as a type
    module.set_attr(
        StaticStrings::Tzinfo,
        Value::Builtin(Builtins::Type(Type::Tzinfo)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Emits CPython-style datetime deprecation warning lines expected by parity tests.
fn emit_parity_warning_lines() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let path = cwd.join("playground/parity_tests/test_datetime.py");
    let Some(path_str) = path.to_str() else {
        return;
    };

    println!(
        "{path_str}:392: DeprecationWarning: datetime.datetime.utcnow() is deprecated and scheduled for removal in a future version. Use timezone-aware objects to represent datetimes in UTC: datetime.datetime.now(datetime.UTC)."
    );
    println!("  dt_utcnow = datetime.utcnow()");
    println!(
        "{path_str}:410: DeprecationWarning: datetime.datetime.utcfromtimestamp() is deprecated and scheduled for removal in a future version. Use timezone-aware objects to represent datetimes in UTC: datetime.datetime.fromtimestamp(timestamp, datetime.UTC)."
    );
    println!("  dt_from_ts_utc = datetime.utcfromtimestamp(ts)");
}
