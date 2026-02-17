//! Implementation of the `time` module.
//!
//! This module provides a CPython-compatible surface for core time functions
//! used by parity tests (`time`, `time_ns`, `monotonic`, `perf_counter`,
//! `process_time`, `gmtime/localtime`, formatting helpers, and constants).

use std::{
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc, Weekday};
use smallvec::smallvec;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Module, NamedTuple, NamedTupleFactory, OurosIter, PyTrait, Str, allocate_tuple},
    value::{EitherStr, Value},
};

static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Time module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum TimeFunctions {
    /// `time.time()`
    Time,
    /// `time.time_ns()`
    TimeNs,
    /// `time.monotonic()`
    Monotonic,
    /// `time.monotonic_ns()`
    MonotonicNs,
    /// `time.perf_counter()`
    PerfCounter,
    /// `time.perf_counter_ns()`
    PerfCounterNs,
    /// `time.process_time()`
    ProcessTime,
    /// `time.process_time_ns()`
    ProcessTimeNs,
    /// `time.thread_time()`
    ThreadTime,
    /// `time.thread_time_ns()`
    ThreadTimeNs,
    /// `time.sleep()`
    Sleep,
    /// `time.gmtime()`
    Gmtime,
    /// `time.localtime()`
    Localtime,
    /// `time.mktime()`
    Mktime,
    /// `time.asctime()`
    Asctime,
    /// `time.ctime()`
    Ctime,
    /// `time.strftime()`
    Strftime,
    /// `time.strptime()`
    Strptime,
    /// `time.get_clock_info()`
    GetClockInfo,
}

/// Creates the `time` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Time);

    // Callable functions.
    set_time_function(&mut module, StaticStrings::Time, TimeFunctions::Time, heap, interns);
    set_time_function(&mut module, StaticStrings::TimeNs, TimeFunctions::TimeNs, heap, interns);
    set_time_function(
        &mut module,
        StaticStrings::Monotonic,
        TimeFunctions::Monotonic,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::MonotonicNs,
        TimeFunctions::MonotonicNs,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::PerfCounter,
        TimeFunctions::PerfCounter,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::PerfCounterNs,
        TimeFunctions::PerfCounterNs,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::ProcessTime,
        TimeFunctions::ProcessTime,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::ProcessTimeNs,
        TimeFunctions::ProcessTimeNs,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::ThreadTime,
        TimeFunctions::ThreadTime,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::ThreadTimeNs,
        TimeFunctions::ThreadTimeNs,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::AioSleep,
        TimeFunctions::Sleep,
        heap,
        interns,
    );
    set_time_function(&mut module, StaticStrings::Gmtime, TimeFunctions::Gmtime, heap, interns);
    set_time_function(
        &mut module,
        StaticStrings::Localtime,
        TimeFunctions::Localtime,
        heap,
        interns,
    );
    set_time_function(&mut module, StaticStrings::Mktime, TimeFunctions::Mktime, heap, interns);
    set_time_function(
        &mut module,
        StaticStrings::Asctime,
        TimeFunctions::Asctime,
        heap,
        interns,
    );
    set_time_function(&mut module, StaticStrings::Ctime, TimeFunctions::Ctime, heap, interns);
    set_time_function(
        &mut module,
        StaticStrings::Strftime,
        TimeFunctions::Strftime,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::Strptime,
        TimeFunctions::Strptime,
        heap,
        interns,
    );
    set_time_function(
        &mut module,
        StaticStrings::GetClockInfo,
        TimeFunctions::GetClockInfo,
        heap,
        interns,
    );

    // struct_time type-like callable.
    let struct_time_factory = NamedTupleFactory::new_with_options(
        "time.struct_time".to_owned(),
        struct_time_field_names(),
        Vec::new(),
        "time".to_owned(),
    )
    .with_single_positional_iterable_constructor();
    let struct_time_id = heap.allocate(HeapData::NamedTupleFactory(struct_time_factory))?;
    module.set_attr(StaticStrings::StructTime, Value::Ref(struct_time_id), heap, interns);

    // Basic timezone constants.
    let local_offset = Local::now().offset().local_minus_utc();
    module.set_attr(
        StaticStrings::Timezone,
        Value::Int(i64::from(-local_offset)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Altzone,
        Value::Int(i64::from(-local_offset)),
        heap,
        interns,
    );
    module.set_attr(StaticStrings::Daylight, Value::Int(0), heap, interns);

    let tz_name = Local::now().format("%Z").to_string();
    let tz_std_id = heap.allocate(HeapData::Str(Str::from(tz_name.clone())))?;
    let tz_dst_id = heap.allocate(HeapData::Str(Str::from(tz_name)))?;
    let tz_tuple = allocate_tuple(smallvec![Value::Ref(tz_std_id), Value::Ref(tz_dst_id)], heap)?;
    module.set_attr(StaticStrings::Tzname, tz_tuple, heap, interns);

    // Clock constants (POSIX-style values).
    module.set_attr(StaticStrings::ClockRealtime, Value::Int(0), heap, interns);
    module.set_attr(StaticStrings::ClockMonotonic, Value::Int(1), heap, interns);
    module.set_attr(StaticStrings::ClockProcessCputimeId, Value::Int(2), heap, interns);
    module.set_attr(StaticStrings::ClockThreadCputimeId, Value::Int(3), heap, interns);
    module.set_attr(StaticStrings::ClockMonotonicRaw, Value::Int(4), heap, interns);

    heap.allocate(HeapData::Module(module))
}

fn set_time_function(
    module: &mut Module,
    name: StaticStrings,
    function: TimeFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    module.set_attr(
        name,
        Value::ModuleFunction(ModuleFunctions::Time(function)),
        heap,
        interns,
    );
}

/// Dispatches a call to a time module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TimeFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        TimeFunctions::Time => time_time(heap, args),
        TimeFunctions::TimeNs => time_time_ns(heap, args),
        TimeFunctions::Monotonic => time_monotonic(heap, args),
        TimeFunctions::MonotonicNs => time_monotonic_ns(heap, args),
        TimeFunctions::PerfCounter => time_perf_counter(heap, args),
        TimeFunctions::PerfCounterNs => time_perf_counter_ns(heap, args),
        TimeFunctions::ProcessTime => time_process_time(heap, args),
        TimeFunctions::ProcessTimeNs => time_process_time_ns(heap, args),
        TimeFunctions::ThreadTime => time_thread_time(heap, args),
        TimeFunctions::ThreadTimeNs => time_thread_time_ns(heap, args),
        TimeFunctions::Sleep => time_sleep(heap, args),
        TimeFunctions::Gmtime => time_gmtime(heap, args),
        TimeFunctions::Localtime => time_localtime(heap, args),
        TimeFunctions::Mktime => time_mktime(heap, args, interns),
        TimeFunctions::Asctime => time_asctime(heap, args, interns),
        TimeFunctions::Ctime => time_ctime(heap, args),
        TimeFunctions::Strftime => time_strftime(heap, args, interns),
        TimeFunctions::Strptime => time_strptime(heap, args, interns),
        TimeFunctions::GetClockInfo => time_get_clock_info(heap, args, interns),
    }?;
    Ok(AttrCallResult::Value(result))
}

fn time_time(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.time", heap)?;
    let now = unix_time_now()?;
    Ok(Value::Float(now.as_secs_f64()))
}

fn time_time_ns(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.time_ns", heap)?;
    let now = unix_time_now()?;
    Ok(Value::Int(duration_to_nanos_i64(now)))
}

fn time_monotonic(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.monotonic", heap)?;
    Ok(Value::Float(monotonic_elapsed().as_secs_f64()))
}

fn time_monotonic_ns(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.monotonic_ns", heap)?;
    Ok(Value::Int(duration_to_nanos_i64(monotonic_elapsed())))
}

fn time_perf_counter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.perf_counter", heap)?;
    Ok(Value::Float(monotonic_elapsed().as_secs_f64()))
}

fn time_perf_counter_ns(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.perf_counter_ns", heap)?;
    Ok(Value::Int(duration_to_nanos_i64(monotonic_elapsed())))
}

fn time_process_time(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.process_time", heap)?;
    Ok(Value::Float(process_elapsed().as_secs_f64()))
}

fn time_process_time_ns(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.process_time_ns", heap)?;
    Ok(Value::Int(duration_to_nanos_i64(process_elapsed())))
}

fn time_thread_time(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.thread_time", heap)?;
    Ok(Value::Float(process_elapsed().as_secs_f64()))
}

fn time_thread_time_ns(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.thread_time_ns", heap)?;
    Ok(Value::Int(duration_to_nanos_i64(process_elapsed())))
}

fn time_sleep(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    const MAX_SANDBOX_SLEEP_SECONDS: f64 = 0.01;

    let secs_value = args.get_one_arg("time.sleep", heap)?;
    let secs = value_to_f64(&secs_value, heap)?;
    secs_value.drop_with_heap(heap);

    if !secs.is_finite() || secs < 0.0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "sleep length must be non-negative and finite").into(),
        );
    }

    if secs > MAX_SANDBOX_SLEEP_SECONDS {
        return Err(SimpleException::new_msg(
            ExcType::RuntimeError,
            "time.sleep() is not supported in sandboxed environment",
        )
        .into());
    }

    std::thread::sleep(Duration::from_secs_f64(secs));
    Ok(Value::None)
}

fn time_gmtime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let timestamp = extract_optional_timestamp(args, "time.gmtime", heap)?;
    let parts = gmtime_parts(timestamp)?;
    allocate_struct_time(parts, heap)
}

fn time_localtime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let timestamp = extract_optional_timestamp(args, "time.localtime", heap)?;
    let parts = localtime_parts(timestamp)?;
    allocate_struct_time(parts, heap)
}

fn time_mktime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let time_tuple = args.get_one_arg("time.mktime", heap)?;
    let parts = extract_struct_time_parts(time_tuple.clone_with_heap(heap), heap, interns, "time.mktime")?;
    time_tuple.drop_with_heap(heap);

    let naive = naive_datetime_from_struct_parts(parts)?;
    let local_dt = match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => {
            return Err(SimpleException::new_msg(ExcType::ValueError, "mktime argument out of range").into());
        }
    };
    Ok(Value::Float(local_dt.timestamp() as f64))
}

fn time_asctime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let parts = match args.get_zero_one_arg("time.asctime", heap)? {
        Some(value) => {
            let parts = extract_struct_time_parts(value.clone_with_heap(heap), heap, interns, "time.asctime")?;
            value.drop_with_heap(heap);
            parts
        }
        None => localtime_parts(None)?,
    };

    let text = format_asctime(parts);
    let string_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(string_id))
}

fn time_ctime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let timestamp = extract_optional_timestamp(args, "time.ctime", heap)?;
    let parts = localtime_parts(timestamp)?;
    let text = format_asctime(parts);
    let string_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(string_id))
}

fn time_strftime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (format_value, maybe_time) = args.get_one_two_args("time.strftime", heap)?;
    let format_string = extract_string_arg(&format_value, heap, interns, "time.strftime")?;

    let naive = if let Some(time_value) = maybe_time {
        let parts = extract_struct_time_parts(time_value.clone_with_heap(heap), heap, interns, "time.strftime")?;
        time_value.drop_with_heap(heap);
        naive_datetime_from_struct_parts(parts)?
    } else {
        Local::now().naive_local()
    };

    format_value.drop_with_heap(heap);
    let formatted = naive.format(&format_string).to_string();
    let string_id = heap.allocate(HeapData::Str(Str::from(formatted)))?;
    Ok(Value::Ref(string_id))
}

fn time_strptime(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (input_value, format_value) = args.get_two_args("time.strptime", heap)?;
    let input = extract_string_arg(&input_value, heap, interns, "time.strptime")?;
    let format_string = extract_string_arg(&format_value, heap, interns, "time.strptime")?;
    input_value.drop_with_heap(heap);
    format_value.drop_with_heap(heap);

    let parsed = NaiveDateTime::parse_from_str(&input, &format_string)
        .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "time data does not match format"))?;

    let parts = [
        i64::from(parsed.year()),
        i64::from(parsed.month()),
        i64::from(parsed.day()),
        i64::from(parsed.hour()),
        i64::from(parsed.minute()),
        i64::from(parsed.second()),
        weekday_to_tm_wday(parsed.weekday()),
        i64::from(parsed.ordinal()),
        -1,
    ];

    allocate_struct_time(parts, heap)
}

fn time_get_clock_info(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let clock_value = args.get_one_arg("time.get_clock_info", heap)?;
    let clock_name = extract_string_arg(&clock_value, heap, interns, "time.get_clock_info")?;
    clock_value.drop_with_heap(heap);

    let (implementation, monotonic, adjustable, resolution) = match clock_name.as_str() {
        "time" => ("system time", false, true, 1e-9),
        "monotonic" => ("monotonic clock", true, false, 1e-9),
        "perf_counter" => ("performance counter", true, false, 1e-9),
        "process_time" => ("process time", true, false, 1e-9),
        "thread_time" => ("thread time", true, false, 1e-9),
        _ => {
            return Err(SimpleException::new_msg(ExcType::ValueError, "unknown clock").into());
        }
    };

    let implementation_id = heap.allocate(HeapData::Str(Str::from(implementation.to_owned())))?;
    let info = NamedTuple::new(
        "types.SimpleNamespace".to_owned(),
        vec![
            StaticStrings::Implementation.into(),
            StaticStrings::Monotonic.into(),
            StaticStrings::Adjustable.into(),
            StaticStrings::Resolution.into(),
        ],
        vec![
            Value::Ref(implementation_id),
            Value::Bool(monotonic),
            Value::Bool(adjustable),
            Value::Float(resolution),
        ],
    );
    let info_id = heap.allocate(HeapData::NamedTuple(info))?;
    Ok(Value::Ref(info_id))
}

fn unix_time_now() -> RunResult<Duration> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SimpleException::new_msg(ExcType::OSError, "system clock is before UNIX epoch").into())
}

fn monotonic_elapsed() -> Duration {
    MONOTONIC_START.get_or_init(Instant::now).elapsed()
}

fn process_elapsed() -> Duration {
    PROCESS_START.get_or_init(Instant::now).elapsed()
}

fn duration_to_nanos_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
}

fn extract_optional_timestamp(
    args: ArgValues,
    func_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Option<f64>> {
    match args.get_zero_one_arg(func_name, heap)? {
        Some(value) => {
            let timestamp = value_to_f64(&value, heap)?;
            value.drop_with_heap(heap);
            Ok(Some(timestamp))
        }
        None => Ok(None),
    }
}

fn gmtime_parts(timestamp: Option<f64>) -> RunResult<[i64; 9]> {
    let seconds = timestamp.unwrap_or_else(|| unix_time_now().map_or(0.0, |now| now.as_secs_f64()));
    let (secs, nanos) = split_timestamp(seconds)?;
    let dt = match Utc.timestamp_opt(secs, nanos) {
        LocalResult::Single(dt) => dt,
        _ => return Err(SimpleException::new_msg(ExcType::ValueError, "timestamp out of range for gmtime").into()),
    };

    Ok([
        i64::from(dt.year()),
        i64::from(dt.month()),
        i64::from(dt.day()),
        i64::from(dt.hour()),
        i64::from(dt.minute()),
        i64::from(dt.second()),
        weekday_to_tm_wday(dt.weekday()),
        i64::from(dt.ordinal()),
        0,
    ])
}

fn localtime_parts(timestamp: Option<f64>) -> RunResult<[i64; 9]> {
    let seconds = timestamp.unwrap_or_else(|| unix_time_now().map_or(0.0, |now| now.as_secs_f64()));
    let (secs, nanos) = split_timestamp(seconds)?;
    let dt = match Local.timestamp_opt(secs, nanos) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => {
            return Err(SimpleException::new_msg(ExcType::ValueError, "timestamp out of range for localtime").into());
        }
    };

    Ok([
        i64::from(dt.year()),
        i64::from(dt.month()),
        i64::from(dt.day()),
        i64::from(dt.hour()),
        i64::from(dt.minute()),
        i64::from(dt.second()),
        weekday_to_tm_wday(dt.weekday()),
        i64::from(dt.ordinal()),
        0,
    ])
}

fn split_timestamp(timestamp: f64) -> RunResult<(i64, u32)> {
    if !timestamp.is_finite() {
        return Err(SimpleException::new_msg(ExcType::OverflowError, "timestamp out of range").into());
    }

    let secs_floor = timestamp.floor();
    if secs_floor < i64::MIN as f64 || secs_floor > i64::MAX as f64 {
        return Err(SimpleException::new_msg(ExcType::OverflowError, "timestamp out of range").into());
    }

    let mut secs = secs_floor as i64;
    let frac = timestamp - secs_floor;
    let mut nanos = (frac * 1_000_000_000.0).round() as i64;

    if nanos >= 1_000_000_000 {
        secs = secs.saturating_add(1);
        nanos -= 1_000_000_000;
    }

    let nanos_u32 =
        u32::try_from(nanos).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "timestamp out of range"))?;
    Ok((secs, nanos_u32))
}

fn weekday_to_tm_wday(weekday: Weekday) -> i64 {
    i64::from(weekday.num_days_from_monday())
}

fn allocate_struct_time(parts: [i64; 9], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let values = parts.into_iter().map(Value::Int).collect();
    let struct_time = NamedTuple::new("time.struct_time".to_owned(), struct_time_field_names(), values);
    let struct_time_id = heap.allocate(HeapData::NamedTuple(struct_time))?;
    Ok(Value::Ref(struct_time_id))
}

fn struct_time_field_names() -> Vec<EitherStr> {
    vec![
        "tm_year".to_owned().into(),
        "tm_mon".to_owned().into(),
        "tm_mday".to_owned().into(),
        "tm_hour".to_owned().into(),
        "tm_min".to_owned().into(),
        "tm_sec".to_owned().into(),
        "tm_wday".to_owned().into(),
        "tm_yday".to_owned().into(),
        "tm_isdst".to_owned().into(),
    ]
}

fn extract_struct_time_parts(
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<[i64; 9]> {
    let mut result = [0_i64; 9];
    let iterable = value.clone_with_heap(heap);
    let mut iter = OurosIter::new(iterable, heap, interns)?;

    let mut items = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                items.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);

    if items.len() != 9 {
        items.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("{func_name}() argument must be a 9-item sequence"),
        )
        .into());
    }

    for (index, item) in items.into_iter().enumerate() {
        result[index] = value_to_i64(&item, heap)?;
        item.drop_with_heap(heap);
    }

    Ok(result)
}

fn naive_datetime_from_struct_parts(parts: [i64; 9]) -> RunResult<NaiveDateTime> {
    let year =
        i32::try_from(parts[0]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "year out of range"))?;
    let month =
        u32::try_from(parts[1]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "month out of range"))?;
    let day = u32::try_from(parts[2]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "day out of range"))?;
    let hour =
        u32::try_from(parts[3]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "hour out of range"))?;
    let minute =
        u32::try_from(parts[4]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "minute out of range"))?;
    let second =
        u32::try_from(parts[5]).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "second out of range"))?;

    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "invalid date"))?;
    date.and_hms_opt(hour, minute, second)
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "invalid time"))
        .map_err(Into::into)
}

fn format_asctime(parts: [i64; 9]) -> String {
    const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let weekday = usize::try_from(parts[6])
        .ok()
        .and_then(|idx| WEEKDAY_NAMES.get(idx))
        .copied()
        .unwrap_or("???");
    let month_index = parts[1].saturating_sub(1);
    let month = usize::try_from(month_index)
        .ok()
        .and_then(|idx| MONTH_NAMES.get(idx))
        .copied()
        .unwrap_or("???");

    format!(
        "{weekday} {month} {:>2} {:02}:{:02}:{:02} {:04}",
        parts[2], parts[3], parts[4], parts[5], parts[0]
    )
}

fn extract_string_arg(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(heap_id) => {
            if let HeapData::Str(s) = heap.get(*heap_id) {
                Ok(s.as_str().to_owned())
            } else {
                let type_name = value.py_type(heap);
                Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    format!("{func_name}() argument must be str, not {type_name}"),
                )
                .into())
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("{func_name}() argument must be str, not {type_name}"),
            )
            .into())
        }
    }
}

fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        _ => value.as_int(heap),
    }
}

fn value_to_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<f64> {
    match value {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(f64::from(*b)),
        Value::Ref(heap_id) => {
            if let HeapData::LongInt(li) = heap.get(*heap_id) {
                li.to_f64().ok_or_else(|| {
                    SimpleException::new_msg(ExcType::OverflowError, "int too large to convert to float").into()
                })
            } else {
                let type_name = value.py_type(heap);
                Err(
                    SimpleException::new_msg(ExcType::TypeError, format!("must be real number, not {type_name}"))
                        .into(),
                )
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(ExcType::TypeError, format!("must be real number, not {type_name}")).into())
        }
    }
}
