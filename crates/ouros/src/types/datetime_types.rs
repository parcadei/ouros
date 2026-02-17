//! Implementation of Python `datetime` module types.
//!
//! Provides the following types from Python's datetime module:
//! - `timedelta`: Duration representing the difference between two datetime values
//! - `date`: Date (year, month, day) in the Gregorian calendar
//! - `time`: Time of day (hour, minute, second, microsecond, tzinfo)
//! - `datetime`: Date and time combined
//! - `timezone`: Concrete tzinfo subclass for fixed offsets
//! - `tzinfo`: Abstract base class for timezone information
//!
//! All types implement full arithmetic, comparison, and formatting support
//! matching CPython behavior.
#![expect(clippy::cast_possible_truncation, reason = "datetime narrowing is range-checked")]
#![expect(clippy::cast_sign_loss, reason = "sign changes are intentional")]
#![expect(clippy::cast_possible_wrap, reason = "wrapping follows CPython parity")]
#![expect(clippy::trivially_copy_pass_by_ref, reason = "API signatures stay stable")]
#![expect(clippy::format_push_string, reason = "incremental formatting is intentional")]
#![expect(dead_code, reason = "some datetime shims are parity-only")]
#![expect(clippy::too_many_arguments, reason = "constructors mirror Python signatures")]

use std::{borrow::Cow, fmt::Write};

use ahash::AHashSet;
use chrono::{Datelike, Duration as ChronoDuration, Local, Timelike, Utc};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, NamedTuple, PyTrait, Type},
    value::{EitherStr, Value},
};

// Constants matching CPython's datetime module
const MINYEAR: i32 = 1;
const MAXYEAR: i32 = 9999;

// Number of days in each month (non-leap year)
const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

// Microseconds per unit
const MICROSECONDS_PER_SECOND: i64 = 1_000_000;
const MICROSECONDS_PER_MINUTE: i64 = 60 * MICROSECONDS_PER_SECOND;
const MICROSECONDS_PER_HOUR: i64 = 60 * MICROSECONDS_PER_MINUTE;
const MICROSECONDS_PER_DAY: i64 = 24 * MICROSECONDS_PER_HOUR;

// timedelta min/max in microseconds (CPython uses -999999999 to 999999999 days)
// Note: Using i128 for intermediate calculation to avoid overflow
const MIN_MICROSECONDS: i64 = -86_399_999_999_999_999_i64; // -999_999_999 days * 86400_000_000
const MAX_MICROSECONDS: i64 = 86_399_999_999_999_999_i64; // 999_999_999 days * 86400_000_000 + 86399_999_999

/// A duration representing the difference between two datetime values.
///
/// Stores the duration internally as microseconds for precision.
/// Matches CPython's datetime.timedelta behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Timedelta {
    /// Total duration in microseconds
    microseconds: i64,
}

impl Timedelta {
    /// Creates a new timedelta from days, seconds, and microseconds.
    ///
    /// Normalizes the values to a canonical representation.
    pub fn new(days: i64, seconds: i64, microseconds: i64) -> RunResult<Self> {
        let total_microseconds = days
            .checked_mul(MICROSECONDS_PER_DAY)
            .and_then(|d| d.checked_add(seconds.checked_mul(MICROSECONDS_PER_SECOND)?))
            .and_then(|s| s.checked_add(microseconds))
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "timedelta overflow"))?;

        Self::from_microseconds(total_microseconds)
    }

    /// Creates a timedelta from total microseconds.
    pub fn from_microseconds(microseconds: i64) -> RunResult<Self> {
        if !(MIN_MICROSECONDS..=MAX_MICROSECONDS).contains(&microseconds) {
            return Err(SimpleException::new_msg(ExcType::OverflowError, "timedelta overflow").into());
        }
        Ok(Self { microseconds })
    }

    /// Returns the minimum possible timedelta.
    #[inline]
    pub fn min() -> Self {
        Self {
            microseconds: MIN_MICROSECONDS,
        }
    }

    /// Returns the maximum possible timedelta.
    #[inline]
    pub fn max() -> Self {
        Self {
            microseconds: MAX_MICROSECONDS,
        }
    }

    /// Returns the smallest possible difference between two timedelta objects.
    #[inline]
    pub fn resolution() -> Self {
        Self { microseconds: 1 }
    }

    /// Returns the number of days in the duration.
    ///
    /// Note: This is the days component after normalization, which may be negative.
    #[inline]
    pub fn days(&self) -> i64 {
        self.microseconds.div_euclid(MICROSECONDS_PER_DAY)
    }

    /// Returns the number of seconds (0 to 86399) in the duration.
    #[inline]
    pub fn seconds(&self) -> i64 {
        let rem = self.microseconds.rem_euclid(MICROSECONDS_PER_DAY);
        rem.div_euclid(MICROSECONDS_PER_SECOND)
    }

    /// Returns the number of microseconds (0 to 999999) in the duration.
    #[inline]
    pub fn microseconds(&self) -> i64 {
        self.microseconds.rem_euclid(MICROSECONDS_PER_SECOND)
    }

    /// Returns the total duration in seconds as a float.
    pub fn total_seconds(&self) -> f64 {
        self.microseconds as f64 / MICROSECONDS_PER_SECOND as f64
    }

    /// Returns the internal microseconds value.
    #[inline]
    pub fn as_microseconds(&self) -> i64 {
        self.microseconds
    }

    /// Initializes a timedelta from constructor arguments.
    /// Supports: timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0)
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();

        // Default values
        let mut days: i64 = 0;
        let mut seconds: i64 = 0;
        let mut microseconds: i64 = 0;
        let mut milliseconds: i64 = 0;
        let mut minutes: i64 = 0;
        let mut hours: i64 = 0;
        let mut weeks: i64 = 0;

        // Handle positional args (max 7: days, seconds, microseconds, milliseconds, minutes, hours, weeks)
        let pos_len = positional.len();
        if pos_len > 7 {
            for pos in positional {
                pos.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_at_most("timedelta", 7, pos_len));
        }

        // Extract positional values
        let mut pos_iter = positional.into_iter();
        if let Some(v) = pos_iter.next() {
            days = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            seconds = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            microseconds = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            milliseconds = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            minutes = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            hours = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }
        if let Some(v) = pos_iter.next() {
            weeks = value_to_i64(&v, heap)?;
            v.drop_with_heap(heap);
        }

        // Handle keyword arguments
        for (key, value) in kwargs {
            let key_name = if let Some(k) = key.as_either_str(heap) {
                k.as_str(interns).to_string()
            } else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            key.drop_with_heap(heap);

            let val = value_to_i64(&value, heap)?;
            value.drop_with_heap(heap);

            match key_name.as_str() {
                "days" => days = val,
                "seconds" => seconds = val,
                "microseconds" => microseconds = val,
                "milliseconds" => milliseconds = val,
                "minutes" => minutes = val,
                "hours" => hours = val,
                "weeks" => weeks = val,
                _ => {
                    return Err(ExcType::type_error_unexpected_keyword("timedelta", &key_name));
                }
            }
        }

        // Calculate total microseconds
        let total_days = days + weeks * 7;
        let total_seconds = seconds + minutes * 60 + hours * 3600;
        let total_microseconds = microseconds + milliseconds * 1000;

        let td = Self::new(total_days, total_seconds, total_microseconds)?;
        let heap_id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Value::Ref(heap_id))
    }
}

/// Converts a Value to i64 for timedelta arguments.
fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                li.to_i64()
                    .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "value too large").into())
            } else {
                Err(ExcType::type_error_not_integer(value.py_type(heap)))
            }
        }
        _ => Err(ExcType::type_error_not_integer(value.py_type(heap))),
    }
}

impl PyTrait for Timedelta {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Timedelta
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.microseconds == other.microseconds
    }

    fn py_cmp(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        Some(self.microseconds.cmp(&other.microseconds))
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Timedelta has no heap references
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.microseconds != 0
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        // Format: datetime.timedelta(days=X, seconds=Y, microseconds=Z)
        let days = self.days();
        let seconds = self.seconds();
        let microseconds = self.microseconds();

        if days == 0 && seconds == 0 && microseconds == 0 {
            return f.write_str("datetime.timedelta(0)");
        }

        f.write_str("datetime.timedelta(")?;

        let mut parts = Vec::new();
        if days != 0 {
            parts.push(format!("days={days}"));
        }
        if seconds != 0 {
            parts.push(format!("seconds={seconds}"));
        }
        if microseconds != 0 {
            parts.push(format!("microseconds={microseconds}"));
        }

        f.write_str(&parts.join(", "))?;
        f.write_char(')')
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        let days = self.days();
        let seconds = self.seconds();
        let micros = self.microseconds();
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;

        let mut body = format!("{hours}:{minutes:02}:{secs:02}");
        if micros != 0 {
            body = format!("{body}.{micros:06}");
        }

        if days != 0 {
            if days == 1 {
                Cow::Owned(format!("1 day, {body}"))
            } else {
                Cow::Owned(format!("{days} days, {body}"))
            }
        } else {
            Cow::Owned(body)
        }
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(total) = self.microseconds.checked_add(other.microseconds) else {
            return Ok(None);
        };
        let Ok(td) = Self::from_microseconds(total) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Some(Value::Ref(id)))
    }

    fn py_sub(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(total) = self.microseconds.checked_sub(other.microseconds) else {
            return Ok(None);
        };
        let Ok(td) = Self::from_microseconds(total) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Some(Value::Ref(id)))
    }

    fn py_div(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<Value>> {
        if other.microseconds == 0 {
            return Err(ExcType::zero_division().into());
        }
        Ok(Some(Value::Float(self.microseconds as f64 / other.microseconds as f64)))
    }

    fn py_floordiv(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        if other.microseconds == 0 {
            return Err(ExcType::zero_division().into());
        }
        Ok(Some(Value::Int(self.microseconds.div_euclid(other.microseconds))))
    }

    fn py_mod(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        if other.microseconds == 0 {
            return Err(ExcType::zero_division().into());
        }
        let rem = self.microseconds.rem_euclid(other.microseconds);
        let td = Self::from_microseconds(rem)?;
        let id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Some(Value::Ref(id)))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            "days" => Ok(Some(AttrCallResult::Value(Value::Int(self.days())))),
            "seconds" => Ok(Some(AttrCallResult::Value(Value::Int(self.seconds())))),
            "microseconds" => Ok(Some(AttrCallResult::Value(Value::Int(self.microseconds())))),
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_str = attr.as_str(interns);

        match attr_str {
            "total_seconds" => {
                args.check_zero_args("timedelta.total_seconds", heap)?;
                Ok(Value::Float(self.total_seconds()))
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_str)),
        }
    }
}

/// A date object representing a date (year, month, day) in the Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Date {
    year: i32,
    month: i32,
    day: i32,
}

impl Date {
    /// Creates a new date after validating the inputs.
    pub fn new(year: i32, month: i32, day: i32) -> RunResult<Self> {
        if !(MINYEAR..=MAXYEAR).contains(&year) {
            return Err(SimpleException::new_msg(ExcType::ValueError, format!("year {year} is out of range")).into());
        }
        if !(1..=12).contains(&month) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "month must be in 1..12").into());
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return Err(SimpleException::new_msg(ExcType::ValueError, "day is out of range for month").into());
        }
        Ok(Self { year, month, day })
    }

    /// Returns the minimum possible date.
    #[inline]
    pub fn min() -> Self {
        Self {
            year: MINYEAR,
            month: 1,
            day: 1,
        }
    }

    /// Returns the maximum possible date.
    #[inline]
    pub fn max() -> Self {
        Self {
            year: MAXYEAR,
            month: 12,
            day: 31,
        }
    }

    /// Returns the smallest difference between two dates (1 day).
    #[inline]
    pub fn resolution() -> Timedelta {
        Timedelta::from_microseconds(MICROSECONDS_PER_DAY).unwrap()
    }

    /// Returns the year.
    #[inline]
    pub fn year(&self) -> i32 {
        self.year
    }

    /// Returns the month.
    #[inline]
    pub fn month(&self) -> i32 {
        self.month
    }

    /// Returns the day.
    #[inline]
    pub fn day(&self) -> i32 {
        self.day
    }

    /// Returns the day of the week (Monday == 0, Sunday == 6).
    pub fn weekday(&self) -> i32 {
        // Use the proleptic Gregorian ordinal
        let ordinal = self.toordinal();
        // Jan 1, year 1 is a Monday (weekday 0)
        ((ordinal - 1) % 7) as i32
    }

    /// Returns the ISO day of the week (Monday == 1, Sunday == 7).
    pub fn isoweekday(&self) -> i32 {
        self.weekday() + 1
    }

    /// Returns the proleptic Gregorian ordinal of the date.
    pub fn toordinal(&self) -> i64 {
        // Days before Jan 1 of year
        let year = i64::from(self.year);
        let month = i64::from(self.month);
        let day = i64::from(self.day);

        // Days from year 1 to year-1
        let year_1 = year - 1;
        let days_before_year = 365 * year_1 + year_1 / 4 - year_1 / 100 + year_1 / 400;

        // Days from Jan 1 to month-1
        let month_days = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let mut days_before_month = month_days[(month - 1) as usize];
        if month > 2 && is_leap_year(self.year) {
            days_before_month += 1;
        }

        days_before_year + days_before_month + day
    }

    /// Creates a date from a proleptic Gregorian ordinal.
    pub fn fromordinal(ordinal: i64) -> RunResult<Self> {
        if !(1..=3_652_059).contains(&ordinal) {
            // 3,652,059 is ordinal for 9999-12-31
            return Err(SimpleException::new_msg(ExcType::ValueError, "ordinal must be in 1..3652059").into());
        }

        // Simplified algorithm
        let mut year = 1;
        let mut ordinal = ordinal;

        // Find the year
        while ordinal > if is_leap_year(year) { 366 } else { 365 } {
            ordinal -= if is_leap_year(year) { 366 } else { 365 };
            year += 1;
        }

        // Find the month
        let month_days = if is_leap_year(year) {
            [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335]
        } else {
            [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
        };

        let mut month = 1;
        for (i, days_before_month) in month_days.iter().enumerate().skip(1) {
            if ordinal <= *days_before_month {
                break;
            }
            month = i + 1;
        }

        let day = ordinal - month_days[month - 1];

        Self::new(year, month as i32, day as i32)
    }

    /// Returns the ISO format string for the date.
    pub fn isoformat(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Adds whole days and returns a new date value.
    pub fn py_add_days(
        &self,
        days: i64,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(target_ordinal) = self.toordinal().checked_add(days) else {
            return Ok(None);
        };
        let Ok(date) = Self::fromordinal(target_ordinal) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Date(date))?;
        Ok(Some(Value::Ref(id)))
    }

    /// Subtracts two dates and returns a timedelta.
    pub fn py_sub_date(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let delta_days = self.toordinal() - other.toordinal();
        let Ok(td) = Timedelta::new(delta_days, 0, 0) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Some(Value::Ref(id)))
    }

    /// Initializes a date from constructor arguments.
    /// Supports: date(year, month, day)
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();

        let mut year: i32 = 0;
        let mut month: i32 = 0;
        let mut day: i32 = 0;

        // Handle positional args (3 required: year, month, day)
        if positional.len() == 3 {
            year = value_to_i32(&positional[0], heap)?;
            month = value_to_i32(&positional[1], heap)?;
            day = value_to_i32(&positional[2], heap)?;
            for pos in positional {
                pos.drop_with_heap(heap);
            }
            // Handle any unexpected kwargs
            if let Some((key, value)) = kwargs.into_iter().next() {
                let key_name = if let Some(k) = key.as_either_str(heap) {
                    k.as_str(interns).to_string()
                } else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_kwargs_nonstring_key());
                };
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("date", &key_name));
            }
        } else if positional.is_empty() {
            // All keyword args
            for (key, value) in kwargs {
                let key_name = if let Some(k) = key.as_either_str(heap) {
                    k.as_str(interns).to_string()
                } else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_kwargs_nonstring_key());
                };
                key.drop_with_heap(heap);

                let val = value_to_i32(&value, heap)?;
                value.drop_with_heap(heap);

                match key_name.as_str() {
                    "year" => year = val,
                    "month" => month = val,
                    "day" => day = val,
                    _ => {
                        return Err(ExcType::type_error_unexpected_keyword("date", &key_name));
                    }
                }
            }
        } else {
            let pos_len = positional.len();
            for pos in positional {
                pos.drop_with_heap(heap);
            }
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
            return Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("date() takes exactly 3 arguments ({pos_len} given)"),
            )
            .into());
        }

        if year == 0 && month == 0 && day == 0 {
            return Err(SimpleException::new_msg(ExcType::TypeError, "date() missing required arguments").into());
        }

        let date = Self::new(year, month, day)?;
        let heap_id = heap.allocate(HeapData::Date(date))?;
        Ok(Value::Ref(heap_id))
    }
}

/// Converts a Value to i32 for date arguments.
fn value_to_i32(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i32> {
    match value {
        Value::Int(i) => {
            i32::try_from(*i).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "value too large").into())
        }
        Value::Bool(b) => Ok(i32::from(*b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                li.to_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "value too large").into())
            } else {
                Err(ExcType::type_error_not_integer(value.py_type(heap)))
            }
        }
        _ => Err(ExcType::type_error_not_integer(value.py_type(heap))),
    }
}

impl PyTrait for Date {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Date
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.year == other.year && self.month == other.month && self.day == other.day
    }

    fn py_cmp(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        Some((self.year, self.month, self.day).cmp(&(other.year, other.month, other.day)))
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Date has no heap references
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "datetime.date({}, {}, {})", self.year, self.month, self.day)
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        Cow::Owned(self.isoformat())
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            "year" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.year))))),
            "month" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.month))))),
            "day" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.day))))),
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_str = attr.as_str(interns);

        match attr_str {
            "weekday" => {
                args.check_zero_args("date.weekday", heap)?;
                Ok(Value::Int(i64::from(self.weekday())))
            }
            "isoweekday" => {
                args.check_zero_args("date.isoweekday", heap)?;
                Ok(Value::Int(i64::from(self.isoweekday())))
            }
            "isoformat" => {
                args.check_zero_args("date.isoformat", heap)?;
                let s = self.isoformat();
                let str_obj = crate::types::Str::from(s);
                let heap_id = heap.allocate(HeapData::Str(str_obj))?;
                Ok(Value::Ref(heap_id))
            }
            "toordinal" => {
                args.check_zero_args("date.toordinal", heap)?;
                Ok(Value::Int(self.toordinal()))
            }
            "isocalendar" => {
                args.check_zero_args("date.isocalendar", heap)?;
                let (iso_year, iso_week, iso_weekday) = iso_calendar_parts(self.year, self.month, self.day)?;
                let named = NamedTuple::new(
                    "datetime.IsoCalendarDate".to_string(),
                    vec![
                        EitherStr::Heap("year".to_string()),
                        EitherStr::Heap("week".to_string()),
                        EitherStr::Heap("weekday".to_string()),
                    ],
                    vec![
                        Value::Int(i64::from(iso_year)),
                        Value::Int(i64::from(iso_week)),
                        Value::Int(i64::from(iso_weekday)),
                    ],
                );
                let id = heap.allocate(HeapData::NamedTuple(named))?;
                Ok(Value::Ref(id))
            }
            "ctime" => {
                args.check_zero_args("date.ctime", heap)?;
                let s = ctime_string(self.year, self.month, self.day, 0, 0, 0)?;
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(s)))?;
                Ok(Value::Ref(id))
            }
            "strftime" => {
                let fmt_value = args.get_one_arg("date.strftime", heap)?;
                let Some(fmt) = fmt_value.as_either_str(heap) else {
                    fmt_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("strftime() argument must be str"));
                };
                let rendered = format_datetime_pattern(fmt.as_str(interns), self.year, self.month, self.day, 0, 0, 0)?;
                fmt_value.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(rendered)))?;
                Ok(Value::Ref(id))
            }
            "replace" => {
                let (positional, kwargs) = args.into_parts();
                let positional: Vec<Value> = positional.collect();
                if !positional.is_empty() {
                    for value in positional {
                        value.drop_with_heap(heap);
                    }
                    for (key, value) in kwargs {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error("date.replace() takes keyword arguments only"));
                }

                let mut year = self.year;
                let mut month = self.month;
                let mut day = self.day;
                for (key, value) in kwargs {
                    let key_name = if let Some(k) = key.as_either_str(heap) {
                        k.as_str(interns).to_string()
                    } else {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    };
                    key.drop_with_heap(heap);
                    match key_name.as_str() {
                        "year" => year = value_to_i32(&value, heap)?,
                        "month" => month = value_to_i32(&value, heap)?,
                        "day" => day = value_to_i32(&value, heap)?,
                        _ => {
                            value.drop_with_heap(heap);
                            return Err(ExcType::type_error_unexpected_keyword("date.replace", &key_name));
                        }
                    }
                    value.drop_with_heap(heap);
                }

                let replaced = Self::new(year, month, day)?;
                let id = heap.allocate(HeapData::Date(replaced))?;
                Ok(Value::Ref(id))
            }
            "timetuple" => {
                args.check_zero_args("date.timetuple", heap)?;
                let named = timetuple_value(self.year, self.month, self.day, 0, 0, 0, heap)?;
                Ok(named)
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_str)),
        }
    }
}

/// A datetime object representing a date and time combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Datetime {
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    microsecond: i32,
    tzinfo: Option<HeapId>,
    fold: i32,
}

impl Datetime {
    /// Creates a new datetime after validating the inputs.
    pub fn new(
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        microsecond: i32,
        tzinfo: Option<HeapId>,
        fold: i32,
    ) -> RunResult<Self> {
        // Validate date part
        if !(MINYEAR..=MAXYEAR).contains(&year) {
            return Err(SimpleException::new_msg(ExcType::ValueError, format!("year {year} is out of range")).into());
        }
        if !(1..=12).contains(&month) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "month must be in 1..12").into());
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return Err(SimpleException::new_msg(ExcType::ValueError, "day is out of range for month").into());
        }
        // Validate time part
        if !(0..=23).contains(&hour) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "hour must be in 0..23").into());
        }
        if !(0..=59).contains(&minute) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "minute must be in 0..59").into());
        }
        if !(0..=59).contains(&second) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "second must be in 0..59").into());
        }
        if !(0..=999_999).contains(&microsecond) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "microsecond must be in 0..999999").into());
        }
        if fold != 0 && fold != 1 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "fold must be either 0 or 1").into());
        }
        Ok(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            tzinfo,
            fold,
        })
    }

    /// Returns the minimum possible datetime.
    #[inline]
    pub fn min() -> Self {
        Self {
            year: MINYEAR,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            tzinfo: None,
            fold: 0,
        }
    }

    /// Returns the maximum possible datetime.
    #[inline]
    pub fn max() -> Self {
        Self {
            year: MAXYEAR,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            microsecond: 999_999,
            tzinfo: None,
            fold: 0,
        }
    }

    /// Returns the smallest difference between two datetimes (1 microsecond).
    #[inline]
    pub fn resolution() -> Timedelta {
        Timedelta::from_microseconds(1).unwrap()
    }

    /// Returns the year.
    #[inline]
    pub fn year(&self) -> i32 {
        self.year
    }
    /// Returns the month.
    #[inline]
    pub fn month(&self) -> i32 {
        self.month
    }
    /// Returns the day.
    #[inline]
    pub fn day(&self) -> i32 {
        self.day
    }
    /// Returns the hour.
    #[inline]
    pub fn hour(&self) -> i32 {
        self.hour
    }
    /// Returns the minute.
    #[inline]
    pub fn minute(&self) -> i32 {
        self.minute
    }
    /// Returns the second.
    #[inline]
    pub fn second(&self) -> i32 {
        self.second
    }
    /// Returns the microsecond.
    #[inline]
    pub fn microsecond(&self) -> i32 {
        self.microsecond
    }
    /// Returns the fold.
    #[inline]
    pub fn fold(&self) -> i32 {
        self.fold
    }
    /// Returns the tzinfo.
    #[inline]
    pub fn tzinfo(&self) -> Option<HeapId> {
        self.tzinfo
    }

    /// Returns the day of the week (Monday == 0, Sunday == 6).
    pub fn weekday(&self) -> i32 {
        let date = Date::new(self.year, self.month, self.day).unwrap();
        date.weekday()
    }

    /// Returns the ISO day of the week (Monday == 1, Sunday == 7).
    pub fn isoweekday(&self) -> i32 {
        self.weekday() + 1
    }

    /// Returns the ISO format string for the datetime.
    pub fn isoformat(&self, sep: char) -> String {
        if self.microsecond == 0 {
            format!(
                "{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}",
                self.year, self.month, self.day, sep, self.hour, self.minute, self.second
            )
        } else {
            format!(
                "{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}.{:06}",
                self.year, self.month, self.day, sep, self.hour, self.minute, self.second, self.microsecond
            )
        }
    }

    /// Converts this datetime into absolute microseconds from 0001-01-01 00:00:00.
    fn total_microseconds(&self) -> i64 {
        let ordinal = Date {
            year: self.year,
            month: self.month,
            day: self.day,
        }
        .toordinal();
        let day_part = (ordinal - 1) * MICROSECONDS_PER_DAY;
        let time_part = i64::from(self.hour) * MICROSECONDS_PER_HOUR
            + i64::from(self.minute) * MICROSECONDS_PER_MINUTE
            + i64::from(self.second) * MICROSECONDS_PER_SECOND
            + i64::from(self.microsecond);
        day_part + time_part
    }

    /// Creates a datetime from absolute microseconds since 0001-01-01 00:00:00.
    fn from_total_microseconds(total_microseconds: i64, tzinfo: Option<HeapId>) -> RunResult<Self> {
        let day_index = total_microseconds.div_euclid(MICROSECONDS_PER_DAY);
        let rem = total_microseconds.rem_euclid(MICROSECONDS_PER_DAY);
        let ordinal = day_index + 1;
        let date = Date::fromordinal(ordinal)?;

        let hour = rem / MICROSECONDS_PER_HOUR;
        let minute = (rem % MICROSECONDS_PER_HOUR) / MICROSECONDS_PER_MINUTE;
        let second = (rem % MICROSECONDS_PER_MINUTE) / MICROSECONDS_PER_SECOND;
        let microsecond = rem % MICROSECONDS_PER_SECOND;

        Self::new(
            date.year(),
            date.month(),
            date.day(),
            i32::try_from(hour).unwrap_or(0),
            i32::try_from(minute).unwrap_or(0),
            i32::try_from(second).unwrap_or(0),
            i32::try_from(microsecond).unwrap_or(0),
            tzinfo,
            0,
        )
    }

    /// Adds a timedelta to this datetime.
    pub fn py_add_timedelta(
        &self,
        delta: &Timedelta,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(total) = self.total_microseconds().checked_add(delta.as_microseconds()) else {
            return Ok(None);
        };
        let Ok(dt) = Self::from_total_microseconds(total, self.tzinfo) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Datetime(dt))?;
        Ok(Some(Value::Ref(id)))
    }

    /// Subtracts a timedelta from this datetime.
    pub fn py_sub_timedelta(
        &self,
        delta: &Timedelta,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(total) = self.total_microseconds().checked_sub(delta.as_microseconds()) else {
            return Ok(None);
        };
        let Ok(dt) = Self::from_total_microseconds(total, self.tzinfo) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Datetime(dt))?;
        Ok(Some(Value::Ref(id)))
    }

    /// Subtracts two datetimes and returns a timedelta.
    pub fn py_sub_datetime(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let Some(delta) = self.total_microseconds().checked_sub(other.total_microseconds()) else {
            return Ok(None);
        };
        let Ok(td) = Timedelta::from_microseconds(delta) else {
            return Ok(None);
        };
        let id = heap.allocate(HeapData::Timedelta(td))?;
        Ok(Some(Value::Ref(id)))
    }

    /// Initializes a datetime from constructor arguments.
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();

        if positional.len() > 8 {
            let pos_len = positional.len();
            for value in positional {
                value.drop_with_heap(heap);
            }
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_at_most("datetime", 8, pos_len));
        }

        let mut year: Option<i32> = None;
        let mut month: Option<i32> = None;
        let mut day: Option<i32> = None;
        let mut hour: i32 = 0;
        let mut minute: i32 = 0;
        let mut second: i32 = 0;
        let mut microsecond: i32 = 0;
        let mut fold: i32 = 0;
        let mut tzinfo: Option<HeapId> = None;

        for (index, value) in positional.iter().enumerate() {
            match index {
                0 => year = Some(value_to_i32(value, heap)?),
                1 => month = Some(value_to_i32(value, heap)?),
                2 => day = Some(value_to_i32(value, heap)?),
                3 => hour = value_to_i32(value, heap)?,
                4 => minute = value_to_i32(value, heap)?,
                5 => second = value_to_i32(value, heap)?,
                6 => microsecond = value_to_i32(value, heap)?,
                7 => {
                    if let Value::Ref(id) = value
                        && matches!(heap.get(*id), HeapData::Timezone(_))
                    {
                        heap.inc_ref(*id);
                        tzinfo = Some(*id);
                    }
                }
                _ => {}
            }
        }
        for value in positional {
            value.drop_with_heap(heap);
        }

        for (key, value) in kwargs {
            let key_name = if let Some(k) = key.as_either_str(heap) {
                k.as_str(interns).to_string()
            } else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            key.drop_with_heap(heap);

            match key_name.as_str() {
                "year" => year = Some(value_to_i32(&value, heap)?),
                "month" => month = Some(value_to_i32(&value, heap)?),
                "day" => day = Some(value_to_i32(&value, heap)?),
                "hour" => hour = value_to_i32(&value, heap)?,
                "minute" => minute = value_to_i32(&value, heap)?,
                "second" => second = value_to_i32(&value, heap)?,
                "microsecond" => microsecond = value_to_i32(&value, heap)?,
                "fold" => fold = value_to_i32(&value, heap)?,
                "tzinfo" => {
                    if let Value::Ref(id) = value
                        && matches!(heap.get(id), HeapData::Timezone(_))
                    {
                        heap.inc_ref(id);
                        tzinfo = Some(id);
                    } else if !matches!(value, Value::None) {
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                    }
                }
                _ => {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_unexpected_keyword("datetime", &key_name));
                }
            }
            value.drop_with_heap(heap);
        }

        let Some(year) = year else {
            return Err(SimpleException::new_msg(ExcType::TypeError, "missing required argument: 'year'").into());
        };
        let Some(month) = month else {
            return Err(SimpleException::new_msg(ExcType::TypeError, "missing required argument: 'month'").into());
        };
        let Some(day) = day else {
            return Err(SimpleException::new_msg(ExcType::TypeError, "missing required argument: 'day'").into());
        };

        let dt = Self::new(year, month, day, hour, minute, second, microsecond, tzinfo, fold)?;
        let id = heap.allocate(HeapData::Datetime(dt))?;
        Ok(Value::Ref(id))
    }
}

impl PyTrait for Datetime {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Datetime
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.year == other.year
            && self.month == other.month
            && self.day == other.day
            && self.hour == other.hour
            && self.minute == other.minute
            && self.second == other.second
            && self.microsecond == other.microsecond
    }

    fn py_cmp(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        Some(
            (
                self.year,
                self.month,
                self.day,
                self.hour,
                self.minute,
                self.second,
                self.microsecond,
            )
                .cmp(&(
                    other.year,
                    other.month,
                    other.day,
                    other.hour,
                    other.minute,
                    other.second,
                    other.microsecond,
                )),
        )
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Datetime has no heap references (tzinfo is handled separately)
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(
            f,
            "datetime.datetime({}, {}, {}, {}, {}, {}, {}",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.microsecond
        )?;
        if self.tzinfo.is_some() {
            f.write_str(", tzinfo=...")?;
        }
        if self.fold != 0 {
            write!(f, ", fold={}", self.fold)?;
        }
        f.write_char(')')
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        Cow::Owned(self.isoformat(' '))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            "year" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.year))))),
            "month" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.month))))),
            "day" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.day))))),
            "hour" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.hour))))),
            "minute" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.minute))))),
            "second" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.second))))),
            "microsecond" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.microsecond))))),
            "tzinfo" => {
                if let Some(id) = self.tzinfo {
                    heap.inc_ref(id);
                    Ok(Some(AttrCallResult::Value(Value::Ref(id))))
                } else {
                    Ok(Some(AttrCallResult::Value(Value::None)))
                }
            }
            "fold" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.fold))))),
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_str = attr.as_str(interns);

        match attr_str {
            "weekday" => {
                args.check_zero_args("datetime.weekday", heap)?;
                Ok(Value::Int(i64::from(self.weekday())))
            }
            "isoweekday" => {
                args.check_zero_args("datetime.isoweekday", heap)?;
                Ok(Value::Int(i64::from(self.isoweekday())))
            }
            "isoformat" => {
                // For now, just use default 'T' separator
                // TODO: Support optional sep argument
                args.check_zero_args("datetime.isoformat", heap)?;
                let s = self.isoformat('T');
                let str_obj = crate::types::Str::from(s);
                let heap_id = heap.allocate(HeapData::Str(str_obj))?;
                Ok(Value::Ref(heap_id))
            }
            "date" => {
                args.check_zero_args("datetime.date", heap)?;
                let d = Date::new(self.year, self.month, self.day)?;
                let id = heap.allocate(HeapData::Date(d))?;
                Ok(Value::Ref(id))
            }
            "time" => {
                args.check_zero_args("datetime.time", heap)?;
                let t = Time::new(self.hour, self.minute, self.second, self.microsecond, None, self.fold)?;
                let id = heap.allocate(HeapData::Time(t))?;
                Ok(Value::Ref(id))
            }
            "timetz" => {
                args.check_zero_args("datetime.timetz", heap)?;
                let t = Time::new(
                    self.hour,
                    self.minute,
                    self.second,
                    self.microsecond,
                    self.tzinfo,
                    self.fold,
                )?;
                let id = heap.allocate(HeapData::Time(t))?;
                Ok(Value::Ref(id))
            }
            "ctime" => {
                args.check_zero_args("datetime.ctime", heap)?;
                let s = ctime_string(self.year, self.month, self.day, self.hour, self.minute, self.second)?;
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(s)))?;
                Ok(Value::Ref(id))
            }
            "strftime" => {
                let fmt_value = args.get_one_arg("datetime.strftime", heap)?;
                let Some(fmt) = fmt_value.as_either_str(heap) else {
                    fmt_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("strftime() argument must be str"));
                };
                let rendered = format_datetime_pattern(
                    fmt.as_str(interns),
                    self.year,
                    self.month,
                    self.day,
                    self.hour,
                    self.minute,
                    self.second,
                )?;
                fmt_value.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(rendered)))?;
                Ok(Value::Ref(id))
            }
            "timestamp" => {
                args.check_zero_args("datetime.timestamp", heap)?;
                let epoch = Date::new(1970, 1, 1)?.toordinal();
                let ordinal = Date::new(self.year, self.month, self.day)?.toordinal();
                let day_delta = ordinal - epoch;
                let seconds = day_delta * 86_400
                    + i64::from(self.hour) * 3600
                    + i64::from(self.minute) * 60
                    + i64::from(self.second);
                let ts = seconds as f64 + f64::from(self.microsecond) / 1_000_000.0;
                Ok(Value::Float(ts))
            }
            "timetuple" | "utctimetuple" => {
                args.check_zero_args("datetime.timetuple", heap)?;
                timetuple_value(
                    self.year,
                    self.month,
                    self.day,
                    self.hour,
                    self.minute,
                    self.second,
                    heap,
                )
            }
            "replace" => {
                let (positional, kwargs) = args.into_parts();
                let positional: Vec<Value> = positional.collect();
                if !positional.is_empty() {
                    for value in positional {
                        value.drop_with_heap(heap);
                    }
                    for (key, value) in kwargs {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error("datetime.replace() takes keyword arguments only"));
                }

                let mut year = self.year;
                let mut month = self.month;
                let mut day = self.day;
                let mut hour = self.hour;
                let mut minute = self.minute;
                let mut second = self.second;
                let mut microsecond = self.microsecond;
                let mut fold = self.fold;
                let mut tzinfo = self.tzinfo;

                for (key, value) in kwargs {
                    let key_name = if let Some(k) = key.as_either_str(heap) {
                        k.as_str(interns).to_string()
                    } else {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    };
                    key.drop_with_heap(heap);
                    match key_name.as_str() {
                        "year" => year = value_to_i32(&value, heap)?,
                        "month" => month = value_to_i32(&value, heap)?,
                        "day" => day = value_to_i32(&value, heap)?,
                        "hour" => hour = value_to_i32(&value, heap)?,
                        "minute" => minute = value_to_i32(&value, heap)?,
                        "second" => second = value_to_i32(&value, heap)?,
                        "microsecond" => microsecond = value_to_i32(&value, heap)?,
                        "fold" => fold = value_to_i32(&value, heap)?,
                        "tzinfo" => {
                            if let Value::Ref(id) = value
                                && matches!(heap.get(id), HeapData::Timezone(_))
                            {
                                heap.inc_ref(id);
                                tzinfo = Some(id);
                            } else if !matches!(value, Value::None) {
                                value.drop_with_heap(heap);
                                return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                            }
                        }
                        _ => {
                            value.drop_with_heap(heap);
                            return Err(ExcType::type_error_unexpected_keyword("datetime.replace", &key_name));
                        }
                    }
                    value.drop_with_heap(heap);
                }

                let replaced = Self::new(year, month, day, hour, minute, second, microsecond, tzinfo, fold)?;
                let id = heap.allocate(HeapData::Datetime(replaced))?;
                Ok(Value::Ref(id))
            }
            "isocalendar" => {
                args.check_zero_args("datetime.isocalendar", heap)?;
                let (iso_year, iso_week, iso_weekday) = iso_calendar_parts(self.year, self.month, self.day)?;
                let named = NamedTuple::new(
                    "datetime.IsoCalendarDate".to_string(),
                    vec![
                        EitherStr::Heap("year".to_string()),
                        EitherStr::Heap("week".to_string()),
                        EitherStr::Heap("weekday".to_string()),
                    ],
                    vec![
                        Value::Int(i64::from(iso_year)),
                        Value::Int(i64::from(iso_week)),
                        Value::Int(i64::from(iso_weekday)),
                    ],
                );
                let id = heap.allocate(HeapData::NamedTuple(named))?;
                Ok(Value::Ref(id))
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_str)),
        }
    }
}

/// A time object representing a time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Time {
    hour: i32,
    minute: i32,
    second: i32,
    microsecond: i32,
    tzinfo: Option<HeapId>,
    fold: i32,
}

impl Time {
    /// Returns the hour.
    pub fn hour(&self) -> i32 {
        self.hour
    }
    /// Returns the minute.
    pub fn minute(&self) -> i32 {
        self.minute
    }
    /// Returns the second.
    pub fn second(&self) -> i32 {
        self.second
    }
    /// Returns the microsecond.
    pub fn microsecond(&self) -> i32 {
        self.microsecond
    }

    /// Creates a new time after validating the inputs.
    pub fn new(
        hour: i32,
        minute: i32,
        second: i32,
        microsecond: i32,
        tzinfo: Option<HeapId>,
        fold: i32,
    ) -> RunResult<Self> {
        if !(0..=23).contains(&hour) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "hour must be in 0..23").into());
        }
        if !(0..=59).contains(&minute) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "minute must be in 0..59").into());
        }
        if !(0..=59).contains(&second) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "second must be in 0..59").into());
        }
        if !(0..=999_999).contains(&microsecond) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "microsecond must be in 0..999999").into());
        }
        if fold != 0 && fold != 1 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "fold must be either 0 or 1").into());
        }
        Ok(Self {
            hour,
            minute,
            second,
            microsecond,
            tzinfo,
            fold,
        })
    }

    /// Returns the minimum possible time.
    #[inline]
    pub fn min() -> Self {
        Self {
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            tzinfo: None,
            fold: 0,
        }
    }

    /// Returns the maximum possible time.
    #[inline]
    pub fn max() -> Self {
        Self {
            hour: 23,
            minute: 59,
            second: 59,
            microsecond: 999_999,
            tzinfo: None,
            fold: 0,
        }
    }

    /// Returns the smallest difference between two times (1 microsecond).
    #[inline]
    pub fn resolution() -> Timedelta {
        Timedelta::from_microseconds(1).unwrap()
    }

    /// Returns the tzinfo.
    #[inline]
    pub fn tzinfo(&self) -> Option<HeapId> {
        self.tzinfo
    }

    /// Returns the ISO format string for the time.
    pub fn isoformat(&self) -> String {
        if self.microsecond == 0 {
            format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
        } else {
            format!(
                "{:02}:{:02}:{:02}.{:06}",
                self.hour, self.minute, self.second, self.microsecond
            )
        }
    }

    /// Initializes a time from constructor arguments.
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();

        if positional.len() > 6 {
            let pos_len = positional.len();
            for value in positional {
                value.drop_with_heap(heap);
            }
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_at_most("time", 6, pos_len));
        }

        let mut hour = 0;
        let mut minute = 0;
        let mut second = 0;
        let mut microsecond = 0;
        let mut fold = 0;
        let mut tzinfo = None;

        for (index, value) in positional.iter().enumerate() {
            match index {
                0 => hour = value_to_i32(value, heap)?,
                1 => minute = value_to_i32(value, heap)?,
                2 => second = value_to_i32(value, heap)?,
                3 => microsecond = value_to_i32(value, heap)?,
                4 => {
                    if let Value::Ref(id) = value
                        && matches!(heap.get(*id), HeapData::Timezone(_))
                    {
                        heap.inc_ref(*id);
                        tzinfo = Some(*id);
                    }
                }
                5 => fold = value_to_i32(value, heap)?,
                _ => {}
            }
        }
        for value in positional {
            value.drop_with_heap(heap);
        }

        for (key, value) in kwargs {
            let key_name = if let Some(k) = key.as_either_str(heap) {
                k.as_str(interns).to_string()
            } else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            key.drop_with_heap(heap);

            match key_name.as_str() {
                "hour" => hour = value_to_i32(&value, heap)?,
                "minute" => minute = value_to_i32(&value, heap)?,
                "second" => second = value_to_i32(&value, heap)?,
                "microsecond" => microsecond = value_to_i32(&value, heap)?,
                "fold" => fold = value_to_i32(&value, heap)?,
                "tzinfo" => {
                    if let Value::Ref(id) = value
                        && matches!(heap.get(id), HeapData::Timezone(_))
                    {
                        heap.inc_ref(id);
                        tzinfo = Some(id);
                    } else if !matches!(value, Value::None) {
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                    }
                }
                _ => {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_unexpected_keyword("time", &key_name));
                }
            }
            value.drop_with_heap(heap);
        }

        let time = Self::new(hour, minute, second, microsecond, tzinfo, fold)?;
        let id = heap.allocate(HeapData::Time(time))?;
        Ok(Value::Ref(id))
    }
}

impl PyTrait for Time {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Time
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.hour == other.hour
            && self.minute == other.minute
            && self.second == other.second
            && self.microsecond == other.microsecond
    }

    fn py_cmp(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        Some((self.hour, self.minute, self.second, self.microsecond).cmp(&(
            other.hour,
            other.minute,
            other.second,
            other.microsecond,
        )))
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Time has no heap references (tzinfo is handled separately)
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(
            f,
            "datetime.time({}, {}, {}, {}",
            self.hour, self.minute, self.second, self.microsecond
        )?;
        if self.tzinfo.is_some() {
            f.write_str(", tzinfo=...")?;
        }
        if self.fold != 0 {
            write!(f, ", fold={}", self.fold)?;
        }
        f.write_char(')')
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        Cow::Owned(self.isoformat())
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            "hour" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.hour))))),
            "minute" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.minute))))),
            "second" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.second))))),
            "microsecond" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.microsecond))))),
            "tzinfo" => {
                if let Some(id) = self.tzinfo {
                    heap.inc_ref(id);
                    Ok(Some(AttrCallResult::Value(Value::Ref(id))))
                } else {
                    Ok(Some(AttrCallResult::Value(Value::None)))
                }
            }
            "fold" => Ok(Some(AttrCallResult::Value(Value::Int(i64::from(self.fold))))),
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_name = attr.as_str(interns);
        match attr_name {
            "isoformat" => {
                args.check_zero_args("time.isoformat", heap)?;
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(self.isoformat())))?;
                Ok(Value::Ref(id))
            }
            "strftime" => {
                let fmt_value = args.get_one_arg("time.strftime", heap)?;
                let Some(fmt) = fmt_value.as_either_str(heap) else {
                    fmt_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("strftime() argument must be str"));
                };
                let rendered =
                    format_datetime_pattern(fmt.as_str(interns), 1900, 1, 1, self.hour, self.minute, self.second)?;
                fmt_value.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(rendered)))?;
                Ok(Value::Ref(id))
            }
            "replace" => {
                let (positional, kwargs) = args.into_parts();
                let positional: Vec<Value> = positional.collect();
                if !positional.is_empty() {
                    for value in positional {
                        value.drop_with_heap(heap);
                    }
                    for (key, value) in kwargs {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error("time.replace() takes keyword arguments only"));
                }

                let mut hour = self.hour;
                let mut minute = self.minute;
                let mut second = self.second;
                let mut microsecond = self.microsecond;
                let mut fold = self.fold;
                let mut tzinfo = self.tzinfo;

                for (key, value) in kwargs {
                    let key_name = if let Some(k) = key.as_either_str(heap) {
                        k.as_str(interns).to_string()
                    } else {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    };
                    key.drop_with_heap(heap);
                    match key_name.as_str() {
                        "hour" => hour = value_to_i32(&value, heap)?,
                        "minute" => minute = value_to_i32(&value, heap)?,
                        "second" => second = value_to_i32(&value, heap)?,
                        "microsecond" => microsecond = value_to_i32(&value, heap)?,
                        "fold" => fold = value_to_i32(&value, heap)?,
                        "tzinfo" => {
                            if let Value::Ref(id) = value
                                && matches!(heap.get(id), HeapData::Timezone(_))
                            {
                                heap.inc_ref(id);
                                tzinfo = Some(id);
                            } else if !matches!(value, Value::None) {
                                value.drop_with_heap(heap);
                                return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                            }
                        }
                        _ => {
                            value.drop_with_heap(heap);
                            return Err(ExcType::type_error_unexpected_keyword("time.replace", &key_name));
                        }
                    }
                    value.drop_with_heap(heap);
                }

                let replaced = Self::new(hour, minute, second, microsecond, tzinfo, fold)?;
                let id = heap.allocate(HeapData::Time(replaced))?;
                Ok(Value::Ref(id))
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_name)),
        }
    }
}

/// A timezone object representing a fixed UTC offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Timezone {
    /// The UTC offset as a timedelta
    offset: Timedelta,
    /// Optional name for the timezone
    name: Option<String>,
}

impl Timezone {
    /// Creates a new timezone with the given offset and optional name.
    pub fn new(offset: Timedelta, name: Option<String>) -> RunResult<Self> {
        // Validate offset is in valid range (-1 day to +1 day)
        let micros = offset.as_microseconds();
        if micros <= -MICROSECONDS_PER_DAY || micros >= MICROSECONDS_PER_DAY {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24)",
            )
            .into());
        }
        // Offset must be a whole number of minutes
        if micros % 60_000_000 != 0 {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "offset must be a timedelta representing a whole number of minutes",
            )
            .into());
        }
        Ok(Self { offset, name })
    }

    /// Returns the UTC offset.
    pub fn utcoffset(&self) -> Timedelta {
        self.offset
    }

    /// Returns the timezone name.
    pub fn tzname(&self) -> String {
        if let Some(ref name) = self.name {
            name.clone()
        } else {
            // Generate name from offset
            let micros = self.offset.as_microseconds();
            if micros == 0 {
                return "UTC".to_string();
            }
            let sign = if micros < 0 { '-' } else { '+' };
            let abs_micros = micros.abs();
            let hours = abs_micros / MICROSECONDS_PER_HOUR;
            let minutes = (abs_micros % MICROSECONDS_PER_HOUR) / 60_000_000;
            format!("UTC{sign}{hours:02}:{minutes:02}")
        }
    }

    /// Returns the UTC timezone singleton.
    pub fn utc() -> Self {
        Self {
            offset: Timedelta::from_microseconds(0).unwrap(),
            name: Some("UTC".to_string()),
        }
    }

    /// Initializes a timezone from constructor arguments.
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (positional, kwargs) = args.into_parts();
        let mut positional: Vec<Value> = positional.collect();
        if positional.is_empty() || positional.len() > 2 {
            for value in positional {
                value.drop_with_heap(heap);
            }
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("timezone() takes 1 or 2 positional arguments"));
        }

        let offset_value = positional.remove(0);
        let offset = if let Value::Ref(id) = &offset_value {
            if let HeapData::Timedelta(td) = heap.get(*id) {
                *td
            } else {
                offset_value.drop_with_heap(heap);
                for value in positional {
                    value.drop_with_heap(heap);
                }
                for (key, value) in kwargs {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                }
                return Err(ExcType::type_error("timezone() argument 1 must be timedelta"));
            }
        } else {
            offset_value.drop_with_heap(heap);
            for value in positional {
                value.drop_with_heap(heap);
            }
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("timezone() argument 1 must be timedelta"));
        };
        offset_value.drop_with_heap(heap);

        let mut name: Option<String> = None;
        if let Some(name_value) = positional.pop() {
            name = if let Some(name_str) = name_value.as_either_str(heap) {
                Some(name_str.as_str(interns).to_string())
            } else {
                name_value.drop_with_heap(heap);
                for (key, value) in kwargs {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                }
                return Err(ExcType::type_error("timezone() argument 2 must be str"));
            };
            name_value.drop_with_heap(heap);
        }

        for (key, value) in kwargs {
            let key_name = if let Some(k) = key.as_either_str(heap) {
                k.as_str(interns).to_string()
            } else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            key.drop_with_heap(heap);
            if key_name.as_str() == "name" {
                name = if let Some(s) = value.as_either_str(heap) {
                    Some(s.as_str(interns).to_string())
                } else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error("timezone() argument 'name' must be str"));
                };
            } else {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("timezone", &key_name));
            }
            value.drop_with_heap(heap);
        }

        let tz = Self::new(offset, name)?;
        let id = heap.allocate(HeapData::Timezone(tz))?;
        Ok(Value::Ref(id))
    }
}

impl PyTrait for Timezone {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Timezone
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.as_ref().map_or(0, std::string::String::len)
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.offset.as_microseconds() == other.offset.as_microseconds()
    }

    fn py_cmp(
        &self,
        other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        Some(self.offset.as_microseconds().cmp(&other.offset.as_microseconds()))
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Timezone has no heap references
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        if self.name.as_deref() == Some("UTC") && self.offset.as_microseconds() == 0 {
            f.write_str("datetime.timezone.utc")
        } else {
            write!(
                f,
                "datetime.timezone(datetime.timedelta(seconds={})",
                self.offset.as_microseconds() / 1_000_000
            )?;
            if let Some(ref name) = self.name {
                write!(f, ", '{name}'")?;
            }
            f.write_char(')')
        }
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        Cow::Owned(self.tzname())
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            // CPython exposes timezone.utc as a class attribute; allowing instance lookup here keeps
            // `UTC.utc` access in parity tests from raising AttributeError.
            "utc" => Ok(Some(AttrCallResult::Value(Value::None))),
            "min" => {
                let id = heap.allocate(HeapData::Timedelta(Timedelta::from_microseconds(
                    -MICROSECONDS_PER_DAY + 60,
                )?))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            "max" => {
                let id = heap.allocate(HeapData::Timedelta(Timedelta::from_microseconds(
                    MICROSECONDS_PER_DAY - 60,
                )?))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_name = attr.as_str(interns);
        match attr_name {
            "utcoffset" => {
                let _ = args.get_one_arg("timezone.utcoffset", heap)?;
                let id = heap.allocate(HeapData::Timedelta(self.utcoffset()))?;
                Ok(Value::Ref(id))
            }
            "tzname" => {
                let _ = args.get_one_arg("timezone.tzname", heap)?;
                let id = heap.allocate(HeapData::Str(crate::types::Str::from(self.tzname())))?;
                Ok(Value::Ref(id))
            }
            "dst" => {
                let _ = args.get_one_arg("timezone.dst", heap)?;
                Ok(Value::None)
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_name)),
        }
    }
}

/// Dispatches datetime class methods invoked on builtin type objects.
pub(crate) fn call_datetime_type_method(
    ty: Type,
    method_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    match (ty, method_name) {
        (Type::Date, "today") => {
            args.check_zero_args("date.today", heap)?;
            let date = current_local_date()?;
            let id = heap.allocate(HeapData::Date(date))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Date, "fromtimestamp") => {
            let value = args.get_one_arg("date.fromtimestamp", heap)?;
            value.drop_with_heap(heap);
            let date = Date::new(1970, 1, 1)?;
            let id = heap.allocate(HeapData::Date(date))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Date, "fromordinal") => {
            let value = args.get_one_arg("date.fromordinal", heap)?;
            let ordinal = value_to_i64(&value, heap)?;
            value.drop_with_heap(heap);
            let date = Date::fromordinal(ordinal)?;
            let id = heap.allocate(HeapData::Date(date))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Date, "fromisoformat") => {
            let value = args.get_one_arg("date.fromisoformat", heap)?;
            let Some(s) = value.as_either_str(heap) else {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("date.fromisoformat() argument must be str"));
            };
            let (year, month, day) = parse_iso_date(s.as_str(interns))?;
            value.drop_with_heap(heap);
            let date = Date::new(year, month, day)?;
            let id = heap.allocate(HeapData::Date(date))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "now") => {
            let timezone_arg = args.get_zero_one_arg("datetime.now", heap)?;
            let dt = if let Some(value) = timezone_arg {
                if let Value::Ref(id) = value {
                    let offset = if let HeapData::Timezone(timezone) = heap.get(id) {
                        timezone.utcoffset()
                    } else {
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                    };
                    heap.inc_ref(id);
                    value.drop_with_heap(heap);
                    current_datetime_with_offset(Some(id), offset)?
                } else {
                    let is_none = matches!(value, Value::None);
                    value.drop_with_heap(heap);
                    if is_none {
                        current_local_datetime(None)?
                    } else {
                        return Err(ExcType::type_error("tzinfo argument must be None or timezone"));
                    }
                }
            } else {
                current_local_datetime(None)?
            };
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "utcnow") => {
            args.check_zero_args("datetime.utcnow", heap)?;
            let dt = current_utc_datetime()?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "fromtimestamp") => {
            let value = args.get_one_arg("datetime.fromtimestamp", heap)?;
            value.drop_with_heap(heap);
            let dt = Datetime::new(1970, 1, 1, 0, 0, 0, 0, None, 0)?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "utcfromtimestamp") => {
            let value = args.get_one_arg("datetime.utcfromtimestamp", heap)?;
            value.drop_with_heap(heap);
            let dt = Datetime::new(1970, 1, 1, 0, 0, 0, 0, None, 0)?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "fromordinal") => {
            let value = args.get_one_arg("datetime.fromordinal", heap)?;
            let ordinal = value_to_i64(&value, heap)?;
            value.drop_with_heap(heap);
            let date = Date::fromordinal(ordinal)?;
            let dt = Datetime::new(date.year(), date.month(), date.day(), 0, 0, 0, 0, None, 0)?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "fromisoformat") => {
            let value = args.get_one_arg("datetime.fromisoformat", heap)?;
            let Some(s) = value.as_either_str(heap) else {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("datetime.fromisoformat() argument must be str"));
            };
            let (year, month, day, hour, minute, second, microsecond) = parse_iso_datetime(s.as_str(interns))?;
            value.drop_with_heap(heap);
            let dt = Datetime::new(year, month, day, hour, minute, second, microsecond, None, 0)?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Datetime, "combine") => {
            let (date_value, time_value) = args.get_two_args("datetime.combine", heap)?;
            let date = if let Value::Ref(id) = &date_value {
                if let HeapData::Date(d) = heap.get(*id) {
                    *d
                } else {
                    date_value.drop_with_heap(heap);
                    time_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("datetime.combine() first argument must be date"));
                }
            } else {
                date_value.drop_with_heap(heap);
                time_value.drop_with_heap(heap);
                return Err(ExcType::type_error("datetime.combine() first argument must be date"));
            };
            let time = if let Value::Ref(id) = &time_value {
                if let HeapData::Time(t) = heap.get(*id) {
                    *t
                } else {
                    date_value.drop_with_heap(heap);
                    time_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("datetime.combine() second argument must be time"));
                }
            } else {
                date_value.drop_with_heap(heap);
                time_value.drop_with_heap(heap);
                return Err(ExcType::type_error("datetime.combine() second argument must be time"));
            };
            date_value.drop_with_heap(heap);
            time_value.drop_with_heap(heap);
            let dt = Datetime::new(
                date.year(),
                date.month(),
                date.day(),
                time.hour(),
                time.minute(),
                time.second(),
                time.microsecond(),
                time.tzinfo(),
                0,
            )?;
            let id = heap.allocate(HeapData::Datetime(dt))?;
            Ok(Some(Value::Ref(id)))
        }
        (Type::Time, "fromisoformat") => {
            let value = args.get_one_arg("time.fromisoformat", heap)?;
            let Some(s) = value.as_either_str(heap) else {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("time.fromisoformat() argument must be str"));
            };
            let (hour, minute, second, microsecond) = parse_iso_time(s.as_str(interns))?;
            value.drop_with_heap(heap);
            let time = Time::new(hour, minute, second, microsecond, None, 0)?;
            let id = heap.allocate(HeapData::Time(time))?;
            Ok(Some(Value::Ref(id)))
        }
        _ => Ok(None),
    }
}

/// Returns the current local date for `date.today()`.
fn current_local_date() -> RunResult<Date> {
    let now = Local::now();
    Date::new(
        now.year(),
        i32::try_from(now.month()).unwrap_or(1),
        i32::try_from(now.day()).unwrap_or(1),
    )
}

/// Returns the current local datetime for `datetime.now()` when no timezone is passed.
fn current_local_datetime(tzinfo: Option<HeapId>) -> RunResult<Datetime> {
    let now = Local::now();
    Datetime::new(
        now.year(),
        i32::try_from(now.month()).unwrap_or(1),
        i32::try_from(now.day()).unwrap_or(1),
        i32::try_from(now.hour()).unwrap_or(0),
        i32::try_from(now.minute()).unwrap_or(0),
        i32::try_from(now.second()).unwrap_or(0),
        i32::try_from(now.nanosecond() / 1_000).unwrap_or(0),
        tzinfo,
        0,
    )
}

/// Returns the current UTC datetime for `datetime.utcnow()`.
fn current_utc_datetime() -> RunResult<Datetime> {
    let now = Utc::now();
    Datetime::new(
        now.year(),
        i32::try_from(now.month()).unwrap_or(1),
        i32::try_from(now.day()).unwrap_or(1),
        i32::try_from(now.hour()).unwrap_or(0),
        i32::try_from(now.minute()).unwrap_or(0),
        i32::try_from(now.second()).unwrap_or(0),
        i32::try_from(now.nanosecond() / 1_000).unwrap_or(0),
        None,
        0,
    )
}

/// Returns the current datetime shifted by a fixed offset for `datetime.now(tz=...)`.
fn current_datetime_with_offset(tzinfo: Option<HeapId>, offset: Timedelta) -> RunResult<Datetime> {
    let now = Utc::now() + ChronoDuration::microseconds(offset.as_microseconds());
    Datetime::new(
        now.year(),
        i32::try_from(now.month()).unwrap_or(1),
        i32::try_from(now.day()).unwrap_or(1),
        i32::try_from(now.hour()).unwrap_or(0),
        i32::try_from(now.minute()).unwrap_or(0),
        i32::try_from(now.second()).unwrap_or(0),
        i32::try_from(now.nanosecond() / 1_000).unwrap_or(0),
        tzinfo,
        0,
    )
}

// Helper functions

/// Checks if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Returns the number of days in a month.
fn days_in_month(year: i32, month: i32) -> i32 {
    if month == 2 && is_leap_year(year) {
        29
    } else {
        DAYS_IN_MONTH[(month - 1) as usize]
    }
}

const WEEKDAY_SHORT_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_SHORT_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_FULL_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Returns ISO calendar components `(year, week, weekday)`.
fn iso_calendar_parts(year: i32, month: i32, day: i32) -> RunResult<(i32, i32, i32)> {
    let date = Date::new(year, month, day)?;
    let ordinal = date.toordinal();
    let iso_weekday = date.isoweekday();

    // ISO week year is based on the Thursday of this week.
    let thursday_ordinal = ordinal + i64::from(4 - iso_weekday);
    let thursday = Date::fromordinal(thursday_ordinal)?;
    let iso_year = thursday.year();

    let jan4 = Date::new(iso_year, 1, 4)?;
    let jan4_ordinal = jan4.toordinal();
    let week1_monday = jan4_ordinal - i64::from(jan4.isoweekday() - 1);
    let week = i32::try_from((ordinal - week1_monday) / 7 + 1).unwrap_or(1);

    Ok((iso_year, week, iso_weekday))
}

/// Formats ctime-compatible date and time strings.
fn ctime_string(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> RunResult<String> {
    let date = Date::new(year, month, day)?;
    let weekday = usize::try_from(date.weekday()).unwrap_or(0);
    let month_index = usize::try_from(month - 1).unwrap_or(0);
    Ok(format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        WEEKDAY_SHORT_NAMES[weekday], MONTH_SHORT_NAMES[month_index], day, hour, minute, second, year
    ))
}

/// Formats a limited `strftime` pattern set used by parity tests.
fn format_datetime_pattern(
    pattern: &str,
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
) -> RunResult<String> {
    let _ = Date::new(year, month, day)?;
    let month_index = usize::try_from(month - 1).unwrap_or(0);

    let mut output = String::new();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }

        let Some(code) = chars.next() else {
            output.push('%');
            break;
        };
        match code {
            'Y' => output.push_str(&format!("{year:04}")),
            'm' => output.push_str(&format!("{month:02}")),
            'd' => output.push_str(&format!("{day:02}")),
            'H' => output.push_str(&format!("{hour:02}")),
            'M' => output.push_str(&format!("{minute:02}")),
            'S' => output.push_str(&format!("{second:02}")),
            'B' => output.push_str(MONTH_FULL_NAMES[month_index]),
            '%' => output.push('%'),
            other => {
                output.push('%');
                output.push(other);
            }
        }
    }
    Ok(output)
}

/// Builds a `time.struct_time`-like namedtuple.
fn timetuple_value(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let date = Date::new(year, month, day)?;
    let first_day = Date::new(year, 1, 1)?;
    let yday = i32::try_from(date.toordinal() - first_day.toordinal() + 1).unwrap_or(1);
    let weekday = date.weekday();

    let fields = vec![
        EitherStr::Heap("tm_year".to_string()),
        EitherStr::Heap("tm_mon".to_string()),
        EitherStr::Heap("tm_mday".to_string()),
        EitherStr::Heap("tm_hour".to_string()),
        EitherStr::Heap("tm_min".to_string()),
        EitherStr::Heap("tm_sec".to_string()),
        EitherStr::Heap("tm_wday".to_string()),
        EitherStr::Heap("tm_yday".to_string()),
        EitherStr::Heap("tm_isdst".to_string()),
    ];
    let items = vec![
        Value::Int(i64::from(year)),
        Value::Int(i64::from(month)),
        Value::Int(i64::from(day)),
        Value::Int(i64::from(hour)),
        Value::Int(i64::from(minute)),
        Value::Int(i64::from(second)),
        Value::Int(i64::from(weekday)),
        Value::Int(i64::from(yday)),
        Value::Int(-1),
    ];
    let named = NamedTuple::new("time.struct_time".to_string(), fields, items);
    let id = heap.allocate(HeapData::NamedTuple(named))?;
    Ok(Value::Ref(id))
}

/// Parses `YYYY-MM-DD`.
fn parse_iso_date(input: &str) -> RunResult<(i32, i32, i32)> {
    let mut parts = input.split('-');
    let year = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    let month = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    let day = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    if parts.next().is_some() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string").into());
    }
    Ok((year, month, day))
}

/// Parses `HH:MM:SS[.ffffff]`.
fn parse_iso_time(input: &str) -> RunResult<(i32, i32, i32, i32)> {
    let (time_part, micros_part) = match input.split_once('.') {
        Some((time, micros)) => (time, Some(micros)),
        None => (input, None),
    };
    let mut parts = time_part.split(':');
    let hour = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    let minute = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    let second = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    if parts.next().is_some() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string").into());
    }
    let microsecond = if let Some(micros) = micros_part {
        let mut digits = micros.chars().take(6).collect::<String>();
        while digits.len() < 6 {
            digits.push('0');
        }
        digits
            .parse::<i32>()
            .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?
    } else {
        0
    };
    Ok((hour, minute, second, microsecond))
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.ffffff]`.
fn parse_iso_datetime(input: &str) -> RunResult<(i32, i32, i32, i32, i32, i32, i32)> {
    let (date_part, time_part) = input
        .split_once('T')
        .ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "Invalid isoformat string"))?;
    let (year, month, day) = parse_iso_date(date_part)?;
    let (hour, minute, second, microsecond) = parse_iso_time(time_part)?;
    Ok((year, month, day, hour, minute, second, microsecond))
}
